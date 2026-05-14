# Proposal: Custom error pages, full L2 security, custom response headers

## Why

Stage 6e shipped configured rewrites + `--single` (`007-configured-rewrites`):
phase 7 of the 13-phase dispatcher is wired. The next un-defer slice in the
L1/L2 roadmap (`docs/stage6_l1_l2_capabilities.md:54`) is the cross-cutting
6f bundle — custom error pages, the full SRV-SEC-001 wire surface,
single-pass URL decode (SRV-SEC-002), and the `headers` array in
`serve.json` (SRV-HDR-001 + SRV-HDR-002).

This change closes:

- **SRV-FILE-003** — `<statusCode>.html` at the served root replaces the
  built-in HTML body for any error status sent to HTML-accepting clients.
  Previously `verified` against the reference but deferred from the
  strict-L0 cutoff per `D-008` / `D-011`.
- **SRV-SEC-001 (full surface) + SRV-SEC-002** — strict `%xx` rejection
  with 400, lexical containment check at phase 1 with 400 on `..`-escape,
  benign leading `//` collapsing to 404. Stage 5b only wired the
  L0-hygiene 404 piece; D-010 deferred the strict-decode promotion to 6f.
- **SRV-HDR-001 + SRV-HDR-002** — `headers: [{source, headers: [{key,
  value}]}]` from `serve.json`. Glob-matched per request, accumulated in
  declaration order, layered case-insensitively over default response
  headers, and applied to every successful and 4xx response (3xx
  redirects skipped per the reference's writeHead-only path). `value:
  null` deletes a previously-applied header.

## What

- **Generalize `notfound.rs` → `error.rs`** with `error_response(status,
  request_headers, root)`. JSON-preferring clients receive a status-keyed
  envelope (`not_found` for 404, `bad_request` for 400, `server_error`
  fallback for any other status). HTML clients first try `root/<status>.html`
  via `tokio::fs::read`; on miss, the response body is a generic
  `<h1>STATUS REASON</h1>\n` (`StatusCode::canonical_reason()` provides
  the reason phrase). D-003 disclaims markup parity, so the fallback
  body deliberately diverges from the reference's full `errorTemplate`
  HTML. Mirrors `serve-handler/src/index.js:467-524` (`sendError`).

- **Phase-1 strict %xx decode** in `dispatch.rs`. A new
  `try_percent_decode` helper walks the URI path and rejects any `%`
  not followed by exactly two ASCII-hex chars. On `Err`, dispatch
  short-circuits to `error_response(400, ..)` before any phase 2+ runs.
  Mirrors `decodeURIComponent`'s URIError branch at
  `serve-handler/src/index.js:561-567`.

- **Phase-1 lexical containment check.** A new
  `lexical_path_escapes_root` walks the decoded path's segments and
  returns true when `..` segments would pop above the served root.
  Purely lexical — no filesystem I/O — so a `..` traversal whose
  escaped target happens not to exist still yields 400 (not the 404 a
  fs-only check would emit). Mirrors `isPathInside(path.join(current,
  relativePath), current)` at `index.js:570-580`. The `EscapedRoot`
  branch in `dispatch.rs` also routes to 400 as defense-in-depth for
  symlink/canonicalize-based escapes.

- **Custom-headers post-dispatch pass.** A new
  `crates/irserve-core/src/custom_headers.rs` module compiles
  `serve.json` `headers` rules at startup via `path_pattern.rs`'s
  `Matcher::compile` (passing a placeholder destination `"/"` since
  headers have no template) and exposes
  `apply_custom_headers(response, request_path, &compiled_rules)`.
  `dispatch` runs this pass on every response except 3xx redirects
  (matches the reference's `response.writeHead(redirect.statusCode,
  { Location: ... })` path at `index.js:586-588`, which bypasses
  `getHeaders` entirely). Two passes: accumulate-and-merge first, then
  null-prune second — mirrors `index.js:200-251`'s `Object.assign(
  defaultHeaders, related)` followed by the `delete headers[key] ===
  null` loop.

- **`HeaderItem::value: Option<String>`.** `config.rs` widens the field
  from `String` to `Option<String>` so JSON `null` deserializes to
  `None`. Existing fixtures with string values continue to parse
  unchanged.

- **Probe coverage.**
  - `error-page-custom#missing_with_custom_404` (ORC-007) — opted into
    `runner.l0.clean`.
  - `notfound-custom#{missing_html_custom, missing_html_custom_json_accept}`
    (ORC-059) — opted in (with `contentLengthMayDiffer` on the JSON
    branch; reference omits `Content-Length` on JSON 404, irserve emits
    it via axum).
  - `traversal-raw-encoded#{raw_dotdot_literal,
    raw_dotdot_percent_encoded, raw_double_slash, raw_malformed_percent}`
    (ORC-038, ORC-039, ORC-040, ORC-041) — all four opted in with
    `bodyMayDiffer` (reference emits the full `errorTemplate` HTML;
    irserve emits the generic `<h1>STATUS REASON</h1>` per D-003).
  - `traversal-encoded#{encoded_dotdot, double_slash_passwd, raw_dotdot,
    encoded_in_subpath}` (ORC-034, ORC-035, ORC-036, ORC-037) — opted
    in. Fetch-mode client normalization reduces all four to in-root
    resolves; first three with `bodyMayDiffer`, last with byte-exact
    body.
  - `headers-applied#css_get` (ORC-053) — opted in.
  - `headers-custom#html_get` (ORC-031) — opted in with
    `contentLengthMayDiffer` (axum emits `Content-Length: 0` on 301
    empty bodies; Node's `http` does not).
  - `headers-on-error#not_found_carries_x_test` (NEW; ORC-159) —
    proves `**` headers rule applies to 404 responses.
    `bodyMayDiffer`.
  - `headers-accumulate#css_carries_both` (NEW; ORC-160) — proves
    multiple matching rules layer distinct keys onto the same response.

## Out of scope

- **L3 cache headers as defaults** (`Cache-Control`, `ETag`,
  `Last-Modified`, conditional GETs) — Stage 7+. Custom headers can set
  these via `serve.json`, but no irserve-side default emission.

- **HTML body markup parity for error pages.** D-003 stands; the
  default HTML body for non-custom responses is generic
  `<h1>STATUS REASON</h1>\n`. The existing `notfound-shape`
  `bodyMayDiffer` partition stays as-is.

- **`<status>.html` lookup beyond root.** Reference checks
  `${current}/${statusCode}.html` only (`index.js:490`). No nested-
  subdir lookup, no extension fallback. IrServe mirrors.

- **Custom headers on 3xx redirects.** Reference's redirect path bypasses
  `getHeaders` (`index.js:586-588`). IrServe's `apply_custom_headers`
  short-circuits on `response.status().is_redirection()` to match.

- **Custom headers on 400 path-traversal responses.** Empirical
  finding: reference's `getHeaders` is invoked from `sendError(400)`
  with an `absolutePath` outside `current`, so `path.relative` yields a
  `../`-prefixed string that minimatch's `slasher`-normalized form does
  not match `**` against. Out of scope for the `headers-on-error` probe;
  irserve's behavior matches because the lexical 400 short-circuit fires
  before `apply_custom_headers` sees the response and the path-rules
  pipeline hasn't computed a meaningful `relativePath` for the escaped
  case either.

- **`headers.source` patterns beyond Literal/Glob.** `:name` is
  unsupported by reference's `getHeaders` (it uses minimatch only).
  Inherits Q-012's no-extglob limitation from 6d/6e.

- **`HeaderItem::value` types beyond `string | null`.** JSON numbers,
  booleans, arrays in the value position are rejected at config-load
  time by serde's `Option<String>` deserializer. Reference uses raw
  string concatenation with undefined behavior for non-strings.

- **Reference-CLI parity for `value: null`.** The public `serve` CLI
  validates `serve.json` against `@zeit/schemas/deployment/config-static`,
  which declares `value: { type: 'string', minLength: 1 }`. `serve`
  refuses to start with a `value: null` entry (D-015 records this).
  IrServe ships SRV-HDR-002 as an irserve-extension verified by unit
  tests in `custom_headers::tests` rather than an oracle probe — the
  reference cannot validate the feature end-to-end.
