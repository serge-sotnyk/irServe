use std::borrow::Cow;
use std::path::Path;

use axum::body::Body;
use axum::http::header::{HeaderValue, CONTENT_TYPE, LOCATION};
use axum::http::{Method, Request, Response, StatusCode};
use percent_encoding::{percent_decode_str, utf8_percent_encode, AsciiSet, CONTROLS};

use crate::clean_urls::{
    compute_clean_urls_redirect, try_clean_urls_resolve, CleanUrlsView,
};
use crate::config::ServeConfig;
use crate::custom_headers::{apply_custom_headers, HeaderRuleCompiled};
use crate::error::error_response;
use crate::mime::mime_for;
use crate::normalize::collapse_slashes;
use crate::redirects::{compute_configured_redirects, RedirectRuleCompiled};
use crate::resolve::{resolve, ResolveOutcome};
use crate::rewrites::{compute_configured_rewrites, RewriteRuleCompiled};
use crate::trailing_slash::compute_trailing_slash_redirect;

pub async fn dispatch(
    req: Request<Body>,
    root: &Path,
    serve_config: &ServeConfig,
    clean_urls_view: &CleanUrlsView,
    redirect_rules: &[RedirectRuleCompiled],
    rewrite_rules: &[RewriteRuleCompiled],
    header_rules: &[HeaderRuleCompiled],
) -> Response<Body> {
    // dispatch_inner returns (response, headers_path):
    //
    // - `Some(path)` — caller (this wrapper) applies `apply_custom_headers`
    //   with that path. Used for plain success / 405 paths whose
    //   reference equivalent flows through `getHeaders` at the success
    //   site (`serve-handler/src/index.js:746`) and/or where the
    //   matching path is the request path itself.
    // - `None` — headers either are not applicable (3xx redirects per
    //   `index.js:586-588` bypass `getHeaders`) or have already been
    //   applied per-branch inside `error_response` (Codex review round
    //   1 P1: JSON errors skip; custom `<status>.html` matches against
    //   the page path; fallback HTML matches against the request path).
    let (response, headers_path) = dispatch_inner(
        req,
        root,
        serve_config,
        clean_urls_view,
        redirect_rules,
        rewrite_rules,
        header_rules,
    )
    .await;
    match headers_path {
        Some(p) => apply_custom_headers(response, &p, header_rules),
        None => response,
    }
}

