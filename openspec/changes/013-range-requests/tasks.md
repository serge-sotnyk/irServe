# Tasks: Range requests (`206 Partial Content` / `416 Range Not Satisfiable`)

Four iterative slices, one commit per green slice (the slice-0
probe commit lands before any Rust code; slice 3 is the meta
slice and lands last so spec deltas reflect what was actually
shipped).

## Slice 0 — Probe extension + reference snapshot pin

- [x] Extend `tools/probe/cases/range-request.json` with 5 new
  anchors (per plan-mode AskUserQuestion D3):
  - `suffix_last_3` (`bytes=-3`),
  - `single_byte` (`bytes=0-0`),
  - `clip_to_end` (`bytes=8-999` — partial-overlap clip),
  - `range_on_missing_file` (`bytes=0-3` on
    `/does-not-exist.txt` — sanity-pin that the 404 path
    is upstream of Range parsing),
  - `range_with_inm_match` (`bytes=0-3` + `If-None-Match`
    via the Stage-7a `$fromResponse` capture-replay).
- [x] Snapshot recorded under `target=reference` via
  `node tools/probe/run.mjs range-request --target=reference
  --snapshot=update`. Pins: `bytes=-3` → 206 + `bytes 8-10/11`;
  `bytes=0-0` → 206 + `bytes 0-0/11` + 1-byte body
  (inclusive-end semantics); `bytes=8-999` → 206 + `bytes
  8-10/11` (partial-overlap **clips** to `total-1`, NOT 416 —
  the empirical surprise); missing-file Range → 404 with the
  reference's 1641-byte vercel HTML body; Range + matching
  INM → 206, not 304 (Range pre-empts the conditional-GET
  short-circuit per `serve-handler/src/index.js:760`).
- [x] Add **ORC-173..ORC-177** in
  `docs/reference/serve/oracle-matrix.md` (reference-only
  initially; promoted to dual-target in slice 2 via the
  `runner.l0.clean` partition).
- [x] Verify: `node tools/probe/run.mjs range-request
  --target=reference --snapshot=verify` green; no Rust changes
  so `cargo` untouched.
- Commit: `probe(stage-7c): extend range-request fixture with edge cases (suffix, single-byte, clip, 404, INM-interaction)`
  (`4d86eeb`).

## Slice 1 — `range` parser module + apply helper (not yet wired)

- [x] New module `crates/irserve-core/src/range.rs`:
  - [x] `pub enum RangeOutcome { InRange { start, end },
    Unsatisfiable }`.
  - [x] `pub fn parse_range(value: &str, total: u64) ->
    RangeOutcome` — grammar: `bytes=<s>-<e>` /
    `bytes=<s>-` / `bytes=-<n>`. Multi-range comma list
    takes the first segment only via
    `rest.split(',').next()`. Wrong unit (`pixels=0-3`),
    malformed input, and strictly out-of-range
    (`bytes=999-1000`, `bytes=10-1`) collapse to
    `Unsatisfiable` so the caller routes everything non-OK
    through one 416 branch. Partial-overlap clipping
    (`bytes=8-999` on 11-byte total → `{8, 10}`) via
    `n.min(total - 1)`. Suffix-larger-than-total
    (`bytes=-999` on 11-byte total → full representation)
    per RFC 7233 §2.1.
  - [x] `pub fn apply(merged, range_header, bytes, total) ->
    Response<Body>` — dispatches on
    `range_header.to_str()` (non-ASCII → 416) and then on the
    parser outcome. `build_206` slices the body to
    `bytes[start..=end]`, sets status to
    `StatusCode::PARTIAL_CONTENT`, and `inserts`
    `Content-Range: bytes <s>-<e>/<total>` +
    `Content-Length: <e-s+1>` (last-write-wins over any user
    rule, mirrors reference's post-`getHeaders` injection at
    `serve-handler/src/index.js:749-752`). `build_416` keeps
    the merged response's full body verbatim, swaps status
    to `StatusCode::RANGE_NOT_SATISFIABLE`, and inserts
    `Content-Range: bytes */<total>`. Other headers
    (Content-Type, ETag, Last-Modified, user-rule headers)
    carry over from `merged` unchanged.
