# Delta for http-cache

## ADDED Requirements

### Requirement: `ETag` is sent by default on file responses and supports 304

File responses (200 OK, served from a static asset on disk) SHALL
carry a strong `ETag` header by default. A subsequent request
for the same resource that includes an `If-None-Match` header
whose value matches the response `ETag` exactly (verbatim
string equality, including the surrounding double quotes) SHALL
short-circuit to status `304 Not Modified` with no body, no
`Content-Type`, and no `ETag` echo, provided no `Range` header
is present on the conditional request.

Generation of the DEFAULT `ETag` header MAY be disabled
per-deployment by setting `"etag": false` in `serve.json`. When
disabled, irserve does not compute the sha1-based default; the
304 short-circuit therefore never fires on a default value.
User `serve.json#headers` rules can still set `ETag` even when
`"etag": false` — and a 304 still fires when `If-None-Match`
matches the user-supplied value. This mirrors the reference
(see Compatibility note below). To suppress ETag entirely, set
`"etag": false` AND ensure no `headers` rule sets `ETag`.

ETag is NOT applied to directory listings, 3xx redirects, JSON
error responses, or the synthetic fallback HTML error body. ETag
on custom `<status>.html` error pages is a known divergence from
the reference (see Compatibility note below).

Evidence: SRV-CACHE-001 (status: verified, level: L3); oracle:
ORC-042 (`cases/etag-roundtrip.json#first_get`), ORC-043
(`cases/etag-roundtrip.json#second_with_inm`); D-017.

Implementation: `crates/irserve-core/src/etag.rs::compute_etag`
mirrors `serve-handler/src/index.js:24-36` byte-for-byte. The
dispatcher helper `build_file_or_304` at
`crates/irserve-core/src/dispatch.rs:687` builds a 200 response
with the default ETag (when enabled), applies user `headers`
rules to it via the existing `apply_custom_headers`, and then
checks the MERGED response's `ETag` against the request's
`If-None-Match`. Mirrors `serve-handler/src/index.js:194-254`
(`getHeaders` builds `defaultHeaders`, then merges user
`customHeaders` via `Object.assign(defaultHeaders, related)` at
`:241`) immediately followed by the 304 check at `:760` against
the merged `headers.ETag`. The dispatcher's File/Index and
renderSingle call sites therefore return `None` for the
wrapper's `headers_path` slot — the headers pass is already
done. The `Range`-absent guard on the 304 path mirrors
`index.js:760` and is a Stage 7c precursor (Range parsing
itself is deferred to 7c).

User `serve.json#headers` rules CAN override the default ETag,
and the override DRIVES the 304 decision. Replaying the
overridden value as `If-None-Match` returns 304; replaying the
default sha1 (now masked by the override) returns 200. This
mirrors the reference's `Object.assign`-then-check ordering at
`serve-handler/src/index.js:241, 760` and is the round-trip
contract clients rely on: replaying the response's actual
`ETag` always 304s, whatever its provenance.

#### Scenario: First GET emits ETag

- GIVEN `serve` (defaults) over a directory containing
  `asset.css` with body `body{color:red}\n`
- WHEN `GET /asset.css`
- THEN status is 200
- AND the response carries an `ETag` header whose value is a
  strong-quoted hex string (`"\"<40 hex chars>\""`)
- AND the response body is `body{color:red}\n`

#### Scenario: Second GET with matching `If-None-Match` returns 304

- GIVEN the same file's `ETag` value `E` captured from a prior
  response
- WHEN `GET /asset.css` with `If-None-Match: E`
- THEN status is 304
- AND the response has no body
- AND the response carries no `Content-Type` header
- AND the response carries no `ETag` header

#### Scenario: User `headers` rule override drives the 304 decision

- GIVEN `serve.json` carries `headers: [{ source: "**/*.css",
  headers: [{ key: "ETag", value: "\"custom\"" }] }]`
- WHEN `GET /asset.css`
- THEN status is 200
- AND the response carries `ETag: "custom"` (not the default
  sha1)
- AND a subsequent `GET /asset.css` with `If-None-Match:
  "custom"` returns 304
