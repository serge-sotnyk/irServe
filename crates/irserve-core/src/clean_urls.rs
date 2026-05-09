//! Phase 4 (and, in slice 2, phase 8) of the dispatcher pipeline:
//! `cleanUrls` semantics.
//!
//! Mirrors `third_party/serve-handler/src/index.js`:
//! - `applicable(decodedPath, configEntry)` (lines 256-274): scope check.
//! - `shouldRedirect` cleanUrls branch (lines 121-143): the 301 emit
//!   pulls off `(\.html|\/index)$`, collapses any resulting `//`, and
//!   re-prepends `/` if the strip emptied the path.
//! - `slasher` (`./glob-slash.js`) ensures a leading `/` on the source
//!   pattern before `minimatch` runs.

use std::path::Path;

use globset::{GlobBuilder, GlobMatcher};

use crate::config::BoolOrGlobs;
use crate::normalize::collapse_slashes;
use crate::resolve::ResolveOutcome;

/// Precompiled view of `serve.json#cleanUrls`. Built once at server
/// start so the dispatcher can do per-request scope checks without
/// recompiling globs.
#[derive(Debug)]
pub struct CleanUrlsView {
    inner: Mode,
}

#[derive(Debug)]
enum Mode {
    /// `cleanUrls: false`.
    Off,
    /// `cleanUrls: true` (default when the field is absent).
    On,
    /// `cleanUrls: ["/docs/**", "!/secret/**", ...]`. The dispatcher
    /// emits cleanUrls behavior only when at least one pattern in
    /// the list matches the request path; negation patterns
    /// (`!`-prefixed) match paths that do NOT match the rest of the
    /// pattern (mirrors minimatch's `nonegate: false` default
    /// behavior, used by `serve-handler` via `sourceMatches` →
    /// `minimatch`).
    Scoped(Vec<ScopedPattern>),
}

/// One compiled cleanUrls glob with its negation flag. Mirrors
/// minimatch's per-pattern `negate` semantics (`!`-prefix in the
/// raw pattern, after `slasher` normalization).
#[derive(Debug)]
struct ScopedPattern {
    matcher: GlobMatcher,
    /// `true` when the user-supplied pattern began with `!` (after
    /// `slasher`). The match result is XOR'd with this flag, so a
    /// negation pattern matches paths that do NOT match the rest.
    negate: bool,
}

/// One cleanUrls pattern that failed to compile. The reference's
/// minimatch silently treats unparseable patterns as never-matching
/// (`serve-handler` keeps running and other patterns in the list
/// continue to work). We mirror this: invalid patterns are
/// collected here and surfaced to the bin layer for stderr
/// warnings; the server continues to start.
#[derive(Debug)]
pub struct InvalidGlob {
    pub pattern: String,
    pub error: globset::Error,
}

impl CleanUrlsView {
    /// Build the view from the parsed config field. Never fails:
    /// invalid glob patterns are collected into the returned
    /// `Vec<InvalidGlob>` for the bin to surface as warnings. This
    /// matches the reference's behavior at
    /// `serve-handler/src/index.js:38-67` (via `minimatch`), which
    /// silently treats unparseable patterns as never-matching and
    /// keeps the server running.
    pub fn from_config(cfg: &Option<BoolOrGlobs>) -> (Self, Vec<InvalidGlob>) {
        let mut invalid = Vec::new();
        let inner = match cfg {
            // SRV-ROUT-001/002 default: cleanUrls is on when the field
            // is absent, matching `serve-handler/src/index.js:273`
            // (`applicable` returns `true` for non-boolean,
            // non-array configEntry — including `undefined`).
            None => Mode::On,
            Some(BoolOrGlobs::Bool(true)) => Mode::On,
            Some(BoolOrGlobs::Bool(false)) => Mode::Off,
            Some(BoolOrGlobs::Globs(patterns)) => {
                let mut compiled = Vec::with_capacity(patterns.len());
                for pat in patterns {
                    match compile_scoped_pattern(pat) {
                        Ok(scoped) => compiled.push(scoped),
                        Err(error) => invalid.push(InvalidGlob {
                            pattern: pat.clone(),
                            error,
                        }),
                    }
                }
                Mode::Scoped(compiled)
            }
        };
        (Self { inner }, invalid)
    }

