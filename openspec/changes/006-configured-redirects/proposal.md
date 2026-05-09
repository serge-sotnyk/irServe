# Proposal: Configured redirects (phase 6)

## Why

Stage 6c shipped `cleanUrls` (`005-clean-urls`): phases 4 and 8 of the
13-phase dispatcher are wired, with explicit comment-stubs for phases
6 (configured redirects) and 7 (rewrites). The next un-defer slice in
the L1/L2 roadmap (`docs/stage6_l1_l2_capabilities.md:52`) is
configured redirects — phase 6 of the dispatcher.

This change wires phase 6 against the existing
`openspec/specs/redirects/spec.md` contract (already `verified` for all
three SRV-RDIR requirements; only oracle lists need updating). It also
closes Q-007 (the open question on how the reference renders the
`Location` header for absolute / scheme-relative / relative
destination forms) and extends the SRV-ROUT-006 precedence surface
with the trailingSlash↔redirects compose corner.

## What

- **Phase 6** — configured `redirects` from `serve.json`
  (SRV-RDIR-001/002/003). Mirrors `shouldRedirect`'s redirects branch
  at `serve-handler/src/index.js:172-182`: first-match-wins iteration
  over the rules, returning `{target, statusCode}` for the first rule
  whose source matches the (already-decoded, already-collapsed)
  request path. Source patterns route through three compiled
  matchers in `crates/irserve-core/src/redirects.rs`:
  - `Literal` — sources without glob meta and without `:name`
    segments. Trailing-slash flexion mirrors `pathToRegExp("/old", [])
    = ^/old/?$`, so `/old` and `/old/` are interchangeable.
  - `Glob` — sources with glob meta (`*`, `?`, `[`, `{`) but no
    `:name`. Compiled via `globset::GlobBuilder` with
    `literal_separator(true)` so a single `*` does not cross `/`,
    matching minimatch's default. `!`-prefix negation works through
    the same XOR pattern as cleanUrls (the shared `slasher` from
    `serve-handler/src/glob-slash.js:8`).
  - `Pattern` — sources with `:name` segments (and possibly `*`
    tokens). Compiled into a `regex::Regex` with named capture
    groups, mirroring the reference's
    `slashed.replace('*', '(.*)') + pathToRegExp(normalized, keys)`
    first-pass at `index.js:46-49`. `:name` becomes
    `(?P<name>[^/]+)`; `*` becomes `(.*)`; literals are
    `regex::escape`d. The regex is anchored `^...\/?$` (optional
    trailing slash).

- **Destination interpolation.** `:name` segments in destinations
  are pre-parsed into a `DestTemplate` (Vec of literal + param
  fragments). At render time each captured value is
  `encodeURIComponent`-ed before splicing, mirroring
  `pathToRegExp.compile`'s per-prop encoding (`index.js:81-87`). The
  surrounding `encodeURI` over the full target lives in the
  pre-existing `dispatch::encode_uri_target` helper from 6b. A
  destination `:name` without a matching source capture renders as
  empty (fail-open), where the reference would throw at request
  time.

- **Destination normalization (Q-007 closure).** The reference's
  `toTarget` at `index.js:80` runs `protocol ? destination :
  slasher(destination)` where `slasher` is `glob-slash`'s
  `path.posix.normalize` plus a leading-slash guarantee. This means:
  - Absolute URLs (`https://example.com/x`) skip normalization and
    pass through verbatim.
  - Scheme-relative URLs (`//example.com/x`) are normalized to
    `/example.com/x` because `path.posix.normalize` collapses
    consecutive slashes — they become same-origin redirects, not
    true scheme-relative URLs (the Q-007 surprise).
  - Relative paths (`foo/bar`) get a leading `/` prepended.
  - Absolute paths (`/foo/bar`) pass through unchanged.
  IrServe mirrors all four exactly via
  `redirects::normalize_destination`. Implemented as `collapse_slashes`
  + leading-`/` guarantee — fuller `path.posix.normalize` semantics
  (`.`/`..` resolution) are not implemented because real-world
  redirect destinations don't carry those segments.

- **`type` override (SRV-RDIR-002).** A rule's optional `type`
  field overrides the default 301 status code. Out-of-range u16
  values (e.g. 999) fall back to 301, mirroring the spec's "accept
  any 3xx; range-checking is not specified" stance. Implemented via
  the new `redirect_with_status(target, status)` helper in
  `dispatch.rs` (which generalizes the existing `redirect_301`).

- **Compile-time error handling.** `compile_rules` returns
  `(Vec<RedirectRuleCompiled>, Vec<InvalidRedirect>)`. Invalid glob
  patterns and invalid path-pattern regexes are silently skipped at
  startup with a stderr warning emitted from `server.rs::serve`,
  mirroring the cleanUrls treatment from 6c and the reference's
  silent try/catch at `index.js:38-67`. A new `CompileError` enum
  carries the variant (`Glob | Regex | NegatedParam`).

- **Probe coverage.** The following anchors flip into
  `runner.l0.clean`:
  - `redirects-types#explicit_302` (literal source, type override).
  - `redirects-types#default_301_segment` (path-segment source,
    `:id` interpolation).
  - `prec-rewrites-redirects#go_root` (literal redirect wins over
    competing rewrite).
  - `prec-rewrites-redirects#page_html_cleanurl_default` (cleanUrls
    301 wins over redirect — already correct post-6c, just newly
    declared L0-clean).
  - `redirects-destination-forms#{absolute_https, scheme_relative,
    relative_no_leading_slash, absolute_path_baseline}` (Q-007
    closure, four destination forms).
  - `prec-trailing-redirects#trailing_add_wins_over_redirect`
    (trailingSlash 301 wins over redirect — closes the
    trailingSlash↔redirects corner of SRV-ROUT-006).
  All anchors mark `contentLengthMayDiffer`: axum emits
  `content-length: 0` on empty redirects; the reference's Node
  `http` does not. Same treatment as cleanUrls in 6c.

## Out of scope

- **Extglob support** in glob source patterns (`+(...)`, `@(...)`,
  `?(...)`, `*(...)`, `!(...)`). The reference inherits these from
  minimatch via the shared `sourceMatches` helper, but `globset`
  does not support them. Inherits Q-012's open status and
  `globset` limitation from cleanUrls; redirects glob sources have
  the same restriction.

- **Redirects↔rewrites compose corner of SRV-ROUT-006.** The
  redirect-wins side is pinned (`prec-rewrites-redirects#go_root`),
  but the full surface (rewrite-wins when redirect doesn't match)
  needs phase 7 wired. Lands in 6e (`007-configured-rewrites`).

- **`:name(custom-regex)` source patterns.** path-to-regexp v3
  supports `:foo(\d+)` for custom per-segment regexes. Not in 6d's
  scope; if a probe surfaces a user need, escalate to a Q-NNN.

- **Destination `path.posix.normalize` `.`/`..` resolution.**
  `redirects::normalize_destination` only collapses consecutive
  slashes; it does not resolve `.` or `..` segments. Real-world
  redirect destinations don't carry those segments; if a divergence
  surfaces, escalate to a Q-NNN.

- **`:name` + `!`-prefix negation combination.** Rejected at compile
  time via `CompileError::NegatedParam`. The reference's minimatch
  fallback for that combination treats `:name` as literal characters
  that no real request path will match — the rule never fires there
  either. Surfaced as a stderr warning rather than silently producing
  a never-matching rule.
