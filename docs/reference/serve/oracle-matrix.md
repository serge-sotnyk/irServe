# Oracle matrix

## Status

Stage 3 deliverable. Pinned reference: `serve@14.2.6`,
`serve-handler@6.1.7`. The matrix is the index; the canonical evidence
lives under [`tools/probe/snapshots/`](../../../tools/probe/snapshots/).
Behavior is recorded against the pinned reference using the probe runner
(`node tools/probe/run.mjs --all --snapshot=verify`); see
[`tools/probe/README.md`](../../../tools/probe/README.md) for the
mechanism.

The matrix exists so Stage 4 (OpenSpec bootstrap) can compose specs from
verified behavior, and so Stage 5b's Rust oracle harness has a stable
diff target.

## Conventions

- **ID format.** `ORC-NNN`, three-digit, gap-free, allocated in scope
  order (L0 → L1 → L2 → L3).
- **Verifies.** Every `SRV-*` whose scenarios this oracle covers — at
  least one of the SRV's GIVEN/WHEN/THEN scenarios is exercised by the
  cited probe request.
- **Probe.** `cases/<id>.json#<request-name>` references a single request
  inside the case; `cases/<id>.json` (without anchor) references the
  whole case (used when one ORC is a batch of homogeneous requests).
- **Layer.** Stage 3 records two layers per ORC, mirroring
  [`compatibility-levels.md`](./compatibility-levels.md):
  - **must-match** — fields that the future Rust port must reproduce
    exactly (status code, `Location`, body shape for static files,
    relevant tracked headers, response framing).
  - **may-differ** — fields recorded for audit but not asserted as part
    of compatibility (`Date`, `Server`, exact HTML markup of directory
    listings or error pages, `ETag` value when serve uses mtime,
    `Last-Modified`).
- **Status.**
  - `pending` — listed, no committed snapshot yet.
  - `recorded` — snapshot committed, not yet diffed against.
  - `verified` — snapshot committed AND `--snapshot=verify` passes
    locally (the runner re-runs the case against the pinned reference
    and matches the snapshot byte-for-byte after volatile-field masking).

All entries below are `verified` unless noted; every snapshot listed in
the table is committed and the `--all --snapshot=verify` end-to-end run
exits 0.

## Entries

### L0 — minimal useful server

| ID      | Area          | Request                                | Verifies                                            | Probe                                                | Layer                                                              | Status   |
|---------|---------------|----------------------------------------|-----------------------------------------------------|------------------------------------------------------|--------------------------------------------------------------------|----------|
| ORC-001 | static-files  | `GET /` on dir with `index.html`       | SRV-CLI-002, SRV-CLI-007 (scenarios 1-2), SRV-FILE-001, SRV-FILE-005 | `cases/_smoke.json#root`                              | must-match: status=200, body=`hello\n`, content-type `text/html; charset=utf-8`. may-differ: ETag, Last-Modified.                                                  | verified |
| ORC-002 | routing       | `GET /index.html` (cleanUrls default)  | SRV-ROUT-001                                        | `cases/_smoke.json#index_html_redirect`              | must-match: status=301, `location: /index`, body empty             | verified |
| ORC-003 | static-files  | `GET /<file>` for 10 typical extensions| SRV-FILE-004                                        | `cases/mime-defaults.json`                            | must-match: status=200, content-type per extension (html, js, json, css, txt, wasm, svg, png) and a 200 with default `application/octet-stream`/text fallback for unknown extensions and extensionless files. may-differ: ETag | verified |
| ORC-004 | static-files  | `GET /does-not-exist` (no 404 page)    | SRV-FILE-002                                        | `cases/notfound-shape.json#missing_html`             | must-match: status=404, content-type `text/html; charset=utf-8`     | verified |
| ORC-005 | static-files  | `GET /does-not-exist` with `Accept: application/json` | SRV-FILE-002                          | `cases/notfound-shape.json#missing_html_with_accept` | must-match: status=404, content-type `application/json; charset=utf-8` (serve emits a templated JSON error body when the client prefers JSON) | verified |

### L0 (process-level) — exit-code probes

These ORCs do not exercise HTTP. The runner spawns
`node third_party/serve/build/main.js <args>` to completion and snapshots
exit code + stdout + stderr summaries. See `cases/<id>.json` files with a
top-level `cli` array (alternative to `requests`).

