# Delta for http-cache

## MODIFIED Purpose

HTTP cache validators on static file responses. Covers
strong `ETag` emission and the `304 Not Modified`
short-circuit on matching `If-None-Match` (Stage 7a), the
mutually exclusive `Last-Modified` + `If-Modified-Since`
branch under `--no-etag` / `etag: false` (Stage 7b,
irserve-only IMS 304 per D-018), `Range` request handling
— `206 Partial Content` on satisfiable byte-ranges,
`416 Range Not Satisfiable` on strictly-out-of-range
values (Stage 7c) — and the absence-of-default
`Cache-Control` contract (Stage 7d). The Stage 7c surface
also generalises the `Range`-absent guard introduced in 7b
so a `Range`-bearing conditional GET pre-empts both 304
branches and emits 206/416 instead. Stage 7d adds the
absence-of-default `Cache-Control` contract: irserve emits
NO default `Cache-Control` on any response; the header
appears verbatim only when a user `serve.json#headers`
rule matches the response path.

## ADDED Requirements

### Requirement: Default `Cache-Control` is absent; appears only via user `headers` rule

irserve SHALL NOT emit a default `Cache-Control` header on
any response shape — file responses (200 OK), directory
listings (HTML or JSON), 404 error responses (HTML or
JSON), 3xx redirects, the synthetic fallback HTML error
body, or custom `<status>.html` error pages. The header
SHALL appear on a response **only** when a user
`serve.json#headers` rule matched the response path and
contained a `Cache-Control` key, in which case the value
SHALL be emitted verbatim. The user-rule deletion semantic
(`value: null`) inherited from SRV-HDR-001 also applies:
a rule with `Cache-Control` `value: null` deletes any
earlier `Cache-Control` value the same response carried,
mirroring reference's `if (headers[key] === null) delete
headers[key]` loop at
`third_party/serve-handler/src/index.js:246-250`.

Mirrors reference's `getHeaders` at
`third_party/serve-handler/src/index.js:194-254`. The
`defaultHeaders` object at L215-243 (the only place the
reference assembles default headers for a file response)
contains exactly:

- `Content-Length` (L217)
- `Content-Disposition` (L218)
- `Accept-Ranges: bytes` (L219)
- Exactly one of `ETag` or `Last-Modified` (L222-236; see
  Stages 7a / 7b's Requirements on this capability)
- `Content-Type` (L239-241; only when `mime.contentType`
  returns a value)

`Cache-Control` is **not** in that list. The header reaches
the response only via `Object.assign(defaultHeaders,
related)` at L241, where `related` is populated by the
user-`customHeaders` matcher at L201-211. The
`appendHeaders` helper at L186-192 writes each user-rule
key-value into `related`. So a `Cache-Control` value can
ride on a response if and only if a user rule both matched
the path and supplied the key.

irserve mirrors via a single seam: `apply_custom_headers`
at `crates/irserve-core/src/custom_headers.rs:178-221`
(Stage 6f, SRV-HDR-001) is the only code path in the
crate that inserts a `Cache-Control` header. A grep across
`crates/` for `cache-control` / `Cache-Control` returns
**zero matches outside this seam**. The seam supports
both insertion and `value: null` deletion; the
`Cache-Control` key has no special-case handling.

Evidence: SRV-CACHE-005 (status: verified, level: L3);
oracle: ORC-047 (`cases/cache-control-default.json#default_file_no_rule`),
ORC-048 (`#rule_applies_cache_control`), ORC-049
(`#default_listing_html`, body diverges per D-002 —
`bodyMayDiffer`), ORC-050 (`#default_listing_json`, body
diverges per D-002 — `bodyMayDiffer`), ORC-051
(`#default_404_html`, body diverges per D-002 —
`bodyMayDiffer`), ORC-052 (`#default_404_json`, body
diverges per D-002 — `bodyMayDiffer`); no `D-NNN` entry —
irserve already mirrored the reference since Stage 6f.

Implementation: there is NO `Cache-Control` emission
point in `crates/`. The seam responsible for any
`Cache-Control` value that lands on a response is
`apply_custom_headers` at
`crates/irserve-core/src/custom_headers.rs:178-221`,
called from the dispatcher's success and error arms via
`apply_custom_headers(response, request_path,
header_rules)`. The seam compiles the user
`serve.json#headers` rules (Stage 6f), matches each rule's
`source` glob against the request path, appends matched
headers to the response, and finally deletes any header
whose user-rule value was `null`. The Stage 7d slice 0
commit (`47a56f9`) ratcheted
`tools/probe/cases/cache-control-default.json` to L0-clean
dual-target across all 6 anchors without touching any Rust
code.

#### Scenario: File response without matching rule carries no `Cache-Control`

