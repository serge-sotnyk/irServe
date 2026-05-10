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
- **Stage 4 — OpenSpec bootstrap change.** Done.
- **Stage 5a — first implementation proposal.** Done.
- **Stage 5b — Rust scaffold + first vertical slice.** Done.
- **Stage 6a — `serve.json` loader.** Done.
- **Stage 6b — routing normalization (trailingSlash, multi-slash).** Done.
- **Stage 6c — cleanUrls (301 + extensionless resolution).** Done.
- **Stage 6d — configured redirects.** Done.
- **Stage 6e — configured rewrites + `--single`.** Done.

Rust code lives under `crates/irserve` (the bin) and `crates/irserve-core` (the lib). The first slice (Stage 5b) is strict-L0: eight SRVs (`SRV-CLI-001/002/007/019`, `SRV-FILE-001/002/004/005`). Stage 6a un-defers SRV-CFG-001, SRV-CFG-002, and SRV-CLI-009 (`-c/--config`); the remaining capabilities are still deferred per `D-008`/`D-009`.

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
| 5a | First implementation proposal (specs delta + design + tasks, no code) | `openspec/changes/001-port-minimal-static-server/` (proposal/design/tasks/specs only) | done |
| 5b | Rust scaffold + first vertical slice | `Cargo.toml` + `tests/oracle/` + first crate code | done |
| 6a | `serve.json` loader | `openspec/changes/003-load-serve-json` | done |
| 6b | Routing normalization (trailingSlash, multi-slash) | `openspec/changes/004-route-normalization` | done |
| 6c | cleanUrls (301 + extensionless resolution) | `openspec/changes/005-clean-urls` | done |
| 6d | Configured redirects | `openspec/changes/006-configured-redirects` | done |
| 6e | Configured rewrites + `--single` SPA fallback | `openspec/changes/007-configured-rewrites` | done |
| 6f | Custom error pages, full L2 security, custom response headers | `openspec/changes/008-error-pages-and-security` | todo |
| 6g | Directory listing (HTML / JSON, `unlisted`, `renderSingle`) | `openspec/changes/009-directory-listing` | todo |
| 6h | CLI fill-in (`tcp://`, `-p`, `--cors` L1, `--debug`, `--no-request-logging`, `--no-port-switching`) | `openspec/changes/010-cli-fill-in` | todo |
| 7 | Polish: terminal output, Windows quirks, edge cases (L3+) | `openspec/changes/011...` | todo |

The first concrete Rust crate appears at Stage 5b, not earlier. Stages 1–5a produce only research notes and OpenSpec specs.

Sub-stages 6a–6h are the canonical decomposition of Stage 6 (L1 + L2). Per-sub-stage SRV mappings, dependency edges, and what each one closes / touches live in [`docs/stage6_l1_l2_capabilities.md`](docs/stage6_l1_l2_capabilities.md). Updates to Stage 6 scope or status land there first, then propagate to the rows above.

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

Rust toolchain (1.81+) is required to build and test `irserve`. `cargo build` produces the bin under `target/debug/irserve(.exe)`. `cargo test --test oracle` builds the bin and shells out to `node tools/probe/run.mjs --all --target=irserve --snapshot=verify`; Node 18+ on PATH is a prerequisite (already needed for the reference oracle bundle above).

## Try IrServe (post-6e)

The current binary covers the Stage-5b strict-L0 SRVs (`SRV-CLI-001/002/007/019`, `SRV-FILE-001/002/004/005`), the Stage-6a `serve.json` loader surface (`SRV-CFG-001`, `SRV-CFG-002`, `SRV-CLI-009`), the Stage-6b routing normalization phases (`SRV-ROUT-003` `trailingSlash` add, `SRV-ROUT-004` `trailingSlash` strip, `SRV-ROUT-005` silent multi-slash collapse), the Stage-6c `cleanUrls` phases (`SRV-ROUT-001` `.html`/`/index` 301, `SRV-ROUT-002` extensionless `<P>/index.html`-then-`<P>.html` resolution; both `bool` and `string[]` glob forms), the Stage-6d configured redirects (`SRV-RDIR-001`/`002`/`003`: phase-6 first-match-wins iteration with literal / glob / `:name`-pattern source matching, `type` override for any 3xx, absolute / scheme-relative / relative destination handling per Q-007), and the Stage-6e configured rewrites + `--single` SPA fallback (`SRV-RWRT-001`, `SRV-CLI-008`: phase-7 chained recursion mirroring `applyRewrites` at `serve-handler/src/index.js:91-117`, with an irserve-only depth cap of 64 per D-014; `--single` injects a synthetic `**` rewrite at config-load time per `main.ts:78-90`). Per-field behavior beyond `public`, `trailingSlash`, `cleanUrls`, `redirects`, and `rewrites` (headers, listings, etc.) is parsed into the typed configuration but not yet observable; remaining capabilities are deferred per `D-008`/`D-009`/`D-010`/`D-011`/`D-012`/`D-013`/`D-014` (see `docs/reference/serve/decisions.md`).

