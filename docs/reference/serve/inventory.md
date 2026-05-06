# Serve reverse-requirements inventory

Working document for Stage 1: reverse-engineering of `vercel/serve` and `vercel/serve-handler` observable behavior.

> **This file is not the source of truth.** Accepted requirements are migrated into OpenSpec specs in Stage 4. Until then, every entry here is a candidate, draft, or note.

## Format

Each requirement uses the following template:

```
## SRV-<AREA>-<NNN>: <short title>

Status: candidate | accepted | verified | adapted | deferred | rejected | unknown
Area: cli | config | static-files | routing | redirects | rewrites | headers | directory-listing | http-cache | security | symlinks
Compatibility level: 0 | 1 | 2 | 3 | 4
Priority: P0 | P1 | P2

Reference source:
- README: documented | partial | absent
- serve / serve-handler source: checked | not-checked
- Existing test: pass | fail | absent
- Oracle test: planned | pending | passing | failing

Requirement (draft):
<observable behavior, no implementation details>

Scenarios:
- GIVEN ...
  WHEN ...
  THEN ...

Compatibility notes:
- ...

Open questions:
- ...
```

## Status taxonomy

| Status | Meaning |
|---|---|
| `candidate` | Found in README/source/test, not yet validated. |
| `accepted` | Confirmed as a target requirement for IrServe. |
| `verified` | Has a passing oracle test against `npm serve`. |
| `adapted` | Behavior intentionally diverges from `serve`; logged in `decisions.md`. |
| `deferred` | Postponed to a higher compatibility level or post-MVP. |
| `rejected` | Will not be supported; logged in `decisions.md`. |
| `unknown` | Behavior is unclear; logged in `open-questions.md` until resolved. |

## Entries

_To be populated in Stage 1 per capability (cli → config → static-files → path-resolution → routing → redirects → rewrites → headers → directory-listing → http-cache → security → symlinks)._
