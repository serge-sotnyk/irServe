//! Phase 11 of the dispatcher pipeline: directory listing rendering.
//!
//! Mirrors `third_party/serve-handler/src/index.js:325-465`
//! (`renderDirectory`) and `applicable` at lines 256-274:
//!
//! * `directoryListing: bool | string[]` scope — `Off` / `On` /
//!   `Scoped` patterns. The default (when the field is absent) is
//!   `On` per reference's `applicable` returning truthy for non-bool /
//!   non-array `configEntry` (`index.js:273`).
//! * The HTML branch emits a basic listing (breadcrumb header,
//!   dirs-first sort, parent `..` link when not at root, `<ul>` of
//!   anchors). Body bytes are out of scope per D-003; only status,
//!   `Content-Type`, and the structural existence of the entries
//!   matter contractually.
//! * Reference's listing dispatch (`index.js:644-672`) emits the
//!   response with a single `setHeader('Content-Type', ...)` +
//!   `response.end(directory)` and returns BEFORE the success-site
//!   `getHeaders` call (`index.js:746`). Custom response headers
//!   therefore do NOT layer onto listing responses, mirroring how
//!   3xx redirects also bypass `getHeaders`.
//!
//! Slice 2 implements only the HTML branch (`Off`/`On`/scope plus
//! markup). JSON content negotiation, `unlisted` filtering, and
//! `renderSingle` short-circuit land in slices 3-5.

use std::path::Path;

use axum::body::Body;
use axum::http::header::{HeaderValue, CONTENT_TYPE};
use axum::http::{Response, StatusCode};
use globset::{GlobBuilder, GlobMatcher};

use crate::config::BoolOrGlobs;
use crate::normalize::collapse_slashes;

/// Precompiled view of `serve.json#directoryListing`. Built once at
/// server start so the dispatcher can do per-request scope checks
/// without recompiling globs. Mirrors `CleanUrlsView` in
/// `crates/irserve-core/src/clean_urls.rs:23-101`.
#[derive(Debug)]
pub struct DirectoryListingView {
    inner: Mode,
}

#[derive(Debug)]
enum Mode {
    Off,
    On,
    Scoped(Vec<ScopedPattern>),
}

#[derive(Debug)]
struct ScopedPattern {
    matcher: GlobMatcher,
    negate: bool,
}

/// One `directoryListing` glob pattern that failed to compile.
/// Mirrors `CleanUrlsView::InvalidGlob`.
#[derive(Debug)]
pub struct InvalidGlob {
    pub pattern: String,
    pub error: globset::Error,
}

