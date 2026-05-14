# Design: Strict-L0 IrServe runtime

This design is **code-level**. The architectural decisions (crate
layout, HTTP-stack pins, request-lifecycle order, CLI surface, oracle
mapping) live in `openspec/changes/archive/2026-05-15-001-port-minimal-static-server/
design.md` and are taken as given. Read change 001's design first.

## 1. Module map (`crates/irserve-core/src/`)

| Module | Responsibility | Wires phase(s) |
|---|---|---|
| `lib.rs` | Public surface: `pub struct ServerConfig`, `pub enum Error`, `pub async fn run(cfg) -> Result<(), Error>`. Re-exports nothing else. | n/a |
| `server.rs` | Builds `axum::Router::new().fallback(handler).with_state(Arc<PathBuf>)`, binds via `tokio::net::TcpListener::bind`, awaits `axum::serve`. | 1 (axum extractor) |
| `dispatch.rs` | Per-request entry. Phase 2 method check (GET/HEAD; rest → 405). Phases 3–8 are no-op pass-throughs (empty function calls in this slice; design 001 § 4 reserves them for L1+). Phases 9, 11, 12 wire the resolve→serve path. Phase 13 invokes `not_found_response`. | 2, 9, 11, 12, 13 |
| `resolve.rs` | URL path → filesystem path resolver. Joins under root, runs `tokio::fs::metadata`, branches file/dir/notfound, handles directory `index.html`, and runs phase-10 containment via `tokio::fs::canonicalize` + `Path::starts_with`. Returns `ResolveOutcome { File, Index, NotFound, EscapedRoot }`. | 9, 10, 11 |
| `mime.rs` | `pub fn mime_for(&Path) -> Option<&'static str>`. Hand-rolled match on the eight FILE-004 extensions (`.html`, `.js`, `.json`, `.css`, `.txt`, `.svg`, `.wasm`, `.png`); falls through to `mime_guess::from_path(...).first_raw()`; returns `None` for extensionless or `mime_guess`-unknown extensions. | 12 |
| `notfound.rs` | `pub fn not_found_response(&HeaderMap) -> Response<Body>`. If the `Accept` header lists `application/json` (case-insensitive substring match on each comma-separated part), returns 404 + the literal 80-byte JSON envelope + `application/json; charset=utf-8`. Else returns 404 + `<h1>404 Not Found</h1>\n` + `text/html; charset=utf-8`. | 13 |

The bin (`crates/irserve/src/main.rs`) is < 60 lines: clap derive
struct, `resolve_port` (priority `--listen` > `PORT` env > 3000),
`SocketAddr` construction on `127.0.0.1`, `cli.directory.canonicalize()?`,
hand off to `irserve_core::run`.

## 2. Error type

```rust
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
}
```

A single `Io` variant covers `TcpListener::bind` and `axum::serve`
errors. The bin propagates via `Box<dyn std::error::Error>` from
`#[tokio::main]`. Per-request errors (file disappeared between
`metadata` and `read`, `canonicalize` fails) are converted to 404 inside
`dispatch::dispatch` rather than bubbled up; no 5xx surface exists in
strict L0.

This is **less** elaborate than change 001's design § 4 sketch (which
proposed `Error { Bind, Io, Resolve }`). Slice-2 implementation found
that distinguishing those cases adds no observable behavior at L0 — the
bin treats every startup failure the same way. If L1+ adds a
`serve.json` parser or other startup-time work that needs distinct
error reporting, the variant set widens then.

## 3. CLI surface — clap 4 derive resolution

The `version: ()` unit-field pattern with `ArgAction::Version` works on
clap 4.6. The struct shape:

```rust
#[derive(Parser)]
#[command(
    name = "irserve",
    version,
    about = "Strict-L0 Rust port of vercel/serve",
    disable_version_flag = true,
)]
struct Cli {
    #[arg(short = 'v', long = "version", action = ArgAction::Version)]
    version: (),
    #[arg(short = 'l', long = "listen", value_name = "PORT")]
    listen: Option<u16>,
    #[arg(short = 'n', long = "no-clipboard")]
    no_clipboard: bool,
    #[arg(value_name = "DIRECTORY", default_value = ".")]
    directory: PathBuf,
}
```

`disable_version_flag = true` removes clap's auto-added `-V/--version`;
the manually-declared field reintroduces the lowercase `-v` short
required by SRV-CLI-019 and oracle case `cli-help-version.json#version_short`.

## 4. Request lifecycle — phase wiring in this slice

Per change 001 § 4 the dispatcher mirrors SRV-ROUT-006's six pipeline
stages plus filesystem and response phases. In this change:

- **Phases 1, 2** wired (axum extractor, method check).
- **Phases 3–8** are no-ops; the pipeline is a straight pass-through to
  phase 9. Each phase is implemented as a separate function only when
  L1+ wires it; in this slice they don't appear in `dispatch.rs`.
- **Phases 9, 10, 11** are wired in `resolve.rs` (URL→FS, containment,
  directory→index.html).
- **Phase 12** wired in `dispatch.rs::file_response` + `mime::mime_for`.
- **Phase 13** wired via `notfound::not_found_response`.

Phase 10 (containment) implementation:

```text
1. Compute candidate = root.join(url_path.trim_start_matches('/')).
2. metadata(candidate)? — if Err, return NotFound.
3. If is_file: resolved_path = candidate, kind = File.
4. If is_dir: index = candidate.join("index.html");
   metadata(index) is_file → resolved_path = index, kind = Index;
   else NotFound.
5. canonicalize(resolved_path)? — if Err, return NotFound.
6. canonical.starts_with(root) — false → EscapedRoot.
7. Return File or Index based on kind.
```

`root` is canonicalized once in the bin via `cli.directory.canonicalize()?`
and passed to `irserve_core::run`. Both root and the candidate's
`canonical` are UNC-prefixed on Windows (`\\?\C:\...`), so the
`Path::starts_with` component comparison works across platforms.

## 5. Probe runner adapter shape (`tools/probe/run.mjs`)

Per change 001 § 6, the runner gains a `--target=<reference|irserve>`
flag and matching `PROBE_TARGET` env. Adapter logic added in this
change:

- `spawnServe({port, target, skipAutoListen, extraEnv})` —
  branches on target. `reference`: existing `node SERVE_ENTRY` plus
  `--listen <port>`, `--no-clipboard`, `--no-port-switching` injection
  unless `skipAutoListen`. `irserve`: spawns `IRSERVE_BIN` directly,
  injects only `--listen <port>` and `--no-clipboard` (never
  `--no-port-switching`).
- D-008 deferred-flag refusal — if a case's `serveArgs` carries a
  deferred flag (`-p`, `-c`, `-C`, `-d`, `-L`, `-s`, `--no-port-switching`,
  or a `tcp://` URI under `--listen`), the runner aborts with a clear
  error rather than silently stripping. `--no-clipboard` is exempt
  per D-005.
- Per-case `runner.skipAutoListen: true` plus
  `runner.defaultPortScenario: "no-flag-no-env" | "env-var"` — the
  latter allocates a free port `P`, exports `PORT=P` into the spawned
  process, and probes `P`.
- Per-case `runner.l0` block:
  - `clean: [...]` — anchor names that must match (subject to volatile
    overlay below).
  - `divergent: [...]` — anchor names recorded as informational; their
    request still executes against irserve, but the result is stripped
    from BOTH sides of the diff.
  - `bodyMayDiffer: [...]` — strips `body.length`, `body.sha256`,
    `body.preview`, AND `content-length` for listed anchors.
  - `contentLengthMayDiffer: [...]` — strips `content-length` only
    (body bytes still must-match).
  - `exitCodeMayDiffer: [...]` — for CLI-mode anchors: asserts both
    sides have a non-zero exit code, then strips `exitCode`. Refusing
    to strip a zero exit code preserves the spec's "non-zero is
    contractual" guarantee.
- Cases without a `runner.l0` block are **skipped** under
  `target=irserve` (logged, not failed). This is the explicit opt-in
  contract that limits what the L0 mode runs.
- Volatile-headers overlay — when `target=irserve`, the case's
  `volatileHeaders` is augmented with `['etag', 'vary', 'accept-ranges']`
  before the masking pass runs. These headers `serve` emits and IrServe
  omits at L0 by deliberate design (no ETag, no compression, no Range
  support).

`target=reference` paths are unchanged; existing snapshots verify
byte-for-byte.

## 6. Findings — divergences mediated by the L0 mask

(Summarized in `proposal.md` § Findings; this section gives the
mediation rationale.)

### 6.1 `default-port-l0` — index.html resolution under `cleanUrls: false`

`serve-handler/src/index.js:620` skips `findRelated()` when
`cleanUrls: false`. The same call also resolves `<dir>/index.html` at
phase 9, so disabling cleanUrls disables index.html resolution at `/`.
Reference falls through to a directory-listing render. IrServe (which
has no cleanUrls) always resolves `/` → `index.html`.

The original Slice-4 fixture shipped `serve.json: {cleanUrls: false}`
to avoid 301 redirects on `/page.html` (which the case does not
probe). The fixture was simplified to drop `serve.json` entirely; the
case probes only `GET /`, so default cleanUrls applies and both
runtimes return `index.html`. The fix is in the case file, not in
IrServe.

