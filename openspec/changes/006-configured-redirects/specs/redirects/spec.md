# Delta for redirects

## MODIFIED Requirements

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

Evidence: SRV-RDIR-001 (status: verified, level: L2); oracle:
ORC-030, ORC-084, ORC-085, ORC-089, ORC-090, ORC-091, ORC-092,
ORC-093, ORC-094.

Note: Redirects fire after the cleanUrls / trailingSlash redirect
stage; see the routing capability's "Operation precedence in the
request pipeline" Requirement for the full ordering.

Implementation: `crates/irserve-core/src/redirects.rs::compute_configured_redirects`
walks compiled rules in order (first-match-wins), mirroring
`shouldRedirect`'s redirects branch at
`serve-handler/src/index.js:172-182`. Sources route through three
matchers built at server start by `compile_rules`:

- `Literal` — sources without glob meta and without `:name`
  segments. Trailing-slash flexion mirrors
  `pathToRegExp("/old", []) = ^/old/?$` (both `/old` and `/old/`
  match).
- `Glob` — sources with glob meta (`*`, `?`, `[`, `{`) but no
  `:name`. Compiled via `globset::GlobBuilder::new(body).literal_separator(true)`
  so `*` does not cross `/`, matching minimatch's default. `slasher`
  (`serve-handler/src/glob-slash.js:8`) preserves the `!`-prefix and
  ensures the body has a leading `/`; `compile_one` toggles
  `negate: bool` so `applicable` evaluates `is_match(path) ^ negate`,
  matching minimatch's `nonegate: false` per-pattern semantics.
- `Pattern` — sources with `:name` segments (and possibly `*`
  tokens). The mini path-to-regexp compiler in
  `redirects::compile_source_regex` walks the source byte-by-byte:
  `:name` (where `name` is `[A-Za-z0-9_]+`) becomes
  `(?P<name>[^/]+)`; `*` becomes `(.*)`; literal runs are
  `regex::escape`d. The regex is anchored `^...\/?$` (optional
  trailing slash, mirroring path-to-regexp v3's default).

Destinations are pre-parsed at compile time. For Pattern matchers,
`compile_dest_template` walks the destination string and emits
`Vec<DestFrag>` (literal | param) fragments; at match time each
captured value is `encodeURIComponent`-ed before splicing, mirroring
`pathToRegExp.compile`'s per-prop encoding (`index.js:81-87`). The
surrounding `encodeURI` over the full target lives in
`dispatch::encode_uri_target` (added in 6b for cleanUrls).
Destinations without a protocol pass through `redirects::normalize_destination`,
which mirrors `serve-handler/src/index.js:80`'s `protocol ?
destination : slasher(destination)` — see SRV-RDIR-003 for the
detailed semantics.

The `!`-prefix combined with a `:name` segment routes to `Glob`
(negate=true): minimatch's negation handler treats `:name`-like
fragments as literal characters, mirroring the reference's
`sourceMatches` fallback at `serve-handler/src/index.js:38-67`
(path-to-regexp's first-pass on `!`-bearing patterns produces a
regex that real request paths never match, so it falls through to
minimatch). The rule fires for every path that does NOT literally
equal the (slasher-normalized) source string. Codex review round 2
P1 corrected an earlier compile-time rejection of this combination.

`compile_rules` returns `(Vec<RedirectRuleCompiled>, Vec<InvalidRedirect>)`
and the bin layer (`server.rs::serve`) emits one stderr warning per
skipped rule, mirroring the cleanUrls treatment from 6c and the
reference's silent try/catch at `index.js:38-67`. The `CompileError`
enum carries the variants `Glob | Regex` (a malformed glob or a
malformed compiled regex from `compile_source_regex`).

Glob syntax scope: redirect glob sources support the **standard
glob set** — `*` (single segment), `**` (multi-segment), `?`
(single non-`/` char), character classes `[abc]` / `[a-z]`, brace
alternation `{a,b}`, and `!`-prefix negation. Bash-style extglob
constructs (`+(a|b)`, `@(a|b)`, `?(a|b)`, `*(a|b)`, `!(a|b)`) are
NOT supported — same limitation as cleanUrls. Tracked as Q-012 in
`docs/reference/serve/open-questions.md`.

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

Implementation: `redirect_with_status(target, status)` in
`dispatch.rs` generalizes the existing `redirect_301`. Out-of-range
u16 values that `axum::http::StatusCode::from_u16` rejects fall back
to 301 (matches the "accept any 3xx; range-checking is not specified"
stance — the reference does no validation either, and JS's
`response.writeHead(999, ...)` would produce an arbitrary
non-standard status). `compute_configured_redirects` returns
`(target, status)` where the status defaults to 301 when the rule
has no `type` override (mirrors `index.js:179`'s
`statusCode: type || defaultType`).

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
(`third_party/serve-handler/src/glob-slash.js:6`) — which:

- Collapses consecutive slashes (so `//example.com/x` becomes
  `/example.com/x` — a same-origin redirect, NOT a true
  scheme-relative URL).
- Resolves `.` and `..` segments (so `a/../b` becomes `/b`).
- Guarantees a leading `/`.

Evidence: SRV-RDIR-003 (status: verified, level: L2); oracle:
ORC-079, ORC-080, ORC-081, ORC-082, ORC-086 (mid-path `..`
resolution), ORC-087 (leading `..` resolution), ORC-088 (empty
destination becomes root).

Implementation: `crates/irserve-core/src/redirects.rs::normalize_destination`
mirrors `serve-handler/src/index.js:80`'s
`protocol ? destination : slasher(destination)` exactly. The
`has_protocol` helper checks for a valid URL scheme prefix
(`[A-Za-z][A-Za-z0-9+.\-]*:`). Destinations whose protocol is
truthy pass through verbatim; everything else goes through the
local `path_posix_normalize` (which mirrors Node's
`path.posix.normalize` — consecutive-slash collapse plus `.`/`..`
resolution, with `..` above the absolute root silently dropped) and
then a leading-`/` guarantee.

This closes Q-007 (`docs/reference/serve/open-questions.md`). The
surprising scheme-relative behavior (`//host/x` → `/host/x`) is a
direct consequence of `path.posix.normalize`'s consecutive-slash
collapse, pinned by snapshot
`tools/probe/snapshots/redirects-destination-forms.json`. If a
future user asks for true scheme-relative passthrough, escalate to
a D-NNN divergence — this requirement preserves reference parity.

#### Scenario: Absolute URL destination

- GIVEN redirect `{ "source": "/external", "destination": "https://example.com/x" }`
- WHEN `GET /external`
- THEN status is 301
- AND `Location: https://example.com/x`
