# http-cache Specification

## Purpose

HTTP cache validators on static file responses. Covers strong
`ETag` emission and the `304 Not Modified` short-circuit on
matching `If-None-Match` (Stage 7a), the mutually exclusive
`Last-Modified` + `If-Modified-Since` branch under
`--no-etag` / `etag: false` (Stage 7b, irserve-only IMS 304 per
D-018), and `Range` request handling — `206 Partial Content`
on satisfiable byte-ranges, `416 Range Not Satisfiable` on
strictly-out-of-range values (Stage 7c). The Stage 7c surface
also generalises the `Range`-absent guard introduced in 7b so a
`Range`-bearing conditional GET pre-empts both 304 branches and
emits 206/416 instead. Future sub-stages will add default
`Cache-Control` (Stage 7d) under the same capability.

## Requirements

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
per-deployment by setting `"etag": false` in `serve.json` OR by
passing the `--no-etag` CLI flag (Stage 7b, SRV-CLI-013). When
disabled, irserve does not compute the sha1-based default; the
304 short-circuit therefore never fires on a default value.
User `serve.json#headers` rules can still set `ETag` even when
`"etag": false` — and a 304 still fires when `If-None-Match`
matches the user-supplied value. This mirrors the reference
(see Compatibility note below). To suppress ETag entirely, set
`"etag": false` AND ensure no `headers` rule sets `ETag`.

**`ETag` and `Last-Modified` are mutually exclusive per file
response.** Mirrors the `if (etag) / else` branch at
`third_party/serve-handler/src/index.js:227-236`: the reference
emits exactly one of the two headers from its default-header
build. Under default `etag` (or explicit `"etag": true`), the
response carries `ETag` and no `Last-Modified`. Under
`"etag": false` (or `--no-etag`), the response carries
`Last-Modified` and no `ETag`. The mutex is enforced at the
default-emission seam (`crates/irserve-core/src/dispatch.rs:767-769`,
where `etag_value` and `last_modified_value` return opposite-
gated `Some`), NOT in `file_response`'s body. User
`serve.json#headers` rules applied later by
`apply_custom_headers` MAY supplement either header, exactly
mirroring the reference's `Object.assign(defaultHeaders,
related)` at `index.js:241`. See the `Last-Modified` Requirement
below for the IMS-304 surface under `etag: false`.

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
`crates/irserve-core/src/dispatch.rs:758` builds a 200 response
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
`index.js:760` and wraps both the ETag/INM and (Stage 7b)
Last-Modified/IMS branches.

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

### Requirement: `Last-Modified` is sent under `etag: false` and `If-Modified-Since` short-circuits to 304

File responses under `etag: false` SHALL carry a `Last-Modified` header whose value is the file's mtime formatted as RFC 7231 IMF-fixdate (UTC, whole-second resolution, `"Wed, 06 May 2026 23:39:00 GMT"` shape). The `etag: false` mode is selected by either the `--no-etag` CLI flag (SRV-CLI-013) or `"etag": false` in `serve.json`. The `Last-Modified` header is mutually exclusive with
`ETag` per request — the default-emission seam at
`crates/irserve-core/src/dispatch.rs:767-769` returns exactly
one `Some` from the two value helpers (`etag_value` and
`last_modified_value`) on the same `serve_config.etag`
predicate, mirroring the reference's `if (etag) / else`
branch at `third_party/serve-handler/src/index.js:227-236`.
User `serve.json#headers` rules applied later by
`apply_custom_headers` MAY still supplement either header on
the response unchanged; the mutex lives at the default-emission
seam, not at the wire.

The `etag: false` mode is selected by either the `--no-etag`
CLI flag or `"etag": false` in `serve.json` (SRV-CLI-013).
The CLI flag wins over `serve.json` when set; when unset, the
`serve.json` value (if any) is honored. The flag is long-only
(`--no-etag`, no short alias) per
`third_party/serve/source/utilities/cli.ts:155`.

A subsequent request for the same resource, **when the
response was emitted under `etag: false`** (the CLI flag or
`serve.json#etag: false`), that includes an `If-Modified-Since`
header whose value parses as an HTTP-date and is `>=` the
MERGED response's `Last-Modified` SHALL short-circuit to status
`304 Not Modified` with no body, no `Content-Type`, and no
`Last-Modified` echo, provided no `Range` header is present on
the conditional request. This is an **irserve adaptation**
(D-018) — the reference has no IMS handling and returns 200
with the full body in every IMS case (see Compatibility notes
below).

