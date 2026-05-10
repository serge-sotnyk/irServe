# Delta for headers

## ADDED Requirements

### Requirement: Custom response headers from `serve.json`'s `headers` array

The server SHALL accept a top-level `headers` array in `serve.json`,
where each entry has the shape `{source, headers: [{key, value}]}`.
For each request whose path matches a rule's `source` (compiled in
**minimatch-only** mode — no path-to-regexp routing), the server
SHALL apply that rule's `headers` to the response, layering them
over the default response headers. Mirrors `sourceMatches(source,
slasher(relativePath))` at `serve-handler/src/index.js:207`, which
omits the `allowSegments` argument and so reaches only the minimatch
branch at `index.js:59`.

Multiple matching rules SHALL accumulate in declaration order — the
loop SHALL NOT short-circuit on the first match. Mirrors
`serve-handler/src/index.js:200-210` (the `for` loop over
`customHeaders` never `break`s) plus the final
`Object.assign(defaultHeaders, related)` merge at `index.js:245`.

Headers SHALL be merged case-insensitively, with last-write-wins
semantics for keys that collide across rules — both naturally provided
by Rust HTTP libraries' `HeaderMap` keyed on case-insensitive
`HeaderName`.

Custom headers SHALL apply per-branch of `sendError`'s contract
(`serve-handler/src/index.js:467-524`):

- **200 success** — apply, matched against the FINAL resolved file
  path (post-cleanUrls / post-rewrite), not the pre-resolution URL.
- **JSON-preferring error response** — SKIP entirely; reference
  returns at `index.js:477-487` before reaching `getHeaders`.
- **HTML error response with custom `<status>.html` at served root**
  — apply, matched against `/<status>.html`. Mirrors
  `getHeaders(.., errorPage, stats)` at `index.js:508`.
- **HTML fallback error response (no custom page)** — apply,
  matched against the request path, EXCEPT when the error originates
  from a path-traversal / malformed-decode 400 (the reference's
  `getHeaders` call site at `index.js:519` runs against an
  outside-root `absolutePath` whose `path.relative`-then-`slasher`
  form fails to match common rules in practice). After the apply
  pass, the fallback branch SHALL force `Content-Type: text/html;
  charset=utf-8` (mirrors `index.js:520`), so a user rule cannot
  override the fallback's content type.
- **3xx redirect response** — SKIP entirely; reference's redirect
  path at `index.js:586-588` builds the response via
  `response.writeHead(redirect.statusCode, { Location: ... })`
  without going through `getHeaders`.

Header source matching SHALL be case-sensitive (mirrors minimatch's
default `nocase: false`). Source `/Case` SHALL NOT match request
path `/case`, and source `/api/:id` SHALL match only the literal
request path `/api/:id` — `:name` segments are NOT path-to-regexp
captures (reference's `getHeaders` calls `sourceMatches` without the
truthy `allowSegments` argument at `index.js:207`, so the
path-to-regexp branch at `index.js:45-57` is skipped and only
minimatch runs).

Header source matching SHALL apply against the FINAL resolved file
path on the success path — i.e. AFTER cleanUrls extensionless
resolution and rewrite resolution. A request `GET /page` whose
cleanUrls path resolves to `/page.html` SHALL be matched against
`/page.html` (so source `**/*.html` applies). Mirrors
`getHeaders(.., absolutePath, stats)` at `index.js:746` where
`absolutePath` reflects the resolved file after `findRelated`'s
update at `index.js:622-625`.

Evidence: SRV-HDR-001 (status: verified, level: L2); oracle: ORC-053
(`cases/headers-applied.json#css_get`), ORC-031
(`cases/headers-custom.json#html_get` — cleanUrls 301 case, custom
headers correctly absent), ORC-159
(`cases/headers-on-error.json#not_found_carries_x_test`), ORC-160
(`cases/headers-accumulate.json#css_carries_both`), ORC-161
(`cases/headers-after-cleanurls.json#extensionless_resolves_to_html`),
ORC-162
(`cases/error-page-400-headers.json#traversal_serves_custom_400_with_headers`);
D-015.

