# Delta for http-cache

## ADDED Requirements

### Requirement: `Last-Modified` is sent under `etag: false` and `If-Modified-Since` short-circuits to 304

File responses under `etag: false` SHALL carry a `Last-Modified` header whose value is the file's mtime formatted as RFC 7231 IMF-fixdate (UTC, whole-second resolution, `"Wed, 06 May 2026 23:39:00 GMT"` shape). The `etag: false` mode is selected by either the `--no-etag` CLI flag (SRV-CLI-013) or `"etag": false` in `serve.json`. The `Last-Modified` header is mutually exclusive with
`ETag` per request — the default-emission seam at
`crates/irserve-core/src/dispatch.rs:763-765` returns exactly
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

A subsequent request for the same resource that includes an
`If-Modified-Since` header whose value parses as an HTTP-date
and is `>=` the MERGED response's `Last-Modified` SHALL
short-circuit to status `304 Not Modified` with no body, no
`Content-Type`, and no `Last-Modified` echo, provided no
`Range` header is present on the conditional request. This is
an **irserve adaptation** (D-018) — the reference has no IMS
handling and returns 200 with the full body in every IMS case
(see Compatibility notes below).

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

Evidence: SRV-CACHE-002 (status: accepted, level: L3),
SRV-CACHE-003 (status: verified for the reference path,
adapted for the irserve path, level: L3), SRV-CLI-013
(status: accepted, level: L3); oracle: ORC-167
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
`crates/irserve-core/src/dispatch.rs:754` extends the Stage-7a
ETag/INM 304 path with a sibling IMS branch at
`dispatch.rs:776-792` and a shared
`not_modified_response()` helper at `dispatch.rs:797-802`.
The IMS branch reads the MERGED `Last-Modified` (after
`apply_custom_headers`), mirroring the merge-before-decide
ordering Stage 7a established for the ETag path (Codex round
1 P1). The `Range`-absent guard at `dispatch.rs:767` wraps
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

#### Scenario: `Range` + matching `If-Modified-Since` returns 200 (Stage 7c precursor)

- GIVEN the file's `Last-Modified` value `L`
- WHEN `GET /asset.css` with `If-Modified-Since: L` AND
  `Range: bytes=0-3`
- THEN status is 200 (NOT 304)
- AND the response carries `Last-Modified: L`

Note: irserve does not yet parse `Range` (no 206 / 416
emission until Stage 7c); the guard is in place now so 7c does
not have to revisit `build_file_or_304`.

## MODIFIED Requirements

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
default-emission seam (`crates/irserve-core/src/dispatch.rs:763-765`,
where `etag_value` and `last_modified_value` return opposite-
gated `Some`), NOT in `file_response`'s body. User
`serve.json#headers` rules applied later by
`apply_custom_headers` MAY supplement either header, exactly
mirroring the reference's `Object.assign(defaultHeaders,
related)` at `index.js:241`. See the Stage-7b ADDED
Requirement above for the `Last-Modified` / IMS-304 surface
under `etag: false`.

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
`crates/irserve-core/src/dispatch.rs:754` builds a 200 response
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

## Compatibility notes

- **`If-Modified-Since` 304 is an irserve adaptation (D-018).**
  The reference (`third_party/serve-handler/src/index.js`) has
  no IMS branch — grep across `third_party/serve-handler/src/`
  and `third_party/serve/src/` returns no hits for
  `if-modified-since`/`ifModifiedSince`. The 304 short-circuit
  at `index.js:760-764` branches only on `if-none-match`.
  Empirically pinned by `tools/probe/snapshots/last-modified-roundtrip.json`:
  every IMS variant under `target=reference` (exact match via
  `$fromResponse`, far-future, epoch, malformed, on-404)
  returns 200 with the full body, status unchanged. irserve
  adapts to short-circuit on `IMS >= merged Last-Modified`
  under the same `Range`-absent guard and the same
  no-body / no-`Content-Type` / no-validator-echo 304 shape
  established for ETag/INM in Stage 7a. The adaptation is
  anchored in anti-hallucination rule #5 (no bug-for-bug
  parity for MVP; SRV-CACHE-003's contract was `unknown`
  until slice 0 — once `verified` as "reference is IMS-inert",
  irserve's adapted status is the correct taxonomy outcome)
  and the symmetry with the ETag/INM path. Pinned in this
  spec by the Scenarios above and by `dispatch::tests::ims_*`
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
  none of these either. See `proposal.md`'s "Out of scope"
  section for the full list.

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

- **Hash function and Last-Modified format are implementation-
  defined; round-trip is the contract (D-017).** Carried
  forward from Stage 7a unchanged for ETag. For
  `Last-Modified`, irserve uses
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

- **`Range` guard is shared with the ETag path.** The 304
  short-circuit (both ETag/INM and Last-Modified/IMS branches)
  is skipped when the request carries a `Range` header,
  matching reference's check at `index.js:760` for the ETag
  branch and carried forward to the IMS branch. IrServe does
  not yet parse `Range` (no 206 / 416 emission) — that lands
  in Stage 7c. Without the guard, a `Range: bytes=0-3` request
  with matching IMS would 304 prematurely (wrong per RFC 9110
  §13.1.3).
