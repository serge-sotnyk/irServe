# Delta for rewrites

## ADDED Requirements

### Requirement: Rewrite rules chain via rule-removal recursion

The server SHALL apply `rewrites` rules with chaining: on the first
matching rule, the matched rule SHALL be removed from the active
list and the matching loop SHALL re-run on the rewritten path
against the remaining rules. The process SHALL terminate when no
remaining rule matches or when the active list is empty; the path
produced by the final successful pass SHALL be used by phases 8/9
(file resolution).

The first call (with no prior match) SHALL return `None` (no
rewrite applied) when no rule matches. Recursive calls (with at
least one prior match) SHALL return the most recent rewritten
path when no further rule matches — mirroring the reference's
`fallback = repetitive ? requestPath : null` at
`serve-handler/src/index.js:97`.

Evidence: SRV-RWRT-001 (status: verified, level: L2); oracle:
ORC-148 (`cases/rewrites-chain.json#chain_a_to_b_to_c`); ORC-154
(`cases/rewrites-prestat.json#extensioned_skipped`); ORC-155
(`cases/rewrites-prestat.json#extensionless_wins`); ORC-156
(`cases/rewrites-prestat.json#extensionless_miss_falls_back`);
ORC-157 (`cases/rewrites-extensioned-miss.json#extensioned_falls_to_rewrite`);
D-013.

Implementation: `crates/irserve-core/src/rewrites.rs::apply_rewrites`
mirrors `applyRewrites` at `serve-handler/src/index.js:91-117`. On
match, the matched rule is removed from the active rule list (Vec
of references; reallocated minus the matched index) and
`apply_rewrites` recurses with `repetitive=true` on the rendered
target. Termination is guaranteed by the rule-removal: any
configuration of N rules can produce at most N consecutive
matching passes before the active list empties.

#### Scenario: Two-rule chain to final destination

- GIVEN rewrites `[{source: "/a", destination: "/b"}, {source: "/b", destination: "/c.html"}]` and only `/c.html` exists
- WHEN `GET /a`
- THEN status is 200
- AND body is the contents of `/c.html`

### Requirement: Rewrite chaining honors a recursion-depth cap (irserve-only)

IrServe SHALL impose a hard cap of 64 on rewrite chaining
recursion depth. On overflow, the function SHALL gracefully clamp
to the path captured by the most recent successful pass; it SHALL
NOT emit an error response. This is a deliberate divergence from
the reference, which imposes no cap (`applyRewrites` could in
principle stack-overflow V8 on a malformed cyclic configuration).

Evidence: D-014 (status: `adapted`).

Implementation: `crates/irserve-core/src/rewrites.rs` defines
`const REWRITE_DEPTH_CAP: usize = 64`. Unit tests cover the cap
behavior with a synthetic 65-rule self-matching configuration.

Note: The cap is unreachable in any realistic configuration:
real-world `serve` rule counts are 2-3, the splice model consumes
one rule per pass, and intentional cycles (self-loops, two-rule
A↔B cycles) terminate naturally after every rule has fired once.

#### Scenario: Synthetic 65-rule self-matching configuration clamps gracefully

- GIVEN 65 identical `rewrites` rules of the form `{source: "/a", destination: "/a"}`
- WHEN `GET /a` (such that 64 chained passes succeed before the cap fires)
- THEN the server SHALL clamp to the path produced by the last successful pass (`/a`) and serve `/a` if it resolves, or 404 otherwise
- AND no error response, panic, or 5xx SHALL be emitted

### Requirement: `--single` CLI flag prepends a synthetic catch-all rewrite

When the CLI receives `-s`/`--single`, the server SHALL prepend
the rule `{source: "**", destination: "/index.html"}` to the
user's `serve.json` rewrites at config-load time, BEFORE the
first request is served. The combined list SHALL participate in
the standard phase-7 rewrite pipeline. Earlier-firing phases
(cleanUrls 301, trailingSlash, redirects) SHALL be unaffected —
a configured redirect SHALL still win for any request whose path
matches it.

Within phase 7, the synthetic catch-all matches first under the
prepend-position contract, so user rewrites listed in
`serve.json` after `--single` are matched against the rewritten
path (`/index.html`) and so generally do not fire.

Evidence: SRV-CLI-008 (status: verified, level: L2); oracle:
ORC-149, ORC-150, ORC-151
(`cases/single-flag.json#{spa_root, spa_deep,
spa_with_existing_html}`), ORC-152
(`cases/single-with-redirect.json#redirect_wins_over_single`),
ORC-153
(`cases/single-with-rewrites.json#extensionless_user_path_shadowed_by_single`);
D-013.

Implementation: `crates/irserve/src/main.rs` defines the clap
flag (`#[arg(short = 's', long = "single")]`). When set, after
`load_serve_json` returns, `main` prepends `RewriteRule { source:
"**", destination: "/index.html" }` to `serve_config.rewrites`
and lets the standard `compile_rewrite_rules` + `dispatch`
pipeline handle the rest. Mirrors `third_party/serve/source/
main.ts:78-90`.

#### Scenario: SPA root request

- GIVEN `serve --single` over a directory containing `/index.html`
- WHEN `GET /`
- THEN status is 200
- AND body is the contents of `/index.html`

#### Scenario: Deep extensionless request

- GIVEN `serve --single` over a directory containing `/index.html`
- WHEN `GET /some/deep/path`
- THEN status is 200
- AND body is the contents of `/index.html`

#### Scenario: Redirect beats `--single` SPA fallback

- GIVEN `serve --single` and `serve.json` with redirect `/old → /new`
- WHEN `GET /old`
- THEN status is 301 (the redirect, not the SPA fallback)
- AND `Location: /new`

## Compatibility notes

- **Recursion-depth cap divergence (D-014).** The reference
  imposes no cap; IrServe imposes 64 with graceful clamp. No
  observable difference for any non-pathological configuration.
- **Inherited from D-012 (redirects).** All path-pattern matcher
  divergences documented for redirects (Unicode special folds,
  `\*\*` per-alternative parsing, `{a\,b,c}` brace-with-escaped-
  comma, segment-internal trailing `\` in glob sources on
  Windows) inherit unchanged because rewrites use the same
  `path_pattern` matcher. Cross-reference D-012 in
  `docs/reference/serve/decisions.md`.
