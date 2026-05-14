# Tasks: Cache-Control default surface + OPTIONS (CORS preflight)

Four iterative slices, one commit per green slice — slice 0
is verification-only, slice 1 is the one-line method-gate
widening, slice 2 collapses to a no-op audit, slice 3 is the
meta slice and lands last so spec deltas reflect what was
actually shipped.

## Slice 0 — Probe partition + ORC promotion (Cache-Control absence-of-default)

- [x] Add `runner.l0` partition to
  `tools/probe/cases/cache-control-default.json`:
  - `clean: [default_file_no_rule, default_listing_html,
    default_listing_json, default_404_html, default_404_json,
    rule_applies_cache_control]` (all 6 anchors).
  - `bodyMayDiffer: [default_listing_html,
    default_listing_json, default_404_html,
    default_404_json]` — the listing HTML / JSON bodies and
    the 404 HTML / JSON bodies are not byte-identical across
    targets (vercel synthetic HTML vs irserve fallback HTML;
    listing layout differs per D-002).
- [x] Promote **ORC-047..ORC-052** in
  `docs/reference/serve/oracle-matrix.md` from reference-only
  to dual-target with the "Promoted to dual-target in Stage
  7d slice 0" annotation.
- [x] Verify: `node tools/probe/run.mjs cache-control-default
  --target=reference --snapshot=verify` green;
  `node tools/probe/run.mjs cache-control-default
  --target=irserve --snapshot=verify` green. No Rust
  changes — `cargo` untouched.
- Commit: `probe(stage-7d): promote cache-control-default to dual-target (ORC-047..052)`
  (`47a56f9`).

## Slice 1 — OPTIONS through the static pipeline (SRV-CORS-001)

- [x] Change the method gate at
  `crates/irserve-core/src/dispatch.rs:91-108` from
  `if req.method() != Method::GET && req.method() != Method::HEAD`
  to
  `if !matches!(*req.method(), Method::GET | Method::HEAD | Method::OPTIONS)`.
  The 405 body and the `headers_path` return slot are
  unchanged.
- [x] Add a code comment at the method gate explaining the
  reference's no-method-branching pipeline (cite
  SRV-CORS-001 and `serve-handler/src/index.js`).
- [x] Add `runner.l0` partition to
  `tools/probe/cases/cors-preflight.json`:
  `clean: [preflight_options]`.
- [x] Promote **ORC-056** in
  `docs/reference/serve/oracle-matrix.md` to dual-target.
- [x] Verify:
  - `node tools/probe/run.mjs cors-preflight
    --target=reference --snapshot=verify` green.
  - `node tools/probe/run.mjs cors-preflight
    --target=irserve --snapshot=verify` green (200 + file
    body + four CORS headers + ETag + Content-Type +
    Accept-Ranges, identical to a GET on the same path).
  - `cargo test --test oracle` green: **79 passed, 3
    skipped, 0 failed** out of 82 (the 3 skips are
    pre-existing cases without an L0 partition or with an
    empty partition; not introduced by 7d).
  - `cargo test --workspace --lib` green — no new unit
    tests in slice 1 because the seam already had unit-test
    coverage on its 405 path; the OPTIONS pass-through is
    exercised end-to-end by the oracle harness.
- Commit: `feat(stage-7d): route OPTIONS through the static pipeline (SRV-CORS-001)`
  (`381f9bd`).

## Slice 2 — Audit remaining `cors-*` probes (collapsed to no-op)

- [x] Inspect the four other `cors-*` probes to confirm
  they already carry `runner.l0.clean` partitions and pass
  dual-target:
  - `tools/probe/cases/cors-applied.json` —
    `clean: [css_with_cors]`. Empirically dual-target
    green.
  - `tools/probe/cases/cors-on-redirect.json` — already
    partitioned and dual-target green.
  - `tools/probe/cases/cors-response-surface.json` —
    `clean: [file_200, cleanurls_301, missing_404]`.
    Empirically dual-target green.
  - `tools/probe/cases/cors-user-override.json` —
    `clean: [css_with_user_acao]`. Empirically dual-target
    green.
