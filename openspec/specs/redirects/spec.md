# redirects Specification

## Purpose

Configured 30x redirects from `serve.json`: pattern matching against
the request path, destination URL handling (path-segment interpolation
and absolute URLs), and the `type` override for explicit status codes.
Redirects fire after the cleanUrls / trailingSlash redirect stage and
before rewrites, per the routing precedence pipeline.

## Requirements

### Requirement: `redirects` produce 301 by default

A `redirects` entry `{source, destination}` SHALL match `source`
(minimatch glob or `path-to-regexp` segment pattern) against the
request path; on match, the server SHALL respond with status 301 and
`Location: <destination>` (with `path-to-regexp` segments
interpolated). The `Location` value SHALL be URI-encoded (`encodeURI`).
Negated patterns (`!`-prefixed glob) SHALL be supported. Bash-style
extglob constructs (`+(...)`, `@(...)`, `?(...)`, `*(...)`,
`!(...)`) — which the reference inherits from minimatch via the
shared `sourceMatches` helper — are NOT supported by IrServe; the
divergence is tracked as Q-012 in
`docs/reference/serve/open-questions.md` and inherited from the
cleanUrls capability (see SRV-ROUT-001/002).

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
with `encodeURI` still applied. Destinations without a protocol
SHALL be normalized via `glob-slash.slasher` — i.e.
`path.posix.normalize(path.posix.join('/', value))`
(`third_party/serve-handler/src/glob-slash.js:6`) — which collapses
consecutive slashes (so `//example.com/x` becomes `/example.com/x`,
NOT a true scheme-relative URL), resolves `.` and `..` segments
(so `a/../b` becomes `/b`), and guarantees a leading `/`.

Evidence: SRV-RDIR-003 (status: verified, level: L2); oracle:
ORC-079, ORC-080, ORC-081, ORC-082, ORC-086 (mid-path `..`),
ORC-087 (leading `..`), ORC-088 (empty destination → root).

Note: Q-007 closed by snapshot
`tools/probe/snapshots/redirects-destination-forms.json`. The
surprising scheme-relative behavior is a direct consequence of
`path.posix.normalize`'s consecutive-slash collapse.

#### Scenario: Absolute URL destination

- GIVEN redirect `{ "source": "/external", "destination": "https://example.com/x" }`
- WHEN `GET /external`
- THEN status is 301
- AND `Location: https://example.com/x`
