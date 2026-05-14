# Design: HTTP compression (`-u` / `--no-compression`)

This document records the architecture for Stage 7e. 7e is
the fifth and final L3 sub-stage and the closure of
SRV-CLI-012, Q-002, and the D-006 deferral. The
architectural foundations (crate layout, HTTP-stack pins,
request lifecycle, 13-phase dispatcher, oracle harness
layer) live in
`openspec/changes/001-port-minimal-static-server/design.md`;
the Stage-6f user-headers seam at
`apply_custom_headers` (`crates/irserve-core/src/custom_headers.rs:178-221`),
the Stage-6h CORS overlay at `apply_cors`
(`crates/irserve-core/src/cors.rs:26-37`), the Stage-7a
ETag / 304 path at `dispatch.rs::build_file_or_304`, the
Stage-7b `Last-Modified` / IMS branch, and the Stage-7c
`range::apply` integration are all reused verbatim. 7e
adds one new module (`compression`), one CLI flag, one
`ServeConfig` field, and one call-site in the dispatcher.
A new **D-020** decision-log entry enumerates the four
deliberate divergences from the reference's wire surface.

## §1. Reference behavior

The reference wires `compression@1.8.1` via Express
middleware at
`third_party/serve/source/utilities/server.ts:8,25,71-72`:

```ts
// L8 (import)
import compression from 'compression';

// L25 (one-time middleware materialization at module load)
const compress = promisify(compression());

// L71-72 (per-request gate)
if (!args['--no-compression'])
  await compress(request as ExpressRequest, response as ExpressResponse);
```

The CLI flag is declared at `cli.ts:53,154,171`:

- L53 — clap-style flag enumeration: `-u, --no-compression`.
- L154 — long-form declaration: `'--no-compression'`.
- L171 — short alias mapping: `'-u': '--no-compression'`.

`serve-handler` does **not** compress (greppable
confirmation — zero references to `compression` /
`gzip` / `Accept-Encoding` in the handler library). Stage
7e is a `serve`-CLI-only feature; the handler library is
untouched.

The `compression@1.8.1` middleware itself lives at
`third_party/serve/node_modules/compression/index.js`.
The pinned empirical surface (captured into
`tools/probe/snapshots/compression-raw.json` via 20
raw-socket anchors in slice 0) is:

- **Default body-size threshold = 1024 bytes**
  (`compression/index.js:76-78`). Bodies strictly below
  the threshold are not compressed; `Vary` is still
  emitted on compressible MIMEs.

- **Negotiation preference = `br > gzip > deflate`** when
  Node has brotli support
  (`compression/index.js:44-45`'s `PREFERRED_ENCODING`
  array; the vendored bundle has brotli, Node 11+).
  Identity is the implicit fallback. `q=0` and `*;q=0`
  are honored as exclusions.

- **`Vary: Accept-Encoding` invariant** — set IFF the
  content-type is compressible AND `Cache-Control:
  no-transform` is NOT present on the response. Once
  set, `Vary` stays on the response even if compression
  is ultimately skipped for HEAD / below-threshold /
  identity-only / all-q-zero reasons. The early-skip
  paths (no-transform, non-compressible MIME) do NOT
  set `Vary`.

- **Compressible MIMEs verified compressed**:
  `text/html`, `text/css`, `application/javascript`,
  `application/json`, `application/wasm`,
  `image/svg+xml`.

- **Non-compressible MIMEs verified passthrough (no
  `Vary`)**: `image/png`, `font/woff2`, `video/mp4`.

- **Skip conditions**:
  - HEAD method → middleware short-circuits at
    `compression/index.js:192-195`, but `Vary` is set
    just above at `:175` so it rides on the HEAD
    response.
  - `Cache-Control: no-transform` on the response → skip
    via `compression/index.js:293-300`. No `Vary` set.
  - Body strictly below the threshold → skip with
    `Vary` retained.
  - `negotiate` returns "identity-only" or `*;q=0` →
    skip with `Vary` retained.

