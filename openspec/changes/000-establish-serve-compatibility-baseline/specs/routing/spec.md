# Delta for routing

## ADDED Requirements

### Requirement: `cleanUrls` strips `.html` and redirects via 301

The server SHALL emit a 301 redirect to the extension-stripped form
for any `.html`, `/index`, or `.../index.html` request when
`cleanUrls` is enabled (the default). The redirect SHALL strip the
matched HTML suffix and collapse any resulting `//` to `/`. The
`Location` header SHALL be unencoded and SHALL start with `/`. When
`cleanUrls` is configured as an array of globs, only matching paths
SHALL receive the redirect.

Evidence: SRV-ROUT-001 (status: verified, level: L2); oracle: ORC-002, ORC-012, ORC-017, ORC-020, ORC-021, ORC-023, ORC-026, ORC-027.

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
`<P>/index.html` first and `<P>.html` second, serving the first that
exists with status 200, when `cleanUrls` is enabled.

Evidence: SRV-ROUT-002 (status: verified, level: L2); oracle: ORC-013, ORC-014, ORC-022, ORC-024.

Note: Index-first order is probe-confirmed (Q-005 closed by
`prec-cleanurls-default`: with both `/about.html` and
`/about/index.html` present, `/about/index.html` wins).

#### Scenario: Extensionless resolution to index.html

- GIVEN fixture has both `/about.html` and `/about/index.html` and default `cleanUrls`
- WHEN `GET /about`
- THEN status is 200
- AND body comes from `/about/index.html`

#### Scenario: Extensionless miss outside array scope

- GIVEN config `{ "cleanUrls": ["/docs/**"] }` and fixture `/blog/post.html`
- WHEN `GET /blog/post`
- THEN status is 404 (no extensionless resolution outside the configured glob)

### Requirement: `trailingSlash: true` adds a trailing slash via 301

The server SHALL redirect (status 301) any extensionless non-dotfile
request without a trailing slash to the slash-appended form when
`trailingSlash` is `true`. Dotfiles and files with extensions SHALL
be exempt from the trailing-slash insertion.

Evidence: SRV-ROUT-003 (status: verified, level: L2); oracle: ORC-015, ORC-016.

#### Scenario: Trailing slash added

- GIVEN `trailingSlash: true` and fixture has `about/index.html`
- WHEN `GET /about`
- THEN status is 301
- AND `Location: /about/`

### Requirement: `trailingSlash: false` strips a trailing slash via 301

The server SHALL redirect (status 301) any request ending in `/` to
the slash-stripped form when `trailingSlash` is `false`. When
`trailingSlash` is `undefined` (default), neither the add nor the
strip behavior SHALL be applied.

Evidence: SRV-ROUT-004 (status: verified, level: L2); oracle: ORC-019.

#### Scenario: Trailing slash stripped

- GIVEN `trailingSlash: false` and fixture has `about/index.html`
- WHEN `GET /about/`
- THEN status is 301
- AND `Location: /about`

### Requirement: Multi-slash path is normalized before routing

The server SHALL normalize consecutive slashes in the request path to
a single slash before subsequent routing stages. Whether a 301
redirect or a 200 response follows SHALL be governed by the
subsequent pipeline stages (cleanUrls, trailingSlash, redirects,
rewrites, static-file resolution) acting on the normalized path —
slash collapse itself SHALL NOT emit a redirect.

Evidence: SRV-ROUT-005 (status: verified, level: L2); oracle: ORC-025, ORC-026, ORC-027.

Note: Q-006 closed by ORC-025/026/027 — `serve` collapses
consecutive slashes silently regardless of `trailingSlash`'s value.
ORC-025 (`GET //` → 200, no redirect) shows the normalization is
silent; ORC-026/027 (301 to `/docs/guide`) show the 301 comes from
cleanUrls firing on the normalized path, not from the slash
collapse itself.

#### Scenario: Double-slash root

- GIVEN any fixture with an `index.html` at the root
- WHEN a wire-level `GET //` is sent
- THEN status is 200 (slashes collapse before resolution)

#### Scenario: Double-slash before segment

- GIVEN default config and fixture has `docs/guide.html`
- WHEN a wire-level `GET //docs/guide.html` is sent
- THEN status is 301
- AND `Location: /docs/guide`

#### Scenario: Internal double slash

- GIVEN default config and fixture has `docs/guide.html`
- WHEN a wire-level `GET /docs//guide.html` is sent
- THEN status is 301
- AND `Location: /docs/guide`

### Requirement: Operation precedence in the request pipeline

The server SHALL first collapse consecutive slashes in P to a single
slash (see "Multi-slash path is normalized before routing"), then
apply the following pipeline stages in fixed order on the normalized
path, stopping at the first stage that produces a response, for any
request whose normalized path does not directly resolve to a regular
file inside the served root:

1. **`cleanUrls` redirect** — if `cleanUrls` is on and P ends with
   `.html` (or with `/index` / `/index.html`), respond 301 to the
   extension-stripped form (see "cleanUrls strips .html and redirects
   via 301").
2. **`trailingSlash` redirect** — if `trailingSlash` is `true` and P
   lacks a trailing slash (and is not a dotfile / has no extension),
   respond 301 to `P + "/"`. If `false` and P ends with `/`, respond
   301 to the stripped form.
3. **Config `redirects`** — first matching `redirects` entry produces
   a 301 (or its `type`-overridden status) (see redirects capability).
4. **`rewrites`** — first matching `rewrites` entry serves the
   destination file with status 200 (see rewrites capability).
   Implicit `--single` rewrites participate at this stage.
5. **`cleanUrls` resolution** — if `cleanUrls` is on, attempt
   `<P>/index.html` first and `<P>.html` second (see "cleanUrls
   resolves extensionless paths to .html files"). Serve the first
   that exists with status 200.
6. **Static file** — final attempt to resolve P (or its `index.html`)
   under the served root. Failure here SHALL yield a 404.

A direct-file pre-stat SHALL short-circuit the rewrite/findRelated
branch only when the request path has a non-empty extension.
Concretely:

- If P has an extension (`/asset.css`, `/page.html`) and the file
  exists, the existing file SHALL be served. Rewrites and the
  cleanUrls-resolution stage SHALL NOT run.
- If P has no extension (`/about`, `/api`), there SHALL be no
  pre-stat. A matching rewrite SHALL displace the original-path file
  even if it exists. The original path SHALL be attempted only at
  stage 6 (when no rewrite matched).

Stages 1 and 2 are unaffected by this gating: they trigger on the URL
form, not on file existence.

Evidence: SRV-ROUT-006 (status: verified, level: L2); oracle: ORC-014, ORC-016, ORC-017, ORC-018, ORC-020, ORC-032, ORC-033.

#### Scenario: Redirect beats rewrite

- GIVEN config has both a redirect `/old → /new` and a rewrite `/old → /alt.html`
- WHEN `GET /old`
- THEN status is 301
- AND `Location: /new`

#### Scenario: cleanUrls 301 wins over existing-file short-circuit

- GIVEN default config and the fixture root has `index.html`
- WHEN `GET /index.html`
- THEN status is 301
- AND `Location: /index`

#### Scenario: Redirect beats `--single` SPA fallback

- GIVEN `--single` and a redirect `/old → /new`
- WHEN `GET /old`
- THEN status is 301 (the redirect, not the SPA fallback)
