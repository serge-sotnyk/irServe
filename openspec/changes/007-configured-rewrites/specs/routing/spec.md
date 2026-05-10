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
4. **`rewrites`** — chained matching `rewrites` entries serve the
   final destination file with status 200 (see rewrites capability).
   Implicit `--single` rewrites participate at this stage. Each
   matched rule is removed from the active list and the loop
   re-runs on the rewritten path; the path produced by the final
   successful pass is used by stages 5/6.
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

When a rewrite matches, the rewritten path's resolution SHALL be the
ONLY candidate tried at stages 5/6 — mirroring the reference's
`findRelated`'s `rewrittenPath ? [rewrittenPath] :
getPossiblePaths(...)` branch at `serve-handler/src/index.js:618-632`.
The rewrite displaces both the original path and any cleanUrls
candidates derived from the original path.

Stages 1 and 2 are unaffected by this gating: they trigger on the URL
form, not on file existence.

Evidence: SRV-ROUT-006 (status: verified, level: L2); oracle:
ORC-014, ORC-016, ORC-017, ORC-018, ORC-020, ORC-032, ORC-033,
ORC-083 (trailingSlash↔redirects compose;
`cases/prec-trailing-redirects.json#trailing_add_wins_over_redirect`),
ORC-148 (rewrite chaining;
`cases/rewrites-chain.json#chain_a_to_b_to_c`), ORC-152
(`cases/single-with-redirect.json#redirect_wins_over_single` —
redirect wins over `--single` rewrite).

Implementation: `crates/irserve-core/src/dispatch.rs::dispatch`
realizes the pipeline. Stages 1, 2, 5 (cleanUrls + trailingSlash)
were wired in 6b/6c; stage 3 (redirects) in 6d. Stage 4
(rewrites + `--single`, phase 7 of the 13-phase pipeline) is wired
in 6e via `rewrites::compute_configured_rewrites` walking the
precompiled `Vec<RewriteRuleCompiled>` with chained recursion, and
the dispatcher's extension-aware fork:

- Has-extension paths: `resolve(url_path)` first; on `NotFound`,
  apply rewrites; on rewrite match, `resolve(target)`; otherwise
  fall to `try_clean_urls_resolve(url_path)`.
- Extensionless paths: apply rewrites; on match, `resolve(target)`;
  otherwise fall to the standard cleanUrls + resolve chain.

The redirects↔rewrites compose corner of SRV-ROUT-006 was already
half-pinned by 6d (`prec-rewrites-redirects#go_root` — redirect-
wins side); 6e closes the rewrite-wins side via
`single-with-redirect#redirect_wins_over_single` (redirect still
fires before `--single`'s catch-all rewrite, proving phase 6
precedes phase 7). The cleanUrls↔rewrites compose corner is
pinned by `prec-rewrites-redirects#page_html_cleanurl_default`
(cleanUrls 301 fires for `/page.html` before any rewrite).

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

#### Scenario: Rewrite chain produces final destination

- GIVEN rewrites `[{source: "/a", destination: "/b"}, {source: "/b", destination: "/c.html"}]` and only `/c.html` exists
- WHEN `GET /a`
- THEN status is 200
- AND body is the contents of `/c.html`
