# Proposal: Configured rewrites + `--single` SPA fallback (phase 7)

## Why

Stage 6d shipped configured redirects (`006-configured-redirects`):
phase 6 of the 13-phase dispatcher is wired, with an explicit
comment-stub for phase 7 (rewrites). The next un-defer slice in the
L1/L2 roadmap (`docs/stage6_l1_l2_capabilities.md:53`) is configured
rewrites + the `--single` SPA fallback — phase 7 of the dispatcher.

This change closes SRV-RWRT-001 (`rewrites` serve a different file
with status 200) and SRV-CLI-008 (`-s`/`--single` rewrites all
not-found requests to `/index.html`) — both `verified` against the
reference but deferred from the strict-L0 cutoff per `D-008`. It
also closes the redirects↔rewrites compose corner of SRV-ROUT-006
left open by 6d (`prec-rewrites-redirects#go_root` already pinned
the redirect-wins side; the rewrite-wins side now lands).

## What

- **Phase 7** — configured `rewrites` from `serve.json`
  (SRV-RWRT-001). Mirrors `applyRewrites` at
  `serve-handler/src/index.js:91-117`: walk the user's rules in
  declared order; on a match, remove the matched rule from the
  active list and recurse on the rewritten path. Termination
  happens when no remaining rule matches or the active list is
  empty; the path produced by the final successful pass is the
  rewrite output. Returns `None` when no rule matches the original
  request path. Unlike redirects (phase 6), a matched rewrite does
  NOT emit a 3xx response — the rewritten path replaces the
  original `url_path` and the dispatcher continues to file
  resolution (phases 8/9) on the rewritten value.

- **Pre-stat asymmetry.** Phase 7 interleaves with phases 8/9 per
  `openspec/specs/rewrites/spec.md:21-29` and mirrored from
  `serve-handler/src/index.js:608-642`:
  - **Has-extension + file exists** → pre-stat short-circuits;
    rewrites are NEVER consulted (`index.js:608-616`).
  - **Has-extension + file missing** → apply rewrites; the
    rewritten path's resolution wins over `<P>.html` cleanUrls
    candidates (mirrors `findRelated`'s `rewrittenPath ?
    [rewrittenPath] : getPossiblePaths(...)` branch).
  - **Extensionless** → apply rewrites unconditionally; a matching
    rewrite serves its destination even when the extensionless
    original exists as a regular file.

- **`--single` SPA fallback (SRV-CLI-008).** Per
  `third_party/serve/source/main.ts:78-90`, the CLI prepends a
  synthetic rewrite `{source: "**", destination: "/index.html"}`
  to the user's rewrites list at config-load time. The combined
  list participates in the standard phase-7 rewrite pipeline.
  Earlier-firing phases (cleanUrls 301, trailingSlash, redirects)
  are unaffected — a redirect (phase 6) still wins. Within phase 7
  the synthetic rule is matched first (prepend-position contract),
  so user rewrites listed AFTER `--single` are effectively shadowed
  by the catch-all unless a user rule fires earlier in the chain.

- **Matcher reuse.** Stage 6d's source-pattern compiler (Literal /
  Glob / Pattern routing, `:name` / `*` / glob-meta classification,
  destination interpolation, `path.posix.resolve` semantics, default
  `i` flag, dot-rule, backslash handling) is identical to what
  rewrites need. Slice 1 of this change extracted those primitives
  from `crates/irserve-core/src/redirects.rs` into a new
  `crates/irserve-core/src/path_pattern.rs` module. Both phase-6
  and phase-7 modules now reduce to thin wrappers
  (`compile_one` → `Matcher::compile`, `try_match` → method on
  `Matcher`). The 6d plan explicitly anticipated this factoring:
  *"If a third capability ends up duplicating the compiler in 6e,
  factor out into `path_pattern.rs` then."*

- **Recursion-depth cap (irserve-only).** The reference imposes no
  cap on the chaining loop — `applyRewrites` could in principle
  recurse until V8 stack overflow on a malformed cyclic
  configuration. IrServe adds a hard cap of 64 as defense-in-depth;
  on overflow, the function gracefully clamps to the path produced
  by the most recent successful pass instead of erroring. This is
  documented as `D-014` (status: `adapted`). The cap is unreachable
  in any realistic configuration: real-world `serve` rule counts
  are 2-3, the `splice` model consumes one rule per pass, and
  intentional cycles terminate naturally after every rule has fired
  once.

- **Probe coverage.** The following anchors flip into
  `runner.l0.clean`:
  - `rewrites-segment#segment_rewrite` (path-to-regexp segment;
    ORC-028).
  - `rewrites-segment#spa_fallback` (SPA wildcard; ORC-029).
  - `rewrites-chain#chain_a_to_b_to_c` (NEW probe; recursive
    chaining; ORC-148; D-013).
  - `single-flag#{spa_root, spa_deep, spa_with_existing_html}` (NEW
    probe; SRV-CLI-008; ORC-149/150/151; cleanUrls disabled in
    fixture to isolate phase-7 behavior — with cleanUrls on, the
    existing-html anchor would 301 via phase 4 instead of testing
    the pre-stat asymmetry).
  - `single-with-redirect#redirect_wins_over_single` (NEW probe;
    closes the redirects↔rewrites compose corner of SRV-ROUT-006;
    ORC-152; `contentLengthMayDiffer` because axum emits
    `content-length: 0` on empty redirects, the reference's Node
    `http` does not).
  - `single-with-rewrites#extensionless_user_path_shadowed_by_single`
    (NEW probe; pins prepend-position contract; ORC-153).

## Out of scope

- **L3 MIME fallback when destination missing (SRV-RWRT-002).**
  When the rewritten path's file does not exist, `serve-handler`
  continues by deriving Content-Type from the destination
  extension. IrServe's L0–L2 contract returns 404 in that case.
  Re-open in Stage 7+ if the L3 cut surfaces a need.

- **Extglob in rewrite source patterns.** Same `globset` limitation
  as cleanUrls (Q-012) and redirects. `+(...)`, `@(...)`, `?(...)`,
  `*(...)`, `!(...)` are not supported.

- **Headers-config interaction with rewrites.** SRV-HEAD-001 lands
  in Stage 6f (`008-error-pages-and-security`). The rewrite-then-
  headers pipeline coupling is decided there.

- **Absolute-URL or scheme-relative rewrite destinations.**
  `serve-handler/src/index.js:79-80` sets
  `normalizedDest = protocol ? destination : slasher(destination)`
  — a `https://example.com/x` destination would be passed verbatim
  as the file-system lookup key, which `lstat` cannot resolve. The
  reference returns 404; IrServe mirrors via the same path through
  `Matcher::compile`. Documented as a no-op guard inherited from
  redirects, not a new decision.

- **`:name(custom-regex)` source patterns.** path-to-regexp v3
  supports `:foo(\d+)` for custom per-segment regexes. Same
  exclusion as 6d; `path_pattern.rs` does not parse them.

- **Bug-for-bug parity with `applyRewrites`'s recursion limit.**
  The reference has none; IrServe adds the depth cap (D-014). Any
  configuration that would observe the divergence is also
  pathological in the reference (V8 stack overflow); no real
  user-facing impact.
