# Stage 6h — CLI fill-in (`tcp://`, `-p`, `--cors`, `--debug`, `--no-request-logging`, `--no-port-switching`)

## Context

Stage 6h is the terminal slice of the CLI capability. It closes six SRVs from
the inventory (`SRV-CLI-003` `tcp://`, `SRV-CLI-006` `-p` alias, `SRV-CLI-010`
`--cors`, `SRV-CLI-014` `--debug`, `SRV-CLI-015` `--no-request-logging`,
`SRV-CLI-016` `--no-port-switching`) and closes Q-001 (defaults for `tcp://`).
After it lands, the `6h` row in README/roadmap flips to `done`, and Stage 6
(L1+L2) is fully closed — only Stage 7 (L3 polish) remains.

`SRV-CLI-009` (`--config`) was closed back in 6a as a side effect of the
serve.json loader; it does not belong here.

Today irserve accepts only `--version`, `-l`/`--listen` (as `Vec<u16>`),
`-n`/`--no-clipboard`, `-c`/`--config`, `-s`/`--single`, and the positional
`DIRECTORY`. All six 6h flags are rejected by clap when passed, and the
runner (`tools/probe/run.mjs:49-55`) keeps an explicit `L0_DEFERRED_FLAGS`
list for them — probes carrying these flags do not run against
`target=irserve` (they error out, not silently). The stage lifts both gates
in parallel with the implementation.

## Goal

Change package `openspec/changes/010-cli-fill-in/` (proposal/design/tasks +
MODIFIED deltas for `cli`) plus working code that passes the oracle harness
on the new and existing probes and unit tests. Plan file —
`docs/features/0014_PLAN_stage6h_cli_fill_in.md` (per the 0001..0013
convention).

## Key decisions

### D-016 (new) — `--no-port-switching` corrects an upstream bug

Situation:

| Scenario | Documentation / spec | Reference (`server.ts:166-181`) | irserve before 6h |
|---|---|---|---|
| default, port busy | retry on ephemeral | retry ✓ | exit ✗ |
| `--no-port-switching`, port busy | exit non-zero | retry ✗ (flag never read) | exit ✓ |

The reference declares the flag (`cli.ts:158`) but **never checks it** —
on a busy port it always retries on `port: 0`. Known bug
[vercel/serve#751](https://github.com/vercel/serve/issues/751) (open since
2022-12, regression introduced in `14.0.0` — it worked in `13.0.4`).

Choice: irserve honors the documented contract. Default = retry, with
flag = exit. D-004 (no bug-for-bug parity) explicitly authorizes this.
We do not file a new upstream issue — duplicate.

D-016 records:
- citation of `cli.ts:158` (flag declaration) and `server.ts:166-181`
  (no check),
- link to vercel/serve#751,
- exact wording of the divergence and rationale via D-004,
- a Compatibility Note in the spec, so that during a future regression
  comparison against the reference no one "fixes" this back.

### Q-001 closes via spec text

Not a separate D entry — a Note on SRV-CLI-003: "`tcp://hostname` without
a port → 3000; `tcp://:port` without a host → `localhost`", citing
`cli.ts:128-130`. Q-001 is marked `closed` in `open-questions.md` with a
pointer to SRV-CLI-003.

### `--cors` — all four reference headers

`server.ts:65-70` sets `access-control-allow-origin: *`,
`access-control-allow-headers: *`, `access-control-allow-credentials: true`,
`access-control-allow-private-network: true` — unconditionally, before
dispatch. The `cors-applied.json` / `cors-response-surface.json` snapshots
already pin this. Anti-hallucination rule #4 (oracle > docs): mirror all
four. The spec's help-text wording `«sets ACAO to *»` is a simplified
phrasing, not the exhaustive contract. SRV-CLI-010 is already `verified`.

The roadmap's "L1 ACAO only" for stage 6h refers to differentiated CORS
handling (preflight, max-age, exposed headers), which stays in Stage 7+.
The static headers are already verified, so they belong in 6h.

### `--debug` / `--no-request-logging` — real per-request log

(Confirmed by user.) In `handler` — one stdout line `{method} {path} ->
{status}` per request. `--no-request-logging` silences it. `--debug` adds
an elapsed-time suffix. Format is implementation-defined under D-002.
SRV-CLI-015's "no per-request log line" Scenario becomes contractual.
No `tracing` framework — a single `println!`.

