# Proposal: Load `serve.json` configuration

## Why

Stage 5b shipped a strict-L0 IrServe runtime that ignores all
configuration: `serve.json`, `now.json#now.static`, and
`package.json#static` are silently skipped (`D-008`). Every Stage-6
sub-stage past 6a — routing normalization, cleanUrls, configured
redirects, configured rewrites, custom headers, directory listing — is
parameterized by `serve.json` and cannot ship as observable behavior
without a loader in place.

This change introduces the configuration layer: the typed `ServeConfig`
struct, a `load_serve_json(served_dir, explicit_path)` function that
mirrors the SRV-CFG-001 lookup chain, the `-c/--config` CLI flag, the
`public` field applied to the served root, and three new probes
(`serve-json-public`, `config-missing-explicit`, `config-malformed`)
that exercise the only behavior surface 6a can probe in isolation.

## What

| SRV | Status before | Status after | Module |
|---|---|---|---|
| SRV-CFG-001 | verified (env-var alt-config redirect via ORC-006 only) | verified (oracle list grows: ORC-064, ORC-065, ORC-066) | `crates/irserve-core/src/config.rs`; bin wiring in `crates/irserve/src/main.rs` |
| SRV-CFG-002 | accepted (meta) | accepted (meta) | parsed by `ServeConfig` deserializer; per-field semantics deferred to 6b–6g |

The change carries one MODIFIED delta on SRV-CFG-001
(`specs/config/spec.md`) extending the `Evidence:` oracle list from
`ORC-006` to `ORC-006, ORC-064, ORC-065, ORC-066` and adding an
`Implementation:` paragraph pointing at `crates/irserve-core/src/
config.rs` and the bin's wiring. The behavioral text and the five
Scenarios are preserved verbatim — no requirement promotion, no
contract narrowing or widening. SRV-CFG-002's schema map is unchanged
(no delta).

## Scope

### In scope

- `crates/irserve-core/src/config.rs` — `ServeConfig` struct (11
  fields) with `serde(deny_unknown_fields, rename_all = "camelCase")`,
  untagged `BoolOrGlobs` enum for `bool | string[]` shapes, typed
  `RewriteRule` / `RedirectRule` / `HeaderRule` / `HeaderItem`, and
  `load_serve_json(served_dir, explicit_path)` implementing the
  SRV-CFG-001 lookup chain with deprecation handling.
- `crates/irserve-core/src/lib.rs` — re-exports of the public surface;
  `ServerConfig` widened with `serve_config: ServeConfig`.
- `crates/irserve/src/main.rs` — clap `-c/--config <PATH>`; calls
  `load_serve_json` before `canonicalize`; emits a stderr deprecation
  warning when source is `now.json` or `package.json`; folds
  `serve_config.public` into the served-root computation; non-zero
  exit on fatal config errors via `Box<dyn Error>` propagation.
- `tools/probe/run.mjs` — drops `-c, --config` from `L0_DEFERRED_FLAGS`.
- New probe `tools/probe/cases/serve-json-public.json` (HTTP-mode):
  `serve.json: {"public": "site"}` re-roots; `GET /` resolves to
  `site/index.html`. Backs ORC-064.
- New probe `tools/probe/cases/config-missing-explicit.json`
  (CLI-mode): `--config does-not-exist.json` exits non-zero. Backs
  ORC-065.
- New probe `tools/probe/cases/config-malformed.json` (CLI-mode):
  `serve.json: "{not json"` exits non-zero. Backs ORC-066.
- 20 unit tests in `crates/irserve-core/src/config.rs` covering: lookup
  chain, deprecated-source extraction, missing top-level `now` key
  fall-through (D-004 micro-divergence), malformed JSON, non-object
  root, unknown-field rejection, untagged-enum decoding, absolute /
  relative explicit-path resolution.
- Research-track edits: ORC-064 / ORC-065 / ORC-066 rows in
  `oracle-matrix.md`; SRV-CFG-001 oracle list extended in
  `inventory.md`; D-009 added in `decisions.md` amending D-008's
  deferred set (drops SRV-CFG-001, SRV-CFG-002).
- README stage map row 6a → done.

### Out of scope