    /// `applicable(decodedPath, configEntry)` from
    /// `serve-handler/src/index.js:256-274`. `true` means the cleanUrls
    /// behavior (redirect or extensionless resolution) applies to the
    /// given path.
    ///
    /// Scoped checks normalize the path via the same slash-collapse
    /// the reference applies inside `sourceMatches`
    /// (`serve-handler/src/index.js:38-67`, `path.posix.resolve(requestPath)`).
    /// Without this, a raw-mode request like `GET //docs/guide.html`
    /// arrives at the dispatcher as `//docs/guide.html` and would miss
    /// an otherwise-matching `/docs/**` glob.
    pub fn applicable(&self, decoded_path: &str) -> bool {
        match &self.inner {
            Mode::Off => false,
            Mode::On => true,
            Mode::Scoped(patterns) => {
                let normalized = collapse_slashes(decoded_path);
                let path = normalized.as_ref();
                // Mirrors the reference's `applicable` iteration at
                // `index.js:261-268`: return `true` on the first
                // pattern whose `sourceMatches` returns truthy.
                // Negation patterns (`!`-prefixed) match paths that
                // do NOT match the rest of the pattern, so a sole
                // `!/secret/**` enables cleanUrls for everything
                // outside `/secret/**`.
                patterns
                    .iter()
                    .any(|p| p.matcher.is_match(path) ^ p.negate)
            }
        }
    }
}

/// Compile one user-supplied cleanUrls pattern into a
/// `ScopedPattern`. Mirrors `slasher` from
/// `serve-handler/src/glob-slash.js:8`: a leading `!` is preserved
/// so it can drive negation, the rest is `path.posix.normalize`'d
/// to ensure a leading `/`.
fn compile_scoped_pattern(raw: &str) -> Result<ScopedPattern, globset::Error> {
    let slashed = slasher(raw);
    let (negate, body) = match slashed.strip_prefix('!') {
        Some(rest) => (true, rest.to_string()),
        None => (false, slashed),
    };
    let glob = GlobBuilder::new(&body)
        .literal_separator(true)
        .build()?;
    Ok(ScopedPattern {
        matcher: glob.compile_matcher(),
        negate,
    })
}

/// Mirrors `third_party/serve-handler/src/glob-slash.js:8` exactly:
///
/// ```js
/// value.charAt(0) === '!' ? `!${normalize(value.substr(1))}` : normalize(value)
/// ```
///
/// where `normalize` ensures a leading `/`. The `!`-prefix is
/// preserved verbatim — `compile_scoped_pattern` then strips it to
/// build the matcher and toggles the `negate` flag. We don't
/// replicate the full `path.posix.normalize` (collapsing `..` etc.)
/// because cleanUrls patterns shouldn't carry such segments in
/// practice; if they do, that's a methodological signal for a
/// Q-NNN entry.
fn slasher(pattern: &str) -> String {
    if let Some(rest) = pattern.strip_prefix('!') {
        let normalized = if rest.starts_with('/') {
            rest.to_string()
        } else {
            format!("/{rest}")
        };
        format!("!{normalized}")
    } else if pattern.starts_with('/') {
        pattern.to_string()
    } else {
        format!("/{pattern}")
    }
}

/// Phase 4: compute the `cleanUrls` 301 target for the given decoded
/// path, or `None` if no redirect should fire.
///
/// Mirrors `shouldRedirect`'s cleanUrls branch
/// (`serve-handler/src/index.js:121-143`):
///
/// 1. If the path is not in scope (bool=false or out-of-scope glob) → None.
/// 2. If the path doesn't match `(\.html|\/index)$` → None.
/// 3. Otherwise: strip the matched suffix (single pass, `g` flag is a
///    no-op against an end-anchored pattern), collapse any resulting
///    `//`, and re-prepend `/` if the strip emptied the path
///    (`ensureSlashStart`).
///
/// Examples:
///   `/index.html`     → Some("/index")
///   `/foo.html`       → Some("/foo")
///   `/dir/index.html` → Some("/dir/index")
///   `/dir/index`      → Some("/dir")
///   `/index`          → Some("/")          // strip empties; ensureSlashStart fires
///   `//foo.html`      → Some("/foo")       // strip+collapse
///   `/foo.txt`        → None
///   `/about`          → None
pub fn compute_clean_urls_redirect(
    decoded_path: &str,
    view: &CleanUrlsView,
) -> Option<String> {
    if !view.applicable(decoded_path) {
        return None;
    }

    let stripped = strip_html_or_index_suffix(decoded_path)?;
    // Reuse the routing-normalization helper (mirrors the reference's
    // `decodedPath.replace(/\/+/g, '/')` at `index.js:137`). Returns
    // `Cow::Borrowed` on the no-`//` happy path (zero-allocation).
    let collapsed = collapse_slashes(stripped);
    Some(ensure_slash_start(collapsed.as_ref()))
}

