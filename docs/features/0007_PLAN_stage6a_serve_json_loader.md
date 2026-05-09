# Stage 6a — `serve.json` loader

## Context

Stage 6a is the next sub-stage of Stage 6 (`docs/stage6_l1_l2_capabilities.md:49`).
It introduces the configuration layer that every other 6x sub-stage
(routing normalization, cleanUrls, redirects, rewrites, headers,
listings, CLI fill-in) reads its parameters from. Without 6a none of
6b–6h can ship as observable changes.

The contract for 6a is already authored in `openspec/specs/config/spec.md`:

- **SRV-CFG-001** — config file lookup chain (`--config <path>` →
  `serve.json` → `now.json#now.static` → `package.json#static`), missing
  implicit silently skipped, missing explicit fatal, malformed JSON
  fatal, `public` resolved relative to served dir. Currently `verified`
  via ORC-006 (only the `--config alt.json` redirect surface). Status
  unchanged after 6a; oracle list grows.
- **SRV-CFG-002** — schema map of 11 accepted fields. Stays `accepted`
  (meta-requirement). Status unchanged after 6a.

D-008 currently defers SRV-CFG-001/CFG-002 from the first IrServe
release; D-009 in this change un-defers them.

## Decisions taken in plan-mode (carry into design.md)

1. **Observable surface in 6a is `public` + fatal-error scenarios.** The
   other 10 fields are parsed into a typed struct and stored on the
   server config for 6b–6g to consume; 6a's dispatcher does not read
   them. Rationale: `public` is the only field whose effect is
   independent of routing/redirects/listings, so it is the only field
   we can probe in isolation at this stage.
2. **Schema validation is `serde` with `#[serde(deny_unknown_fields)]` +
   `rename_all = "camelCase"` + untagged enums for `bool | string[]`
   shapes.** Q-003 stays open. D-002 already excludes byte-for-byte
   AJV-message parity, so serde-shaped messages are fine.
3. **`ServeConfig.public` stays raw `Option<String>` in the parser.**
   Resolution to `PathBuf` happens in the bin after the served-dir
   positional is known. The parser stays pure (no filesystem access
   beyond reading the candidate file).
4. **`now.json` / `package.json` deprecation**: emit a single stderr
   warning when one of those is the source. D-002 keeps wording
   implementation-defined. No D-NNN needed (covered by D-002).
5. **Missing top-level `now` key in `now.json`** (reference would crash
   with `TypeError: Cannot read properties of undefined`): IrServe skips
   cleanly to the next file. Authorize as D-004 micro-divergence in the
   change-package design; no new D-NNN.
6. **`config-explicit.json` (existing probe with redirect assertion)**
   stays without a `runner.l0` block — the runner auto-skips it for
   `target=irserve` until 6d adds the block. Add a `TODO(6d)` comment in
   the case file.

## Critical files

### Read before editing
- `openspec/specs/config/spec.md` — SRV-CFG-001/CFG-002 verbatim, the
  five Scenarios under SRV-CFG-001, the schema map under SRV-CFG-002.
- `openspec/changes/001-port-minimal-static-server/design.md` — §4
  pipeline, §6 oracle harness layer (architectural foundations).
- `openspec/changes/002-implement-strict-l0-runtime/{proposal,design,tasks}.md`
  — shape and tone for the new change package.
- `docs/features/0006_PLAN_stage5b_strict_l0_rust_runtime.md` — plan
  template precedent (slice structure, working-pin section, verify
  steps per slice).
- `third_party/serve/source/utilities/config.ts` — reference loader.
  Key lines: `:31-52` (lookup + explicit fatality), `:73-95` (parse +
  nested-key extraction), `:102-105` (deprecation warning), `:112-120`
  (`public` resolution), `:123-137` (AJV).
- `third_party/serve-handler/node_modules/@zeit/schemas/deployment/config-static.js`
  (or equivalent path) — schema reference for field shapes (already
  digested in this plan).