- [x] `crates/irserve-core/src/lib.rs` declares `mod range;`.
- [x] Tests: **21 new** in `range::tests` — 14 parser cases
  (in-range / open-end / suffix / single-byte /
  partial-overlap-clip / fully-out / start-after-end /
  non-numeric / empty-after-eq / multi-range / wrong-unit /
  negative-zero-suffix / suffix-larger-than-total /
  empty-file) + 7 apply cases (in-range → 206;
  out-of-range → 416 with full body; start-after-end → 416;
  Content-Type and ETag preserved on 206; user custom headers
  preserved on 206; user Content-Length overwritten on 206;
  suffix returns last N bytes).
- [x] Probe partition rollback. Slice 0 added the
  `runner.l0.clean` partition together with the 5 new probes;
  with no irserve implementation, the partition turned
  `target=irserve` red for `range-request.json`. Slice 1
  reverts the partition; slice 2 re-adds it together with the
  dispatch wiring so both ratchet up in lockstep.
- [x] Verify: `cargo test --workspace --lib` green
  (+21 tests); oracle harness unchanged (range-request stays
  skipped under `target=irserve`).
- Commit: `feat(stage-7c): slice 1 — range parser module + apply helper (unit-tested, not yet wired)`
  (`40fc032`).

## Slice 2 — Wire `range::apply` into `build_file_or_304`

- [x] `crates/irserve-core/src/dispatch.rs`: `use crate::range;`
  at the top.
- [x] Reshape `build_file_or_304` at
  `crates/irserve-core/src/dispatch.rs:759`:
  - Compute `total = bytes.len() as u64` at the top.
  - Clone `bytes` **only when Range is present** at
    `dispatch.rs:775-776` via
    `range_value.as_ref().map(|_| bytes.clone())`. The
    no-Range path keeps its single-allocation shape.
  - Convert the previous `if req_headers.get(RANGE).is_none()
    { /* 304 branches */ } merged` shape into a let-else at
    `dispatch.rs:783`:
    `let Some(range_value) = range_value else { /* 304
    branches; return merged; */ };`. The `else` arm carries
    the verbatim Stage-7b ETag/INM and Last-Modified/IMS
    branches with no behavioral change.
  - Tail: `range::apply(merged, &range_value,
    &bytes_for_range, total)` at `dispatch.rs:832`.
- [x] Promote `runner.l0.clean` in `range-request.json` to
  all 8 anchors and add `bodyMayDiffer:
  [range_on_missing_file]` (the 404 HTML body differs
  between targets per D-002 — same pattern as
  `last-modified-roundtrip.json#ims_on_404`).
- [x] Update two pre-existing 7a/7b-precursor tests:
  `range_present_skips_304_even_on_match` and
  `range_present_skips_ims_304_even_on_match` previously
  pinned the 200 fall-through under Range; with Range
  emission live they now pin
  `StatusCode::PARTIAL_CONTENT` and the expected
  `Content-Range`.
- [x] **10 new** integration tests in `dispatch::tests`
  (covering both the `etag: true` default path and the
  `etag: false` / `--no-etag` path):
  - `range_in_range_returns_206`,
  - `range_tail_returns_206`,
  - `range_suffix_returns_206`,
  - `range_out_of_range_returns_416`,
  - `range_with_ims_match_under_no_etag_returns_206_not_304`
    (7b interaction: Range pre-empts the IMS 304 even when
    `If-Modified-Since` matches the captured `Last-Modified`),
  - `range_preserves_etag_on_206`,
  - `range_preserves_user_custom_headers_on_206` (user
    `ETag` override via `compile_etag_override` survives
    the 206 transformation),
  - `range_416_preserves_etag_and_user_headers` (the same
    user-rule survival, but on the 416 path),
  - `range_user_last_modified_delete_under_no_etag_still_emits_206`
    (belt-and-suspenders: user rule deletes `Last-Modified`,
    Range still emits 206; the let-else doesn't depend on
    validator presence),
  - plus the two updated precursor tests above.
