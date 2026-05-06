# IrServe

A Rust port of `vercel/serve`, primarily as a vehicle for an experiment in **AI-assisted porting methodology**.

The methodology, not the binary, is the deliverable:

```
legacy project  →  extracted observable behavior  →  specs  →  oracle tests  →  port
```

Concretely: take `vercel/serve` (a small but real Node.js static file server), reverse-engineer its observable HTTP behavior into specifications, build oracle tests against the pinned reference implementation, then re-implement in Rust against those specs. The resulting Rust binary is `irserve`.

## Status

- **Stage 0 — repository scaffolding.** Done.
- **Stage 1 — reverse inventory of `serve` behavior.** Next.
- Full stage map: see [`AGENTS.md`](./AGENTS.md).

No Rust code exists in this repository yet. It is introduced at Stage 5, alongside the first implementation proposal.

## Repository layout

```
irServe/
├── AGENTS.md                  # methodology + working rules for AI agents
├── CLAUDE.md                  # pointer to AGENTS.md
├── docs/reference/serve/      # reverse-engineering notes (research)
├── openspec/                  # specifications and proposed changes (contract)
└── third_party/
    ├── serve/                 # vercel/serve, pinned release tag (oracle)
    └── serve-handler/         # vercel/serve-handler, pinned release tag (oracle)
```

## Getting started

```bash
git clone --recurse-submodules git@github.com:serge-sotnyk/irServe.git
cd irServe/third_party/serve
corepack pnpm install
corepack pnpm compile
```

`vercel/serve` uses pnpm; `corepack` ships with Node 16+ so no global install is needed. After this, `node third_party/serve/build/main.js -l 3010 <dir>` runs the reference oracle. See [`AGENTS.md`](./AGENTS.md) for a smoke probe.

## References

- [`vercel/serve`](https://github.com/vercel/serve) — reference CLI.
- [`vercel/serve-handler`](https://github.com/vercel/serve-handler) — reference core library.
- [OpenSpec](https://openspec.dev) — specification framework.

## License

[MIT](./LICENSE).
