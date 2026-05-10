# Delta for cli

## MODIFIED Requirements

### Requirement: Default port and host

The server SHALL listen on TCP port 3000 when no `--listen`/`-l` flag
is given and `PORT` is unset. When `PORT` is set in the environment
and no `--listen` is given, the server SHALL listen on that port
instead.

Evidence: SRV-CLI-001 (status: accepted, level: L0); oracle: no probe.

Note: Source-level evidence in `serve/source/main.ts` documents the
default. Every existing probe passes an explicit `--listen <port>`, so
the default-port code path is not exercised. This SRV remains
`accepted`; promotion to `verified` is a Stage-5b probe TODO.

Note (Q-001, closed): the `tcp://` URI form of `-l`/`--listen` (see
SRV-CLI-003) honors the same defaults — host=`localhost`, port=`3000`
— when either field of the URI is omitted. Mirrors
`third_party/serve/source/utilities/cli.ts:128-130`.

#### Scenario: No flag, no environment variable

- GIVEN no `--listen` flag and unset `PORT`
- WHEN the server starts
- THEN it listens on port 3000

#### Scenario: PORT environment variable

- GIVEN `PORT=4567` in the environment and no `--listen`
- WHEN the server starts
- THEN it listens on port 4567

### Requirement: `tcp://host:port` URI for `-l`/`--listen`

The `-l`/`--listen` flag SHALL accept a `tcp://host:port` URI. The
server SHALL bind the given TCP port on the given host. The URI form
SHALL apply default values when either field is omitted: missing port
defaults to 3000, missing host defaults to `localhost`. The IPv6
bracketed form `tcp://[::1]:port` SHALL be accepted. Other schemes
(`pipe:`, `unix:`) SHALL be rejected with a non-zero exit and a
focused error message.

Evidence: SRV-CLI-003 (status: verified, level: L1); oracle: ORC-163
(`cli-tcp-uri`).

Note: Q-001 closed by this requirement's defaults clause. Mirrors
`third_party/serve/source/utilities/cli.ts:128-130` (the `parseEndpoint`
helper). The hand-rolled parser in `crates/irserve/src/listen_spec.rs`
applies the defaults inline in the `tcp://` branch.

Note: `ListenSpec::resolve` short-circuits the literal host string
`"localhost"` to `Ipv4Addr::LOCALHOST` rather than delegating to
`ToSocketAddrs`. Dual-stack Windows sometimes ranks `::1` first via
`dns.lookup`; the short-circuit keeps the manual smoke
(`curl http://localhost:3000/`) predictable across platforms and
matches the bare-port form's default interface. Real IPv6 binds still
work via the explicit `[::1]:port` form.

#### Scenario: TCP URI

- GIVEN `serve -l tcp://127.0.0.1:3011`
- WHEN a client opens `http://127.0.0.1:3011/`
- THEN it reaches the server

#### Scenario: Default port when omitted from URI

- GIVEN `serve -l tcp://localhost`
- WHEN started
- THEN the server listens on TCP port 3000 on host `localhost`

#### Scenario: Default host when omitted from URI

- GIVEN `serve -l tcp://:3010`
- WHEN started
- THEN the server listens on TCP port 3010 on host `localhost`

#### Scenario: Unsupported scheme is rejected

- GIVEN `serve -l pipe:foo` or `serve -l unix:/tmp/sock`
- WHEN started
- THEN the process exits with a non-zero code and an error message on stderr

### Requirement: `-p` deprecated alias for `-l`/`--listen`

The `-p` flag SHALL be accepted as an alias for `-l`/`--listen` and
SHALL parse the same value forms (numeric port or `tcp://` URI).
Mixed forms across `-l` and `-p` (e.g. `-l 3010 -p tcp://:3011`)
SHALL accumulate as additional listen specifications.

Evidence: SRV-CLI-006 (status: verified, level: L1); oracle: ORC-164
(`cli-p-alias`).

Note: `-p` is documented in source comments as kept for backwards
compatibility; the reference accepts it silently and so does irserve
(no deprecation warning). Internally `Cli::p` is a separate hidden
`Vec<ListenSpec>` because clap cannot deposit two distinct flag
spellings into the same `Vec` — the merge happens post-parse in
`crates/irserve/src/main.rs`.

