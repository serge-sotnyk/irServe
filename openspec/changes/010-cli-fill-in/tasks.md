# Tasks: CLI fill-in — `tcp://`, `-p`, `--cors`, `--debug`, `--no-request-logging`, `--no-port-switching`

Six iterative slices, one commit per green slice. Slice 6 is the
meta slice and lands last so spec deltas reflect what was actually
shipped.

## Slice 1 — `ListenSpec` type + `tcp://` parser

- [x] New module `crates/irserve/src/listen_spec.rs` with
  `ListenSpec::Port(u16)` and `ListenSpec::Tcp { host, port }`.
- [x] Hand-rolled `parse(s: &str) -> Result<Self, ParseError>`
  mirroring `third_party/serve/source/utilities/cli.ts:104-143`
  (`parseEndpoint`); no `url` crate dependency.
- [x] Q-001 defaults inline: host=`"localhost"`, port=`3000` when
  either is missing in the `tcp://` URI.
- [x] IPv6 bracketed form `[::1]:port` accepted; `pipe:` / `unix:`
  rejected with focused `ParseError`.
- [x] `resolve(&self) -> Result<SocketAddr, _>` short-circuits the
  literal string `"localhost"` to `Ipv4Addr::LOCALHOST` (dual-stack
  Windows consistency); otherwise delegates to `ToSocketAddrs`.
- [x] `Cli::listen` widened from `Vec<u16>` to `Vec<ListenSpec>`
  via `value_parser = ListenSpec::parse`.
- [x] 18 unit tests in `listen_spec.rs::tests`: bare port,
  `tcp://127.0.0.1:3010`, `tcp://localhost`, `tcp://localhost:3010`,
  `tcp://:3010`, `tcp://[::1]:3010`, both-defaults (`tcp://` →
  `localhost:3000` per Q-001), malformed (`tcp://host:notnum`,
  unbalanced bracket, `garbage`), rejected (`pipe:...`, `unix:...`).
- [x] Verify: `cargo test --workspace`; all 78 existing probes
  green.
- Commit: `feat(stage-6h): slice 1 — ListenSpec type + tcp:// parser`
  (70369fa).

## Slice 2 — Un-defer `tcp://` + `-p`, probe coverage

- [x] In `tools/probe/run.mjs::L0_DEFERRED_FLAGS`, drop `-p` and
  the `tcp://` listen-value check.
- [x] Add `Cli::p: Vec<ListenSpec>` (hidden `-p` short-only flag);
  merge `cli.p` into `cli.listen` post-parse in `main` — clap
  cannot deposit two distinct flags into one `Vec`.
- [x] New runner feature: `runner.defaultPortScenario="free-port"`
  + `{port}` substitution in `serveArgs`.
- [x] New probe `tools/probe/cases/cli-tcp-uri.json` —
  `serveArgs: ["--listen", "tcp://127.0.0.1:{port}"]` +
  `skipAutoListen=true`, `GET /` → 200.
- [x] New probe `tools/probe/cases/cli-p-alias.json` —
  `serveArgs: ["-p", "{port}"]` + `skipAutoListen=true`,
  `GET /` → 200.
- [x] Snapshots recorded via `--snapshot=update --target=reference`.
- [x] Verify: new probes green on both `target=reference` and
  `target=irserve`.
- Commit: `feat(stage-6h): slice 2 — un-defer tcp:// + -p, probe coverage`
  (04dba9d).

## Slice 3 — `--cors` flag + 4-header CORS pass

- [x] New module `crates/irserve-core/src/cors.rs` with
  `apply_cors(Response<Body>) -> Response<Body>` inserting all four
  reference headers unconditionally
  (`access-control-allow-origin: *`,
  `access-control-allow-headers: *`,
  `access-control-allow-credentials: true`,
  `access-control-allow-private-network: true`). Mirrors
  `third_party/serve/source/utilities/server.ts:65-70`.
