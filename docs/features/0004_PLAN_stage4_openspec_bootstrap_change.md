# Stage 4 — OpenSpec bootstrap change

## Context

Per `README.md` stage map, Stage 4 is the next stage: produce `openspec/changes/000-establish-serve-compatibility-baseline/` so that subsequent implementation proposals (Stage 5a+) have a contract to amend rather than authoring requirements ad-hoc against the inventory. Stages 1–3 produced research artifacts (52 SRV entries in `docs/reference/serve/inventory.md`, 59 ORC entries in `docs/reference/serve/oracle-matrix.md`, decisions/open-questions logs, committed probe snapshots in `tools/probe/snapshots/`). Stage 4 promotes the research-stable subset of those SRVs into OpenSpec capability specs without writing Rust code and without touching `tools/probe/`.

The bootstrap change is a *propose-only* delta: it adds requirements to capability namespaces that currently have empty `spec.md` placeholders. `openspec/specs/` stays empty after Stage 4; archiving (merging deltas into `openspec/specs/<capability>/spec.md`) is a separate explicit step at the user's request.

## Confirmed decisions

| # | Decision | Rationale |
|---|---|---|
| 1 | **Scope = L0–L2 only** (37 SRVs included, 15 excluded) | Matches MVP target per `README.md` and `compatibility-levels.md` ("Stage 4 only includes requirements up to the agreed compatibility level"). L3 (CACHE/CORS/HDR/RWRT-002/CLI-012/CLI-013) lands in a later change once implementation reaches that polish layer. |
| 2 | **Populate only `openspec/changes/000-…/`; `openspec/specs/` stays empty** | Clean OpenSpec lifecycle — propose → review → archive. Archiving is a separate user-controlled step. Reviewers see deltas, not duplicated content. |
| 3 | **Reserved namespace cleanup**: drop `path-resolution`, add `cors` | `path-resolution` has zero SRVs (multi-slash lives in `routing`, traversal in `security`). `cors` houses SRV-CORS-001 cleanly without folding into `headers`. Namespace stays reserved (no `spec.md` in this change) since SRV-CORS-001 is L3. |
| 4 | **Run `openspec validate --all --strict` locally**; do not commit deps | `npm i -g @fission-ai/openspec` (or `npx`). Catch structural errors before sending diff to Codex. README/AGENTS.md gets a one-line install pointer. No `package.json` at repo root. |

## In-scope SRVs (37)

Mapped to 8 capability namespaces; `headers`, `http-cache`, `symlinks`, `cors` stay reserved without `spec.md` in this change.

### `cli` — 13 SRVs
| SRV | Status | Level | Evidence (ORC) |
|---|---|---|---|
| SRV-CLI-001 | accepted | L0 | source-only (Q-001 captures default-port gap) |
| SRV-CLI-002 | verified | L0 | ORC-001 |
| SRV-CLI-003 | accepted | L1 | (transitive) |
| SRV-CLI-006 | accepted | L1 | (transitive) |
| SRV-CLI-007 | verified | L0 | ORC-001, ORC-062 |
| SRV-CLI-008 | verified | L2 | ORC-029 |
| SRV-CLI-009 | verified | L1 | ORC-006 |
| SRV-CLI-010 | verified | L1 | ORC-054, ORC-055 |
| SRV-CLI-011 | accepted (D-005) | L1 | unobservable by design |
| SRV-CLI-014 | adapted (D-002) | L1 | logging implementation-defined |
| SRV-CLI-015 | adapted (D-002) | L1 | logging implementation-defined |
| SRV-CLI-016 | accepted | L1 | happy-path only |
| SRV-CLI-019 | verified | L0 | ORC-060, ORC-061 |

### `config` — 2 SRVs
| SRV | Status | Level | Evidence (ORC) |
|---|---|---|---|
| SRV-CFG-001 | verified | L1 | ORC-006 |
| SRV-CFG-002 | accepted | L1 | piecewise via per-capability requirements |

### `static-files` — 5 SRVs
| SRV | Status | Level | Evidence (ORC) |
|---|---|---|---|
| SRV-FILE-001 | verified | L0 | ORC-001 |
| SRV-FILE-002 | verified | L0 | ORC-004, ORC-005, ORC-051, ORC-052, ORC-059 (D-003 limits HTML branch) |
| SRV-FILE-003 | verified | L1 | ORC-007, ORC-059 |
| SRV-FILE-004 | verified | L0 | ORC-003 |
| SRV-FILE-005 | verified | L0 | ORC-001 |

