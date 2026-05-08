# Stage 5b — Strict-L0 Rust runtime

## Context

Stage 5a (`openspec/changes/001-port-minimal-static-server/`) fixed the
architecture for the first Rust port: workspace `irserve` (bin) +
`irserve-core` (lib), axum 0.8 with a custom dispatcher (no `ServeDir`),
13-phase request lifecycle with phases 1/2/9–13 wired and 3–8 as
no-op pass-throughs. The contract for this slice is exactly **eight**
SRVs (`SRV-CLI-001/002/007/019`, `SRV-FILE-001/002/004/005`); every
other capability is deferred by `D-008`. The methodological hypothesis
is "strict L0 passes oracle without contract edits"; Stage 5b is the
first chance to falsify it.

This plan implements the Stage-5b deliverables: workspace + binary,
adapter for `tools/probe/run.mjs`, two new L0-mode probe cases, and a
Stage-5b-flavored OpenSpec change (`002-implement-strict-l0-runtime`).

## Source-of-truth pointers (read before editing)

- `openspec/changes/001-port-minimal-static-server/design.md` — every
  architectural decision (§§1–8) is authoritative. Do **not** redesign;
  cite §§ when justifying code.
- `openspec/specs/cli/spec.md` — SRV-CLI-001/002/007/019 verbatim.
- `openspec/specs/static-files/spec.md` — SRV-FILE-001/002/004/005
  verbatim.
- `docs/reference/serve/decisions.md` — D-002/003/005/006/008 frame
  what is NOT in scope (terminal output, HTML markup, clipboard,
  compression, deferred SRVs).
- `tools/probe/run.mjs` — runner whose entry points 5b adapts (function
  map below in §5).

## Working pins (§2 of design.md, re-verify with `cargo add` at slice 1)

- `axum 0.8` (`Router::new().fallback(handler)`,
  `axum::serve(TcpListener, app)`, `Response::builder()...body(Body::from(...))`)
- `tokio 1` features `["rt-multi-thread", "net", "fs", "macros", "signal"]`
  in `irserve-core`; `["macros", "rt-multi-thread"]` in `irserve` bin
- `tower-http 0.6` — declared but not used in 5b (no middleware needed
  for L0); pin for forward-compat or omit until first use. **Decision:**
  omit until L1 to keep 5b deps minimal; revisit at next change.
- `clap 4` (`features=["derive"]`) with explicit `short = 'v'`
  override on `version` (clap default is `-V`)
- `mime_guess 2` (long-tail fallback)
- `bytes 1` (response body construction)

`tracing`/`tracing-subscriber`, `flate2`, `serde` are **not** added at
5b (D-002/006 + JSON 404 envelope is a fixed-shape literal).

## Workspace and source layout

```
irServe/                       (existing repo root)
├── Cargo.toml                 (NEW: workspace manifest)
├── crates/
│   ├── irserve/               (NEW: bin)
│   │   ├── Cargo.toml
│   │   └── src/main.rs        (clap parse, env PORT lookup, hand off to irserve_core::run)
│   └── irserve-core/          (NEW: lib)
│       ├── Cargo.toml
│       └── src/
│           ├── lib.rs         (re-exports run, ServerConfig)
│           ├── server.rs      (axum Router build, bind, serve)
│           ├── dispatch.rs    (13-phase pipeline; phases 1/2/9–13 wired)
│           ├── resolve.rs     (phases 9–11: URL→FS, containment, index.html)
│           ├── mime.rs        (FILE-004 hand-rolled table + mime_guess fallback)
│           └── notfound.rs    (phase 13: HTML / JSON envelope by Accept)
└── tests/
    └── oracle/
        └── probe.rs           (NEW: cargo-test harness invoking run.mjs)
```

`Cargo.lock` is committed (binary crate). No `tracing` config files. No
`build.rs`. No examples. The bin's `main.rs` stays under ~80 lines per
design §1.

## Iterative slice plan (4 commits, each green-state)

### Slice 1: Workspace + clap CLI (compiles, exits cleanly on `--help`/`--version`)

- Workspace `Cargo.toml` listing both members.
- `crates/irserve/Cargo.toml` with `clap 4` + `tokio 1` + path-dep on
  `irserve-core` + `version = "0.0.1"`.