- `tools/probe/run.mjs:46-54` — `L0_DEFERRED_FLAGS`. `:872-874` —
  auto-skip when no `runner.l0` block. `:651-657` — partition
  contract (empty `clean` is rejected).

### To create
- `openspec/changes/003-load-serve-json/{proposal.md, design.md, tasks.md}`
  — change package, no spec delta files (no MODIFIED needed).
- `crates/irserve-core/src/config.rs` — `ServeConfig` struct +
  `load_serve_json(served_dir, explicit_path) -> Result<Option<ServeConfig>, Error>`.
- `tools/probe/cases/serve-json-public.json` — HTTP-mode probe.
- `tools/probe/cases/config-missing-explicit.json` — CLI-mode probe.
- `tools/probe/cases/config-malformed.json` — CLI-mode probe.
- `tools/probe/snapshots/{serve-json-public, config-missing-explicit, config-malformed}.json`
  — recorded via `--snapshot=update --target=reference`.

### To modify
- `crates/irserve-core/src/lib.rs` — re-export `ServeConfig`,
  `load_serve_json`; widen `Error` enum with `Config(String)` variant
  (or similar) covering parse / validation / non-object.
- `crates/irserve-core/Cargo.toml` — add `serde = { version = "1", features = ["derive"] }`,
  `serde_json = "1"`.
- `crates/irserve/src/main.rs` — new clap field `-c/--config <PATH>`;
  call `load_serve_json` from served-dir; compute effective root from
  `config.public` then canonicalize; propagate fatal errors as non-zero
  exit.
- `tools/probe/run.mjs` — drop `'-c'`, `'--config'` from
  `L0_DEFERRED_FLAGS` (slice 3, after `public` is wired).
- `tools/probe/cases/config-explicit.json` — append `TODO(6d)` note in
  the `description` field; no `runner.l0` block (auto-skip).
- `docs/reference/serve/inventory.md` — extend SRV-CFG-001 oracle list
  with new ORC IDs.
- `docs/reference/serve/oracle-matrix.md` — add three new ORC rows
  (e.g. ORC-064 `public`, ORC-065 `missing-explicit`, ORC-066
  `malformed-json`).
- `docs/reference/serve/decisions.md` — add D-009 amending D-008's
  deferred-SRV list (drop SRV-CFG-001, SRV-CFG-002).
- `README.md` — Stage map row 6a `todo` → `done`.

## ServeConfig sketch

```rust
#[derive(Debug, Clone, Default, serde::Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct ServeConfig {
    pub public: Option<String>,
    pub clean_urls: Option<BoolOrGlobs>,
    pub trailing_slash: Option<bool>,
    #[serde(default)] pub rewrites: Vec<RewriteRule>,
    #[serde(default)] pub redirects: Vec<RedirectRule>,
    #[serde(default)] pub headers: Vec<HeaderRule>,
    pub directory_listing: Option<BoolOrGlobs>,
    #[serde(default)] pub unlisted: Vec<String>,
    pub render_single: Option<bool>,
    pub symlinks: Option<bool>,
    pub etag: Option<bool>,
}

#[derive(Debug, Clone, serde::Deserialize)]
#[serde(untagged)]
pub enum BoolOrGlobs { Bool(bool), Globs(Vec<String>) }

// rules: { source, destination, type? }, headers: nested object, etc.
// — all with deny_unknown_fields.
```

`etag` is included in the spec map but not in the reference's AJV
schema; the spec is the contract and includes it explicitly, so we
accept it. (`config-static.js` omission is the reference's quirk; D-004
authorizes the spec divergence.)

## Iterative slices

### Slice 1 — types + parser (unit-tests only)
- Add `serde`, `serde_json` to `irserve-core/Cargo.toml`.
- Author `crates/irserve-core/src/config.rs` with `ServeConfig`,
  enums, and `load_serve_json(served_dir, explicit_path)`.