### `routing` — 6 SRVs
| SRV | Status | Level | Evidence (ORC) |
|---|---|---|---|
| SRV-ROUT-001 | verified | L2 | ORC-002, ORC-012, ORC-017, ORC-020, ORC-021, ORC-023, ORC-026, ORC-027 |
| SRV-ROUT-002 | verified | L2 | ORC-013, ORC-014, ORC-022, ORC-024 |
| SRV-ROUT-003 | verified | L2 | ORC-015, ORC-016 |
| SRV-ROUT-004 | verified | L2 | ORC-019 |
| SRV-ROUT-005 | verified | L2 | ORC-025, ORC-026, ORC-027 |
| SRV-ROUT-006 | verified | L2 | ORC-014, ORC-016, ORC-017, ORC-018, ORC-020, ORC-032, ORC-033 |

### `redirects` — 3 SRVs
| SRV | Status | Level | Evidence (ORC) |
|---|---|---|---|
| SRV-RDIR-001 | verified | L2 | ORC-030 |
| SRV-RDIR-002 | verified | L2 | ORC-031 |
| SRV-RDIR-003 | accepted | L2 | no probe (Q-007 open) |

### `rewrites` — 1 SRV
| SRV | Status | Level | Evidence (ORC) |
|---|---|---|---|
| SRV-RWRT-001 | verified | L2 | ORC-028, ORC-029 |

(SRV-RWRT-002 is L3; deferred to a future change.)

### `security` — 2 SRVs
| SRV | Status | Level | Evidence (ORC) |
|---|---|---|---|
| SRV-SEC-001 | verified | L2 | ORC-038, ORC-039, ORC-040, ORC-041 |
| SRV-SEC-002 | verified | L2 | ORC-034, ORC-035, ORC-036, ORC-037 |

### `directory-listing` — 3 SRVs
| SRV | Status | Level | Evidence (ORC) |
|---|---|---|---|
| SRV-DLST-001 | verified | L1 | ORC-008, ORC-009, ORC-010 (D-003 limits HTML branch; D-007 sanitizes JSON `dir`) |
| SRV-DLST-002 | verified | L1 | ORC-009, ORC-010 |
| SRV-DLST-003 | verified | L2 | ORC-011 |

## Out-of-scope SRVs (15) — explicitly excluded

| SRV | Reason |
|---|---|
| SRV-CLI-004, SRV-CLI-005, SRV-CLI-017, SRV-CLI-018 | L4 deferred (UDS, Windows pipe, symlinks, TLS) |
| SRV-SYM-001 | L4 deferred |
| SRV-WIN-001 | L4 deferred placeholder |
| SRV-CLI-012, SRV-CLI-013 | L3 (compression, ETag override) |
| SRV-CACHE-001, SRV-CACHE-002, SRV-CACHE-004, SRV-CACHE-005 | L3 (HTTP cache polish) |
| SRV-CACHE-003 | L3 + status `unknown` (Q-009 open) |
| SRV-HDR-001, SRV-HDR-002 | L3 (custom headers) |
| SRV-RWRT-002 | L3 (MIME fallback for rewrites) |
| SRV-CORS-001 | L3 (`--cors` response header surface) |

These SRVs remain in `inventory.md` and `oracle-matrix.md` unchanged; they re-enter scope in a later change once L3 is targeted.

## Deliverables (file tree)

```
openspec/
├── AGENTS.md                                     # MODIFY: namespace list (drop path-resolution, add cors)
└── changes/
    └── 000-establish-serve-compatibility-baseline/
        ├── proposal.md                           # NEW
        ├── design.md                             # NEW
        ├── tasks.md                              # NEW
        └── specs/
            ├── cli/spec.md                       # NEW (13 SRVs)
            ├── config/spec.md                    # NEW (2 SRVs)
            ├── static-files/spec.md              # NEW (5 SRVs)
            ├── routing/spec.md                   # NEW (6 SRVs)
            ├── redirects/spec.md                 # NEW (3 SRVs)
            ├── rewrites/spec.md                  # NEW (1 SRV)
            ├── security/spec.md                  # NEW (2 SRVs)
            └── directory-listing/spec.md         # NEW (3 SRVs)

README.md                                         # MODIFY: stage map row 4 status: todo → in-progress → done
```

No changes under `tools/probe/`, `third_party/`, `docs/reference/serve/`, or any Rust-related path.

## Authoring conventions for spec deltas

Each `specs/<capability>/spec.md` follows the OpenSpec delta format (Context7-verified):

```markdown
# Delta for <capability>

## ADDED Requirements

### Requirement: <Title>
The system SHALL <observable behavior>.

Evidence: SRV-<AREA>-<NNN> (status: <status>, level: L<N>); oracle: ORC-<NNN>[, ORC-<NNN>...] (or "no probe").

#### Scenario: <name from inventory>
- GIVEN <preconditions>
- WHEN <stimulus>
- THEN <observable outcome>
- AND <secondary outcome>
```

