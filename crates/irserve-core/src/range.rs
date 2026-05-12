//! SRV-CACHE-004: `Range` request handling — `206 Partial Content`
//! and `416 Range Not Satisfiable`.
//!
//! Mirrors `vercel/serve-handler`'s range branch at
//! `third_party/serve-handler/src/index.js:717-734` and `:749-752`,
//! which delegates parsing to the `range-parser` npm package. We
//! re-implement the subset we care about: single-range, `bytes`
//! unit, inclusive end, partial-overlap clipping, suffix form
//! (`bytes=-N`). Multiple ranges, `If-Range`, and the `Accept-Ranges`
//! response header are explicitly out of scope (see plan §"Out of
//! scope" entries 1, 2, 3).

use axum::body::Body;
use axum::http::header::{HeaderValue, CONTENT_LENGTH, CONTENT_RANGE};
use axum::http::{Response, StatusCode};

#[derive(Debug, PartialEq, Eq)]
pub enum RangeOutcome {
    /// Inclusive byte offsets — `start <= end < total`.
    InRange { start: u64, end: u64 },
    /// Any malformed input, wrong unit, or strictly-out-of-range
    /// request lands here. The caller emits 416.
    Unsatisfiable,
}

/// Parse a `Range` header value against a known representation size.
/// Any non-`bytes=...` input collapses to `Unsatisfiable` so the
/// caller can route both malformed and wrong-unit cases through the
/// same 416 branch — mirroring reference's `range[0].type !== 'bytes'`
/// and `range-parser`'s `-1` paths both landing at
/// `index.js:730-732`.
pub fn parse_range(value: &str, total: u64) -> RangeOutcome {
    let rest = match value.strip_prefix("bytes=") {
        Some(r) => r,
        None => return RangeOutcome::Unsatisfiable,
    };
    // Multi-range: take the first segment only. Reference's
    // `range-parser` returns all but the caller indexes `range[0]`
    // at `index.js:724` — we collapse here.
    let first = rest.split(',').next().unwrap_or("").trim();
    let (start_s, end_s) = match first.split_once('-') {
        Some(pair) => pair,
        None => return RangeOutcome::Unsatisfiable,
    };
    let start_s = start_s.trim();
    let end_s = end_s.trim();

    if start_s.is_empty() {
        // Suffix form: `bytes=-N` → last N bytes.
        let n: u64 = match end_s.parse() {
            Ok(n) if n > 0 => n,
            _ => return RangeOutcome::Unsatisfiable,
        };
        if total == 0 {
            return RangeOutcome::Unsatisfiable;
        }
        if n >= total {
            // RFC 7233 §2.1: "If the selected representation is
            // shorter than the specified suffix-length, the entire
            // representation is used."
            return RangeOutcome::InRange {
                start: 0,
                end: total - 1,
            };
        }
        return RangeOutcome::InRange {
            start: total - n,
            end: total - 1,
        };
    }

    let start: u64 = match start_s.parse() {
        Ok(n) => n,
        Err(_) => return RangeOutcome::Unsatisfiable,
    };
    if total == 0 || start >= total {
        return RangeOutcome::Unsatisfiable;
    }
    let end: u64 = if end_s.is_empty() {
        // `bytes=N-` → through end of representation.
        total - 1
    } else {
        match end_s.parse::<u64>() {
            Ok(n) => {
                if n < start {
                    return RangeOutcome::Unsatisfiable;
                }
                // Partial overlap — empirically pinned by the
                // `clip_to_end` probe anchor: `bytes=8-999` on an
                // 11-byte file resolves to `bytes 8-10/11`, NOT 416.
                n.min(total - 1)
            }
            Err(_) => return RangeOutcome::Unsatisfiable,
        }
    };
    RangeOutcome::InRange { start, end }
}

/// Mutate the merged 200 response into either 206 (partial body) or
/// 416 (full body + `Content-Range: bytes */<total>`) per the parsed
/// `Range` header. Other headers (Content-Type, ETag, Last-Modified,
/// user-rule custom headers) carry over from `merged` unchanged.
/// `Content-Range` and `Content-Length` are last-write-wins over any
/// user rule — mirrors reference's post-`getHeaders` injection at
/// `serve-handler/src/index.js:749-752`.
pub fn apply(
    merged: Response<Body>,
    range_header: &HeaderValue,
    bytes: &[u8],
    total: u64,
) -> Response<Body> {
    let value = match range_header.to_str() {
        Ok(v) => v,
        Err(_) => return build_416(merged, bytes, total),
    };
    match parse_range(value, total) {
        RangeOutcome::InRange { start, end } => build_206(merged, bytes, start, end, total),
        RangeOutcome::Unsatisfiable => build_416(merged, bytes, total),
    }
}

