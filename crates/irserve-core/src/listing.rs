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
use axum::http::header::{HeaderValue, ACCEPT, CONTENT_TYPE};
use axum::http::{HeaderMap, Response, StatusCode};
use globset::{GlobBuilder, GlobMatcher};
use serde::Serialize;

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

/// Render a directory listing for `dir`, dispatching between HTML
/// and JSON based on the request's `Accept` header. The reference
/// uses a substring check at `serve-handler/src/index.js:556-558`
/// (`request.headers.accept.includes('application/json')`); we
/// mirror it case-insensitively.
///
/// `dir` is the canonicalized absolute path under `root`, already
/// containment-checked by `resolve()`. `decoded_url` is the
/// request's URL form (e.g. `/foo` or `/foo/`); it drives the HTML
/// `<title>` / `<h1>` text and the per-entry hrefs. `root` is the
/// served root, used for D-007 sanitization of the JSON
/// `directory` and `paths` fields.
pub async fn render(
    dir: &Path,
    decoded_url: &str,
    root: &Path,
    request_headers: &HeaderMap,
) -> Result<Response<Body>, std::io::Error> {
    if accepts_json(request_headers) {
        render_json(dir, root).await
    } else {
        render_html_inner(dir, decoded_url, root).await
    }
}

/// `request.headers.accept.includes('application/json')`
/// (`serve-handler/src/index.js:556-558`), case-insensitive.
fn accepts_json(headers: &HeaderMap) -> bool {
    headers
        .get(ACCEPT)
        .and_then(|v| v.to_str().ok())
        .map(|v| v.to_ascii_lowercase().contains("application/json"))
        .unwrap_or(false)
}

