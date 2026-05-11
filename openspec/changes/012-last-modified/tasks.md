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
  initially; ORC-167/170/171/172 promoted to dual-target in
  slice 3 via the `clean` partition; ORC-168/169 stay
  reference-only as the `divergent` partition — irserve
  diverges to 304 per D-018).
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
  `dispatch.rs:758`) gains a `meta: Option<&Metadata>`
  parameter (post-`bytes`, pre-`header_rules`); computes both
  `etag_value` and `last_modified_value` from the same
  `serve_config.etag` predicate at `dispatch.rs:767-769`, so
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
  guard at `dispatch.rs:771`. Parses both
  `If-Modified-Since` and the MERGED `Last-Modified` via
  `httpdate::parse_http_date`; on `ims >= lm`, returns 304
  via the new shared helper.
- [x] New `not_modified_response()` helper at
  `dispatch.rs:814` extracted from the INM 304 path; both
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
  `dispatch.rs:710-756` (full text lands in
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
- [x] **Main agent (NOT the subagent):**
  - [x] Mirror the delta into
    `openspec/specs/http-cache/spec.md` (the canonical merged
    spec) once the change package validates.
  - [x] Append **D-018** to
    `docs/reference/serve/decisions.md` with the IMS 304
    adaptation rationale (anti-hallucination rule #5 framing,
    SRV-CACHE-003 status-taxonomy reasoning, symmetry with
    the ETag/INM path under the `etag: false` gate, user's
    plan-mode AskUserQuestion D1 answer).
  - [x] `README.md`: flip Stage 7b row to `done`; trim the
    "What is NOT yet observable" footer (drop
    `Last-Modified`/IMS); add `--no-etag` + `Last-Modified` +
    IMS-304 curl demo to "Try IrServe".
  - [x] Run `npx -y @fission-ai/openspec@latest validate
    --all --strict` and report any failures (21/21 passed,
    0 failed).
  - [x] Commit: `docs(stage-7b): spec deltas + D-018 +
    http-cache merge + README` (`9605bb5`).

## Codex review rounds

### Round 1 — P2 + P3 fixes (commit `13b66bb`)

- [x] **P2 — gate IMS branch on `etag: false`.** Slice 3's
  symmetric-with-ETag IMS implementation fired 304 even under
  default ETag when a user `serve.json#headers` rule supplied
  a `Last-Modified` on the merged response — diverging from
  reference (which returned 200) and from the inventory rule
  "When ETag is on, IMS is ignored." Added the
  `serve_config.etag == Some(false)` gate to the IMS branch
  in `dispatch.rs::build_file_or_304`; updated doc comment +
  spec.md (ADDED Requirement + Compatibility note) + canonical
  http-cache spec + D-018 in decisions.md. New unit test
  `dispatch::tests::etag_on_ignores_ims_even_with_user_lm_rule`
  pins the corner case.
- [x] **P3 — stage7_l3_capabilities.md row + Methodological
  signals.** Flipped the 7b row to past tense (DONE, with
  partition + D-018 citations); rewrote the "7b / IMS
  semantics (Q-009 closure)" bullet under "Methodological
  signals to watch for" to reflect the slice-0 probe outcome
  + D-018 + the round-1 gate.
- [x] **P3 — tasks.md slice 4 checkboxes.** Marked the
  main-agent slice-4 items as done; pinned the slice-4
  commit hash (`9605bb5`).
- [x] **P3 — remove unused `SystemTime` import** in
  `crates/irserve-core/src/last_modified.rs::tests`. Build
  warning fixed.
- [x] **P3 — fix stale "ORC-167/168/169 promoted to
  dual-target" in tasks.md slice 0.** Slice 3 promoted
  ORC-167/170/171/172 via the `clean` partition;
  ORC-168/169 stay reference-only in `divergent`. Line 31-34
  rewritten to match the actual partition split. The
  oracle-matrix.md rows themselves were already correct;
  only tasks.md carried the stale summary.

### Round 2 — P3 fixes (commit `60058e4`)

- [x] **P3 — oracle-matrix.md Coverage gaps section.** Three
  entries (SRV-CLI-013, SRV-CACHE-002, SRV-CACHE-003) flipped
  from "no probe / probe gap / unknown" to strike-through
  "closed in Stage 7b" with the ORC-167..172 cross-link and
  the D-018 citation for SRV-CACHE-003's irserve adaptation.
- [x] **P3 — proposal.md stale ORC partition.** Same fix as the
  round-1 tasks.md edit, applied to proposal.md L116:
  `ORC-167/170/171/172` clean dual-target,
  `ORC-168/169` reference-only `divergent` per D-018.
- [x] **P3 — stale `dispatch.rs:NNN` citations** after the
  round-1 guard insertion. Updated across 5 files
  (canonical spec, delta spec, proposal, design, tasks +
  D-018 entry): `dispatch.rs:754→:758`, `:763-765→:767-769`,
  `:776-792→:780-809`, `:797-802→:814`, `:797→:814`,
  `:767→:771`, `:736-748→:710-756`. The 011-etag-conditional
  package's historical citations stay untouched (closed
  change record).

