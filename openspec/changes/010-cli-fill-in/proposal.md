# Proposal: CLI fill-in — `tcp://`, `-p`, `--cors`, `--debug`, `--no-request-logging`, `--no-port-switching`

## Why

Stage 6h is the terminal slice of Stage 6 (L1 + L2). It closes six
SRVs from the CLI inventory — the last flags carried as
`L0_DEFERRED_FLAGS` in `tools/probe/run.mjs:49-55` — and closes the
one remaining open question on the CLI surface (Q-001, `tcp://`
host/port defaults). After this stage, the `6h` row in
`docs/stage6_l1_l2_capabilities.md:56` flips to `done`, the runner's
deferred-flag list is empty, and only Stage 7+ (L3 polish: full CORS
preflight, `ETag`, `Last-Modified`, compression, symlinks, TLS) remains.

This change closes:

- **SRV-CLI-003** (`tcp://host:port` URI for `-l`/`--listen`) —
  flipped `accepted` → `verified`. Hand-rolled parser mirrors
  `third_party/serve/source/utilities/cli.ts:104-143`
  (`parseEndpoint`); supports IPv6 bracketed form `[::1]:port`;
  rejects `pipe:` and `unix:` schemes with a focused error.
- **SRV-CLI-006** (`-p` deprecated alias) — flipped `accepted` →
  `verified`. clap cannot deposit two distinct flags into one Vec,
  so `Cli::p` is a separate hidden `Vec<ListenSpec>` merged
  post-parse with `Cli::listen`.
- **SRV-CLI-010** (`-C`/`--cors`) — already `verified`; wording
  tightened to make the four-header surface explicit
  (`Access-Control-Allow-Origin: *`,
  `Access-Control-Allow-Headers: *`,
  `Access-Control-Allow-Credentials: true`,
  `Access-Control-Allow-Private-Network: true`). Mirrors
  `third_party/serve/source/utilities/server.ts:65-70` followed by
  `serve-handler`'s overwrite at
  `serve-handler/src/index.js:245-251`, `:767`. Applied
  post-dispatch on every status class — including 3xx redirects,
  which `apply_custom_headers` deliberately skips — using
  set-only-if-missing semantics so a user `serve.json#headers` rule
  for any CORS key wins, while the other three defaults still fill in.
- **SRV-CLI-014** (`-d`/`--debug`) — `adapted` (no flip); irserve
  appends an elapsed-ms suffix `(Xms)` to the per-request log line
  under the flag.
- **SRV-CLI-015** (`-L`/`--no-request-logging`) — `adapted`; the
  "no per-request log line written" Scenario becomes contractual.
  When unset, irserve emits a single `{method} {path} -> {status}`
  line per request to stdout. Format is implementation-defined per
  D-002; the flag's silencing effect is not.
- **SRV-CLI-016** (`--no-port-switching`) — flipped `accepted` →
  `adapted` per the new **D-016**. Reference declares the flag at
  `cli.ts:158` but never reads it in `server.ts`'s `startServer`
  (vercel/serve#751, open since 2022-12, regression in v14.0.0;
  worked in v13.0.4). irserve honors the documented contract:
  default = retry on `(addr.ip(), 0)` when `EADDRINUSE`; with
  `--no-port-switching` = surface `Error::PortInUse { addr }` for
  non-zero exit. D-004 (no bug-for-bug parity) authorizes.

Also closes:

- **Q-001** (`tcp://` host/port defaults) via Note on SRV-CLI-003.
  Defaults are host=`localhost`, port=`3000`. Mirrors
  `cli.ts:128-130`. No separate D-NNN entry — the contract lives
  in the spec text.

`SRV-CLI-009` (`-c`/`--config`) was closed back in 6a as a
side-effect of the `serve.json` loader (D-009); it does not belong
to 6h.

## What

- **`ListenSpec` enum + parser.** `Cli::listen` widened from
  `Vec<u16>` to `Vec<ListenSpec>`. New module
  `crates/irserve/src/listen_spec.rs` carries `ListenSpec::Port(u16)`
  and `ListenSpec::Tcp { host: String, port: u16 }` plus a hand-
  rolled `parse()` that does not pull a `url` crate dependency. The
  parser handles IPv6 brackets, applies Q-001 defaults inline, and
  rejects `pipe:` / `unix:` with a focused error. `resolve()`
  short-circuits the literal string `"localhost"` to
  `Ipv4Addr::LOCALHOST` for Windows consistency (dual-stack
  Windows sometimes ranks `::1` first via `dns.lookup`).