- [x] Update `docs/reference/serve/oracle-matrix.md`:
  ORC-044/045/046 + ORC-173..177 flip from reference-only to
  dual-target with the "Promoted to dual-target in Stage 7c
  slice 2" annotation.
- [x] Verify: `cargo test --workspace --lib` green
  (+10 tests over slice 1); `cargo test --test oracle`
  **82 total, 77 passed, 5 skipped, 0 failed** (was 76 / 6 —
  range-request flipped from skipped to passed); both probe
  targets `--snapshot=verify` green.
- Commit: `feat(stage-7c): slice 2 — emit 206/416 on Range requests`
  (`794fe6c`).

## Slice 3 — Spec deltas + meta (this slice)

- [x] Author `openspec/changes/013-range-requests/`:
  - [x] `proposal.md` — closes SRV-CACHE-004 (dual-target
    verification); out-of-scope list mirrors the kickoff
    plan's 12-item pre-stage list.
  - [x] `design.md` — §1 the Range emission seam (reference
    L717-741 + L749-752), §2 the `range` module, §3 the
    dispatch-level seam (let-else + conditional clone), §4
    probe partition + edge cases, §5 methodological signals
    (no `D-NNN`), §6 verification.
  - [x] `tasks.md` (this file).
  - [x] `specs/http-cache/spec.md` — ADDED Requirement for
    SRV-CACHE-004 with 5 Scenarios; MODIFIED Requirement on
    the Stage 7b Last-Modified entry (the `Range + matching
    If-Modified-Since` Scenario flips from "Stage 7c
    precursor returns 200" to "Range pre-empts the IMS 304 →
    206"); Compatibility notes section.
- [x] **Main agent (NOT the subagent):**
  - [x] Mirror the delta into
    `openspec/specs/http-cache/spec.md` (the canonical
    merged spec) once the change package validates.
  - [x] **No `D-NNN` entry.** The user picked Mirror in D1;
    no intentional divergence to record. The last `D-NNN`
    is D-018 (Stage 7b).
  - [x] `README.md`: flip Stage 7c row to `done`; trim the
    "What is NOT yet observable" footer (drop `Range
    requests (206/416)`); add a Range curl demo to "Try
    IrServe".
  - [x] Update `docs/reference/serve/inventory.md` — append
    extended-probe note under SRV-CACHE-004's Probe bullet
    (status stays `verified` — was already that).
  - [x] Run `npx -y @fission-ai/openspec@latest validate
    --all --strict` and report any failures. (22/22 passed.)
  - [ ] Commit: `docs(stage-7c): spec deltas + oracle-matrix
    promotion + meta`

## Validation

Latest totals at end of Stage 7c (refreshed after every
Codex round; doc-hygiene rounds do not change the underlying
counts):

- `cargo test --workspace --lib` — total rises by **31** vs.
  end-of-7b (21 in `range::tests` from slice 1; 10 in
  `dispatch::tests` from slice 2 — 8 new Range scenarios + 2
  updated 7a/7b-precursor tests whose expected status flipped
  from 200 to `PARTIAL_CONTENT`). Exact counts refreshed in
  the commit message of the slice-3 doc commit and any
  subsequent Codex-round commits.
- Oracle harness: `target=irserve total=82 passed=77 skipped=5
  failed=0`. Promotion delta vs. slice 1: `range-request.json`
  (all 8 anchors via the new `runner.l0.clean` partition).
- `node tools/probe/run.mjs range-request
  --target=reference --snapshot=verify` — green.
- `node tools/probe/run.mjs range-request
  --target=irserve --snapshot=verify` — green.
- `npx -y @fission-ai/openspec@latest validate --all --strict`
  — passed at end-of-stage. Run end of every Codex review
  round.
