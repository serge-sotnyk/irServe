# Tasks

## 1. Slice 1 — workspace + clap CLI

- [x] 1.1 Workspace `Cargo.toml` listing `crates/irserve` and
  `crates/irserve-core`
- [x] 1.2 `crates/irserve/Cargo.toml` with `clap 4` (derive),
  `tokio 1`, path-dep on `irserve-core`
- [x] 1.3 `crates/irserve-core/Cargo.toml` with `axum 0.8`, `tokio 1`
  (rt-multi-thread, net, fs, macros, signal), `mime_guess 2`, `bytes 1`,
  `thiserror 2`
- [x] 1.4 `crates/irserve/src/main.rs` — clap derive struct (`-l`,
  `-h`, `-v` lowercase via `disable_version_flag` + manual field, `-n`
  no-op, positional `[DIRECTORY]`), `resolve_port` helper (priority
  `--listen` > `PORT` env > 3000), `127.0.0.1` SocketAddr, hand off to
  `irserve_core::run`
- [x] 1.5 `crates/irserve-core/src/lib.rs` — public `ServerConfig`,
  `Error` (single `Io` variant), `pub async fn run` (panics with
  `unimplemented!()` until Slice 2)
- [x] 1.6 Verify: `cargo build` clean; `cargo run -- --help` exit 0;
  `cargo run -- -v` prints `irserve 0.0.1`; `cargo run -- a b` exit
  non-zero; `cargo run -- --no-port-switching` exit non-zero

## 2. Slice 2 — bind + dispatch (no MIME, no containment)

- [x] 2.1 `crates/irserve-core/src/server.rs` — `Router::new()
  .fallback(handler).with_state(Arc<PathBuf>)`,
  `tokio::net::TcpListener::bind`, `axum::serve`
- [x] 2.2 `crates/irserve-core/src/dispatch.rs` — phase 2 method
  check, phases 9/11/12 (resolve → file/index → 200 + body), phase 13
  empty-body 404 stub
- [x] 2.3 `crates/irserve-core/src/resolve.rs` — `ResolveOutcome`
  enum, lexical join, metadata branch, `index.html` lookup
- [x] 2.4 Verify: `cargo build` clean; smoke `_smoke#root` by hand:
  `curl http://127.0.0.1:3010/` returns 200 + `hello\n`

## 3. Slice 3 — MIME + 404 envelope + phase 10 containment

- [x] 3.1 `crates/irserve-core/src/mime.rs` — `mime_for` with the
  eight FILE-004 bindings hand-rolled, `mime_guess::from_path` long-
  tail fallback, `None` for extensionless / unknown
- [x] 3.2 `crates/irserve-core/src/notfound.rs` — `not_found_response`
  with `Accept: application/json` content-negotiation (substring on
  comma-separated parts, lowercase) → 80-byte JSON envelope; else
  `<h1>404 Not Found</h1>\n` HTML body. `Content-Type` per branch.
- [x] 3.3 `dispatch.rs::file_response` — sets `Content-Type` from
  `mime_for`; omits the header when `mime_for` returns `None`
- [x] 3.4 `resolve.rs` — phase-10 containment via `tokio::fs::canonicalize`
  + `Path::starts_with`; `EscapedRoot` returned on prefix mismatch;
  `dispatch.rs` maps `EscapedRoot` to 404 (L0 hygiene; SRV-SEC-001's
  400 surface deferred per design 001 § 4)
- [x] 3.5 Verify: smoke against fixture matching `mime-defaults.json`
  + `notfound-shape.json`: every MIME extension correct, 404 HTML +
  JSON envelope correct, `--path-as-is` traversal returns 404, POST
  returns 405

## 4. Slice 4 — probe runner adapter + tests/oracle + new L0 cases

- [x] 4.1 `tools/probe/run.mjs` — pluggable target (`--target=` flag
  + `PROBE_TARGET` env), `IRSERVE_BIN` resolution, `spawnServe` branch
  on target, `runCliInvocation` branch on target
- [x] 4.2 `tools/probe/run.mjs` — D-008 deferred-flag refusal in
  `serveArgs` when `target=irserve`
- [x] 4.3 `tools/probe/run.mjs` — `runner.skipAutoListen` + `runner
  .defaultPortScenario` (`no-flag-no-env` checks port 3000 free,
  `env-var` allocates free port and injects `PORT` env)
