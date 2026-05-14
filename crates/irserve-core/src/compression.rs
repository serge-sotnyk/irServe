//! HTTP compression (Stage 7e, SRV-CLI-012).
//!
//! Mirrors the reference's `compression@1.8.1` middleware
//! (`third_party/serve/node_modules/compression/index.js`) — the surface
//! pinned in `tools/probe/snapshots/compression-raw.json`:
//!
//! - Default body-size threshold: 1024 bytes.
//! - Negotiation preference: brotli > gzip > deflate. Identity is the
//!   implicit fallback; we honor `q=0` exclusions and `*;q=0` wildcard
//!   rejection. Q-rank values between 0 and 1 are NOT honored (out of
//!   scope per D-020).
//! - Compressible MIMEs: curated allowlist (`text/*`,
//!   `application/json`, `application/javascript`, `application/wasm`,
//!   `image/svg+xml`) + regex fallback `^text/|\+(?:json|text|xml)$`
//!   (case-insensitive) per `compressible@2.0.18`'s rules. The full
//!   `mime-db` `compressible` table is NOT ported (out of scope per
//!   D-020).
//! - `Vary: Accept-Encoding` is set IFF the content-type is
//!   compressible AND the response is not skipping via `no-transform`
//!   — set BEFORE the HEAD / threshold / negotiation skips, so it
//!   appears on every "would-have-compressed-but-X" path.
//! - Skips: HEAD method (Vary kept, body emptied by axum/hyper),
//!   `Cache-Control: no-transform` (no Vary, no compression), body
//!   below threshold (Vary kept, no compression), no acceptable
//!   encoding (Vary kept, no compression).
//! - Range requests pre-empt compression entirely (the caller — see
//!   `dispatch.rs::build_file_or_304` — never enters `maybe_apply`
//!   when a `Range` header is present).
//!
//! Divergences from reference are enumerated in D-020:
//! - Compressed responses send `Content-Length: <compressed-len>` and
//!   no `Transfer-Encoding: chunked` (irserve has the final bytes in
//!   memory; the reference uses chunked because compression is
//!   stream-based).
//! - Body bytes of compressed responses are NOT byte-identical to the
//!   reference's — both sides produce valid encodings of the same
//!   logical body, but encoder defaults differ in quality / window
//!   size between Node zlib/brotli and the Rust `flate2` / `brotli`
//!   crates.

use std::io::Write;

use axum::body::Body;
use axum::http::header::{
    HeaderMap, HeaderValue, CACHE_CONTROL, CONTENT_ENCODING, CONTENT_LENGTH, CONTENT_TYPE, VARY,
};
use axum::http::{Method, Response, StatusCode};

use crate::config::ServeConfig;

/// Reference threshold (`compression/index.js:76-78`).
pub const DEFAULT_THRESHOLD: usize = 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Encoding {
    Brotli,
    Gzip,
    Deflate,
}

impl Encoding {
    fn as_str(self) -> &'static str {
        match self {
            Encoding::Brotli => "br",
            Encoding::Gzip => "gzip",
            Encoding::Deflate => "deflate",
        }
    }
}

