# Tasks: Last-Modified + --no-etag + If-Modified-Since

Five iterative slices, one commit per green slice (the slice-0
probe commit lands before any Rust code; slice 4 is the meta
slice and lands last so spec deltas reflect what was actually
shipped).

## Slice 0 — Reference probes closing Q-009

- [x] New `tools/probe/cases/last-modified-roundtrip.json` (6
  requests under `serveArgs: ["--no-etag"]`): `first_get`,
  `ims_exact` (via `$fromResponse` capture-replay of
  `first_get`'s `last-modified`), `ims_future`, `ims_past`,
  `ims_malformed`, `ims_on_404`.
- [x] Snapshot recorded under `target=reference` via
  `node tools/probe/run.mjs last-modified-roundtrip
  --target=reference --snapshot=update`. Pins
  `first_get` → 200 + `Last-Modified` (no `ETag`, per the
  reference's mutex at `serve-handler/src/index.js:227-236`);
  every IMS variant → 200 with the full body (the reference
  has no IMS branch — grep across `third_party/serve-handler/src/`
  and `third_party/serve/src/` returns no hits for
  `if-modified-since`); `ims_on_404` → 404.
- [x] Close **Q-009** in
  `docs/reference/serve/open-questions.md` with the snapshot
  citation.
- [x] Flip **SRV-CACHE-003** in
  `docs/reference/serve/inventory.md` from `unknown` to
  `verified` (reference) / `adapted` (irserve — flagged as
  pending until D-018 lands in slice 4).
- [x] Add **ORC-167..ORC-172** in
  `docs/reference/serve/oracle-matrix.md` (reference-only
  initially; ORC-167/168/169 promoted to dual-target in
  slice 3).
- [x] Verify: `node tools/probe/run.mjs last-modified-roundtrip
  --target=reference --snapshot=verify` green; no Rust changes
  so `cargo` untouched.
- Commit: `probe(stage-7b): close Q-009 — reference IMS handling under --no-etag`
  (`351b3a7`).

## Slice 1 — `--no-etag` CLI flag

- [x] `crates/irserve/src/main.rs:82-83`: add
  `#[arg(long = "no-etag")] no_etag: bool` to `Cli`,
  long-only (no short alias). Mirrors
  `third_party/serve/source/utilities/cli.ts:155`
  (`'--no-etag': Boolean,`).
- [x] Post-parse override at `main.rs:144-146`: when
  `cli.no_etag`, force `serve_config.etag = Some(false)`
  after the `serve.json` merge. Diverges slightly from
  reference's `force-true-without-flag` at
  `source/utilities/config.ts:140` — irserve preserves the
  Stage-7a `serve.json#etag` honoring when the flag is unset;
  documented as a Compatibility note.
- [x] SRV-CLI-013 doc comment on the field.
- [x] No new unit tests this slice — threading is a single
  boolean override; behavioral coverage lands in slices 2 and
  3. The planned dedicated `no-etag-flag.json` smoke probe
  was dropped (redundant with
  `last-modified-roundtrip.json#first_get`).
- [x] Verify: `cargo build --workspace` green;
  `cargo test --workspace` 265/265 unit tests green;
  `cargo test --test oracle` 82 total, 75 passed, 7 skipped
  (last-modified-roundtrip is in the skipped bucket — slice 3
  promotes its clean partition); `cargo run -- --help` lists
  `--no-etag`.
- Commit: `feat(stage-7b): slice 1 — --no-etag CLI flag wired through to ServeConfig`
  (`127b6ce`).

## Slice 2 — `last_modified` module + emission on 200

- [x] New module `crates/irserve-core/src/last_modified.rs`
  exposing `last_modified_value(serve_config, meta:
  Option<&Metadata>) -> Option<HeaderValue>`. Returns
  `Some(httpdate::fmt_http_date(mtime))` only when
  `serve_config.etag == Some(false)` AND `meta.modified()`
  succeeds. Mutex with ETag — direct mirror of
  `serve-handler/src/index.js:227-236`'s `if (etag) / else`
  branch.
- [x] `crates/irserve-core/Cargo.toml` gains
  `httpdate = "1"` (pinned-stable 1.0.3). Workspace
  `Cargo.toml` updated accordingly.
- [x] `crates/irserve-core/src/lib.rs` declares
  `mod last_modified;`.
- [x] `dispatch.rs::file_response` (now at `dispatch.rs:666-691`)
  gains a `last_modified: Option<HeaderValue>` parameter; the
  header is written when `Some`. Both headers (ETag,
  Last-Modified) are written independently so user `headers`
  rules later in `apply_custom_headers` can override or
  supplement either — the mutex is upstream in the value
  helpers, not in `file_response`.
- [x] `dispatch.rs::build_file_or_304` (now at
  `dispatch.rs:754`) gains a `meta: Option<&Metadata>`
  parameter (post-`bytes`, pre-`header_rules`); computes both
  `etag_value` and `last_modified_value` from the same
  `serve_config.etag` predicate at `dispatch.rs:763-765`, so
  exactly one of the two ends up `Some` per call. 304 logic
  unchanged in this slice; the IMS branch lands in slice 3.
- [x] Both call sites (File/Index arm at `dispatch.rs:346`,
  renderSingle branch at `dispatch.rs:480`) now
  `tokio::fs::metadata(&p).await.ok()` alongside the bytes
  read. Two-syscall pattern; TOCTOU on mtime is immaterial
  at IMF-fixdate's whole-second resolution.
- [x] Tests: 4 new in `last_modified::tests` (known-mtime →
  fixed wire string `"Fri, 01 Jan 2021 00:00:00 GMT"`;
  ETag-on suppresses LM for both default and explicit `true`;
  ETag-off emits LM with IMF-fixdate shape sanity; ETag-off +
  no meta → `None`). 2 new in `dispatch::tests`
  (`etag_off_emits_last_modified_from_meta` — LM present,
  no ETag, IMF-fixdate shape; `etag_on_suppresses_last_modified_even_with_meta`
  — ETag present, LM absent under default config). All 10
  existing dispatch tests updated to pass `None` for the new
  `meta` argument (they exercise the ETag-on path).
- [x] Verify: `cargo test --workspace` 271/271 green (was
  265 in slice 1; +6 = 4 LM + 2 dispatch); oracle 82 total,
  75 / 7 (unchanged from slice 1; promotion lands in slice 3).
- Commit: `feat(stage-7b): slice 2 — emit Last-Modified header under --no-etag`
  (`a3a0aaf`).

## Slice 3 — IMS 304 short-circuit (D-018)

- [x] New IMS branch in
  `dispatch.rs::build_file_or_304:776-792`, sibling to the
  existing INM branch and inside the same `Range`-absent
  guard at `dispatch.rs:767`. Parses both
  `If-Modified-Since` and the MERGED `Last-Modified` via
  `httpdate::parse_http_date`; on `ims >= lm`, returns 304
  via the new shared helper.
- [x] New `not_modified_response()` helper at
  `dispatch.rs:797-802` extracted from the INM 304 path; both
  branches now call through the helper. Response shape: 304
  status, no body, no `Content-Type`, no validator-echo —
  mirrors `serve-handler/src/index.js:761-764` for the ETag
  path and extends symmetrically.
- [x] Malformed IMS or malformed LM falls through to 200
  (RFC 9111 §13.1.3 — recipients SHOULD ignore unparseable
  IMS). Whole-second comparison naturally falls out of the
  `httpdate` formatter/parser round-trip.
- [x] Symmetric with ETag/INM: reads the MERGED
  `Last-Modified` (after `apply_custom_headers`), so a user
  `serve.json#headers` rule that overrides or deletes
  `Last-Modified` drives the 304 decision against the
  override (or suppresses 304 entirely under
  `Last-Modified: null`).
- [x] **D-018 captured in code comments** at
  `dispatch.rs:736-748` (full text lands in
  `docs/reference/serve/decisions.md` in slice 4 — main
  agent's responsibility, NOT this change package).
- [x] Tests: 7 new in `dispatch::tests` (all green):
  `ims_exact_match_returns_304` (round-trip via captured
  LM), `ims_future_returns_304` (far-future static IMS,
  day-of-week cross-checked: 2099-01-01 is Thursday),
  `ims_past_returns_200_with_last_modified` (epoch IMS),
  `ims_malformed_returns_200` (unparseable IMS),
  `range_present_skips_ims_304_even_on_match` (7c precursor),
  `user_last_modified_null_rule_suppresses_304` (SRV-HDR-002
  prune deletes LM → no 304 possible),
  `user_last_modified_override_drives_304_decision` (custom
  LM rule wins; replay of override → 304, IMS predating
  override → 200). New `compile_last_modified_override`
  helper parallels the Stage-7a `compile_etag_override`.
- [x] Probe partition: `runner.l0` in
  `last-modified-roundtrip.json` gains
  `clean: [first_get, ims_past, ims_malformed, ims_on_404]`,
  `divergent: [ims_exact, ims_future]`,
  `bodyMayDiffer: [ims_on_404]`. The four clean requests
  promote to dual-target; the two divergent requests stay
  reference-only with irserve coverage in `dispatch::tests`;
  `ims_on_404` runs both sides on status + headers but ignores
  the synthetic HTML body content (D-002).
- [x] **Day-of-week bug fix.** `Fri, 01 Jan 2099` → `Thu, 01
  Jan 2099` in `ims_future` and `ims_on_404` IMS headers
  (slice 0 had it wrong; reference is IMS-inert so the
  mismatch was undetectable under `target=reference`, but
  irserve's strict `httpdate::parse_http_date` rejected the
  malformed day-of-week per RFC 7231 and fell through to 200,
  breaking the matching unit test). Snapshot re-recorded
  (only the `if-modified-since` request-header string
  changed; the response stays 200 since the reference is
  IMS-inert).
- [x] Verify: `cargo test --workspace --lib` 278/278 green
  (was 271; +7 from this slice); `cargo test --test oracle`
  82 total, 76 passed, 6 skipped, 0 failed (was 75 / 7); both
  probe targets `--snapshot=verify` green.
- Commit: `feat(stage-7b): slice 3 — 304 short-circuit on If-Modified-Since (D-018)`
  (`40a58bc`).

## Slice 4 — Spec deltas + meta (this slice)

- [x] Author `openspec/changes/012-last-modified/`:
  - [x] `proposal.md` — closes SRV-CLI-013, SRV-CACHE-002,
    SRV-CACHE-003, Q-009; out-of-scope list mirrors the
    kickoff plan's 10-item pre-stage list.
  - [x] `design.md` — §1 mutex, §2 where LM is computed,
    §3 IMS 304 (D-018 adaptation), §4 `--no-etag` CLI flag,
    §5 probe partition + day-of-week bug, §6 methodological
    signals, §7 verification.
  - [x] `tasks.md` (this file).
  - [x] `specs/http-cache/spec.md` — ADDED Requirement for
    SRV-CACHE-002 / SRV-CACHE-003 / SRV-CLI-013 (consolidated)
    with multiple Scenarios; MODIFIED note on the Stage 7a
    SRV-CACHE-001 Requirement adding the ETag/Last-Modified
    mutex paragraph; Compatibility notes section.
- [ ] **Main agent only (NOT this subagent):**
  - [ ] Mirror the delta into
    `openspec/specs/http-cache/spec.md` (the canonical merged
    spec) once the change package validates.
  - [ ] Append **D-018** to
    `docs/reference/serve/decisions.md` with the IMS 304
    adaptation rationale (anti-hallucination rule #5 framing,
    SRV-CACHE-003 status-taxonomy reasoning, symmetry with
    the ETag/INM path, user's plan-mode AskUserQuestion D1
    answer).
  - [ ] `README.md`: flip Stage 7b row to `done`; trim the
    "What is NOT yet observable" footer (drop
    `Last-Modified`, drop IMS semantics, drop `--no-etag`);
    add `--no-etag` + `Last-Modified` + IMS-304 curl demo to
    "Try IrServe".
  - [ ] Run `npx -y @fission-ai/openspec@latest validate
    --all --strict` and report any failures.
  - [ ] Commit (after main-agent review of subagent output):
    `docs(stage-7b): spec deltas + D-018 + oracle-matrix promotion + meta`.

## Validation

- `cargo test --workspace --lib` — **278/278** unit tests
  green at end of slice 3 (was 265 in slice 1; +6 in slice 2;
  +7 in slice 3). New tests across stages: 4
  (`last_modified.rs`) + 9 (`dispatch::tests` LM + IMS
  surface) = 13.
- Oracle harness: `target=irserve total=82 passed=76 skipped=6
  failed=0` at end of slice 3; reference 81/81 (the
  reference-only `divergent` requests count under reference
  but skip under irserve, hence the asymmetric totals).
  Promotion delta: +4 dual-target requests from
  `last-modified-roundtrip.json` clean partition.
- `node tools/probe/run.mjs last-modified-roundtrip
  --target=reference --snapshot=verify` — green.
- `node tools/probe/run.mjs last-modified-roundtrip
  --target=irserve --snapshot=verify` — green.
- `openspec validate --all --strict` — slice 4 final step
  (main agent runs).