- [x] 4.4 `tools/probe/run.mjs` — `applyL0Filter` adds
  `bodyMayDiffer`, `contentLengthMayDiffer`, `exitCodeMayDiffer`
  per-anchor overlays; `divergent` anchors stripped from both sides
  pre-diff; markdown report includes `Informational (L1-divergent)`
  section
- [x] 4.5 `tools/probe/run.mjs` — `target=irserve` extends
  `volatileHeaders` with `['etag', 'vary', 'accept-ranges']` before
  the masking pass
- [x] 4.6 `tools/probe/run.mjs` — `runCliProbe` passes `l0` through
  to `diffSnapshots` (so CLI-mode L0 overlays apply)
- [x] 4.7 `tools/probe/cases/_smoke.json` — add `runner.l0` block
  (clean: `[root]`, divergent: `[index_html_redirect]`)
- [x] 4.8 `tools/probe/cases/mime-defaults.json` — add `runner.l0`
  block (clean: `[js, json, css, txt, wasm, svg, png, noext,
  unknownext]`, divergent: `[html]`)
- [x] 4.9 `tools/probe/cases/notfound-shape.json` — add `runner.l0`
  block (clean: both anchors; bodyMayDiffer: `[missing_html]`;
  contentLengthMayDiffer: `[missing_html_with_accept]`)
- [x] 4.10 `tools/probe/cases/cli-help-version.json` — add
  `runner.l0` block (clean: all four anchors)
- [x] 4.11 `tools/probe/cases/cli-positional-error.json` — add
  `runner.l0` block (clean: `[two_positionals]`; exitCodeMayDiffer:
  `[two_positionals]`)
- [x] 4.12 `tools/probe/cases/default-port-l0.json` — env-var scenario,
  fixture is `index.html` only (no `serve.json`; default cleanUrls
  applies on the reference and routes `/` → `index.html`)
- [x] 4.13 `tools/probe/cases/mime-defaults-l0.json` — `serve.json:
  {cleanUrls: false}` so `GET /page.html` returns 200 directly (not
  the 301 redirect from default cleanUrls)
- [x] 4.14 Snapshots recorded with `--snapshot=update --target=reference`:
  `tools/probe/snapshots/default-port-l0.json` (200 + index.html, 13
  bytes); `tools/probe/snapshots/mime-defaults-l0.json` (200 +
  page.html, 12 bytes, `text/html; charset=utf-8`)
- [x] 4.15 `crates/irserve/tests/oracle.rs` — single
  `oracle_l0_must_match` test that shells out to `node tools/probe/
  run.mjs --all --target=irserve --snapshot=verify` with
  `IRSERVE_BIN` env set to `CARGO_BIN_EXE_irserve`
- [x] 4.16 Verify: `cargo test --test oracle` green (7 cases passed,
  27 skipped, 0 failed)
- [x] 4.17 Verify: `node tools/probe/run.mjs --all --target=reference
  --snapshot=verify` green (34 of 34 passed; reference path unchanged)

## 5. Spec delta

- [x] 5.1 `specs/cli/spec.md` — single MODIFIED for SRV-CLI-001
  promoting `accepted` → `verified` with the new `oracle: ORC-063`
  citation and updated Note. Implementation line preserved from
  change 001's MODIFIED.

## 6. Research-track edits (not OpenSpec deltas)

- [ ] 6.1 `docs/reference/serve/oracle-matrix.md` — add ORC-063 row
  for `default-port-l0.json#root` backing SRV-CLI-001 (env-var
  scenario)
- [ ] 6.2 `docs/reference/serve/oracle-matrix.md` — relax ORC-062
  must-match from `exit=1` to `exit=non-zero` (matches spec text
  SRV-CLI-007 sc.3)
- [ ] 6.3 `docs/reference/serve/inventory.md` — promote SRV-CLI-001
  status `accepted` → `verified`

## 7. Stage map update

- [ ] 7.1 `README.md` Status block: mark `Stage 5b` as `Done.`
- [ ] 7.2 `README.md` Stage map: row 5b status `todo` → `done`

## 8. Validate

- [ ] 8.1 `npx -y @fission-ai/openspec@latest validate --all --strict
  --concurrency 12` exit 0 (with both 001 and 002 in flight)
- [ ] 8.2 `cargo build --release` clean
- [ ] 8.3 `cargo test --test oracle` green (final)
- [ ] 8.4 `git diff --stat` confirms zero lines under `third_party/`;
  no edits to existing `tools/probe/snapshots/*.json` (only adds);
  no edits to existing `tools/probe/cases/*.json` beyond `runner.l0`
  blocks