| ID      | Area | Invocation                          | Verifies                                                         | Probe                                                          | Layer                                                                                                              | Status   |
|---------|------|--------------------------------------|------------------------------------------------------------------|----------------------------------------------------------------|--------------------------------------------------------------------------------------------------------------------|----------|
| ORC-060 | cli  | `serve --help` and `serve -h`        | SRV-CLI-019                                                      | `cases/cli-help-version.json#help_long`, `#help_short`         | must-match: exit=0, stdout `kind: text` (non-empty), stderr `kind: empty`. may-differ: exact help text and length (excluded by `D-002`; recorded on disk but masked at verify). | verified |
| ORC-061 | cli  | `serve --version` and `serve -v`     | SRV-CLI-019                                                      | `cases/cli-help-version.json#version_long`, `#version_short`   | must-match: exit=0, stdout `kind: text`, stderr `kind: empty`. may-differ: exact version string (`14.2.6\n` for the pinned reference; `D-002`). | verified |
| ORC-062 | cli  | `serve a b` (two positional args)    | SRV-CLI-007 (scenario 3)                                         | `cases/cli-positional-error.json#two_positionals`              | must-match: exit=non-zero, stdout `kind: empty`, stderr `kind: text` (non-empty). may-differ: exact exit code (reference exits 1; clap defaults to 2; spec text says only "non-zero") and exact stderr text (`D-002`).                                  | verified |
| ORC-063 | cli  | `PORT=<n>` env var without `--listen` | SRV-CLI-001 (env-var scenario)                                  | `cases/default-port-l0.json#root`                              | must-match: status=200, body=`default-port\n` (proves the spawned process honored `PORT` env). may-differ: ETag, Last-Modified, Vary, Accept-Ranges, Content-Length (transport details). | verified |

### L1 — serve-style CLI and configuration

| ID      | Area               | Request                                                  | Verifies                                          | Probe                                                          | Layer                                                                      | Status   |
|---------|--------------------|----------------------------------------------------------|---------------------------------------------------|----------------------------------------------------------------|----------------------------------------------------------------------------|----------|
| ORC-006 | cli                | `--config routes.json` overrides the default `serve.json`| SRV-CLI-009, SRV-CFG-001                          | `cases/config-explicit.json#redirect_from_alternate_config`    | must-match: status=301, `location: /page.html` (proves alt file applied)   | verified |
| ORC-007 | static-files       | `GET /no-such-page` with fixture `404.html` present      | SRV-FILE-003                                      | `cases/error-page-custom.json#missing_with_custom_404`         | must-match: status=404, body=fixture `404.html`, content-type `text/html`   | verified |
| ORC-008 | directory-listing  | `GET /` with `serve.json: directoryListing: false`       | SRV-DLST-001                                      | `cases/listing-disabled.json#root_no_listing`                  | must-match: status=404 (no listing emitted)                                | verified |
| ORC-009 | directory-listing  | `GET /` returns HTML listing                              | SRV-DLST-001, SRV-DLST-002                        | `cases/listing-unlisted.json#listing_html`                     | must-match: status=200, content-type `text/html; charset=utf-8`. may-differ: body markup (D-003), absolute path embedded — recorded but volatile | verified |
| ORC-010 | directory-listing  | `GET /` with `Accept: application/json`                  | SRV-DLST-001, SRV-DLST-002, D-007 (sanitization plan) | `cases/listing-unlisted.json#listing_json`                | must-match: status=200, content-type `application/json`. may-differ: body content (path-leak `dir` field is the planned-adapt site, see D-007) | verified |
| ORC-011 | directory-listing  | `GET /media/` with one file inside (renderSingle)        | SRV-DLST-003                                      | `cases/rendersingle.json#single_file`                          | must-match: status=200, content-type `image/png` (matches the lone fixture file), body=fixture PNG bytes | verified |
| ORC-064 | config             | `serve.json: {"public":"site"}` re-roots to `site/`      | SRV-CFG-001 (`public` field scenario)             | `cases/serve-json-public.json#root_serves_public_index`         | must-match: status=200, body=`<p>public site</p>\n`, content-type `text/html; charset=utf-8` (proves the loader applied `public`). may-differ: ETag, Last-Modified, Vary, Accept-Ranges, Content-Length (transport details). | verified |
| ORC-065 | config             | `--config <missing>` exits non-zero                       | SRV-CFG-001 (missing-explicit scenario)           | `cases/config-missing-explicit.json#missing_explicit_fatal`     | must-match: exit=non-zero, stdout `kind: empty`, stderr `kind: text` (non-empty). may-differ: exact exit code (D-002) and exact stderr text (D-002).                  | verified |
| ORC-066 | config             | malformed `serve.json` exits non-zero                     | SRV-CFG-001 (malformed-JSON scenario)             | `cases/config-malformed.json#malformed_json_fatal`              | must-match: exit=non-zero, stdout `kind: empty`, stderr `kind: text` (non-empty). may-differ: exact exit code (D-002) and exact stderr text (D-002).                  | verified |

