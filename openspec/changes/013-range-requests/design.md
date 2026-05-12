# Design: Range requests (`206 Partial Content` / `416 Range Not Satisfiable`)

This document records the architecture for Stage 7c. 7c is the
third L3 sub-stage and lands one SRV (SRV-CACHE-004 — already
`verified` against the reference side; Stage 7c reaches dual-
target verification). No new SRV is introduced; no `D-NNN`
entry is authored. The architectural foundations (crate
layout, HTTP-stack pins, request lifecycle, oracle harness
layer) live in
`openspec/changes/001-port-minimal-static-server/design.md`;
the Stage-7a foundations (`build_file_or_304`, the
merge-before-decide ordering, the probe-runner `$fromResponse`
capture-replay) live in
`openspec/changes/011-etag-conditional/design.md`; the Stage-7b
foundations (the `Range`-absent guard wrapping BOTH 304 paths)
live in `openspec/changes/012-last-modified/design.md` — all
three are reused verbatim. The contract for the extended
capability lives in `openspec/specs/http-cache/spec.md`; the
wire-level scenarios specific to 7c are in
`specs/http-cache/spec.md` of this change package. 7c does not
add a new phase to the 13-phase dispatcher; the Range
emission sits inside the same file-response slot (phase 12)
that 7a/7b already occupy.

## §1. The Range emission seam

The reference's source-of-truth is the block at
`third_party/serve-handler/src/index.js:717-741`, plus the
post-`getHeaders` header injection at `:749-752`:

```js
// L717-741: Range parsing + status setup
const streamOpts = {};

// TODO ? if-range
if (request.headers.range && stats.size) {
  const range = parseRange(stats.size, request.headers.range);
  if (typeof range === 'object' && range.type === 'bytes') {
    const {start, end} = range[0];
    streamOpts.start = start;
    streamOpts.end = end;
    response.statusCode = 206;
  } else {
    response.statusCode = 416;
    response.setHeader('Content-Range', `bytes */${stats.size}`);
  }
}

// TODO ? multiple ranges

let stream = null;
try {
  stream = await handlers.createReadStream(absolutePath, streamOpts);
} catch (err) { return internalError(...); }

const headers = await getHeaders(handlers, config, current, absolutePath, stats);

// L749-752: 206-only header injection AFTER getHeaders merge
if (streamOpts.start !== undefined && streamOpts.end !== undefined) {
  headers['Content-Range'] = `bytes ${streamOpts.start}-${streamOpts.end}/${stats.size}`;
  headers['Content-Length'] = streamOpts.end - streamOpts.start + 1;
}
```

Three structural observations that drive the irserve
implementation:

1. **Range emission runs AFTER `getHeaders`** (which merges
   user `customHeaders` via `Object.assign` at `:241`). The
   `Content-Range` and `Content-Length` injections at
   `:749-752` overwrite anything user rules supplied for the
   same keys → **range headers are last-write-wins over user
   rules**. irserve mirrors this by calling `range::apply`
   AFTER `apply_custom_headers` in `build_file_or_304`, and
   `apply`'s `parts.headers.insert(CONTENT_RANGE, ...)` /
   `parts.headers.insert(CONTENT_LENGTH, ...)` calls at
   `range.rs:133-141` replace any existing value.
   Empirically pinned by
   `range::tests::apply_overwrites_user_content_length_on_206`.

