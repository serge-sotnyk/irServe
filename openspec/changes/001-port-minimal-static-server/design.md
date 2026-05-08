# Design: Minimal static server (strict L0)

This document fixes the architectural decisions a Stage-5b implementer
needs to start writing Rust against the eight strict-L0 SRVs without
re-deriving the choices below. It is **prose plus signatures**; no Rust
code blocks longer than function signatures appear, by Stage-5a rule.

Decisions cite SRV/ORC/D-NNN/Q-NNN evidence from the archived
`openspec/specs/`, `docs/reference/serve/inventory.md`,
`docs/reference/serve/oracle-matrix.md`,
`docs/reference/serve/decisions.md`, and
`docs/reference/serve/open-questions.md`.

## 1. Crate layout

A two-crate Cargo workspace, with the bin held thin and the lib carrying
all observable-behavior logic:

```
irserve/                       (workspace root; Cargo.toml lives here)
├── crates/
│   ├── irserve/               (bin: thin CLI entry; arg-parse, init, hand off)
│   │   ├── Cargo.toml
│   │   └── src/main.rs
│   └── irserve-core/          (lib: Server, dispatcher, mime, types)
│       ├── Cargo.toml
│       └── src/lib.rs
├── tests/oracle/              (Stage 5b only; integration tests vs tools/probe/)
└── Cargo.toml                 (workspace manifest)
```

Justification:

- `irserve-core` hosts every observable-behavior pathway. Stage 5b's
  `tests/oracle/` and any future embedded mode (e.g. WASM, an in-process
  test server, or a future Rust-level handler API outside the rejected
  D-001 surface) link against `irserve-core` without going through clap.
- The split is two crates rather than `lib.rs + bin/` because the
  workspace boundary forces a clean dependency graph: clap stays in
  `irserve` only; `irserve-core` cannot accidentally depend on the CLI
  parser. This rules out a class of leakage early.
- The crate boundary aligns with future `02-…` and `03-…` change
  proposals: capabilities (config, routing, redirects, rewrites,
  directory-listing) all land inside `irserve-core` modules. The bin
  shell stays untouched as L1+ behavior accretes.

The workspace manifest lives at the repository root only when Stage 5b
materializes. Stage 5a does not write any `Cargo.toml`.

## 2. HTTP stack and direct dependencies

All version pins below are working pins for Stage 5b's `Cargo.toml`. Each
pin reflects a `cargo add`-time current-stable as of 2026-05; Stage 5b
re-pins to the then-current stable when authoring Cargo manifests.

### `irserve-core` direct dependencies

- `axum` 0.8.x — HTTP framework. Provides `Router`, extractors,
  `IntoResponse`, and a `fallback` handler for the unmatched-route 404.
  Built on `hyper` and `tokio`; idiomatic for static handler-based
  servers.
- `tokio` 1.x with features `["rt-multi-thread", "net", "fs", "macros",
  "signal"]` — async runtime. `signal` enables graceful shutdown on
  Ctrl-C (a polish item, not contractual).
- `tower-http` 0.6.x — included only for utility middleware
  (`RequestBodyLimitLayer`, `SetResponseHeaderLayer`) **as needed**;
  **not** used for `ServeDir`. Rationale: see § 3.
- `bytes` 1.x — body construction without copying.
- `mime_guess` 2.x — MIME long-tail fallback **only**. The eight bindings
  fixed by FILE-004 are produced from a hand-rolled table inside
  `irserve-core` because `mime_guess` does not append `; charset=utf-8`
  to text types by default.
- `http` 1.x — re-exported by axum; not declared directly.

Hyper itself is **not** a direct dependency: axum re-exports the parts
the dispatcher needs (`http::Request`, `http::Response`, `axum::body::Body`).

### `irserve` (bin) direct dependencies

- `clap` 4.x with feature `derive` — argument parsing. Confirmed via the
  user's Stage-5a interview. Supports the four L0 flags (positional dir,
  `-l/--listen <PORT>`, `-h/--help`, `-V/--version`) with strict
  rejection of unknown flags.
