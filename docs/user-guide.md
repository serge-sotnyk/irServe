# irServe user guide

A short tour of every flag and `serve.json` knob irServe understands. The full behavior contract lives in [`openspec/specs/`](../openspec/specs/); the response-shape evidence (status codes, headers, bodies) lives in [`tools/probe/snapshots/`](../tools/probe/snapshots/).

Each section gives one representative example. For the next-layer detail (which patterns are supported, divergences from `vercel/serve`, edge cases), see [`CHANGELOG.md`](../CHANGELOG.md) §"Known limitations" and [`docs/reference/serve/decisions.md`](./reference/serve/decisions.md).

## Quick start

```bash
mkdir _tmp && echo hello > _tmp/index.html
irserve --listen 3010 _tmp
curl -i http://127.0.0.1:3010/         # 200, body: hello
```

Omit `--listen` to bind port 3000. Pass `PORT=3010` as env. Pass `--listen 3010 --listen 3011 _tmp` to bind multiple ports.

## Routing

`cleanUrls` is on by default. Requests to `.html` / `/index` redirect 301 to their extensionless form; extensionless requests resolve to `<P>/index.html` then `<P>.html`.

```bash
curl -i http://127.0.0.1:3010/index.html         # 301 Location: /index
curl -i http://127.0.0.1:3010/about              # 200 (body of about.html)
```

`trailingSlash` and multi-slash normalization are controlled through `serve.json`:

```jsonc
{ "trailingSlash": true, "cleanUrls": false }
```

- `trailingSlash: true` → 301 from `/about` to `/about/`.
- `trailingSlash: false` → 301 from `/about/` to `/about`.
- Multi-slash paths (`//`, `/a//b`) silently collapse — no redirect.

## Redirects and rewrites

```jsonc
{
  "redirects": [
    { "source": "/old", "destination": "/new", "type": 302 },
    { "source": "/old-docs/:id", "destination": "/new-docs/:id" }
  ],
  "rewrites": [
    { "source": "/projects/:id/edit", "destination": "/edit-project-:id.html" },
    { "source": "/spa/**", "destination": "/index.html" }
  ]
}
```

Redirects fire before rewrites. `type` defaults to 301 and accepts any 3xx. Rewrites chain (capped at depth 64) and do not change the URL the client sees.

## SPA fallback

`--single` injects a synthetic `** → /index.html` rewrite at config-load time:

```bash
irserve --single --listen 3010 _tmp
curl -i http://127.0.0.1:3010/anything/deep    # 200 (body of index.html)
```

## Custom error pages

Drop `<status>.html` in the document root and irServe serves it on that status:

```bash
echo '<p>custom-not-found</p>' > _tmp/404.html
curl -i http://127.0.0.1:3010/missing                                 # 404, body of 404.html
curl -i -H 'Accept: application/json' http://127.0.0.1:3010/missing   # 404, JSON envelope (HTML page is HTML-only)
```

## Custom response headers

```jsonc
{
  "headers": [{
    "source": "**/*.css",
    "headers": [
      { "key": "Cache-Control", "value": "public, max-age=600" },
      { "key": "X-Custom", "value": "yes" }
    ]
  }]
}
```

Rules accumulate across matching globs; case-insensitive key override; `"value": null` prunes; rules do not apply to 3xx redirects.

## Caching

ETag is on by default. Capture it once, replay it with `If-None-Match`:

```bash
ETAG=$(curl -sI http://127.0.0.1:3010/asset.css | awk -F'"' '/^[Ee][Tt][Aa][Gg]:/ {print $2}')
curl -i -H "If-None-Match: \"$ETAG\"" http://127.0.0.1:3010/asset.css   # 304
```

`--no-etag` (or `{"etag": false}` in `serve.json`) replaces ETag with `Last-Modified`. irServe honors `If-Modified-Since` under that flag (irserve-only adaptation, see D-018):

```bash
irserve --no-etag --listen 3010 _tmp
LM=$(curl -sI http://127.0.0.1:3010/asset.css | awk -F': ' '/^[Ll]ast-[Mm]odified:/ {print $2}' | tr -d '\r')
curl -i -H "If-Modified-Since: $LM" http://127.0.0.1:3010/asset.css     # 304
```

There is no default `Cache-Control` header — only user `headers` rules emit one.

## Range requests

```bash
curl -i -H 'Range: bytes=0-3' http://127.0.0.1:3010/asset.css           # 206, content-range: bytes 0-3/<total>
curl -i -H 'Range: bytes=-4' http://127.0.0.1:3010/asset.css            # 206, last 4 bytes
curl -i -H 'Range: bytes=999-1000' http://127.0.0.1:3010/asset.css      # 416, content-range: bytes */<total>
```

`Range` pre-empts both 304 short-circuits. Comma-separated multi-ranges use the first segment only (D-019).

## CORS

`--cors` adds the four reference CORS headers (`access-control-allow-origin: *`, `-headers: *`, `-credentials: true`, `-private-network: true`) to every response, including 3xx redirects. `OPTIONS` is not short-circuited — it flows through the static pipeline, returning the file body, ETag, and the CORS headers all together.

## Compression

On by default. Negotiation order `br > gzip > deflate`; threshold 1024 bytes; MIME allowlist + `^text/|\+(?:json|text|xml)$/i` fallback. Disable with `-u` / `--no-compression`. See D-020 for the four declared divergences from `vercel/serve`'s `compression@1.8.1` middleware (framing, MIME table, q-rank, body bytes).

```bash
curl -i -H 'Accept-Encoding: gzip, deflate, br' http://127.0.0.1:3010/big.css
# 200 + vary: Accept-Encoding + content-encoding: br
```

## Directory listing

When the served path has no `index.html`, irServe emits a listing (HTML or JSON, content-negotiated):

```bash
curl -i http://127.0.0.1:3010/                                   # 200 text/html
curl -i -H 'Accept: application/json' http://127.0.0.1:3010/     # 200 application/json
```

Knobs:

- `{"directoryListing": false}` disables listings (the directory returns 404).
- `{"directoryListing": ["/docs/**"]}` scopes listings via globs.
- `{"unlisted": ["secret.txt"]}` hides files from listings (direct fetch still works); `.DS_Store` and `.git` are hidden by default.
- `{"renderSingle": true}` serves the single file in a directory directly instead of listing it.

## CLI reference (short)

| Flag | Effect |
|---|---|
| `-l/--listen <port|host:port|tcp://host:port>` | Bind. Repeat the flag to bind multiple sockets. |
| `-p <port>` | Deprecated alias for `--listen`. |
| `-c/--config <file>` | Alternate `serve.json` path. |
| `--single` | SPA fallback (synthetic `**` rewrite). |
| `--cors` | Add the four reference CORS headers. |
| `--no-etag` | Replace ETag with `Last-Modified`; enables IMS. |
| `--no-compression` (`-u`) | Disable HTTP compression. |
| `-d/--debug` | Add elapsed-ms suffix to the per-request log. |
| `-L/--no-request-logging` | Silence per-request logs. |
| `--no-port-switching` | Fail (non-zero exit) on `EADDRINUSE` instead of falling back to another port. |
| `-h/--help`, `-v/--version` | Standard. |

`--no-clipboard` is accepted and silently ignored (clipboard side effect is intentionally not implemented per D-005).