/// Choose an encoder from `Accept-Encoding`. Honors `q=0` exclusions
/// and `*` wildcard accept / reject. Preference order matches the
/// reference's `PREFERRED_ENCODING` array at
/// `compression/index.js:44-45`: brotli first when available.
///
/// Returns `None` when no acceptable encoding is available (header
/// absent, identity-only, all excluded).
pub fn negotiate(accept_encoding: Option<&HeaderValue>) -> Option<Encoding> {
    let header = accept_encoding?.to_str().ok()?;

    // Parse `name;q=N` tokens.
    let mut allow_br = false;
    let mut allow_gzip = false;
    let mut allow_deflate = false;
    let mut wildcard_accept = false;
    let mut wildcard_reject = false;
    let mut br_q0 = false;
    let mut gzip_q0 = false;
    let mut deflate_q0 = false;

    for token in header.split(',') {
        let token = token.trim();
        if token.is_empty() {
            continue;
        }
        let (name, q) = parse_token(token);
        let name_lower = name.to_ascii_lowercase();
        let is_zero = q == Some(0.0);
        match name_lower.as_str() {
            "br" => {
                if is_zero {
                    br_q0 = true;
                } else {
                    allow_br = true;
                }
            }
            "gzip" => {
                if is_zero {
                    gzip_q0 = true;
                } else {
                    allow_gzip = true;
                }
            }
            "deflate" => {
                if is_zero {
                    deflate_q0 = true;
                } else {
                    allow_deflate = true;
                }
            }
            "*" => {
                if is_zero {
                    wildcard_reject = true;
                } else {
                    wildcard_accept = true;
                }
            }
            _ => {}
        }
    }

    // `*;q=0` means "reject everything not explicitly accepted".
    // If nothing else is accepted, return None.
    let br_ok = (allow_br || (wildcard_accept && !br_q0)) && !br_q0;
    let gzip_ok = (allow_gzip || (wildcard_accept && !gzip_q0)) && !gzip_q0;
    let deflate_ok = (allow_deflate || (wildcard_accept && !deflate_q0)) && !deflate_q0;

    if wildcard_reject && !allow_br && !allow_gzip && !allow_deflate {
        return None;
    }

    if br_ok {
        Some(Encoding::Brotli)
    } else if gzip_ok {
        Some(Encoding::Gzip)
    } else if deflate_ok {
        Some(Encoding::Deflate)
    } else {
        None
    }
}

fn parse_token(token: &str) -> (&str, Option<f32>) {
    if let Some((name, rest)) = token.split_once(';') {
        let name = name.trim();
        for param in rest.split(';') {
            let param = param.trim();
            if let Some(qv) = param.strip_prefix("q=").or_else(|| param.strip_prefix("Q=")) {
                return (name, qv.trim().parse().ok());
            }
        }
        (name, None)
    } else {
        (token, None)
    }
}

/// Decide whether the response's content-type is compressible.
/// Mirrors `compressible@2.0.18`'s `compressible()` shape: a curated
/// allowlist (drawn from `mime-db@1.33.0`'s `compressible: true`
/// entries for common types) combined with the regex fallback
/// `^text/|\+(?:json|text|xml)$/i` for long-tail subtypes.
///
/// Examples that match the allowlist: `text/html`, `text/css`,
/// `application/json`, `application/javascript`, `application/wasm`,
/// `image/svg+xml`. Examples that match the regex fallback:
/// `application/ld+json`, `application/atom+xml`, `text/plain`.
/// Examples that do NOT match (so `is_compressible` returns false):
/// `image/png`, `font/woff2`, `video/mp4`, `application/octet-stream`.
pub fn is_compressible(content_type: &str) -> bool {
    let mime = content_type
        .split(';')
        .next()
        .unwrap_or("")
        .trim()
        .to_ascii_lowercase();
    if mime.is_empty() {
        return false;
    }

    // Curated allowlist.
    const ALLOWLIST: &[&str] = &[
        "application/json",
        "application/javascript",
        "application/wasm",
        "image/svg+xml",
    ];
    if ALLOWLIST.contains(&mime.as_str()) {
        return true;
    }

    // Regex fallback: anything `text/*` or `*+json` / `*+text` / `*+xml`.
    if mime.starts_with("text/") {
        return true;
    }
    if let Some(plus_idx) = mime.find('+') {
        let suffix = &mime[plus_idx + 1..];
        if suffix == "json" || suffix == "text" || suffix == "xml" {
            return true;
        }
    }
    false
}

