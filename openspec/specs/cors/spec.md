# cors Specification

## Purpose

Cross-Origin Resource Sharing surface under the `-C` /
`--cors` CLI flag. Covers (a) the full four-header response
set the reference emits as defaults under `--cors` —
`Access-Control-Allow-Origin: *`,
`Access-Control-Allow-Headers: *`,
`Access-Control-Allow-Credentials: true`, and
`Access-Control-Allow-Private-Network: true` — applied
post-dispatch via `apply_cors` so they ride on every response
status (200, 3xx, 304, 4xx including 405, 416); and (b) the
OPTIONS-routed-as-GET preflight semantics introduced in
Stage 7d slice 1, where an `OPTIONS` request flows through
phases 3..13 of the dispatcher identically to a `GET` request
and yields the same wire shape (200 + file body / 304 / 404 /
206 / 416) plus the CORS overlay.

This capability's surface is L3 (verified-behavior catalogue).
The L1 baseline Requirement on the `-C` / `--cors` CLI flag
itself (just `Access-Control-Allow-Origin: *` flag-presence,
SRV-CLI-010) lives in `openspec/specs/cli/spec.md` and is NOT
duplicated here — the L1 contract is "flag wired, header
`Access-Control-Allow-Origin: *` present on at least one
response", while this capability's L3 contracts pin the full
four-header surface and the OPTIONS routing.

Background: the four-header surface was introduced in Stage
6h (`apply_cors` at `crates/irserve-core/src/cors.rs`). The
OPTIONS routing was introduced in Stage 7d slice 1 (commit
`381f9bd`).

## Requirements

### Requirement: `--cors` emits the four reference CORS headers on every response

Every response under `-C` / `--cors` SHALL carry the
following four headers:

- `Access-Control-Allow-Origin: *`
- `Access-Control-Allow-Headers: *`
- `Access-Control-Allow-Credentials: true`
- `Access-Control-Allow-Private-Network: true`

The four headers SHALL be applied post-dispatch via the
`apply_cors` overlay, AFTER `apply_custom_headers` has merged
any user `serve.json#headers` rules into the response. The
overlay SHALL use **set-only-if-missing** semantics: any of
the four headers that the merged response already carries
(because a user rule supplied a value) SHALL be left
untouched; the default SHALL fill in only for keys the merged
response is missing. Mirrors the reference's
`setHeader`-before-`serve-handler` ordering at
`third_party/serve/source/utilities/server.ts:65-70`, where
the CLI sets the four defaults via `response.setHeader(...)`
BEFORE delegating to `serve-handler`, and the handler's
final `response.setHeader` loop at
`third_party/serve-handler/src/index.js:767` overwrites the
CLI defaults with whatever the user `headers` rule matched.

The four headers SHALL ride on **every** response shape,
regardless of status code — 200 file responses, 200 directory
listings, 3xx redirects (`cleanUrls`, `redirects`, trailing-
slash), 304 conditional-GET short-circuits, 4xx errors
(including 404 and 405), and 206 / 416 Range responses (Stage
7c). `apply_cors` runs unconditionally on every response in
the server's response chain at
`crates/irserve-core/src/server.rs:220-229`. It does NOT gate
on response status, body type, or request method.

irserve SHALL NOT emit `Access-Control-Allow-Methods`,
`Access-Control-Expose-Headers`, or
`Access-Control-Max-Age` — the reference's `--cors` block at
`third_party/serve/source/utilities/server.ts:65-70` sets
only the four headers above, and irserve mirrors.

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
(no dedicated ORC row but pinned dual-target). No `D-NNN`
entry — mirror semantics, not adaptation.

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
deliberately does NOT run), on 404, and the set-only-if-
missing semantics under a user-supplied override.

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

- GIVEN `serve --cors` over a directory with `cleanUrls`
  rewrites enabled (so a request for `/index.html`
  301-redirects to `/index`)
- WHEN `GET /index.html` (triggering a 301) and `GET /no-such`
  (triggering a 404)
- THEN both responses carry the four CORS headers verbatim
- AND the 301's `Location` header is unchanged by the CORS
  overlay
- AND the 404's body is the synthetic HTML error page (content
  is not contractual per D-002 — `bodyMayDiffer`)

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

A request whose method is `OPTIONS` SHALL be routed through
the same 13-phase dispatcher pipeline as a `GET` request. The
dispatcher's method gate at
`crates/irserve-core/src/dispatch.rs:91-108` SHALL permit
`OPTIONS` alongside `GET` and `HEAD`; all other methods SHALL
continue to return `405 Method Not Allowed` from the gate.
There SHALL be NO 204 No Content short-circuit; there SHALL
be NO body suppression for OPTIONS — the response body MUST
match what a `GET` for the same path would produce.