### Listen-spec widening

`Vec<u16>` → `Vec<ListenSpec>` where `ListenSpec` is an enum:
- `Port(u16)` — bare-port form, host = `127.0.0.1` (current behavior),
- `Tcp { host: String, port: u16 }` — for the `tcp://` URI.

Parser in `crates/irserve/src/listen_spec.rs` (new module). No `url`
crate — hand-rolled split, like the reference, so we handle the IPv6
bracketed form correctly and avoid pulling a transitive dep for two call
sites. `pipe:` and `unix:` are rejected with a focused error message
(out of scope for Stage 6h).

### CORS — separate post-dispatch pass

New `crates/irserve-core/src/cors.rs::apply_cors(response)`. Called from
`handler` **after** `apply_custom_headers` (i.e. after `dispatch`),
unconditionally, with no 3xx skip (unlike `apply_custom_headers`). The
reference applies CORS headers via `setHeader` *before* `serve-handler`,
so they survive redirects too. We do not route through
`apply_custom_headers` — it skips redirections and is rule-list driven,
whereas CORS is global and unconditional.

## Pre-stage out-of-scope list (rule #10)

1. `pipe:` scheme (Windows named pipes). Rejected with focused error.
2. `unix:` scheme (UDS). Rejected with focused error.
3. `--ssl-cert` / `--ssl-key` / `--ssl-pass` (Stage 7+).
4. `--no-compression` (D-006).
5. `--no-etag` (Stage 7+).
6. `--symlinks` / `-S` (Stage 7+).
7. The full L3 CORS surface (preflight `OPTIONS`,
   `Access-Control-Max-Age`, `Access-Control-Expose-Headers`). Stage 7+.
8. Banner / `boxen` output at startup (D-002).
9. Exact log-line format — the reference's `{date} {ip} {url}` (D-002);
   irserve picks its own minimal format.
10. `--debug` affecting anything beyond one extra column in the log line.
    The reference uses `--debug` only for update-check verbosity
    (`main.ts:37,40`); irserve has no update check.
11. Reference's hostname-DNS quirks (Node `dns.lookup` vs Rust
    `ToSocketAddrs`). We do not mirror multi-result-ordering differences.
12. IPv6-binding edge cases (dual-stack vs v6-only socket behavior
    differs across OSes). The parser accepts the `[::1]:port` syntax,
    but actual v6 bind is exercised only by a trivial unit test.
13. `-p` deprecation warning. The reference accepts it silently; so do we.
14. Mixed listen forms (`-l 3010 -p tcp://localhost:3011`) — the type
    supports them (`Vec<ListenSpec>` accumulates), but they are not a
    purpose-built test target.

## Approach — slice plan (6 slices, mirroring 6f/6g cadence)

### Slice 1 — `ListenSpec` type + `tcp://` parser (no gate flips yet)

Goal: widen the listen-spec type while keeping all existing probes green.
Wire `tcp://` through clap's `value_parser`, but do not touch
`L0_DEFERRED_FLAGS` yet — there are no `tcp://` probes yet.

Files:
- `crates/irserve/src/listen_spec.rs` — NEW. `ListenSpec` enum + `parse()`
  + `resolve()` (hand-rolled split-host-port, IPv6 brackets, `localhost`
  → `Ipv4Addr::LOCALHOST` short-circuit for Windows consistency).
- `crates/irserve/src/main.rs` — `Cli::listen: Vec<ListenSpec>` with
  `value_parser = ListenSpec::parse`; `resolve_listens` returns
  `Vec<SocketAddr>` via `ListenSpec::resolve`.
- Unit tests in `listen_spec.rs::tests`: bare port, `tcp://127.0.0.1:3010`,
  `tcp://localhost`, `tcp://localhost:3010`, `tcp://:3010`,
  `tcp://[::1]:3010`, malformed (`tcp://`, `tcp://host:notnum`), rejected
  (`pipe:...`, `unix:...`).

Verify: `cargo test --workspace`; all 78 existing probes green.

Commit: `feat(stage-6h): slice 1 — ListenSpec type + tcp:// parser`