- `crates/irserve/src/main.rs` declaring `#[tokio::main]`, `clap`
  derive struct with: positional `[DIRECTORY]` (default `.`),
  `-l/--listen <PORT>` numeric, `-h/--help`, `-v/--version`
  (`#[command(version = env!("CARGO_PKG_VERSION"))]` + struct field
  `#[arg(short = 'v', long = "version", action = ArgAction::Version)]`
  if needed to override clap's `-V` default), `-n/--no-clipboard`
  boolean (silently ignored per D-005). `--no-port-switching` and other
  D-008 flags are NOT declared → clap rejects them with non-zero exit.
- `crates/irserve-core/Cargo.toml` (axum, tokio, mime_guess, bytes,
  thiserror) + skeleton `lib.rs` exporting `run(ServerConfig)` that
  panics with `unimplemented!()`.
- Verification: `cargo build`; `cargo run -- --help` exit 0;
  `cargo run -- -v` prints version, exit 0; `cargo run -- a b` non-zero
  exit (positional clap rejection); `cargo run -- --no-port-switching`
  non-zero exit. **No HTTP yet.**

### Slice 2: Server bind + FILE-001/005 (root + simple file)

- `irserve_core::run` resolves the served root via `Path::canonicalize`,
  builds `Router::new().fallback(handler)`, binds via
  `tokio::net::TcpListener::bind(SocketAddr)`, awaits `axum::serve`.
- Bin reads `--listen` first; if absent, reads `std::env::var("PORT")`;
  default 3000 (per SRV-CLI-001 spec text and design §5). Constructs
  `SocketAddr` on `127.0.0.1` (loopback default; D-002 keeps interface
  choice implementation-defined — match `serve`'s loopback-on-localhost
  behavior, which is also what every probe assumes via its
  `http://127.0.0.1:<port>` requests).
- `dispatch::handler(req, root)` implements phases 1, 2, 9, 11, 12 only:
  GET/HEAD → resolve URL path under root (phase 9 stub: lexical
  `path.join`, no traversal containment yet), if directory append
  `index.html` (FILE-005), if file → 200 + body via `tokio::fs::read`,
  else 404 stub.
- Verification: smoke `_smoke.json#root` anchor by hand: spawn
  irserve against fixture, `curl /` returns 200 + `hello\n`.

### Slice 3: MIME table + 404 envelope + phase 10 containment

- `mime::mime_for(path: &Path) -> Option<&'static str>` with the eight
  FILE-004 bindings hand-rolled (`.html`, `.js`, `.json`, `.css`,
  `.txt`, `.svg`, `.wasm`, `.png`); fallback to
  `mime_guess::from_path(...).first_raw()` for the long tail; `None`
  for extensionless or unknown (per FILE-004's `no Content-Type` rule).
  Only set `Content-Type` header if `Some(_)`.
- `notfound::not_found_response(accept_header)`:
  - If `accept_header` lists `application/json` (case-insensitive,
    parse comma-separated, ignore `q=` weights for L0 — match
    `serve-handler`'s observable behavior: any presence of
    `application/json` in `Accept` triggers JSON), return 404 with
    `Content-Type: application/json; charset=utf-8` and the literal
    body `{"error":{"code":"not_found","message":"The requested path
    could not be found"}}`.
  - Else 404 with `Content-Type: text/html; charset=utf-8` and an
    implementation-defined HTML body (D-003). Suggested body: a single
    `<h1>404 Not Found</h1>` line; oracle does not compare HTML
    content.
- Phase 10 (containment): canonicalize the joined `root.join(rel)` via
  `tokio::fs::canonicalize` (or `std::fs::canonicalize` inside a blocking
  task); compare its prefix against the canonicalized root prefix; on
  escape (or canonicalize fails because the path doesn't exist) treat
  as 404. **Windows note:** `canonicalize` returns UNC-prefixed paths
  (`\\?\C:\...`) on Windows; canonicalize the root **once** at startup
  and compare both as `Path` (not strings). Escape → 404 is L0
  hygiene; the contractual 400 from `SRV-SEC-001` is L2 (deferred by
  scope, not by D-008).
- Verification: against fixture, `_smoke#root` (200, `text/html;
  charset=utf-8`), `mime-defaults#js/json/css/txt/wasm/svg/png/noext/unknownext`
  (statuses + content-types match must-match layer of ORC-003),
  `notfound-shape#missing_html` (404 HTML), `#missing_html_with_accept`
  (404 JSON with exact envelope).

### Slice 4: Probe runner adapter + tests/oracle/probe.rs + new L0-mode cases

- `tools/probe/run.mjs` adapter (full spec §5 below).
- `tools/probe/cases/default-port-l0.json` (env-var scenario; spec §6
  below).
- *Optional but in-scope:* `tools/probe/cases/mime-defaults-l0.json`
  (replaces L1-divergent `mime-defaults#html` anchor with one bypassing
  cleanUrls). Decision: include if §6's env-var scenario lands cleanly
  and time permits within the slice; skip if not, leaving the anchor
  as L1-divergent for now (already partitioned in design §6).
- `tests/oracle/probe.rs` Rust integration test that spawns
  `node tools/probe/run.mjs --all --target=irserve` (or env
  `PROBE_TARGET=irserve`) via `std::process::Command`; passes if exit
  code is 0. Tests are gated behind `cfg(not(target_os = "ios"))` —
  not gated otherwise; CI assumed to provide Node + the compiled
  reference bundle (already a project-level prerequisite, see
  README.md "Getting started").
- OpenSpec change `002-implement-strict-l0-runtime/` with proposal/
  design/tasks (spec §7).
- README stage-map row 5b → done; README mentions Rust toolchain prereq
  (`rustup` 1.81+, `cargo`).
- Verification: `cargo test --test probe` green;
  `cargo test --test probe -- --nocapture` shows no anchor mismatches
  in `tools/probe/output/*.md`.

## Probe runner adapter (`tools/probe/run.mjs`)

The runner currently hardcodes the reference binary in
`spawnServe` (lines 111-131): `node third_party/serve/build/main.js`
plus injected `--listen <port>`, `--no-clipboard`,
`--no-port-switching`. Snapshot comparison at line 533 is canonical-JSON
deep-equal with per-case `volatileHeaders` masking (default
`['last-modified']`).

Adapter changes (corresponds to `tasks.md` §6.2 of change 001):

### A. Pluggable target binary

- New CLI flag `--target=<reference|irserve>` and env `PROBE_TARGET`;
  CLI wins when both set; default `reference`.
- New const `IRSERVE_BIN` resolved as: env `IRSERVE_BIN` if set,
  else `target/debug/irserve` (or `.exe` on Windows) under the workspace
  root. Builds invoked from `tests/oracle/probe.rs` will have already
  produced this binary; standalone runs require the user to
  `cargo build` first.
- `spawnServe({port, fixtureDir, extraArgs, target})` branches on
  `target`: reference path keeps current behavior verbatim; `irserve`
  path uses `IRSERVE_BIN`.

### B. Strip D-008-deferred flags when `target=irserve`

When `target=irserve`, remove from the spawn args any of:
`--no-port-switching`, `-p`, `-c`, `--config`, `-C`, `--cors`, `-d`,
`--debug`, `-L`, `--no-request-logging`, `-s`, `--single`. Also strip
`tcp://` URI forms in `--listen` (not currently injected by the runner;
guard against per-case `serveArgs` sneaking them in: if a case's
`serveArgs` contains a deferred flag and `target=irserve`, **abort with
a clear error** rather than silently strip — that case is L1+ and not
applicable to strict-L0).

`--no-clipboard` is **kept** (D-005 invariant; irserve accepts it).
`--listen <port>` is kept (or stripped per §C below).

### C. Per-case opt-out for auto-`--listen`

- New per-case field `runner.skipAutoListen: true`. When set:
  - The runner does NOT inject `--listen <port>`.
  - The case must declare ONE of two scenario shapes via
    `runner.defaultPortScenario: "no-flag-no-env" | "env-var"`:
    - `no-flag-no-env`: runner verifies port 3000 is free via
      `getFreePort`-style probe; aborts with clear error if occupied;
      HTTP probe requests target port 3000.
    - `env-var`: runner allocates a free port `P` via existing
      `getFreePort`, sets `PORT=P` in spawn env, HTTP probes target `P`.
- `default-port-l0.json` (§6) uses `env-var` to avoid 3000-occupancy
  flakes on developer machines.

### D. L0-mode anchor filter (hybrid: runner-level volatile + per-case partition)

- When `target=irserve`, the runner automatically extends
  `volatileHeaders` (currently `['last-modified']` default) with
  `['etag', 'vary', 'accept-ranges']`. These are headers `serve` emits
  that strict-L0 irserve does not, and the must-match layer of every
  L0-clean ORC explicitly excludes them (design §6 "May-differ" rows).
- Optional per-case `runner.l0` block:
  ```
  "l0": {
    "clean": ["root", "noext", "unknownext", ...],
    "divergent": ["index_html_redirect", "html"]
  }
  ```
  When `target=irserve` AND the block exists:
  - Comparison runs only against the `clean` anchors (via canonical-JSON
    per-anchor comparison; `divergent` anchors are stripped from both
    expected and actual snapshots before diff).
  - `divergent` anchors are still executed (irserve handles them) and
    their actual responses recorded in the markdown report under an
    `Informational (L1-divergent)` heading; pass/fail does NOT depend
    on them.
- For L0-clean cases without an explicit `l0` block (e.g. CLI-mode
  `cli-help-version.json` and `cli-positional-error.json`), the runner
  defaults to "all anchors are clean" and applies the volatile-header
  set as usual. CLI streams remain unconditionally masked (run.mjs
  already does this — design §6 must-match for ORC-060/061/062 only
  asserts exit codes).

### E. Per-case L0 partition map (initial population)

Populate `runner.l0` blocks per design §6 table:
- `_smoke.json`: clean `[root]`, divergent `[index_html_redirect]`
- `mime-defaults.json`: clean `[js, json, css, txt, wasm, svg, png,
  noext, unknownext]`, divergent `[html]`
- `notfound-shape.json`: clean `[missing_html, missing_html_with_accept]`,
  divergent `[]`

Cases not in the L0-clean partition (every other case) are skipped
when `target=irserve` via `runner.l0Skip: true` (or absence of the
`l0` block triggers skip — final shape: explicit `l0` opt-in per case;
absence means the case isn't run for `target=irserve`). This avoids
attempting cleanUrls/redirects/etc. probes against an irserve that
doesn't implement them.

## New probe case: `default-port-l0.json`

Closes SRV-CLI-001's "no probe" gap (Risk #3 of proposal 001).

```jsonc
{
  "id": "default-port-l0",
  "description": "SRV-CLI-001: server uses PORT env var when --listen omitted",
  "fixture": {
    "files": { "index.html": "default-port\n" },
    "serveJson": { "cleanUrls": false }
  },
  "runner": {
    "skipAutoListen": true,
    "defaultPortScenario": "env-var",
    "l0": { "clean": ["root"], "divergent": [] }
  },
  "requests": [
    { "name": "root", "path": "/" }
  ]
}
```

When run against `target=reference`: snapshot is recorded as the
baseline (200, `text/html; charset=utf-8`, body `default-port\n`).
When run against `target=irserve`: must match the L0-clean baseline.
Promotes SRV-CLI-001 from `accepted` → `verified` after Stage 5b lands.

`mime-defaults-l0.json` (optional, design §6 / tasks 6.5): same shape,
`serveJson: {"cleanUrls": false}`, includes the `html` anchor missing
from the L0-clean partition of the original case. Land if and only if
the env-var scenario in `default-port-l0.json` works without runner
churn.

## OpenSpec change `002-implement-strict-l0-runtime/`

Per the forward reference in `001-…/proposal.md` ("change
`002-implement-strict-l0-runtime` (Stage 5b)"). Validated with
`npx -y @fission-ai/openspec@latest validate --all --strict`.

### `proposal.md`

- **Why** — Stage-5a fixed the architecture; Stage-5b implements it.
  Methodology hypothesis: strict-L0 implementation passes oracle
  must-match without contract edits.
- **What** — workspace + bin + lib + adapter + tests + new probe case;
  reuses every architectural decision from change 001 unchanged. Lists
  the 8 SRVs being implemented.
- **Scope** — in: items above. Out: any L1+ behavior; any change to
  third_party; any change to existing snapshots (only ADD
  `default-port-l0.json` and optionally `mime-defaults-l0.json`).
- **Risks** —
  1. Windows UNC canonicalize divergence (§Slice 3). Mitigation:
     canonicalize root once, compare `Path` not strings.
  2. axum 0.8 fallback handler signature drift. Mitigation: pin to
     0.8.x at slice 1 with `cargo add` and re-verify against
     Context7-fetched docs (already done at planning time).
  3. `mime_guess` long-tail returns include charset for some text types
     and not others; if a long-tail extension matters, FILE-004's
     hand-rolled table wins. Mitigation: hand-rolled table is checked
     first.
  4. `default-port-l0` env-var scenario assumes irserve's bin reads
     `PORT` correctly. Verified by Slice 4 itself; if it fails Slice 4
     reverts to `no-flag-no-env`.

### `design.md`

Code-level (not architectural — that lives in change 001):
- Module map under `crates/irserve-core/src/` with per-module
  responsibility one-liners.
- Error type: `pub enum Error { Bind(io::Error), Io(io::Error),
  Resolve(ResolveError) }`. axum handlers return `Result<Response,
  Infallible>` with errors mapped to 5xx in a single helper; for L0,
  `tokio::fs::read` errors on a file that resolved-but-disappeared
  → 404 (not 5xx; race window with phase 9 result).
- Integration test layout under `tests/oracle/probe.rs`: single
  `#[test] fn oracle_l0_must_match()` that builds the bin via
  `cargo build --bin irserve` (the test runner does this implicitly
  via cargo) and shells out to the runner.
- `default-port-l0` env-var scenario mechanics in detail (matches §6
  above).
- Open assumptions list — Q-001..Q-004 stay open per design 001 §7.
  Add new assumption: "irserve binds `127.0.0.1` by default" (matches
  every probe's `http://127.0.0.1:<port>` URL; no SRV constrains the
  default interface; documented as implementation-defined under D-002).
- Spec delta plan: **none**. The contract is unchanged. Validator
  requirement of ≥1 delta — see §"Spec deltas in 002" below.

### Spec deltas in 002

The validator requires ≥1 delta. Two viable options:

1. **Promote SRV-CLI-001 `accepted` → `verified`** in `cli/spec.md`
   via a MODIFIED that updates the `Evidence:` line from "no probe"
   to "ORC-NEW (default-port-l0)" and updates the `Note:` to remove
   the "Stage-5b probe TODO" sentence. Add a new ORC row in
   `oracle-matrix.md` (e.g. ORC-063) for the new probe — but
   `oracle-matrix.md` is not under `openspec/`, so its update is a
   research-track edit alongside, not an OpenSpec delta.
2. **Traceability MODIFIED** on the eight strict-L0 SRVs adding an
   `Implementation:` line pointing at `002-…` and the relevant
   crate/module path.

**Recommendation:** option 1 (single MODIFIED on SRV-CLI-001) — it
captures real new evidence (status promotion) and avoids 8 trivial
metadata-only deltas. Cite `decisions.md` D-008 unchanged. If
`default-port-l0` doesn't land in Slice 4 (env-var scenario aborts),
fall back to option 2 with two MODIFIED entries (SRV-CLI-001 +
SRV-FILE-001) to keep the delta minimal.

### `tasks.md`

Mirrors the 4-slice plan with check items per file touched. Closing
this change is gated on:
- All 4 slices committed and green.
- `cargo build` clean on `target_os` Windows (primary developer
  platform per AGENTS.md).
- `cargo test --test probe` green with both `target=reference` (sanity:
  existing snapshots still match) and `target=irserve` (L0 must-match).
- `npx ... openspec validate --all --strict` exit 0.
- README stage map row 5b → done.

## Decisions log addendum (live document, per AGENTS § "decisions.md")

If Slice 2 or 3 forces a runtime adaptation not anticipated by 001's
design (e.g. axum 0.8's `IntoResponse` doesn't permit raw `Body`
construction the way the design assumes; a `mime_guess` quirk forces
extending the hand-rolled table), record as `D-009+` in
`docs/reference/serve/decisions.md` BEFORE the slice commits, with a
one-line user check-in. Do NOT silently adapt the contract.

## Open assumptions (Q-NNN remain open)

Carried over from 001 design §7 unchanged: Q-001/002/003/004 stay
`open`. Q-005..Q-008 closed (no impact). Q-009/010/011 unaffected
(L3+/L4).

New 5b-specific assumption (NOT a Q-NNN closure):
- **A1 — Default interface is loopback.** `irserve` binds `127.0.0.1`
  when `--listen <port>` provides no host and `PORT` env supplies only
  a port. No SRV mandates a specific interface; `serve` defaults to a
  string-formatted `localhost` URL in its banner; loopback matches
  every existing probe's request URL. Implementation-defined under
  D-002. If a probe ever needs binding to `0.0.0.0`, that's an L4
  decision.

## Verification

End-to-end check before declaring 5b done:

1. `cargo build --release` clean on Windows.
2. `cargo test --test probe` green.
3. Manual smoke: `cargo run --release -- _tmp` (where `_tmp` contains
   `index.html` with `hello`); `curl -i http://127.0.0.1:3000/`
   returns 200 + `text/html; charset=utf-8` + `hello\n`.
4. Manual smoke MIME: `_tmp/data.json` → `application/json;
   charset=utf-8`; `_tmp/icon.svg` → `image/svg+xml`; `_tmp/noext`
   → no Content-Type header.
5. Manual smoke 404: `curl -i http://127.0.0.1:3000/missing` →
   404 + `text/html`; `curl -i -H 'Accept: application/json'
   http://127.0.0.1:3000/missing` → 404 + JSON envelope verbatim.
6. Manual smoke containment: `curl -i 'http://127.0.0.1:3000/../etc/passwd'`
   → 404 (after path normalization by curl this becomes
   `/etc/passwd` which is outside root → containment 404).
7. Manual smoke port: `PORT=4567 cargo run` → server on 4567;
   `cargo run -- --no-port-switching` → non-zero exit (clap reject).
8. `npx -y @fission-ai/openspec@latest validate --all --strict` exit 0.
9. `git diff --stat` shows zero lines under `third_party/`; only
   additions under `tools/probe/cases/` and `tools/probe/snapshots/`
   for the new L0-mode cases (no edits to existing snapshots).

## Subagent delegation plan

Per interview answer "Run.mjs + workspace scaffold":

- **Sub-agent A — workspace scaffold (Slice 1):** general-purpose
  agent with explicit file paths, version pins, and exact CLI surface
  (clap struct field-by-field). Returns the four Cargo.toml files +
  `main.rs` + `lib.rs` skeletons. Main agent reviews against design.md
  §§1, 2, 5 before commit.
- **Sub-agent B — run.mjs surgery (Slice 4):** general-purpose agent
  with §5 above as spec. Returns the modified `run.mjs` + the new
  `default-port-l0.json` (and optionally `mime-defaults-l0.json`).
  Main agent reviews against §5 line-by-line.
- **Slices 2 & 3 (irserve-core, dispatcher, MIME, 404, containment)
  — main agent only.** These are the architectural decisions the
  experiment is testing; delegating them defeats the point.
- **OpenSpec change 002 — main agent.** Authored after Slice 4 lands
  green so the proposal can cite real evidence (probe IDs, file paths,
  test names).

## What this plan does NOT do

- Does not archive change `001-port-minimal-static-server`. User
  decides separately after 5b is accepted. The plan's references to
  `openspec/changes/001-…/specs/cli/spec.md` keep working until then.
- Does not close any Q-NNN.
- Does not edit existing snapshots.
- Does not add `tracing`, `serde`, or `tower-http` dependencies.
- Does not add Linux/macOS-specific paths or behaviors not exercised
  by the primary Windows developer environment (cross-platform
  compatibility is a stated AGENTS rule but L0 doesn't push beyond
  POSIX-on-NTFS basics).