Under `--cors`, the four CORS headers ride on the OPTIONS
response via the `apply_cors` overlay (per the preceding
Requirement). Because OPTIONS walks the same pipeline as GET,
all downstream behaviors — `apply_custom_headers` merging,
`build_file_or_304` (ETag/304 short-circuit), Range emission,
404 routing — apply identically. Those derivative behaviors
are NOT separately pinned by Stage 7d; the contractual surface
in 7d is the pipeline routing observable via the
`cors-preflight` probe. Mirrors the reference's
`serve-handler/src/index.js:548-769`, which never inspects
`request.method`.

Evidence: SRV-CORS-001 (status: verified, level: L3); oracle:
ORC-056 (`cases/cors-preflight.json#preflight_options`); no
`D-NNN` entry — the inventory open question on SRV-CORS-001
("Whether IrServe should adopt or diverge from the no-
preflight-short-circuit behavior") is resolved to **adopt**
(user picked Mirror in plan-mode AskUserQuestion D1).

Implementation: `crates/irserve-core/src/dispatch.rs:102`
gates on `!matches!(*req.method(), Method::GET |
Method::HEAD | Method::OPTIONS)` — the widening from the
previous `GET | HEAD` allow-list to include `OPTIONS` is the
only code change introduced in Stage 7d slice 1 (commit
`381f9bd`). Everything downstream (phases 3..13,
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
  charset=utf-8`
- AND the response carries the same `ETag` as a GET would
- AND the response carries `Access-Control-Allow-Origin: *`
- AND the response carries `Access-Control-Allow-Headers: *`
- AND the response carries `Access-Control-Allow-Credentials: true`
- AND the response carries `Access-Control-Allow-Private-Network: true`

## Compatibility notes

- **Mirror reference, no `D-NNN`.** The 204 No Content
  preflight short-circuit was considered in plan-mode and
  rejected for parity. The reference's `serve-handler` has no
  method-branching anywhere; the irserve implementation
  widens the method gate to include OPTIONS so the full
  pipeline runs. The alternative — a 204 short-circuit —
  would have been a wire-observable divergence and would have
  required a `D-NNN` entry.

- **`Access-Control-Allow-Private-Network: true` is
  unconditional.** This is a relatively recent Chrome / WICG
  extension to the CORS spec
  (https://wicg.github.io/private-network-access/) that
  signals a server in the public-network address space is
  willing to be accessed from clients in private-network
  address space. The reference emits it unconditionally under
  `--cors` regardless of whether the request carries an
  `Access-Control-Request-Private-Network: true` preflight
  header. irserve mirrors. A future change could gate the
  header on the preflight request, but doing so would diverge
  from reference and is not planned.

- **`Access-Control-Allow-Methods`,
  `Access-Control-Expose-Headers`,
  `Access-Control-Max-Age` are NOT emitted.** The reference's
  `--cors` block at
  `third_party/serve/source/utilities/server.ts:65-70`
  contains only the four `setHeader` calls catalogued above;
  no `Allow-Methods`, no `Expose-Headers`, no `Max-Age`.
  irserve mirrors. Adding any of the three would require: (a)
  updating the SRV-CORS-001 inventory entry; (b) re-recording
  the `cors-*` probe snapshots; (c) a `D-NNN` entry if the
  reference still does not emit them in practice. Closing the
  "could 7d also add these?" question definitively: no, not
  in 7d, and not planned for 7e.

- **OPTIONS structural-routing only — derivative behaviors
  not separately pinned by 7d.** Because OPTIONS walks the
  same pipeline as GET, the ETag/304 short-circuit, the Range
  emission, and the missing-path 404 routing all apply
  identically (the reference's 304 check at `index.js:760`
  and the Range block at `index.js:717-741` are both
  method-blind, as is the dispatcher's missing-path arm
  upstream of `build_file_or_304`). 7d pins only the
  one-anchor structural contract via `cors-preflight#preflight_options`
  (ORC-056). The derivative behaviors are testable but not
  separately probed at L0 in 7d; if a future stage wants to
  pin OPTIONS-specific edge cases, it should add probe
  anchors (e.g. `preflight_on_missing`,
  `preflight_with_inm`, `preflight_with_range`) rather than
  rely on the structural-routing contract here.

- **HEAD body suppression is out of scope.** Neither
  reference nor irserve suppresses the HEAD response body —
  reference's `stream.pipe(response)` at `index.js:769` runs
  unconditionally regardless of method, so a HEAD response
  carries the same body as a GET. irserve mirrors. The
  RFC 7231 §4.3.2 "HEAD has no body" semantic is not
  implemented by either side. This is flagged here for
  future-stage clarification but is not addressed by 7d.

- **Other HTTP methods stay at 405.** Stage 7d only widens
  the allow-list to include OPTIONS; TRACE, CONNECT, PATCH,
  POST, PUT, DELETE continue to return `405 Method Not
  Allowed` from the dispatcher gate. The reference accepts
  every method (no method check at all), so this is a known
  divergence inherited from Stage 1 (the original method-
  gate decision) and not addressed by 7d. Adopting reference's
  "accept every method" stance would require widening the
  gate to `_ => continue` and would surface additional probes
  for the POST / PUT / DELETE pipeline shape; that is a
  separate change.