/// Apply HTTP compression to a response if the gate passes.
///
/// Centralized post-dispatch pass (Stage 7e Codex round 1 P2 — moved
/// here from the per-branch `build_file_or_304` site so listings and
/// error pages get compression too, mirroring the reference's
/// `compression` middleware which fires on every response). Consumes
/// the body asynchronously to inspect its bytes against the
/// 1024-byte threshold.
///
/// `method` controls HEAD skipping (mirroring
/// `compression/index.js:192-195`).
///
/// `accept_encoding` provides the negotiation source.
///
/// `serve_config.compression` gates the entire feature — when
/// `Some(false)` (the `-u`/`--no-compression` flag was set), the
/// response is returned unchanged with no `Vary` added.
///
/// Mutations applied to the response on the compress / Vary path:
/// - On 206 Partial Content (Range): nothing (pre-emption — body is
///   already a sliced range and `Content-Range` would lie after
///   re-compression).
/// - On non-compressible MIME OR `Cache-Control: no-transform`:
///   nothing (response returned as-is).
/// - On compressible MIME, no `no-transform`: `Accept-Encoding` is
///   APPENDED to any existing `Vary` header (case-insensitive,
///   deduplicated), mirroring the reference's `vary()` utility at
///   `third_party/serve/node_modules/compression/index.js:174`.
/// - On the actual compress path: `Content-Encoding` set to the
///   chosen encoder's token, `Content-Length` overwritten with the
///   compressed body length, body replaced.
pub async fn maybe_apply(
    response: Response<Body>,
    method: &Method,
    accept_encoding: Option<&HeaderValue>,
    serve_config: &ServeConfig,
) -> Response<Body> {
    if serve_config.compression == Some(false) {
        return response;
    }

    let content_type = response
        .headers()
        .get(CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_string();

    // Non-compressible MIME → middleware never sets Vary; mirror.
    if !is_compressible(&content_type) {
        return response;
    }

    let cache_control = response
        .headers()
        .get(CACHE_CONTROL)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_string();

    // `Cache-Control: no-transform` short-circuits BEFORE vary(),
    // per the empirical capture in `compression-raw.json` for the
    // `no_transform_big_html_gzip` anchor.
    if cache_control_contains_no_transform(&cache_control) {
        return response;
    }

    let (mut parts, body) = response.into_parts();

    // Vary append. Codex round 1 P2 — mirror the reference's
    // `vary()` utility (at `third_party/serve/node_modules/vary/index.js`,
    // invoked from `compression/index.js:174`) which appends
    // `Accept-Encoding` to any existing `Vary` instead of skipping.
    // Without append, a user `headers` rule setting `Vary: Cookie`
    // would emit `Vary: Cookie` + `Content-Encoding: br` — a
    // cache-incorrect surface (downstream caches would key on
    // Cookie only and serve the brotli body to clients that asked
    // for identity).
    append_vary_accept_encoding(&mut parts.headers);

    // SRV-CACHE-004 / Stage 7c composition: Range pre-emption.
    // `build_file_or_304` already emitted a 206 with a sliced body
    // and a `Content-Range: bytes <s>-<e>/<total>` header that
    // addresses the ORIGINAL bytes; compressing the slice now
    // would invalidate Content-Range. Vary is kept (the negotiation
    // hook engaged on a compressible MIME, mirroring the reference's
    // 206 anchor in `compression-raw.json`).
    if parts.status == StatusCode::PARTIAL_CONTENT {
        return Response::from_parts(parts, body);
    }

    // HEAD: middleware skips compression but Vary is already set
    // above. The HTTP layer (axum/hyper) strips the body on the wire
    // for HEAD; we return the full response and let that happen.
    if method == Method::HEAD {
        return Response::from_parts(parts, body);
    }

    // Codex round 2 P2 (refined in round 3 P2): already-encoded
    // passthrough. Mirrors the reference's
    // `compression/index.js:182-188`:
    //
    //     var encoding = res.getHeader('Content-Encoding') || 'identity'
    //     if (encoding !== 'identity') {
    //       debug('skip already encoded')
    //       return
    //     }
    //
    // Reference treats `Content-Encoding: identity` as "no encoding
    // applied; please compress normally"; it does NOT skip on the
    // mere presence of the header. Round 2 P2's initial fix used a
    // truthy check (`contains_key`) which incorrectly skipped the
    // identity case — Codex round 3 P2 surfaced the bug via a raw
    // smoke. Now the skip fires only when an existing
    // `Content-Encoding` value is something other than the literal
    // `identity` (case-sensitive match, mirroring JS strict
    // equality).
    //
    // Vary stays set (the check sits AFTER the Vary append above,
    // mirroring the reference's `vary()` call order at
    // `compression/index.js:174`).
    let existing_encoding = parts
        .headers
        .get(CONTENT_ENCODING)
        .and_then(|v| v.to_str().ok());
    if let Some(value) = existing_encoding {
        if value != "identity" {
            return Response::from_parts(parts, body);
        }
    }

    let bytes = match axum::body::to_bytes(body, usize::MAX).await {
        Ok(b) => b,
        Err(_) => {
            // Body read failed (e.g. stream error). Surface an empty
            // body — the response headers remain meaningful for
            // logging / proxying.
            return Response::from_parts(parts, Body::empty());
        }
    };

    if bytes.len() < DEFAULT_THRESHOLD {
        return Response::from_parts(parts, Body::from(bytes));
    }

    let Some(encoding) = negotiate(accept_encoding) else {
        return Response::from_parts(parts, Body::from(bytes));
    };

    let compressed = encode(&bytes, encoding);
    parts
        .headers
        .insert(CONTENT_ENCODING, HeaderValue::from_static(encoding.as_str()));
    if let Ok(len) = HeaderValue::from_str(&compressed.len().to_string()) {
        parts.headers.insert(CONTENT_LENGTH, len);
    }
    Response::from_parts(parts, Body::from(compressed))
}

/// Append `Accept-Encoding` to the response's `Vary` header,
/// mirroring `vary()` at `third_party/serve/node_modules/vary/index.js`
/// (invoked from `compression/index.js:174`). Semantics:
/// - No existing `Vary`: insert `Accept-Encoding`.
/// - Existing `Vary: *` (wildcard): leave alone — `*` already
///   covers everything.
/// - Existing `Vary` listing other field names: append
///   `, Accept-Encoding` IF `Accept-Encoding` is not already
///   present (case-insensitive, comma-separated).
fn append_vary_accept_encoding(headers: &mut HeaderMap) {
    let existing = headers.get(VARY).and_then(|v| v.to_str().ok());
    let next = match existing {
        None => "Accept-Encoding".to_string(),
        Some(value) => {
            // RFC 7231 §7.1.4: Vary `*` is a wildcard.
            if value.split(',').any(|t| t.trim() == "*") {
                return;
            }
            if value
                .split(',')
                .any(|t| t.trim().eq_ignore_ascii_case("Accept-Encoding"))
            {
                return;
            }
            format!("{value}, Accept-Encoding")
        }
    };
    if let Ok(v) = HeaderValue::from_str(&next) {
        headers.insert(VARY, v);
    }
}

fn cache_control_contains_no_transform(value: &str) -> bool {
    value.split(',').any(|token| {
        let token = token.trim();
        token.eq_ignore_ascii_case("no-transform")
    })
}

fn encode(bytes: &[u8], encoding: Encoding) -> Vec<u8> {
    match encoding {
        Encoding::Gzip => {
            let mut enc =
                flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
            enc.write_all(bytes).expect("gzip encoder write");
            enc.finish().expect("gzip encoder finish")
        }
        Encoding::Deflate => {
            let mut enc =
                flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
            enc.write_all(bytes).expect("deflate encoder write");
            enc.finish().expect("deflate encoder finish")
        }
        Encoding::Brotli => {
            let mut out = Vec::new();
            {
                let mut enc = brotli::CompressorWriter::new(&mut out, 4096, 4, 22);
                enc.write_all(bytes).expect("brotli encoder write");
            }
            out
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::HeaderName;

    fn hv(s: &str) -> HeaderValue {
        HeaderValue::from_str(s).unwrap()
    }

    #[test]
    fn negotiate_prefers_brotli() {
        assert_eq!(
            negotiate(Some(&hv("gzip, deflate, br"))),
            Some(Encoding::Brotli)
        );
    }

    #[test]
    fn negotiate_gzip_when_only_gzip() {
        assert_eq!(negotiate(Some(&hv("gzip"))), Some(Encoding::Gzip));
    }

    #[test]
    fn negotiate_deflate_when_only_deflate() {
        assert_eq!(negotiate(Some(&hv("deflate"))), Some(Encoding::Deflate));
    }

    #[test]
    fn negotiate_identity_only_returns_none() {
        assert_eq!(negotiate(Some(&hv("identity"))), None);
    }

    #[test]
    fn negotiate_absent_header_returns_none() {
        assert_eq!(negotiate(None), None);
    }

    #[test]
    fn negotiate_q0_excludes() {
        // gzip;q=0, br → must pick br
        assert_eq!(
            negotiate(Some(&hv("gzip;q=0, br"))),
            Some(Encoding::Brotli)
        );
    }

    #[test]
    fn negotiate_star_q0_rejects_all() {
        assert_eq!(negotiate(Some(&hv("*;q=0"))), None);
    }

    #[test]
    fn negotiate_wildcard_picks_brotli() {
        assert_eq!(negotiate(Some(&hv("*"))), Some(Encoding::Brotli));
    }

    #[test]
    fn negotiate_strips_whitespace() {
        assert_eq!(
            negotiate(Some(&hv("  br , gzip "))),
            Some(Encoding::Brotli)
        );
    }

    #[test]
    fn is_compressible_text_html() {
        assert!(is_compressible("text/html"));
    }

    #[test]
    fn is_compressible_text_html_with_charset() {
        assert!(is_compressible("text/html; charset=utf-8"));
    }

    #[test]
    fn is_compressible_application_json() {
        assert!(is_compressible("application/json"));
    }

    #[test]
    fn is_compressible_application_wasm() {
        assert!(is_compressible("application/wasm"));
    }

    #[test]
    fn is_compressible_image_svg_xml() {
        assert!(is_compressible("image/svg+xml"));
    }

    #[test]
    fn is_compressible_ld_plus_json_via_regex() {
        assert!(is_compressible("application/ld+json"));
    }

    #[test]
    fn is_compressible_atom_plus_xml_via_regex() {
        assert!(is_compressible("application/atom+xml"));
    }

    #[test]
    fn not_compressible_image_png() {
        assert!(!is_compressible("image/png"));
    }

    #[test]
    fn not_compressible_video_mp4() {
        assert!(!is_compressible("video/mp4"));
    }

    #[test]
    fn not_compressible_font_woff2() {
        assert!(!is_compressible("font/woff2"));
    }

    #[test]
    fn not_compressible_empty() {
        assert!(!is_compressible(""));
    }

    fn ok_response(content_type: &str, body: Vec<u8>) -> Response<Body> {
        Response::builder()
            .status(200)
            .header(CONTENT_TYPE, content_type)
            .header(CONTENT_LENGTH, body.len().to_string())
            .body(Body::from(body))
            .unwrap()
    }

    fn ae(value: &str) -> HeaderValue {
        hv(value)
    }

    #[tokio::test]
    async fn maybe_apply_disabled_via_flag_returns_unchanged() {
        let bytes = vec![b'a'; 4096];
        let resp = ok_response("text/html", bytes);
        let cfg = ServeConfig {
            compression: Some(false),
            ..Default::default()
        };
        let ae = ae("br");
        let out = maybe_apply(resp, &Method::GET, Some(&ae), &cfg).await;
        assert!(out.headers().get(VARY).is_none());
        assert!(out.headers().get(CONTENT_ENCODING).is_none());
    }

    #[tokio::test]
    async fn maybe_apply_non_compressible_mime_skips_vary() {
        let bytes = vec![b'a'; 4096];
        let resp = ok_response("image/png", bytes);
        let ae = ae("br, gzip, deflate");
        let out = maybe_apply(resp, &Method::GET, Some(&ae), &ServeConfig::default()).await;
        assert!(out.headers().get(VARY).is_none());
        assert!(out.headers().get(CONTENT_ENCODING).is_none());
    }

    #[tokio::test]
    async fn maybe_apply_below_threshold_sets_vary_only() {
        let bytes = vec![b'a'; 100];
        let resp = ok_response("text/html", bytes);
        let ae = ae("br, gzip");
        let out = maybe_apply(resp, &Method::GET, Some(&ae), &ServeConfig::default()).await;
        assert_eq!(
            out.headers().get(VARY).and_then(|v| v.to_str().ok()),
            Some("Accept-Encoding")
        );
        assert!(out.headers().get(CONTENT_ENCODING).is_none());
    }

    #[tokio::test]
    async fn maybe_apply_no_transform_skips_vary() {
        let bytes = vec![b'a'; 4096];
        let len = bytes.len();
        let resp = Response::builder()
            .status(200)
            .header(CONTENT_TYPE, "text/html")
            .header(CACHE_CONTROL, "no-transform")
            .header(CONTENT_LENGTH, len.to_string())
            .body(Body::from(bytes))
            .unwrap();
        let ae = ae("br");
        let out = maybe_apply(resp, &Method::GET, Some(&ae), &ServeConfig::default()).await;
        assert!(out.headers().get(VARY).is_none());
        assert!(out.headers().get(CONTENT_ENCODING).is_none());
    }

    #[tokio::test]
    async fn maybe_apply_head_keeps_vary_skips_encoding() {
        let bytes = vec![b'a'; 4096];
        let resp = ok_response("text/html", bytes);
        let ae = ae("br");
        let out = maybe_apply(resp, &Method::HEAD, Some(&ae), &ServeConfig::default()).await;
        assert_eq!(
            out.headers().get(VARY).and_then(|v| v.to_str().ok()),
            Some("Accept-Encoding")
        );
        assert!(out.headers().get(CONTENT_ENCODING).is_none());
        // Content-Length stays at original body size — middleware did not
        // consume the body on the HEAD path.
        assert_eq!(
            out.headers()
                .get(CONTENT_LENGTH)
                .and_then(|v| v.to_str().ok()),
            Some("4096")
        );
    }

    #[tokio::test]
    async fn maybe_apply_compresses_brotli_when_offered() {
        let bytes = vec![b'a'; 4096];
        let resp = ok_response("text/html", bytes);
        let ae = ae("gzip, deflate, br");
        let out = maybe_apply(resp, &Method::GET, Some(&ae), &ServeConfig::default()).await;
        assert_eq!(
            out.headers().get(VARY).and_then(|v| v.to_str().ok()),
            Some("Accept-Encoding")
        );
        assert_eq!(
            out.headers()
                .get(CONTENT_ENCODING)
                .and_then(|v| v.to_str().ok()),
            Some("br")
        );
    }

    #[tokio::test]
    async fn maybe_apply_falls_back_to_gzip_when_brotli_q0() {
        let bytes = vec![b'a'; 4096];
        let resp = ok_response("text/html", bytes);
        let ae = ae("br;q=0, gzip");
        let out = maybe_apply(resp, &Method::GET, Some(&ae), &ServeConfig::default()).await;
        assert_eq!(
            out.headers()
                .get(CONTENT_ENCODING)
                .and_then(|v| v.to_str().ok()),
            Some("gzip")
        );
    }

    #[tokio::test]
    async fn maybe_apply_identity_only_skips_compression_keeps_vary() {
        let bytes = vec![b'a'; 4096];
        let resp = ok_response("text/html", bytes);
        let ae = ae("identity");
        let out = maybe_apply(resp, &Method::GET, Some(&ae), &ServeConfig::default()).await;
        assert_eq!(
            out.headers().get(VARY).and_then(|v| v.to_str().ok()),
            Some("Accept-Encoding")
        );
        assert!(out.headers().get(CONTENT_ENCODING).is_none());
    }

    #[tokio::test]
    async fn maybe_apply_appends_to_existing_vary() {
        // Codex round 1 P2: when a user `headers` rule (or any prior
        // pass) already set Vary, the compression pass MUST append
        // `Accept-Encoding`, not overwrite, not skip. A response with
        // `Vary: Cookie` + `Content-Encoding: br` and no
        // `Accept-Encoding` in the Vary list is cache-incorrect —
        // downstream caches would key only on Cookie and serve the
        // brotli body to clients that didn't accept brotli.
        let bytes = vec![b'a'; 4096];
        let mut resp = ok_response("text/html", bytes);
        resp.headers_mut().insert(
            HeaderName::from_static("vary"),
            HeaderValue::from_static("Cookie"),
        );
        let ae = ae("br");
        let out = maybe_apply(resp, &Method::GET, Some(&ae), &ServeConfig::default()).await;
        assert_eq!(
            out.headers().get(VARY).and_then(|v| v.to_str().ok()),
            Some("Cookie, Accept-Encoding")
        );
        assert_eq!(
            out.headers()
                .get(CONTENT_ENCODING)
                .and_then(|v| v.to_str().ok()),
            Some("br")
        );
    }

    #[tokio::test]
    async fn maybe_apply_existing_vary_with_accept_encoding_is_not_duplicated() {
        let bytes = vec![b'a'; 4096];
        let mut resp = ok_response("text/html", bytes);
        resp.headers_mut().insert(
            HeaderName::from_static("vary"),
            HeaderValue::from_static("accept-encoding, cookie"),
        );
        let ae = ae("br");
        let out = maybe_apply(resp, &Method::GET, Some(&ae), &ServeConfig::default()).await;
        // No second `Accept-Encoding` appended (case-insensitive dedup).
        assert_eq!(
            out.headers().get(VARY).and_then(|v| v.to_str().ok()),
            Some("accept-encoding, cookie")
        );
    }

    #[tokio::test]
    async fn maybe_apply_existing_vary_star_is_left_alone() {
        // Vary: * is a wildcard that already covers everything.
        let bytes = vec![b'a'; 4096];
        let mut resp = ok_response("text/html", bytes);
        resp.headers_mut().insert(
            HeaderName::from_static("vary"),
            HeaderValue::from_static("*"),
        );
        let ae = ae("br");
        let out = maybe_apply(resp, &Method::GET, Some(&ae), &ServeConfig::default()).await;
        assert_eq!(
            out.headers().get(VARY).and_then(|v| v.to_str().ok()),
            Some("*")
        );
    }

    #[tokio::test]
    async fn maybe_apply_existing_non_identity_content_encoding_passthrough() {
        // Codex round 2 P2 + round 3 P2: a user `headers` rule that
        // set `Content-Encoding` to a NON-identity value upstream of
        // the compression pass MUST be honored — the reference's
        // `compression@1.8.1` middleware at
        // `compression/index.js:182-188` reads
        // `encoding = res.getHeader('Content-Encoding') || 'identity'`
        // and skips when `encoding !== 'identity'`. Vary is still
        // set — the reference's `vary()` call runs before the
        // already-encoded check.
        let bytes = vec![b'a'; 4096];
        let mut resp = ok_response("text/html", bytes);
        resp.headers_mut()
            .insert(CONTENT_ENCODING, HeaderValue::from_static("br"));
        let ae = ae("gzip, deflate, br");
        let out = maybe_apply(resp, &Method::GET, Some(&ae), &ServeConfig::default()).await;
        assert_eq!(
            out.headers().get(VARY).and_then(|v| v.to_str().ok()),
            Some("Accept-Encoding")
        );
        // The user-set Content-Encoding is preserved verbatim.
        assert_eq!(
            out.headers()
                .get(CONTENT_ENCODING)
                .and_then(|v| v.to_str().ok()),
            Some("br")
        );
        // Content-Length stays at the original body size — we never
        // re-encoded.
        assert_eq!(
            out.headers()
                .get(CONTENT_LENGTH)
                .and_then(|v| v.to_str().ok()),
            Some("4096")
        );
    }

    #[tokio::test]
    async fn maybe_apply_existing_identity_falls_through_and_compresses() {
        // Codex round 3 P2: `Content-Encoding: identity` is NOT the
        // already-encoded sentinel — the reference treats it as "no
        // encoding applied; compress normally". A user `headers`
        // rule that set `identity` upstream MUST be ignored by the
        // passthrough gate and the response MUST be encoded using
        // the negotiated encoder, with the user's `identity` header
        // overwritten by the chosen encoding's token.
        let bytes = vec![b'a'; 4096];
        let mut resp = ok_response("text/html", bytes);
        resp.headers_mut()
            .insert(CONTENT_ENCODING, HeaderValue::from_static("identity"));
        let ae = ae("gzip, deflate, br");
        let out = maybe_apply(resp, &Method::GET, Some(&ae), &ServeConfig::default()).await;
        assert_eq!(
            out.headers().get(VARY).and_then(|v| v.to_str().ok()),
            Some("Accept-Encoding")
        );
        assert_eq!(
            out.headers()
                .get(CONTENT_ENCODING)
                .and_then(|v| v.to_str().ok()),
            Some("br")
        );
        // Content-Length is overwritten with the compressed-body
        // length (4096 raw bytes of `a` compresses well — the
        // compressed size is much smaller).
        let compressed_len = out
            .headers()
            .get(CONTENT_LENGTH)
            .and_then(|v| v.to_str().ok())
            .and_then(|s| s.parse::<usize>().ok())
            .unwrap();
        assert!(
            compressed_len < 4096,
            "expected compressed length < 4096, got {compressed_len}"
        );
    }

    #[tokio::test]
    async fn maybe_apply_partial_content_keeps_vary_skips_encoding() {
        // SRV-CACHE-004 / Stage 7c composition: Range pre-empts
        // compression. The 206 body is already a sliced range and
        // `Content-Range` addresses the ORIGINAL bytes — compressing
        // the slice would invalidate it. But `Vary` is STILL set,
        // mirroring the reference's `range_big_html_gzip` anchor in
        // `compression-raw.json`: the negotiation hook engaged on a
        // compressible MIME, so the response varies by Accept-Encoding
        // even though no encoder ran.
        let bytes = vec![b'a'; 4096];
        let len = bytes.len();
        let resp = Response::builder()
            .status(StatusCode::PARTIAL_CONTENT)
            .header(CONTENT_TYPE, "text/html")
            .header(CONTENT_LENGTH, len.to_string())
            .header("content-range", "bytes 0-15/4096")
            .body(Body::from(bytes))
            .unwrap();
        let ae = ae("br");
        let out = maybe_apply(resp, &Method::GET, Some(&ae), &ServeConfig::default()).await;
        assert_eq!(
            out.headers().get(VARY).and_then(|v| v.to_str().ok()),
            Some("Accept-Encoding")
        );
        assert!(out.headers().get(CONTENT_ENCODING).is_none());
    }
}
