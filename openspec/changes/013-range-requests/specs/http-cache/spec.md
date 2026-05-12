# Delta for http-cache

## ADDED Requirements

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
range branch **only when no user-rule value is already present
on the merged response** — mirrors reference's setHeader-
before-getHeaders ordering at `index.js:730-732, 746, 767`
where `response.setHeader('Content-Range', 'bytes */N')` runs
BEFORE `getHeaders` and the subsequent `writeHead(statusCode,
headers)` lets the merged user-rule map override prior
setHeader values. irserve mirrors via `entry().or_insert_with(...)`
in `range::build_416`. `Content-Length` is NOT explicitly set
on 416 — axum derives it from the full body length, matching
the reference's Node-`http`-inferred `Content-Length: <total>`.
Other headers (`Content-Type`, `ETag`, `Last-Modified`,
user-rule headers) carry over from the merged 200 unchanged.

**Range parsing grammar** (single-range subset of RFC 7233 §2.1):

- `bytes=<s>-<e>` — explicit pair. Both `<s>` and `<e>` are
  parsed as the **leading ASCII-digit prefix** of the token
  (mirrors JS `parseInt(token, 10)` used by the reference's
  `range-parser` at
  `third_party/serve/node_modules/.../range-parser/index.js:44`),
  so `Range: bytes=0-3x` is honored as `0-3` and emits 206.
  An empty or leading-non-digit token yields `Unsatisfiable`.
  If `<e>` exceeds `total-1`, it is silently clipped to
  `total-1` (partial-overlap clipping per `range-parser`
  semantics, empirically pinned by
  `tools/probe/cases/range-request.json#clip_to_end` —
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
- Empty file (`total == 0`) — Range processing is **skipped
  entirely** and the request returns the normal 200 response
  with an empty body and no `Content-Range` header. Mirrors
  reference's `if (request.headers.range && stats.size)`
  guard at `serve-handler/src/index.js:720` — a falsy
  `stats.size` (a 0-byte file) bypasses the Range branch.
  irserve mirrors via a `total == 0 → return merged` guard
  in `build_file_or_304` right before the `range::apply`
  call. The `parse_range` helper still returns
  `Unsatisfiable` for `total == 0` (so calling
  `range::apply` on an empty file would otherwise produce
  416), but the dispatch-level guard ensures
  `range::apply` is never reached in that case. Pinned by
  `tools/probe/cases/range-request.json#range_on_empty_file`
  (ORC-178) and `dispatch::tests::range_on_empty_file_returns_200_not_416`.

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
under `etag: false` — see the MODIFIED Requirement below for
the IMS-specific Scenario.

## MODIFIED Requirements

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
`crates/irserve-core/src/dispatch.rs:759` extends the Stage-7a
ETag/INM 304 path with a sibling IMS branch and a shared
`not_modified_response()` helper. The IMS branch reads the
MERGED `Last-Modified` (after `apply_custom_headers`),
mirroring the merge-before-decide ordering Stage 7a
established for the ETag path (Codex round 1 P1). Both 304
branches sit inside the `else` arm of a let-else at
`dispatch.rs:783` (`let Some(range_value) = range_value else
{ ... }`), so a `Range`-bearing request never reaches them —
the function tail emits 206/416 via `range::apply` at
`dispatch.rs:832` instead.

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

## Compatibility notes

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
  change. See `proposal.md`'s "Out of scope" section items
  1, 2, 3.

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

- **First range only on multi-range comma list.** Mirrors
  reference's `range[0]` at `index.js:724`. The manual
  parser does `rest.split(',').next()` and ignores trailing
  segments. A user `Range: bytes=0-3, 8-10` request receives
  206 with `Content-Range: bytes 0-3/<total>`. No
  `multipart/byteranges` response shape. Pinned by
  `range::tests::parse_multi_range_uses_first_only`.

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

- **Hash function and Last-Modified format compatibility
  notes inherit from Stage 7a/7b unchanged.** D-017's
  "round-trip is the contract" framing for ETag, and the
  Stage 7b `httpdate` round-trip framing for Last-Modified,
  carry forward. Range emission does not change the
  validator headers; they pass through `apply` verbatim.