- **OPTIONS routes through the GET pipeline** (Stage 7d
  composition — `serve-handler` does no method
  branching), so OPTIONS responses with `Accept-Encoding`
  above threshold ARE compressed. Verified by the
  `options_big_html_gzip` anchor.

- **Range pre-empts compression entirely**. A 206
  response carries the raw body slice + `Content-Range`
  and NO `Content-Encoding`; `Vary: Accept-Encoding`
  IS set (the negotiation hook engaged before Range
  short-circuited). Verified by the `range_big_html_gzip`
  anchor.

The full empirical anchor list lives at
`tools/probe/cases/compression-raw.json`'s
`requests` array (20 anchors). Slice 0's captured
snapshot is the pinned upstream contract for all four
encoder paths.

## §2. Negotiation algorithm in irserve

`crates/irserve-core/src/compression.rs::negotiate` takes
an `Option<&HeaderValue>` and returns
`Option<Encoding>` where `Encoding` is one of
`Brotli`, `Gzip`, `Deflate`. The control flow is:

1. **Tokenize** the header by `,` and trim each token.
2. **Per token**, split off the `q=N` suffix (case-
   insensitive `Q=`); the head is the encoder name
   (`br` / `gzip` / `deflate` / `*` / anything else,
   case-folded). The tail's `q` value parses via
   `f32::from_str`; failures yield `None`.
3. **Accumulate** allow / reject flags per known
   encoder name plus the two wildcard variants.
   `q=0` flips the per-encoder reject flag; non-zero
   or absent `q` flips the allow flag.
4. **Resolve**: for each of `Brotli`, `Gzip`,
   `Deflate` in preference order, compute
   `ok = (explicit_allow || (wildcard_accept &&
   !explicit_q0)) && !explicit_q0`. If
   `wildcard_reject && !allow_br && !allow_gzip &&
   !allow_deflate`, short-circuit to `None`. Otherwise
   return the first ok encoder, or `None` if none
   matched.

Tokens like `identity`, `compress`, `x-gzip`, etc. fall
through the `match` and are silently ignored — the
implicit identity fallback means an absent or
all-unknown header simply returns `None`.

The only documented divergence from the reference's
`Negotiator` package is **q-rank within `(0, 1)` not
honored**: irserve treats any non-zero `q` as accept
and does not order encoders by `q` rank. Real-world
`Accept-Encoding` values are all-equal-priority; this
is D-020 #3 above.

Unit tests in `compression::tests` pin the corners
(missing header, identity-only, all-q-zero, wildcard
accept, wildcard reject, preference order br > gzip >
deflate, `q=0` excludes).

## §3. MIME filter (D2)

`crates/irserve-core/src/compression.rs::is_compressible`
splits the content-type value at `;`, trims and
case-folds the MIME portion, and decides
compressibility via:

1. **Curated allowlist** of exact matches drawn from
   the `mime-db@1.33.0` `compressible: true` set
   restricted to MIMEs that appear in real production
   traffic: `application/json`,
   `application/javascript`, `application/wasm`,
   `image/svg+xml`.
2. **Regex fallback** per `compressible/index.js:23`:
   anything starting with `text/`, OR anything whose
   `+`-suffix is `json` / `text` / `xml`. Implemented
   as `mime.starts_with("text/") || ((+idx) suffix ∈
   {"json", "text", "xml"})`.

What is intentionally NOT ported: the full
`mime-db@1.33.0` `compressible` table (~150 entries).
The reference's `compressible@2.0.18` package falls
back to that table for MIMEs the regex doesn't catch
(e.g. `application/postscript`, `application/xml-dtd`,
`application/x-perl`). irserve diverges here as
**D-020 #2**.

`mime_for` (`crates/irserve-core/src/mime.rs`) is NOT
re-derived inside the compression module — the module
takes the `Content-Type` value as a string straight
off the response's headers (after
`apply_custom_headers`), so a user `headers` rule that
overrides the content-type drives the compressibility
decision.

## §4. Dispatcher seam

