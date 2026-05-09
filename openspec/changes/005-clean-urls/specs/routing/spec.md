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

Evidence: SRV-ROUT-001 (status: verified, level: L2); oracle: ORC-002, ORC-012, ORC-017, ORC-020, ORC-021, ORC-023, ORC-026, ORC-027.

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
a `globset::GlobSet` via `CleanUrlsView::from_config`, mirroring
the reference's `applicable()` helper at `index.js:256-274`. Pattern
normalization mirrors `slasher` from `serve-handler/src/glob-slash.js`:
patterns without a leading `/` get one prepended before
`GlobSetBuilder::add`. Invalid glob patterns surface as a startup
error (`Error::CleanUrlsGlob`) rather than per-request.

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

Evidence: SRV-ROUT-002 (status: verified, level: L2); oracle: ORC-013, ORC-014, ORC-022, ORC-024.

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