- `irserve-core` — workspace member, default features.
- `tokio` 1.x with features `["macros", "rt-multi-thread"]` — for
  `#[tokio::main]`.

### Deferred dependencies

- `tracing` / `tracing-subscriber` — log format is implementation-defined
  per D-002. Stage 5a does not pin them. Stage 5b MAY add a single
  `eprintln!` for the bound URL on startup; structured logging arrives
  at L1 if needed.
- `flate2` / compression — explicitly deferred per D-006.
- `serde` / `serde_json` — needed only when `serve.json` parsing lands at
  L1. Stage 5b does **not** depend on serde for the JSON 404 envelope,
  which is a fixed-shape literal (see § 4 phase 13).

## 3. Why a custom handler instead of `tower-http::ServeDir`

`ServeDir` resolves a URL path to a filesystem path in one shot
(canonicalization, directory→`index.html`, MIME, body) and returns a
ready-made response. That is convenient for L0 only; subsequent slices
need to interpose logic **before** static-file resolution per SRV-ROUT-006:

- L1 adds `cleanUrls` 301 (SRV-ROUT-001) and extensionless resolution
  (SRV-ROUT-002).
- L1/L2 add `trailingSlash` (SRV-ROUT-003/004), multi-slash collapse
  (SRV-ROUT-005), config-driven `redirects` (SRV-RDIR-001..003), and
  `rewrites` (SRV-RWRT-001).
- L2 binds them with the operation-precedence pipeline (SRV-ROUT-006).

If Stage 5a adopts `ServeDir`, Stage 6 either layers logic **on top of
it** (fragile — `ServeDir` already does its own canonicalization, so
running redirects after it produces inconsistent state) or rips it out
and rewrites. The cheaper path is to write the dispatcher with the
SRV-ROUT-006 pipeline shape from day one, even if early stages of the
pipeline are no-ops in L0.

`tower-http` is therefore declared only for ancillary middleware, not
for `ServeDir`.

## 4. Request lifecycle (skeleton)

The dispatcher mirrors SRV-ROUT-006's six pipeline stages, plus the
filesystem-resolution and response-shaping phases the spec implies. In
L0, only the bracketed phases are wired; the rest are no-ops that return
the request unchanged for the next phase to consume.

| # | Phase | Wired in 5a (L0)? | Owning SRV / spec Requirement |
|---|---|---|---|
| 1 | Parse incoming request (axum extractor) | yes | implicit (axum/hyper) |
| 2 | Method check (GET/HEAD; rest → 405) | yes | implicit |
| 3 | Multi-slash collapse | no (passthrough) | SRV-ROUT-005 (L2) |
| 4 | `cleanUrls` 301 | no | SRV-ROUT-001 (L2) |
| 5 | `trailingSlash` add/strip 301 | no | SRV-ROUT-003/004 (L2) |
| 6 | Config `redirects` | no | SRV-RDIR-001..003 (L2) |
| 7 | Config `rewrites` (incl. `--single`) | no | SRV-RWRT-001 (L2) |
| 8 | `cleanUrls` extensionless resolution | no | SRV-ROUT-002 (L2) |
| 9 | Resolve URL path → filesystem path under root | yes | SRV-FILE-001/005, SRV-CLI-007 |
| 10 | Path-traversal containment check | yes | (transitive of SEC-001 / hygiene) |
| 11 | Directory → `index.html` (or 404 — listing is L1) | yes | SRV-FILE-005, SRV-DLST-001 (L1, deferred) |
| 12 | File → 200 with MIME and body | yes | SRV-FILE-001, SRV-FILE-004 |
| 13 | Otherwise → 404 (HTML or JSON envelope per Accept) | yes | SRV-FILE-002 |