This is a serve-handler implementation detail (cleanUrls and index.html
resolution share a code path); it is not part of the IrServe contract.
A note is added to `open-questions.md` only if a future probe needs
`cleanUrls: false` AND an index.html — `default-port-l0` itself does
not.

### 6.2 `notfound-shape#missing_html_with_accept` — Content-Length on JSON 404

`vercel/serve` writes the JSON 404 body via Express `res.send()`, which
defaults to chunked transfer-encoding for dynamically-sized bodies and
omits `Content-Length`. axum's `Body::from(&'static str)` knows the
length up front and emits `Content-Length: 80`.

Per design 001 § 6 ORC-005 must-match: status, content-type, JSON
envelope body bytes. Content-Length is not part of must-match — both
chunked and content-length transports deliver the same 80 bytes. The
runner's L0 overlay grows a `contentLengthMayDiffer` field that strips
`content-length` for listed anchors only (HTML body bytes for that
anchor still must-match — no body-mask is applied).

### 6.3 `cli-positional-error#two_positionals` — exit code 1 vs 2

`vercel/serve` rejects two positionals with exit code 1 (caught by
its own validation). `clap` defaults to exit code 2 for any
argument-parse error. SRV-CLI-007 sc.3 spec text says "non-zero exit
code"; design 001 § 6 ORC-062 must-match agrees ("non-zero exit
code"). The committed snapshot encodes `exitCode: 1` literally; that's
historical reference baseline, not contract.

The runner's L0 overlay grows an `exitCodeMayDiffer` field that asserts
both sides have a non-zero `exitCode`, then deletes the field from
both sides before diff. If irserve ever exits 0 on this anchor, the
filter throws (refuses to strip a zero) — non-zero remains
contractual.

`oracle-matrix.md` ORC-062 must-match is relaxed from `exit=1` to
`exit=non-zero` in the same commit (research-track edit; not an
OpenSpec delta).

## 7. Spec delta

One MODIFIED in `specs/cli/spec.md` — promotes SRV-CLI-001 from
`accepted` to `verified`, adds `Implementation:` line pointing at this
change, and updates the `Note:` to reflect the new probe coverage.
The Requirement body and scenarios are otherwise reproduced verbatim.

The MODIFIED is **stronger than change 001's MODIFIED on the same
SRV** (which only added an `Implementation:` line; status stayed
`accepted`). The two changes both modify SRV-CLI-001:

- Change 001 adds the Implementation line; status stays `accepted`
  because no probe yet existed.
- Change 002 (this) supersedes 001's MODIFIED — it carries the same
  Implementation line plus the status promotion to `verified` and the
  Note update.

OpenSpec's validator runs each change against the canonical pre-state.
If the validator rejects two in-flight changes touching the same
Requirement, this change's MODIFIED is the authoritative post-state
and change 001 should be archived first (a separate operation; user
decides per the Stage 5a → 5b acceptance flow). Until then, both
deltas are valid against the canonical pre-state and produce
self-consistent post-states; `validate --strict` should accept both.

## 8. Integration test layout

`crates/irserve/tests/oracle.rs` is the single integration test. Cargo
auto-builds the bin before running it (the test exists under the
`irserve` crate's `tests/` dir, so `CARGO_BIN_EXE_irserve` is set).
The test:

1. Resolves the workspace root from `CARGO_MANIFEST_DIR` (two parents up).
2. Asserts `tools/probe/run.mjs` exists.
3. Spawns `node tools/probe/run.mjs --all --target=irserve --snapshot=verify`
   with `IRSERVE_BIN` env pointing at the just-built binary.
4. Asserts the harness exited with success.

Failures surface as the harness's own diff output; a green test means
all 7 L0-clean cases passed verbatim and 27 L1+ cases were correctly
skipped.

The test depends on `node` being on PATH. AGENTS.md treats Node as a
project-level prereq (already required for the reference oracle). No
graceful skip is implemented; failure to spawn `node` panics with a
clear message.

## 9. Open assumptions

- **Q-001..Q-004** — unchanged. None affects strict-L0 SRVs.
- **A1 — `irserve` binds `127.0.0.1` by default.** Documented in
  `proposal.md`. Not a Q-NNN: no SRV constrains the interface.
- **A2 — serve's `cleanUrls: false` disables index.html resolution at
  `/`.** Documented in `proposal.md`. Not a Q-NNN: serve-handler
  implementation detail, not contractual for IrServe.

No Q-NNN entry is closed by this change.
