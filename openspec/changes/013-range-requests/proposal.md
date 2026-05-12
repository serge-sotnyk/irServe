# Proposal: Range requests (`206 Partial Content` / `416 Range Not Satisfiable`)

## Why

Stage 7c is the third sub-stage of Stage 7 (L3 polish) per
`docs/stage7_l3_capabilities.md` and the next "Next" row in
the README stage map. It builds on the structural foundations
from Stage 7a (`openspec/changes/011-etag-conditional/`) and
Stage 7b (`openspec/changes/012-last-modified/`): the
`build_file_or_304` dispatcher helper, the merge-before-decide
ordering against the user `headers` overlay, and the
`Range`-absent guard at `crates/irserve-core/src/dispatch.rs:783`
that already short-circuited the ETag/INM and Last-Modified/IMS
304 branches whenever a `Range` header was present. 7c fills in
the `else` arm of that guard: instead of falling through to the
merged 200, irserve now parses the `Range` header and emits
either 206 (in-range) or 416 (out-of-range) with the
appropriate `Content-Range` / `Content-Length` headers — all
mutating the merged response in place so ETag, Last-Modified,
Content-Type, and user-rule headers carry through unchanged.

This change closes:

- **SRV-CACHE-004** (P2, status: `verified`, level: L3) —
  `Range: bytes=<s>-<e>` / `bytes=<s>-` / `bytes=-<n>` on file
  responses returns `206 Partial Content` with
  `Content-Range: bytes <s>-<e>/<total>` and
  `Content-Length: <e-s+1>` when satisfiable; strictly
  out-of-range or malformed values return `416 Range Not
  Satisfiable` with `Content-Range: bytes */<total>` and the
  full file as the response body (mirrors reference's fall-
  through to `stream.pipe` with empty `streamOpts` at
  `third_party/serve-handler/src/index.js:730-741`; RFC 7233
  §4.4 permits a representation on 416). The reference status
  was already `verified` via the pre-existing
  `tools/probe/snapshots/range-request.json`; after Stage 7c
  the irserve implementation reaches dual-target verification
  across all 8 anchors.

7c **does not add a `D-NNN` entry** to
`docs/reference/serve/decisions.md`. The user picked Mirror in
plan-mode AskUserQuestion D1 (416 body is the full file, per
RFC 7233 §4.4 + reference fall-through); there is no
intentional divergence to record. The last `D-NNN` is D-018
(from Stage 7b).

The Stage 7b `Range`-absent guard reshaping (let-else
pre-empts BOTH 304 short-circuits when Range is present)
extends Stage 7a's `if (request.headers.range == null) { ... }`
pattern from a single ETag/INM branch into a wrapper around
both the ETag/INM and Last-Modified/IMS branches. Stage 7c
generalises that further with the `let Some(range_value) =
range_value else { ... return merged; }` shape at
`crates/irserve-core/src/dispatch.rs:783` — the `else` arm
carries the no-Range 304 logic verbatim, and the function tail
calls `range::apply` on the merged response when a Range was
present.

## What

- **`range` parser + apply helper (slice 1).** New
  `crates/irserve-core/src/range.rs` exposes
  `parse_range(value: &str, total: u64) -> RangeOutcome`
  (single-range, `bytes` unit, inclusive end, partial-overlap
  clipping, suffix form) and `apply(merged, range_header,
  bytes, total) -> Response<Body>` which mutates the merged
  200 response into either 206 (slice the body, set status,
  inject `Content-Range` + `Content-Length`) or 416 (keep the
  full body, set status, inject `Content-Range: bytes */<total>`).
  21 unit tests pin the parser surface and the apply
  transformations. Module declared at
  `crates/irserve-core/src/lib.rs`. Mirrors the subset of
  vercel/serve-handler's `range-parser` npm dependency we
  need — see Compatibility note "Manual parser vs range-parser"
  in `specs/http-cache/spec.md` for the divergence ledger.