fn build_206(
    merged: Response<Body>,
    bytes: &[u8],
    start: u64,
    end: u64,
    total: u64,
) -> Response<Body> {
    let (mut parts, _) = merged.into_parts();
    parts.status = StatusCode::PARTIAL_CONTENT;
    let slice = &bytes[start as usize..=end as usize];
    let len = slice.len() as u64;
    parts.headers.insert(
        CONTENT_RANGE,
        HeaderValue::from_str(&format!("bytes {start}-{end}/{total}"))
            .expect("content-range value is ascii"),
    );
    parts.headers.insert(
        CONTENT_LENGTH,
        HeaderValue::from_str(&len.to_string()).expect("content-length is ascii digits"),
    );
    Response::from_parts(parts, Body::from(slice.to_vec()))
}

fn build_416(merged: Response<Body>, bytes: &[u8], total: u64) -> Response<Body> {
    let (mut parts, _) = merged.into_parts();
    parts.status = StatusCode::RANGE_NOT_SATISFIABLE;
    parts.headers.insert(
        CONTENT_RANGE,
        HeaderValue::from_str(&format!("bytes */{total}"))
            .expect("content-range value is ascii"),
    );
    // 416 body is the full representation — mirrors reference at
    // `serve-handler/src/index.js:730-741` (statusCode = 416 inline,
    // then falls through to `stream.pipe(response)` with empty
    // `streamOpts` which sends the whole file). RFC 7233 §4.4
    // permits this; the integration test
    // `range request not satisfiable` (`test/integration.test.js:1203-1227`)
    // and the `out_of_range` snapshot anchor pin it.
    Response::from_parts(parts, Body::from(bytes.to_vec()))
}

#[cfg(test)]
mod tests {
    use super::{apply, parse_range, RangeOutcome};
    use axum::body::Body;
    use axum::http::header::{HeaderValue, CONTENT_LENGTH, CONTENT_RANGE, CONTENT_TYPE, ETAG};
    use axum::http::{Response, StatusCode};

    // The 11-byte fixture matches the `range-request.json` probe so
    // expected offsets stay in lockstep with the reference snapshot.
    const TOTAL: u64 = 11;
    const FIXTURE: &[u8] = b"abcdefghij\n";

    fn merged_200_for(custom_headers: &[(&str, &str)]) -> Response<Body> {
        let mut builder = Response::builder()
            .status(StatusCode::OK)
            .header(CONTENT_TYPE, "text/plain; charset=utf-8")
            .header(ETAG, "\"df13ea7b971f3dad54f0deb102c5bc9fa0d052ae\"");
        for (k, v) in custom_headers {
            builder = builder.header(*k, *v);
        }
        builder
            .body(Body::from(FIXTURE.to_vec()))
            .expect("merged 200 should always build")
    }

    async fn collect_body(resp: Response<Body>) -> Vec<u8> {
        axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .expect("collect body")
            .to_vec()
    }

    // ---- parser ----

    #[test]
    fn parse_in_range_first_4() {
        assert_eq!(
            parse_range("bytes=0-3", TOTAL),
            RangeOutcome::InRange { start: 0, end: 3 },
        );
    }

    #[test]
    fn parse_open_end() {
        assert_eq!(
            parse_range("bytes=8-", TOTAL),
            RangeOutcome::InRange { start: 8, end: 10 },
        );
    }

    #[test]
    fn parse_suffix() {
        assert_eq!(
            parse_range("bytes=-3", TOTAL),
            RangeOutcome::InRange { start: 8, end: 10 },
        );
    }

    #[test]
    fn parse_single_byte() {
        assert_eq!(
            parse_range("bytes=0-0", TOTAL),
            RangeOutcome::InRange { start: 0, end: 0 },
        );
    }

    #[test]
    fn parse_partial_overlap_clips_to_end() {
        // bytes=8-999 → InRange{8, total-1}. Empirically pinned by
        // `range-request.json#clip_to_end` against the reference.
        assert_eq!(
            parse_range("bytes=8-999", TOTAL),
            RangeOutcome::InRange { start: 8, end: 10 },
        );
    }

    #[test]
    fn parse_fully_out_of_range_is_unsatisfiable() {
        assert_eq!(
            parse_range("bytes=999-1000", TOTAL),
            RangeOutcome::Unsatisfiable,
        );
    }

    #[test]
    fn parse_start_after_end_is_unsatisfiable() {
        assert_eq!(parse_range("bytes=10-1", TOTAL), RangeOutcome::Unsatisfiable);
    }

    #[test]
    fn parse_non_numeric_is_unsatisfiable() {
        assert_eq!(parse_range("bytes=abc", TOTAL), RangeOutcome::Unsatisfiable);
    }

    #[test]
    fn parse_empty_after_eq_is_unsatisfiable() {
        assert_eq!(parse_range("bytes=", TOTAL), RangeOutcome::Unsatisfiable);
    }

    #[test]
    fn parse_multi_range_uses_first_only() {
        assert_eq!(
            parse_range("bytes=0-3, 8-10", TOTAL),
            RangeOutcome::InRange { start: 0, end: 3 },
        );
    }