- Lookup behavior: explicit path absolute or relative-to-served-dir
  (mirror `config.ts:38`); explicit missing → `Err`; implicit chain
  searches `serve.json`, `now.json` (extracts `.now.static`),
  `package.json` (extracts `.static`); first valid wins; rest skipped.
- Empty object `{}` → `Some(ServeConfig::default())`. Non-object root
  → `Err`. Malformed JSON → `Err`. Unknown field → `Err`. Missing
  `now.static` / `static` → continue chain. Missing top-level `now` in
  `now.json` → continue chain (D-004 micro-divergence).
- Unit tests in `config.rs` (`#[cfg(test)] mod tests`):
  parsing each field shape, `deny_unknown_fields` rejects, untagged
  enum decodes both shapes, malformed JSON errors, non-object errors,
  empty object OK, absolute `--config` path, relative `--config` path,
  now.json with and without `now.static`, package.json with and
  without `static`, now.json without top-level `now`.
- Verify: `cargo build` clean, `cargo test -p irserve-core` green,
  `cargo test --test oracle` still green (no behavior change).

### Slice 2 — bin wires `--config`, fatal errors propagate
- New clap field `#[arg(short = 'c', long = "config", value_name = "PATH")]
  config: Option<PathBuf>`.
- Bin calls `load_serve_json(&cli.directory, cli.config.as_deref())`
  before `canonicalize`. On error: print to stderr, exit non-zero
  (use `Box<dyn Error>` propagation already in place at `main.rs:46`).
- Effective root **still** = `cli.directory.canonicalize()` —
  `public` not wired yet.
- `ServerConfig` extended with `serve_config: ServeConfig` (Default
  if `load_serve_json` returns `Ok(None)`); core does not consume it.
- Do **not** touch `L0_DEFERRED_FLAGS` in this slice. Calling `--config`
  silently no-ops on `public` would violate the runner's
  refuse-rather-than-silent contract (`run.mjs:881`).
- Verify: `cargo build` clean, `cargo test --test oracle` green.

### Slice 3 — `public` + new probes + flag un-defer
- Bin computes effective root: `cli.directory.join(serve_config.public
  .as_deref().unwrap_or("."))` then `.canonicalize()?`. Pass to
  `ServerConfig.root`.
- Drop `'-c'`, `'--config'` from `tools/probe/run.mjs:46-54`
  `L0_DEFERRED_FLAGS`.
- `tools/probe/cases/config-explicit.json` — add `TODO(6d): redirect
  anchor depends on configured-redirects (6d); auto-skipped under
  target=irserve until then.` to its `description`. No `runner.l0`
  block.
- Add `serve-json-public.json`: fixture has `serve.json:
  {"public":"site"}` and `site/index.html`; request `GET /`; assert
  200 + index body. `runner.l0.clean: ["root_serves_public_index"]`.
- Add `config-missing-explicit.json` (CLI-mode probe per
  `cli-positional-error.json` precedent): `serveArgs: ["--config",
  "does-not-exist.json"]`; assert non-zero exit; `runner.l0.clean:
  [...anchor...]`, `exitCodeMayDiffer` if exit codes diverge between
  reference (likely 1) and IrServe (likely 1 too via `Box<dyn Error>`,
  but verify and relax if needed).
- Add `config-malformed.json` (CLI-mode probe): fixture has
  `serve.json: "{not json"`; assert non-zero exit. Same `runner.l0`
  shape as above.
- Snapshots recorded via `node tools/probe/run.mjs <case>
  --snapshot=update --target=reference`.
- Verify: `cargo test --test oracle` green; `node tools/probe/run.mjs
  --all --target=reference --snapshot=verify` green (existing
  reference snapshots untouched); the three new snapshots added.

### Slice 4 — change package, research-track, README, validate
- `openspec/changes/003-load-serve-json/proposal.md` — pattern after
  002's proposal. SRV-CFG-001 stays `verified` (no MODIFIED), but the
  ORC list grows. Note in proposal: no spec deltas; only research-track
  edits + behavioral implementation. Findings section if any
  divergences surfaced during slice 1–3.