Under ETag-on (default or explicit `"etag": true`), `If-Modified-Since`
is **ignored** — even if a user `serve.json#headers` rule
supplies a `Last-Modified` on the merged response, the 304
trigger is exclusively `If-None-Match` per SRV-CACHE-001. This
gate narrows D-018's scope to the `etag: false` path, matching
the inventory rule for SRV-CACHE-003 and minimising divergence
from the reference (which has no IMS branch at all).

A request whose `If-Modified-Since` is parseable but `<` the
merged `Last-Modified`, or whose IMS is unparseable, or which
arrives with no IMS header at all, SHALL receive the merged
200 response (full body, `Last-Modified` present). Malformed
IMS is treated as absent, mirroring RFC 9111 §13.1.3 recipient
guidance.

`Last-Modified` is NOT applied to directory listings, 3xx
redirects, JSON error responses, the synthetic fallback HTML
error body, or custom `<status>.html` error pages. Same scope
rules as ETag in Stage 7a (and the same custom-error-page
divergence from reference — see Compatibility note).

**Range pre-empts the IMS 304 short-circuit** (Stage 7c,
SRV-CACHE-004). When a `Range` header is present on a request
that also carries a matching `If-Modified-Since` under
`etag: false`, irserve emits 206 (or 416) and bypasses the IMS
304 branch entirely. The let-else at
`crates/irserve-core/src/dispatch.rs:783` carries both 304
branches in its `else` arm — when `range_value` is `Some`, the
function tail jumps straight to `range::apply`, never reaching
the IMS comparison. Mirrors the reference's L760 guard
(`request.headers.range == null`) generalised to the irserve-
adapted IMS branch.

Evidence: SRV-CACHE-002 (status: verified, level: L3),
SRV-CACHE-003 (status: verified for the reference path,
adapted for the irserve path, level: L3), SRV-CLI-013
(status: verified, level: L3); oracle: ORC-167
(`cases/last-modified-roundtrip.json#first_get`), ORC-168
(`#ims_exact`, reference-only — D-018 divergence), ORC-169
(`#ims_future`, reference-only — D-018 divergence), ORC-170
(`#ims_past`), ORC-171 (`#ims_malformed`), ORC-172
(`#ims_on_404`); D-018.

Implementation:
`crates/irserve-core/src/last_modified.rs::last_modified_value`
returns `Some(httpdate::fmt_http_date(mtime))` iff
`serve_config.etag == Some(false)` AND `meta.modified()`
succeeds. The dispatcher helper `build_file_or_304` at
`crates/irserve-core/src/dispatch.rs:758` extends the Stage-7a
ETag/INM 304 path with a sibling IMS branch at
`dispatch.rs:780-809` and a shared
`not_modified_response()` helper at `dispatch.rs:814`.
The IMS branch reads the MERGED `Last-Modified` (after
`apply_custom_headers`), mirroring the merge-before-decide
ordering Stage 7a established for the ETag path (Codex round
1 P1). The `Range`-absent guard at `dispatch.rs:771` wraps
both branches.

The `--no-etag` CLI flag is wired at
`crates/irserve/src/main.rs:82-83` as `#[arg(long = "no-etag")]
no_etag: bool`; the post-parse override at `main.rs:144-146`
forces `serve_config.etag = Some(false)` when set.

#### Scenario: First GET under `--no-etag` emits `Last-Modified`

- GIVEN `serve --no-etag` (or `serve.json` carrying
  `"etag": false`) over a directory containing `asset.css`
  with body `body{color:red}\n`
- WHEN `GET /asset.css`
- THEN status is 200
- AND the response carries a `Last-Modified` header whose
  value is an IMF-fixdate string (RFC 7231, UTC,
  whole-second, `"<day-of-week>, <DD> <Mon> <YYYY>
  <HH>:<MM>:<SS> GMT"` shape)
- AND the response carries no `ETag` header (mutex)
- AND the response body is `body{color:red}\n`

#### Scenario: Second GET with `If-Modified-Since` ≥ `Last-Modified` returns 304