- **Dispatch wiring + 304 pre-emption refactor (slice 2).**
  `build_file_or_304` at
  `crates/irserve-core/src/dispatch.rs:759` was reshaped:
  the previous `if req_headers.get(RANGE).is_none() { /* 304
  branches */ } merged` flowed into a `let Some(range_value)
  = range_value else { /* 304 branches; return merged; */ };
  range::apply(merged, &range_value, &bytes_for_range, total)`
  shape. The let-else preserves the two 304 short-circuits
  verbatim in its `else` arm (no behavioral change for
  Range-less requests) and pre-empts both when Range is
  present (mirrors `serve-handler/src/index.js:760`'s
  `request.headers.range == null` guard, generalised over the
  irserve adaptation that wraps both validators). The file
  bytes are cloned at
  `crates/irserve-core/src/dispatch.rs:775-776` **only when a
  Range header is present** (`range_value.as_ref().map(|_|
  bytes.clone())`) so the common no-Range path keeps its
  single-allocation shape; `range::apply` needs the original
  bytes back for both the 206 slice and the 416 retransmit
  because `file_response` moves `bytes: Vec<u8>` into the
  response body. The `range::apply` call lands at
  `crates/irserve-core/src/dispatch.rs:832`. Ten new
  integration tests + two updated 7a/7b-precursor tests
  (`range_present_skips_304_even_on_match` and
  `range_present_skips_ims_304_even_on_match` — both
  previously pinned the 200 fall-through; with Range emission
  live they now pin `StatusCode::PARTIAL_CONTENT`) cover the
  dispatch-level seam.

- **Probes + edge-case extension (slice 0).** Extended
  `tools/probe/cases/range-request.json` with 5 new anchors
  (per plan-mode AskUserQuestion D3): `suffix_last_3`
  (`bytes=-3`), `single_byte` (`bytes=0-0`), `clip_to_end`
  (`bytes=8-999` — partial overlap clips to file end per
  `range-parser` semantics, NOT 416), `range_on_missing_file`
  (`bytes=0-3` on `/does-not-exist.txt` — Range parsing is
  downstream of the 404 path, sanity-pinned), and
  `range_with_inm_match` (Range pre-empts the 304
  short-circuit even with matching `If-None-Match`, via the
  `$fromResponse` capture-replay introduced in Stage 7a). The
  `runner.l0` partition gains
  `clean: [in_range_first_4, in_range_tail, out_of_range,
  suffix_last_3, single_byte, clip_to_end,
  range_on_missing_file, range_with_inm_match]` (all 8
  anchors) and `bodyMayDiffer: [range_on_missing_file]` —
  the 404 HTML body content is not contractual per D-002 (same
  pattern as `last-modified-roundtrip.json#ims_on_404` from
  Stage 7b). Snapshot re-recorded under `target=reference`
  with the 5 new anchors; the 3 pre-existing anchors stay
  byte-identical.