- Per-field behavior beyond `public` (cleanUrls, trailingSlash,
  rewrites, redirects, headers, directoryListing, unlisted,
  renderSingle, symlinks, etag): parsed into `ServeConfig` but not
  consumed by the dispatcher; sub-stages 6b–6g wire each one.
- The existing probe `config-explicit.json` whose only anchor asserts
  a redirect: stays without a `runner.l0` block, so the runner
  auto-skips it under `target=irserve` until 6d (configured-redirects)
  adds the block. A `TODO(6d)` comment lives in the case description.
- Q-003 (exact validation error format and exit codes): stays open;
  D-002 already excludes byte-for-byte AJV-message parity. The probe
  asserts only `exit code = non-zero` and `stderr kind = text`.
- Bug-for-bug parity with the reference's AJV error messages. The
  serde-shaped messages produced by `deny_unknown_fields` are
  acceptable per D-002.
- `--single` SPA flag (stays in `L0_DEFERRED_FLAGS`; lands in 6e).
- All other deferred CLI flags (`-C`/`--cors`, `-d`/`--debug`,
  `-L`/`--no-request-logging`, `-p`, `--no-port-switching`).
- Any change to `third_party/`. No edits to existing snapshots beyond
  `tools/probe/snapshots/config-explicit.json`, which is refreshed
  to absorb the metadata-only `description` change (TODO(6d) note);
  reference behavior is unchanged.

## Findings (methodological signals)

One real divergence surfaced during slice 1 unit-test design and was
mediated as a **D-004 micro-divergence**, not a contract edit.

1. **`now.json` without a top-level `now` key.** The reference's
   loader (`config.ts:88-95`) reads `parsedJson.now.static`, which
   throws `TypeError: Cannot read properties of undefined` when
   `now` is missing. This is an upstream accident, not a contractual
   surface. IrServe's `load_serve_json` skips cleanly to the next
   file in the chain. D-004 ("no bug-for-bug parity with `serve`")
   authorizes the divergence; no new D-NNN. Documented in
   `design.md` § Findings.

No contract edits were required. Q-003 stays open per D-002.

## Risks and mitigations

1. **Windows path joining for `public`.** `cli.directory.join(public)
   .canonicalize()` may fail when `cli.directory` is relative. The
   bin canonicalizes the joined path; per-platform behavior is
   exercised by `cargo test --test oracle` on the developer's
   Windows 11 machine.
2. **`exitCodeMayDiffer` precaution on the two CLI probes.** The
   reference and IrServe both exit 1 for the missing-explicit and
   malformed-JSON cases (manually smoked), but exact exit codes are
   not specified by SRV-CFG-001 ("non-zero exit code"). The probes
   use `exitCodeMayDiffer` to assert non-zero on both sides without
   pinning the integer.
3. **`deny_unknown_fields` interaction with untagged enums.** serde
   does not enforce `deny_unknown_fields` uniformly inside untagged
   variants. Mitigation: explicit unit test
   `clean_urls_object_is_rejected` ensures `{cleanUrls: {foo: 1}}`
   does not silently match either variant of `BoolOrGlobs`.
4. **`config-explicit.json` auto-skip lifetime.** The case has no
   `runner.l0` block, so the runner auto-skips it under
   `target=irserve` (`run.mjs:872-874`). It will be unblocked when
   6d (configured redirects) adds the block. A `TODO(6d)` note in
   the case description marks this dependency.
5. **`etag` field accepted over reference's AJV omission.** The
   reference's schema does not list `etag` (it is set programmatically
   from `--no-etag`). The spec map (`config/spec.md:71`) explicitly
   includes it; we honor the spec over the reference quirk per D-004.
   No probe asserts on this in 6a; cache work is L3 (Stage 7+).

## Open assumptions

- **A1 — Reference and IrServe both exit 1 on fatal config errors.**
  Smoked manually during slice 2; the probe runner records the actual
  exit codes against snapshots. If a future reference upgrade changes
  the integer, `exitCodeMayDiffer` already absorbs it.
- **A2 — `serve.json` `public` is resolved relative to the served
  directory regardless of where the config file lives.** Mirrors
  reference `config.ts:113-119`. The bin computes
  `cli.directory.join(public_segment)` before canonicalizing.