Implementation: `crates/irserve-core/src/custom_headers.rs::compile_rules`
walks the user's `serve_config.headers` and compiles each `source`
into a slim `HeaderMatcher` enum (Literal / Glob), bypassing
`path_pattern::Matcher` entirely so neither path-to-regexp routing
nor case-insensitive Literal comparison leaks into header matching
(Codex review round 2 P1). Compiled rules thread through
`server.rs`'s `AppState` into `dispatch`. The top-level `dispatch`
wraps the existing pipeline (`dispatch_inner`) and applies
`apply_custom_headers(response, path_for_headers, header_rules)`
ONLY when `dispatch_inner` returns `(_, Some(path))` —
`dispatch_inner` returns `None` for paths that already applied
custom headers per-branch (error responses) or that should skip them
entirely (3xx redirects).

On the success file path, `dispatch_inner` tracks a `lexical_url`
String alongside the `ResolveOutcome`: the URL form of the path that
ultimately resolved (`/page.html` when cleanUrls picked the
`<P>.html` candidate, `/page/index.html` when it picked
`<P>/index.html`, the rewrite's destination string when a rewrite
matched, otherwise the original `url_path`). Codex review round 3 P2:
the prior round-2 fix derived the matching path from the
canonicalized `PathBuf`, which on Windows folds case (`canonicalize`
of `/ASSET.CSS` yields `/asset.css`) and admitted matches that
reference's case-sensitive minimatch rejects. Reference's
`getHeaders` runs against the lexical `path.relative(current,
absolutePath)` where `absolutePath` is the candidate's lexical form
that `findRelated` selected — never canonicalized.

On 4xx error paths, `error_response(status, request_headers, root,
header_rules, request_path, skip_fallback_headers)` applies headers
per `sendError`'s branches at `index.js:467-524`:

- JSON-preferring client: skip headers entirely (reference returns
  before `getHeaders` at `index.js:477-487`).
- Custom `<status>.html` exists: ALWAYS apply headers matched
  against `/<status>.html`. Codex review round 3 P1: the prior
  round-2 fix passed `&[]` for traversal/decode 400 sites, which
  incorrectly suppressed headers on the custom-page branch too.
- Fallback HTML (no custom page): apply headers matched against the
  request path, then FORCE `Content-Type: text/html; charset=utf-8`
  AFTER the apply pass — mirrors reference's order at
  `index.js:519-520` so a user rule cannot override the fallback's
  content type. `skip_fallback_headers=true` is set by callers whose
  reference equivalent invokes `getHeaders` with an outside-root
  `absolutePath` (lexical-escape 400, malformed-decode 400, symlink-
  escape 400) — empirically those calls fail to match common rules
  in practice, and skipping cleanly mirrors that absence (D-015).

#### Scenario: Single rule applies to a matching path

- GIVEN `serve.json` has `headers: [{source: "**/*.css", headers: [{key: "X-Custom", value: "yes"}]}]` and `asset.css` exists
- WHEN `GET /asset.css`
- THEN status is 200
- AND `X-Custom: yes` is present on the response
- AND the body is the contents of `asset.css`

#### Scenario: Two rules with different keys both layer onto the response

- GIVEN `serve.json` has rules `[{source: "**", headers: [{key: "X-One", value: "1"}]}, {source: "**/*.css", headers: [{key: "X-Two", value: "2"}]}]`
- WHEN `GET /asset.css`
- THEN both `X-One: 1` and `X-Two: 2` are present on the response

#### Scenario: Custom headers apply to 404 responses

- GIVEN `serve.json` has `headers: [{source: "**", headers: [{key: "X-Test", value: "yes"}]}]` and no file matches the request path
- WHEN `GET /no-such`
- THEN status is 404
- AND `X-Test: yes` is present on the response

