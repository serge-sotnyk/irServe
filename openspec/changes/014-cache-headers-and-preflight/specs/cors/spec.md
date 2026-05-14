# cors Specification

## Purpose

Cross-Origin Resource Sharing surface under the `-C` /
`--cors` CLI flag. Covers (a) the full four-header
response set the reference emits as defaults under
`--cors` — `Access-Control-Allow-Origin: *`,
`Access-Control-Allow-Headers: *`,
`Access-Control-Allow-Credentials: true`, and
`Access-Control-Allow-Private-Network: true` — applied
post-dispatch via `apply_cors` so they ride on every
response status (200, 3xx, 304, 4xx including 405, 416);
and (b) the OPTIONS-routed-as-GET preflight semantics
introduced in Stage 7d slice 1, where an `OPTIONS`
request flows through phases 3..13 of the dispatcher
identically to a `GET` request and yields the same wire
shape (200 + file body / 304 / 404 / 206 / 416 / etc.)
plus the CORS overlay.

This capability's surface is L3 (verified-behavior catalogue).
The L1 baseline Requirement on the `-C` / `--cors` CLI flag
itself (just `Access-Control-Allow-Origin: *` flag-presence,
SRV-CLI-010) lives in `openspec/specs/cli/spec.md` and is
NOT duplicated here — the L1 contract is "flag wired,
header `Access-Control-Allow-Origin: *` present on at least
one response", while this capability's L3 contracts pin the
full four-header surface and the OPTIONS routing.

Background: the four-header surface was introduced in Stage
6h (commit prior to 014; introduced `apply_cors` at
`crates/irserve-core/src/cors.rs`). The OPTIONS routing
was introduced in Stage 7d slice 1 (commit `381f9bd`).

## Requirements

### Requirement: `--cors` emits the four reference CORS headers on every response

When the server is started with the `-C` / `--cors` CLI
flag, every response SHALL carry the following four
headers:

- `Access-Control-Allow-Origin: *`
- `Access-Control-Allow-Headers: *`
- `Access-Control-Allow-Credentials: true`
- `Access-Control-Allow-Private-Network: true`

These four headers SHALL be applied post-dispatch via
the `apply_cors` overlay, AFTER `apply_custom_headers`
has merged any user `serve.json#headers` rules into the
response. The overlay uses **set-only-if-missing**
semantics: any of the four headers that the merged
response already carries (because a user rule supplied a
value) is left untouched; the default fills in only for
keys the merged response is missing. This mirrors the
reference's `setHeader`-before-`serve-handler` ordering
at `third_party/serve/source/utilities/server.ts:65-70`,
where the CLI sets the defaults via
`response.setHeader(...)` BEFORE delegating to
`serve-handler`, and the handler's final
`response.setHeader` loop at
`third_party/serve-handler/src/index.js:767` overwrites
the CLI defaults with whatever the user `headers` rule
matched.

The four headers SHALL ride on **every** response shape,
regardless of status code:

- 200 file responses, 200 directory listings.
- 3xx redirects (`clean-urls`, `redirects`, trailing-slash
  rewrites).
- 304 conditional-GET short-circuits (both ETag/INM and
  Last-Modified/IMS branches).
- 4xx error responses including 404 and 405.
- 206 / 416 Range responses (Stage 7c).

`apply_cors` runs unconditionally on every response in
the server's response chain at
`crates/irserve-core/src/server.rs:220-229`. It does NOT
gate on response status, body type, or request method.

Note: irserve does NOT emit
`Access-Control-Allow-Methods`,
`Access-Control-Expose-Headers`, or
`Access-Control-Max-Age`. The reference also does NOT
emit these under `--cors` for the static-serve handler
path (the CLI's `server.ts:65-70` block does include an
`Allow-Methods` line, but that header is not part of the
inventory entry for SRV-CORS-001 nor observable via the
`cors-*` probe snapshots — see Compatibility notes for
the divergence ledger).