- GIVEN the same file's `Last-Modified` value `L` captured
  from a prior response under `etag: false`
- WHEN `GET /asset.css` with `If-Modified-Since: L`
- THEN status is 304
- AND the response has no body
- AND the response carries no `Content-Type` header
- AND the response carries no `Last-Modified` header

#### Scenario: GET with `If-Modified-Since` < `Last-Modified` returns 200

- GIVEN the file's `Last-Modified` value `L` (current mtime)
- WHEN `GET /asset.css` with
  `If-Modified-Since: Thu, 01 Jan 1970 00:00:00 GMT`
- THEN status is 200
- AND the response body is the full file contents
- AND the response carries `Last-Modified: L`
- AND the response carries no `ETag` header

#### Scenario: User `headers` rule override drives the 304 decision

- GIVEN `serve --no-etag` and `serve.json` carries
  `headers: [{ source: "**/*.css", headers: [{ key:
  "Last-Modified", value: "Wed, 06 May 2026 23:39:00 GMT" }] }]`
- WHEN `GET /asset.css`
- THEN status is 200
- AND the response carries `Last-Modified: Wed, 06 May 2026
  23:39:00 GMT` (the user override, not the file's actual
  mtime-derived default)
- AND a subsequent `GET /asset.css` with
  `If-Modified-Since: Wed, 06 May 2026 23:39:00 GMT` returns
  304
- AND a subsequent `GET /asset.css` with
  `If-Modified-Since: Tue, 05 May 2026 23:39:00 GMT` (one day
  before the override) returns 200 with the full body

#### Scenario: User `Last-Modified: null` rule suppresses 304

- GIVEN `serve --no-etag` and `serve.json` carries
  `headers: [{ source: "**/*.css", headers: [{ key:
  "Last-Modified", value: null }] }]`
- WHEN `GET /asset.css` with `If-Modified-Since:` any value
  (parseable or not)
- THEN status is 200
- AND the response carries no `Last-Modified` header (deleted
  by the user rule)
- AND the response body is the full file contents

#### Scenario: Malformed `If-Modified-Since` is treated as absent

- GIVEN a file response under `etag: false`
- WHEN `GET /asset.css` with `If-Modified-Since: not-a-date`
- THEN status is 200
- AND the response body is the full file contents
- AND the response carries `Last-Modified` for the file

#### Scenario: `Range` + matching `If-Modified-Since` returns 206 (Range pre-empts the IMS 304)

- GIVEN the file's `Last-Modified` value `L` captured from a
  prior response under `etag: false`
- WHEN `GET /asset.css` with `If-Modified-Since: L` AND
  `Range: bytes=0-3`
- THEN status is 206 (NOT 304, NOT 200)
- AND the response carries `Content-Range: bytes 0-3/<total>`
- AND the response body is the 4-byte partial slice of
  `asset.css`
- AND the response carries the same `Last-Modified: L` as a
  Range-less GET would

Note: Stage 7c lands the actual 206 emission — this Scenario
previously pinned the 200 fall-through under the "Stage 7c
precursor" framing (the `Range`-absent guard was in place,
but `else` fell through to the merged 200). With
`range::apply` wired into `build_file_or_304`, the expected
status flips to 206. The guard itself is unchanged;
SRV-CACHE-004's emission contract is what changed.

### Requirement: `Range` requests return `206 Partial Content` or `416 Range Not Satisfiable`

