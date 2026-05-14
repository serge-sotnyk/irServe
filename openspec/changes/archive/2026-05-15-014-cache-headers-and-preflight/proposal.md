# Proposal: Cache-Control default surface + OPTIONS (CORS preflight)

## Why

Stage 7d is the fourth sub-stage of Stage 7 (L3 polish) per
`docs/stage7_l3_capabilities.md` and the next "Next" row in
the README stage map. It cleans up two small loose ends left
after Stages 7a/7b/7c: (a) the absence-of-default
`Cache-Control` contract, which irserve already mirrored
since Stage 6f (`apply_custom_headers`) but had never been
pinned at L0; and (b) the OPTIONS routing surface under
`--cors`, where irserve was returning `405 Method Not
Allowed` while the reference's `serve-handler` never
inspects `request.method` and therefore runs the full
static pipeline for OPTIONS too.

Both pieces of work fit the methodology's smallest-cut
profile: one is verification-only (slice 0), one is a
one-line method-gate widening at the dispatcher entry
(slice 1), and the empirical audit of the four other
`cors-*` probes (slice 2) collapsed to a no-op because they
were already L0-clean dual-target. Stage 7d is the smallest
of Stage 7 by total commit count (three pre-meta commits).

This change closes:

- **SRV-CACHE-005** (P2, status: `verified`, level: L3) —
  irserve emits NO default `Cache-Control` header on any
  response (file, directory listing, 404, redirect); the
  header appears verbatim only when a user
  `serve.json#headers` rule sets it. Mirrors reference's
  `getHeaders` at `third_party/serve-handler/src/index.js:194-254`
  where `defaultHeaders` (L215-243) never includes
  `Cache-Control` — the header reaches the response only via
  `customHeaders` matched + merged by `Object.assign(defaultHeaders,
  related)` at `:241`. irserve's only entry point for the
  header is `apply_custom_headers`
  (`crates/irserve-core/src/custom_headers.rs:178-221`,
  introduced in Stage 6f for SRV-HDR-001); a grep across
  `crates/` for `cache-control` / `Cache-Control` returns
  zero matches outside the user-rules code path. Verified
  empirically by `tools/probe/cases/cache-control-default.json`
  running L0-clean dual-target across 6 anchors after slice 0.

