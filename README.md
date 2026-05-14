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
- **Stage 6f — custom error pages, full L2 security, custom response headers.** Done.
- **Stage 6g — directory listing (HTML / JSON, `unlisted`, `renderSingle`).** Done.
- **Stage 6h — CLI fill-in (`tcp://`, `-p`, `--cors`, `--debug`, `--no-request-logging`, `--no-port-switching`).** Done.
- **Stage 7a — ETag + 304 conditional GET.** Done.
- **Stage 7b — `Last-Modified` + `--no-etag` + `If-Modified-Since`.** Done.
- **Stage 7c — Range requests (`206`/`416`).** Done.
- **Stage 7d — `Cache-Control` default + `OPTIONS` (CORS preflight).** Done.

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
| 6f | Custom error pages, full L2 security, custom response headers | `openspec/changes/008-error-pages-and-security` | done |
| 6g | Directory listing (HTML / JSON, `unlisted`, `renderSingle`) | `openspec/changes/009-directory-listing` | done |
| 6h | CLI fill-in (`tcp://`, `-p`, `--cors` L1, `--debug`, `--no-request-logging`, `--no-port-switching`) | `openspec/changes/010-cli-fill-in` | done |
| 7a | ETag + 304 conditional GET | `openspec/changes/011-etag-conditional` | done |
| 7b | `Last-Modified` + `--no-etag` + `If-Modified-Since` | `openspec/changes/012-last-modified` | done |
| 7c | Range requests (`206`/`416`) | `openspec/changes/013-range-requests` | done |
| 7d | `Cache-Control` default + `OPTIONS` (CORS preflight) | `openspec/changes/014-cache-headers-and-preflight` | done |
| 7e | HTTP compression (`-u`/`--no-compression`) | `openspec/changes/015-compression` | todo |

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

## Try IrServe (post-6h)

