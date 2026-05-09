# Delta for config

## MODIFIED Requirements

### Requirement: Configuration file lookup, location, and error handling

Configuration SHALL be loaded from the served directory. The lookup
order SHALL be: `--config <path>` (if given) → `serve.json` →
`now.json` (deprecated, key `now.static`) → `package.json` (deprecated,
key `static`). The first existing file with a usable section SHALL win;
the rest SHALL be ignored. Missing implicit files SHALL be silently
skipped. A missing `--config` file SHALL be a fatal error. Invalid JSON
or non-object content SHALL be a fatal error. The `public` field SHALL
be resolved relative to the served directory.

Evidence: SRV-CFG-001 (status: verified, level: L1); oracle: ORC-006
(`cases/config-explicit.json#redirect_from_alternate_config`),
ORC-064 (`cases/serve-json-public.json#root_serves_public_index`,
`public` field scenario), ORC-065 (`cases/config-missing-explicit.json
#missing_explicit_fatal`, missing-explicit scenario), ORC-066
(`cases/config-malformed.json#malformed_json_fatal`, malformed-JSON
scenario).

Implementation: Landed by change `003-load-serve-json` (Stage 6a).
The loader lives in `crates/irserve-core/src/config.rs`
(`load_serve_json`, `ServeConfig`, `ConfigError`). The bin
(`crates/irserve/src/main.rs`) wires the `-c/--config <PATH>` flag,
calls `load_serve_json` before `canonicalize`, emits a stderr
deprecation warning when the source is `now.json` or `package.json
#static`, and folds `serve_config.public` into the served-root
computation. Schema validation uses `serde` with
`#[serde(deny_unknown_fields)]`; per `D-002`, validation error
wording is implementation-defined (Q-003 stays open).

Note: Q-003 is open on the exact validation error format and exit
codes (deferred to oracle harness work). IrServe MAY emit a deprecation
warning when `now.json` or the `package.json#static` paths are used.
Per D-004, IrServe falls through cleanly when `now.json` exists but
lacks a top-level `now` key, where the reference would crash with a
`TypeError`.

#### Scenario: No config files exist

- GIVEN no `serve.json`, `now.json`, or `package.json#static` in the served directory
- WHEN the server starts
- THEN startup succeeds and defaults apply

#### Scenario: `serve.json` wins over `package.json`

- GIVEN both `serve.json` and `package.json` (with `static` key) in the same directory
- WHEN the server starts
- THEN the contents of `serve.json` are used and `package.json` is ignored

#### Scenario: Malformed JSON is fatal

- GIVEN `serve.json` with malformed JSON
- WHEN the server starts
- THEN startup fails with a non-zero exit code

#### Scenario: Missing `--config` file is fatal

- GIVEN `--config ./does-not-exist.json`
- WHEN the server starts
- THEN startup fails with a non-zero exit code

#### Scenario: Public field resolves relative to served directory

- GIVEN `serve.json` with `{ "public": "site" }` in served directory `D`
- WHEN the server starts
- THEN files are served from `D/site`
