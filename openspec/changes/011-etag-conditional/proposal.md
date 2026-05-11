# Proposal: ETag + 304 conditional GET

## Why

Stage 7a is the first sub-stage of Stage 7 (L3 polish) per
`docs/stage7_l3_capabilities.md` and the next row in the
README stage map. It closes **SRV-CACHE-001** (P0): file
responses carry a strong `ETag` header by default, and a request
with matching `If-None-Match` short-circuits to `304 Not Modified`
with no body. After this stage, the runner's ORC-042 / ORC-043
rows run under `target=irserve` as well as `target=reference` —
the prior probe `etag-roundtrip.json` pinned a static
`If-None-Match` value that could never match a freshly-computed
hash under `target=irserve`, so a runner-level capture-replay
extension was prerequisite to flipping these rows to dual-target.

Stages 7b (`Last-Modified` + `--no-etag` + `If-Modified-Since`)
and 7c (Range requests) build on the same response-emission slot
this stage establishes; the structural decisions here (where the
ETag is computed, when the 304 short-circuit fires, how the
header sits relative to user `serve.json#headers` rules)
propagate forward.

This change closes:

- **SRV-CACHE-001** (`ETag` is sent by default and supports 304)
  — already `verified` in `inventory.md` from Stage 1 evidence;
  Stage 7a delivers the irserve-side wire surface. Mirrors
  `serve-handler/src/index.js:227-233` (etag computation) and
  `:758-765` (304 short-circuit).

It also lands a methodologically reusable extension:

- **Probe runner capture-replay.** `tools/probe/run.mjs` now
  accepts request-header values shaped
  `{"$fromResponse": {"request": "<name>", "header": "<lc>"}}`
  and substitutes the named prior response's header at request
  time, target-agnostically. This is the prerequisite for any
  conditional-GET probe under `target=irserve` (the hash differs
  per implementation; static `If-None-Match` literals would never
  match). The same mechanism is reusable for 7b
  (`If-Modified-Since`) and 7c (`If-Range`).

## What

- **`compute_etag` helper.** New `crates/irserve-core/src/etag.rs`
  exposes `compute_etag(path: &Path, bytes: &[u8]) -> String`
  returning the strong-quoted form `"\"<40 hex chars>\""`. Hash
  is `sha1(extname.as_bytes() ++ b"-" ++ bytes)` — exact mirror
  of `serve-handler/src/index.js:24-36`. The choice of `sha1`
  (and the `extname + '-' + bytes` framing) is recorded as
  **D-017**. Unit tests pin the value
  `"\"3638b78821a961fcf35969f0bc67cc5944d64a0b\""` for the
  fixture `asset.css` with body `body{color:red}\n` — proving
  byte-equality with the reference's pinned snapshot in
  `etag-roundtrip.json`.

- **ETag on 200 file responses.** `file_response` at
  `crates/irserve-core/src/dispatch.rs` gains an
  `etag: Option<HeaderValue>` parameter; when `Some`, the header
  is inserted into the response. Both call sites (the regular
  File/Index arm at `dispatch.rs:320`-ish and the renderSingle
  branch at `dispatch.rs:430`-ish) pass `etag_value(serve_config,
  path, bytes)` — which returns `Some` unless `serve.json`
  explicitly sets `"etag": false`. Mirrors `vercel/serve`'s CLI
  default (`config.etag = !args['--no-etag']` in
  `third_party/serve/source/main.ts`); under irserve, the CLI is
  the only entry point, so the default-true semantics ride on
  every invocation that does not opt out via `serve.json`.

