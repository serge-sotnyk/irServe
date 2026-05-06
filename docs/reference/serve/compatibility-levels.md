# IrServe compatibility target

IrServe is a Rust CLI static file server inspired by `vercel/serve`.

## Compatibility principle

IrServe aims to match the **observable HTTP behavior** of `npm serve` where practical. The reference implementations are `vercel/serve` and `vercel/serve-handler`, used as oracles for selected behavior tests, not as a source-level template.

## Explicit non-goals

- Node.js middleware API compatibility (`serve-handler` as embeddable library).
- Exact terminal output / stdout formatting.
- Exact HTML / CSS of the directory listing in MVP.
- Bug-for-bug compatibility.

## Compatibility levels

These levels frame what reverse-engineering and implementation should target. For the experiment, reaching **Level 2** or **Level 3** is sufficient. Level 4 may consume disproportionate effort.

### Level 0 — minimal useful server

- Serve the current directory.
- Serve a specified directory.
- Bind host and port.
- Return static files for existing paths.
- Return 404 for missing paths.
- Basic MIME type / `Content-Type` resolution.

### Level 1 — serve-style CLI and configuration

- Subset of `serve` CLI options (port, listen, no-clipboard equivalents, etc. — to be inventoried).
- Load `serve.json` from the served directory.
- `public` root.
- Directory listing on/off.
- `unlisted` files.
- `trailingSlash` behavior.

### Level 2 — routing behavior

- `cleanUrls` (extensionless `.html` resolution).
- `redirects`.
- `rewrites`.
- SPA-fallback equivalent (single-file rewrite).
- Documented precedence between the above and static files.

### Level 3 — HTTP polish

- Custom `headers` rules.
- `ETag`.
- `Last-Modified`.
- Conditional requests (`If-None-Match`, `If-Modified-Since`).
- Cache-control behavior.

### Level 4 — edge compatibility

- Symlinks.
- Path traversal edge cases.
- Windows path quirks.
- Obscure historical behaviors.

## How this document is used

- Stage 1 (reverse inventory) groups every candidate requirement by the level it belongs to.
- Stage 4 (OpenSpec bootstrap change) only includes requirements up to the agreed compatibility level.
- Anything above the agreed level is recorded with status `deferred` in `inventory.md`.