- [x] **No additional partition flips and no ORC-row
  promotions needed.** No commit.

## Slice 3 — Spec deltas + meta (this slice)

- [x] Author `openspec/changes/014-cache-headers-and-preflight/`:
  - [x] `proposal.md` — closes SRV-CACHE-005 (dual-target
    verification, no Rust change since 6f mirror) and
    SRV-CORS-001 (dual-target verification, OPTIONS routing
    added in slice 1); out-of-scope list covers HEAD body
    suppression, other HTTP methods staying at 405, the
    three additional CORS headers reference does not emit,
    Stage 7e compression, and default `Cache-Control`
    emission.
  - [x] `design.md` — §1 OPTIONS routing as GET (verbatim
    reference proof points at
    `serve-handler/src/index.js:548-769` plus CLI-side at
    `server.ts:42-93`; irserve seam at `dispatch.rs:91-108`
    before/after); §2 Cache-Control absence-of-default
    (verbatim `getHeaders` block at
    `serve-handler/src/index.js:194-254` with the
    no-default observation; irserve mirror via
    `apply_custom_headers` only); §3 why no `D-NNN`
    (Mirror picked in plan-mode D1; alternatives rejected).
  - [x] `tasks.md` (this file).
  - [x] `specs/http-cache/spec.md` — MODIFIED delta that
    adds the Cache-Control absence-of-default Requirement
    with 6 Scenarios (one per probe anchor) and updates the
    `## Purpose` line to past-tense ("Stage 7d adds...").
  - [x] `specs/cors/spec.md` — ADDED capability spec
    covering the full four-header surface and OPTIONS
    routing under `--cors`. Notes that the L1 baseline
    Requirement (just `Access-Control-Allow-Origin: *`
    flag-presence) stays in `openspec/specs/cli/spec.md`.
- [x] **Main agent (NOT the subagent):**
  - [x] Mirror the deltas into `openspec/specs/http-cache/spec.md`
    and create the new `openspec/specs/cors/spec.md` once
    the change package validates.
  - [x] **No `D-NNN` entry.** The user picked Mirror in
    plan-mode D1; no intentional divergence to record. The
    last `D-NNN` is D-018 (Stage 7b).
  - [x] `README.md`: flip Stage 7d row to `done`; trim the
    "What is NOT yet observable" footer if it lists OPTIONS
    preflight; consider adding an OPTIONS curl demo to "Try
    IrServe".
  - [x] Update `docs/reference/serve/inventory.md` — strike
    the SRV-CORS-001 open question ("Whether IrServe
    should adopt or diverge from the no-preflight-short-
    circuit behavior") since it's resolved to **adopt**.
    Status stays `verified` for both SRVs.
  - [ ] Run `npx -y @fission-ai/openspec@latest validate
    --all --strict` and report any failures.
  - [ ] Commit: `docs(stage-7d): spec deltas + cors capability + meta`

## Validation

Latest totals at end of Stage 7d (refreshed after every
Codex round; doc-hygiene rounds do not change the
underlying counts):

- `cargo test --workspace --lib` — unchanged from
  end-of-7c. Slice 1 added no unit tests; the OPTIONS
  pass-through is exercised end-to-end by the oracle.
- Oracle harness: `target=irserve total=82 passed=79
  skipped=3 failed=0`. Promotion delta vs. end-of-7c:
  `cache-control-default.json` (6 anchors via slice 0)
  and `cors-preflight.json` (1 anchor via slice 1) both
  promoted from skipped to passed.
- `node tools/probe/run.mjs cache-control-default
  --target=reference --snapshot=verify` — green.
- `node tools/probe/run.mjs cache-control-default
  --target=irserve --snapshot=verify` — green.
- `node tools/probe/run.mjs cors-preflight
  --target=reference --snapshot=verify` — green.
- `node tools/probe/run.mjs cors-preflight
  --target=irserve --snapshot=verify` — green.
- `npx -y @fission-ai/openspec@latest validate --all
  --strict` — to be run at end-of-stage. Re-run at end of
  every Codex review round.