- `openspec/changes/003-load-serve-json/design.md` — module map (one
  new module: `config.rs`), serde-vs-AJV justification, lookup &
  resolution algorithm, deprecation-warning shape, D-004 micro-
  divergence note for missing-`now`-key.
- `openspec/changes/003-load-serve-json/tasks.md` — slice-by-slice
  task checklist (mirror 002's tasks.md format).
- `docs/reference/serve/inventory.md` — append new ORC IDs to
  SRV-CFG-001's oracle line.
- `docs/reference/serve/oracle-matrix.md` — three new rows; one ORC
  per new probe.
- `docs/reference/serve/decisions.md` — append D-009 (drops SRV-CFG-001,
  SRV-CFG-002 from D-008's deferred set; references this change).
- `README.md` — Stage map row 6a `todo` → `done` (and any Status block
  text update mirroring 5b's done-flip).
- Validate: `npx -y @fission-ai/openspec@latest validate --all
  --strict` exit 0; `cargo test --test oracle` green; `cargo build
  --release` clean; `git diff --stat` confirms zero `third_party/`
  edits and no edits to existing snapshots (only adds).

## Risks and open assumptions

- **R1 — Windows `public` path joining + canonicalize.** When
  `cli.directory` is a relative path (`.`), `cli.directory.join(public)`
  may produce non-existent intermediate components on Windows. Mitigation:
  canonicalize the served-dir positional first, then join `public`,
  then canonicalize again, then verify `starts_with(canonical_served_dir)`
  to reject `public: ".."` from escaping.
- **R2 — serde error message stability across versions.** D-002 keeps
  wording implementation-defined; the only oracle assertion on stderr
  is `kind: text` (per `run.mjs:601-609`). Q-003 stays open.
- **R3 — serde `deny_unknown_fields` interaction with untagged enums.**
  serde does not enforce `deny_unknown_fields` inside untagged variants
  uniformly; verify `BoolOrGlobs` does not silently accept odd shapes.
  Mitigation: explicit unit test (e.g. `clean_urls: { foo: 1 }` → Err).
- **R4 — `now.json` without top-level `now` key.** Explicit D-004
  micro-divergence (skip cleanly). Document in design.md, no D-NNN.
- **A1 — `etag` field accepted even though absent from reference's AJV
  schema.** The spec map at `spec.md:71` includes it; we honor the spec
  over the reference quirk per D-004. Document in design.md.
- **A2 — `--config` and the served-dir positional do not conflict in
  clap.** Flags and positionals are distinguished by `--`-prefix; no
  conflict possible. Downgrade from "risk" to "non-issue" once
  manually confirmed in slice 2.

## Verification (end-to-end)

After all four slices land:

```
cargo build --release
cargo test --test oracle               # all L0 cases green; new probes green
node tools/probe/run.mjs --all --target=reference --snapshot=verify
node tools/probe/run.mjs --all --target=irserve  --snapshot=verify
npx -y @fission-ai/openspec@latest validate --all --strict
```

Manual smoke for `public`:
```
mkdir tmp-test/site && echo "<p>hi</p>" > tmp-test/site/index.html
echo '{"public":"site"}' > tmp-test/serve.json
target/release/irserve tmp-test &
curl http://127.0.0.1:3000/    # → 200 + <p>hi</p>
```

Manual smoke for fatal cases:
```
target/release/irserve --config does-not-exist.json .   # non-zero exit
echo '{not json' > tmp-test/serve.json
target/release/irserve tmp-test                          # non-zero exit
```

## Process reminders (per kickoff template)

- Iterative green-state commits; ask user before each `git commit`.
- Codex review rounds: one commit per round titled
  `docs(stage-6a): address Codex review round N (P{priorities} fixes)`.
- Do not modify `third_party/`.
- Existing snapshots are not modified — only the three new ones are
  added.
- The contract changes only via D-009 (an `adapted` D-NNN amending
  D-008's deferred set); no MODIFIED spec delta in this change package.
