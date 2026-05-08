# Tasks

## 1. Pre-authoring

- [x] 1.1 Confirm reserved capability namespaces in `openspec/AGENTS.md`
- [x] 1.2 Re-read `docs/reference/serve/decisions.md` D-001 through D-007
- [x] 1.3 Re-read `docs/reference/serve/compatibility-levels.md` to confirm L0–L2 cutoff

## 2. Author capability deltas

- [x] 2.1 `specs/cli/spec.md` (13 SRVs)
- [x] 2.2 `specs/config/spec.md` (2 SRVs)
- [x] 2.3 `specs/static-files/spec.md` (5 SRVs)
- [x] 2.4 `specs/routing/spec.md` (6 SRVs, including SRV-ROUT-006 precedence)
- [x] 2.5 `specs/redirects/spec.md` (3 SRVs)
- [x] 2.6 `specs/rewrites/spec.md` (1 SRV)
- [x] 2.7 `specs/security/spec.md` (2 SRVs)
- [x] 2.8 `specs/directory-listing/spec.md` (3 SRVs, including D-007 reference)

## 3. Author change-level artifacts

- [x] 3.1 `proposal.md` (intent, scope, non-goals, risks)
- [x] 3.2 `design.md` (authoring methodology, evidence gaps, adaptation references, archive plan)

## 4. Side updates

- [x] 4.1 `openspec/AGENTS.md` — drop `path-resolution`, add `cors` to the reserved namespace list
- [x] 4.2 `openspec/AGENTS.md` — add a one-line OpenSpec CLI install pointer
- [x] 4.3 `README.md` — stage map row 4 status: `todo` → `done`

## 5. Validate

- [x] 5.1 `npx @fission-ai/openspec validate --all --strict --concurrency 12` exits 0
- [x] 5.2 Cross-check: every `Evidence: SRV-...` line cites an SRV present in `docs/reference/serve/inventory.md` (35 unique SRVs, all present)
- [x] 5.3 Cross-check: every ORC ID cited resolves to a row in `docs/reference/serve/oracle-matrix.md` (49 unique ORCs, all present)
- [x] 5.4 Cross-check: no SRV cited has status `deferred`, `unknown`, or `rejected`
- [x] 5.5 Cross-check: no SRV cited is at level L3 or L4
- [x] 5.6 Confirm spec bodies contain no source-language references (no Rust types, no `serve-handler` source line numbers, no JS module paths beyond evidence citations)

## 6. Final review pre-commit

- [x] 6.1 `git diff --name-only` shows changes only under `openspec/` and `README.md`
- [x] 6.2 No new dependencies committed (no new `package.json` at repo root, no `tools/probe/package.json` modifications)
- [x] 6.3 `openspec/specs/` unchanged from Stage 0 (`.gitkeep` files only)
- [x] 6.4 No Rust code, no probe-cases changes, no `third_party/` changes
