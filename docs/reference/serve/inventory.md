# Serve reverse-requirements inventory

Working document for Stage 1: reverse-engineering of `vercel/serve` and `vercel/serve-handler` observable behavior.

> **This file is not the source of truth.** Accepted requirements are migrated into OpenSpec specs in Stage 4. Until then, every entry here is a candidate, draft, or note.

## Format

Each requirement uses the following template:

```
## SRV-<AREA>-<NNN>: <short title>

Status: accepted | accepted | verified | adapted | deferred | rejected | unknown
Area: cli | config | static-files | routing | redirects | rewrites | headers | directory-listing | http-cache | security | symlinks | cors | windows
Compatibility level: 0 | 1 | 2 | 3 | 4
Priority: P0 | P1 | P2

Reference source:
- README: yes | partial | absent — <which README, which section>
- serve-handler source: src/index.js:<line-range> (when applicable)
- Existing test: <"test name"> in test/<file>
- Probe: tools/probe/cases/<id>.json
- Oracle test: planned

Requirement (draft):
<observable behavior, no implementation details>

Scenarios:
- GIVEN ...
  WHEN ...
  THEN ...

Compatibility notes:
- ...

Open questions:
- ...
```

## Status taxonomy

| Status | Meaning |
|---|---|
| `candidate` | Found in README/source/test, not yet validated. |
| `accepted` | Confirmed as a target requirement for IrServe. |
| `verified` | Has a passing oracle test against the reference. |
| `adapted` | Behavior intentionally diverges from `serve`; logged in `decisions.md`. |
| `deferred` | Postponed to a higher compatibility level or post-MVP. |
| `rejected` | Will not be supported; logged in `decisions.md`. |
| `unknown` | Behavior is unclear; logged in `open-questions.md` until resolved. |

## Reference snapshot

- `serve` version: `serve@14.2.6` (submodule SHA `f3c702c6bb312c6d6b05315721a6d3ea245d86b8`).
- `serve-handler` version: `serve-handler@6.1.7` (submodule SHA `5158ae776863f0d597187e11260d963e7a78c6a0`).
- Date of inventory pass: 2026-05-07.
- Out-of-scope MVP areas (Node middleware API, exact terminal output, exact directory-listing markup, bug-for-bug parity) are recorded in `decisions.md` rather than as SRVs. Behavior that this pass could not pin down is captured in `open-questions.md` with `Q-<NNN>` ids and back-referenced from any SRV with `Status: unknown`.

## Entries

### CLI

#### SRV-CLI-001: Listen on default port 3000 on all interfaces

Status: verified
Area: cli
Compatibility level: 0
Priority: P0

Reference source:
- README: yes — `third_party/serve/readme.md`, the help-text excerpt and the smoke example.
- serve source (CLI flag enumeration): `third_party/serve/source/main.ts:55-56` — when no `--listen` is provided the default endpoint is `{ port: parseInt(env.PORT ?? '3000', 10) }`.
- Existing test: absent.
- Probe: `tools/probe/cases/default-port-l0.json` (env-var scenario; backs ORC-063). The no-flag-no-env scenario remains source-evidence only because port 3000 cannot be reliably reserved on developer machines.
- Oracle test: ORC-063.

Requirement (draft):
With no `--listen`/`-l` and no `PORT` env var, the server binds TCP port `3000`. With the `PORT` environment variable set, the value of `PORT` is used in place of `3000`.

Scenarios:
- GIVEN no `--listen` flag and unset `PORT`.
  WHEN the server starts.
  THEN it listens on port 3000.
- GIVEN `PORT=4567` in the environment and no `--listen`.
  WHEN the server starts.
  THEN it listens on port 4567.

Compatibility notes:
- The bind hostname is whatever Node's `server.listen(port)` chooses (per `source/utilities/server.ts`, only the port is passed when no host is given). Documenting the exact wildcard interface is L4 and out of MVP scope.

Open questions:
- None.

#### SRV-CLI-002: `-l <port>` accepts a bare port number

Status: verified
Area: cli
Compatibility level: 0
Priority: P0

Reference source:
- README: yes — `third_party/serve/readme.md` Usage / help text mentions `-l listen_uri`.
- serve source: `third_party/serve/source/main.ts` references `parseEndpoint`; numeric branch returns `{ port }`.
- Existing test: absent (covered transitively by `tools/probe/cases/_smoke.json`).
- Probe: `tools/probe/cases/_smoke.json` (run with `--listen <port>`).
- Oracle test: ORC-001 (snapshots in tools/probe/snapshots/).

Requirement (draft):
The `-l`/`--listen` flag accepts a numeric port. The server then listens on TCP port _N_ on the default interface.

Scenarios:
- GIVEN `serve -l 3010` started in a directory.
  WHEN a client opens `http://127.0.0.1:3010/`.
  THEN it reaches the server.

Compatibility notes:
- Multiple `-l` flags are additive (per help text "more than one may be specified").

Open questions:
- None.

#### SRV-CLI-003: `-l tcp://host:port` accepts a TCP URI

Status: accepted
Area: cli
Compatibility level: 1
Priority: P1

Reference source:
- README: yes — help text `ENDPOINTS` section: `serve -l tcp://hostname:1234`.
- serve source: CLI flag enumeration in `source/main.ts` (parseEndpoint dispatch by URL protocol).
- Existing test: absent.
- Probe: not run for this pass.
- Oracle test: planned.

Requirement (draft):
The `-l`/`--listen` flag accepts a `tcp://host:port` URI. The server binds the given TCP port on the given host.

Scenarios:
- GIVEN `serve -l tcp://127.0.0.1:3011`.
  WHEN a client opens `http://127.0.0.1:3011/`.
  THEN it reaches the server.

Compatibility notes:
- Hostname defaults to `localhost` and port to `3000` if either is missing in the URI (per source).

Open questions:
- Q-001 (whether port-defaulting on a URI without `:port` is observable in 14.2.6 builds).

#### SRV-CLI-004: `-l unix:/path/to/socket` accepts a UNIX domain socket

Status: deferred
Area: cli
Compatibility level: 4
Priority: P2

Reference source:
- README: yes — help text `ENDPOINTS` section.
- serve source: CLI flag enumeration in `source/main.ts`.
- Existing test: absent.
- Probe: not run (Windows-first developer environment; UDS is Unix-only).
- Oracle test: planned.

Requirement (draft):
The `-l`/`--listen` flag accepts a `unix:<path>` URI. The server listens on the given UNIX domain socket path.

Scenarios:
- GIVEN `serve -l unix:/tmp/serve.sock` on Linux.
  WHEN a client connects via the socket.
  THEN it reaches the server.

Compatibility notes:
- Deferred to L4. Cross-platform parity (UDS on Windows? named pipes on Linux?) is post-MVP.

Open questions:
- None.

#### SRV-CLI-005: `-l pipe:\\.\pipe\Name` accepts a Windows named pipe

Status: deferred
Area: cli
Compatibility level: 4
Priority: P2

Reference source:
- README: yes — help text `ENDPOINTS` section.
- serve source: CLI flag enumeration in `source/main.ts`.
- Existing test: absent.
- Probe: not run.
- Oracle test: planned.

Requirement (draft):
The `-l`/`--listen` flag accepts a `pipe:\\.\pipe\Name` URI on Windows. The server listens on the named pipe.

Compatibility notes:
- Deferred to L4.

Open questions:
- None.

#### SRV-CLI-006: `-p` is a deprecated alias for `--listen`

Status: accepted
Area: cli
Compatibility level: 1
Priority: P2

Reference source:
- README: partial — help text shows `-p` as "Specify custom port" but does not flag it as deprecated.
- serve source: `third_party/serve/source/main.ts` (CLI flag enumeration; comment in source notes `-p` is kept for backwards compatibility).
- Existing test: absent.
- Probe: not run.
- Oracle test: planned.

Requirement (draft):
The `-p` flag is accepted as an alias for `-l`/`--listen` and parses the same value forms.

Compatibility notes:
- Marked deprecated in source comments. IrServe MAY accept it without further documentation.

Open questions:
- None.

#### SRV-CLI-007: `<directory>` positional argument selects the directory to serve

Status: verified
Area: cli
Compatibility level: 0
Priority: P0

Reference source:
- README: yes — `third_party/serve/readme.md` Usage section: `serve folder-name/`.
- serve source: `third_party/serve/source/main.ts:58-65` — at most one positional, resolved relative to cwd.
- Existing test: absent.
- Probe: covered indirectly by every probe (the runner passes a fixture directory). Two-positional-error scenario is exercised by `tools/probe/cases/cli-positional-error.json`.
- Oracle test: ORC-001 (scenarios 1-2), ORC-062 (scenario 3) (snapshots in tools/probe/snapshots/).

Requirement (draft):
A single optional positional argument selects the directory to serve. If omitted, the current working directory is served. Supplying more than one positional argument is a fatal error.

Scenarios:
- GIVEN `serve` invoked in directory `D`.
  WHEN no positional arg is given.
  THEN files served are resolved relative to `D`.
- GIVEN `serve ./public`.
  WHEN started.
  THEN files served are resolved relative to `<cwd>/public`.
- GIVEN `serve a b`.
  WHEN started.
  THEN the process exits with a non-zero code and an error message.

Compatibility notes:
- Path is resolved with `path.resolve` in source, so absolute paths are accepted.

Open questions:
- None.

#### SRV-CLI-008: `-s`/`--single` rewrites all not-found requests to `/index.html`

Status: verified
Area: cli
Compatibility level: 2
Priority: P0