#### Scenario: Custom headers do NOT apply to 3xx redirect responses

- GIVEN `serve.json` has `headers: [{source: "**/*.html", headers: [{key: "X-Custom", value: "yes"}]}]` and the cleanUrls default applies
- WHEN `GET /page.html` (which 301-redirects to `/page` under cleanUrls)
- THEN status is 301
- AND only `Location: /page` appears among non-default headers
- AND `X-Custom` is NOT present on the response

### Requirement: `value: null` deletes a previously-applied header (last-write-wins)

A `headers` entry whose `value` is JSON `null` SHALL delete any
header with the same `key` (case-insensitive) when the matched rule
is the LAST writer for that key in declaration order. Equivalently:
the FINAL accumulated state for each key wins — if the last matching
rule that targets a given key sets `value: null`, the key is
deleted; if it sets a string value, that value is present (even if
an earlier matching rule had `value: null` for the same key).

This mirrors reference's two-stage merge at
`serve-handler/src/index.js:200-251`: `appendHeaders(related,
headers)` accumulates into a single object via `Object.assign`-like
semantics (last write per key wins, including null), then
`Object.assign(defaultHeaders, related)` produces the final map, and
finally `for (key in headers) if (headers[key] === null)
delete headers[key]` prunes only keys whose FINAL value is null.

Evidence: SRV-HDR-002 (status: accepted, level: L2); oracle: covered
by `crates/irserve-core/src/custom_headers::tests` rather than an
oracle probe; D-015.

Note on reference-CLI reachability: `serve`'s public CLI validates
`serve.json` against `@zeit/schemas/deployment/config-static.js`,
which declares `value: { type: 'string', minLength: 1, maxLength:
2048, pattern: ... }`. The CLI refuses to start with a `value: null`
entry. So while `serve-handler`'s library code implements null-pruning
and the requirement text mirrors that behavior, the contract is
unreachable through the reference CLI we probe against. IrServe
accepts `value: null` (config.rs widens `HeaderItem::value` to
`Option<String>`) and verifies the prune logic via the unit tests in
`custom_headers::tests` covering: insert, accumulate, case-sensitive
literal source, `:name` literal source, case-insensitive override,
3xx-skip, null-prune deletes prior header, null-prune doesn't delete
when rule doesn't match, null-prune is case-insensitive on key,
later-set value wins over earlier null-prune, and empty-rules
pass-through.

Implementation: `apply_custom_headers` walks rules in declaration
order in a single pass. For each matched item, it either inserts
(`Some(value)`) or removes (`None`) the header — last write wins
per key. axum's `HeaderMap::insert` and `remove` are
case-insensitive on `HeaderName`, matching reference's
case-insensitive HTTP header semantics. The single-pass form is
algorithmically equivalent to the two-stage merge described in the
requirement (Codex review round 1 P1).

#### Scenario: Earlier rule sets, later rule prunes (only-later matches)

- GIVEN `serve.json` has rules `[{source: "**", headers: [{key: "X-Set", value: "v"}]}, {source: "**/*.css", headers: [{key: "X-Set", value: null}]}]`
- WHEN `GET /style.css` (matches both rules)
- THEN no `X-Set` header is present on the response

#### Scenario: Earlier null does NOT delete a later set value

- GIVEN `serve.json` has rules `[{source: "**", headers: [{key: "X-Set", value: null}]}, {source: "**", headers: [{key: "X-Set", value: "v"}]}]`
- WHEN any matching request fires
- THEN `X-Set: v` is present on the response (the later non-null write wins, mirroring reference's `Object.assign`-then-prune)

#### Scenario: Prune rule does not match — header is preserved

- GIVEN `serve.json` has rules `[{source: "**", headers: [{key: "X-Set", value: "v"}]}, {source: "**/*.css", headers: [{key: "X-Set", value: null}]}]`
- WHEN `GET /data.json` (matches only the first rule)
- THEN `X-Set: v` is present on the response