The current binary covers the Stage-5b strict-L0 SRVs (`SRV-CLI-001/002/007/019`, `SRV-FILE-001/002/004/005`), the Stage-6a `serve.json` loader surface (`SRV-CFG-001`, `SRV-CFG-002`, `SRV-CLI-009`), the Stage-6b routing normalization phases (`SRV-ROUT-003` `trailingSlash` add, `SRV-ROUT-004` `trailingSlash` strip, `SRV-ROUT-005` silent multi-slash collapse), the Stage-6c `cleanUrls` phases (`SRV-ROUT-001` `.html`/`/index` 301, `SRV-ROUT-002` extensionless `<P>/index.html`-then-`<P>.html` resolution; both `bool` and `string[]` glob forms), the Stage-6d configured redirects (`SRV-RDIR-001`/`002`/`003`: phase-6 first-match-wins iteration with literal / glob / `:name`-pattern source matching, `type` override for any 3xx, absolute / scheme-relative / relative destination handling per Q-007), the Stage-6e configured rewrites + `--single` SPA fallback (`SRV-RWRT-001`, `SRV-CLI-008`: phase-7 chained recursion mirroring `applyRewrites` at `serve-handler/src/index.js:91-117`, with an irserve-only depth cap of 64 per D-014; `--single` injects a synthetic `**` rewrite at config-load time per `main.ts:78-90`), the Stage-6f cross-cutting bundle (`SRV-FILE-003` custom `<status>.html` from served root, `SRV-SEC-001` full wire-level surface — strict `%xx` syntax + lexical `..` containment both yielding 400 — and `SRV-SEC-002` single-pass URL decode, plus `SRV-HDR-001`/`002` custom response headers with accumulate / case-insensitive override / 3xx-skip / `value: null` prune), the Stage-6g directory listing branch (`SRV-DLST-001`/`002`/`003`: phase 11 with HTML / JSON content negotiation; `directoryListing: bool | string[]` scope; hardcoded `[".DS_Store", ".git"]` defaults plus user `unlisted` globs; `renderSingle` short-circuit checked against the unfiltered count per reference; D-007 sanitization with `"."`/`"sub"`/`"sub/deep"` JSON shape; listing 200 responses bypass `apply_custom_headers` mirroring reference), and the Stage-6h CLI fill-in (`SRV-CLI-003` `tcp://host:port` URI form with Q-001 defaults host=`localhost`/port=`3000`, `SRV-CLI-006` `-p` deprecated alias, `SRV-CLI-010` `-C`/`--cors` enabling all four reference CORS headers — `access-control-allow-origin: *`, `access-control-allow-headers: *`, `access-control-allow-credentials: true`, `access-control-allow-private-network: true` — applied post-dispatch so they ride on 3xx redirects too, `SRV-CLI-014` `-d`/`--debug` with elapsed-ms suffix on the per-request log, `SRV-CLI-015` `-L`/`--no-request-logging` silencing the per-request log line, and `SRV-CLI-016` `--no-port-switching` enforcing the documented contract — default retry on `EADDRINUSE`, flag-set non-zero exit — per D-016 since the reference's flag is a no-op in v14+ via vercel/serve#751). Remaining capabilities are deferred per `D-008`/`D-009`/`D-010`/`D-011`/`D-012`/`D-013`/`D-014`/`D-015`/`D-016` (see `docs/reference/serve/decisions.md`).

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

# Custom error page (Stage 6f, SRV-FILE-003). Drop a <status>.html in the served root:
# echo '<p>custom-not-found</p>' > _tmp/404.html
curl -i http://127.0.0.1:3010/missing                                # 404, body of 404.html
curl -i -H 'Accept: application/json' http://127.0.0.1:3010/missing  # 404, JSON envelope (custom HTML page is HTML-only)

# Path-traversal → 400 (Stage 6f, SRV-SEC-001).
curl -i --path-as-is http://127.0.0.1:3010/../package.json           # 400 (lexical `..` escapes root)
curl -i 'http://127.0.0.1:3010/%zz'                                  # 400 (malformed %xx escape)

# Custom response headers (Stage 6f, SRV-HDR-001). In _tmp create serve.json:
# {"headers":[{"source":"**/*.css","headers":[
#   {"key":"Cache-Control","value":"public, max-age=600"},
#   {"key":"X-Custom","value":"yes"}
# ]}]}
# echo 'body{}' > _tmp/asset.css
curl -i http://127.0.0.1:3010/asset.css                              # 200, x-custom: yes, cache-control: public, max-age=600

# ETag + 304 conditional GET (Stage 7a, SRV-CACHE-001).
# echo 'body{color:red}' > _tmp/asset.css
curl -sI http://127.0.0.1:3010/asset.css | grep -i ^etag             # capture the ETag value (strong-quoted hex)
# Then replay with the captured value:
curl -i -H 'If-None-Match: "<captured-etag>"' http://127.0.0.1:3010/asset.css   # 304, no body
# Disable ETag per-deployment via serve.json: {"etag": false} (no header emitted, no 304).

# Last-Modified + If-Modified-Since (Stage 7b, SRV-CACHE-002/003 + SRV-CLI-013, D-018).
# Under --no-etag, the file response carries Last-Modified instead of ETag
# (mutex per serve-handler/src/index.js:227-236).
cargo run -- --no-etag --listen 3010 _tmp
curl -sI http://127.0.0.1:3010/asset.css | grep -iE '^(etag|last-modified):'
# Expect: last-modified: <IMF-fixdate UTC>; no etag header.
# Then replay the captured Last-Modified as If-Modified-Since:
curl -i -H 'If-Modified-Since: <captured-LM>' http://127.0.0.1:3010/asset.css   # 304, no body
# (D-018 irserve adaptation — the pinned reference returns 200 here since it has no IMS branch.)
# Malformed IMS is treated as absent per RFC 9111 §13.1.3:
curl -i -H 'If-Modified-Since: not-a-date' http://127.0.0.1:3010/asset.css      # 200, full body
# serve.json {"etag": false} achieves the same Last-Modified surface without the CLI flag.

# Range requests (Stage 7c, SRV-CACHE-004). On a 16-byte asset.css:
curl -i -H 'Range: bytes=0-3' http://127.0.0.1:3010/asset.css
# 206 Partial Content; content-range: bytes 0-3/16; content-length: 4; body: 4 bytes
curl -i -H 'Range: bytes=-4' http://127.0.0.1:3010/asset.css
# 206; suffix form returns the last 4 bytes; content-range: bytes 12-15/16
curl -i -H 'Range: bytes=999-1000' http://127.0.0.1:3010/asset.css
# 416 Range Not Satisfiable; content-range: bytes */16; body: full file (RFC 7233 §4.4)
# Range pre-empts both 304 short-circuits — even with a matching If-None-Match
# (or, under --no-etag, a matching If-Modified-Since), Range emits 206/416 not 304.

# Directory listing (Stage 6g). Default config — start in a directory
# without index.html and request its root:
# rm _tmp/index.html; touch _tmp/a.txt _tmp/b.txt; mkdir _tmp/sub
curl -i http://127.0.0.1:3010/                                       # 200, text/html, listing of a.txt / b.txt / sub/
curl -i -H 'Accept: application/json' http://127.0.0.1:3010/         # 200, application/json, {"files":[...],"directory":".","paths":[]}

# Disable listing globally (SRV-DLST-001). In _tmp/serve.json:
# {"directoryListing": false}
curl -i http://127.0.0.1:3010/                                       # 404

# unlisted filter (SRV-DLST-002). In _tmp/serve.json:
# {"unlisted":["secret.txt"]}
# echo shh > _tmp/secret.txt; touch _tmp/.DS_Store
curl -i http://127.0.0.1:3010/                                       # 200; listing omits secret.txt and .DS_Store
curl -i http://127.0.0.1:3010/secret.txt                             # 200; unlisted only hides from listings, not from direct fetch

# renderSingle (SRV-DLST-003). In _tmp/serve.json:
# {"renderSingle": true}
# rm -rf _tmp/*; mkdir _tmp/media; cp some.png _tmp/media/photo.png
curl -i http://127.0.0.1:3010/media/                                 # 200, image/png, body is photo.png bytes (no listing)

# tcp:// listen URI (Stage 6h, SRV-CLI-003).
cargo run -- --listen tcp://127.0.0.1:3010 _tmp
curl -i http://127.0.0.1:3010/                   # 200
# Q-001 default port (3000) when omitted:
cargo run -- --listen tcp://localhost _tmp
curl -i http://localhost:3000/                   # 200

# `-p` deprecated alias for `--listen` (Stage 6h, SRV-CLI-006).
cargo run -- -p 3011 _tmp
curl -i http://127.0.0.1:3011/                   # 200

# --cors enables all four reference CORS headers (Stage 6h, SRV-CLI-010).
cargo run -- --cors --listen 3010 _tmp
curl -i http://127.0.0.1:3010/                   # 200 + 4 access-control-* headers

# OPTIONS preflight under --cors (Stage 7d, SRV-CORS-001).
# Reference does NOT short-circuit; OPTIONS flows through the static pipeline like GET.
curl -i -X OPTIONS -H 'Origin: https://example.com' \
  -H 'Access-Control-Request-Method: GET' \
  -H 'Access-Control-Request-Headers: x-custom-header' \
  http://127.0.0.1:3010/asset.css
# 200 + asset body + ETag + content-type + 4 access-control-* headers; no 204.

# Default Cache-Control is absent (Stage 7d, SRV-CACHE-005); only user `headers` rules emit it.
curl -sI http://127.0.0.1:3010/asset.css | grep -i cache-control || echo "(no cache-control)"

# --no-port-switching (Stage 6h, SRV-CLI-016, D-016): refuse fallback on a busy port.
# Terminal A:
cargo run -- --listen 3010 _tmp
# Terminal B (default — retries):
cargo run -- --listen 3010 _tmp
# stderr: "warning: listen address 127.0.0.1:3010 is already in use, switched to 127.0.0.1:NNNNN"
# Terminal B (with flag — exits):
cargo run -- --no-port-switching --listen 3010 _tmp
# stderr: "error: listen address 127.0.0.1:3010 is already in use (--no-port-switching is set)"; non-zero exit.

# --debug + --no-request-logging (Stage 6h, SRV-CLI-014/015).
cargo run -- --debug --listen 3010 _tmp
curl -s http://127.0.0.1:3010/ > /dev/null
# stdout: "GET / -> 200 (Xms)"
cargo run -- --no-request-logging --listen 3010 _tmp
curl -s http://127.0.0.1:3010/ > /dev/null
# stdout: (empty)

./target/release/irserve --help        # exit 0
./target/release/irserve -v            # exit 0, prints version
./target/release/irserve a b           # exit non-zero, two positionals rejected
```

What is NOT yet observable (still deferred to Stage 7e+ / L4):
gzip compression, symlink resolution, TLS.

## References

- [`vercel/serve`](https://github.com/vercel/serve) — reference CLI.
- [`vercel/serve-handler`](https://github.com/vercel/serve-handler) — reference core library.
- [OpenSpec](https://openspec.dev) — specification framework.

## License

[MIT](./LICENSE).