Phase 10 (containment check) is wired in 5a even though SRV-SEC-001 is
L2. Reasoning: a strict-L0 server that opens `..` is a security
regression, not a deferred feature. The L0 implementation
canonicalizes via `std::path::Path::canonicalize` and compares the
result's prefix to the canonicalized served-root prefix. On escape, L0
returns 404 (path "does not exist" inside root). The 400-status response
for wire-level traversal that SRV-SEC-001 mandates is **not** wired
until L2; design.md flags this as the single L0→L2 behavioral upgrade
on this code path.

### Module layout inside `irserve-core` (signatures only)

- `pub fn run(config: ServerConfig) -> Result<(), Error>`
- `pub struct ServerConfig { root: PathBuf, listen: SocketAddr }`
- `fn dispatch(req: Request<Body>, root: &Path) -> Response<Body>` —
  the per-request entry; runs phases 1–13.
- `fn resolve(url_path: &str, root: &Path) -> ResolveOutcome` — phases
  9–11.
- `enum ResolveOutcome { File(PathBuf), Index(PathBuf), NotFound, EscapedRoot }`
- `fn mime_for(path: &Path) -> Option<&'static str>` — phase 12. Returns
  `serve`-compatible bindings for the FILE-004 probed set; falls through
  to `mime_guess::from_path(...)` for unmatched extensions; returns
  `None` for extensionless and unknown-extension files (per FILE-004).
- `fn not_found_response(accept: &HeaderValue) -> Response<Body>` —
  phase 13. Inspects `Accept` for `application/json` and emits the
  fixed-shape JSON envelope or an HTML body. The HTML body content is
  implementation-defined per D-003.

These signatures are **descriptive**, not prescriptive: Stage 5b may
adjust types (e.g. `Bytes` instead of `&'static str`) as long as the
observable contract is preserved.

## 5. CLI surface (clap derive)

`irserve` (bin) recognizes exactly the four strict-L0 flags. All other
flags from the broader `cli/spec.md` are **deferred** by D-008 and
SHALL be rejected by clap as unknown flags (non-zero exit). This is
conservative and matches Stage-5b oracle expectation: an L1 change
widens acceptance.

| Form | Spec source | Notes |
|---|---|---|
| `[DIRECTORY]` (positional, default `.`) | SRV-CLI-007 | One positional only; two positionals → fatal (ORC-062). |
| `-l, --listen <PORT>` | SRV-CLI-002 | Numeric port only at L0. `tcp://` URIs (SRV-CLI-003) are L1. |
| `-h, --help` | SRV-CLI-019 | Exit 0; clap default. Help text is implementation-defined per D-002. |
| `-V, --version` | SRV-CLI-019 | Exit 0; clap default. Version string is `MAJOR.MINOR.PATCH` from `Cargo.toml`. |

Default port (SRV-CLI-001) is **3000** when `-l` is omitted and `PORT`
is unset; the `PORT` env var, if set, is used instead. This is wired in
the bin (clap `default_value_t` cannot read env at compile time —
Stage 5b reads `std::env::var("PORT")` after clap parses).

`--no-clipboard` (SRV-CLI-011) is **not** accepted at L0. D-008 defers
it; D-005 says clipboard is unimplemented anyway. The flag becomes a
clap-level no-op only when SRV-CLI-011 ships in a future change. The
strict rejection at L0 is intentional — Stage-5b oracle harness will
detect any divergence from contract.

## 6. Stage-5b oracle harness mapping

The strict-L0 binary must pass the following existing probe cases under
`tools/probe/cases/` without modifying any case or snapshot. The mapping
below cross-references probe → SRV → ORC and lists the response surface
the oracle harness asserts:

| Probe case (existing) | SRV(s) covered | ORC(s) | What the snapshot pins |
|---|---|---|---|
| `_smoke.json` | FILE-001, FILE-004, FILE-005, CLI-002, CLI-007 | ORC-001 | base smoke (200, body, MIME, `Content-Length`) |
| `mime-defaults.json` | FILE-004 | ORC-003 | MIME bindings for the eight required extensions |
| `notfound-shape.json` | FILE-002 | ORC-004 (HTML), ORC-005 (JSON) | 404 status, `Content-Type`, JSON envelope shape |
| `cli-help-version.json` | CLI-019 | ORC-060 (help), ORC-061 (version) | exit 0; stdout non-empty; server not started |
| `cli-positional-error.json` | CLI-007 sc.3 | ORC-062 | non-zero exit on two positionals |

