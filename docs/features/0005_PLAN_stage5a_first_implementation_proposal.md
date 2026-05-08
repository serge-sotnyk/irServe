# Stage 5a — first implementation proposal: `001-port-minimal-static-server`

## Context

`README.md` stage-map line 20 marks Stage 5a as **Next**. Stages 1–3 produced
the research base (52-entry SRV inventory, 59 ORC oracle matrix, decisions
log D-001..D-007, open-questions log, committed snapshots). Stage 4 (change
`000-establish-serve-compatibility-baseline`) promoted 35 SRVs across 8
capability namespaces into the OpenSpec contract; the bootstrap was authored
**propose-only** and is still not archived (`openspec/specs/` has only
`.gitkeep`).

Stage 5a delivers the **first implementation proposal**: a textual contract
for the smallest useful Rust port — a strict-L0 static server (8 SRVs).
Stage 5a produces no Rust code; that lands at Stage 5b. The value of 5a is
forcing concrete architectural decisions (crate layout, HTTP stack,
request-lifecycle order) into a reviewable artifact **before** any
`Cargo.toml` exists.

Decisions taken during interview:

1. **Archive bootstrap 000 first**, as a separate commit, before 5a authoring
   begins. After archive, the contract lives in `openspec/specs/<cap>/spec.md`
   (permanent) and `openspec/changes/archive/000-…/` (history).
2. **First-slice scope = strict L0 (8 SRVs)** — no `serve.json`, no
   `cleanUrls`, no directory listing. The slice exists to validate the
   methodology end-to-end against the existing oracle, not to ship a useful
   product.
3. **Crate layout = workspace** with `irserve` (bin) + `irserve-core` (lib).
   The split is fixed in 5a's `design.md`; the `Cargo.toml` files appear in
   5b.
4. **HTTP stack = axum 0.8 on hyper+tokio+tower-http**.
5. **No spec deltas in change 001** — strict L0 is already in the archived
   contract; 5a is `proposal.md + design.md + tasks.md` only.
6. **D-008 added to `decisions.md`** to record the strict-L0 cutoff as a
   release-scoping decision (not a behavioral divergence from `serve`).
7. **`compatibility-levels.md` reconciled**: SRV-CLI-003 moved from the L0
   "Bind host and port" bullet to the L1 "Subset of `serve` CLI options"
   bullet, matching the archived spec and `inventory.md`.

## Critical files

### Read-only inputs (drive authoring; do not modify)

- `docs/reference/serve/inventory.md` — SRV bodies (8 in scope: CLI-001,
  CLI-002, CLI-007, CLI-019, FILE-001, FILE-002, FILE-004, FILE-005).
- `docs/reference/serve/oracle-matrix.md` — ORC IDs the Stage-5b harness will
  reuse: ORC-001 (file-serving root), ORC-003 (MIME), ORC-004/005 (404
  HTML/JSON), ORC-060/061 (help/version), ORC-062 (two-positional error).
- `openspec/specs/cli/spec.md`, `openspec/specs/static-files/spec.md` (after
  archive) — Requirement bodies that change 001 implements without
  modification.
- `docs/reference/serve/decisions.md` — D-001..D-007.
- `docs/reference/serve/open-questions.md` — open Q-001..Q-011 to list as
  "open assumptions" if 5a touches their area (Q-001 default port — relevant
  to L0; Q-004 MIME beyond probed set — relevant to L0).
- `openspec/AGENTS.md`, `openspec/project.md` — authoring rules.
- `tools/probe/cases/`, `tools/probe/snapshots/` — to enumerate what the
  Stage-5b oracle harness can call without modification.
- `third_party/serve/source/`, `third_party/serve-handler/source/` — read-only
  reference; cited only where source-level evidence backs a Requirement that
  no probe yet exercises.

### Files modified by Stage 5a

- `openspec/changes/001-port-minimal-static-server/proposal.md` — new.
- `openspec/changes/001-port-minimal-static-server/design.md` — new.
- `openspec/changes/001-port-minimal-static-server/tasks.md` — new.
- `docs/reference/serve/decisions.md` — append D-008.
- `docs/reference/serve/compatibility-levels.md` — single-line reconcile of
  SRV-CLI-003 (L0 bullet → L1 bullet).
- `README.md` — stage-map row 5a `todo` → `done` (only at end of stage).

### Files explicitly NOT touched

- Any file under `tools/probe/` or `third_party/`.
- `openspec/specs/` (no Requirement bodies change).
- `openspec/changes/archive/000-…/` after archive (frozen history).
- No `Cargo.toml`, no `*.rs`, no `tests/`. Stage 5a is text-only.

