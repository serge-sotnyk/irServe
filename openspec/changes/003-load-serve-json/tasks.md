# Tasks

## 1. Slice 1 — types + parser (unit-tests only)

- [x] 1.1 `Cargo.toml` — add `serde = { version = "1", features =
  ["derive"] }` and `serde_json = "1"` to `[workspace.dependencies]`
- [x] 1.2 `crates/irserve-core/Cargo.toml` — pull `serde`,
  `serde_json` from workspace; add `tempfile = "3"` as
  `[dev-dependencies]`
- [x] 1.3 `crates/irserve-core/src/config.rs` — `ServeConfig`
  (11 fields, `deny_unknown_fields`, `rename_all = "camelCase"`),
  `BoolOrGlobs` (untagged), `RewriteRule`, `RedirectRule` (with
  optional `type` field renamed to `kind`), `HeaderRule`, `HeaderItem`
- [x] 1.4 `crates/irserve-core/src/config.rs` —
  `load_serve_json(served_dir, explicit_path)` implementing the
  SRV-CFG-001 lookup chain; returns `Result<Option<LoadedConfig>,
  ConfigError>` with `LoadedConfig { config, source: ConfigSource }`
- [x] 1.5 `crates/irserve-core/src/config.rs` — `ConfigError`
  thiserror enum with 5 variants (`ExplicitMissing`, `Read`, `Json`,
  `NotObject`, `Schema`)
- [x] 1.6 `crates/irserve-core/src/config.rs` — `#[cfg(test)] mod
  tests` with 20 unit tests covering: missing implicit, empty object,
  precedence (`serve.json` > `package.json`), `now.static` extraction,
  fall-through on missing `now.static` and missing top-level `now`,
  malformed JSON, non-object root, unknown-field rejection,
  `deny_unknown_fields` per-rule, `BoolOrGlobs` decoding both shapes,
  rejection of object shape for `BoolOrGlobs`, explicit absolute /
  relative paths, redirect-rule optional `type`, header nested shape,
  `etag` accepted
- [x] 1.7 `crates/irserve-core/src/lib.rs` — `pub mod config;` and
  re-exports for the public surface
- [x] 1.8 `crates/irserve-core/src/lib.rs` — `ServerConfig` widened
  with `serve_config: ServeConfig`
- [x] 1.9 `crates/irserve/src/main.rs` — `ServerConfig` constructor
  passes `ServeConfig::default()` (still no behavior change)
- [x] 1.10 Verify: `cargo build` clean; `cargo test -p irserve-core
  config::` 20/20 green; `cargo test --test oracle` green (7 cases
  passed, 27 skipped, 0 failed; behavior unchanged)

## 2. Slice 2 — bin wires `--config`, fatal errors propagate

- [x] 2.1 `crates/irserve/src/main.rs` — clap field `#[arg(short =
  'c', long = "config", value_name = "PATH")] config: Option<PathBuf>`
- [x] 2.2 `crates/irserve/src/main.rs` — call `load_serve_json(&cli
  .directory, cli.config.as_deref())?` before `canonicalize`
- [x] 2.3 `crates/irserve/src/main.rs` — emit stderr deprecation
  warning when `loaded.source` is `NowJson` or `PackageJson`
- [x] 2.4 `crates/irserve/src/main.rs` — populate
  `ServerConfig.serve_config` from `loaded.map(|l| l.config)
  .unwrap_or_default()`
- [x] 2.5 Do NOT modify `tools/probe/run.mjs` `L0_DEFERRED_FLAGS`
  in this slice — `--config` would silently no-op on `public`,
  violating the runner's refuse-rather-than-silently-drop contract
- [x] 2.6 Verify: `cargo build` clean; `cargo test --test oracle`
  green (7/27/0); manual smokes — `target/debug/irserve --config
  does-not-exist.json .` exits 1; malformed `serve.json` exits 1

## 3. Slice 3 — `public` field + new probes + flag un-defer

