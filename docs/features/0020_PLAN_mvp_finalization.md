# Plan: MVP finalization for irServe

## Context

irServe has reached its declared MVP scope (L2) and the L3 stretch goal. Stage 7e is the last `done` row in the README stage map; working tree is clean on `main`; `cargo test --workspace --lib` = 362/0/0; `cargo test --test oracle` = 81/2/0. L4 (symlinks, TLS, Windows path quirks) is explicitly deferred per `docs/reference/serve/compatibility-levels.md` §L4 and Stage 7 “Out of scope”.

The experiment’s primary deliverable is the methodology, not the binary (README §1, AGENTS.md preamble). Therefore finalization must (a) freeze the codebase as a tagged release with installable instructions, (b) archive the OpenSpec change packages that comprised the work, and (c) capture the methodology lessons in a single retrospective. In parallel, the README and `AGENTS.md` should be pruned: README accumulated ~190 lines of curl-by-feature “Try IrServe” that grew per-stage and now duplicates `openspec/specs/*` + probe snapshots; AGENTS.md grew sub-sections (notably “Subagent delegation triggers”) that belong in the retrospective.

User decisions locked before this plan:
- Version: `0.1.0` (semver-appropriate “first usable, sub-1.0”).
- Retrospective format: terse executive summary (~150–250 lines).
- README: drop the curl wall; keep one minimal example; move a curated subset to `docs/user-guide.md`.
- Install pathway: `cargo install --git https://github.com/serge-sotnyk/irServe`. No crates.io publish. No CI.

## Recommended approach

Five workstreams; each ends with a single commit. Run them in the listed order — workstream A bumps versions referenced by D and E; workstream B reshapes paths that the retrospective C may want to cite.

### A. Versioning + CHANGELOG

Bump both crates from `0.0.1` to `0.1.0`; do so via the workspace package table so future bumps touch one place.

Files to modify:
- `Cargo.toml` — add `version = "0.1.0"` to `[workspace.package]`.
- `crates/irserve/Cargo.toml` — change `version = "0.0.1"` to `version.workspace = true`.
- `crates/irserve-core/Cargo.toml` — same.

Files to create:
- `CHANGELOG.md` at repo root. Follow keep-a-changelog format (already familiar to Rust users):
  - `## [0.1.0] - 2026-05-15` heading with subsections `### Added` (L0/L1/L2/L3 capability bullets sourced from README stage rows 5b → 7e), `### Known limitations` (L4 deferred surface; D-018/D-019/D-020 adapted divergences).
  - `## [Unreleased]` placeholder above it.

After commits land, tag locally: `git tag -a v0.1.0 -m "MVP (L0–L3)"`. **Do NOT push the tag** without explicit user authorization (push is a shared-state action).

Verification:
- `cargo build --workspace --release` succeeds with new versions.
- `cargo test --workspace` green.
- `target/release/irserve --version` prints `0.1.0`.

### B. README + AGENTS.md prune; user-guide extraction

Current README has a “Status” section listing every sub-stage as `done` (rows 5b–7e), then ~190 lines of curl-per-feature under “Try IrServe (post-6h)”. The curl wall is duplicate evidence: every behavior it demonstrates is already pinned in `openspec/specs/*` + `tools/probe/snapshots/*`. Drop it.

Files to modify:
- `README.md`:
  - Replace the per-sub-stage `Stage Na — ... done` bullet list under “Status” with a single line: `MVP shipped as v0.1.0 (2026-05-15). Levels L0–L3 implemented; L4 deferred. See [CHANGELOG.md](./CHANGELOG.md).` Keep the stage-map table — it’s the methodology artifact.
  - Replace the “Try IrServe (post-6h)” section with a 12–15-line “Try it” block: one `cargo install --git ...` line, one fixture-creation line, one `curl http://127.0.0.1:3010/` line, then a one-paragraph pointer: “For per-feature usage examples, see [docs/user-guide.md](./docs/user-guide.md). For the full behavior contract, see [openspec/specs/](./openspec/specs/).”
  - Update the “Getting started” section to make the cargo-install path first-class for *users* and the submodule-clone path second-class (labeled “For development / contributors”). The cargo-install path should NOT require submodules (the oracle suite is dev-only).
  - Drop the final “What is NOT yet observable” bullet about symlinks/TLS — already covered in compatibility-levels.md and the new CHANGELOG “Known limitations”.

