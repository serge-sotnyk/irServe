# Design: Load `serve.json` configuration

This design is **code-level**. The architectural foundations (crate
layout, HTTP-stack pins, request-lifecycle order, oracle harness layer)
live in `openspec/changes/001-port-minimal-static-server/design.md`
(§§1, 4, 6). The contract for the capability lives in
`openspec/specs/config/spec.md` (SRV-CFG-001, SRV-CFG-002).

## 1. Module map (deltas to the Stage-5b layout)

| Module | Delta | Wires |
|---|---|---|
| `crates/irserve-core/src/config.rs` | **NEW.** `ServeConfig` struct, `BoolOrGlobs` / `RewriteRule` / `RedirectRule` / `HeaderRule` / `HeaderItem` types, `load_serve_json()`, `LoadedConfig`, `ConfigSource`. 20 in-module unit tests. | n/a |
| `crates/irserve-core/src/lib.rs` | `pub mod config;` and re-exports. `ServerConfig` widened with `serve_config: ServeConfig`. | n/a |
| `crates/irserve/src/main.rs` | Clap `-c/--config <PATH>`. Calls `load_serve_json` after CLI parse, before `canonicalize`. Emits stderr deprecation warning on `now.json` / `package.json` source. Folds `serve_config.public` into served-root computation. | startup |
| `tools/probe/run.mjs` | `L0_DEFERRED_FLAGS` no longer contains `-c`, `--config`. | runner |

The dispatcher (`dispatch.rs`, `resolve.rs`, `mime.rs`,
`notfound.rs`) is untouched; `ServeConfig`'s non-`public` fields are
parsed but unused in 6a. Sub-stages 6b–6g wire them.

## 2. Lookup algorithm

```
load_serve_json(served_dir, explicit_path) -> Result<Option<LoadedConfig>>
  if explicit_path is Some(rel):
    path = rel.is_absolute() ? rel : served_dir.join(rel)
    if !path.exists():
      return Err(ExplicitMissing(path))
    value = read_json(path)?
    config = parse_serve_section(path, value)?
    return Ok(Some({ config, source: Explicit }))

  for (name, source) in [(serve.json, ServeJson),
                         (now.json,   NowJson),
                         (package.json, PackageJson)]:
    path = served_dir.join(name)
    if !path.exists(): continue
    value = read_json(path)?
    section = match source:
      ServeJson | Explicit  -> Some(value)
      NowJson               -> extract_nested(value, ["now", "static"])
      PackageJson           -> extract_nested(value, ["static"])
    if section is None: continue
    config = parse_serve_section(path, section)?
    return Ok(Some({ config, source }))

  return Ok(None)
```

`parse_serve_section` enforces "non-object root → fatal" before
deserializing, so `[]` or `"foo"` returns `ConfigError::NotObject`.
serde with `deny_unknown_fields + rename_all="camelCase"` does the
rest.

## 3. Public field application (bin)

```rust
let loaded = load_serve_json(&cli.directory, cli.config.as_deref())?;
// (deprecation warning on stderr if loaded.source matches NowJson | PackageJson)
let serve_config = loaded.map(|l| l.config).unwrap_or_default();

let public_segment = serve_config.public.as_deref().unwrap_or(".");
let root = cli.directory.join(public_segment).canonicalize()?;
```

The bin does **not** containment-check `public`. The reference's
loader (`config.ts:113-119`) does not either; `public: ".."` is
permitted to escape the served directory. SRV-CFG-001 says only
"resolved relative to the served directory", which is satisfied by
the join. Phase-10 containment in `resolve.rs` still protects against
**URL** escapes.

## 4. Schema validation: serde vs. AJV

The reference uses AJV with a JSON Schema at
`@zeit/schemas/deployment/config-static.js` (`additionalProperties:
false`). IrServe uses `serde` with `#[serde(deny_unknown_fields)]`.

Justification:

- `D-002` excludes byte-for-byte parity with reference error messages.
  The serde-shaped messages produced by `deny_unknown_fields` and
  `untagged` decoding are acceptable.
- Q-003 stays open on "exact validation error format and exit codes".
  The probe (`config-malformed`) asserts only `exit = non-zero` and
  `stderr kind = text`, never the wording.
- Adding the `jsonschema` crate would mean defining the schema twice
  (JSON Schema for validation + Rust struct for use), and would not
  close Q-003.

`untagged` enum caveat: serde does not enforce `deny_unknown_fields`
inside untagged variants uniformly. Mitigated by an explicit unit test
(`clean_urls_object_is_rejected`) ensuring the `BoolOrGlobs` enum
rejects object shapes.

## 5. Findings — `now.json` without top-level `now`

The reference's loader (`config.ts:88-95`) reads
`parsedJson.now.static`. When `now.json` exists but lacks a top-level
`now` key, JavaScript's lookup yields `undefined.static` → `TypeError:
Cannot read properties of undefined`, which crashes `serve` with a
non-zero exit.

This is an upstream accident, not a contractual surface. IrServe's
`extract_nested` returns `None` on missing keys, so the loop continues
to `package.json`. **D-004** ("no bug-for-bug parity") authorizes the
divergence; no new D-NNN entry. Documented inline in
`config.rs::tests::now_json_with_now_but_no_static_falls_through`.

## 6. Error type

`ConfigError` (in `config.rs`) is its own thiserror enum with five
variants: `ExplicitMissing(PathBuf)`, `Read { path, source }`,
`Json { path, source }`, `NotObject { path }`, `Schema { path, source
}`. The bin propagates via `Box<dyn Error>` from `#[tokio::main]` —
the same channel Stage 5b uses for `io::Error`. No promotion to a
`Config(...)` variant inside `irserve_core::Error` is needed because
the bin handles config errors before `run()` is called.

## 7. Probe wiring

`tools/probe/run.mjs:46-54` `L0_DEFERRED_FLAGS` drops `-c, --config`
in slice 3 (only after `public` is wired). The two CLI-mode probes
(`config-missing-explicit`, `config-malformed`) use the existing
`runCliInvocation` path with `cwd: fixtureDir`; the binary exits
before binding because `load_serve_json` runs on startup, ahead of
`TcpListener::bind`. No new runner machinery is needed.

`exitCodeMayDiffer` is set on both CLI probes as a precaution: the
spec text is "non-zero exit code", and pinning a specific integer
would couple us to either clap's or the reference's choice.

## 8. Methodological signals — none of these required edits

- The reference emits a deprecation warning on stderr for
  `now.json` / `package.json#static` sources. The probe runner does
  not assert stderr text in HTTP-mode (`run.mjs:984` is gated on
  `PROBE_VERBOSE`), and in CLI-mode the runner masks
  length/sha256/preview but not `kind`. None of the 6a probes
  fixture a `now.json` or `package.json#static`, so the deprecation
  path is not exercised here. A future probe targeting that path
  will need to either match `kind: text` or extend the masking
  surface.
- `etag` is accepted by `ServeConfig` but not present in the
  reference's AJV schema. The spec map at `spec.md:71` includes
  `etag` explicitly; we honor the spec, per D-004. No probe asserts
  on this in 6a.