### L2 — routing behavior

| ID      | Area               | Request                                                          | Verifies                                | Probe                                                            | Layer                                                                                            | Status   |
|---------|--------------------|------------------------------------------------------------------|-----------------------------------------|------------------------------------------------------------------|--------------------------------------------------------------------------------------------------|----------|
| ORC-012 | routing            | `GET /about.html` with default `cleanUrls`                       | SRV-ROUT-001                            | `cases/prec-cleanurls-default.json#about_html`                   | must-match: status=301, `location: /about`                                                       | verified |
| ORC-013 | routing            | `GET /about` resolves to `about/index.html` (index-first)        | SRV-ROUT-002                            | `cases/prec-cleanurls-default.json#about_no_slash`               | must-match: status=200, content-type `text/html; charset=utf-8`                                  | verified |
| ORC-014 | routing            | `GET /about/` with default `trailingSlash`                       | SRV-ROUT-002, SRV-ROUT-006              | `cases/prec-cleanurls-default.json#about_with_slash`             | must-match: status=200 (trailing slash variant resolves to file when not configured)             | verified |
| ORC-015 | routing            | `GET /about` with `trailingSlash: true`                          | SRV-ROUT-003                            | `cases/prec-cleanurls-trailing.json#about_no_slash`              | must-match: status=301, `location: /about/`                                                      | verified |
| ORC-016 | routing            | `GET /about/` with `trailingSlash: true`                         | SRV-ROUT-003, SRV-ROUT-006              | `cases/prec-cleanurls-trailing.json#about_with_slash`            | must-match: status=200 (file served at slash form)                                               | verified |
| ORC-017 | routing            | `GET /about.html` with `trailingSlash: true`                     | SRV-ROUT-001, SRV-ROUT-006              | `cases/prec-cleanurls-trailing.json#about_html`                  | must-match: status=301, `location: /about/` (cleanUrls strip + trailingSlash add compose)        | verified |
| ORC-018 | routing            | `GET /about` with `trailingSlash: false`                         | SRV-ROUT-002, SRV-ROUT-006              | `cases/prec-cleanurls-trailing-false.json#about_no_slash`        | must-match: status=200                                                                            | verified |
| ORC-019 | routing            | `GET /about/` with `trailingSlash: false`                        | SRV-ROUT-004                            | `cases/prec-cleanurls-trailing-false.json#about_with_slash`      | must-match: status=301, `location: /about`                                                       | verified |
| ORC-020 | routing            | `GET /about.html` with `trailingSlash: false`                    | SRV-ROUT-001, SRV-ROUT-006              | `cases/prec-cleanurls-trailing-false.json#about_html`            | must-match: status=301, `location: /about`                                                       | verified |
| ORC-021 | routing            | `GET /docs/guide.html` with array `cleanUrls: ["/docs/**"]`      | SRV-ROUT-001 (array form, closes Q-005) | `cases/cleanurls-array.json#in_scope_redirect`                   | must-match: status=301, `location: /docs/guide`                                                  | verified |
| ORC-022 | routing            | `GET /docs/guide` with array cleanUrls                            | SRV-ROUT-002 (array form)               | `cases/cleanurls-array.json#in_scope_extensionless`              | must-match: status=200, content-type `text/html; charset=utf-8`                                  | verified |
| ORC-023 | routing            | `GET /blog/post.html` (out of cleanUrls scope) — no redirect      | SRV-ROUT-001 (array form, scope)        | `cases/cleanurls-array.json#out_of_scope_html_direct`            | must-match: status=200 (no 301 produced for paths outside the array glob)                        | verified |
| ORC-024 | routing            | `GET /blog/post` (out of cleanUrls scope) — extensionless miss    | SRV-ROUT-002 (array form, scope)        | `cases/cleanurls-array.json#out_of_scope_extensionless_miss`     | must-match: status=404 (no extensionless resolution outside the array glob)                       | verified |
| ORC-025 | routing            | Wire-level `GET //` (raw socket)                                  | SRV-ROUT-005 (closes Q-006)             | `cases/multislash-collapse.json#double_slash_root`               | must-match: status=200 (consecutive slashes collapse to a single slash before resolution)         | verified |
| ORC-026 | routing            | Wire-level `GET //docs/guide.html`                                | SRV-ROUT-005, SRV-ROUT-001              | `cases/multislash-collapse.json#double_slash_segment`            | must-match: status=301, `location: /docs/guide`                                                   | verified |
| ORC-027 | routing            | Wire-level `GET /docs//guide.html`                                | SRV-ROUT-005, SRV-ROUT-001              | `cases/multislash-collapse.json#internal_double_slash`           | must-match: status=301, `location: /docs/guide`                                                   | verified |
| ORC-028 | rewrites           | `GET /projects/123/edit` with rewrites (path-to-regexp segment)   | SRV-RWRT-001, SRV-RWRT-002              | `cases/rewrites-segment.json#segment_rewrite`                    | must-match: status=200, content-type `text/html; charset=utf-8`, body=destination file            | verified |
| ORC-029 | rewrites           | SPA fallback `GET /spa/some/deep/path`                            | SRV-RWRT-001, SRV-CLI-008               | `cases/rewrites-segment.json#spa_fallback`                       | must-match: status=200, body=`<p>spa-root</p>`                                                    | verified |
| ORC-030 | redirects          | `GET /old-docs/12` default 301                                    | SRV-RDIR-001                            | `cases/redirects-types.json#default_301_segment`                 | must-match: status=301, `location: /docs/12`                                                      | verified |
| ORC-031 | redirects          | `GET /old` with explicit `type: 302`                              | SRV-RDIR-002                            | `cases/redirects-types.json#explicit_302`                        | must-match: status=302, `location: /new`                                                          | verified |
| ORC-032 | routing            | `GET /go` (rewrites + redirects precedence)                       | SRV-ROUT-006                            | `cases/prec-rewrites-redirects.json#go_root`                     | must-match: status=301 (redirect wins over a competing rewrite)                                   | verified |
| ORC-033 | routing            | `GET /page.html` with rewrites also defined                       | SRV-ROUT-006                            | `cases/prec-rewrites-redirects.json#page_html_cleanurl_default`  | must-match: status=301, `location: /page` (cleanUrls 301 fires before rewrites resolve)           | verified |
| ORC-034 | security           | `GET /%2e%2e/etc/passwd` (fetch normalization)                    | SRV-SEC-002                             | `cases/traversal-encoded.json#encoded_dotdot`                    | must-match: status=404 (fetch normalizes the encoded `..`, the resulting path is inside-root)     | verified |
| ORC-035 | security           | `GET //etc/passwd` (fetch)                                        | SRV-SEC-002                             | `cases/traversal-encoded.json#double_slash_passwd`               | must-match: status=404                                                                             | verified |
| ORC-036 | security           | `GET /../../etc/passwd` (fetch)                                   | SRV-SEC-002                             | `cases/traversal-encoded.json#raw_dotdot`                        | must-match: status=404                                                                             | verified |
| ORC-037 | security           | `GET /sub/%2e%2e/secret.txt` (fetch — single decode)              | SRV-SEC-002                             | `cases/traversal-encoded.json#encoded_in_subpath`                | must-match: status=200 (decoded once → resolves to `/secret.txt`, which IS inside root and is not unlisted in this fixture). Confirms single-pass decode is symmetric. | verified |
| ORC-038 | security           | Wire-level `GET /../package.json` (raw socket)                    | SRV-SEC-001 (closes Q-010 wire-level)   | `cases/traversal-raw-encoded.json#raw_dotdot_literal`            | must-match: status=400 (literal `..` segment rejected at parse), wire request line includes the literal segment | verified |
| ORC-039 | security           | Wire-level `GET /%2e%2e/package.json` (raw)                       | SRV-SEC-001 (closes Q-010 wire-level)   | `cases/traversal-raw-encoded.json#raw_dotdot_percent_encoded`    | must-match: status=400, request line preserves `%2e%2e`                                          | verified |
| ORC-040 | security           | Wire-level `GET //etc/passwd` (raw)                                | SRV-SEC-001                             | `cases/traversal-raw-encoded.json#raw_double_slash`              | must-match: status=404 (multi-slash collapse is benign — request stays inside root)               | verified |
| ORC-041 | security           | Wire-level `GET /%zz` (malformed percent-encoding)                 | SRV-SEC-001                             | `cases/traversal-raw-encoded.json#raw_malformed_percent`         | must-match: status=400                                                                             | verified |

