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

/// Mirror JS `parseInt(s, 10)` closely enough for `range-parser`'s
/// purposes: leading whitespace is skipped, an optional `+` or `-`
/// sign is accepted, then consecutive ASCII digits are consumed
/// and the remainder is ignored. Returns `None` (≡ JS `NaN`) when
/// no digits are found after the optional sign.
///
/// Overflow: JS numbers are f64 with effectively unbounded integer
/// magnitude for `range-parser`'s clipping path (`end > size - 1`
/// is always true for large values, so the value is overwritten
/// before any arithmetic). We saturate at `i64::MAX` / `i64::MIN`
/// to preserve the comparison outcome — a saturated end > `size - 1`
/// still clips to `size - 1`; a saturated negative start still
/// trips the `start < 0` invalidation. See
/// `parse_range::tests::parse_overflow_end_is_clipped`.
///
/// Reference site: `range-parser/index.js:44` calls
/// `parseInt(range[0], 10)` and `parseInt(range[1], 10)`. Codex
/// round 2 P2 enumerated five inputs where my prior digit-prefix
/// helper diverged from JS `parseInt`: `+0-3`, `0-+3`, `-+3`,
/// `0-184467440737095516160` (overflow), and `0--3` (interacts
/// with the `split('-')` shape — see `parse_range`).
fn parse_int_js(s: &str) -> Option<i64> {
    let s = s.trim_start();
    let bytes = s.as_bytes();
    let mut i = 0usize;
    let sign: i64 = if i < bytes.len() && (bytes[i] == b'+' || bytes[i] == b'-') {
        let was_minus = bytes[i] == b'-';
        i += 1;
        if was_minus {
            -1
        } else {
            1
        }
    } else {
        1
    };
    let digit_start = i;
    while i < bytes.len() && bytes[i].is_ascii_digit() {
        i += 1;
    }
    if i == digit_start {
        return None;
    }
    // Parse via u128 so we can detect overflow without panicking;
    // saturate to i64::MAX before applying the sign.
    let magnitude: u128 = std::str::from_utf8(&bytes[digit_start..i])
        .expect("ascii digits are utf-8")
        .parse()
        .unwrap_or(u128::MAX);
    let clamped = if magnitude > i64::MAX as u128 {
        i64::MAX
    } else {
        magnitude as i64
    };
    Some(sign.saturating_mul(clamped))
}