Evidence: SRV-CLI-010 (status: verified, level: L1 — flag
presence; the L1 contract lives in
`openspec/specs/cli/spec.md`); SRV-CORS-001 (status:
verified, level: L3 — full four-header surface + OPTIONS
routing); oracle: ORC-054
(`cases/cors-applied.json#css_with_cors`), ORC-055
(`cases/cors-flag.json#html_with_cors`), ORC-057
(`cases/cors-response-surface.json#file_200`,
`#cleanurls_301`, `#missing_404`); plus the user-override
exercise via `cases/cors-user-override.json#css_with_user_acao`
(no dedicated ORC row but pinned dual-target). No
`D-NNN` entry — mirror semantics, not adaptation.

Implementation:
`crates/irserve-core/src/cors.rs::apply_cors` at L26-37
iterates the four static `(name, value)` pairs in
`CORS_HEADERS`, calling `headers.insert(name, value)` only
when `!headers.contains_key(&name)`. The call site in
`crates/irserve-core/src/server.rs:220-229` wraps the
dispatched response and runs unconditionally when the
`--cors` flag is set. Unit tests at
`crates/irserve-core/src/cors.rs#tests` pin the four-header
emission on 200, on 3xx (where `apply_custom_headers`
deliberately does NOT run), on 404, and the
set-only-if-missing semantics under a user-supplied
override.

#### Scenario: `--cors` emits all four CORS headers on a 200 file response

- GIVEN `serve --cors` over a directory containing
  `asset.css` with body `body{}\n`
- WHEN `GET /asset.css`
- THEN status is 200
- AND the response carries `Access-Control-Allow-Origin: *`
- AND the response carries `Access-Control-Allow-Headers: *`
- AND the response carries `Access-Control-Allow-Credentials: true`
- AND the response carries `Access-Control-Allow-Private-Network: true`
- AND the response body is `body{}\n`

#### Scenario: `--cors` headers ride on 3xx redirects and 404s

- GIVEN `serve --cors` over a directory with `clean-urls`
  rewrites enabled (so requests for `/blob` 301-redirect
  to `/blob.html` or similar; cf. SRV-RW-001)
- WHEN `GET /blob` (triggering a 301) and `GET /no-such`
  (triggering a 404)
- THEN both responses carry the four CORS headers verbatim
- AND the 301's `Location` header is unchanged by the
  CORS overlay
- AND the 404's body is the synthetic HTML error page
  (content is not contractual per D-002 —
  `bodyMayDiffer` if probed)

#### Scenario: User `serve.json#headers` rule overrides a CORS default

- GIVEN `serve --cors` and `serve.json` carrying
  `headers: [{ source: "**/*.css", headers: [{ key:
  "Access-Control-Allow-Origin", value:
  "https://example.com" }] }]`