### L3 (disputed) — HTTP polish

| ID      | Area          | Request                                                            | Verifies                                  | Probe                                                          | Layer                                                                                     | Status   |
|---------|---------------|--------------------------------------------------------------------|-------------------------------------------|----------------------------------------------------------------|-------------------------------------------------------------------------------------------|----------|
| ORC-042 | http-cache    | First `GET /asset.css` records ETag                                | SRV-CACHE-001                             | `cases/etag-roundtrip.json#first_get`                          | must-match: status=200, ETag present (content-based hash, deterministic per fixture content) | verified |
| ORC-043 | http-cache    | Second `GET /asset.css` with `If-None-Match: <etag>`               | SRV-CACHE-001                             | `cases/etag-roundtrip.json#second_with_inm`                    | must-match: status=304 (no body)                                                          | verified |
| ORC-044 | http-cache    | `GET /blob.txt` with `Range: bytes=0-3`                            | SRV-CACHE-004                             | `cases/range-request.json#in_range_first_4`                    | must-match: status=206, `Content-Range: bytes 0-3/11`, `Content-Length: 4`, `accept-ranges: bytes`, body sha for first 4 bytes | verified |
| ORC-045 | http-cache    | `GET /blob.txt` with `Range: bytes=8-`                             | SRV-CACHE-004                             | `cases/range-request.json#in_range_tail`                       | must-match: status=206, `Content-Range: bytes 8-10/11`                                    | verified |
| ORC-046 | http-cache    | `GET /blob.txt` with out-of-range `Range`                          | SRV-CACHE-004                             | `cases/range-request.json#out_of_range`                        | must-match: status=416, `Content-Range: bytes */11`                                       | verified |
| ORC-047 | caching       | Default `GET /asset.css` — no `Cache-Control` unless rule          | SRV-CACHE-005                             | `cases/cache-control-default.json#default_file_no_rule`        | must-match: status=200, `cache-control` header NOT present                                | verified |
| ORC-048 | caching       | `GET /tagged.css` matches a `headers` rule with `Cache-Control`    | SRV-CACHE-005, SRV-HDR-001                | `cases/cache-control-default.json#rule_applies_cache_control`  | must-match: status=200, `cache-control: public, max-age=600`                              | verified |
| ORC-049 | caching       | `GET /` listing has no default `Cache-Control`                     | SRV-CACHE-005                             | `cases/cache-control-default.json#default_listing_html`        | must-match: status=200, `cache-control` absent. may-differ: body (volatile listing)        | verified |
| ORC-050 | caching       | `GET /` JSON listing has no default `Cache-Control`                | SRV-CACHE-005                             | `cases/cache-control-default.json#default_listing_json`        | must-match: status=200, content-type `application/json`, `cache-control` absent             | verified |
| ORC-051 | caching       | 404 (HTML) — no default `Cache-Control`                            | SRV-CACHE-005, SRV-FILE-002               | `cases/cache-control-default.json#default_404_html`            | must-match: status=404, `cache-control` absent                                            | verified |
| ORC-052 | caching       | 404 (JSON) — no default `Cache-Control`                            | SRV-CACHE-005, SRV-FILE-002               | `cases/cache-control-default.json#default_404_json`            | must-match: status=404, content-type `application/json`, `cache-control` absent             | verified |
| ORC-053 | headers       | `headers` glob (`**/*.css`) applies `Cache-Control` and `X-Custom` to a CSS asset | SRV-HDR-001                                  | `cases/headers-applied.json#css_get`                           | must-match: status=200, `cache-control: public, max-age=600`, `x-custom: yes` (the case opts in via `snapshot.extraTrackedHeaders: ["x-custom"]`) | verified |
| ORC-054 | cors          | `--cors` adds `Access-Control-Allow-Origin: *` on a 200 response   | SRV-CLI-010, SRV-CORS-001                 | `cases/cors-applied.json#css_with_cors`                        | must-match: status=200, `access-control-allow-origin: *`                                  | verified |
| ORC-055 | cors          | `--cors` flag also applies on a 301                                | SRV-CLI-010, SRV-CORS-001                 | `cases/cors-flag.json#html_with_cors`                          | must-match: status=301, ACAO header present                                               | verified |
| ORC-056 | cors          | OPTIONS preflight under `--cors`                                   | SRV-CORS-001                              | `cases/cors-preflight.json#preflight_options`                  | must-match: status=200, headers `access-control-allow-origin: *`, `access-control-allow-headers: *`, `access-control-allow-credentials: true`, `access-control-allow-private-network: true`. Note: serve does NOT emit `access-control-allow-methods`. | verified |
| ORC-057 | cors          | `--cors` — full response surface across 200 / 301 / 404 paths      | SRV-CORS-001                              | `cases/cors-response-surface.json` (3 requests)                | must-match: same four ACA-* headers as ORC-056 are present on the 200, 301, and 404 responses uniformly; ACAM still absent.            | verified |
| ORC-058 | compression   | Default GET with `Accept-Encoding: gzip, deflate`                  | SRV-CLI-012                               | `cases/compression-default.json#with_accept_encoding`          | must-match: status=200, `Vary: Accept-Encoding` present (compression negotiation hook)    | verified |
| ORC-059 | static-files  | Custom 404 page (`404.html`) under both Accept variants            | SRV-FILE-002, SRV-FILE-003                | `cases/notfound-custom.json` (2 requests)                      | must-match: HTML accept → status=404 + body=`404.html`; JSON accept → status=404 + content-type `application/json; charset=utf-8` + templated JSON body (the custom `404.html` is HTML-only; JSON clients always get the built-in JSON template). | verified |