    #[test]
    fn parse_wrong_unit_is_unsatisfiable() {
        // Reference's `range-parser` returns -1 for non-bytes;
        // mirrors to the same 416 branch as malformed bytes.
        assert_eq!(
            parse_range("pixels=0-3", TOTAL),
            RangeOutcome::Unsatisfiable,
        );
    }

    #[test]
    fn parse_negative_zero_suffix_is_unsatisfiable() {
        // Plan §Out-of-scope #9: `range-parser` returns Unsatisfiable
        // for `bytes=-0`; we mirror.
        assert_eq!(parse_range("bytes=-0", TOTAL), RangeOutcome::Unsatisfiable);
    }

    #[test]
    fn parse_suffix_larger_than_total_returns_full() {
        // RFC 7233 §2.1: "If the selected representation is shorter
        // than the specified suffix-length, the entire representation
        // is used."
        assert_eq!(
            parse_range("bytes=-999", TOTAL),
            RangeOutcome::InRange { start: 0, end: 10 },
        );
    }

    #[test]
    fn parse_empty_file_is_always_unsatisfiable() {
        assert_eq!(parse_range("bytes=0-0", 0), RangeOutcome::Unsatisfiable);
        assert_eq!(parse_range("bytes=-1", 0), RangeOutcome::Unsatisfiable);
        assert_eq!(parse_range("bytes=0-", 0), RangeOutcome::Unsatisfiable);
    }

    // ---- apply ----

    #[tokio::test]
    async fn apply_in_range_returns_206_with_partial_body() {
        let resp = apply(
            merged_200_for(&[]),
            &HeaderValue::from_static("bytes=0-3"),
            FIXTURE,
            TOTAL,
        );
        assert_eq!(resp.status(), StatusCode::PARTIAL_CONTENT);
        assert_eq!(
            resp.headers().get(CONTENT_RANGE).unwrap(),
            "bytes 0-3/11",
        );
        assert_eq!(resp.headers().get(CONTENT_LENGTH).unwrap(), "4");
        let body = collect_body(resp).await;
        assert_eq!(body, b"abcd");
    }

    #[tokio::test]
    async fn apply_out_of_range_returns_416_with_full_body() {
        let resp = apply(
            merged_200_for(&[]),
            &HeaderValue::from_static("bytes=999-1000"),
            FIXTURE,
            TOTAL,
        );
        assert_eq!(resp.status(), StatusCode::RANGE_NOT_SATISFIABLE);
        assert_eq!(resp.headers().get(CONTENT_RANGE).unwrap(), "bytes */11");
        let body = collect_body(resp).await;
        assert_eq!(body, FIXTURE);
    }

    #[tokio::test]
    async fn apply_start_after_end_returns_416() {
        let resp = apply(
            merged_200_for(&[]),
            &HeaderValue::from_static("bytes=10-1"),
            FIXTURE,
            TOTAL,
        );
        assert_eq!(resp.status(), StatusCode::RANGE_NOT_SATISFIABLE);
        assert_eq!(resp.headers().get(CONTENT_RANGE).unwrap(), "bytes */11");
    }

    #[tokio::test]
    async fn apply_preserves_content_type_and_etag_on_206() {
        let resp = apply(
            merged_200_for(&[]),
            &HeaderValue::from_static("bytes=0-3"),
            FIXTURE,
            TOTAL,
        );
        assert_eq!(
            resp.headers().get(CONTENT_TYPE).unwrap(),
            "text/plain; charset=utf-8",
        );
        assert_eq!(
            resp.headers().get(ETAG).unwrap(),
            "\"df13ea7b971f3dad54f0deb102c5bc9fa0d052ae\"",
        );
    }

    #[tokio::test]
    async fn apply_preserves_user_custom_headers_on_206() {
        let resp = apply(
            merged_200_for(&[("x-custom", "yes")]),
            &HeaderValue::from_static("bytes=0-3"),
            FIXTURE,
            TOTAL,
        );
        assert_eq!(resp.headers().get("x-custom").unwrap(), "yes");
    }

    #[tokio::test]
    async fn apply_overwrites_user_content_length_on_206() {
        // User rule supplies a stale Content-Length; the 206 branch
        // must win (mirrors reference's post-`getHeaders` injection
        // at `serve-handler/src/index.js:751`).
        let resp = apply(
            merged_200_for(&[("content-length", "999")]),
            &HeaderValue::from_static("bytes=0-3"),
            FIXTURE,
            TOTAL,
        );
        assert_eq!(resp.headers().get(CONTENT_LENGTH).unwrap(), "4");
    }

    #[tokio::test]
    async fn apply_suffix_returns_last_n_bytes() {
        let resp = apply(
            merged_200_for(&[]),
            &HeaderValue::from_static("bytes=-3"),
            FIXTURE,
            TOTAL,
        );
        assert_eq!(resp.status(), StatusCode::PARTIAL_CONTENT);
        assert_eq!(resp.headers().get(CONTENT_RANGE).unwrap(), "bytes 8-10/11");
        let body = collect_body(resp).await;
        assert_eq!(body, b"ij\n");
    }
}