### Slice 2 — Lift the `tcp://` + `-p` gate, flip SRV-CLI-003 / -006

Goal: open the runner gate; add two targeted probes.

Files:
- `tools/probe/run.mjs` — drop `-p` from `L0_DEFERRED_FLAGS`. For
  `tcp://`, drop the listen-value check (if present) or just stop
  blocking `--listen tcp://...`.
- `tools/probe/cases/cli-tcp-uri.json` — NEW. `serveArgs:
  ["--listen","tcp://127.0.0.1:{{port}}"]` + `skipAutoListen=true`,
  `GET /` → 200.
- `tools/probe/cases/cli-p-alias.json` — NEW. `serveArgs:
  ["-p","{{port}}"]` + `skipAutoListen=true`, `GET /` → 200.
- Snapshots — `--snapshot=update --target=reference` for the two new probes.

Verify: new probes green on `target=reference` AND `target=irserve`.

Commit: `feat(stage-6h): slice 2 — un-defer tcp:// + -p, probe coverage`

### Slice 3 — `--cors` flag + post-dispatch CORS pass

Goal: implement `--cors` end-to-end, four headers, on every response
(including 3xx).

Files:
- `crates/irserve-core/src/cors.rs` — NEW. `apply_cors(Response<Body>) ->
  Response<Body>` with a `CORS_HEADERS: &[(&str, &str); 4]` constant.
- `crates/irserve-core/src/lib.rs` — `pub mod cors;`.
- `crates/irserve-core/src/server.rs` — `ServerConfig::cors: bool`;
  `AppState::cors`; in `handler`, after `dispatch` — `if state.cors {
  apply_cors(resp) } else { resp }`.
- `crates/irserve/src/main.rs` — `Cli::cors: bool` (`-C` short).
- `tools/probe/run.mjs` — drop `-C`, `--cors` from `L0_DEFERRED_FLAGS`.
- `tools/probe/cases/cors-on-redirect.json` — NEW. Fixture with a
  `serve.json` redirect `/old → /new`; `serveArgs: ["--cors"]`;
  `GET /old` → 301 + 4 CORS headers. Verifies pass-through across 3xx.
- Snapshot for the new probe — re-record reference.

Verify: `cors-flag.json`, `cors-applied.json`, `cors-response-surface.json`
+ `cors-on-redirect.json` all green on both targets. Existing CORS
snapshots untouched.

Commit: `feat(stage-6h): slice 3 — --cors flag + 4-header CORS pass`

### Slice 4 — `--no-port-switching` + default-retry (D-016)

Goal: documented contract. Default = retry on ephemeral; flag = exit
non-zero on `EADDRINUSE`.

Files:
- `crates/irserve-core/src/server.rs` — `bind_with_fallback(addr,
  allow_switching) -> Result<TcpListener, Error>`. Catch
  `ErrorKind::AddrInUse` only; retry binds `SocketAddr::new(addr.ip(), 0)`.
  Stderr messages (format implementation-defined). Replaces the inline
  `TcpListener::bind(addr).await?`.
- `crates/irserve-core/src/error.rs` (or `server.rs`) — `Error::PortInUse
  { addr }` if it does not exist yet.
- `crates/irserve/src/main.rs` — `Cli::no_port_switching: bool`. Thread
  through to `ServerConfig`. `allow_switching = !no_port_switching`.
- `tools/probe/run.mjs` — drop `--no-port-switching` from
  `L0_DEFERRED_FLAGS`. Keep the line-172 injection for the reference
  (the runner wants determinism; the reference's no-op makes
  `--no-port-switching` harmless). Do **not** add the inject for
  irserve — irserve does default retry, which on the runner's free
  ports will not surface anything.
- Integration tests in `crates/irserve-core/src/server.rs::tests`:
  - `bind_retries_on_addr_in_use_when_switching_allowed` — pre-bind
    `127.0.0.1:0`, capture the assigned port, run `bind_with_fallback`
    on that port with `allow_switching=true`, expect `Ok` on a different
    port.
  - `bind_fails_on_addr_in_use_when_switching_disabled` — same setup
    with `allow_switching=false`, expect `Err(PortInUse)`.

