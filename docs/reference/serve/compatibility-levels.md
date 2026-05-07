# IrServe compatibility target

IrServe is a Rust CLI static file server inspired by `vercel/serve`.

## Compatibility principle

IrServe aims to match the **observable HTTP behavior** of `npm serve` where practical. The reference implementations are `vercel/serve` and `vercel/serve-handler`, used as oracles for selected behavior tests, not as a source-level template.

## Explicit non-goals

- Node.js middleware API compatibility (`serve-handler` as embeddable library). See `decisions.md` D-001.
- Exact terminal output / stdout formatting. See `decisions.md` D-002.
- Exact HTML / CSS of the directory listing in MVP. See `decisions.md` D-003.
- Bug-for-bug compatibility. See `decisions.md` D-004.

## Compatibility levels

These levels frame what reverse-engineering and implementation should target. For the experiment, reaching **Level 2** or **Level 3** is sufficient. Level 4 may consume disproportionate effort.

Each bullet cites the backing `SRV-*` entries from `inventory.md`. Bullets with no citation are tracked under "Coverage gaps" at the bottom of this file.

### Level 0 — minimal useful server

- Serve the current directory by default — SRV-CLI-001, SRV-CLI-007.
- Serve a specified directory — SRV-CLI-007.
- Bind host and port — SRV-CLI-001, SRV-CLI-002, SRV-CLI-003.
- Return static files for existing paths — SRV-FILE-001, SRV-FILE-005.
- Return 404 for missing paths — SRV-FILE-002.
- Basic MIME type / `Content-Type` resolution — SRV-FILE-004.
- Help and version flags — SRV-CLI-019.

### Level 1 — serve-style CLI and configuration

- Subset of `serve` CLI options — SRV-CLI-006 (`-p` deprecated alias), SRV-CLI-009 (`--config`), SRV-CLI-010 (`--cors` flag presence; full response surface is L3 — see SRV-CORS-001), SRV-CLI-011 (`--no-clipboard`), SRV-CLI-014 (`--debug`), SRV-CLI-015 (`--no-request-logging`), SRV-CLI-016 (`--no-port-switching`).
- Load `serve.json` from the served directory — SRV-CFG-001.
- `public` root and configuration schema — SRV-CFG-002.
- Custom error pages (`<status>.html`) — SRV-FILE-003.
- Directory listing on/off — SRV-DLST-001.
- `unlisted` files — SRV-DLST-002.

### Level 2 — routing behavior

- `cleanUrls` (extensionless `.html` resolution and 301) — SRV-ROUT-001, SRV-ROUT-002.
- `trailingSlash` add / strip via 301 — SRV-ROUT-003, SRV-ROUT-004. (Confirmed at Level 2: the observable effect is a 301 routing decision, not configuration loading.)
- Multi-slash path normalization — SRV-ROUT-005.
- `redirects` — SRV-RDIR-001, SRV-RDIR-002, SRV-RDIR-003.
- `rewrites` — SRV-RWRT-001.
- Operation precedence (cleanUrls 301 → trailingSlash 301 → config redirects → rewrites → cleanUrls resolution → static file) — SRV-ROUT-006.
- SPA-fallback equivalent (`--single`) — SRV-CLI-008.
- `renderSingle` (single-file directory rendering) — SRV-DLST-003.
- Path traversal denial (with wire-level scenarios) — SRV-SEC-001.
- URL single-pass decoding — SRV-SEC-002.

### Level 3 — HTTP polish

- Custom `headers` rules — SRV-HDR-001, SRV-HDR-002.
- MIME-fallback for rewritten responses — SRV-RWRT-002.
- `ETag` and `If-None-Match` 304 — SRV-CACHE-001, with override SRV-CLI-013.
- `Last-Modified` and `If-Modified-Since` — SRV-CACHE-002, SRV-CACHE-003 (status `unknown`, see Q-009).
- Range requests / 206 / 416 — SRV-CACHE-004.
- `Cache-Control` header default and override — SRV-CACHE-005.
- HTTP compression on/off — SRV-CLI-012.
- CORS response-header surface under `--cors` — SRV-CORS-001.

### Level 4 — edge compatibility

- Symlinks (`-S`/`--symlinks`, `serve.json` `symlinks: true`) — SRV-CLI-017, SRV-SYM-001.
- UNIX-domain-socket bind — SRV-CLI-004.
- Windows named-pipe bind — SRV-CLI-005.
- TLS (`--ssl-cert`, `--ssl-key`, `--ssl-pass`) — SRV-CLI-018.
- Windows path quirks (case-sensitivity, separator handling, drive letters, `\\?\` long-path syntax, reserved names, trailing-dot/space stripping) — SRV-WIN-001 (placeholder, deferred).
- Path-traversal corner cases beyond denial — captured under SRV-SEC-001 with explicit wire-level scenarios; remaining deep edge cases (mixed-separator forms across platforms) live with SRV-WIN-001.

## Coverage gaps (status after Stage 2)

The six Stage-2 follow-ups previously logged here have been resolved as follows:

1. ✅ Documented precedence rules between rewrites / redirects / cleanUrls / trailingSlash / static files — captured in **SRV-ROUT-006**.
2. ✅ `Cache-Control` header behavior — captured in **SRV-CACHE-005** with probe `cache-control-default`.
3. ✅ `trailingSlash` level placement — confirmed at Level 2 (this file was reconciled with the inventory).
4. ✅ CORS response-header semantics — captured in **SRV-CORS-001** with probes `cors-applied`, `cors-response-surface`, `cors-preflight`. Probe runner now tracks the `access-control-allow-*` allowlist.
5. ⏳ Windows path quirks — gap acknowledged via the **SRV-WIN-001** deferred placeholder; full SRV bodies are out of MVP scope per the Level 4 policy.
6. ⏳ Wire-level path traversal probing (Q-010) — raw-socket probe mode added (`tools/probe/run.mjs` `mode: "raw"`); probe `traversal-raw-encoded` recorded; SRV-SEC-001 updated with the four wire-level scenarios. Remaining: an oracle case in Stage 3 must assert these status codes against the reference before SRV-SEC-001 is promoted to `verified`.

No new gaps were introduced during the Stage 2 capability refresh.

## How this document is used

- Stage 1 (reverse inventory) groups every candidate requirement by the level it belongs to.
- Stage 4 (OpenSpec bootstrap change) only includes requirements up to the agreed compatibility level.
- Anything above the agreed level is recorded with status `deferred` in `inventory.md`.