- **304 short-circuit.** A new helper `build_file_or_304(...)`
  builds the candidate 200 (with the default ETag, when
  enabled), applies user `headers` rules to it via the existing
  `apply_custom_headers`, then compares the MERGED response's
  `ETag` against the request's `If-None-Match`. On a match (and
  `Range` absent), emits 304 (no body, no `Content-Type`, no
  `ETag` echo — `serve-handler/src/index.js:761-764`); otherwise
  returns the merged 200. The Range guard is a 7c precursor —
  Range parsing itself is deferred — and mirrors the
  reference's `req.headers.range` check at `index.js:760`. The
  merge-before-decide ordering mirrors the reference's
  `getHeaders` (`index.js:194-254`) immediately followed by the
  304 check at `:760` against the merged `headers.ETag`, and was
  reshaped in Codex review round 1 P1 (the prior implementation
  compared against the default sha1, breaking the round-trip
  contract for any deployment overriding `ETag` via `headers`).
  Seven unit tests in `dispatch::tests` pin: match → 304,
  mismatch → 200, Range present + match → still 200, ETag
  disabled → never 304, no `If-None-Match` → 200 + ETag, custom
  `ETag: "custom"` override drives the 304 decision (304 only
  on `"custom"`, 200 on the default sha1), and `ETag: null`
  delete suppresses 304 (SRV-HDR-002 prune).

- **Probe runner capture-replay.** `tools/probe/run.mjs` and the
  case schema extended so request-header values may be either a
  string or an object `{"$fromResponse": {"request": "<name>",
  "header": "<lc>"}}`. Resolution is lazy: when issuing request
  N, the runner looks up the captured response for the named
  prior request and substitutes the header value (verbatim
  string, including quotes). If the named request has no recorded
  response yet, the runner fails loudly with the case id. The
  `etag-roundtrip.json` case now uses this for the second
  request's `If-None-Match`; both etag probes carry `runner.l0`
  partitions and run under both targets. The oracle harness goes
  from `73 / 8 skipped` to `75 / 6 skipped` under
  `target=irserve` (`etag-roundtrip` two requests promoted off
  the deferred list).

- **Documentation updates.** New `openspec/specs/http-cache/`
  capability (first L3 capability shipped). New **D-017** in
  `docs/reference/serve/decisions.md`. ORC-042 / ORC-043
  flipped to dual-target coverage in
  `docs/reference/serve/oracle-matrix.md` (D-017 cross-link in
  Verifies / Layer columns). README's stage-7a row flipped to
  `done`, the "What is NOT yet observable" footer loses `ETag`,
  and the "Try IrServe" section gains a curl-based ETag /
  conditional-GET demo. SRV-CACHE-001 in `inventory.md` is
  untouched — it was already `verified` from Stage 1 evidence
  and Stage 7a delivers the irserve-side surface that the
  existing Compatibility note already anticipates.

## Out of scope

Mirrors `docs/features/0015_PLAN_stage7a_etag_and_conditional_get.md`'s
pre-stage out-of-scope list (anti-hallucination rule #10):

1. **`--no-etag` CLI flag** — Stage 7b. `serve.json`'s
   `"etag": false` IS honored; the CLI flag is not parsed yet.
2. **`Last-Modified` emission and `If-Modified-Since` 304**
   (closes Q-009) — Stage 7b.
3. **Range parsing and `206`/`416`** — Stage 7c. Only the
   `Range`-absent guard for the 304 check lands now.
4. **ETag on custom HTML error pages.** The reference applies
   ETag to custom `<status>.html` responses when `etag=true`
   (`serve-handler/src/index.js:508`). Stage 7a defers; documented
   as a Compatibility note on the new requirement.
5. **In-memory ETag cache** (the reference's
   `Map<absPath, [mtime, sha]>` at `index.js:22` consulted at
   `:228-231`). Hash on every request; future optimization,
   not contractual. Captured in D-017.
6. **Weak ETags (`W/"..."`).** Reference does not emit weak ETags
   on file responses; irserve mirrors.
7. **Comma-separated `If-None-Match` lists and wildcard `*`.**
   Reference does literal string equality only
   (`serve-handler/src/index.js:760`); irserve mirrors.
8. **ETag on directory listings, 3xx redirects, JSON error
   responses.** Neither reference nor irserve emits ETag on these
   paths.