Verify: `cargo test --workspace` + all probes green. Probe coverage for
the `--no-port-switching` busy-port case — NONE (the runner does not
support pre-binding cleanly; unit tests cover it). Document in
SRV-CLI-016's Note: "failure mode verified by unit test, not probe".

Commit: `feat(stage-6h): slice 4 — port-switching default + --no-port-switching contract (D-016)`

### Slice 5 — `--debug` + `--no-request-logging` + minimal per-request log

Goal: per-request stdout log, two flags drive it.

Files:
- `crates/irserve/src/main.rs` — `Cli::debug: bool` (`-d`),
  `Cli::no_request_logging: bool` (`-L`).
- `crates/irserve-core/src/server.rs` — `ServerConfig::{debug,
  no_request_logging}`; in `handler`, after the `resp` is finalized
  (and after `apply_cors`) — guarded `println!`. With `--debug` — append
  `elapsed.as_millis()`.
- `tools/probe/run.mjs` — drop `-d`, `--debug`, `-L`,
  `--no-request-logging` from `L0_DEFERRED_FLAGS`.
- `tools/probe/cases/cli-debug-flag.json` — NEW. Acceptance:
  `serveArgs: ["--debug"]` + `GET /` → 200.
- `tools/probe/cases/cli-no-request-logging.json` — NEW. Acceptance.
- Snapshots for the two new cases.

Verify: all probes green. The runner already strips stdout/stderr from
snapshots, so log lines do not enter the diff envelope. The
`cli-help-version.json` probe will likely re-record (clap auto-generates
`--help`, and it grows with the new flags) — re-record via
`--snapshot=update --target=irserve` and confirm the case is marked
`bodyMayDiffer` or equivalent (D-002).

Commit: `feat(stage-6h): slice 5 — --debug + --no-request-logging + per-request log`

### Slice 6 — Spec deltas + meta (delegate to subagent)

Goal: openspec change package + status flips + Q-001 closure + D-016 entry
+ README/roadmap flips. Delegated to a subagent with a briefing (slice
plan + commit log + D-016 text + peer change package
`009-directory-listing` for style). The main agent reviews and Edits if
needed.

Files:
- `openspec/changes/010-cli-fill-in/proposal.md` — NEW.
- `openspec/changes/010-cli-fill-in/design.md` — NEW.
- `openspec/changes/010-cli-fill-in/tasks.md` — NEW (mark `[x]` for
  completed slices 1-5).
- `openspec/changes/010-cli-fill-in/specs/cli/spec.md` — MODIFIED
  Requirements:
  - **SRV-CLI-001** (`Default port and host`) — Note adding a pointer
    to SRV-CLI-003 for the default-host/port `tcp://` semantics.
  - **SRV-CLI-003** (`tcp://host:port` URI) — `accepted` → `verified`.
    Evidence: ORC-NNN (new, `cli-tcp-uri`). Note about defaults
    (host=`localhost`, port=`3000`), citing `cli.ts:128-130`. Drop
    "transitive over SRV-CLI-002".
  - **SRV-CLI-006** (`-p` alias) — `accepted` → `verified`. Evidence:
    ORC-NNN+1 (`cli-p-alias`).
  - **SRV-CLI-010** (`--cors`) — keep `verified`; widen evidence to
    `cors-flag` + `cors-applied` + `cors-response-surface` +
    `cors-on-redirect`. Note: four headers, not one; the real L3
    preflight surface stays deferred.
  - **SRV-CLI-014** (`--debug`) — `adapted` (no flip); the "startup
    succeeds, HTTP unchanged" Scenario stays. Note: minimal log with
    elapsed-suffix, format implementation-defined.
  - **SRV-CLI-015** (`--no-request-logging`) — `adapted`. The "no
    per-request log line written" Scenario becomes contractual (we
    really do silence the stdout log).
  - **SRV-CLI-016** (`--no-port-switching`) — `accepted` → `adapted`
    per D-016. Compatibility Note: the reference has open issue #751
    (`--no-port-switching` ignored since v14.0.0); irserve implements
    the documented contract. Failure mode verified by
    `crates/irserve-core/src/server.rs::tests` integration test (not
    by probe).
- `docs/reference/serve/decisions.md` — NEW entry **D-016**. Light
  D-002 update to remove ambiguity around SRV-CLI-016 (the behavior is
  scoped; only the wording is not).
