# Tasks

This change is design-only. The Rust implementation tasks live in a
future change (`002-implement-strict-l0-runtime`) opened at Stage 5b.

## 1. Pre-authoring

- [x] 1.1 Confirm bootstrap `000-establish-serve-compatibility-baseline`
  is archived as `openspec/changes/archive/2026-05-08-000-establish-serve-compatibility-baseline/`
  and that `openspec/specs/{cli,config,static-files,routing,redirects,rewrites,security,directory-listing}/spec.md`
  exist as canonical specs (Pre-step A of Stage 5a)
- [x] 1.2 Confirm `docs/reference/serve/compatibility-levels.md` lists
  SRV-CLI-003 in the L1 bullet, not the L0 "Bind host and port" bullet
  (Pre-step B of Stage 5a)
- [x] 1.3 Append D-008 (First-slice strict-L0 cutoff) to
  `docs/reference/serve/decisions.md`

## 2. Author proposal.md

- [x] 2.1 Why — first implementation proposal; design before code
- [x] 2.2 What — table of the 8 strict-L0 SRVs (CLI-001/002/007/019,
  FILE-001/002/004/005) with status, level, spec pointer, and oracle
  evidence
- [x] 2.3 Scope (in scope: workspace skeleton, HTTP-stack pin, CLI
  surface, oracle mapping, D-008)
- [x] 2.4 Non-goals — every deferred SRV named explicitly with the
  capability it belongs to
- [x] 2.5 Risks — `ServeDir` mismatch with SRV-ROUT-006, `mime_guess` vs.
  FILE-004 charset suffix, default-port path has no probe
- [x] 2.6 Open assumptions — Q-001/002/003/004 stay open; per-Q line on
  how design touches them

## 3. Author design.md

- [x] 3.1 Crate layout — `crates/irserve` (bin) + `crates/irserve-core`
  (lib) workspace, with justification
- [x] 3.2 Dependencies — version pins for `axum 0.8`, `tokio 1`,
  `tower-http 0.6`, `bytes 1`, `mime_guess 2`, `clap 4`. Deferred
  dependencies named (`tracing`, `flate2`, `serde`)
- [x] 3.3 Why custom handler instead of `tower-http::ServeDir` — anchored
  to SRV-ROUT-006
- [x] 3.4 Request lifecycle — 13 numbered phases, table of which are
  wired in 5a vs deferred. Phase 10 (containment) wired despite
  SRV-SEC-001 being L2.
- [x] 3.5 CLI surface — table of accepted L0 flags + behavior of every
  deferred flag (rejection)
- [x] 3.6 Stage-5b oracle harness mapping — table of probe cases →
  SRV/ORC under `tools/probe/cases/`
- [x] 3.7 Open assumptions — Q-NNN list, none closed
- [x] 3.8 Spec delta — traceability-only MODIFIED for SRV-CLI-001 (rationale + scope)

## 4. Validate

- [x] 4.1 `npx -y @fission-ai/openspec@latest validate --all --strict
  --concurrency 12` exits 0
- [ ] 4.2 No accidental Rust artifacts: `git diff --stat` shows zero
  lines under `tools/probe/`, `third_party/`, or any `*.rs` / `Cargo.*`
  file
- [x] 4.3 Single delta only: `openspec/changes/001-port-minimal-static-server/specs/cli/spec.md`
  exists and contains exactly one MODIFIED Requirement (SRV-CLI-001),
  reproducing the original body verbatim plus an `Implementation:` line

## 5. Cross-check

- [ ] 5.1 Every SRV ID cited in `proposal.md` and `design.md` is present
  in `docs/reference/serve/inventory.md`
- [ ] 5.2 Every ORC ID cited resolves to a row in
  `docs/reference/serve/oracle-matrix.md`
- [ ] 5.3 Every D-NNN cited resolves to an entry in
  `docs/reference/serve/decisions.md` (D-001..D-008, with D-008 being
  the new entry from this stage)
- [ ] 5.4 Every Q-NNN cited resolves to an entry in
  `docs/reference/serve/open-questions.md`; none of those entries was
  modified by Stage 5a
- [ ] 5.5 Every probe case named in `design.md` § 6 exists under
  `tools/probe/cases/` and `tools/probe/snapshots/`

## 6. Stage-5b prep (informational; not closed by change 001)

These items are out-of-scope for this change but flagged here so the
Stage-5b implementer does not have to re-derive them:

- [ ] 6.1 Add `tools/probe/cases/default-port.json` (omits `--listen`)
  to promote SRV-CLI-001 from `accepted` to `verified`
- [ ] 6.2 Adapt `tools/probe/run.js` to launch `irserve` in addition to
  `node third_party/serve/build/main.js`, selectable via env var
- [ ] 6.3 Author Stage-5b change `002-implement-strict-l0-runtime` with
  the actual Rust module structure, `Cargo.toml`, and `tests/oracle/`
  scaffolding
- [ ] 6.4 Pin Cargo dependencies to current-stable versions at 5b
  authoring time (re-verify the working pins in `design.md` § 2)

## 7. Stage map update

- [ ] 7.1 Mark `Stage 5a` as `Done.` in `README.md` Status block (line ≈20)
- [ ] 7.2 Update `README.md` Stage map row 5a status `todo` → `done`