## Pre-stage steps (separate commits, before authoring change 001)

### Pre-step A — Archive bootstrap 000

Move content per OpenSpec lifecycle (`propose → apply → sync → archive`):

1. For each `openspec/changes/000-…/specs/<cap>/spec.md`, replace
   `## ADDED Requirements` with `# <cap> spec` (or whatever heading the
   destination expects), then write to `openspec/specs/<cap>/spec.md`.
   Capability list to migrate: `cli`, `config`, `static-files`, `routing`,
   `redirects`, `rewrites`, `security`, `directory-listing`. Reserved
   namespaces with no spec stay reserved (`headers`, `http-cache`, `cors`,
   `symlinks`).
2. Move `openspec/changes/000-establish-serve-compatibility-baseline/` to
   `openspec/changes/archive/000-establish-serve-compatibility-baseline/`.
3. Delete `openspec/specs/.gitkeep` (replaced by real `spec.md` files).
4. Run `npx -y @fission-ai/openspec@latest validate --all --strict
   --concurrency 12` → exits 0.
5. Update `openspec/AGENTS.md` if the layout block still says "populated
   after Stage 4 archives the bootstrap change" (line 11) — drop that hedge.

Verification: `git diff --name-only` shows changes only under `openspec/`;
`grep -RIl "ADDED Requirements" openspec/specs/` returns empty.

### Pre-step B — Reconcile `compatibility-levels.md`