impl DirectoryListingView {
    /// Build the view from the parsed config field. Never fails:
    /// invalid glob patterns are collected for the bin to surface
    /// as warnings. The reference treats unparseable patterns
    /// silently (`serve-handler/src/index.js:38-67` via `minimatch`).
    pub fn from_config(cfg: &Option<BoolOrGlobs>) -> (Self, Vec<InvalidGlob>) {
        let mut invalid = Vec::new();
        let inner = match cfg {
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

    /// `applicable(decodedPath, directoryListing)` from
    /// `serve-handler/src/index.js:256-274` and `index.js:336`. Returns
    /// `true` when the listing branch should fire for the given path.
    pub fn applicable(&self, decoded_path: &str) -> bool {
        match &self.inner {
            Mode::Off => false,
            Mode::On => true,
            Mode::Scoped(patterns) => {
                let normalized = collapse_slashes(decoded_path);
                let path = normalized.as_ref();
                patterns.iter().any(|p| p.matcher.is_match(path) ^ p.negate)
            }
        }
    }
}

/// Mirrors `compile_scoped_pattern` from `clean_urls.rs`. Kept as a
/// private duplicate to avoid coupling: `directoryListing` and
/// `cleanUrls` share the same `BoolOrGlobs` shape but their
/// downstream evolutions are independent (a future negation-default
/// or extglob change in one capability should not silently bleed
/// into the other).
fn compile_scoped_pattern(raw: &str) -> Result<ScopedPattern, globset::Error> {
    let slashed = slasher(raw);
    let (negate, body) = match slashed.strip_prefix('!') {
        Some(rest) => (true, rest.to_string()),
        None => (false, slashed),
    };
    let glob = GlobBuilder::new(&body).literal_separator(true).build()?;
    Ok(ScopedPattern {
        matcher: glob.compile_matcher(),
        negate,
    })
}

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

/// Render an HTML directory listing for `dir` (canonicalized
/// absolute path under `root`, already containment-checked by
/// `resolve()`). `decoded_url` is the request's URL form (e.g.
/// `/foo` or `/foo/`); it drives the `<title>` / `<h1>` text and
/// the per-entry hrefs.
pub async fn render_html(
    dir: &Path,
    decoded_url: &str,
    root: &Path,
) -> Result<Response<Body>, std::io::Error> {
    let entries = read_sorted_entries(dir).await?;
    let body = build_html(&entries, decoded_url, dir, root);
    Ok(Response::builder()
        .status(StatusCode::OK)
        .header(
            CONTENT_TYPE,
            HeaderValue::from_static("text/html; charset=utf-8"),
        )
        .body(Body::from(body))
        .expect("listing response should always build"))
}

#[derive(Debug)]
struct Entry {
    name: String,
    is_dir: bool,
}

async fn read_sorted_entries(dir: &Path) -> Result<Vec<Entry>, std::io::Error> {
    let mut rd = tokio::fs::read_dir(dir).await?;
    let mut out = Vec::new();
    while let Some(e) = rd.next_entry().await? {
        let name = e.file_name().to_string_lossy().into_owned();
        let ft = e.file_type().await?;
        out.push(Entry {
            name,
            is_dir: ft.is_dir(),
        });
    }
    // Dirs first, then alphabetic within each group. Mirrors the
    // reference sort at `serve-handler/src/index.js:402-413`.
    out.sort_by(|a, b| match (a.is_dir, b.is_dir) {
        (true, false) => std::cmp::Ordering::Less,
        (false, true) => std::cmp::Ordering::Greater,
        _ => a.name.cmp(&b.name),
    });
    Ok(out)
}

fn build_html(entries: &[Entry], decoded_url: &str, dir: &Path, root: &Path) -> String {
    // The href prefix is `decoded_url` extended with a trailing `/`
    // if absent. Each entry's href is `<prefix><name>` (plus a
    // trailing `/` for sub-directories). Using an absolute-from-root
    // path lets browsers resolve links correctly even when the
    // request URL itself does not end with `/` (which is common
    // because phase 5 only redirects when `trailingSlash` is set).
    let prefix = if decoded_url.ends_with('/') {
        decoded_url.to_string()
    } else {
        format!("{decoded_url}/")
    };
    let title = html_escape_text(decoded_url);

    let mut html = String::with_capacity(256);
    html.push_str("<!doctype html>\n<html><head><meta charset=\"utf-8\"><title>Index of ");
    html.push_str(&title);
    html.push_str("</title></head><body><h1>Index of ");
    html.push_str(&title);
    html.push_str("</h1>\n<ul>\n");
    if dir != root {
        html.push_str("<li><a href=\"../\">..</a></li>\n");
    }
    for e in entries {
        let suffix = if e.is_dir { "/" } else { "" };
        let name_text = html_escape_text(&e.name);
        let name_href = encode_href_segment(&e.name);
        html.push_str("<li><a href=\"");
        html.push_str(&prefix);
        html.push_str(&name_href);
        html.push_str(suffix);
        html.push_str("\">");
        html.push_str(&name_text);
        html.push_str(suffix);
        html.push_str("</a></li>\n");
    }
    html.push_str("</ul></body></html>\n");
    html
}

fn html_escape_text(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for ch in s.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            _ => out.push(ch),
        }
    }
    out
}