Files to create:
- `docs/user-guide.md` — curated cookbook. Trim from current ~190 lines to ~80–100 lines by:
  - Grouping by capability area (Routing, Headers & caching, Range, Compression, CLI flags) rather than by stage.
  - One example per feature, not three.
  - Drop everything that requires a serve.json edit just to demonstrate (those configurations are in `openspec/specs/*` examples and `tools/probe/cases/*`).

Files to modify:
- `AGENTS.md`:
  - Drop the “Subagent delegation triggers” section entirely (45–47). Its content is a Stage-6/7 methodology learning and belongs in the retrospective. Replace with a one-line pointer to the retrospective.
  - Trim the “Notes for agents” bullet on OpenSpec install — by the time someone reads this post-archive, OpenSpec is no longer producing new change packages.
  - Keep: Language policy; anti-hallucination one-liners (these are *the* methodology distilled — already the right shape); Pointers; Markdown style.
  - Add (one new bullet under “Pointers”): `Methodology retrospective → docs/methodology_retrospective.md.`

Verification:
- `cargo install --git <local-file-url>` or `cargo install --path crates/irserve` produces a working binary that passes the README smoke-test curl. (No need to fully test the submodule-free build pathway separately — `cargo install` from a path automatically excludes `third_party/` since it’s not part of the crate.)
- Manual link-check: every relative link in README and AGENTS.md resolves.

### C. Methodology retrospective

Single new file: `docs/methodology_retrospective.md`. Target length 150–250 lines.

Structure:
1. **Outcome** (3–5 lines). L2 + L3 shipped; binary works; methodology is the deliverable.
2. **Anti-hallucination rules: per-rule verdict table.** 10 rows × 3 columns (rule, verdict, one citation). Verdict in `{worked / saved-tokens / fired-late / under-specified}`. Citation = one D-NNN or one stage-number where the rule observably mattered or failed. Source material: `docs/reference/serve/decisions.md` (21 D-NNN), `docs/reference/serve/open-questions.md` (13 Q-NNN), stage-7 plan files’ “Methodological signals to watch for” sections.
3. **Cost shape.** Token estimates per stage cluster (Stage 1–4 research; Stage 5a–5b first slice; Stage 6a–6h L2 build; Stage 7a–7e L3 build). Numbers come from the user’s session experience, not from any persisted log — flag as informal in the doc.
4. **Subagent delegation outcomes.** Move the two bullets currently in `AGENTS.md` §“Subagent delegation triggers” here, verbatim, then add a one-line verdict each.
5. **Process changes worth keeping for the next port.** 5–10 bullets. Examples surfaced by D-NNN inspection: probe-first when mirroring a library (rule 8); explicit per-stage out-of-scope list (rule 10); pause-after-3-rounds (rule 9) and the parity-scope declaration pattern from Stages 6d and 7e.
6. **What did NOT work / overhead.** Stage 6d ran 12 review rounds — root cause notes (path-to-regexp + minimatch quirks per `inventory.md` Q-007/Q-012). Stage 7e ran 11 review rounds — root cause notes (compression-middleware undocumented defaults per Q-002 / D-020).

Critical files to read while writing:
- `docs/reference/serve/decisions.md` (full, all 21 D-NNN).
- `docs/reference/serve/open-questions.md` (full, 13 Q-NNN).
- `docs/stage6_l1_l2_capabilities.md` + `docs/stage7_l3_capabilities.md` (existing methodology signals sections).
- Recent commit log for Stages 6d and 7e (`git log --oneline -- docs/features/0010_*` and `docs/features/0019_*`).
- `AGENTS.md` §“Subagent delegation triggers” (text to move).

