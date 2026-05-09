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

### Requirement: Multi-slash path is normalized before routing

The server SHALL normalize consecutive slashes in the request path to
a single slash before subsequent routing stages. Whether a 301
redirect or a 200 response follows SHALL be governed by the
subsequent pipeline stages (cleanUrls, trailingSlash, redirects,
rewrites, static-file resolution) acting on the normalized path —
slash collapse itself SHALL NOT emit a redirect when `trailingSlash`
is unset. When `trailingSlash` IS set and the input path contains
`//`, the server SHALL emit a 301 to the slash-collapsed form,
overriding the add/strip targets that the trailingSlash rule would
otherwise compute.

Evidence: SRV-ROUT-005 (status: verified, level: L2); oracle: ORC-025
(`cases/multislash-collapse.json#double_slash_root`, silent collapse
under default `trailingSlash`), ORC-026, ORC-027 (compose with
cleanUrls 301; reference-only until 6c lands phase 4), ORC-072
(`cases/trailingslash-add.json#trailing_double_slash_collapses_via_redirect`,
`trailingSlash: true` + `//` → 301 to collapsed form), ORC-073
(`cases/trailingslash-add.json#encoded_double_slash_collapses_via_redirect`,
`%2F%2F` decodes to `//` and triggers the same 301), ORC-074
(`cases/trailingslash-strip.json#trailing_double_slash_collapses_via_redirect`,
`trailingSlash: false` + `//` → 301 to collapsed form, NOT to the
strip target).

Implementation: `crates/irserve-core/src/normalize.rs::collapse_slashes`
performs the silent collapse for the path that flows into resolve.
The multi-slash-with-trailingSlash override lives in
`crates/irserve-core/src/trailing_slash.rs::
compute_trailing_slash_redirect` (top branch), mirroring the
reference's coupling at `serve-handler/src/index.js:158-160`. URL
percent-decoding runs at dispatcher entry
(`crates/irserve-core/src/dispatch.rs`) so encoded `%2F%2F` decodes
to `//` and participates identically.

Note: Q-006 closed by ORC-025 (silent collapse holds when
`trailingSlash` is unset).

#### Scenario: Double-slash with `trailingSlash: true` redirects to collapsed form

- GIVEN `trailingSlash: true`, `cleanUrls: false`
- WHEN `GET /about//`
- THEN status is 301
- AND `Location: /about/` (the `//` triggers a 301 to the slash-collapsed form regardless of the add branch)

#### Scenario: Encoded double-slash decodes and collapses

- GIVEN `trailingSlash: true`, `cleanUrls: false`
- WHEN `GET /about%2F%2F`
- THEN status is 301
- AND `Location: /about/`