/// Minimal href encoding for a single path segment. Body bytes of
/// listings are not contractual (D-003), so this only handles the
/// characters that would otherwise break the URL when a browser
/// resolves the relative link: spaces, `?`, `#`, plus the HTML-attr
/// metacharacters.
fn encode_href_segment(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for ch in s.chars() {
        match ch {
            ' ' => out.push_str("%20"),
            '#' => out.push_str("%23"),
            '?' => out.push_str("%3F"),
            '"' => out.push_str("&quot;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '&' => out.push_str("&amp;"),
            _ => out.push(ch),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    fn view_on() -> DirectoryListingView {
        DirectoryListingView::from_config(&None).0
    }

    fn view_off() -> DirectoryListingView {
        DirectoryListingView::from_config(&Some(BoolOrGlobs::Bool(false))).0
    }

    #[test]
    fn applicable_default_on() {
        assert!(view_on().applicable("/anything"));
        assert!(view_on().applicable("/"));
    }

    #[test]
    fn applicable_off() {
        assert!(!view_off().applicable("/anything"));
    }

    #[test]
    fn applicable_scoped_in_glob() {
        let cfg = Some(BoolOrGlobs::Globs(vec!["/docs/**".to_string()]));
        let (v, invalid) = DirectoryListingView::from_config(&cfg);
        assert!(invalid.is_empty());
        assert!(v.applicable("/docs/sub"));
        assert!(!v.applicable("/blog"));
    }

    #[test]
    fn applicable_scoped_negation() {
        let cfg = Some(BoolOrGlobs::Globs(vec!["!/secret/**".to_string()]));
        let (v, _) = DirectoryListingView::from_config(&cfg);
        assert!(v.applicable("/about"));
        assert!(!v.applicable("/secret/x"));
    }

    #[test]
    fn from_config_invalid_glob_skipped() {
        let cfg = Some(BoolOrGlobs::Globs(vec!["[bad".to_string()]));
        let (v, invalid) = DirectoryListingView::from_config(&cfg);
        assert_eq!(invalid.len(), 1);
        assert!(!v.applicable("/anything"));
    }

    #[tokio::test]
    async fn render_html_lists_entries_dirs_first() {
        let dir = tempdir().unwrap();
        let root = fs::canonicalize(dir.path()).unwrap();
        fs::write(root.join("a.txt"), "a").unwrap();
        fs::write(root.join("b.txt"), "b").unwrap();
        fs::create_dir_all(root.join("zfolder")).unwrap();
        let resp = render_html(&root, "/", &root).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        assert_eq!(
            resp.headers()
                .get(CONTENT_TYPE)
                .and_then(|v| v.to_str().ok()),
            Some("text/html; charset=utf-8"),
        );
        let body_bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let body = std::str::from_utf8(&body_bytes).unwrap();
        // Dirs first, then alpha. zfolder must precede a.txt.
        let pz = body.find("zfolder/").unwrap();
        let pa = body.find("a.txt").unwrap();
        assert!(pz < pa, "expected zfolder before a.txt:\n{body}");
        // No parent link at root.
        assert!(!body.contains("href=\"../\""));
    }

    #[tokio::test]
    async fn render_html_includes_parent_link_in_subdir() {
        let dir = tempdir().unwrap();
        let root = fs::canonicalize(dir.path()).unwrap();
        fs::create_dir_all(root.join("sub")).unwrap();
        fs::write(root.join("sub/a.txt"), "x").unwrap();
        let sub = fs::canonicalize(root.join("sub")).unwrap();
        let resp = render_html(&sub, "/sub/", &root).await.unwrap();
        let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let body = std::str::from_utf8(&body).unwrap();
        assert!(body.contains("href=\"../\""), "missing parent link:\n{body}");
        // Entries' hrefs are prefixed with the request URL.
        assert!(
            body.contains("href=\"/sub/a.txt\""),
            "missing prefixed entry href:\n{body}"
        );
    }

    #[tokio::test]
    async fn render_html_prefix_when_url_lacks_trailing_slash() {
        let dir = tempdir().unwrap();
        let root = fs::canonicalize(dir.path()).unwrap();
        fs::create_dir_all(root.join("sub")).unwrap();
        fs::write(root.join("sub/a.txt"), "x").unwrap();
        let sub = fs::canonicalize(root.join("sub")).unwrap();
        // URL without trailing slash — phase 5 only emits a redirect
        // when `trailingSlash` is set, so requests like `GET /sub`
        // can land at the listing renderer with a trailing-slash-less
        // URL. Hrefs must still resolve correctly.
        let resp = render_html(&sub, "/sub", &root).await.unwrap();
        let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let body = std::str::from_utf8(&body).unwrap();
        assert!(
            body.contains("href=\"/sub/a.txt\""),
            "missing prefixed entry href:\n{body}"
        );
    }

    #[test]
    fn html_escape_text_handles_metacharacters() {
        assert_eq!(html_escape_text("a&b<c>d\"e"), "a&amp;b&lt;c&gt;d&quot;e");
    }

    #[test]
    fn encode_href_segment_handles_space_hash_query() {
        assert_eq!(encode_href_segment("a b#c?d"), "a%20b%23c%3Fd");
    }
}