2. **416 carries the full file body** (recon point 1 in the
   plan). The reference sets `statusCode = 416` inline +
   `Content-Range: bytes */<size>`, then falls through to
   `stream.pipe(response)` with empty `streamOpts` so
   `createReadStream` reads the whole file. The integration
   test `range request not satisfiable`
   (`third_party/serve-handler/test/integration.test.js:1203-1227`)
   pins `expect(length).toBe(content.length)` AND
   `expect(text).toBe(spec)`. RFC 7233 §4.4 permits a
   representation on 416 ("The 416 response message is
   composed in the same manner as a 200 response"). irserve
   mirrors via `range::apply::build_416` at
   `range.rs:145-161`, which keeps the merged response's full
   body verbatim. **D1** (Mirror) captures the user-confirmed
   intent; no `D-NNN` since there is no intentional
   divergence from the reference.

3. **Range pre-empts the 304 short-circuit**, even with a
   matching `If-None-Match` or (under `--no-etag`) a matching
   `If-Modified-Since`. The reference's L760 guard
   (`request.headers.range == null && ...`) gates ONLY the
   ETag/INM 304 short-circuit (the reference has no IMS
   branch — see Stage 7b's D-018). irserve generalises the
   guard to wrap BOTH the ETag/INM and the (irserve-adapted)
   Last-Modified/IMS 304 branches — that generalisation
   landed in Stage 7b. Stage 7c does not touch the guard
   itself; it fills the previously-empty `else` arm with the
   `range::apply` call.

## §2. The `range` module

`crates/irserve-core/src/range.rs` (~160 lines + 21 unit
tests) exposes two symbols:

- `pub enum RangeOutcome { InRange { start: u64, end: u64 },
  Unsatisfiable }` — the parser result. `InRange` carries
  inclusive byte offsets (`start <= end < total`). Any
  malformed input, wrong unit (`pixels=0-3`), or strictly
  out-of-range request collapses to `Unsatisfiable` so the
  caller routes both malformed and wrong-unit cases through
  one 416 branch (mirrors reference's `range[0].type !==
  'bytes'` and `range-parser`'s `-1` paths both landing at
  `index.js:730-732`).

- `pub fn apply(merged: Response<Body>, range_header:
  &HeaderValue, bytes: &[u8], total: u64) -> Response<Body>` —
  takes the merged 200 response (post-`apply_custom_headers`)
  and mutates it into either 206 (slice the body to
  `bytes[start..=end]`, set status, inject `Content-Range` +
  `Content-Length`) or 416 (keep the merged body verbatim —
  it already contains the full file bytes — swap status,
  inject `Content-Range: bytes */<total>`). Other headers
  (Content-Type, ETag, Last-Modified, user-rule headers)
  carry over from `merged` unchanged. `range::apply` does
  NOT call `parse_range` directly — it first dispatches on
  `range_header.to_str()` (returns 416 on non-ASCII header
  values per RFC 7233 §2.1 syntax) and then on the parser
  outcome. Both paths converge on `build_416` for any
  outcome that's not `InRange`.

The parser grammar handled by `parse_range`:

- `bytes=<s>-<e>` — explicit pair. Both ends parse as `u64`.
  Partial-overlap clipping: if `e >= total`, `e` is silently
  reduced to `total - 1` (RFC 7233 §2.1 / `range-parser`
  semantics, empirically pinned by the `clip_to_end` probe
  anchor — see §3 below). Start-past-end (`bytes=10-1`) →
  `Unsatisfiable`. Start-past-EOF (`start >= total`) →
  `Unsatisfiable`.
- `bytes=<s>-` — open-ended. `end` becomes `total - 1`.
- `bytes=-<n>` — suffix form. Returns the last `n` bytes.
  RFC 7233 §2.1 says "If the selected representation is
  shorter than the specified suffix-length, the entire
  representation is used" — handled at `range.rs:57-65` by
  the `n >= total` branch. `bytes=-0` →
  `Unsatisfiable` (the `Ok(n) if n > 0` guard at `:50-52`;
  matches `range-parser`'s `-2` return for zero suffix
  length).
- Multi-range comma list — `rest.split(',').next()` peels
  the first segment, the rest is silently ignored (mirrors
  reference's `range[0]` at `index.js:724`; plan §"Out of
  scope" item 1).
- Empty file (`total == 0`) — every Range value is
  `Unsatisfiable`. Pinned by
  `range::tests::parse_empty_file_is_always_unsatisfiable`.

**Why not the `http-range` crate?** Plan-mode D2 chose a
manual parser. The reference's `range-parser` npm dependency
is ~80 LOC; our subset (single-range, `bytes` unit,
inclusive end, clip, suffix) is smaller. A crate dependency
would either pull a comprehensive multi-range parser we
don't use (e.g. `http-range = "0.1"` returns `Vec` of
ranges) or impose a different API shape. The manual parser
also localises the divergence ledger from `range-parser` to
one file with 21 unit tests pinning every grammar corner
(suffix, clip, multi-range first-only, `bytes=-0`,
wrong unit, empty file).

## §3. The dispatch-level seam: let-else + conditional bytes clone

The reshape at
`crates/irserve-core/src/dispatch.rs:759-833` turns the
previous `if req_headers.get(RANGE).is_none() { /* 304
branches */ } merged` shape into:

```rust
let total = bytes.len() as u64;
let range_value = req_headers.get(RANGE).cloned();
let bytes_for_range = range_value.as_ref().map(|_| bytes.clone());

let etag = etag_value(serve_config, path, &bytes);
let last_modified = last_modified_value(serve_config, meta);
let response_200 = file_response(path, bytes, etag, last_modified);
let merged = apply_custom_headers(response_200, request_path, header_rules);

let Some(range_value) = range_value else {
    // No Range — try 304 short-circuits, else fall through to 200.
    // ... ETag/INM branch ... Last-Modified/IMS branch ...
    return merged;
};

let bytes_for_range = bytes_for_range.expect("bytes cloned when range present");
range::apply(merged, &range_value, &bytes_for_range, total)
```

Three design choices land here:

**(a) Conditional clone of `bytes`.** `file_response` takes
`bytes: Vec<u8>` by value (it moves the vec into
`Body::from(bytes)`), so `range::apply` cannot reach the
original bytes through `merged` — axum's `Body` is opaque to
sync access. Three options were considered in plan-mode:
clone-always before `file_response` (simple, +1 alloc per
file response even without Range); clone-conditionally when
Range is present (skips the alloc on the common no-Range
path, costs a branch); refactor `file_response` to take
`&[u8]` and clone internally (pushes the alloc into
`file_response` and complicates its API). **Clone-
conditionally was chosen** — Range requests are rare in
typical static-serve workloads, and skipping the extra
allocation on the hot path is worth the single boolean
branch. The clone lands at
`crates/irserve-core/src/dispatch.rs:776` via
`range_value.as_ref().map(|_| bytes.clone())`. `bytes_for_range`
is `Option<Vec<u8>>`; the let-else path then
`.expect("bytes cloned when range present")`s it — the
invariant is locally enforced (the `Option` is `Some` iff
`range_value` was `Some`).

**(b) Let-else over nested `if`.** The previous shape
(`if req_headers.get(RANGE).is_none() { /* 304 ... */ }
merged`) had the 304 branches indented under a guard, with
the function returning `merged` whether Range was present or
not. Stage 7c needed two different terminal paths: `merged`
for no-Range, `range::apply(merged, ...)` for Range. The
let-else (`let Some(range_value) = range_value else { ...;
return merged; };`) reads top-down: "if there's no Range,
run the 304 logic and return merged; otherwise, fall through
to the Range branch at the tail." This avoids deepening the
indentation by one level and keeps the no-Range 304 code
identical to its Stage 7b shape. The verbose alternative
(`match range_value { Some(v) => ..., None => { ... 304 ...;
merged } }`) was rejected for readability — the let-else
puts the rare Range path at the function tail, the common
no-Range 304 logic in the early-return arm.

**(c) `bytes_for_range` is shadowed.** The outer
`bytes_for_range: Option<Vec<u8>>` at `:776` becomes a
`Vec<u8>` at `:831` via `.expect(...)`. This costs one
identifier reuse for the readability of "the variable now
carries a concrete `Vec<u8>`, not an `Option<Vec<u8>>`". The
`expect` message names the invariant explicitly. An
alternative would be `if let Some(rv) = range_value { let
bfr = bytes_for_range.unwrap(); ... }` — same cost, less
clear about the let-else early-return.

## §4. Probe partition + edge cases (slice 0)

`tools/probe/cases/range-request.json` carries 8 requests
across the 5 grammar shapes Stage 7c cares about:

- `in_range_first_4` (`bytes=0-3`) — explicit pair, anchor of
  the original 7a-era oracle. 206 + `bytes 0-3/11`.
- `in_range_tail` (`bytes=8-`) — open-ended. 206 + `bytes
  8-10/11`.
- `out_of_range` (`bytes=999-1000`) — strictly out of range.
  416 + `bytes */11` + full body.
- `suffix_last_3` (`bytes=-3`) — suffix form. 206 + `bytes
  8-10/11`. Confirms the formula `start = total - n` (the
  reference's `range-parser` lands here too).
- `single_byte` (`bytes=0-0`) — inclusive-end sanity. 206 +
  `bytes 0-0/11`, body is one byte. Pins inclusive-end
  semantics (Stage 7c reviewers might double-take on
  `Content-Length: 1` for a "0-0" range — that's correct).
- `clip_to_end` (`bytes=8-999`) — partial overlap. 206 +
  `bytes 8-10/11`, NOT 416. `range-parser` clips silently;
  irserve mirrors via `n.min(total - 1)` at
  `range.rs:91`. **This was the empirical surprise** —
  intuition might say "out of range = 416", but
  `range-parser` distinguishes "starts in range, extends
  past EOF" (clip) from "starts past EOF" (416). Mirror by
  prediction, validate by snapshot.
- `range_on_missing_file` (`bytes=0-3` on
  `/does-not-exist.txt`) — 404 sanity. Range parsing is
  downstream of the 404 path; Range header is ignored. Body
  diverges between reference (1641-byte vercel HTML) and
  irserve (23-byte fallback `<h1>404 Not Found</h1>\n`),
  covered by `bodyMayDiffer` per D-002 (same pattern as
  Stage 7b's `last-modified-roundtrip.json#ims_on_404`).
- `range_with_inm_match` (`bytes=0-3` + `If-None-Match`
  capture-replay from `in_range_first_4`) — Range pre-empts
  the 304 short-circuit. 206 + partial body, NOT 304. Pins
  the 7a/7b guard at `dispatch.rs:783` (now a let-else)
  under both targets via `$fromResponse`. Stage 7c's most
  load-bearing probe — it covers the architectural
  difference between Stage 7b's "Range absent → fall through
  to 200" and Stage 7c's "Range present → 206 emission" with
  conditional-GET interaction.

The `runner.l0.clean` partition lists all 8 anchors;
`bodyMayDiffer: [range_on_missing_file]` is the only carve-
out. No `divergent` array (mirror-only per D1 + no `D-NNN`).

## §5. Methodological signals

This stage **does not** add a new `D-NNN` entry. The user
chose Mirror in plan-mode D1 (416 body is the full file
mirror reference fall-through); D2 (manual parser in
irserve-core); D3 (moderate edge-probe extension). None of
the three is an intentional divergence from the reference;
they are implementation choices that mirror reference
behavior. The last `D-NNN` is D-018 (Stage 7b's IMS-304
adaptation).

**Anti-hallucination rule #5 in action.** The plan was
explicit that 416-with-full-body is wire-observable and
matches both RFC 7233 §4.4 and the reference's fall-through
to `stream.pipe`. The pre-existing snapshot already pinned
this on the reference side (`out_of_range` body =
`"abcdefghij\n"`, `content-length: 11`); promoting it to
dual-target in slice 2 validated byte-equality across both
targets without re-litigating the contract.

**Anti-hallucination rule #8 in action.** Slice 0 ran the
edge probes (`suffix_last_3`, `single_byte`, `clip_to_end`,
`range_on_missing_file`, `range_with_inm_match`) against the
reference BEFORE implementing the irserve parser, closing
the "what does `range-parser` do on `bytes=8-999`?" question
empirically rather than by reading the npm source. The
`clip_to_end` outcome (206 with clipping, not 416) was the
empirical surprise; the suffix-larger-than-total case
(`bytes=-999` on an 11-byte file) was confirmed via
`range::tests::parse_suffix_larger_than_total_returns_full`
matched against RFC 7233 §2.1.

**Plan-mode AskUserQuestion answers** for the session:

- **D1 — 416 body is the full file.** Mirror. No `D-NNN`.
- **D2 — Manual parser in `range.rs`.** Mirror. ~80 LOC + 21
  unit tests, vs. pulling a crate.
- **D3 — Moderate edge-probe extension.** 5 new anchors
  + `runner.l0.clean` partition (all 8 anchors).

## §6. Verification

Slice-by-slice green at each commit; full verification at
end-of-stage (slice 3 commit):

- `cargo test --workspace --lib` — pass count rises by **31**
  across slices 1 and 2 (21 unit tests in
  `range::tests`, plus 10 integration tests in
  `dispatch::tests` for the 8 new Range scenarios + 2
  updated 7a/7b-precursor tests that flipped their expected
  status from 200 to 206). Final totals refreshed at
  end-of-stage in `tasks.md`'s Validation block.
- `cargo test --test oracle` — **82 total, 77 passed, 5
  skipped, 0 failed** (was 76 passed / 6 skipped after Stage
  7b — `range-request.json` flipped from skipped to passed
  via the new `runner.l0.clean` partition).
- `node tools/probe/run.mjs range-request
  --target=reference --snapshot=verify` — green (snapshot
  re-recorded in slice 0 with the 5 new anchors).
- `node tools/probe/run.mjs range-request
  --target=irserve --snapshot=verify` — green with the L0
  partition applied (all 8 anchors round-trip;
  `range_on_missing_file` body ignored via `bodyMayDiffer`).
- `npx -y @fission-ai/openspec@latest validate --all
  --strict` — slice 3 final step (this change package).