Reference source:
- README: yes — help text: `Rewrite all not-found requests to 'index.html'`.
- serve source: `third_party/serve/source/main.ts:78-90` — prepends a `**` rewrite to `/index.html`.
- Existing test: absent (the underlying rewrite mechanism is covered by `set 'rewrites' config property to wildcard path` in `test/integration.test.js`).
- Probe: `tools/probe/cases/rewrites-segment.json` (validates the SPA pattern under serve.json; CLI form is equivalent).
- Oracle test: ORC-029 (snapshots in tools/probe/snapshots/).

Requirement (draft):
With `-s`/`--single`, every request whose path does not resolve to a file is served the contents of `/index.html` (status 200, the file's MIME type). It is implemented as a high-priority rewrite and therefore is overridden by an earlier-matching redirect.

Scenarios:
- GIVEN `serve --single` over a directory containing `index.html`.
  WHEN `GET /no/such/route`.
  THEN status is 200 and body is the contents of `index.html`.
- GIVEN `serve --single` and a `serve.json` with a redirect from `/old` to `/new`.
  WHEN `GET /old`.
  THEN status is 301 (redirect, not the SPA fallback).

Compatibility notes:
- Not a redirect — rewrites are silent; the URL bar does not change.
- See SRV-ROUT-006 for how the SPA rewrite interacts with redirects, cleanUrls, trailingSlash, and existing static files in the request pipeline.

Open questions:
- None.

#### SRV-CLI-009: `-c <path>`/`--config` selects a custom configuration file

Status: verified
Area: cli
Compatibility level: 1
Priority: P1

Reference source:
- README: yes — help text: `Specify custom path to 'serve.json'`.
- serve source: `third_party/serve/source/main.ts` (flag) and `source/utilities/config.ts:31-32` — `--config` is unshifted to the head of the search list.
- Existing test: absent.
- Probe: `tools/probe/cases/config-explicit-public.json` (positive path under `target=irserve` since Stage 6a).
- Oracle test: ORC-006, ORC-067 (snapshots in tools/probe/snapshots/).

Requirement (draft):
With `-c <path>`/`--config <path>`, that file is read first as the configuration source. If it cannot be read, startup fails with an error (i.e. the missing-config-file is fatal in this case, unlike the implicit `serve.json`).

Scenarios:
- GIVEN `serve -c ./missing.json`.
  WHEN started.
  THEN the process exits with a non-zero code.
- GIVEN `serve -c ./conf.json` and the file contains `{ "cleanUrls": false }`.
  WHEN started.
  THEN cleanUrls is disabled.

Compatibility notes:
- See `SRV-CFG-001` for the implicit lookup order when `-c` is not given.

Open questions:
- None.

#### SRV-CLI-010: `-C`/`--cors` enables permissive CORS headers

Status: verified
Area: cli
Compatibility level: 1
Priority: P1

Reference source:
- README: yes — help text: `Enable CORS, sets 'Access-Control-Allow-Origin' to '*'`.
- serve source (CLI flag enumeration): `source/main.ts` flag list.
- Existing test: absent.
- Probe: `tools/probe/cases/cors-applied.json`, `tools/probe/cases/cors-response-surface.json`.
- Oracle test: ORC-054, ORC-055 (snapshots in tools/probe/snapshots/).

Requirement (draft):
With `-C`/`--cors`, every response carries `Access-Control-Allow-Origin: *`, plus permissive `Access-Control-Allow-Headers`, `Access-Control-Allow-Credentials: true`, and `Access-Control-Allow-Private-Network: true` headers.

Scenarios:
- GIVEN `serve --cors`.
  WHEN any request is made.
  THEN the response includes `access-control-allow-origin: *`.

Compatibility notes:
- This SRV captures *flag presence* at L1 — i.e. the user-visible promise that `--cors` "turns CORS on". The full response-header surface is enumerated in SRV-CORS-001 at L3.
- The runner's response-header allowlist now includes `access-control-allow-*` (extended in stage 2), so probe outputs surface the full set directly.

Open questions:
- None.

#### SRV-CLI-011: `-n`/`--no-clipboard` suppresses clipboard side effect

Status: accepted
Area: cli
Compatibility level: 1
Priority: P2

Reference source:
- README: yes — help text: `Do not copy the local address to the clipboard`.
- serve source: `source/main.ts:102-140`.
- Existing test: absent.
- Probe: not applicable. The flag is passed by every probe so the runner's startup path is exercised, but the suppression effect (clipboard NOT modified) cannot be observed via HTTP and the runner does not assert clipboard state.
- Oracle test: not planned (clipboard interaction is intentionally not modeled per `D-005`).

Requirement (draft):
The flag is accepted (silently or with a no-op) so that scripts that pass `--no-clipboard` still work. IrServe does not interact with the clipboard at all by default.

Compatibility notes:
- See `D-005` (decisions.md): IrServe does not modify the clipboard. `-n`/`--no-clipboard` is accepted and ignored to remain CLI-compatible.

Open questions:
- None.

#### SRV-CLI-012: `-u`/`--no-compression` disables HTTP compression

Status: verified
Area: cli
Compatibility level: 3
Priority: P2

Reference source:
- README: yes — help text: `Do not compress files`.
- serve source: CLI flag enumeration; the gating happens in `source/utilities/server.ts` (compression middleware).
- Existing test: absent.
- Probe: `tools/probe/cases/compression-default.json` — with default settings, `vary: Accept-Encoding` is present on text responses, but the probe runner's `fetch` automatically decompresses, so the on-the-wire encoding cannot be observed directly.
- Oracle test: ORC-058 (snapshots in tools/probe/snapshots/).

Requirement (draft):
By default the server applies HTTP compression to text-typed responses for clients that send a compatible `Accept-Encoding`. With `--no-compression`, no compression is applied; the response is sent as-is.

Compatibility notes:
- The presence of `Vary: Accept-Encoding` is a stable signal in probe output even when the client transparently decodes the body.
- L3-priority. MVP MAY ship without compression and gain it via decision `D-006`.

Open questions:
- Q-002 (exact set of compressed content types and minimum body size threshold).

#### SRV-CLI-013: `--no-etag` switches default to `Last-Modified`

Status: accepted
Area: cli
Compatibility level: 3
Priority: P1

Reference source:
- README: yes — help text: `Send 'Last-Modified' header instead of 'ETag'`.
- serve source: `source/utilities/config.ts:140` — `config.etag = !args['--no-etag']` (i.e. ETag is on by default in the CLI even though the handler library defaults to off).
- Existing test: `automatically handle ETag headers for normal files` and `etag header is set` in `test/integration.test.js`.
- Probe: `tools/probe/cases/etag-roundtrip.json` (confirms ETag presence and 304 round-trip in default config).
- Oracle test: planned.

Requirement (draft):
By default the server emits a strong `ETag` on file responses. With `--no-etag`, no `ETag` is emitted and `Last-Modified` is sent instead.

Scenarios:
- GIVEN `serve` (defaults).
  WHEN `GET /file`.
  THEN the response includes an `ETag` header and no `Last-Modified` header.
- GIVEN `serve --no-etag`.
  WHEN `GET /file`.
  THEN the response includes a `Last-Modified` header and no `ETag`.

Compatibility notes:
- This default differs from raw `serve-handler` (which is `Last-Modified` by default). The CLI overrides the library default.

Open questions:
- None.

#### SRV-CLI-014: `-d`/`--debug` toggles verbose output

Status: adapted
Area: cli
Compatibility level: 1
Priority: P2

Reference source:
- README: yes — help text: `Show debugging information`.
- serve source: `source/main.ts:37-41`.
- Existing test: absent.
- Probe: not applicable.
- Oracle test: not planned (debug verbosity is terminal output, which is excluded by `D-002`).

Requirement (draft):
The flag is accepted. IrServe is free to map it to a verbosity level of its own choosing.

Compatibility notes:
- See `D-002` (terminal output not mirrored).

Open questions:
- None.

#### SRV-CLI-015: `-L`/`--no-request-logging` silences per-request logs

Status: adapted
Area: cli
Compatibility level: 1
Priority: P2

Reference source:
- README: yes — help text: `Do not log any request information to the console`.
- serve source: CLI flag enumeration in `source/main.ts`.
- Existing test: absent.
- Probe: not applicable.
- Oracle test: not planned.

Requirement (draft):
The flag is accepted. When set, IrServe MUST NOT emit per-request log lines to stdout/stderr. Exact log format when unset is not specified (see `D-002`).

Compatibility notes:
- L1 cosmetic; IrServe MAY default to silence and treat this flag as a no-op.

Open questions:
- None.

#### SRV-CLI-016: `--no-port-switching` disables fallback to a random port

Status: accepted
Area: cli
Compatibility level: 1
Priority: P1

Reference source:
- README: yes — help text: `Do not open a port other than the one specified when it's taken`.
- serve source: CLI flag enumeration in `source/main.ts`; the fallback logic lives in `source/utilities/server.ts:166-178`.
- Existing test: absent.
- Probe: not run. Every probe passes `--no-port-switching` and binds a free port, so only the happy path (the flag does not prevent startup) is exercised. The actual contract — failure-to-start when the port is occupied — needs a probe that occupies the port first; deferred to Stage 5b.
- Oracle test: planned.

Requirement (draft):
By default, if the requested port is already taken, the server picks a free port instead. With `--no-port-switching`, the server MUST fail to start instead.

Scenarios:
- GIVEN port 3000 is occupied and `serve --no-port-switching -l 3000`.
  WHEN started.
  THEN startup fails with a non-zero exit code.

Compatibility notes:
- Default port-switching behavior is observable; the wording of the warning is not in scope (see `D-002`).

Open questions:
- None.

#### SRV-CLI-017: `-S`/`--symlinks` resolves symlinks instead of 404

Status: deferred
Area: cli
Compatibility level: 4
Priority: P2

Reference source:
- README: yes — help text: `Resolve symlinks instead of showing 404 errors`.
- serve-handler source: `src/index.js:687-715` (default 404 path for symlinks; `realpath` resolution branch when enabled).
- Existing test: `symlinks should not work by default`, `allow symlinks by setting the option`, `A bad symlink should be a 404` in `test/integration.test.js`.
- Probe: not run.
- Oracle test: planned.

Requirement (draft):
By default, requests that resolve to a symlink return 404. With `-S`/`--symlinks`, the server follows the symlink and serves its target.

Compatibility notes:
- Deferred to L4. See `SRV-SYM-001`.

Open questions:
- None.

#### SRV-CLI-018: `--ssl-cert`, `--ssl-key`, `--ssl-pass` enable HTTPS

Status: deferred
Area: cli
Compatibility level: 4
Priority: P2

Reference source:
- README: yes — help text: `Optional path to an SSL/TLS certificate to serve with HTTPS` and the PEM/PKCS12 note.
- serve source: CLI flag enumeration in `source/main.ts`; SSL plumbing in `source/utilities/server.ts:96-117`.
- Existing test: absent.
- Probe: not run.
- Oracle test: planned.

Requirement (draft):
With `--ssl-cert <path>` and either `--ssl-key <path>` (PEM) or `.pfx`/`.p12` extension on the cert (PKCS12), the server listens via HTTPS instead of HTTP. `--ssl-pass <path>` supplies an optional passphrase file.

Compatibility notes:
- Deferred. MVP serves HTTP only.

Open questions:
- None.

#### SRV-CLI-019: `--help` and `-v`/`--version` exit cleanly

Status: verified
Area: cli
Compatibility level: 0
Priority: P1

Reference source:
- README: yes — help text top section.
- serve source: `source/main.ts:45-52`.
- Existing test: absent.
- Probe: `tools/probe/cases/cli-help-version.json` (CLI-mode case; runner branches into `runCliProbe` and snapshots exit code + stdout/stderr).
- Oracle test: ORC-060, ORC-061 (snapshots in tools/probe/snapshots/).

Requirement (draft):
`--help` prints help text and exits 0. `--version`/`-v` prints the version and exits 0. Each takes precedence over starting the server.

Compatibility notes:
- Exact help text formatting is excluded by `D-002`. The version string format is `MAJOR.MINOR.PATCH` (semver) and SHOULD be IrServe's own version.

Open questions:
- None.

### CFG

#### SRV-CFG-001: Configuration file lookup, location, and error handling

Status: verified
Area: config
Compatibility level: 1
Priority: P0

Reference source:
- README: yes — `third_party/serve/readme.md` Configuration section: "create a `serve.json` file in the public folder".
- serve source: `third_party/serve/source/utilities/config.ts:31-108`.
- Existing test: absent (CLI-level lookup) — the field-level behavior is covered per-area (e.g. `set 'cleanUrls' config property to 'true'`).
- Probe: indirect for most fields (per-area probes ship a `serve.json`); direct probes for the loader surface added in change `003-load-serve-json` (Stage 6a).
- Oracle test: ORC-006, ORC-064, ORC-065, ORC-066, ORC-067 (snapshots in tools/probe/snapshots/).

Requirement (draft):
Configuration is loaded from the served directory. The lookup order is: `--config <path>` (if given) → `serve.json` → `now.json` (deprecated, key `now.static`) → `package.json` (deprecated, key `static`). The first existing file with a usable section wins; the rest are ignored. Missing implicit files are silently skipped; a missing `--config` file is a fatal error. Invalid JSON or non-object content is a fatal error. The `public` field is resolved relative to the served directory.

Scenarios:
- GIVEN no config files exist.
  WHEN the server starts.
  THEN no error and defaults apply.
- GIVEN both `serve.json` and `package.json` (with `static` key) in the same directory.
  WHEN the server starts.
  THEN the contents of `serve.json` are used and `package.json` is ignored.
- GIVEN `serve.json` with malformed JSON.
  WHEN the server starts.
  THEN startup fails with a non-zero exit code.
- GIVEN `--config ./does-not-exist.json`.
  WHEN the server starts.
  THEN startup fails with a non-zero exit code.

Compatibility notes:
- The `now.json` and `package.json` paths are deprecated; IrServe MAY emit a deprecation warning when used.
- The schema is validated by AJV against `@zeit/schemas/deployment/config-static`. IrServe MAY use a structurally equivalent JSON Schema.

Open questions:
- Q-003 (exact validation error format and exit codes — deferred until oracle harness exists).

#### SRV-CFG-002: Configuration schema overview

Status: accepted
Area: config
Compatibility level: 1
Priority: P0

Reference source:
- README: yes — `third_party/serve-handler/README.md` Options table.
- serve-handler source: `src/index.js` (full handler reads each field).
- Existing test: per field — see the per-field SRVs.
- Probe: per field — see the per-field SRVs.
- Oracle test: planned.

Requirement (draft):
The configuration object accepts the following fields. Field semantics live in the area SRV referenced in the table; this SRV is purely the schema map.

| Field | Type | Default | Behavior SRV |
|---|---|---|---|
| `public` | string | served directory | (resolved during config load; see SRV-CFG-001) |
| `cleanUrls` | boolean \| string[] (globs) | `true` (CLI default) | SRV-ROUT-001, SRV-ROUT-002 |
| `trailingSlash` | boolean \| undefined | `undefined` | SRV-ROUT-003, SRV-ROUT-004 |
| `rewrites` | `{source, destination}[]` | `[]` | SRV-RWRT-001, SRV-RWRT-002 |
| `redirects` | `{source, destination, type?}[]` | `[]` | SRV-RDIR-001, SRV-RDIR-002 |
| `headers` | `{source, headers: {key, value}[]}[]` | `[]` | SRV-HDR-001, SRV-HDR-002 |
| `directoryListing` | boolean \| string[] (globs) | `true` | SRV-DLST-001 |
| `unlisted` | string[] (globs) | `['.DS_Store', '.git']` baseline | SRV-DLST-002 |
| `renderSingle` | boolean | `false` | SRV-DLST-003 |
| `symlinks` | boolean | `false` | SRV-SYM-001 |
| `etag` | boolean | `true` (CLI default; library default is `false`) | SRV-CACHE-001, SRV-CACHE-002 |

Compatibility notes:
- IrServe MUST accept the same field names. Unknown fields MAY be rejected at startup.
- The "CLI default" column reflects the values seen in `source/utilities/config.ts:140-142` (etag, symlinks).

Open questions:
- None.

### FILE

#### SRV-FILE-001: Serve regular files for matching paths

Status: verified
Area: static-files
Compatibility level: 0
Priority: P0

Reference source:
- README: yes — `third_party/serve/readme.md` Usage section.
- serve-handler source: `src/index.js:594-768` (file resolution → stream).
- Existing test: `render dotfile`, `render json file` in `test/integration.test.js`.
- Probe: `tools/probe/cases/_smoke.json`, `tools/probe/cases/mime-defaults.json`.
- Oracle test: ORC-001 (snapshots in tools/probe/snapshots/).

Requirement (draft):
A `GET` request whose URL path resolves (after redirect/rewrite resolution) to a regular file inside the served root returns status 200 with the file's content as the body and a `Content-Length` header equal to the file size.

Scenarios:
- GIVEN file `/data.json` exists.
  WHEN `GET /data.json`.
  THEN status is 200 and body is the byte-exact file content.

Compatibility notes:
- L0. Range, ETag, etc. are separate SRVs.

Open questions:
- None.

#### SRV-FILE-002: 404 for missing paths

Status: verified
Area: static-files
Compatibility level: 0
Priority: P0

Reference source:
- README: absent — implicit.
- serve-handler source: `src/index.js:687-694` (the `not_found` branch).
- Existing test: `receive not found error`, `receive not found error as json` in `test/integration.test.js`.
- Probe: `tools/probe/cases/notfound-shape.json`.
- Oracle test: ORC-004, ORC-005, ORC-051, ORC-052, ORC-059 (snapshots in tools/probe/snapshots/).

Requirement (draft):
When a request cannot be resolved to a file or a directory listing, the server responds with status `404`. When `Accept: application/json` is honored by content negotiation, the body is `{"error":{"code":"not_found","message":"The requested path could not be found"}}` with `Content-Type: application/json; charset=utf-8`. Otherwise, the body is an HTML error page with `Content-Type: text/html; charset=utf-8`.

Scenarios:
- GIVEN no file at `/missing`.
  WHEN `GET /missing`.
  THEN status is 404 and Content-Type is `text/html; charset=utf-8`.
- GIVEN no file at `/missing` and `Accept: application/json`.
  WHEN `GET /missing`.
  THEN status is 404, Content-Type is `application/json; charset=utf-8`, and body is the JSON error envelope.

Compatibility notes:
- Exact bytes of the HTML error body are not in scope (see `D-003`). The JSON envelope IS in scope.
- See `SRV-FILE-003` for `404.html` overrides.

Open questions:
- None.

#### SRV-FILE-003: Custom error pages via `<status>.html`

Status: verified
Area: static-files
Compatibility level: 1
Priority: P1

Reference source:
- README: yes — serve-handler README "Error templates" section.
- serve-handler source: `src/index.js:467-524`.
- Existing test: `receive custom 404.html error page`, `error is still sent back even if reading 404.html failed` in `test/integration.test.js`.
- Probe: `tools/probe/cases/notfound-custom.json`.
- Oracle test: ORC-007, ORC-059 (snapshots in tools/probe/snapshots/).

Requirement (draft):
If a file `<statusCode>.html` exists at the root of the served directory, it is sent as the body of the corresponding error response (preserving the error status code) for HTML-accept clients. JSON-accept clients still receive the JSON envelope from `SRV-FILE-002` regardless of any `<status>.html` file.

Scenarios:
- GIVEN `404.html` exists in the served root.
  WHEN `GET /missing`.
  THEN status is 404 and body is the contents of `404.html`.
- GIVEN `404.html` exists and `Accept: application/json`.
  WHEN `GET /missing`.
  THEN body is the JSON envelope (per probe `notfound-custom`).

Compatibility notes:
- The mechanism applies to any error code, not only 404 (per the README). Probes only validate 404.

Open questions:
- None.

#### SRV-FILE-004: Default MIME types

Status: verified
Area: static-files
Compatibility level: 0
Priority: P0

Reference source:
- README: absent (handler relies on the `mime-types` Node package).
- serve-handler source: `src/index.js:238-242`.
- Existing test: indirect — many tests assert content-types.
- Probe: `tools/probe/cases/mime-defaults.json`.
- Oracle test: ORC-003 (snapshots in tools/probe/snapshots/).

Requirement (draft):
The `Content-Type` header for a file response is determined by the file's extension. Probe-confirmed bindings:

- `.html` → `text/html; charset=utf-8` (after cleanUrls redirect, when fetched via the canonical URL — see SRV-ROUT-001)
- `.js` → `application/javascript; charset=utf-8`
- `.json` → `application/json; charset=utf-8`
- `.css` → `text/css; charset=utf-8`
- `.txt` → `text/plain; charset=utf-8`
- `.svg` → `image/svg+xml`
- `.wasm` → `application/wasm`
- `.png` → `image/png`
- file with no extension → no `Content-Type` header
- file with an unknown extension → no `Content-Type` header

Scenarios:
- GIVEN `/icon.svg` exists.
  WHEN `GET /icon.svg`.
  THEN `Content-Type: image/svg+xml`.
- GIVEN `/noext` exists with no extension.
  WHEN `GET /noext`.
  THEN no `Content-Type` header is set.

Compatibility notes:
- Probe runner: `tools/probe/cases/mime-defaults.json` for the canonical bindings.
- IrServe MAY ship a different default MIME database, provided the bindings above are preserved at L0.
- The `charset=utf-8` suffix on text types is part of the contract.

Open questions:
- Q-004 (exact `Content-Type` for many less-common extensions; deferred to oracle).

#### SRV-FILE-005: `index.html` resolution for directory paths

Status: verified
Area: static-files
Compatibility level: 0
Priority: P0

Reference source:
- README: absent — implicit.
- serve-handler source: `src/index.js:276-307` (`getPossiblePaths`/`findRelated`) and the directory branch at `src/index.js:644-680`.
- Existing test: indirect (`render html directory listing` shows the path-with-index disambiguation).
- Probe: `tools/probe/cases/_smoke.json` (root with `index.html` returns 200 body "hello").
- Oracle test: ORC-001 (snapshots in tools/probe/snapshots/).

Requirement (draft):
A request whose path resolves to a directory containing `index.html` returns the contents of that file as a 200 response (subject to `cleanUrls` redirects from `/dir/index` and `/dir/index.html`). When no index file is present, the directory listing or 404 path takes over (see `SRV-DLST-001`).

Scenarios:
- GIVEN `index.html` at the served root.
  WHEN `GET /`.
  THEN status is 200 and body is `index.html`.
- GIVEN `about/index.html` and `cleanUrls` defaults.
  WHEN `GET /about/`.
  THEN status is 200 with `about/index.html` (per probe `prec-cleanurls-default`).

Compatibility notes:
- Interaction with cleanUrls/trailingSlash is detailed in SRV-ROUT-001..004.

Open questions:
- None.

### ROUT

#### SRV-ROUT-001: `cleanUrls` strips `.html` and redirects via 301

Status: verified
Area: routing
Compatibility level: 2
Priority: P0

Reference source:
- README: yes — serve-handler README, `cleanUrls (Boolean|Array)` section: "If one of these extensions is used at the end of a filename, it will automatically perform a redirect with status code 301 to the same path, but with the extension dropped."
- serve-handler source: `src/index.js:121-143` (the cleanUrl branch in `shouldRedirect`).
- Existing test: `set 'cleanUrls' config property to 'true'`, `set 'cleanUrls' config property to array`, `set 'cleanUrls' config property to empty array` in `test/integration.test.js`.
- Probe: `tools/probe/cases/prec-cleanurls-default.json` and `tools/probe/cases/_smoke.json` (default config redirects `/index.html` → `/index`; for the about fixture redirects `/about.html` → `/about`).
- Oracle test: ORC-002, ORC-012, ORC-017, ORC-020, ORC-021, ORC-023, ORC-026, ORC-027, ORC-033 (snapshots in tools/probe/snapshots/).

Requirement (draft):
With `cleanUrls` enabled (the default), a request for `/path.html` or `/path/index` (or `.../index.html`) is redirected with status 301 to the extension-stripped form. The redirect strips the matched HTML suffix and collapses any resulting `//` to `/`.

Scenarios:
- GIVEN default config, fixture has `index.html`.
  WHEN `GET /index.html`.
  THEN status is 301 and `Location: /index`.
- GIVEN default config, fixture has `about.html`.
  WHEN `GET /about.html`.
  THEN status is 301 and `Location: /about`.

Compatibility notes:
- Per probe, `Location` is unencoded and starts with `/`.
- When `cleanUrls` is an array of globs, only matching paths receive the redirect (per `set 'cleanUrls' config property to array`).
- Open-redirect prevention via cleanUrls is documented as an existing test (`set 'cleanUrls' config property should prevent open redirects`).
- For full pipeline ordering see SRV-ROUT-006.

Open questions:
- None.

#### SRV-ROUT-002: `cleanUrls` resolves extensionless paths to `.html` files

Status: verified
Area: routing
Compatibility level: 2
Priority: P0

Reference source:
- README: yes — serve-handler README: "By default, all `.html` files can be accessed without their extension."
- serve-handler source: `src/index.js:276-307` (`getPossiblePaths('.html')` → `findRelated`) called when `cleanUrl` is on.
- Existing test: `set 'cleanUrls' config property to 'true' and try with file`, `correctly handle requests to /index if cleanUrls is enabled` in `test/integration.test.js`.
- Probe: `tools/probe/cases/prec-cleanurls-default.json` (`GET /about` returns the `about/index.html` body).
- Oracle test: ORC-013, ORC-014, ORC-018, ORC-022, ORC-024 (snapshots in tools/probe/snapshots/).

Requirement (draft):
With `cleanUrls` enabled, a request `/foo` whose direct path is not a file is resolved by trying `/foo/index.html` first and `/foo.html` second. The first that exists is served with status 200.

Scenarios:
- GIVEN fixture `/about.html` and `/about/index.html`, default cleanUrls.
  WHEN `GET /about`.
  THEN status is 200 and body comes from `/about/index.html` (per probe `prec-cleanurls-default`).

Compatibility notes:
- Index-first order is the runtime-observed behavior, confirmed by probe `prec-cleanurls-default` (with both `/about.html` and `/about/index.html` present, `/about/index.html` wins). The serve-handler source backs this — `getPossiblePaths` orders `index<ext>` first. An earlier draft of this body said "trying `/foo.html` and `/foo/index.html` (in that order)"; that wording was imprecise and has been corrected per anti-hallucination rule #4 (runtime is the arbiter).
- For full pipeline ordering see SRV-ROUT-006.

Open questions:
- Q-005 closed by ORC-013/014/022/024 (index-first is the observed default). If a future configuration combination flips this we will document it here.

#### SRV-ROUT-003: `trailingSlash: true` adds a trailing slash via 301

Status: verified
Area: routing
Compatibility level: 2
Priority: P1

Reference source:
- README: yes — serve-handler README, `trailingSlash (Boolean)` section.
- serve-handler source: `src/index.js:145-168`.
- Existing test: `set 'trailingSlash' config property to 'true'` in `test/integration.test.js`.
- Probe: `tools/probe/cases/prec-cleanurls-trailing.json` (compose with cleanUrls); `tools/probe/cases/trailingslash-add.json` (pure, `cleanUrls: false`; also covers multi-slash override + `Location` re-encoding).
- Oracle test: ORC-015, ORC-016, ORC-068, ORC-069, ORC-072, ORC-073, ORC-075, ORC-076 (snapshots in tools/probe/snapshots/).

Requirement (draft):
With `trailingSlash: true`, a request `/path` (no trailing slash, no extension, not a dotfile) is redirected with 301 to `/path/`.

Scenarios:
- GIVEN `trailingSlash: true` and fixture has `about/index.html`.
  WHEN `GET /about`.
  THEN status is 301 and `Location: /about/` (per probe).

Compatibility notes:
- The probe also confirmed that with `trailingSlash:true`, `/about.html` first redirects to `/about` (cleanUrls), and the client then receives a follow-up redirect to `/about/`. The two effects do not collapse into a single redirect (manual-redirect probe shows step-by-step behavior).
- Dotfiles and files with extensions are exempt from the trailing-slash insertion (per source).

Open questions:
- None.

#### SRV-ROUT-004: `trailingSlash: false` strips a trailing slash via 301

Status: verified
Area: routing
Compatibility level: 2
Priority: P1

Reference source:
- README: yes — serve-handler README, `trailingSlash (Boolean)` section.
- serve-handler source: `src/index.js:145-168`.
- Existing test: `set 'trailingSlash' config property to 'false'` in `test/integration.test.js`.
- Probe: `tools/probe/cases/prec-cleanurls-trailing-false.json` (compose with cleanUrls); `tools/probe/cases/trailingslash-strip.json` (pure, `cleanUrls: false`).
- Oracle test: ORC-019, ORC-070, ORC-071 (snapshots in tools/probe/snapshots/).

Requirement (draft):
With `trailingSlash: false`, a request `/path/` (with trailing slash) is redirected with 301 to `/path`.

Scenarios:
- GIVEN `trailingSlash: false` and fixture has `about/index.html`.
  WHEN `GET /about/`.
  THEN status is 301 and `Location: /about` (per probe).

Compatibility notes:
- When `trailingSlash` is `undefined` (default), neither the add nor the strip behavior is applied.

Open questions:
- None.

#### SRV-ROUT-005: Multi-slash path is silently normalized before routing

Status: verified
Area: routing
Compatibility level: 2
Priority: P1

Reference source:
- README: absent.
- serve-handler source: `src/index.js:158-160` (when `decodedPath.indexOf('//') > -1`, the target is the slash-collapsed path).
- Existing test: `set 'trailingSlash' config property to any boolean and remove multiple slashes` in `test/integration.test.js`.
- Probe: `tools/probe/cases/multislash-collapse.json` (raw-socket; verifies wire-level behavior under default config); coupling with phase 5 also exercised by `tools/probe/cases/trailingslash-add.json` (anchors `trailing_double_slash_collapses_via_redirect`, `encoded_double_slash_collapses_via_redirect`) and `tools/probe/cases/trailingslash-strip.json` (anchor `trailing_double_slash_collapses_via_redirect`).
- Oracle test: ORC-025, ORC-026, ORC-027, ORC-072, ORC-073, ORC-074 (snapshots in tools/probe/snapshots/).

Requirement (draft):
Consecutive slashes in the request path are collapsed to a single slash silently before subsequent routing stages. The collapse itself does NOT emit a redirect; any 301 observed for a multi-slash request comes from a later stage (cleanUrls, trailingSlash, or `redirects`) acting on the normalized path. The collapse fires regardless of whether `trailingSlash` is set.

Scenarios:
- GIVEN any fixture with an `index.html` at the root, default config.
  WHEN a wire-level `GET //` is sent.
  THEN status is 200 (the path collapses to `/` and the root index is served directly; per probe `multislash-collapse`, request `double_slash_root`).
- GIVEN default config and fixture has `docs/guide.html`.
  WHEN a wire-level `GET //docs/guide.html` is sent.
  THEN status is 301 and `Location: /docs/guide` (collapse normalizes the path, then cleanUrls 301 fires; per probe, request `double_slash_segment`).
- GIVEN default config and fixture has `docs/guide.html`.
  WHEN a wire-level `GET /docs//guide.html` is sent.
  THEN status is 301 and `Location: /docs/guide` (same: collapse, then cleanUrls; per probe, request `internal_double_slash`).

Compatibility notes:
- The collapse runs regardless of `trailingSlash`'s value; ORC-025 records it under default `trailingSlash: undefined`. The upstream test name "remove multiple slashes [under] trailingSlash" reflects test-file organization, not a gating condition.
- An earlier draft of this body said "redirected with 301 to the slash-collapsed form (only when `trailingSlash` is set)"; that wording was imprecise — the collapse is silent and the 301s seen for `.html` paths come from cleanUrls. Corrected per anti-hallucination rule #4 (runtime is the arbiter).

Open questions:
- Q-006 closed by ORC-025/026/027.

#### SRV-ROUT-006: Operation precedence (redirect resolution → rewrites → static files)

Status: verified
Area: routing
Compatibility level: 2
Priority: P0

Reference source:
- README: partial — serve-handler README documents each rule type independently; the order in which they apply is implicit.
- serve-handler source: `src/index.js` request pipeline — `shouldRedirect(...)` is called first (it produces 301/302 for cleanUrls, trailingSlash and config `redirects` in that internal order). The pre-rewrite `lstat` of the original path at `src/index.js:608-616` is gated by `path.extname(relativePath) !== ''`: it short-circuits the rewrite/findRelated branch only for paths with non-empty extensions. `applyRewrites(...)` itself is then called unconditionally at `src/index.js:618`; whether its result is taken depends on `findRelated` in the `!stats && (cleanUrl || rewrittenPath)` branch at `src/index.js:620-632`. As a result, an existing extensionless file can lose to a matching rewrite — see SRV-RWRT-001 for the qualified rule.
- Existing test: covered indirectly by `set 'rewrites' config property to wildcard path`, `set 'redirects' config property to ...` and the cleanUrl/trailingSlash family.
- Probe: `tools/probe/cases/prec-rewrites-redirects.json`, `tools/probe/cases/prec-cleanurls-default.json`, `tools/probe/cases/prec-cleanurls-trailing.json`, `tools/probe/cases/prec-cleanurls-trailing-false.json`.
- Oracle test: ORC-014, ORC-016, ORC-017, ORC-018, ORC-020, ORC-032, ORC-033 (snapshots in tools/probe/snapshots/).

Requirement (draft):
The server first collapses consecutive slashes in P to a single slash (silent pre-routing normalization, SRV-ROUT-005). Then, for a request whose normalized path does not directly resolve to a regular file inside the served root, the server applies the following stages in order, stopping at the first stage that produces a response:

1. **`cleanUrls` redirect** — if `cleanUrls` is on and P ends with `.html` (or with `/index` / `/index.html`), respond 301 to the extension-stripped form (SRV-ROUT-001).
2. **`trailingSlash` redirect** — if `trailingSlash` is `true` and P lacks a trailing slash (and is not a dotfile / has no extension), respond 301 to `P + "/"`. If `false` and P ends with `/`, respond 301 to the stripped form (SRV-ROUT-003, SRV-ROUT-004).
3. **Config `redirects`** — first matching `redirects` entry produces a 301 (or its `type`-overridden status) (SRV-RDIR-001, SRV-RDIR-002).
4. **`rewrites`** — first matching `rewrites` entry serves the destination file with status 200 (SRV-RWRT-001). Implicit `--single` rewrites participate at this stage (SRV-CLI-008).
5. **`cleanUrls` resolution** — if cleanUrls is on, attempt `<P>/index.html` first and `<P>.html` second; serve the first that exists with status 200 (SRV-ROUT-002).
6. **Static file** — final attempt to resolve P (or its index.html) under the served root (SRV-FILE-001, SRV-FILE-005). Failure here yields a 404 (SRV-FILE-002).

Stage 0 — direct file pre-stat — short-circuits the rewrite/findRelated branch only when the request path **has a non-empty extension** (`path.extname(relativePath) !== ''`, per `src/index.js:608-616`). Concretely:

- If P has an extension (`/asset.css`, `/page.html`) and the file exists, `findRelated` is skipped and the existing file is served. Rewrites and the cleanUrls-resolution at stage 5 do not run.
- If P has no extension (`/about`, `/api`), there is **no** pre-stat. `applyRewrites` runs (stage 4 logic), and if it returns a `rewrittenPath`, `findRelated` is called against the rewrite destination. An extensionless file at the original path that *would* have resolved at stage 6 can therefore be displaced by a matching rewrite. The original path is only attempted at stage 6 (the final `lstat` at `src/index.js:634-642`) when no rewrite matched.

The cleanUrl / trailingSlash redirects in stages 1–2 are unaffected by this — they trigger on the *URL form* (e.g. `/about.html` → 301 `/about`), not on file existence.

Scenarios:
- GIVEN config has both a redirect `/old → /new` and a rewrite `/old → /alt.html`.
  WHEN `GET /old`.
  THEN status is 301 with `Location: /new` (redirects beat rewrites; per probe `prec-rewrites-redirects`).
- GIVEN default config and fixture root has `index.html`.
  WHEN `GET /index.html`.
  THEN status is 301 with `Location: /index` (cleanUrl redirect from stage 1 wins over the existing-file short-circuit; per probe `prec-cleanurls-default`).
- GIVEN `--single` and a redirect `/old → /new`.
  WHEN `GET /old`.
  THEN status is 301 (the SPA rewrite is at stage 4, the redirect is at stage 3; per SRV-CLI-008 scenario 2).

Compatibility notes:
- This SRV consolidates pipeline ordering rules that are currently restated piecemeal across SRV-RDIR-001, SRV-ROUT-001, SRV-ROUT-002, SRV-RWRT-001 and SRV-CLI-008.
- The relative order of `cleanUrls`-redirect vs `trailingSlash`-redirect within the same `shouldRedirect` call is observable via the multi-step probes `prec-cleanurls-trailing` and `prec-cleanurls-trailing-false`: both effects do not collapse, the client receives sequential 301s.
- This requirement starts as `candidate` because no single oracle test currently exercises stages 1–6 end-to-end; promotion to `verified` requires a dedicated oracle case in stage 3.

Open questions:
- None directly; the per-stage open questions (Q-005, Q-006) belong to the individual SRVs.

### RDIR

#### SRV-RDIR-001: `redirects` produce 301 by default

Status: verified
Area: redirects
Compatibility level: 2
Priority: P0

Reference source:
- README: yes — serve-handler README, `redirects (Array)` section.
- serve-handler source: `src/index.js:121-185` (`shouldRedirect`).
- Existing test: `set 'redirects' config property to wildcard path`, `set 'redirects' config property to path segment`, `set 'redirects' config property to one-star wildcard path`, `set 'redirects' config property to extglob wildcard path`, `set 'redirects' config property to a negated wildcard path`, `set 'redirects' config property to wildcard path and do not match` in `test/integration.test.js`.
- Probe: `tools/probe/cases/redirects-types.json` (path-segment redirect with default 301), `tools/probe/cases/redirects-glob-source.json` (`*`-source cross-segment, `**`-collapse, multi-`*` no-overmatch), `tools/probe/cases/redirects-source-slasher.json` (source-side `path.posix.normalize` parity), `tools/probe/cases/redirects-negation-source.json` (`!`-prefix + `:name` falls through to minimatch).
- Oracle test: ORC-030, ORC-084, ORC-085, ORC-089, ORC-090, ORC-091, ORC-092, ORC-093, ORC-094 (snapshots in tools/probe/snapshots/).

Requirement (draft):
A `redirects` entry `{source, destination}` matches `source` (minimatch glob or `path-to-regexp` segment pattern) against the request path; on match the server responds with status 301 and `Location: <destination>` (path-to-regexp segments interpolated). The `Location` value is URI-encoded (`encodeURI`).

Scenarios:
- GIVEN redirect `{ "source": "/old-docs/:id", "destination": "/new-docs/:id" }`.
  WHEN `GET /old-docs/12`.
  THEN status is 301 and `Location: /new-docs/12` (per probe).

Compatibility notes:
- Negated patterns (`!`-prefixed glob) are supported. Bash-style extglob constructs (`+(...)`, `@(...)`, `?(...)`, `*(...)`, `!(...)`) are NOT supported by IrServe; tracked as Q-012 (inherited from cleanUrls).
- Redirects fire AFTER the cleanUrl/trailingSlash redirect path (per `shouldRedirect` order). See SRV-ROUT-006 for the full pipeline.

Open questions:
- None.

#### SRV-RDIR-002: `redirects` with explicit `type` use that status code

Status: verified
Area: redirects
Compatibility level: 2
Priority: P1

Reference source:
- README: yes — serve-handler README, `redirects (Array)` section, `type` field.
- serve-handler source: `src/index.js:172-181` (uses `type || defaultType`).
- Existing test: covered by the redirects test family.
- Probe: `tools/probe/cases/redirects-types.json` (`/old` → 302).
- Oracle test: ORC-031 (snapshots in tools/probe/snapshots/).

Requirement (draft):
When a redirect rule includes a numeric `type` field, that value is used as the response status code in place of the default 301.

Scenarios:
- GIVEN `{ "source": "/old", "destination": "/new", "type": 302 }`.
  WHEN `GET /old`.
  THEN status is 302 (per probe).

Compatibility notes:
- IrServe MUST accept any 3xx code provided by the user; range-checking is not specified by `serve`.

Open questions:
- None.

#### SRV-RDIR-003: External-URL redirect destinations are honored

Status: verified
Area: redirects
Compatibility level: 2
Priority: P2

Reference source:
- README: yes — serve-handler README: "you can use this option ... to a different one (or even an external URL)".
- serve-handler source: `src/index.js:79-89` — `protocol`-aware destination handling skips `slasher` for absolute URLs.
- Existing test: absent (implicit in the README; not flagged in test names).
- Probe: `tools/probe/cases/redirects-destination-forms.json` (7 anchors covering absolute URL, scheme-relative, relative-no-leading-slash, absolute-path baseline, mid-path `..` resolution, leading `..` resolution, empty destination → root).
- Oracle test: ORC-079, ORC-080, ORC-081, ORC-082, ORC-086, ORC-087, ORC-088.

Requirement:
A `destination` whose value parses as a URL with a non-empty protocol (e.g. `https://example.com/x`) is used verbatim as the `Location` header value (`encodeURI` is still applied). Destinations without a protocol go through `path.posix.normalize(path.posix.join('/', value))` (`glob-slash.slasher`), which collapses consecutive slashes (so `//example.com/x` becomes `/example.com/x` — same-origin redirect, not a true scheme-relative URL), resolves `.`/`..` segments (so `a/../b` becomes `/b`; `../b` joins to `/../b` then drops `..`-above-root to `/b`), and turns an empty destination into `/`.

Compatibility notes:
- L2 priority. Q-007 closed by `tools/probe/snapshots/redirects-destination-forms.json`.

Open questions:
- None.

### RWRT

#### SRV-RWRT-001: `rewrites` serve a different file with status 200

Status: verified
Area: rewrites
Compatibility level: 2
Priority: P0

Reference source:
- README: yes — serve-handler README, `rewrites (Array)` section.
- serve-handler source: `src/index.js:91-117` (`applyRewrites`) and `src/index.js:618-622` (apply when no direct stat is available).
- Existing test: `set 'rewrites' config property to wildcard path`, `set 'rewrites' config property to non-matching path`, `set 'rewrites' config property to one-star wildcard path`, `set 'rewrites' config property to path segment` in `test/integration.test.js`.
- Probe: `tools/probe/cases/rewrites-segment.json` (segment + SPA wildcard).
- Oracle test: ORC-028, ORC-029 (snapshots in tools/probe/snapshots/).

Requirement (draft):
A `rewrites` entry `{source, destination}` matches the request path against `source` (minimatch or `path-to-regexp`). On match, the server responds with status 200 (no redirect) and serves the file at `destination` (with `path-to-regexp` segments interpolated). Whether rewrites are short-circuited by an existing original-path file depends on the path shape:

- If the original request path has a non-empty extension (e.g. `/page.html`, `/asset.css`) and the file exists, the file is served directly and the rewrite does not apply.
- If the original request path has no extension (e.g. `/about`, `/api`), `serve` does NOT pre-stat it before applying rewrites. A matching rewrite serves its destination instead, even if the extensionless original path exists as a regular file.

This pre-stat gate is `path.extname(relativePath) !== ''` at `src/index.js:608-616`.

Scenarios:
- GIVEN `{ "source": "/projects/:id/edit", "destination": "/edit-project-:id.html" }`.
  WHEN `GET /projects/123/edit`.
  THEN status is 200 and body is `/edit-project-123.html` (per probe).
- GIVEN `{ "source": "/spa/**", "destination": "/index.html" }`.
  WHEN `GET /spa/some/deep/path`.
  THEN status is 200 and body is `/index.html` (per probe).

Compatibility notes:
- The URL bar does not change (rewrites are silent).
- The "existing files take precedence over rewrites" intuition is **only true for paths with non-empty extensions** (`/page.html`, `/asset.css`). Per `src/index.js:608-616`, the pre-rewrite `lstat` is gated by `path.extname(relativePath) !== ''`. For extensionless request paths (e.g. `/about`), no pre-stat happens; `applyRewrites` runs unconditionally, and a matching rewrite displaces the original-path file even if it exists. See SRV-ROUT-006 for the full pipeline.

Open questions:
- None.

#### SRV-RWRT-002: Mime type fallback when rewriting

Status: verified
Area: rewrites
Compatibility level: 3
Priority: P2

Reference source:
- README: absent.
- serve-handler source: `src/index.js:618-632` — `findRelated` returns the `absolutePath` of the rewritten file, whose extension drives the MIME.
- Existing test: `return mime type of the 'rewrittenPath' if mime type of 'relativePath' is null` in `test/integration.test.js`.
- Probe: not run; behavior is asserted by the existing test name.
- Oracle test: ORC-028 (snapshots in tools/probe/snapshots/).

Requirement (draft):
When a request is served via a rewrite, the response `Content-Type` is determined by the destination file's extension, not by the request path's extension.

Compatibility notes:
- L3 polish; required for SPA correctness when the entry path has no extension.

Open questions:
- None.

### HDR

#### SRV-HDR-001: Custom `headers` apply per glob source

Status: verified
Area: headers
Compatibility level: 3
Priority: P1

Reference source:
- README: yes — serve-handler README, `headers (Array)` section.
- serve-handler source: `src/index.js:194-254` (`getHeaders`).
- Existing test: `set 'headers' to wildcard headers`, `set 'headers' to fixed headers and check default headers`, `error responses get custom headers` in `test/integration.test.js`.
- Probe: `tools/probe/cases/headers-applied.json` (a `**/*.css` rule applies `Cache-Control` and `X-Custom`).
- Oracle test: ORC-048, ORC-053 (snapshots in tools/probe/snapshots/).

Requirement (draft):
A `headers` entry `{source, headers: [{key, value}]}` matches the request path against `source` (minimatch glob). Each matching entry contributes its `headers` to the response, in order. Multiple matching entries accumulate. Custom headers override defaults of the same name (case-insensitive). Custom headers are applied to error responses too.

Scenarios:
- GIVEN `{ "source": "**/*.css", "headers": [{ "key": "Cache-Control", "value": "public, max-age=600" }] }`.
  WHEN `GET /asset.css`.
  THEN response includes `Cache-Control: public, max-age=600` alongside the default headers (per probe).

Compatibility notes:
- Header-name comparison is case-insensitive on the wire but recorded literally in source. IrServe MUST treat HTTP header names per RFC.

Open questions:
- None.

#### SRV-HDR-002: A `value: null` removes a previously-set header

Status: accepted
Area: headers
Compatibility level: 3
Priority: P2

Reference source:
- README: yes — serve-handler README, `headers (Array)` section: "If you set a header `value` to `null` it removes any previous defined header with the same key."
- serve-handler source: `src/index.js:247-251` (the null-pruning loop).
- Existing test: `remove header when null` in `test/integration.test.js`.
- Probe: not run for this corner.
- Oracle test: planned.

Requirement (draft):
A custom-header entry whose `value` is JSON `null` deletes any previously-applied header with the same key (case-insensitively).

Compatibility notes:
- The deletion applies after all custom-header entries are processed.

Open questions:
- None.

### CORS

#### SRV-CORS-001: CORS response header surface under `--cors`

Status: verified
Area: cors
Compatibility level: 3
Priority: P1

Reference source:
- README: yes — `serve` help text: `Enable CORS, sets 'Access-Control-Allow-Origin' to '*'`. The full header surface beyond `Allow-Origin` is not documented in the README.
- serve source: `source/utilities/server.ts` (the `--cors` branch wires a permissive headers middleware around `serve-handler`).
- Existing test: absent.
- Probe: `tools/probe/cases/cors-applied.json`, `tools/probe/cases/cors-response-surface.json`, `tools/probe/cases/cors-preflight.json`.
- Oracle test: ORC-054, ORC-055, ORC-056, ORC-057 (snapshots in tools/probe/snapshots/).

Requirement (draft):
With `-C`/`--cors`, every HTTP response (success, redirect, or error) carries the following four response headers in addition to whatever else the response would normally include:

- `Access-Control-Allow-Origin: *`
- `Access-Control-Allow-Headers: *`
- `Access-Control-Allow-Credentials: true`
- `Access-Control-Allow-Private-Network: true`

`serve` does NOT additionally emit `Access-Control-Allow-Methods`, `Access-Control-Expose-Headers`, or `Access-Control-Max-Age`. Probe-confirmed: the `--cors` middleware does not implement preflight short-circuiting either; an `OPTIONS` request is processed by the static-file pipeline like a `GET` and the four headers are appended to whatever response the pipeline produces.

Scenarios:
- GIVEN `serve --cors` and a file `/asset.css`.
  WHEN `GET /asset.css`.
  THEN the response is 200 and the four CORS headers above are present (per probe `cors-response-surface`, request `file_200`).
- GIVEN `serve --cors` and `/index.html` exists (cleanUrls default).
  WHEN `GET /index.html`.
  THEN the response is 301 to `/index` and the four CORS headers are still present on the redirect response (per probe `cors-response-surface`, request `cleanurls_301`).
- GIVEN `serve --cors` and no file `/nope`.
  WHEN `GET /nope`.
  THEN the response is 404 and the four CORS headers are still present (per probe `cors-response-surface`, request `missing_404`).
- GIVEN `serve --cors`.
  WHEN `OPTIONS /asset.css` with `Origin` and `Access-Control-Request-*`.
  THEN the response is 200 with the file body (no preflight short-circuit) and the four CORS headers are present (per probe `cors-preflight`).

Compatibility notes:
- The presence of `Access-Control-Allow-Private-Network: true` is the surprising bit: it's a relatively recent CORS extension and not universally supported by clients.
- IrServe MUST emit at minimum `Access-Control-Allow-Origin: *` for L1 parity with SRV-CLI-010 (the flag-presence requirement). The full four-header surface is L3 polish (this SRV).
- The lack of preflight short-circuit means `OPTIONS` requests against non-existent paths still 404. IrServe MAY choose to return 204 to preflight requests instead — this would be tracked as an `adapted` decision.

Open questions:
- None for the headers themselves. Whether IrServe should adopt or diverge from the no-preflight-short-circuit behavior is an open design call to be settled when this SRV is promoted past `candidate`.

### DLST

#### SRV-DLST-001: Directory listing on/off via `directoryListing`

Status: verified
Area: directory-listing
Compatibility level: 1
Priority: P0

Reference source:
- README: yes — serve-handler README, `directoryListing (Boolean|Array)` section.
- serve-handler source: `src/index.js:325-465`, gated at line 336.
- Existing test: `render html directory listing`, `render json directory listing`, `render html sub directory listing`, `render json sub directory listing`, `disabled directory listing`, `listing the directory failed` in `test/integration.test.js`.
- Probe: `tools/probe/cases/listing-unlisted.json` (default-on, html and json variants), `tools/probe/cases/listing-disabled.json` (false → 404).
- Oracle test: ORC-008, ORC-009, ORC-010 (snapshots in tools/probe/snapshots/).

Requirement (draft):
For a request whose path resolves to a directory containing no usable index file, the server returns a directory listing as 200 when `directoryListing` is `true` (the default) or matches as an array of glob patterns. When `directoryListing` is `false` (or no glob matches), the request falls through to a 404. The listing has `Content-Type: text/html; charset=utf-8` for HTML clients and `application/json; charset=utf-8` for `Accept: application/json` clients.

Scenarios:
- GIVEN default config, fixture root has `a.txt` and `b.txt`.
  WHEN `GET /`.
  THEN status is 200 and `Content-Type: text/html; charset=utf-8`.
- GIVEN `directoryListing: false`.
  WHEN `GET /`.
  THEN status is 404.
- GIVEN default config and `Accept: application/json`.
  WHEN `GET /`.
  THEN status is 200 and `Content-Type: application/json; charset=utf-8`.

Compatibility notes:
- Exact HTML markup is excluded by `D-003`. The JSON shape (per probe `listing-unlisted`) is at least `{"files":[...], "directory":..., "paths":...}` and is in scope at L1.
- The JSON listing in serve leaks absolute filesystem paths in its `dir` field. IrServe diverges here per `D-007`: the `dir` field is rendered relative to the served root.

Open questions:
- Q-008 closed by `D-007` (sanitized JSON listing).

#### SRV-DLST-002: `unlisted` and the default-excluded set

Status: verified
Area: directory-listing
Compatibility level: 1
Priority: P1

Reference source:
- README: yes — serve-handler README, `unlisted (Array)` section: "The items shown above [`.DS_Store`, `.git`] are excluded from the directory listing by default."
- serve-handler source: `src/index.js:325-334` — `excluded = ['.DS_Store', '.git', ...unlisted]`.
- Existing test: `set 'unlisted' config property to array` in `test/integration.test.js`.
- Probe: `tools/probe/cases/listing-unlisted.json`.
- Oracle test: ORC-009, ORC-010 (snapshots in tools/probe/snapshots/).

Requirement (draft):
Files whose names match `.DS_Store` or `.git` (or any glob in `unlisted`) are omitted from the directory listing. They remain directly fetchable via their explicit URL (the `unlisted` filter only affects listings, not file resolution).

Scenarios:
- GIVEN root has `a.txt`, `b.txt`, `secret.txt`, `.DS_Store`, and `.git/HEAD`, with `unlisted: ["secret.txt"]`.
  WHEN `GET /` (HTML or JSON listing).
  THEN the response does not name `secret.txt`, `.DS_Store`, or `.git` (per probe).

Compatibility notes:
- The default exclusion set is fixed to `.DS_Store` and `.git` per source. IrServe SHOULD preserve those names exactly.

Open questions:
- None.

#### SRV-DLST-003: `renderSingle` serves a lone file in place of a listing

Status: verified
Area: directory-listing
Compatibility level: 2
Priority: P2

Reference source:
- README: yes — serve-handler README, `renderSingle (Boolean)` section.
- serve-handler source: `src/index.js:336-374`.
- Existing test: `render file if directory only contains one` in `test/integration.test.js`.
- Probe: `tools/probe/cases/rendersingle.json`.
- Oracle test: ORC-011 (snapshots in tools/probe/snapshots/).

Requirement (draft):
With `renderSingle: true`, when a directory request has exactly one non-html file (and no usable index), the file is served as the response (status 200, the file's MIME type) instead of the directory listing.

Scenarios:
- GIVEN `renderSingle: true` and `media/photo.png` is the only entry under `media/`.
  WHEN `GET /media/`.
  THEN status is 200 and `Content-Type: image/png` (per probe).

Compatibility notes:
- Disabled by default. README notes: "This is only useful for any files that are not `.html` files (for those, `cleanUrls` is faster)."

Open questions:
- None.

### CACHE

#### SRV-CACHE-001: `ETag` is sent by default and supports 304

Status: verified
Area: http-cache
Compatibility level: 3
Priority: P0

Reference source:
- README: yes — serve-handler README, `etag (Boolean)` section, plus the `--no-etag` CLI flag.
- serve-handler source: `src/index.js:227-233` (etag computation), `src/index.js:758-765` (304 short-circuit).
- Existing test: `automatically handle ETag headers for normal files`, `etag header is set` in `test/integration.test.js`.
- Probe: `tools/probe/cases/etag-roundtrip.json` (200 then 304 on If-None-Match match).
- Oracle test: ORC-042, ORC-043 (snapshots in tools/probe/snapshots/).

Requirement (draft):
By default, file responses carry a strong `ETag` header of the form `"<sha1-hex>"`. When a request includes `If-None-Match` exactly matching the response `ETag`, the server returns status 304 with no body and no `Content-Type`. The 304 short-circuit does not apply to range requests.

Scenarios:
- GIVEN `serve` (defaults), file `/asset.css` exists.
  WHEN `GET /asset.css`.
  THEN status is 200 and an `ETag: "..."` header is present.
- GIVEN the same file's `ETag` value.
  WHEN `GET /asset.css` with `If-None-Match: "<that-value>"`.
  THEN status is 304 (per probe).

Compatibility notes:
- The hash is `sha1(extname + '-' + fileContents)` (per source), but the exact hash function is an implementation detail; only the round-trip behavior is the contract.

Open questions:
- None.

#### SRV-CACHE-002: With `--no-etag`, `Last-Modified` is sent instead

Status: accepted
Area: http-cache
Compatibility level: 3
Priority: P1

Reference source:
- README: yes — `--no-etag` CLI flag help text.
- serve-handler source: `src/index.js:227-236`.
- Existing test: covered transitively by `set 'headers' to fixed headers and check default headers`.
- Probe: not run for this branch.
- Oracle test: planned.

Requirement (draft):
With `--no-etag`, file responses include a `Last-Modified` header (RFC 7231 IMF-fixdate of the file's mtime in UTC) and no `ETag`. `If-Modified-Since` handling is governed by SRV-CACHE-003.

Compatibility notes:
- The mtime resolution depends on the underlying filesystem; sub-second values are not preserved in the header.

Open questions:
- None.

#### SRV-CACHE-003: `If-Modified-Since` 304 handling

Status: unknown
Area: http-cache
Compatibility level: 3
Priority: P1

Reference source:
- README: absent.
- serve-handler source: `src/index.js:758-765` only short-circuits on `If-None-Match`. No explicit `If-Modified-Since` branch.
- Existing test: absent.
- Probe: not run.
- Oracle test: planned.

Requirement (draft):
Behavior of `If-Modified-Since` against the `Last-Modified` header (when `--no-etag` is set) is not directly visible in the source. Probing required.

Open questions:
- Q-009.

#### SRV-CACHE-004: Range requests return 206 / 416

Status: verified
Area: http-cache
Compatibility level: 3
Priority: P2

Reference source:
- README: absent.
- serve-handler source: `src/index.js:717-734` and `src/index.js:749-752` (range plumbing).
- Existing test: `range request`, `range request without size`, `range request not satisfiable` in `test/integration.test.js`.
- Probe: not run.
- Oracle test: ORC-044, ORC-045, ORC-046 (snapshots in tools/probe/snapshots/).

Requirement (draft):
A request with a valid `Range: bytes=...` header for a file of known size returns status 206 with `Content-Range: bytes <start>-<end>/<total>` and `Content-Length: <end-start+1>`. An out-of-range value returns 416 with `Content-Range: bytes */<total>`.

Compatibility notes:
- Multiple ranges are not supported per source ("TODO ? multiple ranges").

Open questions:
- None.

#### SRV-CACHE-005: `Cache-Control` header default and override

Status: verified
Area: http-cache
Compatibility level: 3
Priority: P2

Reference source:
- README: absent (the serve README does not promise any default).
- serve-handler source: `src/index.js` writes `Cache-Control` only when produced by a `headers` configuration entry (see `getHeaders`); there is no global default branch.
- Existing test: covered transitively by `set 'headers' to fixed headers and check default headers` in `test/integration.test.js`.
- Probe: `tools/probe/cases/cache-control-default.json`.
- Oracle test: ORC-047, ORC-048, ORC-049, ORC-050, ORC-051, ORC-052 (snapshots in tools/probe/snapshots/).

Requirement (draft):
By default, file responses, directory-listing responses, and 4xx error responses do NOT carry a `Cache-Control` header. A `Cache-Control` header appears only when a matching `headers` configuration entry sets it. When set, the value is reproduced verbatim, no defaults are folded in, and IrServe MUST NOT prepend or append any directives.

Scenarios:
- GIVEN default config and `/asset.css`.
  WHEN `GET /asset.css`.
  THEN the response does NOT include a `Cache-Control` header (per probe `cache-control-default`, request `default_file_no_rule`).
- GIVEN default config and an empty-of-index directory.
  WHEN `GET /` (HTML listing).
  THEN the response does NOT include a `Cache-Control` header (per probe `cache-control-default`, request `default_listing_html`).
- GIVEN default config and an empty-of-index directory and `Accept: application/json`.
  WHEN `GET /` (JSON listing).
  THEN the response does NOT include a `Cache-Control` header (per probe `cache-control-default`, request `default_listing_json`).
- GIVEN default config and a missing path.
  WHEN `GET /missing` (HTML or JSON 404).
  THEN the 404 response does NOT include a `Cache-Control` header (per probe `cache-control-default`, requests `default_404_html` and `default_404_json`).
- GIVEN a `headers` rule `{ "source": "**/tagged.css", "headers": [{ "key": "Cache-Control", "value": "public, max-age=600" }] }` and `/tagged.css`.
  WHEN `GET /tagged.css`.
  THEN the response includes exactly `Cache-Control: public, max-age=600` (per probe `cache-control-default`, request `rule_applies_cache_control`).

Compatibility notes:
- This is the absence-of-default contract. Browser caches will still apply heuristic caching to responses that have only `ETag`/`Last-Modified` — that is the user's choice, not the server's.
- Custom `headers` rules layered on top behave per SRV-HDR-001 and SRV-HDR-002 (including `null`-removal).

Open questions:
- None.

### SEC

#### SRV-SEC-001: Path traversal outside the served root is denied

Status: verified
Area: security
Compatibility level: 2
Priority: P0

Reference source:
- README: absent (security-by-default).
- serve-handler source: `src/index.js:561-580` — URL is decoded once, then `path.join`ed and verified with `isPathInside`. On failure: 400 with `code: 'bad_request'`. On URI-decode failure: 400.
- Existing test: `error if trying to traverse path`, `prevent access to parent directory`, `error for request with malformed URI` in `test/integration.test.js`.
- Probe: `tools/probe/cases/traversal-encoded.json` (fetch-mode; client-side normalization confounds the result), `tools/probe/cases/traversal-raw-encoded.json` (raw-socket mode; sends the unnormalized bytes).
- Oracle test: ORC-038, ORC-039, ORC-040, ORC-041 (snapshots in tools/probe/snapshots/).

Requirement (draft):
The server MUST NOT serve files outside the served root. Wire-level behavior, observed via the raw probe:

- A request whose path decodes to a sequence containing `..` segments that escape the served root returns status 400 with body shape consistent with the `bad_request` template.
- A request whose path is percent-encoded `..` (e.g. `/%2e%2e/...`) follows the same path: single-decode, then containment check, then 400.
- A request with a leading `//` (e.g. `//etc/passwd`) is treated as an in-root path that simply does not resolve, yielding 404 (not 400). The `//` is not, by itself, an escape.
- A request whose URL contains a malformed `%`-escape (e.g. `/%zz`) returns 400.

Scenarios:
- GIVEN any fixture root.
  WHEN a raw `GET /../package.json HTTP/1.1` is sent.
  THEN status is 400 (per probe `traversal-raw-encoded`, request `raw_dotdot_literal`).
- GIVEN any fixture root.
  WHEN a raw `GET /%2e%2e/package.json HTTP/1.1` is sent.
  THEN status is 400 (per probe `traversal-raw-encoded`, request `raw_dotdot_percent_encoded`).
- GIVEN any fixture root.
  WHEN a raw `GET //etc/passwd HTTP/1.1` is sent.
  THEN status is 404 (per probe `traversal-raw-encoded`, request `raw_double_slash`).
- GIVEN any fixture root.
  WHEN a raw `GET /%zz HTTP/1.1` is sent.
  THEN status is 400 (per probe `traversal-raw-encoded`, request `raw_malformed_percent`).

Compatibility notes:
- The fetch-mode probe `traversal-encoded.json` cannot exercise the wire-level escape because Node's URL parser collapses `..` and decodes `%2e%2e` before the request leaves the client. The raw probe runs against `net.Socket` directly to bypass that normalization. Both probes are kept: the fetch one as evidence of *post-normalization* behavior, the raw one as evidence of *wire-level* behavior.
- Single-decode, then `path.join`, then `isPathInside` is the canonical pipeline. IrServe MAY use a different pipeline as long as the root-escape invariant holds and the four scenarios above are preserved.
- Status code 400 vs 404 is now distinguishable by probe and is part of the contract for the four scenarios listed above.

Open questions:
- Q-010 (now annotated with raw-probe evidence; the SRV remains `accepted` and will be promoted to `verified` in stage 3 once an oracle case asserts the four status codes against the reference).

#### SRV-SEC-002: URL is decoded once

Status: verified
Area: security
Compatibility level: 2
Priority: P1

Reference source:
- README: absent.
- serve-handler source: `src/index.js:561` — `decodeURIComponent(url.parse(request.url).pathname)`.
- Existing test: `error for request with malformed URI` in `test/integration.test.js`.
- Probe: not run for the negative case.
- Oracle test: ORC-034, ORC-035, ORC-036, ORC-037 (snapshots in tools/probe/snapshots/).

Requirement (draft):
The path is URI-decoded exactly once before resolution. Double-decoding (e.g. `%252e` → `%2e` → `.`) MUST NOT occur. Malformed escapes yield 400.

Compatibility notes:
- Single-decode is essential to preserve literal `%2e` in filenames if any exist.

Open questions:
- None.

### SYM

#### SRV-SYM-001: Symlinks are 404 by default; `symlinks: true` follows them

Status: deferred
Area: symlinks
Compatibility level: 4
Priority: P2

Reference source:
- README: yes — serve-handler README, `symlinks (Boolean)` section.
- serve-handler source: `src/index.js:682-715`.
- Existing test: `symlinks should not work by default`, `allow symlinks by setting the option`, `A bad symlink should be a 404` in `test/integration.test.js`.
- Probe: not run.
- Oracle test: planned.

Requirement (draft):
By default, a path that resolves to a symlink returns 404. With `symlinks: true` (CLI: `-S`/`--symlinks`), the server resolves the symlink target and serves it.

Compatibility notes:
- L4. Cross-platform handling (Windows symlinks, junctions) is intentionally deferred.

Open questions:
- Q-011 (Windows symlink/junction parity).

### WIN

#### SRV-WIN-001: Windows path quirks (placeholder)

Status: deferred
Area: windows
Compatibility level: 4
Priority: P2

Reference source:
- README: absent.
- serve-handler source: not yet inspected for Windows-specific handling.
- Existing test: absent (the upstream test suite runs on Linux/macOS in CI).
- Probe: not run; would require Windows-only fixture and likely raw-mode probes for separator handling.
- Oracle test: not planned for MVP.

Requirement (draft):
This entry exists to make the Windows-path coverage gap auditable; it does NOT define behavior. Scenarios will be added when this SRV is promoted past `deferred`. Sub-areas under this umbrella (each will become its own SRV-WIN-NNN when probed):

- Case-insensitive filename matching: NTFS filenames compare case-insensitively at the OS level; how the served-root containment check (SRV-SEC-001) interacts with case-only differences is unspecified.
- Path separator handling: `/` vs `\` in the request path; mixed forms; whether `serve-handler` normalizes them before `path.join`.
- Drive letters and UNC roots in served-directory arguments (`serve C:\public`, `serve \\server\share`).
- Long-path syntax (`\\?\C:\very\long\...`) and the 260-character `MAX_PATH` legacy limit.
- Reserved names (`con`, `nul`, `aux`, etc.) — fetching `/con` on Windows can deadlock processes that don't filter.
- Trailing-dot/space stripping on Windows file open (`foo.` is opened as `foo`).

Compatibility notes:
- L4. Out of MVP scope per `compatibility-levels.md` and the README stage map.
- The placeholder is intentional; per anti-hallucination rule #3, scenarios MUST NOT be invented before probes exist.

Open questions:
- None recorded yet; will be opened (Q-NNN) when probes start running.