async fn dispatch_inner(
    req: Request<Body>,
    root: &Path,
    serve_config: &ServeConfig,
    clean_urls_view: &CleanUrlsView,
    redirect_rules: &[RedirectRuleCompiled],
    rewrite_rules: &[RewriteRuleCompiled],
    header_rules: &[HeaderRuleCompiled],
) -> (Response<Body>, Option<String>) {
    // Phase 1–2: method gate (existing). 405 carries the request's
    // raw URI path forward so that `apply_custom_headers` matches
    // against it (decode hasn't run yet).
    if req.method() != Method::GET && req.method() != Method::HEAD {
        let resp = Response::builder()
            .status(StatusCode::METHOD_NOT_ALLOWED)
            .body(Body::empty())
            .expect("405 response should always build");
        return (resp, Some(req.uri().path().to_string()));
    }

    // Decode the URI path once at the dispatcher entry. Mirrors the
    // reference's `decodedPath` invariant (`serve-handler/src/index.js:561`).
    // Subsequent phases operate on this decoded form so that encoded
    // forms like `%2F%2F` collapse identically to literal `//`.
    //
    // Phase-1 strict syntactic + UTF-8 gate (SRV-SEC-002): a malformed
    // `%xx` OR a byte sequence that decodes to invalid UTF-8 yields 400,
    // mirroring `decodeURIComponent`'s URIError branch at
    // `index.js:561-567`. Short-circuit BEFORE any phase 2+ runs. The
    // 400 response goes through `error_response` which applies custom
    // headers per-branch internally; the `None` headers_path tells the
    // wrapper not to apply twice.
    let raw_path = req.uri().path().to_string();
    let decoded_path = match try_percent_decode(&raw_path) {
        Ok(p) => p,
        Err(_) => {
            // Reference's URIError catch passes `'/'` as absolutePath
            // to sendError (`index.js:563`), so the fallback HTML
            // `getHeaders` call runs against a degenerate relative
            // path that no realistic user rule intentionally targets.
            // We mirror that absence with `skip_fallback_headers=true`,
            // but the custom `<status>.html` branch still applies user
            // rules — reference invokes `getHeaders(.., errorPage,
            // stats)` at `index.js:508` whether or not the URIError
            // path was outside root. Codex review round 3 P1.
            let resp = error_response(
                StatusCode::BAD_REQUEST,
                req.headers(),
                root,
                header_rules,
                &raw_path,
                true,
            )
            .await;
            return (resp, None);
        }
    };

    // Phase-10 lexical containment check (SRV-SEC-001). Mirrors the
    // reference's `isPathInside(path.join(current, relativePath), current)`
    // gate at `serve-handler/src/index.js:570-580`: a `..` segment that
    // would pop above the served root yields 400, before any filesystem
    // I/O. The check is purely lexical to match `path.posix.join`'s
    // normalization (e.g. leading `//` collapses to `/` and is benign).
    //
    // Empirically, reference's fallback `getHeaders(.., absolutePath,
    // null)` for traversal 400 fails to match common rules like `**`
    // because `path.relative(current, outsideAbsolutePath)` produces a
    // `../`-prefixed string whose slasher-normalized form is brittle
    // (D-015). Skip the fallback branch's header application, but
    // keep the custom `<status>.html` branch on — Codex review round 3
    // P1: a user-supplied `400.html` MUST still receive its rule-set
    // headers (reference's `getHeaders(.., errorPage, stats)` at
    // `index.js:508` runs regardless).
    if lexical_path_escapes_root(&decoded_path) {
        let resp = error_response(
            StatusCode::BAD_REQUEST,
            req.headers(),
            root,
            header_rules,
            &decoded_path,
            true,
        )
        .await;
        return (resp, None);
    }

    // Phase 4: cleanUrls 301 (SRV-ROUT-001). Runs on the decoded
    // (uncollapsed) path, before phase 5, matching `shouldRedirect`'s
    // ordering at `serve-handler/src/index.js:121-143`. Wins over
    // trailingSlash, redirects, rewrites, and the existing-file
    // pre-stat (per SRV-ROUT-006 scenario "cleanUrls 301 wins over
    // existing-file short-circuit"). 3xx redirects skip custom-header
    // application — reference's redirect path at `index.js:586-588`
    // builds the response via `response.writeHead(redirect.statusCode,
    // { Location: ... })` without going through `getHeaders`.
    if let Some(target) = compute_clean_urls_redirect(&decoded_path, clean_urls_view) {
        return (redirect_301(&target), None);
    }

    // Phase 5: trailingSlash 301 (SRV-ROUT-003 / SRV-ROUT-004), with the
    // multi-slash override from SRV-ROUT-005 (`index.js:158-160`).
    // Operates on the decoded (uncollapsed) path so the override
    // trigger is the input's `//` content.
    if let Some(target) =
        compute_trailing_slash_redirect(&decoded_path, serve_config.trailing_slash)
    {
        return (redirect_301(&target), None);
    }

    // Phase 3: silent multi-slash collapse for resolve-and-onwards
    // stages (SRV-ROUT-005, Q-006 closed). Pure pre-routing transform;
    // never emits a redirect on its own — when `trailingSlash` is set,
    // the redirect for `//` is emitted by phase 5 above.
    let url_path = collapse_slashes(&decoded_path);

    // Phase 6: configured redirects (Stage 6d). First-match-wins
    // iteration over the precompiled rules, mirroring `shouldRedirect`'s
    // redirects branch at `serve-handler/src/index.js:172-182`. The
    // status code defaults to 301 when the rule has no `type` override
    // (`index.js:179`, `statusCode: type || defaultType`).
    if let Some((target, status)) = compute_configured_redirects(&url_path, redirect_rules) {
        return (redirect_with_status(&target, status), None);
    }
    // Phase 7: configured rewrites + `--single` SPA fallback
    // (Stage 6e). Phase 7 interleaves with phases 8/9 per the
    // pre-stat asymmetry mandated by `openspec/specs/rewrites/spec.md`
    // and mirrored from `serve-handler/src/index.js:608-642`:
    //
    //   * Has-extension + file exists → pre-stat short-circuits;
    //     rewrites are NEVER consulted (`index.js:608-616`).
    //   * Has-extension + file missing → apply rewrites; the
    //     rewritten path's resolution wins over `<P>.html` cleanUrls
    //     candidates (mirrors `findRelated`'s
    //     `rewrittenPath ? [rewrittenPath] : getPossiblePaths(...)`
    //     branch).
    //   * Extensionless → apply rewrites unconditionally; a matching
    //     rewrite serves its destination even when the extensionless
    //     original exists as a regular file (per spec
    //     `openspec/specs/rewrites/spec.md:27-29`).
    //
    // Phase 8 (cleanUrls resolution, SRV-ROUT-002) and phase 9
    // (final resolve) are wired below. Phase 8 only runs as the
    // post-rewrite fallback when no rewrite matched — when a rewrite
    // matches, only the rewritten path is tried (mirrors
    // `findRelated`'s "rewrittenPath wins, no further candidates"
    // semantics).
    let url_has_extension = url_path_has_extension(&url_path);

    // `lexical_url` tracks the URL form of the path that ultimately
    // resolved (or attempted to resolve). For successes it feeds
    // `apply_custom_headers` as the matching path, mirroring
    // reference's `path.relative(current, absolutePath)` lookup inside
    // `getHeaders` (where `absolutePath` was updated by `findRelated`
    // to the resolved candidate's lexical path, NOT canonicalized).
    // Codex review round 3 P2: the prior implementation used the
    // canonicalized `PathBuf`, which on Windows folds `/ASSET.CSS`'s
    // case to `/asset.css` and admits matches that reference's
    // case-sensitive minimatch rejects.
    let url_path_str: String = url_path.clone().into_owned();
    let trimmed = url_path_str
        .trim_start_matches('/')
        .trim_end_matches('/');
    let cleanurls_index_url = if trimmed.is_empty() {
        "/index.html".to_string()
    } else {
        format!("/{}/index.html", trimmed)
    };
    let cleanurls_flat_url = if trimmed.is_empty() {
        // try_clean_urls_resolve never produces a `<P>.html` candidate
        // for the empty-P root; this string is unreachable as a
        // lexical-url winner, but we set it for completeness.
        url_path_str.clone()
    } else {
        format!("/{}.html", trimmed)
    };

    let (outcome, lexical_url): (ResolveOutcome, String) = if !url_has_extension {
        // Extensionless. Apply rewrites first; if matched, resolve
        // the rewritten path. If the rewritten path doesn't resolve,
        // fall back to the ORIGINAL path's resolve — mirrors the
        // reference's final `lstat(absolutePath)` at
        // `serve-handler/src/index.js:634-642` after `findRelated`
        // returns null. The fallback is the original path ONLY (no
        // cleanUrls candidates).
        if let Some(target) = compute_configured_rewrites(&url_path, rewrite_rules) {
            match resolve(&target, root).await {
                ResolveOutcome::NotFound | ResolveOutcome::EscapedRoot => {
                    (resolve(&url_path, root).await, url_path_str.clone())
                }
                other => (other, target),
            }
        } else {
            // No rewrite matched. The reference's `findRelated` here
            // consults cleanUrls candidates (`<P>/index.html` then
            // `<P>.html`) for extensionless requests. The variant
            // distinguishes which candidate succeeded:
            //   ResolveOutcome::Index → `<P>/index.html`
            //   ResolveOutcome::File  → `<P>.html`
            match try_clean_urls_resolve(&url_path, root, clean_urls_view).await {
                Some(o @ ResolveOutcome::Index(_)) => (o, cleanurls_index_url.clone()),
                Some(o @ ResolveOutcome::File(_)) => (o, cleanurls_flat_url.clone()),
                Some(o) => (o, url_path_str.clone()),
                None => (resolve(&url_path, root).await, url_path_str.clone()),
            }
        }
    } else {
        // Has-extension. Pre-stat the original path first; if it
        // resolves, serve it (rewrites NEVER consulted). On miss,
        // apply rewrites; if a rewrite matched, resolve its
        // destination. If no rewrite matched, fall back to the
        // existing cleanUrls candidate.
        match resolve(&url_path, root).await {
            ResolveOutcome::NotFound => {
                if let Some(target) = compute_configured_rewrites(&url_path, rewrite_rules) {
                    let r = resolve(&target, root).await;
                    (r, target)
                } else {
                    match try_clean_urls_resolve(&url_path, root, clean_urls_view).await {
                        Some(o @ ResolveOutcome::Index(_)) => (o, cleanurls_index_url.clone()),
                        Some(o @ ResolveOutcome::File(_)) => (o, cleanurls_flat_url.clone()),
                        Some(o) => (o, url_path_str.clone()),
                        None => (ResolveOutcome::NotFound, url_path_str.clone()),
                    }
                }
            }
            other => (other, url_path_str.clone()),
        }
    };

    match outcome {
        ResolveOutcome::File(p) | ResolveOutcome::Index(p) => match tokio::fs::read(&p).await {
            Ok(bytes) => (file_response(&p, bytes), Some(lexical_url)),
            Err(_) => {
                let resp = error_response(
                    StatusCode::NOT_FOUND,
                    req.headers(),
                    root,
                    header_rules,
                    &decoded_path,
                    false,
                )
                .await;
                (resp, None)
            }
        },
        ResolveOutcome::NotFound => {
            let resp = error_response(
                StatusCode::NOT_FOUND,
                req.headers(),
                root,
                header_rules,
                &decoded_path,
                false,
            )
            .await;
            (resp, None)
        }
        // Defense-in-depth: the lexical check at phase 1 already
        // short-circuits `..`-escaping requests; this branch covers
        // symlink/canonicalize-based escapes. Skip the fallback HTML
        // branch's header application (mirrors lexical-escape) but
        // keep the custom `<status>.html` branch on — same reasoning
        // as the lexical-escape site (Codex review round 3 P1).
        ResolveOutcome::EscapedRoot => {
            let resp = error_response(
                StatusCode::BAD_REQUEST,
                req.headers(),
                root,
                header_rules,
                &decoded_path,
                true,
            )
            .await;
            (resp, None)
        }
    }
}