/// Parse a `Range` header value against a known representation size.
/// Mirrors `range-parser`'s control flow at
/// `third_party/serve/node_modules/.../range-parser/index.js:35-76`
/// over the single-range subset:
///
/// 1. `value` must start with `bytes=` (reference's `str.slice(0,
///    index)` becomes the `ranges.type`; non-`bytes` types are
///    invalid).
/// 2. Split the remainder on `,`, take only the first segment
///    (mirrors caller indexing `range[0]` at `index.js:724`).
/// 3. Split the segment on **every** `-` (JS `String.split('-')`).
///    Take `[0]` as `start`, `[1]` as `end`; any further elements
///    are ignored. **This is why `bytes=0--3` resolves to `bytes
///    0-10/11`**: `"0--3".split('-')` yields `["0", "", "3"]`, so
///    `end` is the empty string between the two hyphens, NOT
///    `"-3"`.
/// 4. `parseInt` both. If `start` is NaN, switch to suffix form
///    (`start = size - end; end = size - 1`). If `start` is a
///    number but `end` is NaN, switch to open-end (`end = size - 1`).
/// 5. Clip `end` to `size - 1`.
/// 6. Invalidate (continue / Unsatisfiable) if `start` or `end` is
///    still NaN after step 4, `start > end`, or `start < 0`.
pub fn parse_range(value: &str, total: u64) -> RangeOutcome {
    let rest = match value.strip_prefix("bytes=") {
        Some(r) => r,
        None => return RangeOutcome::Unsatisfiable,
    };
    let first = rest.split(',').next().unwrap_or("").trim();
    // JS `String.split('-')` splits on every hyphen and we only
    // consult positions 0 and 1, ignoring any further elements.
    let mut parts = first.split('-');
    let raw_start = parts.next().unwrap_or("");
    let raw_end = parts.next();

    let start_parsed = parse_int_js(raw_start);
    let end_parsed = raw_end.and_then(parse_int_js);

    let total_i = total as i64;
    let (start, mut end): (i64, i64) = match (start_parsed, end_parsed) {
        (None, Some(e)) => {
            // -nnn (suffix). Reference: `start = size - end; end =
            // size - 1`. `bytes=-3` on an 11-byte file → start=8,
            // end=10. `bytes=-999` → start=-988, then start<0 →
            // Unsatisfiable (NOT "full representation" — earlier
            // versions of this spec mis-cited RFC 7233 §2.1; the
            // reference's range-parser does not honor that RFC
            // suggestion).
            (total_i.saturating_sub(e), total_i.saturating_sub(1))
        }
        (Some(s), None) => {
            // nnn- (open-end). `bytes=8-` → start=8, end=size-1.
            (s, total_i.saturating_sub(1))
        }
        (Some(s), Some(e)) => (s, e),
        (None, None) => {
            // Both NaN. Reference falls into the `isNaN(start)`
            // branch, computes `start = size - end = NaN`, then
            // the `isNaN(start) || isNaN(end)` validation catches
            // it. Same outcome.
            return RangeOutcome::Unsatisfiable;
        }
    };

    // Step 5: clip end to size - 1. Reference: `if (end > size - 1)
    // { end = size - 1 }`. Done unconditionally — works for the
    // overflow case (`bytes=0-184467440737095516160` clamps end to
    // size-1) AND the partial-overlap case (`bytes=8-999` on
    // 11-byte file → end=10).
    let total_minus_1 = total_i.saturating_sub(1);
    if end > total_minus_1 {
        end = total_minus_1;
    }

    // Step 6: invalidate. Reference: `start > end || start < 0`.
    // Empty file (total==0): total_minus_1 = -1, so any non-negative
    // start fails the start > end check → Unsatisfiable. The
    // dispatch-level guard in `build_file_or_304` intercepts the
    // empty-file case before `range::apply` runs, but `parse_range`
    // staying internally consistent for total==0 keeps the helper
    // safe to call in isolation. Also covers the negative-zero
    // suffix corner (`bytes=-0` → start=size, end=size-1 →
    // start > end → Unsatisfiable).
    if start > end || start < 0 || end < 0 {
        return RangeOutcome::Unsatisfiable;
    }

    RangeOutcome::InRange {
        start: start as u64,
        end: end as u64,
    }
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
    // Codex round 1 P2: a user `serve.json#headers` rule that sets
    // `Content-Range` MUST win over the default `bytes */<total>` —
    // mirrors reference's ordering at
    // `serve-handler/src/index.js:730-732, 746, 767`:
    // `response.setHeader('Content-Range', 'bytes */N')` runs BEFORE
    // `getHeaders` returns the merged user-rule headers, and the
    // subsequent `response.writeHead(statusCode, headers)` passes the
    // merged map which overrides prior `setHeader` values for the
    // same name. In irserve, `apply_custom_headers` has already merged
    // the user headers into `merged` by the time we run; we therefore
    // use `entry().or_insert(...)` so a present user value wins and
    // the default `bytes */<total>` fills in only when absent.
    parts
        .headers
        .entry(CONTENT_RANGE)
        .or_insert_with(|| {
            HeaderValue::from_str(&format!("bytes */{total}"))
                .expect("content-range value is ascii")
        });
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
    fn parse_suffix_larger_than_total_is_unsatisfiable() {
        // Codex round 2 P2 reframing: RFC 7233 §2.1 suggests "If the
        // selected representation is shorter than the specified
        // suffix-length, the entire representation is used", but the
        // reference's `range-parser` does NOT honor that — it
        // computes `start = size - end = 11 - 999 = -988` and the
        // `start < 0` validation at `range-parser/index.js:62`
        // rejects it. Reference returns 416 for `bytes=-999` on an
        // 11-byte file. irserve mirrors.
        assert_eq!(parse_range("bytes=-999", TOTAL), RangeOutcome::Unsatisfiable);
    }

    #[test]
    fn parse_empty_file_is_always_unsatisfiable() {
        assert_eq!(parse_range("bytes=0-0", 0), RangeOutcome::Unsatisfiable);
        assert_eq!(parse_range("bytes=-1", 0), RangeOutcome::Unsatisfiable);
        assert_eq!(parse_range("bytes=0-", 0), RangeOutcome::Unsatisfiable);
    }

    #[test]
    fn parse_trailing_garbage_in_end_uses_leading_digits() {
        // Codex round 1 P2: reference's `range-parser` uses JS
        // `parseInt` which accepts `"3x"` as 3. Mirror.
        assert_eq!(
            parse_range("bytes=0-3x", TOTAL),
            RangeOutcome::InRange { start: 0, end: 3 },
        );
    }

    #[test]
    fn parse_trailing_garbage_in_start_uses_leading_digits() {
        // Same lenient parse for the start position.
        assert_eq!(
            parse_range("bytes=8abc-", TOTAL),
            RangeOutcome::InRange { start: 8, end: 10 },
        );
    }

    #[test]
    fn parse_trailing_garbage_in_suffix_uses_leading_digits() {
        // Suffix form also lenient. `bytes=-3x` → last 3 bytes.
        assert_eq!(
            parse_range("bytes=-3x", TOTAL),
            RangeOutcome::InRange { start: 8, end: 10 },
        );
    }

    #[test]
    fn parse_alpha_start_is_treated_as_suffix() {
        // Codex round 2 P2: when `parseInt(start)` is NaN, the
        // reference's `if (isNaN(start))` branch at
        // `range-parser/index.js:48-50` REPURPOSES the parse as a
        // suffix request: `start = size - end; end = size - 1`. So
        // `bytes=abc-3` on 11-byte file → start=8, end=10 → 206.
        // The Round 1 helper rejected this as Unsatisfiable, which
        // was a wire-observable divergence.
        assert_eq!(
            parse_range("bytes=abc-3", TOTAL),
            RangeOutcome::InRange { start: 8, end: 10 },
        );
    }

    #[test]
    fn parse_sign_prefix_in_start() {
        // parseInt('+0', 10) = 0. Codex round 2 P2 smoke #1.
        assert_eq!(
            parse_range("bytes=+0-3", TOTAL),
            RangeOutcome::InRange { start: 0, end: 3 },
        );
    }

    #[test]
    fn parse_sign_prefix_in_end() {
        // parseInt('+3', 10) = 3. Codex round 2 P2 smoke #2.
        assert_eq!(
            parse_range("bytes=0-+3", TOTAL),
            RangeOutcome::InRange { start: 0, end: 3 },
        );
    }

    #[test]
    fn parse_sign_prefix_in_suffix() {
        // `bytes=-+3`: split('-') = ["", "+3"]. start=NaN, end=3 →
        // suffix branch: start = 11 - 3 = 8, end = 10. Codex round
        // 2 P2 smoke #3.
        assert_eq!(
            parse_range("bytes=-+3", TOTAL),
            RangeOutcome::InRange { start: 8, end: 10 },
        );
    }

    #[test]
    fn parse_overflow_end_is_clipped() {
        // parseInt('184...20', 10) returns a JS Number well beyond
        // i64::MAX. Our parse_int_js saturates to i64::MAX, then the
        // `if end > size - 1` clip pulls it back to total - 1. Result:
        // start=0, end=total-1. Codex round 2 P2 smoke #4.
        assert_eq!(
            parse_range("bytes=0-184467440737095516160", TOTAL),
            RangeOutcome::InRange { start: 0, end: 10 },
        );
    }

    #[test]
    fn parse_double_hyphen_in_middle_is_open_end() {
        // `bytes=0--3`: JS `"0--3".split('-')` = ["0", "", "3"].
        // Reference indexes [0]="0" and [1]="" → parseInt("0")=0,
        // parseInt("")=NaN → open-end branch → start=0, end=size-1.
        // The `"3"` segment is silently discarded. Codex round 2 P2
        // smoke #5. THIS IS THE KEY REASON `parse_range` uses
        // `split('-')` not `split_once`.
        assert_eq!(
            parse_range("bytes=0--3", TOTAL),
            RangeOutcome::InRange { start: 0, end: 10 },
        );
    }

    #[test]
    fn parse_three_hyphens_uses_first_two_segments_only() {
        // `bytes=0-3-x`: split('-') = ["0", "3", "x"]. Reference
        // takes [0] and [1]; the trailing "x" is ignored.
        assert_eq!(
            parse_range("bytes=0-3-x", TOTAL),
            RangeOutcome::InRange { start: 0, end: 3 },
        );
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
    async fn apply_416_user_content_range_rule_wins() {
        // Codex round 1 P2: reference sets `Content-Range: bytes */N`
        // via `setHeader` BEFORE `getHeaders` returns; the subsequent
        // `writeHead(statusCode, headers)` overrides prior setHeader
        // values for keys present in the merged map. A user
        // `serve.json#headers` rule for `Content-Range` therefore
        // wins on 416. irserve mirrors via `entry().or_insert(...)`.
        let resp = apply(
            merged_200_for(&[("content-range", "custom-value")]),
            &HeaderValue::from_static("bytes=999-1000"),
            FIXTURE,
            TOTAL,
        );
        assert_eq!(resp.status(), StatusCode::RANGE_NOT_SATISFIABLE);
        assert_eq!(resp.headers().get(CONTENT_RANGE).unwrap(), "custom-value");
    }

    #[tokio::test]
    async fn apply_416_default_content_range_when_no_user_rule() {
        // Regression: without a user rule, the default `bytes */<total>`
        // is still inserted via `or_insert_with`.
        let resp = apply(
            merged_200_for(&[]),
            &HeaderValue::from_static("bytes=999-1000"),
            FIXTURE,
            TOTAL,
        );
        assert_eq!(resp.status(), StatusCode::RANGE_NOT_SATISFIABLE);
        assert_eq!(resp.headers().get(CONTENT_RANGE).unwrap(), "bytes */11");
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