- WHEN `GET /asset.css`
- THEN status is 200
- AND the response carries
  `Access-Control-Allow-Origin: https://example.com`
  (the user override, NOT the CLI flag's default `*`)
- AND the response carries `Access-Control-Allow-Headers: *`
  (the other three CORS defaults still fill in via
  set-only-if-missing)
- AND the response carries `Access-Control-Allow-Credentials: true`
- AND the response carries `Access-Control-Allow-Private-Network: true`

### Requirement: OPTIONS is routed through the static pipeline (no preflight short-circuit)

A request whose method is `OPTIONS` SHALL be routed
through the same 13-phase dispatcher pipeline as a `GET`
request. The dispatcher's method gate at
`crates/irserve-core/src/dispatch.rs:91-108` SHALL permit
`OPTIONS` alongside `GET` and `HEAD`; all other methods
SHALL continue to return `405 Method Not Allowed` from
the gate.

For an `OPTIONS /path` request, the response SHALL be
**identical in shape** to a `GET /path` request with the
same headers:

- If `/path` resolves to a static file, the response is
  `200 OK` with the file body, `Content-Type` (from the
  `mime` mapper), `ETag` (or `Last-Modified` under
  `--no-etag`), `Accept-Ranges: bytes`, and any user
  `serve.json#headers` rules that matched.
- If `/path` matches a `redirects` or `clean-urls` rule,
  the response is `3xx` with the `Location` header.
- If `/path` does not resolve, the response is `404` (or
  the configured custom `<status>.html`).
- If the request carries an `If-None-Match` (under
  default `etag`) or `If-Modified-Since` (under
  `--no-etag`) that matches the merged response's
  validator AND no `Range` header is present, the
  response is `304 Not Modified` per SRV-CACHE-001 /
  SRV-CACHE-003.
- If the request carries a `Range` header, the response
  is `206 Partial Content` or `416 Range Not Satisfiable`
  per SRV-CACHE-004 (Stage 7c).

There SHALL be NO 204 No Content short-circuit; the full
pipeline runs. There SHALL be NO body suppression for
OPTIONS (the response body matches what a GET would
produce). Under `--cors`, the four CORS headers ride on
the response per the Requirement above.

Mirrors reference's `serve-handler/src/index.js:548-769`,
which never inspects `request.method`. The full handler
runs identically for GET / HEAD / OPTIONS / any verb. Key
proof points: the `redirect` path (L591-602), the
`getHeaders` merge (L194-254), the 304 short-circuit
(L749-756; the only conditions are `request.headers.range
== null && headers.ETag && headers.ETag ===
request.headers['if-none-match']`), and the final
`stream.pipe(response)` (L767-769) all run without method
branching.

Evidence: SRV-CORS-001 (status: verified, level: L3);
oracle: ORC-056 (`cases/cors-preflight.json#preflight_options`);
no `D-NNN` entry — the inventory open question on
SRV-CORS-001 ("Whether IrServe should adopt or diverge
from the no-preflight-short-circuit behavior") is
resolved to **adopt** (user picked Mirror in plan-mode
AskUserQuestion D1).

Implementation: `crates/irserve-core/src/dispatch.rs:102`
gates on `!matches!(*req.method(), Method::GET |
Method::HEAD | Method::OPTIONS)` — the widening from the
previous `GET | HEAD` allow-list to include `OPTIONS` is
the only code change introduced in Stage 7d slice 1
(commit `381f9bd`). Everything downstream (phases 3..13,
`apply_custom_headers`, `apply_cors`, `build_file_or_304`)
is unchanged.

#### Scenario: OPTIONS on an existing file under `--cors` returns 200 + file body + CORS headers

- GIVEN `serve --cors` over a directory containing
  `asset.css` with body `body{}\n`
- WHEN `OPTIONS /asset.css` with `Origin: https://example.com`,
  `Access-Control-Request-Method: GET`, and
  `Access-Control-Request-Headers: x-custom-header`
- THEN status is 200 (NOT 204)
- AND the response body is `body{}\n` (identical to what a
  GET would produce)
- AND the response carries `Content-Type: text/css;
  charset=UTF-8`
- AND the response carries the same `ETag` as a GET would
- AND the response carries `Accept-Ranges: bytes`
- AND the response carries `Access-Control-Allow-Origin: *`
- AND the response carries `Access-Control-Allow-Headers: *`
- AND the response carries `Access-Control-Allow-Credentials: true`
- AND the response carries `Access-Control-Allow-Private-Network: true`

#### Scenario: OPTIONS on a missing path under `--cors` returns 404 with CORS headers

- GIVEN `serve --cors` over the same root
- WHEN `OPTIONS /no-such-file` with the same preflight
  headers as above
- THEN status is 404
- AND the response carries the four CORS headers verbatim
- AND the response body is the synthetic HTML error page
  (content is not contractual per D-002 —
  `bodyMayDiffer` if probed)

## Compatibility notes

- **Mirror reference, no `D-NNN`.** The 204 No Content
  preflight short-circuit was considered in plan-mode and
  rejected for parity. The reference's `serve-handler`
  has no method-branching anywhere; the irserve
  implementation widens the method gate to include
  OPTIONS so the full pipeline runs. The single
  alternative — a 204 short-circuit — would have been a
  wire-observable divergence and would have required a
  `D-NNN` entry.

- **`Access-Control-Allow-Private-Network: true` is
  unconditional.** This is a relatively recent Chrome /
  WICG extension to the CORS spec
  (https://wicg.github.io/private-network-access/) that
  signals a server in the public-network address space
  is willing to be accessed from clients in
  private-network address space. The reference emits it
  unconditionally under `--cors` regardless of whether
  the request carries an
  `Access-Control-Request-Private-Network: true`
  preflight header. irserve mirrors. A future change
  could gate the header on the preflight request, but
  doing so would diverge from reference and is not
  planned.

- **`Access-Control-Allow-Methods`,
  `Access-Control-Expose-Headers`,
  `Access-Control-Max-Age` are NOT emitted.** The
  reference's `serve` CLI at `server.ts:65-70` includes
  an `Access-Control-Allow-Methods: GET, POST, PUT,
  DELETE, PATCH, OPTIONS` line in the `setHeader` block,
  but this header is **not** part of the SRV-CORS-001
  inventory entry's catalogued headers, **not** observable
  via any of the dual-target `cors-*` probe snapshots
  (`cors-applied.json`, `cors-response-surface.json`,
  `cors-on-redirect.json`, `cors-user-override.json`,
  `cors-preflight.json`), and **not** mirrored by irserve.
  The empirical wire surface across both targets is the
  four-header set documented in the Requirement above.
  `Access-Control-Expose-Headers` and
  `Access-Control-Max-Age` are likewise NOT in
  reference's `setHeader` block and NOT in irserve.
  Adding any of the three would require: (a) updating
  the SRV-CORS-001 inventory entry; (b) re-recording the
  `cors-*` probe snapshots; (c) a `D-NNN` entry if the
  reference still does not emit them in practice.
  Closing the "could 7d also add these?" question
  definitively: no, not in 7d, and not planned for 7e.

- **OPTIONS + `If-None-Match` match returns 304.** The
  conditional-GET 304 short-circuit at
  `build_file_or_304` does not gate on `req.method()`; an
  OPTIONS request that carries a matching `If-None-Match`
  receives 304. Symmetric with GET. Mirrors reference's
  L760 guard, which checks `request.headers.range == null
  && headers.ETag && headers.ETag ===
  request.headers['if-none-match']` — no method check.
  Same for `If-Modified-Since` under `--no-etag` (the
  Stage 7b irserve-adapted IMS branch is also method-blind).

- **OPTIONS + `Range` honored.** A `Range`-bearing
  OPTIONS request flows through the same
  `range::apply` path at `dispatch.rs:832` as a GET, and
  emits 206 / 416 with the same wire shape. The reference
  does not gate Range parsing on method either (the
  Range block at `index.js:717-741` is method-blind), so
  an `OPTIONS /asset.css` with `Range: bytes=0-3` returns
  `206 Partial Content` + the partial body + the four
  CORS headers + `Content-Range: bytes 0-3/<total>`.

- **HEAD body suppression is out of scope.** Neither
  reference nor irserve suppresses the HEAD response
  body — reference's `stream.pipe(response)` at
  `index.js:769` runs unconditionally regardless of
  method, so a HEAD response carries the same body as a
  GET. irserve mirrors. The RFC 7231 §4.3.2 "HEAD has no
  body" semantic is not implemented by either side. This
  is flagged here for future-stage clarification but is
  not addressed by 7d. If a future stage pins HEAD body
  suppression, this Compatibility note should be
  revisited to clarify whether OPTIONS body is also
  suppressed (an open architectural question — the
  reference does NOT distinguish between OPTIONS and
  HEAD body emission, so a symmetric suppression of both
  is the most parity-preserving option).

- **Other HTTP methods stay at 405.** Stage 7d only
  widens the allow-list to include OPTIONS; TRACE,
  CONNECT, PATCH, POST, PUT, DELETE continue to return
  `405 Method Not Allowed` from the dispatcher gate. The
  reference accepts every method (no method check at
  all), so this is a known divergence inherited from
  Stage 1 (the original method-gate decision) and not
  addressed by 7d. Adopting reference's "accept every
  method" stance would require widening the gate to
  `_ => continue` and would surface additional probes for
  the POST / PUT / DELETE pipeline shape; that is a
  separate change.
