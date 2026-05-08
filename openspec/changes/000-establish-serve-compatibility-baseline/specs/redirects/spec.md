# Delta for redirects

## ADDED Requirements

### Requirement: `redirects` produce 301 by default

A `redirects` entry `{source, destination}` SHALL match `source`
(minimatch glob or `path-to-regexp` segment pattern) against the
request path; on match, the server SHALL respond with status 301 and
`Location: <destination>` (with `path-to-regexp` segments
interpolated). The `Location` value SHALL be URI-encoded (`encodeURI`).
Negated patterns (`!`-prefixed glob) and extglobs SHALL be supported.

Evidence: SRV-RDIR-001 (status: verified, level: L2); oracle: ORC-030.

Note: Redirects fire after the cleanUrls / trailingSlash redirect
stage; see the routing capability's "Operation precedence in the
request pipeline" Requirement for the full ordering.

#### Scenario: Path-segment redirect

- GIVEN `serve.json` with redirect `{ "source": "/old-docs/:id", "destination": "/new-docs/:id" }`
- WHEN `GET /old-docs/12`
- THEN status is 301
- AND `Location: /new-docs/12`

### Requirement: `redirects` with explicit `type` use that status code

When a redirect rule includes a numeric `type` field, that value SHALL
be used as the response status code in place of the default 301.
IrServe SHALL accept any 3xx code provided by the user; range-checking
is not specified.

Evidence: SRV-RDIR-002 (status: verified, level: L2); oracle: ORC-031.

#### Scenario: Explicit 302

- GIVEN redirect `{ "source": "/old", "destination": "/new", "type": 302 }`
- WHEN `GET /old`
- THEN status is 302
- AND `Location: /new`

### Requirement: External-URL redirect destinations are honored

The server SHALL use an absolute-URL `destination` (e.g.
`https://example.com/x`) verbatim as the `Location` header value,
with `encodeURI` still applied.

Evidence: SRV-RDIR-003 (status: accepted, level: L2); oracle: no probe (Q-007 open).

Note: Q-007 tracks the open question of exact handling of relative
vs. scheme-relative destinations. The Requirement covers the
absolute-URL case; relative and scheme-relative variants will be
clarified by a follow-up probe.

#### Scenario: Absolute URL destination

- GIVEN redirect `{ "source": "/external", "destination": "https://example.com/x" }`
- WHEN `GET /external`
- THEN status is 301
- AND `Location: https://example.com/x`
