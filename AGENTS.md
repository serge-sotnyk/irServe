# AGENTS.md

Short working rules for AI agents in this repository. Project overview, stage map, and methodology live in [`README.md`](./README.md). This file is intentionally minimal so it stays cheap to load into every agent context.

## Language policy

- All committed files (code, docs, specs, commit messages, identifiers) MUST be in English.
- Chat with the user MAY be in the user's language; only commit-bound artifacts are constrained.

## Anti-hallucination rules (one-liners)

Authoritative version with examples is in [README §Anti-hallucination rules](./README.md#anti-hallucination-rules). Summary:

1. Cite evidence (README / source path / test name / probe id) for every candidate requirement.
2. Honor the status taxonomy: `candidate / accepted / verified / adapted / deferred / rejected / unknown`.
3. Do not invent behavior. Unknowns go to `docs/reference/serve/open-questions.md`.
4. Source-of-truth order: `README < tests < source < oracle probe`.
5. Out-of-scope for MVP: Node middleware API, exact terminal output, exact HTML listing, bug-for-bug parity.
6. Verify with probes against the pinned reference, not external installs.
7. Stage discipline: inventory → specs → implementation proposals. Do not skip.

## Pointers

- Methodology, stage map, getting started → [`README.md`](./README.md).
- Compatibility levels → [`docs/reference/serve/compatibility-levels.md`](./docs/reference/serve/compatibility-levels.md).
- OpenSpec authoring rules → [`openspec/AGENTS.md`](./openspec/AGENTS.md).
- Reference-behavior probes → [`tools/probe/README.md`](./tools/probe/README.md).

## Notes for agents

- **Source of truth for `serve` behavior** is the local submodule `third_party/serve` (and `third_party/serve-handler`), pinned to a specific release tag. Do not consult `npm install -g serve` or arbitrary GitHub HEAD; versions diverge.
- **OpenSpec lives under `openspec/`.** The OpenSpec CLI is not installed in this repo yet — the skeleton was authored manually because `openspec init` does not produce useful output on an empty repo. Install via `npm i -g @fission-ai/openspec` (or check the latest package name in OpenSpec docs) when `openspec validate` becomes necessary.
- **Cross-platform.** The primary developer is on Windows. Committed scripts must run on both Bash and PowerShell. Probe tooling is Node-based to avoid shell-specific syntax.
- **Use Context7 MCP** (`resolve-library-id` → `query-docs`) to fetch current docs for OpenSpec, Rust crates, and any library before assuming behavior.

## Markdown style

- Headings start at `##` inside files (the document title is `#`).
- One blank line between sections.
- Code fences specify a language (`bash`, `text`, `markdown`, etc.).

Code style for Rust will be added at Stage 5b when the first Rust crate is introduced.