- `docs/reference/serve/open-questions.md` — Q-001 → `closed`, pointing
  to the SRV-CLI-003 spec text.
- `docs/stage6_l1_l2_capabilities.md` — flip the 6h row to `done`.
- `README.md` — Stage 6h → `done`. Update the "Try IrServe" section with
  new commands for `tcp://`, `-p`, `--cors`, `--debug`,
  `--no-request-logging`, `--no-port-switching`.
- `docs/features/0014_PLAN_stage6h_cli_fill_in.md` — final plan file
  per the convention (this file in plan-mode is the draft; the polished
  plan in the repo lives at this path).
- `npx -y @fission-ai/openspec@latest validate --all --strict`.

Commit: `docs(stage-6h): spec deltas + D-016 + Q-001 closure + meta`

After the commit — the usual Codex review pass(es).

## Critical files to modify

- `crates/irserve/src/main.rs` (slices 1, 3, 4, 5)
- `crates/irserve/src/listen_spec.rs` (NEW, slice 1)
- `crates/irserve-core/src/server.rs` (slices 3, 4, 5)
- `crates/irserve-core/src/cors.rs` (NEW, slice 3)
- `crates/irserve-core/src/lib.rs` (re-export, slice 3)
- `tools/probe/run.mjs` (slices 2, 3, 4, 5 — gate)
- `tools/probe/cases/cli-tcp-uri.json`, `cli-p-alias.json`,
  `cors-on-redirect.json`, `cli-debug-flag.json`,
  `cli-no-request-logging.json` (NEW, slices 2-5)
- `openspec/changes/010-cli-fill-in/{proposal,design,tasks,specs/cli/spec}.md` (NEW, slice 6)
- `docs/reference/serve/{decisions,open-questions}.md` (slice 6)
- `docs/stage6_l1_l2_capabilities.md`, `README.md` (slice 6)
- `docs/features/0014_PLAN_stage6h_cli_fill_in.md` (slice 6)

## Files to reuse (no modifications)

- `crates/irserve-core/src/custom_headers.rs::apply_custom_headers` — for
  ordering reference; CORS does not use this function but is placed in
  the pipeline immediately after it.
- `crates/irserve-core/src/dispatch.rs::dispatch` — unchanged; CORS
  injection happens in `handler`, not in `dispatch`.
- `tools/probe/cases/cors-flag.json`, `cors-applied.json`,
  `cors-response-surface.json` — not edited; lifted off the gate in
  slice 3 and verified as-is.
- `openspec/changes/009-directory-listing/` — peer style for slice 6.
- `docs/features/0013_PLAN_stage6g_directory_listing.md` — peer style
  for the final plan file.

## Verification

### Per slice (CI sanity)

```powershell
cargo build -p irserve
cargo test --workspace
node tools\probe\run.mjs --all --target=irserve --snapshot=verify
node tools\probe\run.mjs --all --target=reference --snapshot=verify
```

### Manual smoke (after slice 5)

```powershell
# Slice 1: tcp:// URI + Q-001 default port
mkdir _tmp; "hello" | Set-Content _tmp\index.html
cargo run -- --listen tcp://127.0.0.1:3010 _tmp
curl -i http://127.0.0.1:3010/                  # 200
cargo run -- --listen tcp://localhost _tmp
curl -i http://localhost:3000/                  # 200 (Q-001 default port=3000)

# Slice 2: -p alias
cargo run -- -p 3011 _tmp
curl -i http://127.0.0.1:3011/                  # 200

# Slice 3: --cors
cargo run -- --cors --listen 3010 _tmp
curl -i http://127.0.0.1:3010/                  # 4 CORS headers
# CORS on a 3xx:
'{"redirects":[{"source":"/old","destination":"/new"}]}' | Set-Content _tmp\serve.json
cargo run -- --cors --listen 3010 _tmp
curl -i http://127.0.0.1:3010/old               # 301 + 4 CORS headers

# Slice 4: --no-port-switching
# Terminal A:
cargo run -- --listen 3010 _tmp
# Terminal B (default — should retry):
cargo run -- --listen 3010 _tmp
# Expect: stderr "warning: port 3010 in use, using NNNNN"; responses on the new port.
# Terminal B (flag — should exit):
cargo run -- --no-port-switching --listen 3010 _tmp
# Expect: stderr "error: port 3010 ... in use"; non-zero exit.

# Slice 5: --debug + --no-request-logging
cargo run -- --debug --listen 3010 _tmp
curl -s http://127.0.0.1:3010/ > $null
# Expect: stdout "GET / -> 200 (Xms)"
cargo run -- --no-request-logging --listen 3010 _tmp
curl -s http://127.0.0.1:3010/ > $null
# Expect: stdout empty
```

