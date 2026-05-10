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
) -> Response<Body> {
    // Phase 1–2: method gate (existing).
    if req.method() != Method::GET && req.method() != Method::HEAD {
        return Response::builder()
            .status(StatusCode::METHOD_NOT_ALLOWED)
            .body(Body::empty())
            .expect("405 response should always build");
    }

    // Decode the URI path once at the dispatcher entry. Mirrors the
    // reference's `decodedPath` invariant (`serve-handler/src/index.js:561`).
    // Subsequent phases operate on this decoded form so that encoded
    // forms like `%2F%2F` collapse identically to literal `//`.
    //
    // Phase-1 strict syntactic gate (SRV-SEC-002): a `%` not followed by
    // exactly two ASCII-hex chars is malformed, mirroring `decodeURIComponent`'s
    // URIError branch at `index.js:561-567`. Short-circuit to 400 before
    // any phase 2+ runs.
    let raw_path = req.uri().path();
    let decoded_path = match try_percent_decode(raw_path) {
        Ok(p) => p,
        Err(_) => return error_response(StatusCode::BAD_REQUEST, req.headers(), root).await,
    };

    // Phase-10 lexical containment check (SRV-SEC-001). Mirrors the
    // reference's `isPathInside(path.join(current, relativePath), current)`
    // gate at `serve-handler/src/index.js:570-580`: a `..` segment that
    // would pop above the served root yields 400, before any filesystem
    // I/O. The check is purely lexical to match `path.posix.join`'s
    // normalization (e.g. leading `//` collapses to `/` and is benign).
    if lexical_path_escapes_root(&decoded_path) {
        return error_response(StatusCode::BAD_REQUEST, req.headers(), root).await;
    }

    // Phase 4: cleanUrls 301 (SRV-ROUT-001). Runs on the decoded
    // (uncollapsed) path, before phase 5, matching `shouldRedirect`'s
    // ordering at `serve-handler/src/index.js:121-143`. Wins over
    // trailingSlash, redirects, rewrites, and the existing-file
    // pre-stat (per SRV-ROUT-006 scenario "cleanUrls 301 wins over
    // existing-file short-circuit").
    if let Some(target) = compute_clean_urls_redirect(&decoded_path, clean_urls_view) {
        return redirect_301(&target);
    }

    // Phase 5: trailingSlash 301 (SRV-ROUT-003 / SRV-ROUT-004), with the
    // multi-slash override from SRV-ROUT-005 (`index.js:158-160`).
    // Operates on the decoded (uncollapsed) path so the override
    // trigger is the input's `//` content.
    if let Some(target) =
        compute_trailing_slash_redirect(&decoded_path, serve_config.trailing_slash)
    {
        return redirect_301(&target);
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
        return redirect_with_status(&target, status);
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

    let outcome = if !url_has_extension {
        // Extensionless. Apply rewrites first; if matched, resolve
        // the rewritten path. If the rewritten path doesn't
        // resolve, fall back to the ORIGINAL path's resolve —
        // mirrors the reference's final `lstat(absolutePath)` at
        // `serve-handler/src/index.js:634-642` after `findRelated`
        // returns null. The fallback is the original path ONLY
        // (no cleanUrls candidates) because `findRelated`'s
        // `rewrittenPath ? [rewrittenPath] : getPossiblePaths(...)`
        // branch at `index.js:618-632` consults cleanUrls
        // candidates only when no rewrite matched; once a rewrite
        // is set, only the rewritten path is tried, and the final
        // lstat on the ORIGINAL absolutePath is the sole fallback
        // (which can succeed for extensionless original files
        // that exist as plain files or directories with
        // `index.html`).
        if let Some(target) = compute_configured_rewrites(&url_path, rewrite_rules) {
            match resolve(&target, root).await {
                ResolveOutcome::NotFound | ResolveOutcome::EscapedRoot => {
                    resolve(&url_path, root).await
                }
                other => other,
            }
        } else {
            // No rewrite matched. The reference's `findRelated`
            // here consults cleanUrls candidates
            // (`<P>/index.html` then `<P>.html`) for extensionless
            // requests. Mirrors `getPossiblePaths` at
            // `serve-handler/src/index.js:276` (called from
            // `findRelated` at `index.js:282`).
            match try_clean_urls_resolve(&url_path, root, clean_urls_view).await {
                Some(o) => o,
                None => resolve(&url_path, root).await,
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
                    resolve(&target, root).await
                } else {
                    try_clean_urls_resolve(&url_path, root, clean_urls_view)
                        .await
                        .unwrap_or(ResolveOutcome::NotFound)
                }
            }
            other => other,
        }
    };

    match outcome {
        ResolveOutcome::File(p) | ResolveOutcome::Index(p) => match tokio::fs::read(&p).await {
            Ok(bytes) => file_response(&p, bytes),
            Err(_) => error_response(StatusCode::NOT_FOUND, req.headers(), root).await,
        },
        ResolveOutcome::NotFound => {
            error_response(StatusCode::NOT_FOUND, req.headers(), root).await
        }
        // Defense-in-depth: lexical check above already short-circuits
        // `..`-escaping requests at phase 1; this branch covers the rare
        // case of a symlink (or future routing target) whose canonical
        // form lands outside the served root.
        ResolveOutcome::EscapedRoot => {
            error_response(StatusCode::BAD_REQUEST, req.headers(), root).await
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

/// Single-pass URL decode with strict syntactic validation. Returns `Err`
/// if any `%` is not followed by exactly two ASCII-hex chars; otherwise
/// returns the lossy-UTF-8 decoded form (mirrors the existing decoder
/// for valid escapes — invalid UTF-8 sequences become U+FFFD, matching
/// the prior `decode_utf8_lossy` semantics).
///
/// Reference: `serve-handler/src/index.js:561-567` —
/// `try { relativePath = decodeURIComponent(...) } catch (URIError) { 400 }`.
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
    Ok(percent_decode_str(s).decode_utf8_lossy())
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