- [x] `crates/irserve-core/src/lib.rs` — `pub mod cors;`.
- [x] `ServerConfig::cors: bool` + `AppState::cors`; in
  `server::handler`, wire `apply_cors` AFTER `apply_custom_headers`
  so the four headers ride on 3xx redirects too
  (`apply_custom_headers` skips redirections per D-015 finding #2).
- [x] `Cli::cors: bool` (`-C` / `--cors`).
- [x] Drop `-C`, `--cors` from `L0_DEFERRED_FLAGS`. Lift
  `cors-flag`, `cors-applied`, `cors-response-surface` off the gate
  by adding `runner.l0` partitions (reference snapshots unchanged).
- [x] New probe `tools/probe/cases/cors-on-redirect.json` —
  fixture with `serve.json` redirect `/old → /new`;
  `serveArgs: ["--cors"]`; `GET /old` → 301 + four CORS headers.
- [x] 4 unit tests on `apply_cors` covering the four-header set
  and overwrite semantics.
- [x] Verify: all four CORS probes green on both targets.
- Commit: `feat(stage-6h): slice 3 — --cors flag + 4-header CORS pass`
  (ec865ff).

## Slice 4 — Port-switching default + `--no-port-switching` contract (D-016)

- [x] New `bind_with_fallback(addr, allow_switching) ->
  Result<TcpListener, Error>` in `crates/irserve-core/src/server.rs`.
  Catches `ErrorKind::AddrInUse` only; retries on
  `SocketAddr::new(addr.ip(), 0)` for OS allocation when
  `allow_switching=true`; otherwise surfaces
  `Error::PortInUse { addr }`.
- [x] New `Error::PortInUse { addr }` variant (existing `Io` arm
  unchanged).
- [x] Stderr emits a one-line message on retry / refusal — format
  implementation-defined per D-002.
- [x] `ServerConfig::no_port_switching: bool` + `Cli::no_port_switching`
  (long-only).
- [x] Drop `--no-port-switching` from `L0_DEFERRED_FLAGS`.
- [x] Three integration tests in
  `crates/irserve-core/src/server.rs::tests`:
  - `bind_retries_on_addr_in_use_when_switching_allowed`,
  - `bind_fails_on_addr_in_use_when_switching_disabled`,
  - `bind_succeeds_on_free_port`.
- [x] No new probe — busy-port pre-binding from the runner is
  impractical for one case (Note on SRV-CLI-016 + D-016 Impact).
- [x] D-016 entry drafted in slice 6 (`docs/reference/serve/decisions.md`).
- Commit: `feat(stage-6h): slice 4 — port-switching default + --no-port-switching contract (D-016)`
  (296142c).

## Slice 5 — `--debug` + `--no-request-logging` + per-request log

- [x] `Cli::debug: bool` (`-d` / `--debug`),
  `Cli::no_request_logging: bool` (`-L` / `--no-request-logging`).
- [x] `ServerConfig::{debug, no_request_logging}` +
  `AppState::{debug, no_request_logging}`.
- [x] In `server::handler`, after `apply_cors`: guarded `println!`
  shape `{method} {path} -> {status}` per request, with `(Xms)`
  suffix under `--debug`, fully silenced under
  `--no-request-logging`. Format implementation-defined per D-002.
- [x] Drop `-d`, `--debug`, `-L`, `--no-request-logging` from
  `L0_DEFERRED_FLAGS`. The set is now empty — all stage-6h flags
  accepted.
- [x] New probe `tools/probe/cases/cli-debug-flag.json` —
  acceptance, `GET /` → 200.
- [x] New probe `tools/probe/cases/cli-no-request-logging.json` —
  acceptance, `GET /` → 200.
- [x] Snapshots recorded.
- [x] Verify: all probes green.
- Commit: `feat(stage-6h): slice 5 — --debug + --no-request-logging + per-request log`
  (866521a).

## Slice 6 — Spec deltas + meta (this slice)

- [x] Author `openspec/changes/010-cli-fill-in/`:
  - [x] `proposal.md` — closes SRV-CLI-003 / 006 / 010 / 014 / 015 /
    016; Q-001 closed via SRV-CLI-003 Note; out-of-scope list
    mirrors the kickoff plan's 14-item pre-stage list.
  - [x] `design.md` — ListenSpec architecture + Q-001 closure,
    `-p` post-parse merge, CORS placement vs `apply_custom_headers`,
    port-switching contract / D-016 trade-off, `--debug` /
    `--no-request-logging` rationale, probe + unit-test coverage
    strategy, methodological signals.
  - [x] `tasks.md` (this file).
  - [x] `specs/cli/spec.md` — MODIFIED Requirements: SRV-CLI-001
    (Q-001 Note pointing to SRV-CLI-003), SRV-CLI-003 (`accepted` →
    `verified`, Q-001 defaults Note), SRV-CLI-006 (`accepted` →
    `verified`), SRV-CLI-010 (four-header surface wording, CORS-
    on-redirect Note), SRV-CLI-014 (elapsed-ms suffix Note),
    SRV-CLI-015 (silencing-effect contractual), SRV-CLI-016
    (`accepted` → `adapted` per D-016, default-retry Scenario,
    D-016 Compatibility Note).
- [x] New D-016 entry in `docs/reference/serve/decisions.md`.
- [x] Light D-002 update — SRV-CLI-016 entry clarified
  (bind/exit IS contractual; only wording is implementation-
  defined).
- [x] Q-001 closure in `docs/reference/serve/open-questions.md`.
- [x] `README.md`: flip Stage 6h row to `done`; add Stage-6h
  status bullet; update "Try IrServe" with new flag examples.
- [x] `docs/stage6_l1_l2_capabilities.md`: append
  `(done — \`010-cli-fill-in\`)` to the 6h row's name cell.
- [x] Run `npx -y @fission-ai/openspec@latest validate --all
  --strict` and report any failures.
- Commit: `docs(stage-6h): spec deltas + D-016 + Q-001 closure + meta`.

## Validation

- `cargo test --workspace` — green (slices 1-5).
- Oracle harness: `target=irserve total=80 passed=72 skipped=8
  failed=0`; reference 80/80 via `--snapshot=verify`.
- Unit / integration tests added across slices: 18 (listen_spec) +
  4 (cors) + 3 (server bind_with_fallback) = 25 new tests, plus
  the slice-5 println path covered by the two acceptance probes.
- `openspec validate --all --strict` — green (slice 6 final
  step).
