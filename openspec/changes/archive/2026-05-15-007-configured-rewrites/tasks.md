# Tasks: Configured rewrites + `--single` SPA fallback

Six iterative slices, one commit per green slice (per the project's
"ask between slices" rule). Slice 6 is the meta slice and lands
last so spec deltas reflect what was actually shipped.

## Slice 1 — Extract `path_pattern.rs` from `redirects.rs`

- [x] Create `crates/irserve-core/src/path_pattern.rs` with the
  shared matcher kernel (Matcher, DestTemplate, PatSeg,
  GlobFallback + helpers + `compile_source_regex` +
  `compile_dest_template` + `slasher` + `path_posix_resolve`).
- [x] Drop the moved items from `redirects.rs`; replace with
  `use crate::path_pattern::Matcher;` plus a single re-export
  `pub use crate::path_pattern::CompileError;`.
- [x] Add `mod path_pattern;` to `lib.rs` (pub(crate) surface).
- [x] Verify: `cargo test -p irserve-core` (187/187) +
  `cargo test --test oracle` (all 6d ORCs stay green).
- Commit: `refactor(stage-6e): extract path_pattern.rs from redirects.rs`.

## Slice 2 — Phase 7 wiring + literal/glob rewrites

- [x] Lift `compile_one`'s body into `Matcher::compile` on
  `path_pattern.rs`. Reduce `redirects::compile_one` and
  `RedirectRuleCompiled::try_match` to thin wrappers.
- [x] Create `crates/irserve-core/src/rewrites.rs`:
  `RewriteRuleCompiled`, `InvalidRewrite`, `compile_rules`,
  `compute_configured_rewrites` (single-pass first-match-wins).
- [x] Add `mod rewrites;` to `lib.rs`.
- [x] In `server.rs`: alias both `compile_rules` imports;
  compile rewrite rules at startup; thread
  `&[RewriteRuleCompiled]` into `dispatch`.
- [x] In `dispatch.rs`: replace the phase-7 stub with the
  pre-stat asymmetry block (extensionless: rewrites first;
  has-extension: pre-stat first, rewrites only on miss).
- [x] Add `runner.l0.clean: ["spa_fallback"]` to
  `tools/probe/cases/rewrites-segment.json`.
- [x] Verify: `rewrites-segment#spa_fallback` (ORC-029) green
  under `target=irserve`.
- Commit: `feat(stage-6e): wire phase 7 with literal/glob rewrites`.

## Slice 3 — `:name` patterns + destination interpolation

- [x] Promote `segment_rewrite` to `runner.l0.clean` in
  `rewrites-segment.json`.
- [x] Verify: `rewrites-segment#segment_rewrite` (ORC-028) green.
- Commit: `feat(stage-6e): add :name rewrite anchor to L0 partition`.

(No new code in slice 3 — Pattern matcher with `:name` interpolation
already worked through `Matcher::compile` + `Matcher::try_match`.
The slice graduates the existing reference-verified anchor.)

## Slice 4 — Recursive chaining + depth cap

- [x] Replace `compute_configured_rewrites` with the recursive
  `apply_rewrites` (mirrors `index.js:91-117`'s splice + recurse
  pattern).
- [x] Add `REWRITE_DEPTH_CAP: usize = 64` and the graceful-clamp
  branch on overflow.
- [x] Add 8 unit tests in `rewrites.rs` covering: single-rule,
  chain-two-rules (both rule orders), two-rule cycle, self-loop,
  first-match-wins, depth-cap clamp.
- [x] Author probe `tools/probe/cases/rewrites-chain.json` with
  anchor `chain_a_to_b_to_c`. Record reference snapshot via
  `--snapshot=update --target=reference`.
- [x] Add `runner.l0.clean: ["chain_a_to_b_to_c"]`.
- [x] Verify: chain anchor green under `target=irserve`.
- Commit: `feat(stage-6e): recursive chaining with depth cap`.

## Slice 5 — `--single` CLI flag

- [x] Add `-s`/`--single` to clap in `crates/irserve/src/main.rs`.
- [x] In `main.rs`, when `--single` is set, prepend
  `RewriteRule { source: "**", destination: "/index.html" }` to
  `serve_config.rewrites` (mirrors `main.ts:78-90`).
- [x] Remove `-s`/`--single` from `L0_DEFERRED_FLAGS` in
  `tools/probe/run.mjs`; update the comment to cite `D-013`.
- [x] Author three new probes (each recorded against reference
  first):
  - `tools/probe/cases/single-flag.json` — `spa_root`,
    `spa_deep`, `spa_with_existing_html` (cleanUrls disabled in
    fixture to isolate phase-7 behavior).
  - `tools/probe/cases/single-with-redirect.json` —
    `redirect_wins_over_single` (proves phase 6 fires before
    phase 7).
  - `tools/probe/cases/single-with-rewrites.json` —
    `extensionless_user_path_shadowed_by_single` (pins prepend-
    position contract).
- [x] Promote all three probes' anchors to `runner.l0.clean`.
- [x] Verify: all new anchors green under `target=irserve`.
- Commit: `feat(stage-6e): wire --single SPA fallback (SRV-CLI-008)`.

## Slice 6 — Spec deltas + decisions log + meta

- [ ] Author `openspec/changes/archive/2026-05-15-007-configured-rewrites/`:
  - [x] `proposal.md`
  - [x] `design.md`
  - [x] `tasks.md` (this file)
  - [ ] `specs/rewrites/spec.md` — ADDED Requirements: rewrite
    chaining, recursion-depth cap, `--single` SPA fallback.
  - [ ] `specs/routing/spec.md` — un-defer redirects↔rewrites
    compose corner of SRV-ROUT-006; add new ORC IDs to oracle list.
  - [ ] `specs/cli/spec.md` — un-defer SRV-CLI-008 from D-008;
    add `single-flag.json` ORCs.
- [ ] `docs/reference/serve/decisions.md`: append `D-013`
  (un-defer SRV-RWRT-001 + SRV-CLI-008) and `D-014` (recursion-
  depth cap, irserve-only).
- [ ] `docs/reference/serve/inventory.md`: refresh SRV-RWRT-001
  and SRV-CLI-008 status notes (verified, oracle list extended).
- [ ] `docs/reference/serve/oracle-matrix.md`: append ORC-148
  (chain), ORC-149/150/151 (single-flag), ORC-152
  (single-with-redirect), ORC-153 (single-with-rewrites).
- [ ] `README.md`: flip Stage 6e row to `done`; update "What is
  NOT yet observable" + "Try IrServe" with rewrites + `--single`
  examples.
- [ ] `docs/stage6_l1_l2_capabilities.md`: flip 6e row to `done`.
- [ ] Run `npx -y @fission-ai/openspec@latest validate --all
  --strict`.
- Commit: `docs(stage-6e): spec deltas + decisions log + meta`.