Stage 5b's `tests/oracle/` runs each existing snapshot against `irserve`
in place of `node third_party/serve/build/main.js` and asserts response
equality up to the comparison rules already used by `tools/probe/run.js`
(snapshot fields excluded by D-002/D-003 — exact stdout text, HTML
markup of error pages — are not asserted; status, `Content-Type`,
header set, and body bytes for fixed-shape JSON are).

The probe cases above are the **whole** oracle surface for change 001.
All other probe cases under `tools/probe/cases/` exercise L1+ behavior
(cleanUrls, redirects, rewrites, directory listing, custom error pages,
ETag, Range, headers, CORS) and are deferred to subsequent change
proposals. Stage 5a does **not** modify any probe case or snapshot.

The probe runner currently launches `node third_party/serve/build/main.js`.
Stage 5b adapts the runner to also launch `irserve` (e.g. via an
environment variable selecting the target binary) without breaking
existing per-case parity. That adapter work is **Stage 5b** — not part
of change 001's design. `tasks.md` flags it as a 5b-prep item.

## 7. Open assumptions

These assumptions are baked into change 001's design but are not
verified against the oracle in 5a. Each Q-NNN below stays `open` in
`docs/reference/serve/open-questions.md`; the rule from anti-hallucination
#3 is preserved (Q-NNN close on oracle measurements only).

- **Q-001** — `tcp://` URI default port/host. Strict L0 does not accept
  `tcp://`-form `--listen` values (SRV-CLI-003 is L1, deferred by
  D-008), so the question is irrelevant to change 001's behavior. No
  assumption baked in beyond "the strict-L0 binary rejects `tcp://`
  forms".
- **Q-002** — compression set / threshold. D-006 already defers
  compression entirely; change 001 emits no `Content-Encoding` and no
  `Vary: Accept-Encoding`. Assumed acceptable.
- **Q-003** — schema validation error format. Not relevant — strict L0
  has no `serve.json` parser. Assumed irrelevant.
- **Q-004** — MIME bindings beyond the probed set. The hand-rolled
  table in `mime_for(...)` covers the eight FILE-004 bindings; anything
  else falls through to `mime_guess`. Assumed best-effort. Closing Q-004
  requires extending `tools/probe/cases/mime-defaults.json` and is a
  Stage-5b or later task.

No Q-NNN entry is closed by change 001. The `decisions.md` D-008 entry
explicitly records the strict-L0 cutoff but does not affect the open
status of any Q.

## 8. Spec delta — traceability-only MODIFIED

The change carries one delta file: `specs/cli/spec.md` with a single
MODIFIED Requirement targeting SRV-CLI-001 ("Default port and host").
The delta:

- Reproduces the original Requirement body, scenarios, Evidence line,
  and Note **verbatim**. Observable behavior is unchanged.
- Adds a single `Implementation:` line below `Evidence:` that points
  at change `001-port-minimal-static-server` and notes the env-var-
  vs-clap separation lives in `crates/irserve` (the bin), not in
  `irserve-core`.

Rationale: OpenSpec's `validate --strict` rejects any change with zero
deltas. The Stage-5a interview committed to "no deltas" because the
contract is unchanged; the validator constraint, discovered during the
implementation pass, forces a minimum-viable delta. A traceability
update is the smallest legitimate change to a spec — it does not
redefine behavior, does not create churn for follow-up changes (the
Implementation line can stay or be promoted as 5b lands), and gives
readers of `cli/spec.md` a forward link to where the default-port
behavior is wired.

This is the **only** delta in change 001. It is not a behavioral
adapter, not a new Requirement, and not a status-promotion (the SRV
stays `accepted`; promotion is gated on the Stage-5b probe per Risk #3
in `proposal.md`).