Verification: a reader who has never opened irServe should leave with (a) a clear yes/no on whether each rule was worth its overhead, (b) the three or four most expensive lessons. Aim for the doc to fit on roughly 3 printed pages.

### D. Archive OpenSpec changes 001–015

Pattern set by Stage 4 baseline: `openspec/changes/archive/YYYY-MM-DD-NNN-<short-name>/`. The baseline used a single date (the archive date), not the change’s authoring date — mirror that.

Bash (single archive date `2026-05-15`):
```
git mv openspec/changes/001-port-minimal-static-server     openspec/changes/archive/2026-05-15-001-port-minimal-static-server
git mv openspec/changes/002-implement-strict-l0-runtime    openspec/changes/archive/2026-05-15-002-implement-strict-l0-runtime
# … same for 003 through 015
```

Files to grep-and-update after the move (paths that reference unarchived change directories):
- `README.md` — stage-map table rows (rightmost column) cite e.g. `openspec/changes/003-load-serve-json`. Either point to the archive path or drop the path (the change is preserved in `git log`). Recommendation: update to archive path for traceability.
- `docs/stage6_l1_l2_capabilities.md` and `docs/stage7_l3_capabilities.md` — same kind of references in the decomposition tables.
- `openspec/AGENTS.md` (if it cites individual changes) — usually doesn’t, but verify.

Verification:
- `npx -y @fission-ai/openspec@latest validate --all --strict` runs clean (does NOT validate archived changes by design — but should still pass).
- `ls openspec/changes/` shows only `archive/`.
- `git log --follow` resolves on a moved file (sanity check on `git mv` integrity).

### E. Tag, commit, and ship

One commit per workstream, in order A → B → D → C → E. Workstream E is the tag-and-stop commit:
- Final commit titled `chore: finalize MVP (v0.1.0)`. Updates only the README status line and the CHANGELOG release date if anything drifted.
- `git tag -a v0.1.0 -m "MVP (L0–L3): see CHANGELOG.md"`.

Order rationale: A first (other commits inherit the version); B before C/D because C cites paths that B may shift; D before E because the tag should land on a tree that is already archive-clean.

**Do NOT push** the tag or branch unless the user authorizes — pushing is a shared-state action and `git tag --delete` requires force-push to undo on remote.

## Verification (end-to-end, post-E)

Run each from a clean shell on Windows PowerShell (primary dev) and confirm at least the first three under Bash too (cross-platform commitment per AGENTS.md):

1. `cargo build --workspace --release` — succeeds.
2. `cargo test --workspace --lib` — 362 passed.
3. `cargo test --test oracle` — 81 passed / 2 skipped / 0 failed.
4. `target/release/irserve --version` — prints `irserve 0.1.0`.
5. From a tempdir: `cargo install --path <repo>/crates/irserve --locked` produces a binary; `irserve --help` runs; `irserve --listen 3010 .` serves files. (Locked install simulates the `--git` pathway minus network.)
6. `npx -y @fission-ai/openspec@latest validate --all --strict` — clean.
7. Manual link audit: every relative link in `README.md`, `AGENTS.md`, `CHANGELOG.md`, `docs/user-guide.md`, `docs/methodology_retrospective.md` resolves to an existing file.
8. `git log --oneline -10` — five new commits, last is the tag commit; `git tag -l` shows `v0.1.0`.

## Out of scope (explicitly NOT in this finalization)

- crates.io publish (user excluded).
- CI workflows / GitHub Actions (user excluded).
- L4 capability work (symlinks, TLS, Windows path quirks) — out of scope per compatibility-levels.md.
- New OpenSpec changes — no spec work; archival only.
- Reformatting `docs/reference/serve/inventory.md` or oracle-matrix.md — these are research artifacts, frozen in current shape.
- Renaming the `irserve` binary or the `irserve-core` crate.
