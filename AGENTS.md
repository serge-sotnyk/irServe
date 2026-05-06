# AGENTS.md

## Overview

**IrServe** — a Rust CLI static file server inspired by `vercel/serve`.

This repository is primarily an experiment in **AI-assisted porting methodology**. The chain is:

```
legacy project → extracted observable behavior → specs → oracle tests → port
```

The Rust binary is a side product; the methodology is the deliverable.

- Reference implementation: `vercel/serve` + `vercel/serve-handler` (in `third_party/`).
- Specs: OpenSpec under `openspec/`.
- Compatibility framing: `docs/reference/serve/compatibility-levels.md`.

## Language policy

- All committed files (code, docs, specs, commit messages, identifiers) MUST be in English.
- Chat with the user MAY be in the user's language; only commit-bound artifacts are constrained.

## Stage map

The methodology runs in eight stages. Update the status column when entering or completing a stage.

| # | Stage | Output | Status |
|---|---|---|---|
| 0 | Init project structure | This scaffold | done |
| 1 | Reverse inventory | `docs/reference/serve/inventory.md` populated | todo |
| 2 | Capability map | `docs/reference/serve/compatibility-levels.md` refined | todo (skeleton seeded) |
| 3 | Oracle matrix | `docs/reference/serve/oracle-matrix.md` | todo |
| 4 | OpenSpec bootstrap change | `openspec/changes/000-establish-serve-compatibility-baseline/` | todo |
| 5 | First implementation proposal + Rust scaffold | `openspec/changes/001-port-minimal-static-server/` + `Cargo.toml` + `tests/oracle/` | todo |
| 6 | Implementation proposals (vertical slices, in dependency order) | `openspec/changes/002...010` | todo |
| 7 | Polish: terminal output, Windows quirks, edge cases | `openspec/changes/011...` | todo |

The first concrete Rust crate appears at Stage 5, not earlier.

## Anti-hallucination rules

These rules are the methodology core. Violations undermine the entire experiment.

1. **Evidence is mandatory.** Every candidate requirement extracted from `serve` MUST cite at least one of: README documentation, source file path, test name, or oracle probe ID.
2. **Status-based promotion.** Use the taxonomy: `candidate` / `accepted` / `verified` / `adapted` / `deferred` / `rejected` / `unknown`. A `candidate` does not automatically become a requirement.
3. **Do not invent behavior.** If `serve` does not document, test, or visibly implement a behavior, mark it `unknown` and add to `open-questions.md`. Do not write plausible-sounding scenarios as fact.
4. **README < tests < source < oracle.** When evidence sources conflict, runtime behavior of the pinned `third_party/serve` is the arbiter, not LLM inference.
5. **Out of scope for MVP** (do not propose specs for these without explicit user approval):
   - Node.js middleware API compatibility (`serve-handler` as an embeddable library).
   - Exact terminal output / stdout formatting.
   - Exact HTML/CSS of the directory listing.
   - Bug-for-bug parity with `serve`.
6. **Reverse-engineering before Stage 5.** Until the Rust oracle harness exists at Stage 5, behavior verification uses ad-hoc `curl` against `third_party/serve` (running locally via `npm install` + `npm start`). Probe results are noted in the relevant `inventory.md` entry under "Reference source: oracle test: passing".
7. **Order of artifacts.** `inventory.md` (Stage 1) is research, not contract. OpenSpec specs (Stage 4) are the contract. Implementation proposals (Stage 5+) are change requests. Never skip stages by writing implementation proposals against unverified behavior.

## Getting Started

```bash
# Clone with submodules (or add submodules after cloning)
git clone --recurse-submodules git@github.com:serge-sotnyk/irServe.git
# If already cloned without --recurse-submodules:
git submodule update --init --recursive

# Install npm dependencies for the reference implementation
cd third_party/serve
npm install
cd ../..

# Smoke test that the reference oracle is operational
mkdir -p _tmp && echo hello > _tmp/index.html
node third_party/serve/build/main.js -l 3010 _tmp &
curl -i http://127.0.0.1:3010/index.html
# expect: 200 OK with body "hello"
```

There is no Rust toolchain requirement yet; it appears at Stage 5.

## Project structure

```
irServe/
├── AGENTS.md                          # This file (root agent guide)
├── CLAUDE.md                          # @AGENTS.md pointer
├── LICENSE                            # MIT
├── README.md                          # Methodology overview
├── docs/
│   └── reference/
│       └── serve/                     # Reverse-engineering notes (research)
│           ├── compatibility-levels.md
│           ├── inventory.md
│           ├── decisions.md
│           └── open-questions.md
├── openspec/                          # OpenSpec specs and changes (contract)
│   ├── AGENTS.md                      # OpenSpec-specific agent guide
│   ├── project.md
│   ├── changes/                       # populated from Stage 4
│   └── specs/                         # populated after Stage 4 archive
└── third_party/
    ├── serve/                         # vercel/serve @ pinned release tag
    └── serve-handler/                 # vercel/serve-handler @ pinned release tag
```

## Code style

Rust code does not yet exist in the repo. When it is introduced at Stage 5, a Code Style section will be added covering edition, MSRV, formatter (`cargo fmt`), linter (`cargo clippy`), and error-handling conventions.

For Markdown:

- Headings start at `##` inside files (the document title is `#`).
- One blank line between sections.
- Code fences specify a language (`bash`, `text`, `markdown`, etc.).

## Notes for agents

- **Source of truth for `serve` behavior** is the local submodule `third_party/serve` (and `third_party/serve-handler`), pinned to a specific release tag. Do not consult `npm install -g serve` or arbitrary GitHub HEAD; versions diverge.
- **OpenSpec lives under `openspec/`.** The OpenSpec CLI is not installed in this repo yet — the skeleton was authored manually because `openspec init` does not produce useful output on an empty repo. Install via `npm i -g @fission-ai/openspec` (or check the latest package name in OpenSpec docs) when `openspec validate` becomes necessary.
- **Cross-platform.** The primary developer is on Windows; commands here use POSIX-style shell, but Bash/PowerShell equivalents must work. Do not introduce Bash-only syntax in committed scripts without a PowerShell counterpart.
- **Use Context7 MCP** (`resolve-library-id` → `query-docs`) to fetch current docs for OpenSpec, Rust crates, and any library before assuming behavior.