Rules:

1. **One Requirement block per SRV.** Title mirrors the SRV one-liner.
2. **Evidence line is mandatory** (anti-hallucination rule #1) and cites the SRV ID, status, level, and backing ORC IDs (or "no probe" for `accepted` SRVs without probe coverage).
3. **Status: adapted** SRVs (CLI-014, CLI-015) include a trailing `Note: see decisions.md D-NNN` line.
4. **Status: accepted** SRVs (CLI-001, CLI-003, CLI-006, CLI-011, CLI-016, CFG-002, RDIR-003) carry a `Note: no oracle coverage; SRV remains 'accepted'.` line. Their scenarios are transcribed verbatim from `inventory.md` without inventing additional behavior.
5. **Scenario bodies are transcribed from `inventory.md`** (already in GIVEN/WHEN/THEN form). Where inventory uses source-implementation language, rewrite to behavior-only (per `openspec/project.md`: "Use behavior-first requirements; do not describe Rust types or call sites").
6. **D-003 (no HTML byte-comparison) and D-007 (sanitized JSON `dir`)** are referenced inline for `static-files/spec.md` SRV-FILE-002 and `directory-listing/spec.md` SRV-DLST-001.
7. **Cross-capability requirements** (SRV-ROUT-006 precedence) cite all interacting capabilities in scenario evidence.

## proposal.md content (sketch)

```markdown
# Proposal: Establish serve compatibility baseline

## Why
IrServe's research phase (Stages 1–3) produced 52 SRV entries plus an oracle matrix and committed probe snapshots, but the OpenSpec contract is empty. Implementation proposals (Stage 5a+) need a baseline to amend instead of asserting requirements directly against research notes.

## What
Add ADDED Requirements across 8 capability namespaces, sourced from L0–L2 SRVs whose status is verified, accepted, or adapted (37 of 52). Out-of-scope categories (L3 polish, L4 deferred, status `unknown`) are excluded; their SRVs remain in research notes.

## Scope
- IN: cli, config, static-files, routing, redirects, rewrites, security, directory-listing (L0–L2 surface)
- OUT (future change): http-cache, headers, rewrites L3 surface, cors, symlinks, Windows path quirks, CLI L4 transports/TLS

## Non-goals
- Implementation details (live in design.md of port-* changes, Stage 5a+).
- Rust code (Stage 5b earliest).
- Bug-for-bug parity with `vercel/serve` (decisions.md D-001 through D-007).

## Risks
- Status `accepted` SRVs without probe evidence are admitted on README/source authority alone (anti-hallucination rule #4 fallback). Listed explicitly in design.md §"Evidence gaps".
- `openspec validate --strict` may surface formatting issues unknown at authoring time; remediated before commit.
```

## design.md content (sketch)

Sections:

1. **Authoring methodology** — explains the SRV-to-Requirement mapping, evidence line requirement, scenario transcription rules.
2. **Compatibility-level cutoff** — why L2 (echoes README; references compatibility-levels.md).
3. **Status taxonomy in scope** — verified / accepted / adapted; rejection of L3+, L4, unknown.
4. **Evidence gaps** — explicit list of accepted-without-probe SRVs (CLI-001, CLI-003, CLI-006, CLI-011, CLI-016, CFG-002, RDIR-003) and the rationale for admitting each.
5. **Adaptation references** — D-002 (CLI-014/015), D-003 (FILE-002 HTML branch, DLST-001 HTML branch), D-005 (CLI-011 clipboard), D-007 (DLST-001 JSON `dir`).
6. **Open questions still open at bootstrap** — Q-001, Q-002, Q-003, Q-004, Q-007, Q-009, Q-011 (cross-referenced for traceability; their resolution does not gate Stage 4).
7. **Validation strategy** — `openspec validate --all --strict --concurrency 12` passes locally before commit; reviewer (Codex) sees structurally valid deltas.
8. **Archive plan** — bootstrap is intentionally NOT archived in Stage 4. Archiving (merging deltas into `openspec/specs/<capability>/spec.md`) is a separate user-initiated step after acceptance. Note recorded so reviewers don't expect populated `openspec/specs/`.

## tasks.md content (sketch)

```markdown
# Tasks

## 1. Pre-authoring
- [ ] 1.1 Install OpenSpec CLI locally (do not commit)
- [ ] 1.2 Re-read decisions.md D-001 through D-007 to confirm adapted/rejected scope

## 2. Author capability deltas
- [ ] 2.1 cli/spec.md (13 SRVs)
- [ ] 2.2 config/spec.md (2 SRVs)
- [ ] 2.3 static-files/spec.md (5 SRVs)
- [ ] 2.4 routing/spec.md (6 SRVs, incl. SRV-ROUT-006 precedence)
- [ ] 2.5 redirects/spec.md (3 SRVs)
- [ ] 2.6 rewrites/spec.md (1 SRV)
- [ ] 2.7 security/spec.md (2 SRVs)
- [ ] 2.8 directory-listing/spec.md (3 SRVs, incl. D-007 reference)

## 3. Author change-level artifacts
- [ ] 3.1 proposal.md
- [ ] 3.2 design.md (incl. evidence-gaps section)

## 4. Side updates
- [ ] 4.1 openspec/AGENTS.md: drop `path-resolution`, add `cors` to reserved capability list
- [ ] 4.2 README.md stage map row 4: status → done
- [ ] 4.3 Add one-line OpenSpec CLI install pointer to AGENTS.md ("how to validate")

## 5. Validate
- [ ] 5.1 `openspec validate --all --strict --concurrency 12` exits 0
- [ ] 5.2 Cross-check evidence lines: every Requirement cites an SRV that exists in inventory.md
- [ ] 5.3 Cross-check ORC IDs: every cited ORC exists in oracle-matrix.md
- [ ] 5.4 Confirm no spec contains source-language (no "this function...", no Rust types, no JS module paths beyond evidence citations)

## 6. Final review pre-commit
- [ ] 6.1 No files modified outside `openspec/`, `README.md` (stage map only)
- [ ] 6.2 No new dependencies committed
- [ ] 6.3 No Rust code, no probe-cases changes
```

## Critical files to read during authoring

- `docs/reference/serve/inventory.md` — source for every Requirement scenario (transcribe GIVEN/WHEN/THEN from each SRV).
- `docs/reference/serve/oracle-matrix.md` — source for every ORC ID cited in evidence lines.
- `docs/reference/serve/decisions.md` — D-001…D-007 for adapted/rejected annotations.
- `docs/reference/serve/open-questions.md` — Q-001…Q-011 referenced from `design.md §"Open questions still open at bootstrap"`.
- `docs/reference/serve/compatibility-levels.md` — confirms L2 cutoff and per-SRV level assignments.
- `openspec/AGENTS.md` — reserved namespace list (modified in this change).
- `openspec/project.md` — spec authoring rules ("behavior-first, no implementation details").

## Existing utilities/patterns to reuse

- **Inventory format** (`docs/reference/serve/inventory.md` GIVEN/WHEN/THEN block) is the direct source for scenario bodies; do not paraphrase.
- **Oracle matrix evidence-citation convention** (SRV → ORC → snapshot path) is the model for the per-Requirement Evidence line.
- **Decisions.md format** (`D-NNN: title / Affected requirements / Status / Reason / Impact`) is referenced from `design.md §"Adaptation references"` rather than duplicated.

## Verification

End-to-end check after implementation, before commit:

1. **Structural**: `npx @fission-ai/openspec validate --all --strict --concurrency 12` exits 0. (CLI install verified via Context7 docs at planning time; package name re-confirmed at execution.)
2. **Evidence integrity** (manual + grep):
   - For each `Requirement:` block in `openspec/changes/000-…/specs/**/spec.md`, the Evidence line cites an SRV ID present in `docs/reference/serve/inventory.md`.
   - Every ORC ID cited resolves to a row in `docs/reference/serve/oracle-matrix.md`.
   - No SRV cited has status `deferred`, `unknown`, or `rejected`.
   - No SRV cited has level L3 or L4.
3. **Coverage cross-check**: count `Requirement:` blocks across all 8 capability deltas = 37 (matches in-scope SRV count).
4. **Boundary**: `git diff --name-only` shows changes only under `openspec/`, plus `README.md` (stage map status). No `tools/`, no `third_party/`, no Rust files, no new top-level `package.json`.
5. **Archive boundary**: `openspec/specs/` contains only `.gitkeep` files (unchanged from Stage 0).
6. **Reviewer-readable**: open the largest delta (`cli/spec.md`) and confirm a reader can navigate from Requirement → SRV → inventory.md scenario → ORC → snapshot path without ambiguity.

## Out of scope for this stage

- Rust code, `Cargo.toml`, `tests/oracle/` (Stage 5b).
- New probes or probe-snapshot edits (`tools/probe/`).
- Resolving open questions Q-001 through Q-011 (those are research follow-ups, not blockers).
- Promoting any `accepted` or `unknown` SRV to `verified` (no new probes are run).
- L3 capability specs (`http-cache`, `headers`, `cors`, `rewrites` MIME fallback). They wait for a future change once implementation reaches L3.
- Archiving the bootstrap into `openspec/specs/` (separate explicit step after acceptance).
- Committing the diff (user-controlled; this stage produces working-tree changes only).