## Coverage gaps

SRVs and scenarios intentionally NOT promoted to `verified` in Stage 3,
with one-line rationale each. These remain at their pre-existing status
in `inventory.md`.

- **SRV-CLI-001** (default port 3000 / `PORT` env) — promoted to
  `verified` in Stage-5b change 002 via ORC-063 (`default-port-l0.json`,
  env-var scenario). The no-flag-no-env scenario remains source-evidence
  only because port 3000 cannot be reliably reserved on developer
  machines.
- **SRV-CLI-003** (`-l tcp://host:port`) — single TCP-URI parse, no
  observable HTTP-level divergence vs. SRV-CLI-002. Treated as transitive;
  no dedicated probe. Status stays `accepted`.
- **SRV-CLI-004** (UNIX socket bind, L4 deferred) — Linux-only; oracle
  requires non-Windows CI.
- **SRV-CLI-005** (Windows named pipe, L4 deferred) — pipe-bind is L4.
- **SRV-CLI-006** (`-p` deprecated alias) — covered transitively by every
  `-l` probe; no dedicated ORC.
- **SRV-CLI-011** (`--no-clipboard` suppression) — every probe passes the
  flag so the startup path is exercised, but the actual contract (no
  clipboard write) is unobservable via HTTP and the runner does not
  inspect clipboard state. Stays `accepted`. Per `D-005`, IrServe never
  modifies the clipboard, so an oracle test is not planned.
