# OpenSpec guide for agents

This folder is the canonical home of IrServe specifications and proposed changes.

## Layout

```
openspec/
├── project.md          # Project context (read first)
├── AGENTS.md           # This file
├── specs/              # Source of truth (populated by archived bootstrap change `000-…`)
│   └── <capability>/
│       └── spec.md
└── changes/            # Proposed changes (one folder per change)
    ├── <change-name>/
    │   ├── proposal.md
    │   ├── design.md         # optional
    │   ├── tasks.md
    │   └── specs/            # delta specs (ADDED / MODIFIED / REMOVED), optional when no delta
    │       └── <capability>/
    │           └── spec.md
    └── archive/        # Archived (accepted-and-applied) changes
        └── YYYY-MM-DD-<change-name>/
```

## Lifecycle

`propose → apply → sync → archive`. The bootstrap change `000-establish-serve-compatibility-baseline` (Stage 4) was archived as `archive/2026-05-08-000-establish-serve-compatibility-baseline/` and its delta specs were merged into `openspec/specs/`. Subsequent changes (e.g. Stage 5a's `001-…`) follow the same lifecycle.

## Validating changes

OpenSpec is not committed as a project dependency. To run `openspec validate` on the bootstrap change (or any future change):

```bash
npx @fission-ai/openspec validate --all --strict --concurrency 12
```

## Authoring rules (must read)

- See `project.md` for project context and spec/design rules.
- Specs describe **observable behavior**, not implementation.
- Scenarios use GIVEN / WHEN / THEN.
- Each requirement that originates from reverse-engineering `serve` cites evidence (README, source path, test name, or oracle ID). See the inventory format at `../docs/reference/serve/inventory.md`.
- Intentional deviations from `serve` are flagged with `Status: adapted` and a matching entry in `../docs/reference/serve/decisions.md`.

## Capability list (reserved)

The following capability namespaces are reserved by `compatibility-levels.md` and will appear under `specs/<capability>/` once accepted:

- `cli`
- `config`
- `static-files`
- `routing`
- `redirects`
- `rewrites`
- `headers`
- `directory-listing`
- `http-cache`
- `cors`
- `security`
- `symlinks`
