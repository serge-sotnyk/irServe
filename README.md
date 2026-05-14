# IrServe

A Rust port of `vercel/serve`, primarily as a vehicle for an experiment in **AI-assisted porting methodology**.

The methodology, not the binary, is the deliverable:

```
legacy project  →  extracted observable behavior  →  specs  →  oracle tests  →  port
```

Concretely: take `vercel/serve` (a small but real Node.js static file server), reverse-engineer its observable HTTP behavior into specifications, build oracle tests against the pinned reference implementation, then re-implement in Rust against those specs. The resulting Rust binary is `irserve`.

## Status

MVP shipped as **v0.1.0** (2026-05-15). Levels L0–L3 implemented; L4 (symlinks, TLS, Windows path quirks) deferred. See [`CHANGELOG.md`](./CHANGELOG.md) for the released surface and [`docs/methodology_retrospective.md`](./docs/methodology_retrospective.md) for the experiment write-up.

Rust code lives under `crates/irserve` (the bin) and `crates/irserve-core` (the lib).

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
| 4 | OpenSpec bootstrap change | `openspec/changes/000-establish-serve-compatibility-baseline/` | done |
| 5a | First implementation proposal (specs delta + design + tasks, no code) | `openspec/changes/archive/2026-05-15-001-port-minimal-static-server/` (proposal/design/tasks/specs only) | done |
| 5b | Rust scaffold + first vertical slice | `Cargo.toml` + `tests/oracle/` + first crate code | done |
| 6a | `serve.json` loader | `openspec/changes/archive/2026-05-15-003-load-serve-json` | done |
| 6b | Routing normalization (trailingSlash, multi-slash) | `openspec/changes/archive/2026-05-15-004-route-normalization` | done |
| 6c | cleanUrls (301 + extensionless resolution) | `openspec/changes/archive/2026-05-15-005-clean-urls` | done |
| 6d | Configured redirects | `openspec/changes/archive/2026-05-15-006-configured-redirects` | done |
| 6e | Configured rewrites + `--single` SPA fallback | `openspec/changes/archive/2026-05-15-007-configured-rewrites` | done |
| 6f | Custom error pages, full L2 security, custom response headers | `openspec/changes/archive/2026-05-15-008-error-pages-and-security` | done |
| 6g | Directory listing (HTML / JSON, `unlisted`, `renderSingle`) | `openspec/changes/archive/2026-05-15-009-directory-listing` | done |
| 6h | CLI fill-in (`tcp://`, `-p`, `--cors` L1, `--debug`, `--no-request-logging`, `--no-port-switching`) | `openspec/changes/archive/2026-05-15-010-cli-fill-in` | done |
| 7a | ETag + 304 conditional GET | `openspec/changes/archive/2026-05-15-011-etag-conditional` | done |
| 7b | `Last-Modified` + `--no-etag` + `If-Modified-Since` | `openspec/changes/archive/2026-05-15-012-last-modified` | done |
| 7c | Range requests (`206`/`416`) | `openspec/changes/archive/2026-05-15-013-range-requests` | done |
| 7d | `Cache-Control` default + `OPTIONS` (CORS preflight) | `openspec/changes/archive/2026-05-15-014-cache-headers-and-preflight` | done |
| 7e | HTTP compression (`-u`/`--no-compression`) | `openspec/changes/archive/2026-05-15-015-compression` | done |

The first concrete Rust crate appears at Stage 5b, not earlier. Stages 1–5a produce only research notes and OpenSpec specs.