/// Mirror of Node's `path.extname(p)` for our URL-path use case.
///
/// Node's `path.posix.extname` strips trailing slashes from `p`
/// before computing the basename, so `path.posix.extname('/foo.txt/')`
/// returns `'.txt'` (NOT empty). Codex review round 1 P2: the
/// previous implementation used `rsplit('/').next()` which yields
/// the empty trailing segment, mis-classifying trailing-slash paths
/// as extensionless and letting rewrites fire on directory-like
/// requests where the reference does not.
///
/// An extension is a non-empty `.xxx` substring after the last `/`
/// (post-trailing-slash trim) that is NOT the leading character of
/// the basename. Dotfiles (`/.bashrc`) have no extension; nested
/// dotfiles with extensions (`/.bashrc.bak`) do.
fn url_path_has_extension(path: &str) -> bool {
    let basename = match path.rsplit('/').find(|b| !b.is_empty()) {
        Some(b) => b,
        None => return false, // root or all-empty
    };
    // A leading-dot basename whose ONLY dot is the leading one has no
    // extension. Skip char index 0 when scanning for an extension dot.
    basename.char_indices().skip(1).any(|(_, ch)| ch == '.')
}

/// Single-pass URL decode with strict syntactic AND UTF-8 validation.
/// Returns `Err` if any `%` is not followed by exactly two ASCII-hex
/// chars, OR if the resulting byte sequence is not valid UTF-8.
///
/// Reference: `serve-handler/src/index.js:561-567` —
/// `try { relativePath = decodeURIComponent(...) } catch (URIError) { 400 }`.
/// `decodeURIComponent` throws URIError on both malformed `%xx` syntax
/// AND on byte sequences that are valid escapes but invalid UTF-8
/// (e.g. `/%FF` — a single 0xFF byte that does not start a valid
/// UTF-8 sequence). Codex review round 1 P2 surfaced that the prior
/// `decode_utf8_lossy()` silently mapped invalid UTF-8 to U+FFFD,
/// admitting requests that the reference rejects with 400.
fn try_percent_decode(s: &str) -> Result<Cow<'_, str>, ()> {
    let bytes = s.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' {
            if i + 2 >= bytes.len()
                || !is_ascii_hex(bytes[i + 1])
                || !is_ascii_hex(bytes[i + 2])
            {
                return Err(());
            }
            i += 3;
        } else {
            i += 1;
        }
    }
    percent_decode_str(s).decode_utf8().map_err(|_| ())
}