async fn render_html_inner(
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

async fn render_json(dir: &Path, root: &Path) -> Result<Response<Body>, std::io::Error> {
    let entries = read_sorted_entries(dir).await?;
    let body = build_json(&entries, dir, root);
    Ok(Response::builder()
        .status(StatusCode::OK)
        .header(
            CONTENT_TYPE,
            HeaderValue::from_static("application/json; charset=utf-8"),
        )
        .body(Body::from(body))
        .expect("listing response should always build"))
}

#[derive(Debug)]
struct Entry {
    name: String,
    is_dir: bool,
    /// `None` for directories or files whose `metadata()` call
    /// failed (e.g. broken symlinks). The JSON output omits the
    /// `size` field in that case via `skip_serializing_if`.
    size: Option<u64>,
}

async fn read_sorted_entries(dir: &Path) -> Result<Vec<Entry>, std::io::Error> {
    let mut rd = tokio::fs::read_dir(dir).await?;
    let mut out = Vec::new();
    while let Some(e) = rd.next_entry().await? {
        let name = e.file_name().to_string_lossy().into_owned();
        let ft = e.file_type().await?;
        let is_dir = ft.is_dir();
        let size = if is_dir {
            None
        } else {
            e.metadata().await.ok().map(|m| m.len())
        };
        out.push(Entry { name, is_dir, size });
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

/// D-007 sanitization: emit the JSON listing with `directory`,
/// `paths`, and `relative` rendered relative to the served root,
/// never as host-absolute filesystem paths. Reference's shape leaks
/// `path.basename(current)` plus the absolute `dir` field
/// (`serve-handler/src/index.js:455-462`), tracked under
/// `Q-008` and resolved by `D-007`.
///
/// Chosen relative-path form (kickoff interview, plan key
/// decision #2):
///   * root directory: `"."`
///   * nested:        `"sub"`, `"sub/deep"` — POSIX separators,
///                    no leading `/`, no trailing `/`.
///
/// `paths` follows the same convention. `relative` per-entry is a
/// URL form with a leading `/` and (for folders) a trailing `/`,
/// because the field doubles as an `href` for JSON consumers.
///
/// File-entry shape: `{type, name, base, ext?, relative, size?}`.
/// Folder-entry shape: `{type, name, base, relative}`. `size` is
/// emitted as raw bytes (number) — see the plan's pre-stage
/// out-of-scope note about `bytes`-package formatted strings being
/// non-contractual.
#[derive(Serialize)]
struct ListingJson<'a> {
    files: Vec<EntryJson<'a>>,
    directory: String,
    paths: Vec<PathSegment>,
}

#[derive(Serialize)]
struct EntryJson<'a> {
    #[serde(rename = "type")]
    kind: &'static str,
    name: &'a str,
    base: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    ext: Option<&'a str>,
    relative: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    size: Option<u64>,
}

#[derive(Serialize)]
struct PathSegment {
    name: String,
    url: String,
}

fn build_json(entries: &[Entry], dir: &Path, root: &Path) -> Vec<u8> {
    let directory = sanitized_relative_dir(dir, root);
    let paths = breadcrumb_segments(&directory);
    let files: Vec<EntryJson> = entries
        .iter()
        .map(|e| {
            let (kind, base, ext, size) = if e.is_dir {
                ("folder", format!("{}/", e.name), None, None)
            } else {
                let p = std::path::Path::new(&e.name);
                let ext = p.extension().and_then(|s| s.to_str());
                ("file", e.name.clone(), ext, e.size)
            };
            EntryJson {
                kind,
                name: stem_for(&e.name, e.is_dir),
                base,
                ext,
                relative: entry_relative_url(&directory, &e.name, e.is_dir),
                size,
            }
        })
        .collect();
    let listing = ListingJson {
        files,
        directory,
        paths,
    };
    serde_json::to_vec(&listing).expect("listing json serialization should never fail")
}

/// `dir.strip_prefix(root)` rendered with POSIX separators. Returns
/// `"."` when `dir == root`.
fn sanitized_relative_dir(dir: &Path, root: &Path) -> String {
    match dir.strip_prefix(root) {
        Ok(rel) if rel.as_os_str().is_empty() => ".".to_string(),
        Ok(rel) => rel.to_string_lossy().replace('\\', "/"),
        // Containment check upstream (`resolve()` returns
        // `EscapedRoot` for paths outside `root`) means this branch
        // is unreachable in practice. Default to `.` so we never
        // emit a host-absolute path on the JSON wire.
        Err(_) => ".".to_string(),
    }
}

/// Breadcrumb segments accumulated from the relative directory.
/// Empty for root (`"."`), `[{"sub","sub"}, {"deep","sub/deep"}]`
/// for `"sub/deep"`. The `url` form mirrors the chosen `directory`
/// shape (no leading or trailing slash).
fn breadcrumb_segments(directory: &str) -> Vec<PathSegment> {
    if directory == "." || directory.is_empty() {
        return Vec::new();
    }
    let mut acc: Vec<&str> = Vec::new();
    let mut out = Vec::new();
    for seg in directory.split('/') {
        if seg.is_empty() {
            continue;
        }
        acc.push(seg);
        out.push(PathSegment {
            name: seg.to_string(),
            url: acc.join("/"),
        });
    }
    out
}

/// URL-form `relative` for a per-entry record. Leading `/`,
/// trailing `/` for folders. Root entries: `/<name>`. Nested:
/// `/<rel_dir>/<name>`.
fn entry_relative_url(rel_dir: &str, name: &str, is_dir: bool) -> String {
    let suffix = if is_dir { "/" } else { "" };
    if rel_dir == "." || rel_dir.is_empty() {
        format!("/{name}{suffix}")
    } else {
        format!("/{rel_dir}/{name}{suffix}")
    }
}

/// File stem for the JSON `name` field. Mirrors `path.parse(.).name`
/// from `serve-handler/src/index.js:344`, with the dotfile carve-out
/// (`.bashrc` → name=`.bashrc`, no ext) handled by `Path::file_stem`.
/// For folders, `name` is the directory's own name.
fn stem_for(name: &str, is_dir: bool) -> &str {
    if is_dir {
        return name;
    }
    let p = std::path::Path::new(name);
    p.file_stem().and_then(|s| s.to_str()).unwrap_or(name)
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

    fn empty_headers() -> HeaderMap {
        HeaderMap::new()
    }

    fn json_headers() -> HeaderMap {
        let mut h = HeaderMap::new();
        h.insert(ACCEPT, HeaderValue::from_static("application/json"));
        h
    }

    #[tokio::test]
    async fn render_html_lists_entries_dirs_first() {
        let dir = tempdir().unwrap();
        let root = fs::canonicalize(dir.path()).unwrap();
        fs::write(root.join("a.txt"), "a").unwrap();
        fs::write(root.join("b.txt"), "b").unwrap();
        fs::create_dir_all(root.join("zfolder")).unwrap();
        let resp = render(&root, "/", &root, &empty_headers()).await.unwrap();
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
        let resp = render(&sub, "/sub/", &root, &empty_headers()).await.unwrap();
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
        let resp = render(&sub, "/sub", &root, &empty_headers()).await.unwrap();
        let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let body = std::str::from_utf8(&body).unwrap();
        assert!(
            body.contains("href=\"/sub/a.txt\""),
            "missing prefixed entry href:\n{body}"
        );
    }

    #[tokio::test]
    async fn render_json_emits_application_json_envelope() {
        let dir = tempdir().unwrap();
        let root = fs::canonicalize(dir.path()).unwrap();
        fs::write(root.join("a.txt"), "ab").unwrap();
        fs::create_dir_all(root.join("sub")).unwrap();
        let resp = render(&root, "/", &root, &json_headers()).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        assert_eq!(
            resp.headers()
                .get(CONTENT_TYPE)
                .and_then(|v| v.to_str().ok()),
            Some("application/json; charset=utf-8"),
        );
        let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let v: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        // Root: directory=`.`, paths=[].
        assert_eq!(v["directory"], serde_json::json!("."));
        assert!(v["paths"].as_array().unwrap().is_empty());
        // Folder first (dirs-first sort), then file.
        let files = v["files"].as_array().unwrap();
        assert_eq!(files[0]["type"], serde_json::json!("folder"));
        assert_eq!(files[0]["name"], serde_json::json!("sub"));
        assert_eq!(files[0]["base"], serde_json::json!("sub/"));
        assert_eq!(files[0]["relative"], serde_json::json!("/sub/"));
        assert_eq!(files[1]["type"], serde_json::json!("file"));
        assert_eq!(files[1]["name"], serde_json::json!("a"));
        assert_eq!(files[1]["base"], serde_json::json!("a.txt"));
        assert_eq!(files[1]["ext"], serde_json::json!("txt"));
        assert_eq!(files[1]["relative"], serde_json::json!("/a.txt"));
        assert_eq!(files[1]["size"], serde_json::json!(2));
    }

    #[tokio::test]
    async fn render_json_subdir_directory_and_paths_use_chosen_shape() {
        let dir = tempdir().unwrap();
        let root = fs::canonicalize(dir.path()).unwrap();
        fs::create_dir_all(root.join("docs/api")).unwrap();
        fs::write(root.join("docs/api/index.json"), "{}").unwrap();
        let nested = fs::canonicalize(root.join("docs/api")).unwrap();
        let resp = render(&nested, "/docs/api/", &root, &json_headers())
            .await
            .unwrap();
        let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let v: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        // D-007 chosen shape: no leading/trailing slashes.
        assert_eq!(v["directory"], serde_json::json!("docs/api"));
        assert_eq!(
            v["paths"],
            serde_json::json!([
                {"name": "docs", "url": "docs"},
                {"name": "api", "url": "docs/api"},
            ])
        );
        // Per-entry `relative` is URL-form with leading `/`.
        let files = v["files"].as_array().unwrap();
        assert_eq!(files[0]["relative"], serde_json::json!("/docs/api/index.json"));
    }

    #[tokio::test]
    async fn render_json_dotfile_has_no_ext() {
        let dir = tempdir().unwrap();
        let root = fs::canonicalize(dir.path()).unwrap();
        fs::write(root.join(".bashrc"), "x").unwrap();
        let resp = render(&root, "/", &root, &json_headers()).await.unwrap();
        let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let v: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        let f = &v["files"][0];
        assert_eq!(f["name"], serde_json::json!(".bashrc"));
        assert_eq!(f["base"], serde_json::json!(".bashrc"));
        // `Path::extension` returns None for leading-dot stems with no
        // additional dots — matches `path.extname('.bashrc')` === ''.
        assert!(f.get("ext").is_none() || f["ext"].is_null());
    }

    #[test]
    fn accepts_json_basic() {
        let mut h = HeaderMap::new();
        h.insert(ACCEPT, HeaderValue::from_static("application/json"));
        assert!(accepts_json(&h));
    }

    #[test]
    fn accepts_json_substring_match_in_multivalue() {
        // Reference uses `.includes('application/json')` — substring,
        // case-insensitive in our impl.
        let mut h = HeaderMap::new();
        h.insert(
            ACCEPT,
            HeaderValue::from_static("text/html, Application/JSON;q=0.9"),
        );
        assert!(accepts_json(&h));
    }

    #[test]
    fn accepts_json_false_when_html_only() {
        let mut h = HeaderMap::new();
        h.insert(ACCEPT, HeaderValue::from_static("text/html"));
        assert!(!accepts_json(&h));
    }

    #[test]
    fn accepts_json_missing_header_is_false() {
        assert!(!accepts_json(&HeaderMap::new()));
    }

    #[test]
    fn breadcrumb_segments_root_is_empty() {
        assert!(breadcrumb_segments(".").is_empty());
        assert!(breadcrumb_segments("").is_empty());
    }

    #[test]
    fn breadcrumb_segments_nested() {
        let segs = breadcrumb_segments("a/b/c");
        let names: Vec<&str> = segs.iter().map(|s| s.name.as_str()).collect();
        let urls: Vec<&str> = segs.iter().map(|s| s.url.as_str()).collect();
        assert_eq!(names, vec!["a", "b", "c"]);
        assert_eq!(urls, vec!["a", "a/b", "a/b/c"]);
    }

    #[test]
    fn entry_relative_url_root_no_double_slash() {
        assert_eq!(entry_relative_url(".", "a.txt", false), "/a.txt");
        assert_eq!(entry_relative_url(".", "sub", true), "/sub/");
    }

    #[test]
    fn entry_relative_url_nested() {
        assert_eq!(entry_relative_url("docs", "a.txt", false), "/docs/a.txt");
        assert_eq!(entry_relative_url("docs/api", "v1", true), "/docs/api/v1/");
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