- **Documentation updates (slice 3, this change package +
  main agent).** New change package
  `openspec/changes/013-range-requests/` with this proposal +
  design + tasks + an ADDED Requirement on http-cache + a
  MODIFIED note on the Stage 7b Last-Modified Requirement
  (re-frames the `Range + matching If-Modified-Since`
  Scenario from "7c precursor returns 200" to "Range
  pre-empts the IMS 304 → 206"). **No D-NNN entry** — the
  user chose Mirror in D1; no intentional divergence. README's
  stage-7c row is flipped to `done` by the main agent in
  slice 3, alongside trimming the "What is NOT yet
  observable" footer (drop `Range requests (206/416)`) and
  adding the Range curl demo to "Try IrServe". ORC-044/045/046
  promoted from reference-only to dual-target in
  `docs/reference/serve/oracle-matrix.md`; ORC-173..177 added
  as dual-target rows for the new anchors.

## Out of scope

Mirrors `docs/features/0017_PLAN_stage7c_range_requests.md`'s
pre-stage out-of-scope list (anti-hallucination rule #10):

1. **Multiple ranges (`Range: bytes=0-3, 8-10`).** Reference
   has `// TODO ? multiple ranges` at
   `serve-handler/src/index.js:736` and uses only `range[0]`.
   irserve mirrors: first range only via `rest.split(',').next()`
   in `parse_range`; subsequent ranges silently ignored. No
   `multipart/byteranges` response shape.

2. **`If-Range` header.** Reference has `// TODO ? if-range`
   at `serve-handler/src/index.js:719` — unimplemented.
   irserve mirrors: `If-Range` is read off the request and
   ignored. RFC 7233 §3.2 conditional-range semantics
   deferred.

3. **`Accept-Ranges: bytes` response header on file
   responses.** Reference emits it (visible in
   `tools/probe/snapshots/range-request.json`) but it's
   already in irserve's `L0_EXTRA_VOLATILE_HEADERS` mask in
   `tools/probe/run.mjs`. irserve does NOT emit
   `Accept-Ranges`; the wire contract under SRV-CACHE-004 is
   the 206/416 status + `Content-Range` + `Content-Length`,
   not `Accept-Ranges`. Promoting it to contractual is a
   separate change.

4. **Range support beyond static files.** Directory listings,
   3xx redirects, JSON error responses, custom HTML error
   pages, the synthetic fallback HTML error body: none of
   these honor `Range`. Same surface as ETag/Last-Modified in
   7a/7b — Range parsing lives inside `build_file_or_304`,
   which is reached only from the File/Index arm at
   `dispatch.rs:347` and the renderSingle branch at
   `dispatch.rs:481`. Other dispatcher arms silently ignore
   the request `Range` header (no 206/416 emission).

5. **Range on 404 / non-file responses.** A `Range`-bearing
   request for a missing file returns 404, not 416 + 404.
   The 404 path is structurally upstream of
   `build_file_or_304`; Range parsing never runs. Pinned by
   `range_on_missing_file` (ORC-176).

6. **`Content-Length` explicit set on 416.** Reference's
   416 path doesn't explicitly set `Content-Length`, but
   Node's http response infers it from the full body length
   → `Content-Length: 11` for the 11-byte fixture. axum's
   `Body::from(bytes)` does the same. Wire shape matches; the
   internal "did we set it explicitly" question is non-
   contractual. irserve's 416 path does NOT call
   `headers.insert(CONTENT_LENGTH, ...)` — axum derives it.

7. **Range on POST / PUT / DELETE.** Reference serves only
   GET/HEAD on static files; non-GET methods produce 405 or
   pass through. Range on a 405 is not a thing. No probe.

8. **Range header with garbage AFTER a valid first range
   (`bytes=0-3, garbage`).** The manual parser splits on the
   first `,` and parses only the first segment; trailing
   garbage is silently ignored. Mirrors "first range only"
   semantics. Pinned by
   `range::tests::parse_multi_range_uses_first_only`. Not
   probed against the reference (the existing `range-parser`
   in `range-parser/index.js` either accepts the first and
   ignores garbage or rejects wholesale depending on whether
   the trailing segment is itself parseable; both lands in
   our "first only" outcome).

9. **Negative-zero suffix (`bytes=-0`).** RFC 7233 §2.1's
   suffix-length semantics are undefined for 0;
   `range-parser` returns `Unsatisfiable` (i.e. `-2`). Manual
   parser mirrors via the `Ok(n) if n > 0` guard in the
   suffix arm at `range.rs:50-52`. Pinned by
   `range::tests::parse_negative_zero_suffix_is_unsatisfiable`.

10. **`Vary: Range` response header.** Reference does NOT
    emit. irserve mirrors. RFC suggests `Vary: Range` for
    cacheability with intermediaries, but neither side adds
    it.

11. **HTTP/2 / HTTP/3 range semantics.** irserve serves
    HTTP/1.1 only (axum default). HTTP/2 multiplexed range
    requests are out of scope.

12. **Range on HEAD.** Reference: HEAD requests reuse the
    GET pipeline and emit headers without body, so `Range:
    bytes=0-3` on HEAD would emit `Content-Range: bytes
    0-3/N` and `Content-Length: 4` with no body. irserve
    does not currently have a HEAD-specific test surface
    (axum's behavior on HEAD body stripping was not pinned
    in 7c). No HEAD-with-Range probe; if surfaced in review,
    add a probe and either confirm match or record
    divergence.