/// Strip the suffix matched by JS `/(\.html|\/index)$/g` in a single
/// pass. `.html` is the first alternative, so a path like
/// `/index.html` strips to `/index` (NOT to ``).
fn strip_html_or_index_suffix(path: &str) -> Option<&str> {
    if let Some(rest) = path.strip_suffix(".html") {
        return Some(rest);
    }
    if let Some(rest) = path.strip_suffix("/index") {
        return Some(rest);
    }
    None
}

/// Phase 8: extensionless resolution. When `cleanUrls` is enabled and
/// applicable to the request path, attempt to serve `<P>/index.html`
/// first, then `<P>.html`, returning the first that exists. Returns
/// `None` when neither candidate resolves — the dispatcher then falls
/// through to phase 9 (the existing `resolve()` over the original path).
///
/// Mirrors `findRelated` + `getPossiblePaths(.html)`
/// (`serve-handler/src/index.js:276-307`):
///
/// - Candidate 1: `path.join(P, 'index.html')`.
/// - Candidate 2: trailing-slash variant of `P + '.html'`
///   (`P.replace(/\/$/g, '.html')` when `P` ends with `/`, else `P + '.html'`).
///   For our purposes both forms reduce to `<basename>.html` after
///   trimming a single trailing `/`.
/// - Filter: skip a candidate whose basename is exactly `.html`
///   (matches the `path.basename(item) !== extension` filter in
///   `index.js:279`). In our path space this only fires for the bare
///   root (`/`), where the second candidate would be `/.html`.
///
/// The candidate paths are canonicalized and checked against `root` to
/// reject path-traversal attempts (mirrors the existing `resolve()`
/// behavior at `crates/irserve-core/src/resolve.rs:40-47`).
pub async fn try_clean_urls_resolve(
    url_path: &str,
    root: &Path,
    view: &CleanUrlsView,
) -> Option<ResolveOutcome> {
    if !view.applicable(url_path) {
        return None;
    }

    // Normalize the url path so it can be joined under `root`. Strip
    // leading `/` and any trailing `/` so that `/about/` and `/about`
    // produce the same candidate set, matching `getPossiblePaths`.
    let trimmed = url_path.trim_start_matches('/');
    let trimmed = trimmed.trim_end_matches('/');

    // Candidate 1: <P>/index.html. Always attempted (even for `P=""`,
    // i.e. the root path), matching `getPossiblePaths`.
    let candidate_index = if trimmed.is_empty() {
        root.join("index.html")
    } else {
        root.join(trimmed).join("index.html")
    };
    if let Some(canonical) = stat_under_root(&candidate_index, root).await {
        return Some(ResolveOutcome::Index(canonical));
    }

    // Candidate 2: <P>.html. Skipped when `P` is empty (the second
    // entry would be `/.html`, which `getPossiblePaths` filters out).
    if trimmed.is_empty() {
        return None;
    }
    let candidate_flat = root.join(format!("{trimmed}.html"));
    if let Some(canonical) = stat_under_root(&candidate_flat, root).await {
        return Some(ResolveOutcome::File(canonical));
    }

    None
}

