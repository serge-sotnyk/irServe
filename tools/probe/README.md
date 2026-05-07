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

## Snapshot mode

Probes can be paired with a committed canonical snapshot under
`tools/probe/snapshots/<id>.json`. Snapshots are the Stage-3 artifact-of-record
for what the pinned reference does — see
[`docs/reference/serve/oracle-matrix.md`](../../docs/reference/serve/oracle-matrix.md).

```bash
# Capture / refresh the canonical snapshot for one probe
node tools/probe/run.mjs <probe-id> --snapshot=update

# Verify all probes against their committed snapshots
node tools/probe/run.mjs --all --snapshot=verify

# Force the legacy behavior (only writes results/)
node tools/probe/run.mjs <probe-id> --snapshot=none
```

Without `--snapshot=`, the runner auto-selects: `verify` when a snapshot
exists, `none` otherwise. A snapshot mismatch surfaces as a non-zero exit
with a line-by-line diff of the prettified expected vs actual snapshot.

The snapshot schema is `tools/probe/snapshots/_schema.json`. Recorded fields:
status, response statusText, the same allowlisted headers as the result
file, body shape (kind + length + sha256 + first 200 utf-8 bytes for
textual responses), and (in raw mode) the literal request line. The
`volatileHeaders` list (default: `["last-modified"]`) names headers whose
values are recorded but not asserted during `verify`. Override per case via
`cases/<id>.json#snapshot.volatileHeaders` when, for example, an `etag`
value depends on file mtime.

Snapshots are intentionally hand-edit-hostile: if a diff looks wrong, the
fix is upstream of the snapshot — adjust the case, the runner, or the
fixture, then re-run with `--snapshot=update`.

### Cross-platform note

Snapshots were captured on Windows. The runner normalizes the absolute
fixture directory to `<FIXTURE_ROOT>` in body content, but it does NOT
collapse OS-specific path separators (`\` vs `/`) or drive-letter
prefixes (`C:\` vs `/`) inside response bodies. Two cases —
`listing-unlisted` and `cache-control-default` — embed listing JSON/HTML
that contains those OS markers, so `--snapshot=verify` will diff their
body fields on POSIX. See `docs/reference/serve/oracle-matrix.md`
"Cross-platform note" for the policy.

CLI snapshots (`stdout` / `stderr` summaries) record full
length/sha256/preview on disk for audit, but `verify` masks those fields
— only `exitCode` and stream `kind` (text/empty/binary) are asserted.
This keeps the contract aligned with `D-002` (exact terminal output is
not part of compatibility).

## Conventions

- Probe ids prefixed with `_` (e.g. `_smoke`) are infrastructure, not behavior evidence.
- A probe id should be referenced from one or more SRV-* entries in `docs/reference/serve/inventory.md`. The inventory entry's `Reference source` line should read `Probe: tools/probe/cases/<id>.json`.
- Result files (`tools/probe/results/`) are gitignored — they are reproducible and verbose. Snapshot files (`tools/probe/snapshots/`) are committed and serve as the canonical record.
- The runner is dependency-free (Node 18+ built-ins only). Do not introduce npm dependencies into this directory.

## Limits

This runner freezes one side of the comparison — the reference. It does
not:

- run a Rust implementation against the same fixtures (Stage 5b concern),
- decode `Transfer-Encoding: chunked` in raw mode,
- run in CI.

`--snapshot=verify` proves that the recorded reference behavior is still
reproducible against the pinned `third_party/serve`; it does **not** prove
that `irServe` matches anything. The Rust-side comparison lands at Stage
5b together with `tests/oracle/`.
