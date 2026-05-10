# Design: CLI fill-in — `tcp://`, `-p`, `--cors`, `--debug`, `--no-request-logging`, `--no-port-switching`

This document records the architecture for Stage 6h. 6h is mechanically
simpler than 6f/6g — six flags, one new module per concern, no new
phases in the 13-phase dispatcher. The architectural foundations
(crate layout, HTTP-stack pins, request lifecycle, oracle harness
layer) live in
`openspec/changes/001-port-minimal-static-server/design.md`. The
contract for the capability lives in
`openspec/specs/cli/spec.md` (SRV-CLI-001..019). 6h does not touch
the dispatcher; every change is in the CLI layer
(`crates/irserve/src/main.rs` + `listen_spec.rs`), the new
`crates/irserve-core/src/cors.rs` module, or `server.rs`'s `handler`
+ `bind_with_fallback`.

## 1. `ListenSpec` architecture and Q-001 closure

`Cli::listen` was `Vec<u16>` through Stage 6g — sufficient for
`-l 3010` but rejecting `--listen tcp://...`. Slice 1 widens to
`Vec<ListenSpec>`:

```rust
pub enum ListenSpec {
    Port(u16),
    Tcp { host: String, port: u16 },
}
```

`ListenSpec::parse(s: &str) -> Result<Self, ParseError>` is the
`value_parser` for both `--listen` and `-p`. The parser is
hand-rolled (no `url` crate dependency for two call sites) and
mirrors `third_party/serve/source/utilities/cli.ts:104-143`'s
`parseEndpoint`:

- bare-numeric → `Port(N)`;
- `tcp://...` → split-host-port with IPv6-bracket support;
- `pipe:` / `unix:` → focused `Err`, "not supported on this
  platform" wording per kickoff out-of-scope #1/#2.

**Q-001 defaults** are applied inline in the `tcp://` branch
(`cli.ts:128-130`):

- `tcp://hostname` (no port) → port = 3000;
- `tcp://:port` (no host) → host = `"localhost"`;
- `tcp://` alone → both defaults.

Closing Q-001 is a Note on SRV-CLI-003 plus a Resolution line in
`docs/reference/serve/open-questions.md` — not a D-NNN. The contract
lives in the spec text.

`ListenSpec::resolve(&self) -> Result<SocketAddr, _>` short-circuits
the literal string `"localhost"` to `Ipv4Addr::LOCALHOST` before
calling `ToSocketAddrs`. Dual-stack Windows sometimes ranks `::1`
first via `dns.lookup`, which the reference also does not control
for. The short-circuit matches the bare-port form's default
interface and keeps the manual smoke (`curl http://localhost:3000/`)
predictable across platforms. Real IPv6 binds still work via
`[::1]:port`; the parser accepts the bracketed form and `resolve`
delegates to `ToSocketAddrs` for non-literal hosts.

18 unit tests in `listen_spec.rs::tests` cover: bare port,
`tcp://127.0.0.1:3010`, `tcp://localhost`, `tcp://localhost:3010`,
`tcp://:3010`, `tcp://[::1]:3010`, malformed (`tcp://`, `tcp://
host:notnum`), and rejected (`pipe:`, `unix:`).

## 2. `-p` post-parse merge

clap's `aliases` mechanism wires multiple spellings to the same
flag, but they share the same internal name and `Vec` collector.
Wanting `-l` and `-p` to deposit into a single `Vec<ListenSpec>`
while remaining two distinct flags (the reference treats them as
synonyms for backward compatibility per its source comments) is
not expressible in clap. The slice-2 wiring:

```rust
struct Cli {
    #[arg(short = 'l', long = "listen", value_parser = ListenSpec::parse)]
    listen: Vec<ListenSpec>,

    #[arg(short = 'p', hide = true, value_parser = ListenSpec::parse)]
    p: Vec<ListenSpec>,
    // ...
}

fn main() {
    let mut cli = Cli::parse();
    cli.listen.extend(cli.p.drain(..));   // post-parse merge
    // ...
}
```

