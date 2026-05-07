# IrServe

A Rust port of `vercel/serve`, primarily as a vehicle for an experiment in **AI-assisted porting methodology**.

The methodology, not the binary, is the deliverable:

```
legacy project  →  extracted observable behavior  →  specs  →  oracle tests  →  port
```

Concretely: take `vercel/serve` (a small but real Node.js static file server), reverse-engineer its observable HTTP behavior into specifications, build oracle tests against the pinned reference implementation, then re-implement in Rust against those specs. The resulting Rust binary is `irserve`.

## Status

- **Stage 0 — repository scaffolding.** Done.
- **Stage 1 — reverse inventory of `serve` behavior.** Done.
- **Stage 2 — capability map refresh.** Done.
- **Stage 3 — oracle matrix.** Done.
- **Stage 4 — OpenSpec bootstrap change.** Next.

No Rust code exists in this repository yet. It is introduced at Stage 5b, after the first implementation proposal (Stage 5a) is reviewed.

## Repository layout

```
irServe/
├── AGENTS.md                  # short working rules for AI agents
├── CLAUDE.md                  # @AGENTS.md pointer
├── README.md                  # this file: methodology + stage map
├── docs/reference/serve/      # reverse-engineering notes + oracle-matrix.md
├── openspec/                  # specifications and proposed changes (contract)
├── tools/probe/               # Node.js probe runner against the reference
│   ├── cases/                 # declarative probe inputs
│   └── snapshots/             # canonical responses (committed evidence)
└── third_party/
    ├── serve/                 # vercel/serve, pinned release tag (oracle)
    └── serve-handler/         # vercel/serve-handler, pinned release tag (oracle)
```

## Stage map

The methodology runs in nine stages. Status is updated when entering or completing a stage.

| # | Stage | Output | Status |
|---|---|---|---|
| 0 | Init project structure | This scaffold | done |
| 1 | Reverse inventory | `docs/reference/serve/inventory.md` populated | done |
| 2 | Capability map | `docs/reference/serve/compatibility-levels.md` refined | done |
| 3 | Oracle matrix | `docs/reference/serve/oracle-matrix.md` + `tools/probe/snapshots/` | done |
| 4 | OpenSpec bootstrap change | `openspec/changes/000-establish-serve-compatibility-baseline/` | todo |
| 5a | First implementation proposal (specs delta + design + tasks, no code) | `openspec/changes/001-port-minimal-static-server/` (proposal/design/tasks/specs only) | todo |
| 5b | Rust scaffold + first vertical slice | `Cargo.toml` + `tests/oracle/` + first crate code | todo |
| 6 | Implementation proposals (vertical slices, in dependency order) | `openspec/changes/002...010` | todo |
| 7 | Polish: terminal output, Windows quirks, edge cases | `openspec/changes/011...` | todo |

The first concrete Rust crate appears at Stage 5b, not earlier. Stages 1–5a produce only research notes and OpenSpec specs.

## Anti-hallucination rules

These rules are the methodology core. Violations undermine the entire experiment.

1. **Evidence is mandatory.** Every candidate requirement extracted from `serve` MUST cite at least one of: README documentation, source file path, test name, or oracle probe ID.
2. **Status-based promotion.** Use the taxonomy: `candidate` / `accepted` / `verified` / `adapted` / `deferred` / `rejected` / `unknown`. A `candidate` does not automatically become a requirement.
3. **Do not invent behavior.** If `serve` does not document, test, or visibly implement a behavior, mark it `unknown` and add to `docs/reference/serve/open-questions.md`. Do not write plausible-sounding scenarios as fact.
4. **README < tests < source < oracle.** When evidence sources conflict, runtime behavior of the pinned `third_party/serve` is the arbiter, not LLM inference.
5. **Out of scope for MVP** (do not propose specs for these without explicit user approval):
   - Node.js middleware API compatibility (`serve-handler` as an embeddable library).
   - Exact terminal output / stdout formatting.
   - Exact HTML/CSS of the directory listing.
   - Bug-for-bug parity with `serve`.
6. **Reverse-engineering before Stage 5b.** Until the Rust oracle harness exists at Stage 5b, behavior verification uses ad-hoc probes against the pinned reference (see [`tools/probe/`](./tools/probe/)). Probe results are recorded in `docs/reference/serve/inventory.md` entries. Stage 5a produces the first implementation proposal (specs delta + design + tasks) but no Rust code or harness.
7. **Order of artifacts.** `inventory.md` (Stage 1) is research, not contract. OpenSpec specs (Stage 4) are the contract. Implementation proposals (Stage 5a+) are change requests. Never skip stages by writing implementation proposals against unverified behavior.

## Compatibility levels

`irServe` does not aim for bug-for-bug parity. Behavior is grouped into levels and the project commits to a target level per release. Detail in [`docs/reference/serve/compatibility-levels.md`](./docs/reference/serve/compatibility-levels.md).

- **L0** — Minimal useful server: serve a directory, bind host/port, static files, 404, basic MIME.
- **L1** — Serve-style CLI and `serve.json` loading: `public`, `cleanUrls`, `trailingSlash`, directory listing on/off, `unlisted`.
- **L2** — Routing behavior: `cleanUrls`, `redirects`, `rewrites`, headers, SPA fallback equivalent.
- **L3** — HTTP polish: `ETag`, `Last-Modified`, conditional requests, cache behavior.
- **L4** — Edge compatibility: symlinks, path-traversal corner cases, Windows path quirks.

The MVP target is L2; reaching L3 is a stretch goal. L4 is explicitly out of scope unless prioritized later.

## Getting started

```bash
git clone --recurse-submodules git@github.com:serge-sotnyk/irServe.git
# If already cloned without --recurse-submodules:
git submodule update --init --recursive

# Install reference-implementation dependencies and build the runnable bundle.
# vercel/serve uses pnpm; corepack ships with Node 16+, no global install needed.
cd third_party/serve
corepack pnpm install        # the prepare script may print a non-fatal warning about pnpm not on PATH; ignore it
corepack pnpm compile        # produces build/main.js (the runnable entry point)
cd ../..
```

Smoke-test that the reference oracle is operational:

```bash
mkdir -p _tmp && echo hello > _tmp/index.html
node third_party/serve/build/main.js -l 3010 --no-clipboard _tmp &
sleep 2
curl -i http://127.0.0.1:3010/
# expect: 200 OK, body "hello"
# (note: GET /index.html returns 301 → /index because cleanUrls is on by default in serve)
```

There is no Rust toolchain requirement yet; it appears at Stage 5b.

## References

- [`vercel/serve`](https://github.com/vercel/serve) — reference CLI.
- [`vercel/serve-handler`](https://github.com/vercel/serve-handler) — reference core library.
- [OpenSpec](https://openspec.dev) — specification framework.

## License

[MIT](./LICENSE).