fn is_ascii_hex(b: u8) -> bool {
    matches!(b, b'0'..=b'9' | b'a'..=b'f' | b'A'..=b'F')
}

/// Returns true if the URL path's `..` segments pop above the served
/// root. Mirrors the reference's lexical check via
/// `path.posix.join(root, decoded).startsWith(root)`: empty segments
/// (from `//` runs) and `.` segments are no-ops; `..` decrements depth
/// and below-zero depth is the escape signal. Does no filesystem I/O.
fn lexical_path_escapes_root(decoded_path: &str) -> bool {
    let mut depth: i32 = 0;
    for seg in decoded_path.split('/') {
        match seg {
            "" | "." => {}
            ".." => {
                depth -= 1;
                if depth < 0 {
                    return true;
                }
            }
            _ => depth += 1,
        }
    }
    false
}

/// Mirrors JavaScript's `encodeURI` (the function the reference applies
/// to redirect targets at `serve-handler/src/index.js:586`):
///
/// * Unreserved (kept as-is): `A-Z a-z 0-9 - _ . ! ~ * ' ( )`
/// * Reserved (kept as-is): `; , / ? : @ & = + $ #`
/// * Encoded: SPACE `"` `%` `<` `>` `\` `^` `` ` `` `{` `|` `}` `[` `]`
///   + control chars + every non-ASCII byte (each UTF-8 byte → `%xx`).
const ENCODE_URI_SET: &AsciiSet = &CONTROLS
    .add(b' ')
    .add(b'"')
    .add(b'%')
    .add(b'<')
    .add(b'>')
    .add(b'\\')
    .add(b'^')
    .add(b'`')
    .add(b'{')
    .add(b'|')
    .add(b'}')
    .add(b'[')
    .add(b']');