### Round 3 — P3 fixes (commit `cef7185`)

- [x] **P3 — compatibility-levels.md L61 + L92 stale.** L61
  ("`Last-Modified`/IMS — status `unknown`, see Q-009")
  rewritten to reflect Stage 7b's outcome: SRV-CACHE-002
  promoted to `verified`, SRV-CACHE-003 `verified` for the
  reference path and `adapted` for irserve per D-018, Q-009
  closed. L92 ("Coverage gaps / `--no-etag` / Last-Modified
  path — no probe") flipped to strike-through closed in
  Stage 7b with the ORC-167..172 + D-018 cross-link.
- [x] **P3 — inventory.md L398/1400/1423 stale evidence
  fields.** SRV-CLI-013 + SRV-CACHE-002 promoted
  `accepted` → `verified` (ORC-167 exercises both end-to-end
  via `last-modified-roundtrip.json#first_get` under
  `serveArgs: ["--no-etag"]`). SRV-CACHE-003 (already
  `verified` from slice 0) gains an explicit Oracle line
  citing ORC-167..172 + the irserve-side `dispatch::tests`
  coverage. "Probe: not run for this branch" /
  "Oracle test: planned" replaced with the actual probe +
  ORC anchors. Status taxonomy applied literally as
  README §Anti-hallucination rules #2 prescribes.
- [x] **P3 — tasks.md L228 placeholder.** Replaced
  `commit <this commit>` with `13b66bb` (Round 1 commit hash).

### Round 4 — P3 fixes (commit `3437082`)

- [x] **P3 — propagate the SRV-CLI-013 / SRV-CACHE-002
  `accepted` → `verified` promotion across all docs.** Round 3
  flipped `inventory.md` but missed five sites carrying the
  old status text:
  - `openspec/specs/http-cache/spec.md:179` (canonical
    Evidence line) — both SRVs `accepted` → `verified`.
  - `openspec/changes/012-last-modified/specs/http-cache/spec.md:59`
    (delta) — same.
  - `openspec/changes/012-last-modified/proposal.md:19,25`
    (Why bullets) — phrased as `accepted → verified via
    ORC-167` (transitional phrasing documenting the
    promotion event, not a stale status).
  - `openspec/changes/012-last-modified/proposal.md:131`
    (Documentation updates bullet) — rewrote "stay accepted"
    to describe the round-3 promotion outcome + cite
    README §Anti-hallucination rules #2 as the taxonomy
    rationale.
  - `docs/reference/serve/compatibility-levels.md:61,92` —
    Level 3 list entry + Coverage gaps entry both updated
    to reflect the verified status.

The plan file `docs/features/0016_PLAN_stage7b_last_modified.md`
is intentionally left at the point-in-time `accepted` wording
it carried on commit; plan files are historical snapshots
(same convention as the 011-etag-conditional package's
historical citations).

### Round 5 — P3 fix (this commit)

- [x] **P3 — restore the `## Validation` heading clobbered by
  Round 4.** When the Round 4 section was inserted at the end
  of the round log, the existing `## Validation` heading was
  removed but its bullets were left in place — so the bullets
  visually attached to Round 4 and read as Round-4 validation
  output. The bullets also still cited slice-3 numbers
  (278/278) instead of the current totals. Heading restored
  below; counts refreshed to the post-round-4 state.

## Validation

Latest totals at end of Stage 7b (post-Codex round 4):

- `cargo test --workspace --lib` — **279/279** unit tests
  green. Stage-7b additions: 4 in `last_modified.rs` (slice 2)
  + 9 in `dispatch::tests` (2 LM-shape in slice 2, 7 IMS in
  slice 3) + 1 in `dispatch::tests` (`etag_on_ignores_ims_even_with_user_lm_rule`
  added in Codex round 1 P2) = 14 new tests across the stage.
- Oracle harness: `target=irserve total=82 passed=76 skipped=6
  failed=0`; reference 81/81. The reference-only `divergent`
  requests (`ims_exact`, `ims_future`) count under reference
  but are stripped from the irserve diff via the L0
  partition, hence the asymmetric totals. Promotion delta vs.
  slice 1: +4 dual-target requests from `last-modified-
  roundtrip.json` `clean` partition (slice 3).
- `node tools/probe/run.mjs last-modified-roundtrip
  --target=reference --snapshot=verify` — green.
- `node tools/probe/run.mjs last-modified-roundtrip
  --target=irserve --snapshot=verify` — green.
- `npx -y @fission-ai/openspec@latest validate --all --strict`
  — **21 passed, 0 failed** (12 change packages + 9 canonical
  spec capabilities). Run end of every Codex review round.
