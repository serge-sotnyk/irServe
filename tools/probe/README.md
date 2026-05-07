# tools/probe

Ad-hoc probe runner used during reverse-engineering of `vercel/serve` (Stages 1–3, before the Rust oracle harness exists).

A probe is a declarative JSON case under `cases/<id>.json`. It describes:

- a fixture (inline files plus optional `serve.json`),
- extra CLI args to pass to `serve`,
- an ordered list of HTTP requests to issue.

The runner picks a free port, materializes the fixture into a temp directory, spawns the pinned `node third_party/serve/build/main.js`, executes each request with `redirect: manual`, captures status + selected headers + body summary (length, sha256, ≤200-byte UTF-8 preview when textual), and writes a deterministic Markdown + JSON report to `results/<id>.{md,json}`.

Two request transports are available, selected per-request via the `mode` field:

- `mode: "fetch"` (default) — uses Node's built-in `fetch`. Handy and high-level, but the URL parser normalizes `..`, decodes `%2e%2e`, and collapses `//` before the request reaches the wire. Most behavior probes use this.
- `mode: "raw"` — opens a `net.Socket` and writes the literal HTTP request line and headers bytes verbatim. The runner injects `Host` and `Connection: close` only if the case did not. Use this for wire-level probes (path traversal, malformed paths). The result file's `requestLine` field shows exactly what was sent. Limitations: the parser handles `Content-Length`-framed or connection-close-framed responses only; `Transfer-Encoding: chunked` is not decoded.

## Usage

```bash
# Single probe
node tools/probe/run.mjs _smoke

# All probes under cases/
node tools/probe/run.mjs --all

# List discovered case ids
node tools/probe/run.mjs --list
```

Set `PROBE_VERBOSE=1` to also dump `serve` stderr after each run.

## Conventions

- Probe ids prefixed with `_` (e.g. `_smoke`) are infrastructure, not behavior evidence.
- A probe id should be referenced from one or more SRV-* entries in `docs/reference/serve/inventory.md`. The inventory entry's `Reference source` line should read `Probe: tools/probe/cases/<id>.json`.
- Result files are gitignored. The probe is the source of truth; results are reproducible.
- The runner is dependency-free (Node 18+ built-ins only). Do not introduce npm dependencies into this directory.

## Limits

This is not a full oracle harness. It does not:

- compare two implementations,
- diff against a recorded snapshot,
- run in CI.

Those are Stage 5 concerns. The probe runner is intentionally one-sided: it documents what `serve` does, not whether `irServe` matches.
