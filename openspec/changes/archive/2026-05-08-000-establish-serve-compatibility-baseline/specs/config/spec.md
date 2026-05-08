# Delta for config

## ADDED Requirements

### Requirement: Configuration file lookup, location, and error handling

Configuration SHALL be loaded from the served directory. The lookup
order SHALL be: `--config <path>` (if given) → `serve.json` →
`now.json` (deprecated, key `now.static`) → `package.json` (deprecated,
key `static`). The first existing file with a usable section SHALL win;
the rest SHALL be ignored. Missing implicit files SHALL be silently
skipped. A missing `--config` file SHALL be a fatal error. Invalid JSON
or non-object content SHALL be a fatal error. The `public` field SHALL
be resolved relative to the served directory.

Evidence: SRV-CFG-001 (status: verified, level: L1); oracle: ORC-006.

Note: Q-003 is open on the exact validation error format and exit codes
(deferred to oracle harness work). IrServe MAY emit a deprecation
warning when `now.json` or the `package.json#static` paths are used.

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

### Requirement: Configuration schema overview

The configuration object SHALL accept the following fields. Field
semantics live in the per-area Requirements referenced in the table;
this Requirement only fixes the schema map.

| Field | Type | Default | Behavior |
|---|---|---|---|
| `public` | string | served directory | resolved during config load (see "Configuration file lookup") |
| `cleanUrls` | boolean \| string[] (globs) | `true` (CLI default) | routing capability (cleanUrls 301 / extensionless resolution) |
| `trailingSlash` | boolean \| undefined | `undefined` | routing capability (trailingSlash add/strip) |
| `rewrites` | `{source, destination}[]` | `[]` | rewrites capability |
| `redirects` | `{source, destination, type?}[]` | `[]` | redirects capability |
| `headers` | `{source, headers: {key, value}[]}[]` | `[]` | headers capability (L3, not in this baseline) |
| `directoryListing` | boolean \| string[] (globs) | `true` | directory-listing capability |
| `unlisted` | string[] (globs) | `['.DS_Store', '.git']` baseline | directory-listing capability |
| `renderSingle` | boolean | `false` | directory-listing capability |
| `symlinks` | boolean | `false` | symlinks capability (L4, not in this baseline) |
| `etag` | boolean | `true` (CLI default; library default is `false`) | http-cache capability (L3, not in this baseline) |

IrServe SHALL accept the same field names. Unknown fields MAY be
rejected at startup.

Evidence: SRV-CFG-002 (status: accepted, level: L1); oracle: no probe (meta-requirement; verified piecewise via per-field SRVs).

Note: This is a META requirement; no single observable behavior is
asserted. Field-by-field behavior is verified by the per-area
Requirements (cleanUrls, trailingSlash, redirects, rewrites, etc.).
The status remains `accepted` because no single oracle test exercises
the schema as a whole.

#### Scenario: Known fields are accepted

- GIVEN a `serve.json` containing any combination of the fields named in the schema map
- WHEN the server starts
- THEN startup succeeds and each field's behavior is governed by its per-area Requirement

#### Scenario: Public field resolves relative to served directory

- GIVEN `serve.json` with `{ "public": "site" }` in served directory `D`
- WHEN the server starts
- THEN files are served from `D/site`
