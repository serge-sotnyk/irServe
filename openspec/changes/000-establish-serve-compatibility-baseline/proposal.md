# Proposal: Establish serve compatibility baseline

## Why

The IrServe research phase (Stages 1–3) produced a 52-entry SRV inventory in
`docs/reference/serve/inventory.md`, an oracle matrix of 59 ORC entries in
`docs/reference/serve/oracle-matrix.md`, a decisions log, an open-questions
log, and committed reference snapshots under `tools/probe/snapshots/`.
However, the OpenSpec contract at `openspec/specs/` is empty: there is no
behavior-first source of truth that future implementation proposals
(Stage 5a and beyond) can amend.

This bootstrap change establishes that contract by promoting the
research-stable subset of the SRV inventory into OpenSpec capability
specs. After this change, implementation proposals will modify or extend
existing specs rather than asserting requirements directly against
research notes.

## What

Add ADDED Requirements across eight capability namespaces, sourced from
SRV entries that satisfy all of:

- compatibility level L0, L1, or L2 (matches the MVP target per
  `README.md` and `docs/reference/serve/compatibility-levels.md`);
- status `verified`, `accepted`, or `adapted` (research-stable);
- area maps cleanly to one of the reserved capability namespaces in
  `openspec/AGENTS.md`.

Thirty-five SRV entries qualify and are mapped to capability deltas in
this change. Seventeen entries are explicitly excluded (see "Out of
scope" below) and remain unchanged in the research notes.

## Scope

### In scope

| Namespace | SRVs |
|---|---|
| `cli` | 13 (SRV-CLI-001, 002, 003, 006, 007, 008, 009, 010, 011, 014, 015, 016, 019) |
| `config` | 2 (SRV-CFG-001, 002) |
| `static-files` | 5 (SRV-FILE-001, 002, 003, 004, 005) |
| `routing` | 6 (SRV-ROUT-001 through 006) |
| `redirects` | 3 (SRV-RDIR-001, 002, 003) |
| `rewrites` | 1 (SRV-RWRT-001) |
| `security` | 2 (SRV-SEC-001, 002) |
| `directory-listing` | 3 (SRV-DLST-001, 002, 003) |

Total: 35 ADDED Requirements across 8 capability deltas (13 + 2 + 5 + 6 + 3 + 1 + 2 + 3 = 35).

Level breakdown of the 35 in scope: L0 = 8, L1 = 13, L2 = 14.

### Out of scope (will be added by a later change)

| Reason | SRVs |
|---|---|
| L3 polish (HTTP cache, custom headers, MIME fallback for rewrites, `--cors` response surface, compression, ETag override) | SRV-CLI-012, SRV-CLI-013, SRV-CACHE-001, SRV-CACHE-002, SRV-CACHE-004, SRV-CACHE-005, SRV-HDR-001, SRV-HDR-002, SRV-RWRT-002, SRV-CORS-001 |
| L4 deferred (UDS, Windows pipe, symlinks, TLS) | SRV-CLI-004, SRV-CLI-005, SRV-CLI-017, SRV-CLI-018, SRV-SYM-001 |
| Status `unknown` (Q-009 open) | SRV-CACHE-003 |
| L4 placeholder (no scenarios) | SRV-WIN-001 |

The reserved namespaces `headers`, `http-cache`, `cors`, and `symlinks`
remain reserved in `openspec/AGENTS.md` but receive no `spec.md` in this
change because all of their SRVs are L3 or L4.

## Non-goals

- No Rust code, no `Cargo.toml`, no harness — earliest at Stage 5b.
- No changes under `tools/probe/` or `third_party/`.
- No bug-for-bug parity with `vercel/serve`. Intentional deviations are
  flagged with `Status: adapted` and traced to entries in
  `docs/reference/serve/decisions.md` (D-001 through D-007).
- No archiving of the bootstrap into `openspec/specs/`. Archiving is a
  separate, user-initiated step performed after this change is accepted.

## Risks and mitigations

- **Risk**: Status `accepted` SRVs without probe coverage are admitted
  on README/source authority alone (anti-hallucination rule #4 fallback).
  - **Mitigation**: `design.md` enumerates each such SRV and the
    rationale for inclusion. Each Requirement carries a `Note:` line
    naming the gap so it is auditable from inside the spec itself.
- **Risk**: `openspec validate --strict` may surface formatting issues
  unknown at authoring time.
  - **Mitigation**: validation runs locally via `npx
    @fission-ai/openspec validate --all --strict --concurrency 12`
    before the diff goes to review. Fixes are made in the same change.
- **Risk**: Drift between `inventory.md` scenario bodies and what is
  written here.
  - **Mitigation**: scenarios are transcribed verbatim from `inventory.md`
    (already in GIVEN / WHEN / THEN form), and each Requirement carries
    an `Evidence:` line citing the SRV ID. `tasks.md` includes a
    cross-check step.
