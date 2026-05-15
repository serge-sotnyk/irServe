# Plan: complete OpenSpec consolidation (post-v0.1.0)

## Context

The MVP finalization plan `docs/features/0020_PLAN_mvp_finalization.md` shipped five workstreams: version bump (A), README/AGENTS prune + user-guide extraction (B), methodology retrospective (C), archival of changes 001–015 (D), and tag/ship (E). It executed in commits `3c65ea5 → ab6672c → 5891054 → 1143718 → c848b3e` and produced tag `v0.1.0`.

Workstream D `git mv`-ed the 15 change directories to `openspec/changes/archive/2026-05-15-NNN-*` but **assumed** the OpenSpec `sync` step (merging each change's delta into `openspec/specs/<capability>/spec.md`) had been completed by each Stage 6/7 commit. That assumption was correct for 14 of the 15 changes — `npx @fission-ai/openspec validate --all --strict` passes (11/11 specs green). It was wrong for **change `2026-05-15-008-error-pages-and-security`**:

| Drift | Location | What's missing |
|---|---|---|
| **Capability never created** | `openspec/specs/headers/` | Both `### Requirement: Custom response headers from serve.json's headers array` and `### Requirement: \`value: null\` deletes a previously-applied header` plus their 6 scenarios and ~140 lines of normative body — entire 222-line delta at `openspec/changes/archive/2026-05-15-008-error-pages-and-security/specs/headers/spec.md`. The behavior IS shipped in `crates/irserve-core/src/custom_headers.rs` and tested by oracle ORC-031, ORC-053, ORC-159..162, so the binary contract is intact — but the source-of-truth catalogue does not document it. `openspec validate` does not catch this because validation only checks what's present, not what's expected from archived deltas. |
| **Compatibility notes dropped on merge** | `openspec/specs/security/spec.md` | Trailing `## Compatibility notes` section from the 008 delta (lines 84–103: D-015 un-defer note + `bodyMayDiffer` traversal anchor). The `MODIFIED Requirements` text itself WAS merged correctly. |
| **Compatibility notes dropped on merge** | `openspec/specs/static-files/spec.md` | Trailing `## Compatibility notes` section from the 008 delta (lines 30–47: D-015 un-defer + D-003 markup-parity notes). The `MODIFIED Requirements` text itself WAS merged correctly. |

The 14 other archived changes' deltas (001–007, 009–015) are fully synced — verified by agent audit that walked every `### Requirement:` heading and every non-requirement section in each delta. Change 008 is the sole outlier.

This is a contract / documentation defect, not a behavior defect. No code or binary change. No version bump. One docs-only commit; addendum to the methodology retrospective; CHANGELOG `[Unreleased]` line.

User decisions locked before this plan:

- **Form.** Direct edit of `openspec/specs/` in a single `docs(openspec): …` commit (no formal change package). Change 008 is the authoritative source — wrapping a sync fix in a fresh `propose → apply → sync → archive` ceremony would be theater.
- **Retrospective addendum.** Add a short bullet under `## What did NOT work / overhead`; the underlying lesson (sync step has no automated cross-check) is methodology-relevant.
- **Drift-checker script.** Deferred. No new change packages are planned post-v0.1.0, so an automated archive↔specs differ has no audience. Track as a follow-up if a v0.2.0 cycle starts.

## Recommended approach

Three small edits + one commit. Order matters only because the `headers` capability needs to exist before the retrospective references it.

### 1. Create `openspec/specs/headers/spec.md`

Source: `openspec/changes/archive/2026-05-15-008-error-pages-and-security/specs/headers/spec.md` (222 lines).

Mechanical transformation from delta → source-of-truth:

- Line 1: `# Delta for headers` → `# headers Specification`.
- Line 3: `## ADDED Requirements` → `## Requirements`.
- Insert a `## Purpose` section between the title and `## Requirements`, three to five lines. Suggested wording, derived from the requirement bodies (no invention):

  > Custom response headers driven by `serve.json`'s `headers` array. Rules compile in minimatch-only mode (no path-to-regexp routing), accumulate in declaration order with case-insensitive last-write-wins semantics, layer over default response headers on success and on non-JSON 4xx responses, and skip 3xx redirects entirely. `value: null` entries prune a header whose final accumulated state is null, mirroring `serve-handler`'s two-stage `Object.assign`-then-prune merge.

- Everything from line 5 onward (both `### Requirement:` blocks plus their scenarios, evidence lines, implementation notes) carries over verbatim. Do NOT rewrite — the delta text is the contract, and it cites D-015, SRV-HDR-001/002, the exact `index.js:NNN` references, and the existing `crates/irserve-core/src/custom_headers.rs` implementation seam. Edits to phrasing would force a re-review against the reference.

### 2. Append `## Compatibility notes` to `openspec/specs/security/spec.md`

Current last line: 89. Append a blank line then the section from `openspec/changes/archive/2026-05-15-008-error-pages-and-security/specs/security/spec.md` lines 84–103 verbatim:

```
## Compatibility notes

- **D-015 (irserve un-defer).** Stage 6f delivers the full SRV-SEC-001
  surface. …

- **`bodyMayDiffer` on traversal anchors.** D-003 stands; irserve
  emits a generic `<h1>400 Bad Request</h1>\n` body …
```

### 3. Append `## Compatibility notes` to `openspec/specs/static-files/spec.md`

Current last line: 143. Append a blank line then the section from `openspec/changes/archive/2026-05-15-008-error-pages-and-security/specs/static-files/spec.md` lines 30–47 verbatim:

```
## Compatibility notes

- **D-015 (irserve un-defer).** Stage 6f wires the `<status>.html`
  lookup at the served root inside `error_response`. …

- **D-003 (markup parity).** The HTML body of the fallback (no
  `<status>.html`) deliberately diverges from the reference's full
  `errorTemplate` HTML. …
```

### 4. CHANGELOG `[Unreleased]` entry

`CHANGELOG.md` currently has:

```
## [Unreleased]

Nothing yet.
```

Replace `Nothing yet.` with a `### Fixed` subsection:

```
### Fixed

- OpenSpec consolidation: created missing `openspec/specs/headers/` capability and restored `## Compatibility notes` sections on `security` and `static-files` specs. The corresponding deltas from change `2026-05-15-008-error-pages-and-security` had not been synced into the source-of-truth before archival. Behavior, oracle probes, and `cargo test` results unchanged — this is a spec catalogue fix only.
```

### 5. Methodology retrospective addendum

`docs/methodology_retrospective.md` — append a new bullet to the existing `## What did NOT work / overhead` section (currently at line 81). 5–10 lines, terse, factual:

```
- **OpenSpec sync step had no cross-check.** Change 008 (`error-pages-and-security`) introduced a new `headers` capability and added `## Compatibility notes` sections to `security` and `static-files`. The capability and both notes blocks were never copied into `openspec/specs/` before archival. `openspec validate` did not catch it (it checks what's present, not what's expected from archived deltas), and the gap survived through release and a Codex review pass. Discovered post-v0.1.0 by manual audit. Fixed under `[Unreleased]` in CHANGELOG. Lesson for the next port: either run sync as a scripted `git mv`-and-merge step or add a diff-archive-vs-specs check to the validate command.
```

### 6. Single commit

Title: `docs(openspec): sync change 008 deltas — headers capability + compat notes`

Body cites the audit table from §Context. No code changes, no version bump, no tag. Same shape as commit `5891054` (`docs(openspec): archive changes 001-015 under archive/2026-05-15-NNN-*`).

## Files

**Create:**

- `openspec/specs/headers/spec.md` — derived mechanically from `openspec/changes/archive/2026-05-15-008-error-pages-and-security/specs/headers/spec.md` (delta → source-of-truth transformation per §1).

**Modify:**

- `openspec/specs/security/spec.md` — append `## Compatibility notes` block.
- `openspec/specs/static-files/spec.md` — append `## Compatibility notes` block.
- `CHANGELOG.md` — `[Unreleased]` → `### Fixed` entry.
- `docs/methodology_retrospective.md` — one bullet under `## What did NOT work / overhead`.

**Do NOT modify:**

- `openspec/changes/archive/2026-05-15-008-…/` — archive is immutable.
- `crates/irserve*/` — no behavior change.
- `Cargo.toml`, version fields — no version bump.
- `openspec/AGENTS.md` — `headers` is already in the reserved capability list (line 56 of the file).
- `README.md` — stage map row for Stage 6f already cites the archived path; no relink needed.
- `git tag v0.1.0` — the released contract is unchanged at the binary surface; the missing spec text was always part of the intent of v0.1.0, not a new contract.

## Verification

1. `ls openspec/specs/headers/spec.md` — exists; > 200 lines.
2. `tail -25 openspec/specs/security/spec.md` — shows new `## Compatibility notes` block.
3. `tail -22 openspec/specs/static-files/spec.md` — shows new `## Compatibility notes` block.
4. `npx -y @fission-ai/openspec@latest validate --all --strict --concurrency 12` — `12 passed, 0 failed` (was 11, now includes `headers`).
5. `cargo test --workspace --lib` — 362/0/0 unchanged (sanity check that no source files were accidentally touched).
6. `cargo test --test oracle` — 81/2/0 unchanged.
7. Manual spot-check: open `openspec/specs/headers/spec.md` and confirm both `### Requirement:` headings, all 6 `#### Scenario:` blocks, and the SRV-HDR-001/SRV-HDR-002 + ORC-031/053/159–162 citations are present.
8. `git status` after commit — clean. `git log --oneline -1` shows the new commit. No tag operation.
9. `grep -r "specs/headers" --include="*.md"` — confirm CHANGELOG, retrospective, and the existing 22 doc references all still resolve.

## Out of scope (explicitly NOT in this finalization)

- crates.io publish, CI workflows, L4 capability work — same as 0020 plan.
- New behavior, new oracle probes, new D-NNN decisions.
- `tools/check-openspec-sync.mjs` drift-checker — deferred (no new change packages planned post-v0.1.0).
- Reformatting `docs/reference/serve/inventory.md` or `oracle-matrix.md` — research artifacts, frozen.
- Re-tagging `v0.1.0` — the binary contract is unchanged; the local tag stays where `c848b3e` placed it.
- A new change package wrapping the fix — user-declined as ceremony.
- Backfilling the same `## Compatibility notes` pattern to specs that never had one in their delta (e.g., http-cache, routing) — those changes' deltas didn't carry notes sections, so there is nothing to merge.
