# Delta for routing

## MODIFIED Requirements

### Requirement: `trailingSlash: true` adds a trailing slash via 301

The server SHALL redirect (status 301) any extensionless non-dotfile
request without a trailing slash to the slash-appended form when
`trailingSlash` is `true`. Dotfiles and files with extensions SHALL
be exempt from the trailing-slash insertion.

Evidence: SRV-ROUT-003 (status: verified, level: L2); oracle: ORC-015
(`cases/prec-cleanurls-trailing.json#about_no_slash`, compose with
cleanUrls), ORC-016 (`cases/prec-cleanurls-trailing.json#about_with_slash`,
compose with cleanUrls), ORC-068 (`cases/trailingslash-add.json#about_no_slash_redirects`,
pure trailingSlash add with `cleanUrls: false`), ORC-069
(`cases/trailingslash-add.json#txt_with_extension_no_redirect`,
extension-exempt anti-redirect anchor).

Implementation: `crates/irserve-core/src/trailing_slash.rs::
compute_trailing_slash_redirect` mirrors `serve-handler/src/index.js:
121-185`'s `shouldRedirect` slashing add branch. The dotfile guard
uses `basename.starts_with('.')`; the extension guard uses "any non-
leading dot in the basename" to match Node's `path.parse(p).ext`
shape (`/foo.tar.gz` is exempt; `/.bashrc.bak` is exempt;
`/.htaccess` is exempt).

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

Evidence: SRV-ROUT-004 (status: verified, level: L2); oracle: ORC-019
(`cases/prec-cleanurls-trailing-false.json#about_with_slash`, compose
with cleanUrls), ORC-070 (`cases/trailingslash-strip.json#about_with_slash_redirects`,
pure trailingSlash strip with `cleanUrls: false`), ORC-071
(`cases/trailingslash-strip.json#about_no_slash_no_redirect`,
no-trailing-slash anti-redirect anchor).

Implementation: `crates/irserve-core/src/trailing_slash.rs::
compute_trailing_slash_redirect` strip branch ignores the
dotfile/extension exemptions (matching `serve-handler/src/index.js:
152`). The root path `/` is exempt because the stripped target would
be empty (mirrors the reference's falsy-target branch).

#### Scenario: Trailing slash stripped

- GIVEN `trailingSlash: false` and fixture has `about/index.html`
- WHEN `GET /about/`
- THEN status is 301
- AND `Location: /about`