Single-line edit: in `docs/reference/serve/compatibility-levels.md` line 28
("Bind host and port — SRV-CLI-001, SRV-CLI-002, SRV-CLI-003"), drop
SRV-CLI-003 (it's L1 per `inventory.md` and the archived `cli/spec.md`).
Append SRV-CLI-003 to line 36 (the L1 "Subset of `serve` CLI options"
bullet). After the fix L0 strict count = 8: CLI-001, CLI-002, CLI-007,
CLI-019, FILE-001, FILE-002, FILE-004, FILE-005.

Pre-step A and Pre-step B can ride in one commit or two; user's call. Treat
them as **prerequisites** for change 001, not as part of it.

## Stage 5a authoring

### Change folder layout

```
openspec/changes/001-port-minimal-static-server/
├── proposal.md
├── design.md
└── tasks.md
```

No `specs/` directory. Per OpenSpec authoring rules confirmed via Context7
(`/fission-ai/openspec`), `specs/` is optional when the change carries no
ADDED/MODIFIED/REMOVED Requirements.

### `proposal.md` — content outline

Required sections (concrete content to write, not headings to copy verbatim):

- **Why** — Stage 5b needs a contract with crate layout and HTTP stack
  decided; bootstrap archived but says nothing about implementation. Slice
  exists to validate the oracle-harness round-trip end-to-end on the
  smallest possible code surface.
- **What** — implement strict L0 (8 SRVs) in Rust. Cite each SRV by ID with
  level and current status:
  - CLI-001 (default port 3000, accepted), CLI-002 (`-l <port>`, verified,
    ORC-001), CLI-007 (positional dir, verified, ORC-001/062),
    CLI-019 (`--help`/`--version`, verified, ORC-060/061),
  - FILE-001 (regular file, verified, ORC-001), FILE-002 (404, verified,
    ORC-004/005), FILE-004 (MIME defaults, verified, ORC-003), FILE-005
    (index.html, verified, ORC-001).
- **Scope/Non-goals** — explicit list of what's deliberately out: serve.json,
  cleanUrls, trailingSlash, redirects, rewrites, directory listing,
  --single, --cors, --debug, --no-clipboard, --no-port-switching, --config,
  TLS, symlinks, compression, ETag, Range, Last-Modified. Each name pairs
  with the SRV ID it defers (CFG-001/002, ROUT-*, RDIR-*, RWRT-*, DLST-*,
  CLI-008..016, CACHE-*, CORS-*, SYM-*, HDR-*).
- **Risks** — three concrete:
  1. axum's `tower-http::ServeDir` short-cuts the request lifecycle in a way
     incompatible with later L2 routing (cleanUrls/redirects need to run
     **before** static-file resolution, see SRV-ROUT-006). Mitigation
     described in `design.md`: do not adopt ServeDir as the request engine;
     write a custom handler from the start.
  2. mime_guess's bindings differ from `serve`'s `mime-types` package (no
     `; charset=utf-8` suffix on text types by default). FILE-004 spec
     mandates the suffix. Mitigation: hand-roll a small MIME table for the
     FILE-004 probed set; fall back to mime_guess for the long tail (Q-004).
  3. Default-port path (CLI-001) has no probe (every probe passes `--listen`).
     Mitigation: Stage 5b adds a probe case `default-port.json` that omits
     `--listen` to promote SRV-CLI-001 from `accepted` to `verified`. Logged
     as a follow-up in `tasks.md` § 5b prep.
- **Open assumptions** — explicit list of Q-NNN that the design touches but
  cannot close (no probe added in 5a):
  - Q-001 (`tcp://` default port/host) — strict L0 doesn't accept `tcp://`
    URIs at all (CLI-003 is L1), so Q-001 stays open and is irrelevant to
    this slice.
  - Q-002 (compression set) — not in scope; D-006 covers.
  - Q-004 (MIME beyond probed set) — design pins the probed set as the
    contract; remainder is best-effort via `mime_guess`. Q-004 stays open.

### `design.md` — concrete architectural decisions

This is the heart of Stage 5a. Reviewer must be able to verify each choice
**before any code is written**. **No Rust code blocks longer than function
signatures.** No `fn handle(...) -> Response { ... actual body ... }`. Yes
`fn dispatch(req: Request, root: &Path) -> Response<Body>` as a one-line
signature.

#### 1. Crate layout

```
irserve/                       (workspace root, no Cargo.toml in 5a)
├── crates/
│   ├── irserve/               (bin: thin CLI entry, ~50 LOC: arg-parse, init, hand off)
│   │   └── src/main.rs
│   └── irserve-core/          (lib: Server, dispatcher, mime, config types)
│       └── src/lib.rs
└── tests/oracle/              (Stage 5b only; integration tests against tools/probe/)
```

Justification: `irserve-core` is intended to host all observable-behavior
logic so Stage 5b's `tests/oracle/` and any future replacement of the bin
shell (e.g. an embedded mode) can use the same code path. Two crates rather
than `lib.rs + bin/` because the workspace boundary forces absence of CLI
parsing in core (clap stays in `irserve` only). Justified per user's Stage-5a
interview decision.

#### 2. Dependencies (versions to pin in 5b's `Cargo.toml`)

Direct dependencies for `irserve-core`:

- `axum` 0.8.x — Router, IntoResponse, fallback handler.
- `hyper` 1.x — re-exported via axum; not a direct dep.
- `tokio` 1.x with features `["rt-multi-thread", "net", "fs", "macros",
  "signal"]`.
- `tower-http` 0.6.x — only for `RequestBodyLimitLayer`-style middlewares as
  needed; **NOT** for `ServeDir` (see "Why custom handler" below).
- `bytes` 1.x — body construction.
- `mime_guess` 2.x — MIME long-tail; overridden by the hand-rolled table.

Direct dependencies for `irserve`:

- `clap` 4.x with `derive` feature — argument parsing.
- `irserve-core` — workspace member.
- `tokio` (only `["macros", "rt-multi-thread"]`) — `#[tokio::main]`.

`tracing` / `tracing-subscriber` deferred (D-002 — log format
implementation-defined, not part of L0 contract).

#### 3. Why custom handler instead of `tower-http::ServeDir`

`ServeDir` resolves a URL path to a file in one shot. Stage 6 will add
cleanUrls/trailingSlash/redirects/rewrites which **must run before**
file-resolution per SRV-ROUT-006 ("operation precedence"). If Stage 5a adopts
`ServeDir`, Stage 6 has to either layer logic on top (fragile) or rip
`ServeDir` out and rewrite. Cheaper to write a custom dispatcher now that
already has a "lifecycle" shape, even if early stages of the lifecycle are
no-ops.

#### 4. Request lifecycle (skeleton)

Numbered phases. Each phase a future stage will fill in; in 5a most are
no-ops:

```
1. parse incoming request (axum extractor)         — Stage 5a
2. method check (GET/HEAD only; rest → 405)        — Stage 5a (5b TODO: probe HEAD)
3. normalize multi-slashes                         — Stage 6 (SRV-ROUT-005)
4. cleanUrls 301 redirect                          — Stage 6 (SRV-ROUT-001)
5. trailingSlash 301 redirect                      — Stage 6 (SRV-ROUT-003/004)
6. config redirects                                — Stage 6 (SRV-RDIR-*)
7. config rewrites                                 — Stage 6 (SRV-RWRT-*)
8. cleanUrls extensionless resolution              — Stage 6 (SRV-ROUT-002)
9. resolve to filesystem path inside served root   — Stage 5a
10. path-traversal denial                          — Stage 5a (transitive, std::path::Path::canonicalize + prefix check)
11. directory → index.html OR listing OR 404       — Stage 5a (only index.html branch; listing is L1)
12. file → 200 with MIME and body                  — Stage 5a
13. anything else → 404 (HTML body or JSON envelope per Accept) — Stage 5a
```

Phase 10 is in 5a because traversal denial is L0 hygiene (SEC-001 is L2 but
the underlying canonicalization can't be skipped at L0 — a strict L0 server
that opens `..` is a security regression, not a deferred feature). Document
in design.md as: "We canonicalize and reject; we do not yet wire the
explicit 400-status response from SEC-001 (L2)." Treat 5a as 404 for
escape; 5b decides whether to upgrade to 400.

Sketch as text, not code:

- `Server::new(root: PathBuf, addr: SocketAddr) -> Self`
- `Server::run(self) -> Result<(), Error>`
- `dispatch(req: Request, root: &Path) -> Response`
- `resolve(url_path: &str, root: &Path) -> ResolveOutcome` where outcome is
  `File(PathBuf)` | `Index(PathBuf)` | `NotFound` | `EscapedRoot`.
- `mime_for(path: &Path) -> Option<&'static str>` returning `serve`-compatible
  bindings for the probed set (FILE-004), `mime_guess` fallback otherwise.

#### 5. CLI surface (clap derive)

Map only L0 flags. Reject unknown flags **silently or strictly** — design
choice for 5a:

- `[DIRECTORY]` (positional, defaults to `.`).
- `-l, --listen <PORT>` (numeric only at L0; `tcp://` URIs and host:port are
  L1).
- `-h, --help` and `-V, --version`.

Strict mode: reject unknown flags with non-zero exit. This is conservative
and matches Stage-5b oracle expectation. L1 will widen acceptance.

#### 6. Stage-5b oracle harness mapping

This section earns its keep by enumerating which existing
`tools/probe/cases/*.json` cases the L0 binary must pass without
modification. The table makes Stage 5b's first job obvious:

| Probe case (existing)              | SRV(s) covered | ORC(s) | Notes |
|-----------------------------------|----------------|--------|-------|
| `defaults-baseline.json`          | FILE-001/004/005, CLI-002/007 | ORC-001 | base smoke |
| `mime-defaults.json`              | FILE-004       | ORC-003 | 8 extensions |
| `not-found.json`                  | FILE-002       | ORC-004 | HTML 404 |
| `not-found-json.json`             | FILE-002       | ORC-005 | JSON 404 envelope |
| `cli-help.json`                   | CLI-019        | ORC-060 | exit 0, stdout non-empty |
| `cli-version.json`                | CLI-019        | ORC-061 | exit 0 |
| `cli-two-positional.json`         | CLI-007 sc.3   | ORC-062 | non-zero exit |

Stage 5b's `tests/oracle/` runs each existing snapshot against `irserve`
instead of `node third_party/serve/build/main.js` and asserts byte-/header-
equality up to the response-comparison rules already used by
`tools/probe/run.js`. **Stage 5a does not modify** any probe case, but
design.md cites this list so reviewers can see the slice is end-to-end
testable.

If any of those probe cases doesn't exist by name, replace with the closest
existing case identified in `tools/probe/cases/` listing — Stage 5b job to
reconcile if names drift. Do not invent new probe cases at 5a.

#### 7. Open assumptions

Verbatim list of `Q-NNN` left open and **assumed** for 5a/5b without
verification:

- Q-001 (default port behavior on `tcp://`) — out of L0 scope; assumed
  irrelevant.
- Q-004 (MIME beyond probed set) — assumed `mime_guess` fallback acceptable.
- Q-002 (compression details) — assumed N/A; no compression at L0.

These remain `open` in `open-questions.md` — Stage 5a does **not** close
them.

### `tasks.md` — implementation checklist

Sectioned for both Stage 5a self-checks and Stage 5b prep. Each item is one
line, present-tense imperative, one checkbox. Example sections:

1. **Pre-authoring** (preconditions: bootstrap archived; compatibility-levels
   reconciled; D-008 added).
2. **Author proposal.md** (1.1 Why, 1.2 What+SRV table, 1.3 Scope, 1.4
   Non-goals, 1.5 Risks, 1.6 Open assumptions).
3. **Author design.md** (2.1 Crate layout, 2.2 Deps, 2.3 ServeDir
   rationale, 2.4 Lifecycle, 2.5 CLI surface, 2.6 Oracle harness mapping,
   2.7 Open assumptions).
4. **Validate** — `npx -y @fission-ai/openspec@latest validate --all
   --strict --concurrency 12` exits 0.
5. **Cross-check** — every SRV ID cited in proposal/design is in
   `inventory.md`; every ORC ID cited is in `oracle-matrix.md`; every Q-NNN
   cited is in `open-questions.md`; every D-NNN cited is in `decisions.md`
   (including the new D-008).
6. **Stage-5b prep** (informational, not closed in 5a) — add probe for
   CLI-001 default-port; design oracle harness wiring; pin Cargo deps;
   `tests/oracle/` skeleton.
7. **Stage map update** — `README.md` row 5a `todo` → `done` after
   `proposal.md`/`design.md`/`tasks.md` reach validation green.

`tasks.md` does **not** track Rust implementation tasks themselves — those
belong to a future change `002-implement-strict-l0-runtime` opened at
Stage 5b. Stage 5a is design-only.

### `decisions.md` — D-008

```
## D-008: First-slice strict-L0 cutoff

Date: 2026-05-08
Affected requirements: SRV-CFG-001, SRV-CFG-002, SRV-FILE-003, SRV-ROUT-001..006,
  SRV-RDIR-001..003, SRV-RWRT-001, SRV-DLST-001..003, SRV-CLI-003, SRV-CLI-006,
  SRV-CLI-008..016 (deferred from the first release; not removed from contract)
Status: adapted (release-scoping; not behavioral divergence)
Reason: Change 001 implements only strict-L0 (8 SRVs) to prove the methodology
end-to-end against the existing oracle. Behavioral parity with `serve` for L1+
SRVs is unchanged; their delivery is sequenced into later changes (002+).
Impact: First IrServe release responds to L0 inputs only; L1 inputs (e.g.
`-l tcp://...`, `serve.json`-driven cleanUrls) yield strict CLI rejection or
absent-feature behavior, not parity. No SRV bodies change.
```

D-008 is a **release-scoping** decision, not a behavioral divergence — the
Status field reflects that. Future stages should not need to amend SRV
bodies because of D-008.

## Verification (end-to-end of Stage 5a)

After all writes:

1. `npx -y @fission-ai/openspec@latest validate --all --strict --concurrency 12`
   exits 0 (validates archived bootstrap + new change 001).
2. Manual: open `openspec/specs/cli/spec.md` and confirm CLI-001/002/007/019
   are present (proves archive Pre-step A worked).
3. Manual: `compatibility-levels.md` line 28 no longer mentions SRV-CLI-003;
   line 36 (or thereabouts) does.
4. Manual: `decisions.md` ends with a `D-008` block matching the template
   above; no other D-NNN was changed.
5. Manual: every Evidence-style citation in `proposal.md`/`design.md`/
   `tasks.md` resolves: SRV → `inventory.md`, ORC → `oracle-matrix.md`,
   D-NNN → `decisions.md`, Q-NNN → `open-questions.md`. No fabricated IDs.
6. Manual: `git diff --stat` shows zero lines under `tools/probe/`,
   `third_party/`, or any `*.rs`/`Cargo.*` file. No Rust artifacts.
7. Manual: `design.md` contains no Rust code block longer than a function
   signature. Pseudocode and signatures are OK; concrete bodies are not.
8. `README.md` stage-map row 5a status set to `done`. No other rows touched.

There is no runtime test in Stage 5a — the artifact is a textual contract,
verified by OpenSpec validation, citation resolution, and reviewer reading.

## Reminders for the implementation session (process notes)

- Bootstrap archive is a **precondition**, not part of change 001. Land it
  first (separate commit) and confirm `openspec/specs/` populates before
  starting `proposal.md`.
- Rust code is **forbidden** in this stage. No `Cargo.toml`, no `*.rs`. If a
  reviewer asks for "show me the code", point at the function-signature
  sketches in §4 and §5 of `design.md` — that is the intended depth.
- `decisions.md` is a living document. D-008 lands in this stage; future
  stages append D-009+ as new release-scoping or behavioral-divergence
  decisions arise.
- `open-questions.md` is **not** edited in 5a. Q-NNN entries close only on
  oracle measurements (Stage-3 rule continues to apply).
- Codex review will be more demanding than Stage 4's. Stage 4 was mechanical
  inventory transcription; 5a defends architectural choices. Expect 2–3
  rounds of revision; budget accordingly.