- **SRV-CLI-016** (`--no-port-switching` failure-to-start) — every probe
  passes the flag with an already-free port, so only the happy path is
  exercised. The actual contract (refuse to fall back when the port is
  occupied) requires a probe that occupies the port first; deferred to
  Stage 5b. Stays `accepted`.
- ~~**SRV-CLI-007** scenario 3~~ — closed in Stage 3 review round 1 by
  ORC-062 (`cli-positional-error.json`). The runner gained a CLI-mode
  branch that snapshots exit code + stdout/stderr.
- **SRV-CLI-013** (`--no-etag` switches default to `Last-Modified`) —
  no probe exercises `--no-etag` in this stage. Stays `accepted`.
- **SRV-CLI-014** (`--debug`) — affects logging only; no observable HTTP
  surface. Stays `accepted`.
- **SRV-CLI-015** (`--no-request-logging`) — same: stdout-only effect.
  Stays `accepted`.
- **SRV-CLI-017** / **SRV-SYM-001** (symlinks, L4 deferred) — symlink
  fixture creation is platform-specific; deferred to Stage 5b.
- **SRV-CLI-018** (TLS `--ssl-*`, L4 deferred) — oracle requires fixture
  certificates.
- ~~**SRV-CLI-019**~~ — closed in Stage 3 review round 1 by ORC-060 and
  ORC-061 (`cli-help-version.json`).