/// Stat a candidate file path; return the canonicalized path iff the
/// path exists, is a regular file, and resolves under `root` (anti
/// path-traversal). Mirrors the metadata + canonicalize + starts_with
/// guard in `resolve.rs`.
async fn stat_under_root(candidate: &Path, root: &Path) -> Option<std::path::PathBuf> {
    let meta = tokio::fs::metadata(candidate).await.ok()?;
    if !meta.is_file() {
        return None;
    }
    let canonical = tokio::fs::canonicalize(candidate).await.ok()?;
    if !canonical.starts_with(root) {
        return None;
    }
    Some(canonical)
}

/// Mirrors `ensureSlashStart` (`index.js:119`).
fn ensure_slash_start(target: &str) -> String {
    if target.starts_with('/') {
        target.to_string()
    } else {
        format!("/{target}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn view_on() -> CleanUrlsView {
        CleanUrlsView::from_config(&None).0
    }

    fn view_off() -> CleanUrlsView {
        CleanUrlsView::from_config(&Some(BoolOrGlobs::Bool(false))).0
    }

    fn view_scoped(patterns: &[&str]) -> CleanUrlsView {
        let cfg = Some(BoolOrGlobs::Globs(
            patterns.iter().map(|p| (*p).to_string()).collect(),
        ));
        let (view, invalid) = CleanUrlsView::from_config(&cfg);
        // Test helper: callers that pass valid patterns should never
        // see a non-empty `invalid` list. Tests that exercise
        // invalid patterns explicitly use `from_config` directly.
        assert!(
            invalid.is_empty(),
            "view_scoped helper saw invalid patterns; use from_config directly: {:?}",
            invalid.iter().map(|i| &i.pattern).collect::<Vec<_>>()
        );
        view
    }

    // ----- applicable / scope --------------------------------------

    #[test]
    fn applicable_true_when_on() {
        assert!(view_on().applicable("/anything"));
        assert!(view_on().applicable("/"));
    }

    #[test]
    fn applicable_false_when_off() {
        assert!(!view_off().applicable("/anything"));
    }

    #[test]
    fn applicable_glob_in_scope() {
        let v = view_scoped(&["/docs/**"]);
        assert!(v.applicable("/docs/guide.html"));
        assert!(v.applicable("/docs/sub/page.html"));
    }

    #[test]
    fn applicable_glob_out_of_scope() {
        let v = view_scoped(&["/docs/**"]);
        assert!(!v.applicable("/blog/post.html"));
        assert!(!v.applicable("/about"));
    }

    #[test]
    fn applicable_glob_normalizes_missing_leading_slash() {
        // slasher() prepends `/` so users can write `"docs/**"`.
        let v = view_scoped(&["docs/**"]);
        assert!(v.applicable("/docs/guide.html"));
    }

    #[test]
    fn applicable_single_star_does_not_cross_slash() {
        // Mirror minimatch: `*` matches a single segment, not nested
        // paths. `/docs/*` matches `/docs/page.html` but NOT
        // `/docs/sub/page.html`. Without `literal_separator(true)`
        // globset would match the nested path too — confirmed
        // divergence with the reference (`serve-handler/src/index.js:
        // 38-67` via `sourceMatches` + minimatch).
        let v = view_scoped(&["/docs/*"]);
        assert!(v.applicable("/docs/page.html"));
        assert!(v.applicable("/docs/guide"));
        assert!(!v.applicable("/docs/sub/page.html"));
        assert!(!v.applicable("/docs/sub/sub2/page.html"));
    }

    #[test]
    fn applicable_double_star_crosses_segments() {
        // Sanity: `**` SHOULD cross segments (matching minimatch).
        let v = view_scoped(&["/docs/**"]);
        assert!(v.applicable("/docs/page.html"));
        assert!(v.applicable("/docs/sub/page.html"));
        assert!(v.applicable("/docs/sub/sub2/page.html"));
    }

    #[test]
    fn applicable_normalizes_double_slash_path() {
        // Raw-mode requests can deliver paths with `//` to the
        // dispatcher. The reference's `applicable` runs the path
        // through `path.posix.resolve` inside `sourceMatches` before
        // minimatch, so `//docs/guide.html` collapses to
        // `/docs/guide.html` for scope-check purposes. The redirect
        // target computation downstream handles its own `//` collapse.
        let v = view_scoped(&["/docs/**"]);
        assert!(v.applicable("//docs/guide.html"));
        assert!(v.applicable("/docs//guide.html"));
        assert!(v.applicable("///docs////guide.html"));
    }

    #[test]
    fn from_config_default_is_on() {
        // serve-handler default: cleanUrls is enabled when the field
        // is absent.
        assert!(view_on().applicable("/index.html"));
    }

    #[test]
    fn from_config_invalid_glob_is_skipped_silently() {
        // Reference behavior (`serve-handler/src/index.js:38-67` via
        // `minimatch`): unparseable patterns silently never match,
        // server keeps running. We mirror this — surface invalid
        // patterns to the caller for warnings, but don't fail.
        let cfg = Some(BoolOrGlobs::Globs(vec!["[invalid".to_string()]));
        let (view, invalid) = CleanUrlsView::from_config(&cfg);
        assert_eq!(invalid.len(), 1);
        assert_eq!(invalid[0].pattern, "[invalid");
        // No usable patterns remain; applicable returns false for
        // any path (Mode::Scoped(empty)).
        assert!(!view.applicable("/anything"));
        assert!(!view.applicable("/index.html"));
    }

    #[test]
    fn from_config_mixed_valid_and_invalid_keeps_valid() {
        // One bad pattern doesn't poison the valid ones (matches the
        // reference's per-pattern iteration; a parse failure is
        // local to that pattern).
        let cfg = Some(BoolOrGlobs::Globs(vec![
            "/docs/**".to_string(),
            "[invalid".to_string(),
        ]));
        let (view, invalid) = CleanUrlsView::from_config(&cfg);
        assert_eq!(invalid.len(), 1);
        assert!(view.applicable("/docs/page.html"));
        assert!(!view.applicable("/blog/page.html"));
    }

    #[test]
    fn applicable_negation_excludes_path() {
        // `!/secret/**` is a sole-negation pattern: matches every
        // path that does NOT match `/secret/**`. Mirrors minimatch's
        // `nonegate: false` default + `slasher`'s `!`-preservation.
        let v = view_scoped(&["!/secret/**"]);
        assert!(v.applicable("/public/page.html"));
        assert!(v.applicable("/about"));
        assert!(v.applicable("/index.html"));
        assert!(!v.applicable("/secret/foo"));
        assert!(!v.applicable("/secret/sub/x.html"));
    }

    #[test]
    fn applicable_mixed_positive_and_negation() {
        // Reference iterates patterns and returns true on the first
        // that matches (`sourceMatches` truthy). With both `/docs/**`
        // and `!/secret/**`, every path either hits the positive
        // (in /docs) or the negation (anywhere outside /secret) —
        // only paths under /secret/ are excluded.
        let v = view_scoped(&["/docs/**", "!/secret/**"]);
        assert!(v.applicable("/docs/foo.html"));
        assert!(v.applicable("/docs/sub/page.html"));
        assert!(v.applicable("/about"));
        assert!(!v.applicable("/secret/x"));
        assert!(!v.applicable("/secret/sub/y.html"));
    }

    #[test]
    fn applicable_negation_without_leading_slash_normalizes() {
        // `!secret/**` (no leading `/`) — `slasher` produces
        // `!/secret/**`. Behaves identically to the explicit form.
        let v = view_scoped(&["!secret/**"]);
        assert!(v.applicable("/about"));
        assert!(!v.applicable("/secret/x"));
    }

    // ----- compute_clean_urls_redirect -----------------------------

    #[test]
    fn redirect_strips_html_suffix() {
        assert_eq!(
            compute_clean_urls_redirect("/foo.html", &view_on()).as_deref(),
            Some("/foo")
        );
        assert_eq!(
            compute_clean_urls_redirect("/dir/sub/page.html", &view_on()).as_deref(),
            Some("/dir/sub/page")
        );
    }

    #[test]
    fn redirect_index_html_strips_only_html_suffix() {
        // Reference uses single-pass replace (regex `/(\.html|\/index)$/g`
        // with end-anchor only matches once). Snapshot
        // `_smoke#index_html_redirect` pins the result to `/index`.
        assert_eq!(
            compute_clean_urls_redirect("/index.html", &view_on()).as_deref(),
            Some("/index")
        );
    }

    #[test]
    fn redirect_dir_index_html_strips_only_html_suffix() {
        assert_eq!(
            compute_clean_urls_redirect("/dir/index.html", &view_on()).as_deref(),
            Some("/dir/index")
        );
    }

    #[test]
    fn redirect_strips_index_suffix() {
        assert_eq!(
            compute_clean_urls_redirect("/dir/index", &view_on()).as_deref(),
            Some("/dir")
        );
    }

    #[test]
    fn redirect_bare_index_yields_root() {
        // `/index` → strip `/index` → "" → ensureSlashStart → `/`.
        assert_eq!(
            compute_clean_urls_redirect("/index", &view_on()).as_deref(),
            Some("/")
        );
    }

    #[test]
    fn redirect_collapses_resulting_double_slash() {
        // `//foo.html` → strip `.html` → `//foo` → collapse → `/foo`.
        assert_eq!(
            compute_clean_urls_redirect("//foo.html", &view_on()).as_deref(),
            Some("/foo")
        );
    }

    #[test]
    fn redirect_collapses_internal_double_slash() {
        // `/dir//page.html` → strip → `/dir//page` → collapse → `/dir/page`.
        assert_eq!(
            compute_clean_urls_redirect("/dir//page.html", &view_on()).as_deref(),
            Some("/dir/page")
        );
    }

    #[test]
    fn redirect_no_match_returns_none() {
        // No matching suffix → no redirect.
        assert_eq!(compute_clean_urls_redirect("/foo.txt", &view_on()), None);
        assert_eq!(compute_clean_urls_redirect("/about", &view_on()), None);
        assert_eq!(compute_clean_urls_redirect("/", &view_on()), None);
        assert_eq!(compute_clean_urls_redirect("/foo.htm", &view_on()), None);
    }

    #[test]
    fn redirect_trailing_slash_blocks_match() {
        // The reference regex is end-anchored; trailing `/` prevents
        // matching `.html` or `/index`.
        assert_eq!(
            compute_clean_urls_redirect("/foo.html/", &view_on()),
            None
        );
        assert_eq!(
            compute_clean_urls_redirect("/dir/index/", &view_on()),
            None
        );
    }

    #[test]
    fn redirect_off_short_circuits() {
        assert_eq!(
            compute_clean_urls_redirect("/index.html", &view_off()),
            None
        );
        assert_eq!(
            compute_clean_urls_redirect("/foo.html", &view_off()),
            None
        );
    }

    #[test]
    fn redirect_scope_in_glob() {
        let v = view_scoped(&["/docs/**"]);
        assert_eq!(
            compute_clean_urls_redirect("/docs/guide.html", &v).as_deref(),
            Some("/docs/guide")
        );
    }

    #[test]
    fn redirect_scope_out_of_glob() {
        let v = view_scoped(&["/docs/**"]);
        assert_eq!(compute_clean_urls_redirect("/blog/post.html", &v), None);
    }

    #[test]
    fn redirect_scope_single_star_does_not_match_nested() {
        // Companion to `applicable_single_star_does_not_cross_slash`:
        // a `/docs/*` scope should NOT trigger the cleanUrls 301 for
        // a nested `.html` path. Reference behavior: `GET
        // /docs/sub/page.html` with `cleanUrls: ["/docs/*"]` returns
        // 200 (file served direct).
        let v = view_scoped(&["/docs/*"]);
        assert_eq!(
            compute_clean_urls_redirect("/docs/sub/page.html", &v),
            None
        );
        // Single-segment match still produces the 301.
        assert_eq!(
            compute_clean_urls_redirect("/docs/guide.html", &v).as_deref(),
            Some("/docs/guide")
        );
    }

    #[test]
    fn redirect_scope_double_slash_path_normalizes() {
        // Raw-mode `GET //docs/guide.html` with `cleanUrls:
        // ["/docs/**"]`: the reference's `applicable` normalizes the
        // path before minimatch, so the cleanUrls 301 fires. Our
        // implementation must too. Result Location is computed by the
        // existing strip-then-collapse path (`//docs/guide` →
        // `/docs/guide`).
        let v = view_scoped(&["/docs/**"]);
        assert_eq!(
            compute_clean_urls_redirect("//docs/guide.html", &v).as_deref(),
            Some("/docs/guide")
        );
    }

    // ----- try_clean_urls_resolve ---------------------------------

    use std::fs;
    use tempfile::tempdir;

    fn write_file(dir: &Path, rel: &str, content: &str) {
        let p = dir.join(rel);
        if let Some(parent) = p.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        fs::write(p, content).unwrap();
    }

    #[tokio::test]
    async fn resolve_index_first_when_dir_with_index_exists() {
        let dir = tempdir().unwrap();
        let root = fs::canonicalize(dir.path()).unwrap();
        write_file(&root, "about.html", "flat");
        write_file(&root, "about/index.html", "index");
        let outcome = try_clean_urls_resolve("/about", &root, &view_on())
            .await
            .expect("must resolve");
        match outcome {
            ResolveOutcome::Index(p) => {
                assert!(p.ends_with("about/index.html") || p.ends_with("about\\index.html"));
            }
            other => panic!("expected Index, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn resolve_falls_back_to_html_when_no_dir_index() {
        let dir = tempdir().unwrap();
        let root = fs::canonicalize(dir.path()).unwrap();
        write_file(&root, "about.html", "flat");
        let outcome = try_clean_urls_resolve("/about", &root, &view_on())
            .await
            .expect("must resolve via .html fallback");
        match outcome {
            ResolveOutcome::File(p) => {
                assert!(p.ends_with("about.html"));
            }
            other => panic!("expected File, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn resolve_trailing_slash_normalizes_to_same_candidates() {
        // `/about/` and `/about` should produce the same candidate set.
        let dir = tempdir().unwrap();
        let root = fs::canonicalize(dir.path()).unwrap();
        write_file(&root, "about.html", "flat");
        let outcome = try_clean_urls_resolve("/about/", &root, &view_on())
            .await
            .expect("must resolve via .html fallback");
        match outcome {
            ResolveOutcome::File(p) => assert!(p.ends_with("about.html")),
            other => panic!("expected File, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn resolve_root_only_tries_index_html() {
        // For `P = ""` (or `/`), `getPossiblePaths` filters the second
        // candidate (basename `.html`). Only `index.html` is tried.
        let dir = tempdir().unwrap();
        let root = fs::canonicalize(dir.path()).unwrap();
        write_file(&root, "index.html", "root");
        let outcome = try_clean_urls_resolve("/", &root, &view_on())
            .await
            .expect("must resolve root");
        match outcome {
            ResolveOutcome::Index(p) => assert!(p.ends_with("index.html")),
            other => panic!("expected Index, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn resolve_both_miss_returns_none() {
        let dir = tempdir().unwrap();
        let root = fs::canonicalize(dir.path()).unwrap();
        // No index.html anywhere, no `about.html` either.
        let outcome = try_clean_urls_resolve("/about", &root, &view_on()).await;
        assert!(outcome.is_none());
    }

    #[tokio::test]
    async fn resolve_off_short_circuits() {
        let dir = tempdir().unwrap();
        let root = fs::canonicalize(dir.path()).unwrap();
        write_file(&root, "about.html", "flat");
        let outcome = try_clean_urls_resolve("/about", &root, &view_off()).await;
        assert!(outcome.is_none());
    }

    #[tokio::test]
    async fn resolve_out_of_scope_returns_none() {
        let dir = tempdir().unwrap();
        let root = fs::canonicalize(dir.path()).unwrap();
        write_file(&root, "blog/post.html", "post");
        let v = view_scoped(&["/docs/**"]);
        let outcome = try_clean_urls_resolve("/blog/post", &root, &v).await;
        assert!(outcome.is_none());
    }

    #[tokio::test]
    async fn resolve_in_scope_matches_glob() {
        let dir = tempdir().unwrap();
        let root = fs::canonicalize(dir.path()).unwrap();
        write_file(&root, "docs/guide.html", "guide");
        let v = view_scoped(&["/docs/**"]);
        let outcome = try_clean_urls_resolve("/docs/guide", &root, &v)
            .await
            .expect("must resolve in-scope path");
        match outcome {
            ResolveOutcome::File(p) => assert!(p.ends_with("guide.html")),
            other => panic!("expected File, got {other:?}"),
        }
    }
}