- **SRV-CORS-001** (P1, status: `verified`, level: L3) —
  the full four-header response surface under `--cors`
  (`Access-Control-Allow-Origin: *`,
  `Access-Control-Allow-Headers: *`,
  `Access-Control-Allow-Credentials: true`,
  `Access-Control-Allow-Private-Network: true`), applied
  post-dispatch via `apply_cors`
  (`crates/irserve-core/src/cors.rs:26-37`, introduced in
  Stage 6h), **and** the OPTIONS-routed-as-GET preflight
  semantics introduced in slice 1 of Stage 7d. After 7d,
  an `OPTIONS /asset.css` under `--cors` flows through
  phases 3..13 of the dispatcher and yields `200 OK` with
  the file body + the four CORS headers + ETag +
  Content-Type. (`Accept-Ranges: bytes` appears in the
  reference snapshot but is not part of the dual-target
  contract — `tools/probe/run.mjs` masks it via
  `L0_EXTRA_VOLATILE_HEADERS`; irserve does not emit it.)
  The inventory open
  question on SRV-CORS-001 ("Whether IrServe should adopt
  or diverge from the no-preflight-short-circuit behavior")
  resolves to **adopt** — no 204 short-circuit, mirror the
  reference's no-method-branching pipeline.

7d **does not add a `D-NNN` entry** to
`docs/reference/serve/decisions.md`. The user picked Mirror
in plan-mode AskUserQuestion D1 (OPTIONS through pipeline,
no 204 short-circuit); there is no intentional divergence
to record for either SRV. The last `D-NNN` is D-018 (from
Stage 7b); 7c also added none.

## What

- **Cache-Control absence-of-default probe partition
  (slice 0).** Added `runner.l0.clean` (all 6 anchors) +
  `bodyMayDiffer` (4 volatile-body anchors: the two
  directory listings and the two 404s) to
  `tools/probe/cases/cache-control-default.json`. Promoted
  ORC-047..052 in `docs/reference/serve/oracle-matrix.md`
  from reference-only to dual-target with the "Promoted to
  dual-target in Stage 7d slice 0" annotation. **No Rust
  changes** — verification-only confirmation that the
  6f-vintage `apply_custom_headers` seam was already the
  sole `Cache-Control` entry point and that no other
  branch emitted the header by default. Commit `47a56f9`.

- **OPTIONS method-gate widening (slice 1).** Changed the
  method gate at
  `crates/irserve-core/src/dispatch.rs:91-108` from
  `if req.method() != Method::GET && req.method() != Method::HEAD`
  to `if !matches!(*req.method(), Method::GET | Method::HEAD | Method::OPTIONS)`.
  No other code change — phases 3..13, `apply_custom_headers`,
  `apply_cors`, and `build_file_or_304` (ETag/304/Range)
  carry over automatically. Added `runner.l0.clean:
  ["preflight_options"]` to
  `tools/probe/cases/cors-preflight.json`. Promoted ORC-056
  in `docs/reference/serve/oracle-matrix.md` to dual-target.
  `cargo test --test oracle` ends at **79 passed, 3 skipped,
  0 failed** out of 82 (the 3 skips are pre-existing cases
  without an L0 partition or with an empty partition).
  Commit `381f9bd`.

- **Audit of remaining `cors-*` probes (slice 2,
  collapsed).** Empirically verified that
  `cors-applied.json`, `cors-on-redirect.json`,
  `cors-response-surface.json`, and
  `cors-user-override.json` already carry `runner.l0.clean`
  partitions and pass dual-target. No additional probe
  partition flips and no ORC-row updates were needed.
  Slice 2 produced no commit.

- **Documentation updates (slice 3, this change package +
  main agent).** New change package
  `openspec/changes/archive/2026-05-15-014-cache-headers-and-preflight/` with
  this proposal + design + tasks + a MODIFIED delta on the
  `http-cache` Requirement set (adds the Cache-Control
  absence-of-default Requirement) + an ADDED capability
  spec `cors` covering the full four-header surface and
  the OPTIONS-routed-as-GET semantics. The L1 baseline
  Requirement on `--cors` in
  `openspec/specs/cli/spec.md` (SRV-CLI-010) stays
  unchanged — the new `cors` capability spec covers the L3
  surface without duplicating the L1 flag-presence
  contract. **No `D-NNN` entry.** Inventory open question
  on SRV-CORS-001 is struck. README's stage-7d row flips
  to `done`; the "What is NOT yet observable" footer
  drops "OPTIONS preflight (200 + file body)" if present.

## Out of scope

1. **HEAD body suppression.** Reference's `stream.pipe(response)`
   at `serve-handler/src/index.js:769` runs unconditionally
   regardless of method; HEAD therefore receives the same
   body as GET in the reference. irserve mirrors. Neither
   side implements the RFC 7231 §4.3.2 "HEAD has no body"
   semantic. Pinning this as a stage in its own right is
   deferred.

2. **TRACE / CONNECT / PATCH / POST / PUT / DELETE.** Still
   return `405 Method Not Allowed` from the dispatcher's
   method gate. Stage 7d only widens the allow-list to add
   OPTIONS; other methods stay rejected. The reference
   accepts every method (no method check), so this is a
   known divergence inherited from Stage 1 and not
   addressed by 7d.

3. **`Access-Control-Allow-Methods`, `Access-Control-Expose-Headers`,
   `Access-Control-Max-Age`.** Reference does NOT emit
   these under `--cors`; irserve mirrors. They are NOT
   added by 7d. See the cors capability spec's Compatibility
   notes for the closure rationale.

4. **Stage 7e compression / encoding negotiation.** Out of
   scope for 7d. Independent sub-stage in `stage7_l3_capabilities.md`.

5. **Default `Cache-Control` emission.** 7d only pins
   absence-of-default; it does NOT add any default value.
   Adding a default (e.g. `Cache-Control: no-cache`) would
   diverge from reference, which is explicitly out of scope
   per the SRV-CACHE-005 framing.

## Risks

- **Lowest-risk stage of Stage 7.** Both slices mirror the
  reference; no `D-NNN`, no parser corner cases, no
  divergent probe partitions. Anti-hallucination rule #9
  (3+ consecutive review rounds on the same fine-grained
  aspect → declare parity scope) is **not** expected to
  fire — Codex review rounds projected at 0-1, given the
  scope size. If a fine-grained aspect surfaces in review,
  the most likely candidates are (a) whether
  `Access-Control-Allow-Private-Network: true` should
  remain unconditional under `--cors` (it does — reference
  emits it unconditionally) and (b) whether OPTIONS on
  missing paths should carry the CORS overlay (it does —
  `apply_cors` runs post-dispatch on every response).

- **HEAD-body-suppression aside.** A reviewer may ask why
  HEAD is not suppressed. The answer is symmetric: neither
  reference nor irserve implements it; the question is out
  of scope for 7d and noted in the `cors` capability spec's
  Compatibility notes. No change needed unless the user
  asks for it in a future stage.