- GIVEN `serve` (defaults) over a directory containing
  `asset.css` and `tagged.css`, plus a `serve.json` whose
  `headers` rules match only `**/tagged.css` with a
  `Cache-Control: public, max-age=600` value
- WHEN `GET /asset.css`
- THEN status is 200
- AND the response carries no `Cache-Control` header
- AND the response body is the file contents

#### Scenario: Directory listing (HTML) carries no `Cache-Control`

- GIVEN `serve` over a directory that has no `index.html`
  and no `headers` rule matching the root path
- WHEN `GET /` with `Accept: text/html`
- THEN status is 200
- AND the response carries `Content-Type: text/html;
  charset=utf-8`
- AND the response carries no `Cache-Control` header
- AND the response body is the directory listing (HTML
  shape is not contractual per D-002 —
  `bodyMayDiffer`)

#### Scenario: Directory listing (JSON) carries no `Cache-Control`

- GIVEN `serve` over the same root with no matching
  `headers` rule
- WHEN `GET /` with `Accept: application/json`
- THEN status is 200
- AND the response carries `Content-Type:
  application/json; charset=utf-8`
- AND the response carries no `Cache-Control` header
- AND the response body is the directory JSON (shape is
  not contractual per D-002 — `bodyMayDiffer`)

#### Scenario: 404 (HTML) carries no `Cache-Control`

- GIVEN `serve` over a directory and no `headers` rule
  matching the missing path
- WHEN `GET /missing` with `Accept: text/html`
- THEN status is 404
- AND the response carries no `Cache-Control` header
- AND the response body is an HTML error page (content is
  not contractual per D-002 — `bodyMayDiffer`)

#### Scenario: 404 (JSON) carries no `Cache-Control`

- GIVEN the same fixture
- WHEN `GET /missing` with `Accept: application/json`
- THEN status is 404
- AND the response carries `Content-Type:
  application/json; charset=utf-8`
- AND the response carries no `Cache-Control` header
- AND the response body is a JSON error payload (content
  is not contractual per D-002 —
  `bodyMayDiffer`)

#### Scenario: User `headers` rule applies `Cache-Control` verbatim

- GIVEN `serve` over a directory containing `tagged.css`
  with body `h1{}\n`, plus `serve.json` carrying
  `headers: [{ source: "**/tagged.css", headers: [{ key:
  "Cache-Control", value: "public, max-age=600" }] }]`
- WHEN `GET /tagged.css`
- THEN status is 200
- AND the response carries `Cache-Control: public,
  max-age=600` (verbatim from the user rule)
- AND the response body is `h1{}\n`

## Compatibility notes

- **No default `Cache-Control` value is added by 7d.**
  irserve emits absence-of-default; the user-rule path is
  the only entry point. Adding a default value (e.g.
  `Cache-Control: public, max-age=0` or `no-cache`) would
  be a wire-observable divergence from the reference,
  would require a `D-NNN` entry, and would force a
  `divergent` partition on the
  `cache-control-default.json` probe. Out of scope for 7d
  and explicitly rejected in plan-mode.

- **The 6f `apply_custom_headers` seam is unchanged by
  7d.** Stage 7d is verification-only on the
  Cache-Control axis; the user-rule code path that emits
  the header already existed and already supported
  `value: null` deletion. The L0-clean dual-target
  promotion of the 6-anchor probe is the entire
  observable change.

- **`bodyMayDiffer` covers four of the six anchors.** The
  directory listing HTML / JSON and the 404 HTML / JSON
  bodies are not byte-identical across targets (vercel
  synthetic HTML at `serve-handler/src/error.js` vs.
  irserve's fallback `<h1>404 Not Found</h1>\n`; listing
  HTML layout differs per D-002). Only the absence of
  `Cache-Control` on the response headers is contractual
  for those four anchors; the body content is masked.

- **Symmetric absence on all response shapes.** Mirrors
  reference's `defaultHeaders` blank slate at
  `getHeaders:215-243` — a 404 response builds via
  `sendError(...)` at `index.js:107-148` and never enters
  `getHeaders` at all, so it definitionally carries no
  default `Cache-Control`. A 3xx redirect at L591-602
  similarly never reaches `getHeaders`. Only the
  file-response path (200 OK from disk) ever invokes
  `getHeaders`, and even there the user-`customHeaders`
  branch is the only `Cache-Control` source.

- **Future stages.** Promoting any specific default
  `Cache-Control` value would require: (a) a `D-NNN`
  entry recording the divergence rationale; (b) a probe
  partition flip from `clean` to `divergent` for the
  anchors that would observe the divergence; (c) a
  Compatibility note here clarifying which response
  shapes carry the default and which don't. None of this
  is planned for Stage 7e or beyond as of 7d.