- **SRV-CFG-002** (configuration schema overview) — META requirement; no
  single observable behavior. Verified piecewise via SRV-ROUT-*,
  SRV-RDIR-*, SRV-RWRT-*, SRV-HDR-*, SRV-DLST-* entries. Stays `accepted`.
- **SRV-RDIR-003** (external-URL redirects) — no probe in this stage.
  Q-007 stays open. Stays `accepted`.
- **SRV-HDR-002** (`value: null` removes a header) — the `headers-custom`
  probe was authored before this stage and exercises cleanUrls 301 rather
  than the null-value removal path. See "Probe-design notes" below.
  Stays `accepted`.
- **SRV-CACHE-002** (`Last-Modified` under `--no-etag`) — probe gap,
  same as SRV-CLI-013. Stays `accepted`.
- **SRV-CACHE-003** (`If-Modified-Since` 304 handling, `unknown` linked
  to Q-009) — `etag-conditional` does not exercise the `--no-etag` +
  `If-Modified-Since` path. Q-009 remains `open`. Stays `unknown`.
- **SRV-WIN-001** (Windows path quirks, placeholder, deferred).

### Cross-platform note

Snapshots in this repository were captured on Windows. Two probes are
currently OS-specific:

- `cases/listing-unlisted.json` (ORC-009 / ORC-010) — serve-handler's
  HTML and JSON directory listing embeds the OS root marker (`C:\\` vs
  `/`) and path separator (`\` vs `/`) directly in the response body.
  The runner's body normalization replaces the absolute fixture
  directory with `<FIXTURE_ROOT>` but does not collapse drive prefixes
  or path separators. Re-running `--snapshot=verify` on POSIX will
  diff on `body.sha256` / `body.preview` of these two requests.
- `cases/cache-control-default.json` (ORC-049 / ORC-050) — same listing
  body issue.

Stage 5b (Rust harness) is the right place to harmonize this either by
running the oracle in a Linux container (matching CI) or by
re-recording the listing snapshots cross-platform. Stage 3 deliberately
keeps the runner simple and accepts the Windows-host limitation.

Probe-design notes the matrix records but does not act on:

- `cases/etag-conditional.json` — both requests under default `cleanUrls`
  redirect to `/page` before serving, so the case as written exercises
  cleanUrls rather than the etag/conditional logic it claims. The
  snapshot is locked-in (verifies what was observed); `etag-roundtrip`
  carries SRV-CACHE-001's load. Logged here as a Stage-5b cleanup TODO.
- `cases/headers-custom.json` — the case description claims a `value:
  null` removal scenario but the fixture contains no such entry; the
  request also hits the default cleanUrls 301 before any custom headers
  apply. As a result this snapshot does not currently evidence
  SRV-HDR-002. SRV-HDR-002 stays `accepted`. Stage-5b cleanup TODO:
  rebuild the case with `cleanUrls: false` (or a non-`.html` target) and
  an actual `value: null` rule.

## How the matrix is updated

- **Adding behavior.** Author a probe case under `tools/probe/cases/`,
  run `node tools/probe/run.mjs <id> --snapshot=update`, sanity-check
  the snapshot, append an ORC row in the right level, cross-link the
  SRV(s) it verifies.
- **Promoting status.** Any ORC whose snapshot passes
  `node tools/probe/run.mjs <id> --snapshot=verify` end-to-end is
  `verified`.
- **Removing entries.** Matrix entries are append-only within a stage;
  deletions go through Stage 5b/Stage 6 once Rust-side oracle tests
  exist.
- **Snapshots are the contract.** If a snapshot diverges from a SRV's
  scenarios after re-recording, the SRV body is NOT silently updated in
  Stage 3 — log the divergence in this section's "Coverage gaps" as a
  Stage-5b TODO and keep the SRV scenarios unchanged. The SRV is part of
  Stage 1's reverse inventory; its body is meant to capture what was
  *understood*, while the snapshot captures what was *observed*. When the
  two collide, Stage 5b will reconcile them in code.