pub(crate) fn encode_uri_target(target: &str) -> String {
    utf8_percent_encode(target, ENCODE_URI_SET).to_string()
}

fn redirect_301(target: &str) -> Response<Body> {
    redirect_with_status(target, 301)
}

/// Build a 3xx redirect response with the given status code. Mirrors
/// `serve-handler/src/index.js:586-587` which writes
/// `{Location: encodeURI(redirect.target)}` with whatever status the
/// rule resolved to. Out-of-range or otherwise invalid status codes
/// fall back to 301; this mirrors the "accept any 3xx; range-checking
/// is not specified" stance of the spec
/// (`openspec/specs/redirects/spec.md` SRV-RDIR-002 Note).
fn redirect_with_status(target: &str, status: u16) -> Response<Body> {
    let encoded = encode_uri_target(target);
    let location = HeaderValue::from_str(&encoded)
        .unwrap_or_else(|_| HeaderValue::from_static("/"));
    let status_code = StatusCode::from_u16(status).unwrap_or(StatusCode::MOVED_PERMANENTLY);
    Response::builder()
        .status(status_code)
        .header(LOCATION, location)
        .body(Body::empty())
        .expect("redirect response should always build")
}

#[cfg(test)]
mod tests {
    use super::{encode_uri_target, url_path_has_extension};

