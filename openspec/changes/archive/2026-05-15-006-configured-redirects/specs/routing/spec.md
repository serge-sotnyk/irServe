# Delta for routing

## MODIFIED Requirements

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

Evidence: SRV-ROUT-006 (status: verified, level: L2); oracle:
ORC-014, ORC-016, ORC-017, ORC-018, ORC-020, ORC-032, ORC-033,
ORC-083 (trailingSlash↔redirects compose,
`cases/prec-trailing-redirects.json#trailing_add_wins_over_redirect`).

Implementation: `crates/irserve-core/src/dispatch.rs::dispatch`
realizes the pipeline. Stages 1, 2, and 5 (cleanUrls + trailingSlash)
were wired in 6b/6c. Stage 3 (configured redirects, phase 6 of the
13-phase pipeline) is wired in 6d via the new
`redirects::compute_configured_redirects` walking precompiled
`Vec<RedirectRuleCompiled>` and emitting 301 (or rule-overridden
3xx) via the generalized `redirect_with_status(target, status)`.
The trailingSlash↔redirects compose corner is pinned by ORC-083:
phase 5 (trailingSlash) returns before phase 6 (redirects) is
reached, mirroring the order of the `slashing` branch at
`serve-handler/src/index.js:145-168` versus the redirects loop at
`:172-182`. The cleanUrls↔redirects compose corner was already
covered by ORC-033 (`page_html_cleanurl_default` — cleanUrls 301 to
`/page` fires; the configured redirect `/go → /target` is unrelated
to the page-level path). The redirects↔rewrites compose corner
remains pinned only on the redirect-wins side (ORC-032,
`prec-rewrites-redirects.json#go_root`); the rewrite-wins side
lands when 6e wires phase 7.

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

#### Scenario: trailingSlash redirect beats configured redirect

- GIVEN `trailingSlash: true` and a redirect `/face/mask → /elsewhere`
- WHEN `GET /face/mask`
- THEN status is 301
- AND `Location: /face/mask/` (phase 5 fires before phase 6)
