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

use globset::{Glob, GlobSet, GlobSetBuilder};

use crate::config::BoolOrGlobs;

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
    /// `cleanUrls: ["/docs/**", ...]`. The dispatcher emits cleanUrls
    /// behavior only for paths that match at least one pattern.
    Scoped(GlobSet),
}

impl CleanUrlsView {
    /// Build the view from the parsed config field. Returns the
    /// underlying `globset::Error` if any pattern fails to compile —
    /// the bin surfaces this at startup, not per request.
    pub fn from_config(cfg: &Option<BoolOrGlobs>) -> Result<Self, globset::Error> {
        let inner = match cfg {
            // SRV-ROUT-001/002 default: cleanUrls is on when the field
            // is absent, matching `serve-handler/src/index.js:273`
            // (`applicable` returns `true` for non-boolean,
            // non-array configEntry — including `undefined`).
            None => Mode::On,
            Some(BoolOrGlobs::Bool(true)) => Mode::On,
            Some(BoolOrGlobs::Bool(false)) => Mode::Off,
            Some(BoolOrGlobs::Globs(patterns)) => {
                let mut builder = GlobSetBuilder::new();
                for pat in patterns {
                    builder.add(Glob::new(&slasher(pat))?);
                }
                Mode::Scoped(builder.build()?)
            }
        };
        Ok(Self { inner })
    }

    /// `applicable(decodedPath, configEntry)` from
    /// `serve-handler/src/index.js:256-274`. `true` means the cleanUrls
    /// behavior (redirect or extensionless resolution) applies to the
    /// given path.
    pub fn applicable(&self, decoded_path: &str) -> bool {
        match &self.inner {
            Mode::Off => false,
            Mode::On => true,
            Mode::Scoped(set) => set.is_match(decoded_path),
        }
    }
}

/// Mirrors `third_party/serve-handler/src/glob-slash.js`: prepend `/`
/// when the pattern doesn't already start with one. We don't replicate
/// the full `path.posix.normalize` (collapsing `..` etc.) because
/// cleanUrls patterns shouldn't carry such segments in practice; if
/// they do, that's a methodological signal for a Q-NNN entry.
fn slasher(pattern: &str) -> String {
    if pattern.starts_with('/') {
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
    let target = if stripped.contains("//") {
        collapse_consecutive_slashes(&stripped)
    } else {
        stripped.to_string()
    };
    Some(ensure_slash_start(&target))
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

/// Mirrors `decodedPath.replace(/\/+/g, '/')` from `index.js:137`.
fn collapse_consecutive_slashes(path: &str) -> String {
    let mut out = String::with_capacity(path.len());
    let mut prev_slash = false;
    for ch in path.chars() {
        if ch == '/' {
            if !prev_slash {
                out.push('/');
            }
            prev_slash = true;
        } else {
            out.push(ch);
            prev_slash = false;
        }
    }
    out
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
        CleanUrlsView::from_config(&None).unwrap()
    }

    fn view_off() -> CleanUrlsView {
        CleanUrlsView::from_config(&Some(BoolOrGlobs::Bool(false))).unwrap()
    }

    fn view_scoped(patterns: &[&str]) -> CleanUrlsView {
        let cfg = Some(BoolOrGlobs::Globs(
            patterns.iter().map(|p| (*p).to_string()).collect(),
        ));
        CleanUrlsView::from_config(&cfg).unwrap()
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
    fn from_config_default_is_on() {
        // serve-handler default: cleanUrls is enabled when the field
        // is absent.
        assert!(view_on().applicable("/index.html"));
    }

    #[test]
    fn from_config_invalid_glob_is_error() {
        let cfg = Some(BoolOrGlobs::Globs(vec!["[invalid".to_string()]));
        assert!(CleanUrlsView::from_config(&cfg).is_err());
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
}