#### Scenario: Deprecated alias

- GIVEN `serve -p 3010`
- WHEN the server starts
- THEN it behaves identically to `serve -l 3010`

### Requirement: `-C`/`--cors` enables permissive CORS headers

With `-C`/`--cors`, every response SHALL carry
`Access-Control-Allow-Origin: *`,
`Access-Control-Allow-Headers: *`,
`Access-Control-Allow-Credentials: true`, and
`Access-Control-Allow-Private-Network: true` as defaults. A user
`serve.json#headers` rule whose key matches one of the four CORS
header names SHALL override the corresponding default; the other
three defaults still fill in. The full L3 differentiated CORS
surface (preflight `OPTIONS`, `Access-Control-Max-Age`,
`Access-Control-Expose-Headers`) is documented as a separate L3
requirement under the `cors` capability and is not part of this
baseline.

Evidence: SRV-CLI-010 (status: verified, level: L1); oracle:
ORC-054, ORC-055; plus the runner-`l0` partitions of `cors-flag`,
`cors-applied`, `cors-response-surface`, `cors-on-redirect`, and
`cors-user-override`.

Note (CORS on redirects): the four CORS defaults ride on 3xx
responses too. Mirrors the reference's `setHeader` pre-pass at
`third_party/serve/source/utilities/server.ts:65-70`, which runs
BEFORE `serve-handler` and so survives the redirect branch at
`serve-handler/src/index.js:586-588` (which does not copy default
headers). irserve implements via a post-dispatch `apply_cors` call
in `server::handler`, placed AFTER `apply_custom_headers` (which
skips redirections per D-015 finding #2). Verified by
`cors-on-redirect`.

Note (user-rule precedence): a user `headers` rule setting any of
the four CORS keys wins over the CLI flag's default. Mirrors the
reference's order: `server.ts:65-70` calls `setHeader('ACAO','*')`
BEFORE `serve-handler` runs; `serve-handler` then merges user
rules via `Object.assign(defaultHeaders, related)`
(`serve-handler/src/index.js:245`) and writes them back through
`response.setHeader` (`:767`), overwriting the CLI default. irserve
mirrors with set-only-if-missing semantics in `apply_cors`: it
inserts each CORS default only when the response does not already
carry that header from `apply_custom_headers`. Verified by
`cors-user-override` (a `**/*.css` rule sets
`access-control-allow-origin: https://example.test` and the value
survives intact while the other three defaults fill in).

#### Scenario: All four CORS defaults on a clean 200

- GIVEN `serve --cors` with no user `headers` rule
- WHEN any request returns 200
- THEN the response includes
  `Access-Control-Allow-Origin: *` and the three companion CORS
  defaults

#### Scenario: All four CORS headers on a 3xx redirect

- GIVEN `serve --cors` with `serve.json` redirect
  `{source: "/old", destination: "/new", type: 302}`
- WHEN `GET /old`
- THEN status is 302
- AND the response carries all four CORS defaults
  (`access-control-allow-origin`, `access-control-allow-headers`,
  `access-control-allow-credentials`,
  `access-control-allow-private-network`)

#### Scenario: User rule overrides a CORS default

- GIVEN `serve --cors` with a `serve.json#headers` rule that sets
  `access-control-allow-origin: https://example.test` for the
  request path
- WHEN the request returns 200
- THEN the response includes
  `Access-Control-Allow-Origin: https://example.test`
- AND the other three CORS defaults still appear unchanged

### Requirement: `-d`/`--debug` toggles verbose output

The `-d`/`--debug` flag SHALL be accepted. IrServe MAY map it to a
verbosity level of its own choosing. The exact log format is
implementation-defined.

Evidence: SRV-CLI-014 (status: adapted, level: L1); oracle: no probe
beyond acceptance (the elapsed-ms suffix is stdout-only and stripped
from snapshot envelopes per D-002).

Note: Per `decisions.md` D-002, exact terminal output / stdout
formatting is not in scope. Oracle tests MUST NOT compare stdout/stderr
text for this flag.

Note: irserve appends an elapsed-ms suffix `(Xms)` to the per-request
log line under `--debug`; otherwise the flag is observably a no-op on
the HTTP surface. The print site is a single `println!` in
`server::handler` after `apply_cors`. Format implementation-defined
per D-002.

#### Scenario: Flag is accepted

- GIVEN `serve --debug -l 3010 ./fixture`
- WHEN started
- THEN startup succeeds and the HTTP behavior is unchanged from the no-debug case

### Requirement: `-L`/`--no-request-logging` silences per-request logs

The `-L`/`--no-request-logging` flag SHALL be accepted. When set,
IrServe SHALL NOT emit per-request log lines to stdout or stderr. The
exact log format when the flag is unset is implementation-defined.

Evidence: SRV-CLI-015 (status: adapted, level: L1); oracle: no probe
beyond acceptance (the log surface is stdout-only and stripped from
snapshot envelopes per D-002).

Note: Per `decisions.md` D-002, exact terminal output is not in scope.
The flag's silencing effect, however, IS contractual — under
`--no-request-logging` no per-request line is written to stdout or
stderr.

Note: When unset, irserve emits a single `{method} {path} -> {status}`
line per request to stdout (with `(Xms)` suffix under `--debug`).
Format implementation-defined per D-002.

#### Scenario: Flag is accepted, no per-request logs emitted

- GIVEN `serve --no-request-logging -l 3010 ./fixture`
- WHEN HTTP requests are processed
- THEN no per-request log line is written to stdout or stderr

### Requirement: `--no-port-switching` disables fallback to a random port

The server SHALL by default retry on an ephemeral port (host kept,
port replaced with `0` for OS allocation) when the requested listen
address is already in use. With `--no-port-switching`, the server
SHALL fail to start (non-zero exit code) instead of falling back.

Evidence: SRV-CLI-016 (status: adapted, level: L1); oracle: no probe
(failure-mode is verified by integration tests in
`crates/irserve-core/src/server.rs::tests::bind_fails_on_addr_in_use_when_switching_disabled`
and `::bind_retries_on_addr_in_use_when_switching_allowed`).
Probe coverage of the busy-port fail-mode is intentionally omitted —
pre-binding a port from the probe runner is impractical for one
case. See D-016.

Note (stderr wording): the warning emitted on retry and the error
emitted on refusal are implementation-defined per D-002. The bind /
exit behavior itself IS contractual; only the wording is free to
evolve.

Note (multi-listen): with `-l 3010 -l 3011 --no-port-switching`, if
3010 is busy, the server SHALL exit on the first failure — the
second listen address is never reached. Any one bind failure aborts
startup. Kickoff out-of-scope #14.

Compatibility note (D-016, upstream divergence): the reference
declares `--no-port-switching` at `third_party/serve/source/utilities/cli.ts:158`
but never reads it in `server.ts`'s `startServer` — vercel/serve#751,
open since 2022-12, a regression introduced in v14.0.0 that worked
in v13.0.4. On the reference, both default and `--no-port-switching`
collapse to "retry on `port: 0`". irserve enforces the documented
contract (default retry; flag forces exit) under D-004's
no-bug-for-bug-parity authorization. Probe runner injects
`--no-port-switching` for every reference invocation
(`tools/probe/run.mjs:172`) but it is a no-op there; the runner does
NOT inject the flag for irserve invocations because irserve's
default retry is unobservable on the runner's auto-allocated free
ports. Anyone validating irserve against the reference must remember
the flag is a no-op upstream; comparison requires a harness that
actually pre-binds the requested port.

#### Scenario: Default fallback to ephemeral port

- GIVEN no `--no-port-switching` flag and the requested listen
  address `127.0.0.1:3010` is already in use
- WHEN the server starts
- THEN startup succeeds on `127.0.0.1:N` for some OS-allocated `N`
- AND a one-line warning is emitted on stderr (wording
  implementation-defined per D-002)

#### Scenario: Refuse fallback on occupied port

- GIVEN `--no-port-switching` and the requested listen address is
  already in use
- WHEN started
- THEN the process exits with a non-zero code
- AND a one-line error is emitted on stderr (wording
  implementation-defined per D-002)