Sub-stages 6a–6h are the canonical decomposition of Stage 6 (L1 + L2). Per-sub-stage SRV mappings, dependency edges, and what each one closes / touches live in [`docs/stage6_l1_l2_capabilities.md`](docs/stage6_l1_l2_capabilities.md). Sub-stages 7a–7e play the same role for Stage 7 (L3 polish) — detail in [`docs/stage7_l3_capabilities.md`](docs/stage7_l3_capabilities.md). Updates to Stage 6 or Stage 7 scope / status land in those files first, then propagate to the rows above. L4 (symlinks, TLS, Windows path quirks) is intentionally out of scope until prioritized later.

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
8. **Empirical-before-implement when mirroring third-party libraries.** Reading source code and documentation is necessary but not sufficient. Before writing the irServe equivalent of behavior that depends on a specific library (e.g. `path-to-regexp`, `minimatch`, `mime-db`), prepare 5-10 edge-case probes that exercise the library directly (Node script invoking the library, or a focused unit test against `third_party/`). Documentation systematically under-reports quirks (default flags, platform-specific branches, escape-handling asymmetries). Stage 6d hit 12 review rounds largely because path-to-regexp's default `i` flag and minimatch's Windows-only path-sep replacement were not surfaced by reading docs alone.
9. **Stop after 3+ consecutive review rounds on the same fine-grained aspect.** "Aspect" here is narrower than "subsystem" — it's a single behavior dimension (e.g. backslash handling, case folding, dot-rule, escape semantics). The whole `redirects` matcher is a subsystem; "case-folding within Literal/Pattern matchers" is an aspect. If reviewer findings keep landing on the same aspect for three rounds running, do not start the fourth fix immediately. Pause and: (a) declare a parity scope for that aspect (which behaviors are in-scope to mirror vs which will be documented as known divergences), (b) enumerate the divergences explicitly in the relevant `D-NNN` decision AND in the spec delta's Compatibility note (so the contract does not promise more than will be delivered), (c) ask the user whether to continue iterating or freeze. Note: this is distinct from the project's compatibility levels (L0..L4 in §Compatibility levels) — those classify whole feature groups; this rule is about fine-grained behavior corners within a single feature.
10. **Pre-stage out-of-scope list when zeroing a third-party behavior.** Stages that mirror a non-trivial library MUST include an explicit "out of scope for this stage" list in the proposal/plan, naming concrete quirks that will land as `D-NNN` known divergences if encountered. Discovering scope-creep through review rounds is a signal that the pre-stage list was missing or under-specified, not a normal cost.

## Compatibility levels

`irServe` does not aim for bug-for-bug parity. Behavior is grouped into levels and the project commits to a target level per release. Detail in [`docs/reference/serve/compatibility-levels.md`](./docs/reference/serve/compatibility-levels.md).

- **L0** — Minimal useful server: serve a directory, bind host/port, static files, 404, basic MIME.
- **L1** — Serve-style CLI and `serve.json` loading: `public`, `cleanUrls`, `trailingSlash`, directory listing on/off, `unlisted`.
- **L2** — Routing behavior: `cleanUrls`, `redirects`, `rewrites`, headers, SPA fallback equivalent.
- **L3** — HTTP polish: `ETag`, `Last-Modified`, conditional requests, cache behavior.
- **L4** — Edge compatibility: symlinks, path-traversal corner cases, Windows path quirks.

The MVP target is L2; reaching L3 is a stretch goal. L4 is explicitly out of scope unless prioritized later.

## Install

Requires a Rust toolchain (1.81+). Not published to crates.io; install from the git repo:

```bash
cargo install --git https://github.com/serge-sotnyk/irServe --bin irserve
```

`cargo install` does not need the submodules — those are only required for running the oracle test suite during development.

Smoke-test:

```bash
mkdir _tmp && echo hello > _tmp/index.html
irserve --listen 3010 _tmp &
curl -i http://127.0.0.1:3010/
# expect: 200 OK, body "hello"
```

For per-feature usage examples, see [`docs/user-guide.md`](./docs/user-guide.md). The full behavior contract lives in [`openspec/specs/`](./openspec/specs/).

## For development / contributors

The `third_party/` submodules are pinned to `update = none` in `.gitmodules` so `cargo install` does not pull tens of megabytes of Node fixtures that the build does not need. To fetch them explicitly for the oracle test suite, pass `--checkout` (overrides the `none` strategy):

```bash
git clone https://github.com/serge-sotnyk/irServe.git
cd irServe
git submodule update --init --checkout --recursive

# Build the reference-implementation oracle (vercel/serve, pinned).
# vercel/serve uses pnpm; corepack ships with Node 16+, no global install needed.
cd third_party/serve
corepack pnpm install        # the prepare script may print a non-fatal warning about pnpm not on PATH; ignore it
corepack pnpm compile        # produces build/main.js (the runnable entry point)
cd ../..
```

`cargo build` produces the bin under `target/debug/irserve(.exe)`. `cargo test --test oracle` builds the bin and shells out to `node tools/probe/run.mjs --all --target=irserve --snapshot=verify`; Node 18+ on PATH is a prerequisite.

## References

- [`vercel/serve`](https://github.com/vercel/serve) — reference CLI.
- [`vercel/serve-handler`](https://github.com/vercel/serve-handler) — reference core library.
- [OpenSpec](https://openspec.dev) — specification framework.

## License

[MIT](./LICENSE).