`-p` is hidden (`hide = true`) — the reference's source comment is
"kept for backwards compatibility"; the help text omits it and so
does irserve's. Mixed forms (`-l 3010 -p tcp://localhost:3011`) are
not a purpose-built test target but the type accumulates them
correctly (kickoff out-of-scope #14).

## 3. CORS placement vs `apply_custom_headers`

The reference's `server.ts:65-70`:

```js
response.setHeader('access-control-allow-origin', '*');
response.setHeader('access-control-allow-credentials', 'true');
response.setHeader('access-control-allow-headers', '*');
response.setHeader('access-control-allow-private-network', 'true');
```

is set BEFORE the request enters `serve-handler`, so it survives
the redirect branch at `serve-handler/src/index.js:586-588` (which
calls `response.writeHead(redirect.statusCode, { Location: ... })`
without copying defaults). The four headers therefore ride on 3xx
responses too.

irserve's `apply_custom_headers` short-circuits on
`is_redirection()` (D-015 finding #2) because it mirrors the
`getHeaders` call site, not the CORS pre-pass. So we need a
separate, unconditional post-dispatch pass:

```rust
// server.rs::handler, simplified
let resp = dispatch(req, &state).await;
let resp = if state.cors { apply_cors(resp) } else { resp };
// log line, then return resp
```

`apply_cors` inserts the four headers in declaration order; any
prior rule-emitted value for the same key is overwritten (a user
`headers` rule setting `access-control-allow-origin: foo` loses
to the post-pass `*`, matching the reference's `setHeader` order).
4 unit tests pin the four-header set and the overwrite semantics.

Verified by:

- `cors-flag` / `cors-applied` / `cors-response-surface` (existing
  Stage-1 snapshots, lifted off the runner gate in slice 3),
- `cors-on-redirect` (NEW slice 3) — exercises the 3xx
  pass-through with a `serve.json` redirect `/old → /new`.

## 4. Port-switching contract / D-016 trade-off

D-016 records the divergence in full. The mechanical shape:

```rust
async fn bind_with_fallback(
    addr: SocketAddr,
    allow_switching: bool,
) -> Result<TcpListener, Error> {
    match TcpListener::bind(addr).await {
        Ok(l) => Ok(l),
        Err(e) if e.kind() == ErrorKind::AddrInUse && allow_switching => {
            eprintln!("warning: listen address {addr} is already in use, switching to ephemeral");
            let fallback = SocketAddr::new(addr.ip(), 0);
            TcpListener::bind(fallback).await.map_err(Error::Io)
        }
        Err(e) if e.kind() == ErrorKind::AddrInUse => {
            eprintln!("error: listen address {addr} is already in use (--no-port-switching is set)");
            Err(Error::PortInUse { addr })
        }
        Err(e) => Err(Error::Io(e)),
    }
}
```

Three integration tests in `crates/irserve-core/src/server.rs::tests`:

- `bind_retries_on_addr_in_use_when_switching_allowed` — pre-bind a
  port via `TcpListener::bind("127.0.0.1:0")`, capture the
  assigned port, call `bind_with_fallback` on it with
  `allow_switching=true`, expect `Ok` on a different port.
- `bind_fails_on_addr_in_use_when_switching_disabled` — same setup
  with `allow_switching=false`, expect `Err(PortInUse)`.
- `bind_succeeds_on_free_port` — happy path; `bind_with_fallback`
  on `127.0.0.1:0` returns `Ok` and matches the listener's local
  port.

Probe coverage of the busy-port fail-mode is **intentionally
omitted**. The probe runner allocates a free port per case and
spawns the server fresh; pre-binding the same port from JavaScript
before calling `serveArgs` would require a separate worker holding
the socket open across the spawn — far more harness mechanism than
the one verified-by-unit-test case justifies. Documented as a Note
on SRV-CLI-016 and in D-016 Impact.

Stderr message format is implementation-defined per the light
D-002 update (the wording is excluded; the bind / exit behavior is
contractual).

## 5. `--debug` / `--no-request-logging` rationale

Two paths were available:

1. **Full no-op under D-002**: accept both flags, emit nothing
   either way. The reference's log format is `chalk`-styled
   `{date} {ip} {url}`; mirroring byte-for-byte is out of scope per
   rule #5. A no-op is the minimum-viable interpretation.

2. **Minimal real log** (chosen): emit one stdout line per request,
   shape `{method} {path} -> {status}` with `(Xms)` suffix under
   `--debug`. `--no-request-logging` silences it. No `tracing`
   framework — a single `println!` in `server::handler` after
   `apply_cors`.

Option (2) was confirmed in the kickoff interview. Rationale:

- SRV-CLI-015's existing Scenario already says "no per-request log
  line written" when the flag is set. Under option (1) that
  Scenario is vacuously satisfied; under (2) it becomes contractual
  and falsifiable (the slice-5 probes `cli-debug-flag` and
  `cli-no-request-logging` confirm acceptance; the silencing effect
  is wire-observable to anyone running the binary).
- The runner strips stdout/stderr from snapshot envelopes (D-002),
  so no probe pins the exact wording. Format remains free to
  evolve.
- A `(Xms)` suffix under `--debug` keeps the flag observably
  distinct from the no-flag case without buying into the
  reference's update-check verbosity (`main.ts:37,40`), which has
  no irserve analog (kickoff out-of-scope #10).

`ServerConfig::{debug, no_request_logging}` and `AppState::{debug,
no_request_logging}` carry the flags. The print site:

```rust
if !state.no_request_logging {
    let suffix = if state.debug { format!(" ({}ms)", elapsed.as_millis()) }
                 else { String::new() };
    println!("{method} {path} -> {status}{suffix}");
}
```

## 6. Probe + unit-test coverage strategy

Probe coverage was chosen flag-by-flag based on what the runner can
observe symmetrically against both targets:

| Flag                     | Probe                                       | Unit tests                       |
|--------------------------|---------------------------------------------|----------------------------------|
| `--listen tcp://...`     | `cli-tcp-uri` (slice 2)                     | 18 in `listen_spec.rs::tests`    |
| `-p`                     | `cli-p-alias` (slice 2)                     | (parser shared)                  |
| `--cors`                 | 4 cases (3 lifted + `cors-on-redirect`)     | 4 in `cors.rs::tests`            |
| `--no-port-switching`    | NONE — failure-mode unit-only               | 3 in `server::tests` (slice 4)   |
| `--debug`                | `cli-debug-flag` (acceptance, 200)          | (println path)                   |
| `--no-request-logging`   | `cli-no-request-logging` (acceptance, 200)  | (println path)                   |

The two-row asymmetries are deliberate:

- **`--no-port-switching` busy-port case.** Pre-binding a port from
  the probe runner requires a separate worker holding the socket
  across the spawn. The unit tests use `tokio::net::TcpListener`
  in-process — far cleaner. The contract is verified, just not via
  the oracle envelope.
- **`--debug` / `--no-request-logging` wire surface.** Both
  responses are byte-identical to the no-flag case (the print site
  writes to stdout, not into the response). Probe snapshots
  therefore only confirm the binary accepts the flag and the HTTP
  surface is unchanged; the silencing-vs-emitting distinction is
  observable on the binary's stdout, which the runner strips
  (D-002).

Free-port allocation for slice-2 probes uses the new runner feature
`runner.defaultPortScenario="free-port"` + `{port}` substitution in
`serveArgs` (added in slice 2). Other cases that hardcode a port
remain unchanged.

## 7. Methodological signals

This stage authored **one new D-NNN entry** (D-016) and a light
update to D-002. No other divergences required new decisions:

- The `tcp://` `pipe:` / `unix:` rejection inherits the kickoff
  out-of-scope list (rule #10); no D-NNN.
- CORS four-header surface inherits the existing SRV-CLI-010
  evidence; the wording tightening on the spec is editorial, not a
  divergence.
- `--debug` / `--no-request-logging` log format is implementation-
  defined per D-002; the flag-silences-stdout effect is the
  contractual addition documented in the spec delta's Notes.
- Q-001 closure is spec-text-only — a Note on SRV-CLI-003 plus a
  Resolution line in `open-questions.md` — not a separate D-NNN.

D-002 itself gains a parenthetical clarification on the SRV-CLI-016
entry: the bind/exit behavior IS contractual; only the warning/
error wording is implementation-defined. Recorded inline in
`docs/reference/serve/decisions.md` D-002 so future maintenance
does not re-deferralize the bind/exit contract.

The absence of new D-NNN entries beyond D-016 is itself a signal:
6h is mechanically the simplest stage in Stage 6. Future
maintenance touching CLI behavior should consult D-002, D-004, and
D-016 before authoring a new decision.

## 8. Verification

`cargo test --workspace` covers the unit and integration tests
listed in §6.

`cargo test --test oracle` runs the full harness against
`target=irserve`: 80 probes total / 72 passed / 8 skipped / 0 failed.

Reference still 80 of 80 green via `node tools/probe/run.mjs --all
--target=reference --snapshot=verify`. No reference snapshots were
re-recorded — reference behavior did not change.

Manual smoke commands are documented in `README.md`'s "Try IrServe
(post-6h)" section.
