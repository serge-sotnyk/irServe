# OpenSpec guide for agents

This folder is the canonical home of IrServe specifications and proposed changes.

## Layout

```
openspec/
├── project.md          # Project context (read first)
├── AGENTS.md           # This file
├── specs/              # Source of truth (populated after Stage 4 archives the bootstrap change)
│   └── <capability>/
│       └── spec.md
└── changes/            # Proposed changes (one folder per change)
    └── <change-name>/
        ├── proposal.md
        ├── design.md         # optional
        ├── tasks.md
        └── specs/            # delta specs (ADDED / MODIFIED / REMOVED)
            └── <capability>/
                └── spec.md
```

## Lifecycle

`propose → apply → sync → archive`. The first change is `000-establish-serve-compatibility-baseline`, created in Stage 4. Until then both `specs/` and `changes/` are intentionally empty.

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
- `path-resolution`
- `routing`
- `redirects`
- `rewrites`
- `headers`
- `directory-listing`
- `http-cache`
- `security`
- `symlinks`