    #[test]
    fn extension_basic_paths() {
        assert!(url_path_has_extension("/foo.txt"));
        assert!(url_path_has_extension("/dir/foo.css"));
        assert!(!url_path_has_extension("/about"));
        assert!(!url_path_has_extension("/dir/sub"));
    }

    #[test]
    fn extension_root_and_empty() {
        assert!(!url_path_has_extension("/"));
        assert!(!url_path_has_extension(""));
    }

    #[test]
    fn extension_dotfile_no_ext() {
        // `.bashrc` is a dotfile; Node's path.extname returns ''.
        assert!(!url_path_has_extension("/.bashrc"));
    }

    #[test]
    fn extension_nested_dotfile_with_ext() {
        // `.bashrc.bak` has extension `.bak`.
        assert!(url_path_has_extension("/.bashrc.bak"));
    }

    #[test]
    fn extension_trailing_slash_preserves_extension() {
        // Codex round 1 P2: Node's `path.extname('/foo.txt/')` ===
        // '.txt' because trailing slashes are trimmed before
        // computing the basename. The previous implementation used
        // `rsplit('/').next()` which captured the empty trailing
        // segment and mis-classified `/foo.txt/` as extensionless,
        // letting rewrites fire on directory-like requests where
        // the reference does not.
        assert!(url_path_has_extension("/foo.txt/"));
        assert!(url_path_has_extension("/dir/foo.css/"));
    }

    #[test]
    fn extension_trailing_slash_extensionless_stays_extensionless() {
        assert!(!url_path_has_extension("/about/"));
        assert!(!url_path_has_extension("/dir/sub/"));
    }

    #[test]
    fn encode_keeps_safe_ascii_path() {
        assert_eq!(encode_uri_target("/about/"), "/about/");
        assert_eq!(encode_uri_target("/api/v1/users"), "/api/v1/users");
    }

    #[test]
    fn encode_preserves_query_and_reserved_chars() {
        // encodeURI keeps ? = & : @ + $ , # / ; intact.
        assert_eq!(
            encode_uri_target("/path?a=1&b=2"),
            "/path?a=1&b=2"
        );
        assert_eq!(
            encode_uri_target("/with:colons@and+pluses,$dollars#frag"),
            "/with:colons@and+pluses,$dollars#frag"
        );
    }

    #[test]
    fn encode_space_to_percent_20() {
        assert_eq!(encode_uri_target("/foo bar/"), "/foo%20bar/");
    }

    #[test]
    fn encode_non_ascii_to_utf8_bytes() {
        // 'é' is U+00E9 → UTF-8 bytes C3 A9.
        assert_eq!(encode_uri_target("/café/"), "/caf%C3%A9/");
        // 'д' is U+0434 → UTF-8 bytes D0 B4.
        assert_eq!(encode_uri_target("/привет"), "/%D0%BF%D1%80%D0%B8%D0%B2%D0%B5%D1%82");
    }

    #[test]
    fn encode_literal_percent_becomes_percent_25() {
        // A literal `%` left over after decode (e.g. `%25` in the input
        // path) must be re-encoded to `%25` to match `encodeURI`.
        assert_eq!(encode_uri_target("/100%off/"), "/100%25off/");
    }

    #[test]
    fn encode_brackets_and_quotes() {
        assert_eq!(encode_uri_target("/[x]/\"y\""), "/%5Bx%5D/%22y%22");
    }

    #[test]
    fn encode_control_chars() {
        // Newline (0x0A) is a control character.
        assert_eq!(encode_uri_target("/a\nb"), "/a%0Ab");
    }
}

fn file_response(path: &Path, bytes: Vec<u8>) -> Response<Body> {
    let mut builder = Response::builder().status(StatusCode::OK);
    if let Some(mime) = mime_for(path) {
        builder = builder.header(CONTENT_TYPE, HeaderValue::from_static(mime));
    }
    builder
        .body(Body::from(bytes))
        .expect("file response should always build")
}
