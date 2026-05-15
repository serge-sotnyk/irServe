# Changelog

All notable changes to irServe land here. Format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/); versioning follows
[Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Fixed

- OpenSpec consolidation: created missing `openspec/specs/headers/` capability and restored `## Compatibility notes` sections on `security` and `static-files` specs. The corresponding deltas from change `2026-05-15-008-error-pages-and-security` had not been synced into the source-of-truth before archival. Behavior, oracle probes, and `cargo test` results unchanged — this is a spec catalogue fix only.

## [0.1.0] - 2026-05-15

First MVP release. Levels L0–L3 of the
[compatibility matrix](./docs/reference/serve/compatibility-levels.md) are
implemented; L4 (symlinks, TLS, Windows path quirks) is deferred. See the
[methodology retrospective](./docs/methodology_retrospective.md) for the
experiment write-up.

### Added — L0 (minimal useful server)

- Serve the current or a specified directory; bind host and port via `-l/--listen` (port, `host:port`, or `tcp://host:port` URI).
- Static 200 / 404; basic `Content-Type` resolution.
- `-h/--help` and `-v/--version`.

SRVs: `SRV-CLI-001/002/007/019`, `SRV-FILE-001/002/004/005`.

### Added — L1 (serve-style CLI and configuration)

- `serve.json` loader (`-c/--config`), with the `public`, `cleanUrls`, `trailingSlash`, `redirects`, `rewrites`, `headers`, `directoryListing`, `unlisted`, `renderSingle`, `etag` schema subset.
- Routing normalization: `trailingSlash` 301 add / strip and silent multi-slash collapse.
- Custom `<status>.html` error pages served from the document root.
- Directory listing on / off (global or scoped) with `unlisted` filtering.
- CLI fill-in: `tcp://host:port` URI form, `-p` deprecated alias, `--cors`, `-d/--debug`, `-L/--no-request-logging`, `--no-port-switching`.

SRVs: `SRV-CFG-001/002`, `SRV-CLI-003/006/008/009/010/014/015/016`, `SRV-ROUT-003/004/005`, `SRV-FILE-003`, `SRV-DLST-001/002`.

### Added — L2 (routing behavior, MVP target)

- `cleanUrls` in both `bool` and `string[]` (glob-scoped) forms: 301 from `.html` / `/index`, extensionless `<P>/index.html`-then-`<P>.html` resolution.
- Configured `redirects` (literal, glob, `:name`-pattern source matching; absolute / scheme-relative / relative destinations; any 3xx `type` override).
- Configured `rewrites` (phase-7 chained recursion with an irserve-only depth cap of 64) and `--single` SPA fallback (synthetic `**` rewrite injected at config-load time).
- Custom response headers (`headers` rules): accumulate, case-insensitive override, `value: null` prune, 3xx-skip.
- Path-traversal denial: 400 for lexical `..` escapes and for malformed `%xx` escapes; single-pass URL decode.
- Directory listing HTML / JSON with `Accept`-driven content negotiation, hardcoded `[".DS_Store", ".git"]` defaults, and `renderSingle` short-circuit.

SRVs: `SRV-ROUT-001/002/006`, `SRV-RDIR-001/002/003`, `SRV-RWRT-001`, `SRV-HDR-001/002`, `SRV-SEC-001/002`, `SRV-DLST-003`.

### Added — L3 (HTTP polish, stretch goal)

- `ETag` + `If-None-Match` 304 (`SRV-CACHE-001`). User `headers` rules can override or delete the default ETag before the 304 decision.
- `Last-Modified` + `--no-etag` (`SRV-CLI-013`) with mutex emission (`SRV-CACHE-002`); `If-Modified-Since` 304 short-circuit under the `etag: false` gate (`SRV-CACHE-003`, see D-018).
- Range requests: 206 with `Content-Range`, 416 with `bytes */<total>`; first-segment-only handling of comma lists (`SRV-CACHE-004`, see D-019).
- No default `Cache-Control` header; only user `headers` rules emit one (`SRV-CACHE-005`).
- `OPTIONS` routes through the static pipeline (no preflight short-circuit) so it carries `Vary`, file body, ETag, and the four CORS headers when `--cors` is set (`SRV-CORS-001`).
- HTTP compression (`-u/--no-compression`, `SRV-CLI-012`): `br > gzip > deflate` negotiation, 1024-byte threshold, MIME allowlist + `^text/|\+(?:json|text|xml)$/i` fallback, HEAD / `Cache-Control: no-transform` / identity-only / already-encoded skip conditions, 206 follows the same threshold gate as 200.

### Known limitations and adapted behavior

- **D-018**: `If-Modified-Since` 304 short-circuit is an irserve-only adaptation. The pinned reference is IMS-inert; irserve performs the short-circuit only under the `etag: false` gate so the ETag path stays inert too.
- **D-019**: A `Range` header with a comma-separated list (`bytes=0-3, 8-11`) uses the first segment only. The reference does not implement multi-range either; this is the declared parity scope.
- **D-020**: HTTP compression has four declared divergences from `compression@1.8.1`: framing uses `Content-Length` (not chunked); no `mime-db` table port (curated allowlist + regex fallback instead); `q`-rank within `(0,1)` is not honored (only `q=0` excludes); compressed body bytes are not byte-identical between Node and Rust encoders.
- **L4 deferred**: symlinks (`SRV-CLI-017`, `SRV-SYM-001`), TLS (`SRV-CLI-018`), Windows path quirks (`SRV-WIN-001`), UNIX-domain-socket bind (`SRV-CLI-004`), Windows named-pipe bind (`SRV-CLI-005`).
- **Q-012**: `cleanUrls` extglob patterns (`+(...)`, `@(...)`, `?(...)`, `*(...)`, `!(...)`) are not supported. Standard globs (`*`, `**`, `?`, character classes, brace alternation) work.
- D-001 / D-002 / D-003 / D-004 / D-005: no Node middleware API, no exact terminal output, no exact directory-listing HTML/CSS, no bug-for-bug parity, no clipboard side effect.

### Tooling

- 81 oracle probes pinned against `vercel/serve` v14 + `vercel/serve-handler` (committed under `tools/probe/snapshots/`).
- 362 unit tests across `irserve-core` and `irserve`.
- 16 archived OpenSpec change packages capture the contract evolution: the `000` baseline plus `001`–`015` implementation deltas.

[Unreleased]: https://github.com/serge-sotnyk/irServe/compare/v0.1.0...HEAD
[0.1.0]: https://github.com/serge-sotnyk/irServe/releases/tag/v0.1.0
