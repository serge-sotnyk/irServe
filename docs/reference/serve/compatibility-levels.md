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

- Subset of `serve` CLI options — SRV-CLI-006 (`-p` deprecated alias), SRV-CLI-009 (`--config`), SRV-CLI-010 (`--cors`), SRV-CLI-011 (`--no-clipboard`), SRV-CLI-014 (`--debug`), SRV-CLI-015 (`--no-request-logging`), SRV-CLI-016 (`--no-port-switching`).
- Load `serve.json` from the served directory — SRV-CFG-001.
- `public` root and configuration schema — SRV-CFG-002.
- Custom error pages (`<status>.html`) — SRV-FILE-003.
- Directory listing on/off — SRV-DLST-001.
- `unlisted` files — SRV-DLST-002.

> Note: `trailingSlash` was placed at Level 2 by the inventory (SRV-ROUT-003, SRV-ROUT-004) because its observable effect is a 301 routing decision rather than configuration loading. The skeleton's earlier placement at Level 1 should be revisited at Stage 2.

### Level 2 — routing behavior

- `cleanUrls` (extensionless `.html` resolution and 301) — SRV-ROUT-001, SRV-ROUT-002.
- `trailingSlash` add / strip via 301 — SRV-ROUT-003, SRV-ROUT-004.
- Multi-slash path normalization — SRV-ROUT-005.
- `redirects` — SRV-RDIR-001, SRV-RDIR-002, SRV-RDIR-003.
- `rewrites` — SRV-RWRT-001.
- SPA-fallback equivalent (`--single`) — SRV-CLI-008.
- `renderSingle` (single-file directory rendering) — SRV-DLST-003.
- Path traversal denial — SRV-SEC-001.
- URL single-pass decoding — SRV-SEC-002.
- Documented precedence between rewrites, redirects, cleanUrls, trailingSlash, and static files — partially captured in the Compatibility-notes blocks of SRV-RDIR-001, SRV-ROUT-001, SRV-RWRT-001 and probes `prec-rewrites-redirects`, `prec-cleanurls-trailing`. **Coverage gap:** no dedicated SRV; see Stage-2 todos.

### Level 3 — HTTP polish

- Custom `headers` rules — SRV-HDR-001, SRV-HDR-002.
- MIME-fallback for rewritten responses — SRV-RWRT-002.
- `ETag` and `If-None-Match` 304 — SRV-CACHE-001, with override SRV-CLI-013.
- `Last-Modified` and `If-Modified-Since` — SRV-CACHE-002, SRV-CACHE-003 (status `unknown`, see Q-009).
- Range requests / 206 / 416 — SRV-CACHE-004.
- HTTP compression on/off — SRV-CLI-012.

> **Coverage gap:** no dedicated `Cache-Control` header inventory entry. Tracked as a Stage-2 todo.

### Level 4 — edge compatibility

- Symlinks (`-S`/`--symlinks`, `serve.json` `symlinks: true`) — SRV-CLI-017, SRV-SYM-001.
- UNIX-domain-socket bind — SRV-CLI-004.
- Windows named-pipe bind — SRV-CLI-005.
- TLS (`--ssl-cert`, `--ssl-key`, `--ssl-pass`) — SRV-CLI-018.
- Path-traversal corner cases beyond denial — partly under SRV-SEC-001 (the inventory placed denial at Level 2; deeper edge cases — encoded forms, mixed-separator forms — are tracked under Q-010).

> **Coverage gap:** Windows path quirks (case, separators, drive letters, `\\?\` long-path syntax) have no SRV yet. Stage-2 todo.

## Coverage gaps (Stage-2 follow-ups)

These bullets need either a new SRV in a future inventory pass or a deliberate `decisions.md` rejection. They are NOT to be invented during Stage 1.

1. Documented precedence rules between rewrites / redirects / cleanUrls / trailingSlash / static files — currently scattered across compatibility notes; deserves its own SRV-ROUT entry.
2. `Cache-Control` header behavior (defaults and the interaction with `headers` rules) — no SRV yet.
3. `trailingSlash` level placement — inventory says Level 2, skeleton bullet was Level 1; reconcile when Stage 2 is opened.
4. CORS response-header semantics (`Access-Control-Allow-*`) — SRV-CLI-010 captures the flag, but the response-header surface is not enumerated. The probe runner currently does not capture `access-control-allow-*`; extend before Stage 3.
5. Windows path quirks at Level 4.
6. Wire-level path traversal probing (Q-010): the current probe runner relies on `fetch`, which normalizes `..`, `%2e%2e`, and `//` before sending. A raw-socket probe variant or an `http.request` based probe is needed before SRV-SEC-001 can be promoted to `verified`.

## How this document is used

- Stage 1 (reverse inventory) groups every candidate requirement by the level it belongs to.
- Stage 4 (OpenSpec bootstrap change) only includes requirements up to the agreed compatibility level.
- Anything above the agreed level is recorded with status `deferred` in `inventory.md`.
