use std::path::Path;

use axum::body::Body;
use axum::http::header::{HeaderValue, CONTENT_TYPE, LOCATION};
use axum::http::{Method, Request, Response, StatusCode};
use percent_encoding::{percent_decode_str, utf8_percent_encode, AsciiSet, CONTROLS};

use crate::clean_urls::{
    compute_clean_urls_redirect, try_clean_urls_resolve, CleanUrlsView,
};
use crate::config::ServeConfig;
use crate::mime::mime_for;
use crate::normalize::collapse_slashes;
use crate::notfound::not_found_response;
use crate::resolve::{resolve, ResolveOutcome};
use crate::trailing_slash::compute_trailing_slash_redirect;

pub async fn dispatch(
    req: Request<Body>,
    root: &Path,
    serve_config: &ServeConfig,
    clean_urls_view: &CleanUrlsView,
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
    let raw_path = req.uri().path();
    let decoded_path = percent_decode_str(raw_path).decode_utf8_lossy();

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

    // Phase 6: configured redirects (Stage 6d).
    // Phase 7: rewrites + --single (Stage 6e).

    // Phase 8: cleanUrls resolution (SRV-ROUT-002). Mirrors
    // `serve-handler/src/index.js:608-642` precisely:
    //
    //   * For paths WITH an extension, the reference does a pre-stat
    //     first (`index.js:608-616`) and only falls into `findRelated`
    //     when that pre-stat misses. This avoids `findRelated`
    //     shadowing a real `/foo.css` file via `/foo.css.html`.
    //   * For paths WITHOUT an extension, no pre-stat — `findRelated`
    //     runs first, so an existing `<P>.html` is preferred over a
    //     bare extensionless file at `<P>` (matches the SRV-ROUT-006
    //     scenario "If P has no extension … the original path SHALL be
    //     attempted only at stage 6").
    //
    // We map this onto our existing `resolve()` (which combines
    // pre-stat + directory index handling) by gating the phase-8
    // attempt: extensionless paths try phase 8 first; has-extension
    // paths try phase 8 only if `resolve()` reported NotFound.
    let url_has_extension = url_path_has_extension(&url_path);

    let outcome = if !url_has_extension {
        // Extensionless: phase 8 first, then phase 9 fallback.
        match try_clean_urls_resolve(&url_path, root, clean_urls_view).await {
            Some(o) => o,
            None => resolve(&url_path, root).await,
        }
    } else {
        // Has-ext: phase 9 first; if NotFound, fall to phase 8.
        match resolve(&url_path, root).await {
            ResolveOutcome::NotFound => try_clean_urls_resolve(&url_path, root, clean_urls_view)
                .await
                .unwrap_or(ResolveOutcome::NotFound),
            other => other,
        }
    };

    match outcome {
        ResolveOutcome::File(p) | ResolveOutcome::Index(p) => match tokio::fs::read(&p).await {
            Ok(bytes) => file_response(&p, bytes),
            Err(_) => not_found_response(req.headers()),
        },
        ResolveOutcome::NotFound | ResolveOutcome::EscapedRoot => not_found_response(req.headers()),
    }
}

/// Mirror of Node's `path.extname(p)` for our URL-path use case: an
/// extension is a non-empty `.xxx` substring after the last `/` that
/// is NOT the leading character of the basename. Trailing-slash paths
/// (`/foo.txt/`) and dotfiles (`/.bashrc`) have no extension; nested
/// dotfiles with extensions (`/.bashrc.bak`) do.
fn url_path_has_extension(path: &str) -> bool {
    let basename = match path.rsplit('/').next() {
        Some(b) if !b.is_empty() => b,
        _ => return false, // empty (root or trailing-slash path)
    };
    // A leading-dot basename whose ONLY dot is the leading one has no
    // extension. Skip char index 0 when scanning for an extension dot.
    basename.char_indices().skip(1).any(|(_, ch)| ch == '.')
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
    let encoded = encode_uri_target(target);
    let location = HeaderValue::from_str(&encoded)
        .unwrap_or_else(|_| HeaderValue::from_static("/"));
    Response::builder()
        .status(StatusCode::MOVED_PERMANENTLY)
        .header(LOCATION, location)
        .body(Body::empty())
        .expect("301 response should always build")
}

#[cfg(test)]
mod tests {
    use super::encode_uri_target;

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