```bash
mkdir -p _tmp && echo hello > _tmp/index.html

# Quick dev run — no separate build step, debug profile under target/debug/
cargo run -- --listen 3010 _tmp

# Or build a release binary once and reuse it
cargo build --release
./target/release/irserve --listen 3010 _tmp

# Either form: omit --listen to bind 3000; use `PORT=3010 ...` env;
# pass `--listen 3010 --listen 3011 _tmp` to bind both ports.
```

Exercise it:

```bash
curl -i http://127.0.0.1:3010/                                       # 200, index.html
curl -i http://127.0.0.1:3010/index.html                             # 301, Location: /index   (cleanUrls default)
curl -i http://127.0.0.1:3010/missing                                # 404, text/html, "<h1>404 Not Found</h1>"
curl -i -H 'Accept: application/json' http://127.0.0.1:3010/missing  # 404, JSON envelope verbatim
curl -i -X POST http://127.0.0.1:3010/                               # 405

# cleanUrls extensionless resolution (default cleanUrls=true).
# echo '<p>about</p>' > _tmp/about.html
curl -i http://127.0.0.1:3010/about.html                             # 301, Location: /about
curl -i http://127.0.0.1:3010/about                                  # 200, body of about.html

# cleanUrls array form (scope via globs):
# In _tmp create serve.json: {"cleanUrls": ["/docs/**"]}
# mkdir _tmp/docs _tmp/blog && echo guide > _tmp/docs/guide.html && echo post > _tmp/blog/post.html
curl -i http://127.0.0.1:3010/docs/guide.html                        # 301, Location: /docs/guide
curl -i http://127.0.0.1:3010/blog/post.html                         # 200 (out of scope; served direct)

# Multi-slash collapse (silent; no redirect):
curl -i 'http://127.0.0.1:3010///'                                   # 200, index.html (path collapses to /)

# trailingSlash 301 (requires serve.json with `{"trailingSlash": true|false, "cleanUrls": false}`).
# In _tmp create serve.json: {"trailingSlash": true, "cleanUrls": false}
curl -i http://127.0.0.1:3010/about                                  # 301, Location: /about/  (when trailingSlash=true)
curl -i http://127.0.0.1:3010/about/                                 # 301, Location: /about   (when trailingSlash=false)

# Configured redirects (Stage 6d). In _tmp create serve.json:
# {"redirects":[
#   {"source":"/old","destination":"/new","type":302},
#   {"source":"/old-docs/:id","destination":"/new-docs/:id"}
# ]}
curl -i http://127.0.0.1:3010/old                                    # 302, Location: /new
curl -i http://127.0.0.1:3010/old-docs/42                            # 301, Location: /new-docs/42

# Configured rewrites (Stage 6e). In _tmp create serve.json:
# {"cleanUrls":false,"rewrites":[
#   {"source":"/projects/:id/edit","destination":"/edit-project-:id.html"},
#   {"source":"/spa/**","destination":"/index.html"}
# ]}
# echo '<p>edit-123</p>' > _tmp/edit-project-123.html
curl -i http://127.0.0.1:3010/projects/123/edit                      # 200, body of edit-project-123.html
curl -i http://127.0.0.1:3010/spa/some/deep/path                     # 200, body of index.html

# --single SPA fallback (Stage 6e). No serve.json needed:
# echo '<p>spa-root</p>' > _tmp/index.html
cargo run -- --single --listen 3010 _tmp
curl -i http://127.0.0.1:3010/anything/deep                          # 200, body of index.html

./target/release/irserve --help        # exit 0
./target/release/irserve -v            # exit 0, prints version
./target/release/irserve a b           # exit non-zero, two positionals rejected
./target/release/irserve --no-port-switching   # exit non-zero, deferred flag
```

What is NOT yet observable (still deferred to the remaining Stage 6 sub-stages):
custom `<status>.html` and the full L2 security surface (6f), directory listing (6g), `tcp://host:port` URI form and the rest of the CLI fill-in (6h), and `ETag`/`Last-Modified`/conditional GETs (Stage 7+).

## References

- [`vercel/serve`](https://github.com/vercel/serve) — reference CLI.
- [`vercel/serve-handler`](https://github.com/vercel/serve-handler) — reference core library.
- [OpenSpec](https://openspec.dev) — specification framework.

## License

[MIT](./LICENSE).
