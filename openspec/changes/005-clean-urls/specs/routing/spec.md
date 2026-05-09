# Delta for routing

## MODIFIED Requirements

### Requirement: `cleanUrls` strips `.html` and redirects via 301

The server SHALL emit a 301 redirect to the extension-stripped form
for any `.html`, `/index`, or `.../index.html` request when
`cleanUrls` is enabled (the default). The redirect SHALL strip the
matched HTML suffix and collapse any resulting `//` to `/`. The
`Location` header SHALL be unencoded and SHALL start with `/`. When
`cleanUrls` is configured as an array of globs, only matching paths
SHALL receive the redirect.

Evidence: SRV-ROUT-001 (status: verified, level: L2); oracle: ORC-002, ORC-012, ORC-017, ORC-020, ORC-021, ORC-023, ORC-026, ORC-027, ORC-077 (negation pattern, `cases/cleanurls-negation.json`), ORC-078 (invalid-glob graceful degradation, `cases/cleanurls-invalid-glob.json#html_no_redirect`).

Implementation: `crates/irserve-core/src/clean_urls.rs::compute_clean_urls_redirect`
mirrors the cleanUrls branch of `serve-handler/src/index.js:121-143`'s
`shouldRedirect` exactly. The end-anchored regex
`(\.html|\/index)$` is realized as ordered `strip_suffix` calls
(`.html` first, `/index` second) for **single-pass** semantics —
`/index.html` strips to `/index` (NOT `/`), pinned by the
`_smoke#index_html_redirect` snapshot. The `//` collapse on the
stripped result mirrors `index.js:137` (`decodedPath.replace(/\/+/g,
'/')`); `ensure_slash_start` re-prepends `/` if the strip emptied
the path (mirrors `index.js:119`'s `ensureSlashStart`).

Scope (`cleanUrls` array form) is precompiled at server start into
a `Vec<ScopedPattern>` via `CleanUrlsView::from_config`, mirroring
the reference's `applicable()` helper at `index.js:256-274`. Each
`ScopedPattern` carries a `negate: bool` for the minimatch-style
`!`-prefix negation. Pattern normalization mirrors `slasher` from
`serve-handler/src/glob-slash.js:8`: a leading `!` is preserved
(then stripped by `compile_scoped_pattern` after toggling
`negate=true`); the rest is `path.posix.normalize`'d to a leading
`/` before reaching `GlobBuilder::new`. Globs are built with
`GlobBuilder::new(...).literal_separator(true)` so `*` does NOT
cross `/` (matching minimatch's pathname-aware semantics — the
reference's `sourceMatches` calls minimatch). The request path is
also normalized via `collapse_slashes` inside `applicable` before
`is_match`, mirroring `path.posix.resolve(requestPath)` inside
`sourceMatches` at `index.js:38-67`, so raw-mode requests like
`GET //docs/guide.html` participate correctly in `/docs/**` scope
checks. `applicable` evaluates `matcher.is_match(path) ^ negate`
per pattern and short-circuits on the first truthy result, mirroring
minimatch's `nonegate: false` plus `applicable`'s for-loop at
`index.js:261-268`. A sole `!/secret/**` therefore enables cleanUrls
for every path outside `/secret/**`. Invalid glob patterns are
silently skipped with a stderr warning emitted from
`server.rs::serve` (server keeps running), mirroring the reference's
behavior at `index.js:38-67` via `minimatch`.

Glob syntax scope: array-form `cleanUrls` patterns support the
**standard glob set** — `*` (single segment), `**` (multi-segment),
`?` (single non-`/` char), character classes `[abc]` / `[a-z]`,
brace alternation `{a,b}`, and `!`-prefix negation. **Bash-style
extglob** constructs (`+(a|b)`, `@(a|b)`, `?(a|b)`, `*(a|b)`,
`!(a|b)`) — which the reference inherits from `minimatch` via the
shared `sourceMatches` helper (`serve-handler/src/index.js:38-67`,
called from both `applicable` for cleanUrls at `index.js:265` and
`toTarget` for redirects/rewrites at `index.js:70`) — are NOT
supported by IrServe. The pinned reference's only dedicated
extglob test exercises the redirects branch
(`serve-handler/test/integration.test.js:449`,
`redirects: ["face/+(mask1|mask2)/ideal"]`); the cleanUrls array
test at `integration.test.js:705` uses a plain `/directory**`
glob. The cleanUrls-side extglob behavior is therefore
structurally implied by the shared helper but not directly tested
upstream — captured by the new reference-only probe
`tools/probe/cases/cleanurls-extglob.json` (no `runner.l0` block;
auto-skipped against irserve). `globset::GlobBuilder` treats
extglob constructs as literal characters, so a pattern like
`/public/+(page|other).html` matches the literal path
`/public/+(page|other).html` rather than expanding the
alternation. Tracked as Q-012 in
`docs/reference/serve/open-questions.md`. Closing Q-012 is out of
6c's scope.

The redirect target passes through `dispatch.rs::encode_uri_target`
(unchanged from 6b) for `Location`-header encoding, so SPACEs
become `%20`, non-ASCII bytes become percent-escaped UTF-8, and
reserved-but-safe chars pass through.

#### Scenario: `/index.html` redirects to `/index`

- GIVEN default config and fixture has `index.html`
- WHEN `GET /index.html`
- THEN status is 301
- AND `Location: /index`

#### Scenario: `/about.html` redirects to `/about`

- GIVEN default config and fixture has `about.html`
- WHEN `GET /about.html`
- THEN status is 301
- AND `Location: /about`

#### Scenario: Array-form cleanUrls limits scope

- GIVEN config `{ "cleanUrls": ["/docs/**"] }` and fixtures `/docs/guide.html` and `/blog/post.html`
- WHEN `GET /docs/guide.html`
- THEN status is 301 and `Location: /docs/guide`
- AND when `GET /blog/post.html`
- THEN status is 200 (no redirect; path is outside the configured glob)

### Requirement: `cleanUrls` resolves extensionless paths to `.html` files

The server SHALL resolve an extensionless request by trying
`<P>/index.html` first and `<P>.html` second, serving the first
that exists with status 200, when `cleanUrls` is enabled.

Evidence: SRV-ROUT-002 (status: verified, level: L2); oracle: ORC-013, ORC-014, ORC-022, ORC-024, ORC-077 (negation-pattern resolution, `cases/cleanurls-negation.json#public_extensionless_resolves` + `#secret_extensionless_miss`).

Note: Index-first order is probe-confirmed (Q-005 closed by
`prec-cleanurls-default`: with both `/about.html` and
`/about/index.html` present, `/about/index.html` wins).

Implementation: `crates/irserve-core/src/clean_urls.rs::try_clean_urls_resolve`
mirrors `findRelated` + `getPossiblePaths('.html')` at
`serve-handler/src/index.js:276-307`. Candidate 1 is
`<root>/<P>/index.html`; candidate 2 is `<root>/<P>.html` (skipped
when `<P>` is empty, mirroring the `path.basename(item) !== '.html'`
filter at `index.js:279`). Both candidates run through
`stat_under_root` for the same `metadata` → `is_file` →
`canonicalize` → `starts_with(root)` guard the existing `resolve.rs`
uses, so path-traversal attempts via cleanUrls fall through to a
404, not a leak.

The dispatcher gates phase-8 invocation per `index.js:608-642`:
extensionless paths skip pre-stat and run phase 8 first
(extensionless `<P>.html` is preferred over a bare extensionless
`<P>` file; matches SRV-ROUT-006's "no pre-stat for extensionless"
clause); has-extension paths run `resolve()` first (pre-stat) and
fall back to phase 8 only if `resolve()` returned `NotFound`,
preventing `<P>.html` from shadowing an existing `<P>` file. The
gate is implemented by `dispatch.rs::url_path_has_extension`,
which mirrors Node's `path.extname` for our URL-path use case (a
non-leading dot in the basename signals an extension; trailing-slash
paths and dotfiles have no extension).

`/about` and `/about/` flow through phase 8 with the same candidate
set because `try_clean_urls_resolve` trims a single trailing `/`
before joining — mirroring `getPossiblePaths`'s
`relativePath.endsWith('/') ? replace('/', '.html') : (relativePath
+ '.html')` reduction.

#### Scenario: Extensionless resolution to index.html

- GIVEN fixture has both `/about.html` and `/about/index.html` and default `cleanUrls`
- WHEN `GET /about`
- THEN status is 200
- AND body comes from `/about/index.html`

#### Scenario: Extensionless miss outside array scope

- GIVEN config `{ "cleanUrls": ["/docs/**"] }` and fixture `/blog/post.html`
- WHEN `GET /blog/post`
- THEN status is 404 (no extensionless resolution outside the configured glob)