- [x] 3.1 `crates/irserve/src/main.rs` — compute effective root as
  `cli.directory.join(serve_config.public.as_deref().unwrap_or("."))
  .canonicalize()?`
- [x] 3.2 `tools/probe/run.mjs` — drop `-c`, `--config` from
  `L0_DEFERRED_FLAGS`; update inline comment
- [x] 3.3 `tools/probe/cases/config-explicit.json` — append
  `TODO(6d)` note to the `description` field; no `runner.l0` block
  (auto-skip stays in effect until 6d)
- [x] 3.4 `tools/probe/cases/serve-json-public.json` — HTTP probe;
  `serve.json: {"public": "site"}`, fixture has `site/index.html` and
  a sibling `outside.html`; `GET /` → 200 + `<p>public site</p>\n`;
  `runner.l0.clean: ["root_serves_public_index"]`
- [x] 3.5 `tools/probe/cases/config-missing-explicit.json` — CLI
  probe; `args: ["--config", "does-not-exist.json", "."]`; no fixture;
  `runner.l0.clean + exitCodeMayDiffer: ["missing_explicit_fatal"]`
- [x] 3.6 `tools/probe/cases/config-malformed.json` — CLI probe;
  fixture `serve.json: "{not json"` (raw, not via `serveJson` field);
  `args: ["."]`; `runner.l0.clean + exitCodeMayDiffer:
  ["malformed_json_fatal"]`
- [x] 3.7 Snapshots — record via `node tools/probe/run.mjs <id>
  --target=reference --snapshot=update` for each of the three new
  cases. Verify reference still verifies all 37 cases.
- [x] 3.8 `tools/probe/snapshots/config-explicit.json` — refresh
  via `--snapshot=update --target=reference` to absorb the
  description-only metadata change. No reference behavior changed.
- [x] 3.9 Verify: `cargo test --test oracle` (37 cases total, 10
  passed, 27 skipped, 0 failed); `node tools/probe/run.mjs --all
  --target=reference --snapshot=verify` (37/37)

## 4. Slice 4 — change package, research-track edits, README, validate

- [x] 4.1 `openspec/changes/003-load-serve-json/proposal.md`
- [x] 4.2 `openspec/changes/003-load-serve-json/design.md`
- [x] 4.3 `openspec/changes/003-load-serve-json/tasks.md` (this
  document)
- [x] 4.4 `docs/reference/serve/inventory.md` — extend SRV-CFG-001
  oracle list with ORC-064, ORC-065, ORC-066
- [x] 4.5 `docs/reference/serve/oracle-matrix.md` — three new rows in
  the L1 section: ORC-064 `serve-json-public#root_serves_public_index`,
  ORC-065 `config-missing-explicit#missing_explicit_fatal`, ORC-066
  `config-malformed#malformed_json_fatal`
- [x] 4.6 `docs/reference/serve/decisions.md` — append D-009
  amending D-008's deferred-SRV list (drops SRV-CFG-001, SRV-CFG-002)
- [x] 4.7 `README.md` — Status block: add `Stage 6a — \`serve.json\`
  loader. Done.`; Stage map row 6a `todo` → `done`
- [x] 4.8 Validate: `npx -y @fission-ai/openspec@latest validate
  --all --strict` exit 0 (11/11)
- [x] 4.9 Validate: `cargo test --test oracle` green
- [x] 4.10 Validate: `cargo build --release` clean
- [x] 4.11 Validate: `git diff --stat` confirms zero `third_party/`
  edits and only metadata-only edits to existing snapshots
  (`config-explicit.json`)

## 5. Spec delta

- [x] 5.1 `openspec/changes/003-load-serve-json/specs/config/spec.md`
  — single MODIFIED for SRV-CFG-001 extending the `Evidence:` oracle
  list (`ORC-006` → `ORC-006, ORC-064, ORC-065, ORC-066`) and adding
  an `Implementation:` paragraph. Behavioral text and the five
  Scenarios are preserved verbatim. SRV-CFG-002's schema map is
  unchanged.