- **`-p` post-parse merge.** `Cli::p: Vec<ListenSpec>` is a hidden
  short-only flag; `main` merges `cli.p` into `cli.listen` before
  the loop. clap's `aliases` mechanism cannot route two distinct
  flag spellings into the same `Vec`, so the merge happens in user
  code. Mixed forms (`-l 3010 -p tcp://localhost:3011`) accumulate;
  this is not purpose-built test surface (kickoff out-of-scope #14).

- **`apply_cors` post-dispatch pass.** New
  `crates/irserve-core/src/cors.rs::apply_cors(Response<Body>) ->
  Response<Body>` inserts each of the four reference headers
  **only when the response does not already carry that key**
  (set-only-if-missing). Wired in `server::handler` AFTER
  `apply_custom_headers` so the CORS defaults ride on 3xx redirects
  too — `apply_custom_headers` short-circuits on `is_redirection()`
  (D-015 finding #2), while the reference applies CORS via
  `setHeader` BEFORE dispatch in `server.ts:65-70`, then
  `serve-handler` overwrites those keys when a user rule matches
  (`Object.assign(defaultHeaders, related)` at
  `serve-handler/src/index.js:245-251` + `setHeader` loop at `:767`).
  Net effect: a user `serve.json#headers` rule for any of the four
  CORS keys wins; the other three CORS defaults still fill in.
  Codex review round 1 P1 corrected the slice-3 model.
  Verified by `cors-on-redirect` (3xx pass-through) and
  `cors-user-override` (round-1 regression probe).

- **`bind_with_fallback` and `Error::PortInUse`.** New helper in
  `crates/irserve-core/src/server.rs`: catches `ErrorKind::AddrInUse`
  only, retries on `SocketAddr::new(addr.ip(), 0)` for OS allocation,
  surfaces a one-line stderr message (format implementation-defined
  per D-002). With `--no-port-switching`, the same `AddrInUse` is
  surfaced as `Error::PortInUse { addr }` for non-zero exit. The
  failure-mode is verified by three integration tests in
  `server::tests`; probe coverage is intentionally omitted because
  pre-binding a port from the probe runner is impractical (kickoff
  out-of-scope #14 / D-016 Impact).

- **Per-request log + flags.** Minimal `println!` in
  `server::handler` after `apply_cors`: `{method} {path} -> {status}`
  per request, with `(Xms)` suffix under `--debug`, fully silenced
  under `--no-request-logging`. No `tracing` framework. The runner
  strips stdout/stderr from snapshot envelopes per D-002, so the
  format is not pinned by any probe.

- **Runner gate fully drained.** `tools/probe/run.mjs::L0_DEFERRED_FLAGS`
  drops `tcp://`, `-p`, `-C`, `--cors`, `--no-port-switching`, `-d`,
  `--debug`, `-L`, `--no-request-logging` across slices 2-5. After
  slice 5 the list is empty; all stage-6h flags are accepted
  symmetrically against `target=irserve` and `target=reference`.
  Five new probe cases are authored:
  - `cli-tcp-uri.json` (slice 2) — `--listen tcp://127.0.0.1:{port}`
    + free-port substitution; `GET /` → 200.
  - `cli-p-alias.json` (slice 2) — `-p {port}` + free-port; 200.
  - `cors-on-redirect.json` (slice 3) — `serve.json` redirect
    `/old → /new` + `--cors`; `GET /old` → 301 with all four CORS
    headers. Pins the 3xx-pass-through.
  - `cli-debug-flag.json` (slice 5) — `--debug` acceptance, 200.
  - `cli-no-request-logging.json` (slice 5) — flag acceptance, 200.

- **Final oracle.** `target=irserve total=80 passed=72 skipped=8
  failed=0` after slice 5. Reference still 80 of 80 green via
  `node tools/probe/run.mjs --all --target=reference
  --snapshot=verify`.

## Out of scope

Mirrors `docs/features/0014_PLAN_stage6h_cli_fill_in.md`'s
pre-stage out-of-scope list (rule #10):

1. **`pipe:` scheme** (Windows named pipes). Rejected with focused
   error.
2. **`unix:` scheme** (UDS). Rejected with focused error.
3. **`--ssl-cert` / `--ssl-key` / `--ssl-pass`** — Stage 7+.
4. **`--no-compression`** — Stage 7+ (D-006).
5. **`--no-etag`** — Stage 7+.
6. **`--symlinks` / `-S`** — Stage 7+.
7. **Full L3 CORS surface** — preflight `OPTIONS`,
   `Access-Control-Max-Age`, `Access-Control-Expose-Headers`.
   Stage 7+.
8. **Banner / `boxen` startup output** — D-002.
9. **Exact log-line format** — reference uses `{date} {ip} {url}`
   (D-002); irserve picks its own minimal format.
10. **`--debug` beyond an elapsed-ms suffix** — the reference uses
    `--debug` only for update-check verbosity
    (`main.ts:37,40`); irserve has no update check.
11. **Reference hostname-DNS quirks** — Node `dns.lookup` vs Rust
    `ToSocketAddrs` may rank multi-result results differently;
    irserve does not mirror.
12. **IPv6 dual-stack vs v6-only socket behavior** — parser accepts
    `[::1]:port`, but real v6 bind is exercised only by a trivial
    unit test.
13. **`-p` deprecation warning** — reference accepts it silently;
    irserve mirrors.
14. **Mixed listen forms** (`-l 3010 -p tcp://localhost:3011`) —
    the type supports them, but they are not a purpose-built test
    target. Failure on any one listen address aborts startup
    (Note on SRV-CLI-016).