File responses (200 OK, served from a static asset on disk) SHALL
honor a request `Range: bytes=<s>-<e>` / `bytes=<s>-` /
`bytes=-<n>` header by mutating the merged 200 response into
either `206 Partial Content` (when the parsed range is
satisfiable against the file's representation size) or
`416 Range Not Satisfiable` (when the value is malformed,
specifies a non-`bytes` unit, or starts strictly past the end
of the representation). The mutation runs AFTER user
`serve.json#headers` rules have been merged into the response —
range-emitted headers are last-write-wins over user rules,
mirroring reference's post-`getHeaders` injection at
`third_party/serve-handler/src/index.js:749-752`.

**206 response shape.** Status `206 Partial Content`. Body is
`bytes[start..=end]` (inclusive end). Headers:
`Content-Range: bytes <s>-<e>/<total>` and
`Content-Length: <e-s+1>`, both inserted by the range branch
(last-write-wins). All other headers from the merged 200
response — `Content-Type`, `ETag` (under default `etag`),
`Last-Modified` (under `etag: false`), user-rule headers —
carry over unchanged. Range emission DOES NOT add
`Accept-Ranges`; that header is in irserve's
`L0_EXTRA_VOLATILE_HEADERS` mask and is not contractual at L0
(see Compatibility note below).

**416 response shape.** Status `416 Range Not Satisfiable`.
**Body is the full file representation** (mirrors reference's
fall-through to `stream.pipe(response)` with empty `streamOpts`
at `index.js:730-741`; RFC 7233 §4.4 permits a representation
on 416). `Content-Range: bytes */<total>` is inserted by the
range branch. `Content-Length` is NOT explicitly set on 416 —
axum derives it from the full body length, matching the
reference's Node-`http`-inferred `Content-Length: <total>`.
Other headers (`Content-Type`, `ETag`, `Last-Modified`,
user-rule headers) carry over from the merged 200 unchanged.

**Range parsing grammar** (single-range subset of RFC 7233 §2.1):

- `bytes=<s>-<e>` — explicit pair. Both `<s>` and `<e>` are
  non-negative decimal integers. If `<e>` exceeds `total-1`,
  it is silently clipped to `total-1` (partial-overlap
  clipping per `range-parser` semantics, empirically pinned
  by `tools/probe/cases/range-request.json#clip_to_end` —
  `bytes=8-999` on an 11-byte file → 206 with `bytes
  8-10/11`, NOT 416). If `<s> >= total` or `<s> > <e>`, the
  parser returns `Unsatisfiable` → 416.
- `bytes=<s>-` — open-ended. `<e>` defaults to `total - 1`.
- `bytes=-<n>` — suffix form: last `n` bytes. If `n >= total`,
  the entire representation is returned (RFC 7233 §2.1: "If
  the selected representation is shorter than the specified
  suffix-length, the entire representation is used"). `n == 0`
  → `Unsatisfiable` (matches reference's `range-parser`).
- Multi-range comma list (`bytes=<s1>-<e1>, <s2>-<e2>`) —
  only the first range is honored, subsequent ranges are
  silently ignored (mirrors reference's `range[0]` at
  `index.js:724`).
- Non-`bytes` unit (`pixels=0-3`), malformed values,
  non-ASCII header values — all collapse to `Unsatisfiable`
  → 416.
- Empty file (`total == 0`) — every Range value is
  `Unsatisfiable`.

**Range pre-empts the 304 short-circuit.** When a `Range`
header is present on a conditional GET (with `If-None-Match`
under default `etag` or `If-Modified-Since` under `etag: false`),
irserve emits 206 or 416 — NOT 304 — even when the conditional
validator matches the merged response. This mirrors the
reference's guard at `third_party/serve-handler/src/index.js:760`
(`request.headers.range == null && ...`) for the ETag/INM
branch and extends the same guard to the irserve-adapted
Last-Modified/IMS branch from Stage 7b (D-018). Implemented as
a `let Some(range_value) = range_value else { ... 304
branches ...; return merged; }` at
`crates/irserve-core/src/dispatch.rs:783` — the 304 short-
circuits live inside the `else` arm, and the function tail
unconditionally calls `range::apply` when the let-else
succeeded.

**Range is NOT honored on non-file responses.** Directory
listings, 3xx redirects, JSON error responses, custom
`<status>.html` error pages, and the synthetic fallback HTML
error body silently ignore the request `Range` header (no
206/416 emission). Range parsing lives inside
`build_file_or_304`, which is reached only from the File/Index
arm at `crates/irserve-core/src/dispatch.rs:347` and the
renderSingle branch at `dispatch.rs:481`. Range on a missing
file returns 404 (Range parsing is structurally downstream of
the 404 path), pinned by
`tools/probe/cases/range-request.json#range_on_missing_file`.

`If-Range` is read off the request and ignored, mirroring the
reference's `// TODO ? if-range` at `index.js:719`.

Evidence: SRV-CACHE-004 (status: verified for both targets
after Stage 7c slice 2, level: L3); oracle: ORC-044
(`cases/range-request.json#in_range_first_4`), ORC-045
(`#in_range_tail`), ORC-046 (`#out_of_range`), ORC-173
(`#suffix_last_3`), ORC-174 (`#single_byte`), ORC-175
(`#clip_to_end`), ORC-176 (`#range_on_missing_file`, body
diverges per D-002 — `bodyMayDiffer`), ORC-177
(`#range_with_inm_match`); no `D-NNN` entry — the user picked
Mirror in plan-mode AskUserQuestion D1.

Implementation:
`crates/irserve-core/src/range.rs::parse_range` returns
`RangeOutcome::InRange { start, end }` or
`RangeOutcome::Unsatisfiable`;
`crates/irserve-core/src/range.rs::apply` mutates a merged
`Response<Body>` into either 206 (via `build_206`, slicing
`bytes[start..=end]` and inserting the two range headers) or
416 (via `build_416`, retaining the full body and inserting
`Content-Range: bytes */<total>`). The dispatcher helper
`build_file_or_304` at
`crates/irserve-core/src/dispatch.rs:759` clones `bytes` only
when a `Range` header is present (at `dispatch.rs:776` via
`range_value.as_ref().map(|_| bytes.clone())`), pre-empts the
two 304 short-circuits via a let-else at
`dispatch.rs:783`, and calls `range::apply` at
`dispatch.rs:832` on the merged response when Range was
present. Twenty-one unit tests in `range::tests` pin the
parser and apply surfaces; ten integration tests in
`dispatch::tests` (8 new Range scenarios + 2 updated 7a/7b-
precursor tests) pin the dispatch-level seam under both
`etag: true` (default) and `etag: false` (`--no-etag`)
configurations.

#### Scenario: In-range `Range: bytes=0-3` returns 206 with a partial body

- GIVEN `serve` (defaults) over a directory containing
  `blob.txt` with body `abcdefghij\n` (11 bytes)
- WHEN `GET /blob.txt` with `Range: bytes=0-3`
- THEN status is 206
- AND the response carries `Content-Range: bytes 0-3/11`
- AND the response carries `Content-Length: 4`
- AND the response body is exactly the 4 bytes `abcd`
- AND the response carries the same `ETag` as a Range-less
  GET would
- AND the response carries the same `Content-Type` as a
  Range-less GET would

#### Scenario: Suffix `Range: bytes=-3` returns the last 3 bytes

- GIVEN the same 11-byte `blob.txt`
- WHEN `GET /blob.txt` with `Range: bytes=-3`
- THEN status is 206
- AND the response carries `Content-Range: bytes 8-10/11`
- AND the response carries `Content-Length: 3`
- AND the response body is exactly the 3 bytes `ij\n`

#### Scenario: Partial-overlap `Range: bytes=8-999` clips to file end (NOT 416)

- GIVEN the same 11-byte `blob.txt`
- WHEN `GET /blob.txt` with `Range: bytes=8-999`
- THEN status is 206 (NOT 416)
- AND the response carries `Content-Range: bytes 8-10/11`
  (end clipped to `total-1`)
- AND the response carries `Content-Length: 3`
- AND the response body is exactly the 3 bytes `ij\n`

#### Scenario: Strictly-out-of-range `Range: bytes=999-1000` returns 416 with the full body

- GIVEN the same 11-byte `blob.txt`
- WHEN `GET /blob.txt` with `Range: bytes=999-1000`
- THEN status is 416
- AND the response carries `Content-Range: bytes */11`
- AND the response body is the full file `abcdefghij\n`
  (RFC 7233 §4.4 permits a representation on 416; the
  reference includes it and irserve mirrors)
- AND `Content-Length` matches the full body length (11)
- AND the response carries the same `ETag` and
  `Content-Type` as a Range-less GET would

#### Scenario: `Range` + matching `If-None-Match` returns 206 (Range pre-empts the 304 short-circuit)

- GIVEN the same `blob.txt`'s `ETag` value `E` captured from
  a prior response
- WHEN `GET /blob.txt` with `If-None-Match: E` AND `Range:
  bytes=0-3`
- THEN status is 206 (NOT 304)
- AND the response carries `Content-Range: bytes 0-3/11`
- AND the response body is the 4-byte partial slice `abcd`

Note: the same pre-emption applies symmetrically to the
Stage 7b Last-Modified / `If-Modified-Since` 304 short-circuit
under `etag: false` — see the `Last-Modified` Requirement
above for the IMS-specific Scenario.

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

- **`If-Modified-Since` 304 is an irserve adaptation (D-018),
  gated on `etag: false`.** The reference
  (`third_party/serve-handler/src/index.js`) has no IMS branch —
  grep across `third_party/serve-handler/src/` and
  `third_party/serve/src/` returns no hits for
  `if-modified-since`/`ifModifiedSince`. The 304 short-circuit
  at `index.js:760-764` branches only on `if-none-match`.
  Empirically pinned by `tools/probe/snapshots/last-modified-roundtrip.json`:
  every IMS variant under `target=reference` (exact match via
  `$fromResponse`, far-future, epoch, malformed, on-404)
  returns 200 with the full body, status unchanged. irserve
  adapts to short-circuit on `IMS >= merged Last-Modified`
  under the same `Range`-absent guard and the same
  no-body / no-`Content-Type` / no-validator-echo 304 shape
  established for ETag/INM in Stage 7a, but **only when
  `serve_config.etag == Some(false)`** (the `--no-etag` CLI flag
  or `serve.json#etag: false`). Under ETag-on the IMS branch is
  bypassed even when a user `headers` rule supplies a
  `Last-Modified` on the merged response — the symmetric-with-
  ETag design considered in plan-mode would have widened
  D-018 unnecessarily and contradicted the inventory rule for
  SRV-CACHE-003 ("When ETag is on, IMS is ignored"). The
  narrower gate keeps the divergence from reference contained
  to the documented `--no-etag` path. The adaptation is anchored
  in anti-hallucination rule #5 (no bug-for-bug parity for MVP;
  SRV-CACHE-003's contract was `unknown` until slice 0 — once
  `verified` as "reference is IMS-inert", irserve's adapted
  status is the correct taxonomy outcome) and the symmetry with
  the ETag/INM path under the same `etag` predicate. Pinned in
  this spec by the Scenarios above and by `dispatch::tests::ims_*`
  + `dispatch::tests::etag_on_ignores_ims_even_with_user_lm_rule`
  (irserve path) + `last-modified-roundtrip.json` reference
  snapshot (reference path).

- **Whole-second comparison.** IMF-fixdate wire format carries
  seconds; sub-second mtime drift is not preserved. Round-trip
  via `httpdate::fmt_http_date` then
  `httpdate::parse_http_date` is exact at whole-second
  granularity. Two requests one millisecond apart resolve to
  the same `Last-Modified` and the same 304-vs-200 decision.
  This is already documented as a Compatibility note on
  SRV-CACHE-002's inventory entry.

- **Malformed `If-Modified-Since` is treated as absent
  (RFC 9111 §13.1.3).** Returns 200 with the full body and
  the file's `Last-Modified`. Mirrors recipient-SHOULD
  guidance for unparseable conditional-request headers. Pinned
  by `dispatch::tests::ims_malformed_returns_200` and by
  `last-modified-roundtrip.json#ims_malformed` (clean
  partition — both targets agree).

- **`--no-etag` precedence vs `serve.json#etag`.** When the
  CLI flag is set, irserve forces `serve_config.etag =
  Some(false)` after the `serve.json` merge — the flag wins.
  When the flag is unset, the `serve.json#etag` value (if any)
  is honored as established in Stage 7a; absence defaults to
  ETag-on. This diverges slightly from the reference at
  `third_party/serve/source/utilities/config.ts:140`
  (`config.etag = !args['--no-etag']`), which force-sets
  `config.etag = true` whenever the flag is unset, overriding
  whatever `serve.json` said. The user-observable behavior
  matches the documented per-deployment intent: a deployment
  that opts into `etag: false` via `serve.json` keeps that
  opt-out without needing the CLI flag too. Pinned at
  `crates/irserve/src/main.rs:144-146`.

- **`If-Modified-Since` is NOT applied to directory listings,
  3xx redirects, JSON error responses, the synthetic fallback
  HTML error body, or custom `<status>.html` error pages.**
  Same scope rules as ETag in Stage 7a — `Last-Modified` is a
  file-response header, the listing / redirect / error arms
  are structurally separate from `build_file_or_304`. The
  custom-HTML-error-page divergence from the reference
  (reference applies validators there via `findRelated` +
  `getHeaders`; irserve does not) carries forward from Stage
  7a unchanged.

- **`If-Unmodified-Since`, weak validator semantics, multiple
  IMS values, `Vary` on validators are not supported.** No
  P-class SRV; out of scope for L3. The reference handles
  none of these either. See `openspec/changes/012-last-modified/proposal.md`'s
  "Out of scope" section for the full list.

- **ETag/Last-Modified mutex lives at the default-emission
  seam, not at the wire.** A user `serve.json#headers` rule
  that supplies an `ETag` value lands on the response
  regardless of the `etag` config (mirrors reference's
  `Object.assign(defaultHeaders, related)` at
  `index.js:241`); the same applies symmetrically to a user-
  supplied `Last-Modified`. A deployment under `etag: false`
  with a user `ETag` rule ends up with BOTH `ETag` (from the
  user rule) AND `Last-Modified` (from the default emission).
  To enforce single-validator emission, the user must combine
  the config flag with a matching `headers` rule (either
  setting the same header explicitly to a known value, or
  deleting the unwanted one via `value: null`).

- **`Last-Modified` format is also implementation-defined;
  round-trip is the contract.** Carried forward from Stage 7a's
  D-017 framing. For `Last-Modified`, irserve uses
  `httpdate::fmt_http_date(SystemTime)` which produces RFC
  7231 IMF-fixdate. Reference uses
  `stats.mtime.toUTCString()` (Node.js `Date#toUTCString`,
  also IMF-fixdate). The contract under SRV-CACHE-002 is the
  shape (IMF-fixdate, UTC, whole-second) and the round-trip
  (response value replayed as IMS returns 304 on irserve);
  the literal date string is volatile per fixture and not
  part of the wire contract. The probe-runner
  capture-replay mechanism
  (`{"$fromResponse": {"request": "...", "header":
  "last-modified"}}`, reused from Stage 7a) keeps
  `last-modified-roundtrip.json` mtime-agnostic so the
  round-trip is verified under both targets without pinning
  a literal date.

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

- **`Range` guard is shared with the Last-Modified path.** The
  304 short-circuit (both ETag/INM and Last-Modified/IMS
  branches) is skipped when the request carries a `Range`
  header, matching the reference's check at
  `serve-handler/src/index.js:760` for the ETag branch and
  carried forward to the IMS branch. IrServe does not yet
  parse `Range` (no 206 / 416 emission) — that lands in Stage
  7c. The guard is in place now so 7c does not have to revisit
  `build_file_or_304`; without it, a `Range: bytes=0-3`
  request with matching `If-None-Match` (or `If-Modified-Since`)
  would 304 prematurely (wrong per RFC 9110 §13.1.3).

- **`If-None-Match` matching is verbatim string equality.** No
  comma-separated list parsing, no `*` wildcard handling, no
  weak / strong ETag distinction beyond what the strong-quoted
  literal carries. Mirrors `serve-handler/src/index.js:760`
  (`req.headers['if-none-match'] === stats.etag`).

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

- **416 body is the full representation (mirror reference;
  RFC 7233 §4.4 permits).** The reference does NOT route 416
  through `sendError`; it sets `statusCode = 416` inline at
  `serve-handler/src/index.js:730-731`, adds `Content-Range:
  bytes */<size>`, and falls through to
  `stream.pipe(response)` with empty `streamOpts` →
  `createReadStream` reads the whole file. Pinned by the
  integration test `range request not satisfiable`
  (`third_party/serve-handler/test/integration.test.js:1203-1227`)
  and by the existing reference snapshot
  `tools/probe/snapshots/range-request.json#out_of_range`
  (`content-length: "11"`, `content-range: "bytes */11"`,
  body `"abcdefghij\n"`). RFC 7233 §4.4: "The 416 response
  message is composed in the same manner as a 200 response."
  irserve mirrors via `range::apply::build_416` at
  `crates/irserve-core/src/range.rs:145-161`, which keeps
  the merged response's full body verbatim. No `D-NNN`
  entry — the user picked Mirror in plan-mode D1.

- **Multi-range / `If-Range` / `Accept-Ranges` deferred.**
  Reference has `// TODO ? multiple ranges` at
  `index.js:736` (uses only `range[0]`) and `// TODO ?
  if-range` at `:719`; irserve mirrors both by silently
  ignoring subsequent ranges (parser does
  `rest.split(',').next()`) and by reading `If-Range` off
  the request without acting on it. `Accept-Ranges: bytes`
  is emitted by the reference but is in irserve's
  `L0_EXTRA_VOLATILE_HEADERS` mask in `tools/probe/run.mjs`
  — irserve does NOT emit it. Promoting `Accept-Ranges` to
  contractual, or implementing `multipart/byteranges` for
  multi-range, or honoring `If-Range`, is each a separate
  change.

- **Range pre-empts BOTH 304 short-circuit paths.** Stage
  7a's ETag/INM 304 path (SRV-CACHE-001) and Stage 7b's
  irserve-adapted Last-Modified/IMS 304 path (SRV-CACHE-003,
  D-018) are both wrapped by the same `Range`-absent guard.
  The guard was originally an `if req_headers.get(RANGE).is_none()
  { ... }` block at `dispatch.rs:771` (Stage 7b shape);
  Stage 7c reshapes it into a `let Some(range_value) =
  range_value else { /* 304 branches; return merged; */ };`
  at `dispatch.rs:783`. Both shapes have identical no-Range
  semantics; the let-else exists because Stage 7c needs a
  different terminal path under Range (the `range::apply`
  call) than under no-Range (the merged 200). Mirrors
  reference's L760 (`request.headers.range == null`)
  generalised across both validators. Pinned by
  `range_present_skips_304_even_on_match` (ETag/INM path)
  and `range_present_skips_ims_304_even_on_match` (IMS
  path) in `dispatch::tests` — both previously pinned the
  200 fall-through under Range; with Range emission live
  they now pin `StatusCode::PARTIAL_CONTENT`.

- **Range on missing file: 404 is structurally upstream.**
  A `Range`-bearing request for a missing file returns 404,
  not 416 + 404. The 404 path lives in the dispatcher arms
  upstream of `build_file_or_304`; Range parsing never
  runs. The 404 body content diverges between reference
  (1641-byte vercel HTML at
  `third_party/serve-handler/src/error.js`) and irserve
  (23-byte fallback `<h1>404 Not Found</h1>\n`), covered
  by `bodyMayDiffer: [range_on_missing_file]` in the probe
  partition (D-002 — synthetic HTML body content is not
  contractual). Same pattern as Stage 7b's
  `last-modified-roundtrip.json#ims_on_404`.

- **Partial-overlap clip is empirically pinned, not
  RFC-mandated.** RFC 7233 §2.1 distinguishes `Unsatisfiable`
  (start past EOF) from in-range; clipping the end to
  `total - 1` when `end > total - 1` is a `range-parser`
  implementation choice that the reference inherits. Pinned
  by `tools/probe/cases/range-request.json#clip_to_end`
  (`bytes=8-999` on 11-byte file → 206 with `bytes
  8-10/11`, NOT 416). irserve mirrors via `n.min(total - 1)`
  at `crates/irserve-core/src/range.rs:91`. A future change
  could swap to "strict end → 416" if RFC interpretation
  changes; current contract follows reference.

- **Suffix `bytes=-N` where N > total returns the full
  representation (RFC 7233 §2.1).** "If the selected
  representation is shorter than the specified suffix-
  length, the entire representation is used." Pinned by
  `range::tests::parse_suffix_larger_than_total_returns_full`.
  `bytes=-0` is `Unsatisfiable` (matches `range-parser`'s
  `-2`); pinned by
  `range::tests::parse_negative_zero_suffix_is_unsatisfiable`.

- **`Range`-emitted headers are last-write-wins over user
  rules.** `apply_custom_headers` merges user
  `serve.json#headers` rules into the 200 response BEFORE
  `range::apply` runs; `range::apply` then `inserts`
  `Content-Range` and `Content-Length` (replacing whatever
  the user rule supplied). Mirrors reference's
  post-`getHeaders` injection at
  `serve-handler/src/index.js:749-752`. Pinned by
  `range::tests::apply_overwrites_user_content_length_on_206`.
  All OTHER user-rule headers (e.g. an `ETag` override, an
  `X-Custom` header) survive the range transformation
  unchanged because `range::apply` only touches the two
  range-specific keys. Pinned by
  `dispatch::tests::range_preserves_user_custom_headers_on_206`
  and `range_416_preserves_etag_and_user_headers`.
