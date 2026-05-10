# Delta for headers

## ADDED Requirements

### Requirement: Custom response headers from `serve.json`'s `headers` array

The server SHALL accept a top-level `headers` array in `serve.json`,
where each entry has the shape `{source, headers: [{key, value}]}`.
For each request whose path matches a rule's `source` glob (compiled
with the same Literal/Glob/Pattern matcher kernel as `redirects` and
`rewrites`), the server SHALL apply that rule's `headers` to the
response, layering them over the default response headers.

Multiple matching rules SHALL accumulate in declaration order — the
loop SHALL NOT short-circuit on the first match. Mirrors
`serve-handler/src/index.js:200-210` (the `for` loop over
`customHeaders` never `break`s) plus the final
`Object.assign(defaultHeaders, related)` merge at `index.js:245`.

Headers SHALL be merged case-insensitively, with last-write-wins
semantics for keys that collide across rules — both naturally provided
by Rust HTTP libraries' `HeaderMap` keyed on case-insensitive
`HeaderName`.

Custom headers SHALL apply to successful responses (200) and to error
responses (4xx). 3xx redirect responses SHALL be skipped — the
reference's redirect path at `index.js:586-588` builds the response
via `response.writeHead(redirect.statusCode, { Location: ... })` with
no `getHeaders` call, so custom headers never layer onto redirects.

Evidence: SRV-HDR-001 (status: verified, level: L2); oracle: ORC-053
(`cases/headers-applied.json#css_get`), ORC-031
(`cases/headers-custom.json#html_get` — cleanUrls 301 case, custom
headers correctly absent), ORC-159
(`cases/headers-on-error.json#not_found_carries_x_test`), ORC-160
(`cases/headers-accumulate.json#css_carries_both`); D-015.

Implementation: `crates/irserve-core/src/custom_headers.rs::compile_rules`
walks the user's `serve_config.headers`, calling
`Matcher::compile(&rule.source, "/")` to reuse the same matcher kernel
as redirects/rewrites (the `"/"` destination placeholder is stored
verbatim and never read — `try_match`'s return value is consumed only
as `is_some()` for the boolean match decision). Compiled rules thread
through `server.rs`'s `AppState` into `dispatch`. The new top-level
`dispatch` wraps the existing pipeline (now `dispatch_inner`) and
applies `apply_custom_headers(response, path_for_headers, &header_rules)`
after the inner call returns. The wrapper computes `path_for_headers`
once via `try_percent_decode + collapse_slashes` (mirrors
`getHeaders(.., relativePath)` at `index.js:519`).

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

### Requirement: `value: null` deletes a previously-applied header

A `headers` entry whose `value` is JSON `null` SHALL delete any
header with the same `key` (case-insensitive) that was applied
earlier in the iteration. Mirrors the `for (key in headers) { if
(headers[key] === null) delete headers[key] }` loop at
`serve-handler/src/index.js:247-251`.

The deletion SHALL run as a second pass after the accumulate-and-merge
pass completes, so a `value: null` in an early-firing rule SHALL
delete a `value: "..."` from a later-firing rule when both rules match
and target the same key. This mirrors `Object.assign(defaultHeaders,
related)` followed by the prune loop at `index.js:245-251` (the prune
runs over the merged map, not interleaved with append).

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
`Option<String>`) and verifies the prune logic via 8 unit tests in
`custom_headers::tests` covering: insert, accumulate, case-insensitive
override, 3xx-skip, null-prune deletes prior header, null-prune
doesn't delete when rule doesn't match, null-prune is case-insensitive
on key, empty-rules pass-through.

Implementation: `apply_custom_headers` runs two passes over the
matched-rule list. The first pass inserts/replaces per rule; the
second pass removes any header whose `value` is `None` for matched
rules. axum's `HeaderMap::remove` is case-insensitive on
`HeaderName`, matching the reference's case-insensitive HTTP header
semantics.

#### Scenario: Earlier rule sets, later rule prunes (only-later matches)

- GIVEN `serve.json` has rules `[{source: "**", headers: [{key: "X-Set", value: "v"}]}, {source: "**/*.css", headers: [{key: "X-Set", value: null}]}]`
- WHEN `GET /style.css` (matches both rules)
- THEN no `X-Set` header is present on the response

#### Scenario: Prune rule does not match — header is preserved

- GIVEN `serve.json` has rules `[{source: "**", headers: [{key: "X-Set", value: "v"}]}, {source: "**/*.css", headers: [{key: "X-Set", value: null}]}]`
- WHEN `GET /data.json` (matches only the first rule)
- THEN `X-Set: v` is present on the response