`crates/irserve-core/src/dispatch.rs::build_file_or_304`
is the integration point. The seam slots
`compression::maybe_apply` AFTER `apply_custom_headers`
(so the merged final `Content-Type` and
`Cache-Control` are visible to the negotiator) and
BEFORE Range pre-emption — concretely, the Range branch
runs INSIDE `build_file_or_304` ahead of the
compression call, so a `Range`-bearing request never
enters `maybe_apply`. The call site reads (mirroring
the comment at `dispatch.rs::build_file_or_304`):

```rust
// SRV-CLI-012 (Stage 7e): compression negotiation. Slots in
// AFTER the 304 short-circuits (no point compressing a body
// we won't send) and AFTER apply_custom_headers so the merged
// final Content-Type / Cache-Control drive the decision.
// Range pre-empts compression upstream (matches the reference's
// middleware ordering: compression sees the 206 body framing
// as opaque and skips).
return compression::maybe_apply(
    merged,
    &bytes_for_compression,
    req_headers,
    method,
    serve_config,
);
```

The bytes that feed `maybe_apply` are snapshotted before
the `Body` move into `file_response` (the comment at the
snapshot site enumerates the constraint:
"`compression::maybe_apply` below. We can't recover them
from the `Body` after `file_response` moves them in, and
the Range branch already takes its own clone"). Range's
own clone is unaffected.

`maybe_apply` itself is the only mutating call in the
module. Its gate ordering, top-to-bottom:

1. `serve_config.compression == Some(false)` →
   return response unchanged. No `Vary` emitted —
   pre-7e parity.
2. `!is_compressible(content_type)` → return unchanged.
   No `Vary` (the middleware never sets it on
   non-compressible types).
3. `Cache-Control: no-transform` present on the
   merged response → return unchanged. No `Vary`.
4. **Vary set if missing** (set-only-if-missing
   semantics — see Compatibility note #5 in the
   capability spec).
5. `method == HEAD` → return with `Vary` set, body
   untouched. Axum / hyper strip the HEAD body on the
   wire.
6. `bytes.len() < DEFAULT_THRESHOLD` (1024) → return
   with `Vary` set, body untouched.
7. `negotiate(accept_encoding) == None` → return with
   `Vary` set, body untouched (identity-only or
   all-q-zero).
8. Otherwise: encode `bytes` via the chosen encoder,
   set `Content-Encoding: <token>`, set
   `Content-Length: <compressed-len>`, replace body.
   `Vary` was set in step 4.

The order mirrors the reference's `compression`
middleware exactly EXCEPT for step 8's framing — the
reference emits chunked transfer-encoding without
`Content-Length`. Documented as **D-020 #1**.

CLI propagation: `pub compression: Option<bool>` lives
on `ServeConfig` in
`crates/irserve-core/src/config.rs` with
`#[serde(skip)]` — the field is not exposed via
`serve.json` (reference's `compression` is CLI-only).
The flag-state carrier path is identical to `etag`'s:
`main.rs` parses `-u` / `--no-compression`, the
post-parse override at `main.rs` forces
`serve_config.compression = Some(false)` when the flag
is set; absence leaves it `None` (= compress by
default, mirroring the reference's middleware engaging
unconditionally).

## §5. Probe-runner adaptations

Two extensions of the slice-0 / slice-2 vintage:

1. **`content-encoding` tracking (slice 0).** Added
   `'content-encoding'` to `TRACKED_RESPONSE_HEADERS`
   in `tools/probe/run.mjs`. Previously the header was
   silently stripped by the runner's tracked-set
   filter, so reference snapshots that legitimately
   carry `Content-Encoding` (`etag-roundtrip` first
   GET on a compressible asset above threshold,
   `last-modified-roundtrip#first_get` on the same
   shape, the `range-request` non-Range anchors, etc.)
   were missing it. Re-recorded 21 such legacy
   snapshots in slice 0 to lock the new contract.

2. **`bodyMayDiffer` extended to strip
   `content-encoding` (slice 2).** The runner's L0
   masking overlay for `bodyMayDiffer`-listed anchors
   already stripped `content-length` (since a
   may-differ body trivially has a may-differ length);
   slice 2 extended it to also strip
   `content-encoding`. The rationale: the
   `compression::maybe_apply` decision is a function
   of `bytes.len() vs threshold`, so if the body bytes
   may differ across targets the compression decision
   may also differ — and the `content-encoding`
   header therefore can't be part of the L0 contract
   on a `bodyMayDiffer` anchor.

3. **`runner.l0.clean` blocks on the two compression
   probes (slice 2).** `compression-default.json`
   gains `clean: ["with_accept_encoding"]` (a
   single-anchor Vary-only check that's identical in
   both targets). `compression-raw.json` gains the
   full `clean: [...]` block over all 20 anchors plus
   `bodyMayDiffer: [...]` over the 10 compressed
   anchors per D-020 #4. The slice 0 temporary
   `content-encoding` mask in
   `L0_EXTRA_VOLATILE_HEADERS` was dropped — the
   tracker + masking overlay together fence
   compression's body-bytes divergence cleanly.

The runner's `runRequestRaw` pathway (`tools/probe/run.mjs`)
is reused verbatim from Stage 6f; no new probe-runner
infra was needed.

## §6. D-020 divergences (inline summary)

The canonical entry is in
`docs/reference/serve/decisions.md`. Reproduced here in
brief for spec-delta cross-reference:

1. **Framing**: irserve sends
   `Content-Length: <compressed-len>` on compressed
   responses; reference uses `Transfer-Encoding:
   chunked`. irserve has the final bytes in memory
   (static-file model), reference compresses streaming.

2. **No `mime-db` `compressible` table port**:
   irserve uses a curated allowlist + regex fallback;
   reference consults the full ~150-entry mime-db
   compressible flag.

3. **q-rank within `(0, 1)` not honored**: irserve
   treats `q=0` as exclusion and non-zero `q` as
   accept; reference's `Negotiator` ranks fractional
   q-values.

4. **Compressed body bytes are NOT byte-identical**:
   both sides produce valid encodings; encoder
   defaults (Node `zlib` vs `flate2`, Node brotli vs
   `brotli` crate) differ at the bit level. The
   probe runner's per-anchor `bodyMayDiffer` overlay
   strips `content-encoding` and `content-length`
   from the L0 contract and masks body bytes on the
   10 compressed anchors.

All four are documented in:

- `docs/reference/serve/decisions.md` D-020 (canonical,
  with full reason / impact paragraphs).
- `openspec/specs/http-compression/spec.md`
  Compatibility notes (capability-spec form).
- The proposal's "Out of scope" section above.

There is no `D-021` candidate at the time of
authoring; the four divergences exhaust the declared
parity-scope boundary. Future work that closes any one
of them (e.g. mirroring `mime-db`'s full table)
flips D-020 from `adapted` to `mirrored` and removes
the corresponding bullet.

## §7. Why this stage shape

The plan-mode AskUserQuestion forks resolved:

- **D1 — Encoder set.** Brotli + gzip + deflate full
  parity over "gzip-only" or "gzip + deflate". Cost: one
  extra crate (`brotli`). Avoids a `D-NNN brotli
  omission` divergence that clients with
  `Accept-Encoding: br, gzip` would hit byte-for-byte.
- **D2 — MIME filter.** Curated allowlist + regex
  fallback over the full `mime-db` port. The full table
  port adds ~150 entries with no production-traffic
  surface; the curated + regex covers every MIME
  observed in the slice-0 probe.
- **D3 — D-020 divergence sketch.** Pre-drafted in
  plan-mode and confirmed empirically after slice 0
  (framing observable on raw-socket capture; mime-db
  port is the absent-from-source decision; q-rank
  divergence is internal to `negotiate`'s
  implementation; body-bytes divergence observable in
  the snapshot).

There is no D-021 candidate for Range × compression
interaction — slice 0's `range_big_html_gzip` anchor
confirmed the reference's Range pre-empts
compression (option (a) in the plan-mode risk list:
the reference DOES NOT compress 206 responses). The
irserve implementation mirrors via the dispatcher
ordering: `range::apply` runs inside
`build_file_or_304` ahead of the
`compression::maybe_apply` call.
