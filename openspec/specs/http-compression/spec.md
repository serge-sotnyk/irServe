# http-compression Specification

## Purpose

HTTP compression for static-file responses under the `-u` /
`--no-compression` CLI flag. Covers the encoder set
(`br > gzip > deflate` in preference order), the
1024-byte body-size threshold, the MIME filter (curated
allowlist + regex fallback), the `Vary: Accept-Encoding`
semantics, the skip conditions (HEAD,
`Cache-Control: no-transform`, below-threshold,
identity-only, all-`q=0`), and the Range-pre-empts-
compression interaction. Introduced in Stage 7e (commit
`d1b9a15`); reference behavior pinned empirically in slice
0 via 20 raw-socket anchors in
`tools/probe/snapshots/compression-raw.json` (commit
`9bd52f0`, closing Q-002).

This capability's surface is L3 (verified-behavior
catalogue). The L1 baseline Requirement on the `-u` /
`--no-compression` CLI flag itself (just flag-presence +
the default-on / default-off effect) lives in
`openspec/specs/cli/spec.md` (SRV-CLI-012) and is NOT
duplicated here.

Background: the feature was wired but no-op pre-7e per
**D-006** ("HTTP compression is L3-priority, not
MVP-mandatory"). 7e implements the feature and adds
**D-020** to record the four deliberate divergences from
the reference's `compression@1.8.1` middleware. D-006
stays in `docs/reference/serve/decisions.md` as a
historical record; its rationale is superseded by 7e
closing the L3 stretch goal.

## Requirements

### Requirement: HTTP compression engages on compressible-MIME responses above a 1024-byte body-size threshold

When the server is run without `-u` / `--no-compression`,
a response whose final merged content-type is
**compressible** (per the MIME filter below) and whose
body is **strictly at or above 1024 bytes** SHALL be
encoded using the highest-preference encoder the client's
`Accept-Encoding` admits. The supported encoders, in
preference order, SHALL be `br` (brotli), `gzip`, and
`deflate`. Identity is the implicit fallback. When the
negotiation yields no acceptable encoder (header absent,
identity-only, all-`q=0`), the response body SHALL NOT
be encoded; `Vary: Accept-Encoding` SHALL still be set
on the response (the negotiation hook engaged).

The 1024-byte threshold mirrors the reference's
`compression@1.8.1` default at
`third_party/serve/node_modules/compression/index.js:76-78`;
the preference order mirrors `PREFERRED_ENCODING` at
the same module's `:44-45`. Encoder selection honors
`q=0` exclusions and the `*` wildcard accept / reject
tokens; q-values strictly between 0 and 1 are NOT
ranked (D-020 #3 — Compatibility notes below).

When compression engages, the response SHALL carry
`Content-Encoding: <token>` (`br` / `gzip` / `deflate`),
the body SHALL be replaced with the encoded bytes, and
`Content-Length` SHALL be overwritten with the encoded
body length. `Vary: Accept-Encoding` SHALL also be
present (set earlier in the negotiation hook). irserve
DOES NOT use `Transfer-Encoding: chunked` on compressed
responses; the reference does (D-020 #1 —
Compatibility notes below).

Evidence: SRV-CLI-012 (status: verified, level: L3);
oracle: ORC-191 (`compression-raw.json#big_html_gzip_deflate_br`),
ORC-192 (`#big_css_gzip_deflate_br`), ORC-193
(`#big_js_gzip_deflate_br`), ORC-194
(`#data_json_gzip_deflate_br`), ORC-195
(`#vector_svg_gzip_deflate_br`), ORC-196
(`#wasm_gzip_deflate_br`); per-encoder anchors ORC-198
(`#big_html_gzip`), ORC-199 (`#big_html_deflate`).
Closes Q-002 (resolution dated 2026-05-14 in
`docs/reference/serve/open-questions.md`).

Implementation:
`crates/irserve-core/src/compression.rs::maybe_apply`
is the dispatcher seam called from
`build_file_or_304` after `apply_custom_headers`. The
gate order (mirroring `compression/index.js`):
1. `serve_config.compression == Some(false)` — return
   unchanged (no `Vary`).
2. `!is_compressible(content_type)` — return unchanged
   (no `Vary`).
3. `Cache-Control: no-transform` — return unchanged
   (no `Vary`).
4. **Set `Vary: Accept-Encoding`** if missing.
5. `method == HEAD` — return with `Vary` set; the
   HTTP layer strips the body on the wire.
6. `bytes.len() < 1024` — return with `Vary` set.
7. `negotiate(accept_encoding) == None` — return
   with `Vary` set.
8. Encode + set `Content-Encoding` + overwrite
   `Content-Length` + replace body.

#### Scenario: Compressible MIME above threshold compresses

- GIVEN `serve` (defaults) over a directory containing
  `big.html` whose body is > 1024 bytes
- WHEN `GET /big.html` with
  `Accept-Encoding: gzip, deflate, br`
- THEN status is 200
- AND the response carries `Vary: Accept-Encoding`
- AND the response carries `Content-Encoding: br`
- AND the response body is the brotli-encoded form
  (body bytes not contractual across targets —
  `bodyMayDiffer` per D-020 #4)

#### Scenario: Preference order — gzip wins when brotli excluded

- GIVEN the same fixture
- WHEN `GET /big.html` with
  `Accept-Encoding: gzip;q=0.9, br;q=0`
- THEN status is 200
- AND the response carries `Vary: Accept-Encoding`
- AND the response carries `Content-Encoding: gzip`
  (brotli excluded via `q=0`; gzip preferred over
  deflate)

#### Scenario: Compressible MIME below threshold gets `Vary` but no compression

- GIVEN `serve` (defaults) over a directory containing
  `tiny.html` (< 1024 bytes)
- WHEN `GET /tiny.html` with
  `Accept-Encoding: gzip, deflate, br`
- THEN status is 200
- AND the response carries `Vary: Accept-Encoding`
- AND the response does NOT carry `Content-Encoding`
- AND the response body is the raw file contents

### Requirement: Compressibility is decided by the MIME filter — curated allowlist + regex fallback

A response's content-type SHALL be considered
**compressible** if and only if the case-folded MIME
(content-type with parameters stripped) matches either
the curated allowlist or the regex fallback:

- **Allowlist** (exact match):
  - `application/json`
  - `application/javascript`
  - `application/wasm`
  - `image/svg+xml`
- **Regex fallback**: the MIME starts with `text/`,
  OR its `+`-suffix is one of `json`, `text`, `xml`
  (`^text/|\+(?:json|text|xml)$`, case-insensitive,
  per `compressible/index.js:23`).

A non-compressible MIME SHALL be returned by
`maybe_apply` unchanged with NO `Vary: Accept-Encoding`
emitted — mirroring the reference's `compression`
middleware, which only sets `Vary` once the
compressibility check passes.

irserve does NOT consult the full `mime-db@1.33.0`
`compressible` flag (~150 entries) that the reference's
`compressible@2.0.18` falls back to for long-tail
MIMEs that escape the regex (e.g.
`application/postscript`, `application/xml-dtd`). The
divergence is recorded as **D-020 #2**.

Evidence: SRV-CLI-012 (status: verified, level: L3);
oracle: ORC-200..ORC-202 — `compression-raw.json`
non-compressible anchors `#image_png_gzip`,
`#font_woff2_gzip`, `#media_mp4_gzip`. Compressible
allowlist coverage at ORC-191..ORC-196 (`text/html`,
`text/css`, `application/javascript`,
`application/json`, `image/svg+xml`,
`application/wasm`).

Implementation:
`crates/irserve-core/src/compression.rs::is_compressible`
case-folds the content-type, splits at `;` to drop
parameters, then runs the allowlist match followed by
the prefix-`text/` test and the `+`-suffix test. No
`mime-db` consultation.

#### Scenario: PNG image is non-compressible

- GIVEN `serve` (defaults) over a directory containing
  `image.png`
- WHEN `GET /image.png` with
  `Accept-Encoding: gzip, deflate, br`
- THEN status is 200
- AND the response does NOT carry `Vary`
- AND the response does NOT carry `Content-Encoding`
- AND the response body is the raw PNG bytes

#### Scenario: WOFF2 font is non-compressible

- GIVEN the same fixture with `font.woff2`
- WHEN `GET /font.woff2` with
  `Accept-Encoding: gzip, deflate, br`
- THEN status is 200
- AND the response does NOT carry `Vary`
- AND the response does NOT carry `Content-Encoding`

#### Scenario: MP4 video is non-compressible

- GIVEN the same fixture with `media.mp4`
- WHEN `GET /media.mp4` with
  `Accept-Encoding: gzip, deflate, br`
- THEN status is 200
- AND the response does NOT carry `Vary`
- AND the response does NOT carry `Content-Encoding`

### Requirement: Skip conditions — HEAD, `no-transform`, below-threshold, identity-only

`maybe_apply` SHALL leave a response's body uncompressed
under each of the following conditions, mirroring the
reference's `compression` middleware:

- **HEAD method** (`compression/index.js:192-195`).
  The HTTP layer strips the body on the wire; `Vary`
  has already been set in the negotiation hook so it
  rides on the HEAD response.
- **`Cache-Control: no-transform`** present on the
  merged response, parsed as a comma-separated token
  list with case-insensitive token comparison
  (`compression/index.js:293-300`). NO `Vary` is set —
  the no-transform check runs BEFORE the Vary
  insertion.
- **Body strictly below the 1024-byte threshold**.
  `Vary` is retained (negotiation hook engaged).
- **`negotiate(accept_encoding)` returns `None`** —
  i.e. the `Accept-Encoding` header is absent, names
  only `identity`, or names only encoders excluded
  via `q=0` (including the `*;q=0` wildcard with no
  explicit accepts). `Vary` is retained.

Evidence: SRV-CLI-012 (status: verified, level: L3);
oracle: ORC-203 (`compression-raw.json#head_big_html_gzip`,
HEAD), ORC-204 (`#options_big_html_gzip`, OPTIONS-as-GET
composition — compression engages because OPTIONS walks
the GET pipeline post-7d), ORC-205
(`#no_transform_big_html_gzip`, no-transform skip),
ORC-197 (`#tiny_html_gzip`, below-threshold), ORC-207
(`#big_html_identity_only`), ORC-208
(`#big_html_no_accept_encoding`), ORC-210
(`#big_html_star_q0`). ORC-209
(`#big_html_gzip_q0`) covers the mixed exclusion case
(gzip excluded but brotli accepted → brotli wins).

#### Scenario: HEAD response has `Vary` set, body suppressed by HTTP layer

- GIVEN `serve` (defaults) over a directory containing
  `big.html`
- WHEN `HEAD /big.html` with
  `Accept-Encoding: gzip, deflate, br`
- THEN status is 200
- AND the response carries `Vary: Accept-Encoding`
- AND the response carries no `Content-Encoding` (the
  HEAD skip at `compression/index.js:192-195` runs
  AFTER `Vary` is set but BEFORE the encode step)
- AND the response body is empty on the wire (HTTP
  layer suppresses HEAD bodies)

#### Scenario: `Cache-Control: no-transform` skips compression without setting `Vary`

- GIVEN `serve` (defaults) over a directory containing
  `big-no-transform.html` (> 1024 bytes), with a
  `serve.json` rule
  `{"source": "big-no-transform.html", "headers": [{"key":
  "Cache-Control", "value": "no-transform"}]}`
- WHEN `GET /big-no-transform.html` with
  `Accept-Encoding: gzip, deflate, br`
- THEN status is 200
- AND the response carries
  `Cache-Control: no-transform` (the user rule)
- AND the response does NOT carry `Vary`
- AND the response does NOT carry `Content-Encoding`
- AND the response body is the raw file contents

#### Scenario: Identity-only `Accept-Encoding` retains `Vary` but skips compression

- GIVEN `serve` (defaults) over a directory containing
  `big.html`
- WHEN `GET /big.html` with
  `Accept-Encoding: identity`
- THEN status is 200
- AND the response carries `Vary: Accept-Encoding`
- AND the response does NOT carry `Content-Encoding`
- AND the response body is the raw file contents

#### Scenario: `*;q=0` excludes everything, retains `Vary`

- GIVEN the same fixture
- WHEN `GET /big.html` with `Accept-Encoding: *;q=0`
- THEN status is 200
- AND the response carries `Vary: Accept-Encoding`
- AND the response does NOT carry `Content-Encoding`

### Requirement: Range requests pre-empt compression

A request carrying a `Range` header SHALL pre-empt
compression entirely: the dispatcher's
`build_file_or_304` runs Range handling BEFORE the
compression seam, so a 206 (or 416) response NEVER
enters `maybe_apply`. The 206 response carries the raw
body slice + `Content-Range` + no `Content-Encoding`;
`Vary: Accept-Encoding` MAY be present if a user
`headers` rule set it, but irserve does NOT set it
from the compression path. Mirrors the reference's
ordering: `compression` middleware skips on Range
because the response is already partial when the
middleware sees it.

Evidence: SRV-CLI-012, SRV-CACHE-004 (Stage 7c);
oracle: ORC-206
(`compression-raw.json#range_big_html_gzip`).

#### Scenario: Range bypasses compression

- GIVEN `serve` (defaults) over a directory containing
  `big.html` (> 1024 bytes)
- WHEN `GET /big.html` with
  `Range: bytes=0-15` and
  `Accept-Encoding: gzip, deflate, br`
- THEN status is 206
- AND the response carries `Content-Range: bytes 0-15/<total>`
- AND the response does NOT carry `Content-Encoding`
- AND the response body is the first 16 bytes of the
  raw file

## Compatibility notes

- **`Content-Length` framing on compressed responses
  (D-020 #1).** irserve sends
  `Content-Length: <compressed-len>` with no
  `Transfer-Encoding: chunked` on compressed
  responses. The reference removes `Content-Length`
  and uses chunked transfer-encoding because the
  `compression` middleware is stream-based and doesn't
  know the final length at header-write time. irserve
  has the final compressed bytes in memory
  (static-file model, not streaming) and can declare
  `Content-Length` cleanly. Wire-observable
  difference; the bodies inside the framing carry the
  same compressed content for a given encoder + same
  input. Future change to mirror chunked framing would
  flip D-020 #1 from `adapted` to `mirrored` and
  remove this note's first paragraph.

- **No `mime-db` `compressible` table port (D-020 #2).**
  irserve uses a curated allowlist + regex fallback
  per the second Requirement above. The reference's
  `compressible@2.0.18` package consults the full
  `mime-db@1.33.0` `compressible` flag (~150 entries)
  for MIMEs that escape the regex. Concrete examples
  irserve does NOT treat as compressible:
  `application/postscript`, `application/xml-dtd`,
  `application/x-perl` (mime-db's compressible=true
  entries that don't match the regex). None has
  production-traffic surface in the slice-0 probe;
  future divergences are individually a D-NNN
  opportunity. Promoting the full table would require:
  (a) a new dependency on a `mime-db` port or
  embedding the compressible subset; (b) updating
  this Requirement and the Compatibility note; (c)
  flipping D-020 #2 from `adapted` to `mirrored`.

- **q-rank within `(0, 1)` not honored (D-020 #3).**
  irserve treats any non-zero `q` as accept and does
  not rank fractional q-values. The reference's
  `Negotiator` package does. Real-world
  `Accept-Encoding` values are all-equal-priority
  (`gzip, deflate, br`), so this is a corner without
  production-traffic surface. Promoting to full q-rank
  honoring would require an ordered-traversal pass in
  `negotiate` and would not by itself change the
  output on any current probe.

- **Compressed body bytes are NOT byte-identical to
  reference's (D-020 #4).** Both sides produce valid
  encodings of the same logical body, but encoder
  defaults differ at the bit level: Node `zlib` uses
  `Z_DEFAULT_COMPRESSION` (= 6) for gzip / deflate
  (matches `flate2`'s default), Node brotli's stream
  defaults via the `compression` middleware are
  quality 4 / window 22 (the `brotli` crate's
  defaults nominally match but implementation
  variations may still differ at the bit level). The
  probe runner's per-anchor `bodyMayDiffer` overlay
  strips `content-encoding` and `content-length` from
  the L0 contract and masks body bytes on the 10
  compressed anchors in `compression-raw.json`. This
  is the canonical example of an irserve "body bytes
  not contractual" partition.

- **`Vary: Accept-Encoding` is "set if missing", not
  "append".** The reference's `vary()` utility
  appends `Accept-Encoding` to any existing `Vary`
  header; irserve's `maybe_apply` only inserts when
  no `Vary` header is present on the merged response.
  No current probe exercises a user `headers` rule
  that sets `Vary` on a compressible asset, so this
  corner is undocumented in production. Not a D-NNN —
  documented as a Compatibility note per the
  methodology rule "don't promise more than is
  verified".

- **HEAD body suppression is out of scope.** Neither
  reference nor irserve suppresses the HEAD response
  body at the dispatcher; the HTTP layer (axum/hyper
  on irserve, Node http on reference) strips it on
  the wire. Compression-side, HEAD enters
  `maybe_apply`, has `Vary` set, and short-circuits
  before encode per `compression/index.js:192-195`.

- **Already-encoded passthrough.** irserve never sets
  `Content-Encoding` upstream of `compression::maybe_apply`,
  so the `compression/index.js:183-189` skip is
  vacuously satisfied. If a future user `headers`
  rule ever sets `Content-Encoding`, the behavior is
  undefined-MAY-be-D-NNN.

- **Streaming / backpressure.** irserve buffers all
  bytes in memory — the static-file model. The
  reference's `stream.on('drain')` semantics at
  `compression/index.js:140-152` are not mirrored.

- **Per-request encoder enforcement.** The reference's
  `compression()` `enforceEncoding` option is not
  overridden by the `serve` CLI (default `identity`),
  so this branch is dead in practice and not
  mirrored.

- **Brotli compression-quality tuning.** Both sides
  use crate defaults; body bytes diverge per D-020 #4
  above.

D-020 is the canonical record of the four numbered
divergences and lives at
`docs/reference/serve/decisions.md`. The D-006
deferral entry stays in the same file as a historical
record of when compression was 100 % deferred; its
rationale is superseded by Stage 7e closing the L3
stretch goal.