### Oracle harness (after slice 6)

```powershell
node tools\probe\run.mjs --all --target=irserve --snapshot=verify
node tools\probe\run.mjs --all --target=reference --snapshot=verify
npx -y @fission-ai/openspec@latest validate --all --strict
```

Expected delta: +5 new probes (`cli-tcp-uri`, `cli-p-alias`,
`cors-on-redirect`, `cli-debug-flag`, `cli-no-request-logging`); 3
existing probes (`cors-flag`, `cors-applied`, `cors-response-surface`)
now green on `target=irserve`. 0 changes to reference snapshots
(reference behavior did not change).

## Risks

1. **Help-text snapshot regen.** Adding five flags to clap widens the
   `--help` output. `cli-help-version.json` will be re-recorded via
   `--snapshot=update --target=irserve`. D-002 covers the format, but
   verify the case is marked `bodyMayDiffer` or equivalent.
2. **Windows hostname DNS.** `localhost` sometimes resolves to `::1`
   first on dual-stack Windows. In `ListenSpec::resolve`, short-circuit
   the literal string `"localhost"` to `Ipv4Addr::LOCALHOST` — matches
   the bare-port default.
3. **IPv6 bracketed form.** The parser syntax-validates `[::1]:3010`,
   but real bind-and-connect is not probed. If a future stage demands
   it — widen scope.
4. **`-p` clap collision.** `-l` and `-p` are two separate `Vec`s with
   `value_parser`; the merge happens **post-parse** in `main`, not via
   a clap alias (clap cannot put "two distinct flags into one Vec").
5. **CORS + custom-headers ordering.** If a user `headers` rule sets
   `access-control-allow-origin: foo`, our post-CORS pass overwrites
   it with `*`. The reference does the same. Documented via the
   ordering language in SRV-CLI-010.
6. **Multi-listen + `--no-port-switching`.** With `-l 3010 -l 3011`,
   if 3010 is busy and `--no-port-switching` is set — exit on the
   first; the second never reaches bind. Correct per the contract,
   but a Note on SRV-CLI-016: "any listen failure aborts startup".

## Hard stops

- Do not modify `third_party/serve` or `third_party/serve-handler`.
- Reference snapshots are touched only if reference behavior actually
  changed — here, only NEW snapshots for new cases.
- Before slice 6 — explicit user OK on each commit (per memory:
  iterative commits with per-commit approval).
- Codex review rounds commit separately as: `docs(stage-6h): address
  Codex review round N (P{priorities} fixes)`.

## Sources to re-consult

- Reference: `third_party/serve/source/utilities/cli.ts:104-178`,
  `third_party/serve/source/utilities/server.ts:52-86`, `65-70`,
  `166-181`.
- Existing irserve: `crates/irserve/src/main.rs:7-66`,
  `crates/irserve-core/src/server.rs:130-133`,
  `crates/irserve-core/src/custom_headers.rs:178-221`,
  `crates/irserve-core/src/dispatch.rs:31-70`.
- Runner gate: `tools/probe/run.mjs:49-55`, `:172`, `:887-892`.
- Spec: `openspec/specs/cli/spec.md` (current entries for all 6 SRVs).
- Inventory: `docs/reference/serve/inventory.md`
  (SRV-CLI-003/006/010/014/015/016).
- Decisions: `docs/reference/serve/decisions.md` (D-002, D-004 — basis
  for D-016).
- Open questions: `docs/reference/serve/open-questions.md` (Q-001).
- Peer style: `openspec/changes/009-directory-listing/`,
  `docs/features/0013_PLAN_stage6g_directory_listing.md`.
- Upstream: [vercel/serve#751](https://github.com/vercel/serve/issues/751)
  — for the D-016 reference link.