- AND a subsequent `GET /asset.css` with `If-None-Match: <the
  default sha1>` returns 200 (the default value is no longer the
  response's ETag, so it cannot 304)

## Compatibility notes

- **Hash function is implementation-defined; round-trip is the
  contract (D-017).** IrServe mirrors the reference's formula
  exactly — `sha1(extname.as_bytes() ++ b"-" ++ contents)` per
  `serve-handler/src/index.js:24-36`, strong-quoted as
  `"\"<40 hex chars>\""` — and unit tests pin byte-equality
  against the reference's pinned snapshot for the
  `etag-roundtrip` fixture. The contract under SRV-CACHE-001,
  however, is the round-trip behavior (200 emits ETag; matching
  `If-None-Match` short-circuits to 304); the exact hex value
  is not part of the wire contract. The probe runner's
  capture-replay mechanism
  (`{"$fromResponse": {"request": "...", "header": "etag"}}`)
  keeps `etag-roundtrip.json` hash-agnostic so the round-trip
  is verified under both `target=reference` and `target=irserve`
  without pinning a literal hex string.

- **ETag on custom `<status>.html` error pages is deferred.**
  The reference applies ETag (and conditional-GET 304) to
  custom HTML error pages when `etag=true`, via the same
  `findRelated` + `getHeaders` path used for normal file
  responses (`serve-handler/src/index.js:508`). Stage 7a does
  not mirror this — the irserve `error_response` custom-page
  branch emits the custom body without an `ETag` header.
  Neither SRV-CACHE-001's scenarios nor ORC-042 / ORC-043
  mandate ETag on error pages; SRV-CACHE-001's scope is "file
  responses". The divergence is wire-observable and recorded
  here so a follow-up stage can close it without re-litigating
  the contract.

- **`Range` guard is a 7c precursor; Range parsing itself is
  deferred.** The 304 short-circuit is skipped when the request
  carries a `Range` header, matching the reference's check at
  `serve-handler/src/index.js:760`. IrServe does not yet parse
  `Range` (no 206 / 416 emission) — that lands in Stage 7c. The
  guard is in place now so 7c does not have to revisit
  `build_file_or_304`; without it, a `Range: bytes=0-3` request
  with matching `If-None-Match` would 304 prematurely (wrong per
  both the reference and RFC 9110 §13.1.3).

- **`If-None-Match` matching is verbatim string equality.** No
  comma-separated list parsing, no `*` wildcard handling, no
  weak / strong ETag distinction beyond what the strong-quoted
  literal carries. Mirrors `serve-handler/src/index.js:760`
  (`req.headers['if-none-match'] === stats.etag`).

- **`--no-etag` CLI flag is not yet parsed.** Stage 7b lands the
  flag. The library-level switch (`"etag": false` in
  `serve.json`) IS honored as of Stage 7a — it disables the
  default-generated ETag.

- **`"etag": false` disables only the default-generated ETag.**
  Mirrors `serve-handler/src/index.js:227-241`: the reference
  gates the `defaultHeaders['ETag']` assignment on the `etag`
  flag, then merges user `customHeaders` via
  `Object.assign(defaultHeaders, related)` unconditionally — so
  a user rule keyed by `ETag` lands on the response whether or
  not the default was suppressed. The subsequent 304 check at
  `:760` operates on the merged `headers.ETag`. IrServe matches
  this: with `"etag": false` AND a user rule
  `headers: [{ source: "**/*.css", headers: [{ key: "ETag",
  value: "\"custom\"" }] }]`, the response carries
  `ETag: "custom"` and a 304 fires when `If-None-Match: "custom"`
  is replayed. To suppress ETag emission entirely, set
  `"etag": false` AND ensure no `headers` rule sets `ETag`.

- **In-memory mtime-keyed ETag cache is a future optimization,
  not contractual.** The reference's `Map<absPath, [mtime, sha]>`
  at `index.js:22` consulted at `:228-231` is a performance
  optimization; SRV-CACHE-001's contract is the wire behavior,
  not the internal caching strategy. IrServe hashes on every
  request (D-017).
