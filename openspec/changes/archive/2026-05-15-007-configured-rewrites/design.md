# Design: Configured rewrites + `--single` SPA fallback (phase 7)

This document records the architecture for Stage 6e. It is a peer
of `006-configured-redirects/design.md`; many sections cross-reference
the 6d design rather than duplicate it.

## 1. Pipeline placement

Phase 7 sits between phase 6 (configured redirects, Stage 6d) and
phases 8/9 (cleanUrls resolution + final resolve). The 13-phase
order from `openspec/changes/archive/2026-05-15-001-port-minimal-static-server/design.md`
is unchanged; only the previously-stub phase 7 slot is now wired.

Per `openspec/specs/rewrites/spec.md` and the reference at
`serve-handler/src/index.js:608-642`, phase 7 interleaves with
phase 8/9 file resolution rather than running as a free-standing
pre-resolution stage:

| Original path | File exists? | Behavior |
|---|---|---|
| Has extension (`/page.html`) | Yes | Pre-stat short-circuits. Rewrites NEVER consulted. Serve directly. |
| Has extension (`/page.html`) | No | Apply rewrites. If matched, resolve the rewritten path; the rewrite wins over `<P>.html` cleanUrls candidates (mirrors `findRelated`'s `rewrittenPath ? [rewrittenPath] : getPossiblePaths(...)`). If unmatched, fall to cleanUrls candidate. |
| Extensionless (`/about`) | n/a | No pre-stat. Apply rewrites unconditionally. If matched, resolve the rewritten path; rewrite wins even when extensionless original exists as a file. If unmatched, current cleanUrls + resolve chain runs. |

This is implemented inside the existing extension-split fork in
`crates/irserve-core/src/dispatch.rs`, replacing the single stub
comment `// Phase 7: rewrites + --single (Stage 6e).` with a fork-
aware block that consults `compute_configured_rewrites` and
delegates to `resolve(target, root)` on a hit. The split was
already there for the cleanUrls 6c work, so the change is local.

## 2. Matcher reuse via `path_pattern.rs`

Stage 6d's source-pattern compiler is identical to what rewrites
need: same `Literal`/`Glob`/`Pattern` routing, same `:name`/`*`/
glob-meta classification, same `:name` regex compiler, same
destination template parser, same `path.posix.resolve` and slasher
helpers, same default `i` flag, same dot-rule, same backslash and
escape handling, same Latin-1 case-folding.

Slice 1 of this change extracts the entire kernel into a new
`pub(crate) crates/irserve-core/src/path_pattern.rs` module:

- `pub(crate) enum Matcher { Literal, Glob, Pattern }` with all
  three variants and field shapes.
- `pub(crate) struct DestTemplate`, `enum DestFrag` for destination
  rendering.
- `pub(crate) struct GlobFallback`, `enum PatSeg` for the per-segment
  minimatch fallback.
- `pub enum CompileError { Glob, Regex }` (kept `pub`, re-exposed
  via `redirects::CompileError` and `rewrites::CompileError`).
- `pub(crate) fn match_segments`, `classify_pattern_segment`,
  `slasher`, `slasher_join_normalize`, `path_posix_normalize`,
  `path_posix_resolve`, `de_escape`, `de_escape_keep_trailing`,
  `ends_with_unescaped_backslash`, `has_protocol`,
  `normalize_destination`, `compile_source_regex`,
  `compile_dest_template`, `literal_matches`, `has_glob_meta`,
  `has_path_param`.
- `pub(crate) const ENCODE_URI_COMPONENT_SET`.

Two methods on `Matcher`:
- `pub(crate) fn compile(source: &str, destination: &str) -> Result<Matcher, CompileError>`
  — the routing classifier (lifted verbatim from
  `redirects::compile_one`).
- `pub(crate) fn try_match(&self, path: &str) -> Option<String>`
  — the per-rule matcher (lifted verbatim from
  `redirects::RedirectRuleCompiled::try_match`).

After the extraction, `redirects.rs` keeps only redirect-specific
items (`RedirectRuleCompiled` with `status_code`, `InvalidRedirect`,
`compile_rules`, `compute_configured_redirects`); each becomes a
thin wrapper over `Matcher::compile` / `Matcher::try_match`.

Slice 1 ships zero behavior change: 187/187 irserve-core unit
tests + all 6d-pinned ORCs remain green after the move.

## 3. Recursive chaining (`apply_rewrites`)

Mirrors `serve-handler/src/index.js:91-117`:

```js
const applyRewrites = (requestPath, rewrites = [], repetitive) => {
    const rewritesCopy = rewrites.slice();
    const fallback = repetitive ? requestPath : null;
    if (rewritesCopy.length === 0) return fallback;
    for (let index = 0; index < rewritesCopy.length; index++) {
        const {source, destination} = rewrites[index];
        const target = toTarget(source, destination, requestPath);
        if (target) {
            rewritesCopy.splice(index, 1);
            return applyRewrites(slasher(target), rewritesCopy, true);
        }
    }
    return fallback;
};
```

IrServe's `apply_rewrites` (private, called from
`compute_configured_rewrites`) takes `(request_path, active:
&[&RewriteRuleCompiled], repetitive: bool, depth: usize)` and
returns `Option<String>`:

- On the first call with `repetitive=false`, fallback is `None` —
  "no rule matched" propagates as `None` so the dispatcher falls
  through to the standard cleanUrls + resolve chain.
- On recursive calls with `repetitive=true`, fallback is
  `Some(request_path.to_string())` — the chain has already produced
  at least one rewrite, so an exhausted-list / no-further-match
  outcome returns the latest rewritten path.
- On match, the matched rule is removed from `active` (Vec<&T>
  reallocated minus the matched index — O(n²) with n ≤ 64 cap; not
  a hot path) and `apply_rewrites` recurses on the rendered target
  with `repetitive=true` and `depth + 1`.

### slasher(target) on recursion

The reference applies `slasher(target)` to the rewritten path
before recursing. IrServe does NOT re-slasher: `Matcher::compile`'s
`normalize_destination` already runs slasher on the destination
template at compile time, and `compile_dest_template`'s capture-
rendering applies `encodeURIComponent` per value (which encodes
`/` to `%2F`), so captured segments cannot re-introduce path
separators. For probe-pinned cases the recursive slasher would be
a no-op. If a future probe surfaces a divergence (e.g. a captured
`..` from a literal-`:name` request), the fix is a one-line
`slasher_join_normalize` before recursion. Documented in D-013.

### Depth cap (D-014)

`REWRITE_DEPTH_CAP: usize = 64`. On overflow the function returns
`fallback` — i.e. the path captured by the most recent successful
pass. No error response, no panic. The cap is irserve-only; the
reference imposes none. Justification:

- Real-world `serve` rule counts are 2-3.
- The splice model consumes one rule per pass, so any non-cyclic
  configuration terminates in ≤ N passes (N = rule count).
- Intentional self-cycles (e.g. `/a → /a`) terminate naturally
  after one pass because the matched rule is removed.
- Two-rule cycles (`/a → /b`, `/b → /a`) terminate after both
  rules have fired once.
- A configuration that hits the cap would also stack-overflow V8.

Unit tests in `crates/irserve-core/src/rewrites.rs` exercise:
single-rule, chain-two-rules (both rule orders), two-rule cycle,
self-loop, first-match-wins on the initial call, depth-cap clamp
(65 self-matching rules → returns `Some("/a")` from the cap
fallback rather than overflowing).

## 4. `--single` CLI injection

Per `third_party/serve/source/main.ts:78-90`, the reference
prepends a synthetic rewrite at config-load time:

```ts
if (args['--single']) {
    const { rewrites } = config;
    const existingRewrites = Array.isArray(rewrites) ? rewrites : [];
    config.rewrites = [
        { source: '**', destination: '/index.html' },
        ...existingRewrites,
    ];
}
```

IrServe's `crates/irserve/src/main.rs` mirrors this verbatim:
the new `-s`/`--single` clap flag, when set, prepends `RewriteRule
{ source: "**".to_string(), destination: "/index.html".to_string() }`
to the loaded `serve_config.rewrites` before the config is passed
to `run`. The remainder of the pipeline is unchanged.

Three contractual consequences:

1. **Earlier phases win.** A configured redirect (phase 6) fires
   before the `--single` rewrite (phase 7). Pinned by
   `single-with-redirect#redirect_wins_over_single`.
2. **Pre-stat asymmetry still applies.** A has-extension request
   whose original file exists is served directly; the `**`
   catch-all does not displace it. Pinned by
   `single-flag#spa_with_existing_html` (with cleanUrls disabled
   to isolate phase-7 behavior).
3. **Prepend-position shadows user rewrites.** Because `**` is
   prepended FIRST and matches every path, user rewrites listed
   in `serve.json` are matched against the rewritten path
   (`/index.html`) — they don't fire. Pinned by
   `single-with-rewrites#extensionless_user_path_shadowed_by_single`.

`tools/probe/run.mjs` had `-s`/`--single` in `L0_DEFERRED_FLAGS`
(D-008). Slice 5 removes that entry; the existing dual `serveArgs`
plumbing at `run.mjs:169`/`:173` already passes the flag to both
reference and irserve targets without further changes.

## 5. Composition with phases 4–6

The redirects↔rewrites compose corner of SRV-ROUT-006 was left
half-pinned by 6d (redirect-wins side via
`prec-rewrites-redirects#go_root`). Stage 6e closes the rewrite-
wins side via the new `single-with-redirect` probe (which proves
phase 6 still fires before phase 7) and the existing
`prec-rewrites-redirects#page_html_cleanurl_default` (which proves
phase 4 cleanUrls 301 fires before phase 7).

The trailingSlash↔rewrites compose corner is implicitly closed
by the same SRV-ROUT-006 framework: phase 5 (trailingSlash 301)
fires on URL form before phase 7 reads `url_path`. No new probe
is needed.

## 6. Compatibility with redirect Compatibility notes

D-012's known divergences (Unicode special folds, `\*\*`,
`{a\,b,c}`, segment-internal trailing `\` in glob sources) inherit
unchanged because rewrites use the same `path_pattern` matcher as
redirects. They are not re-listed in the rewrites spec delta;
cross-reference D-012 instead.

## 7. Out-of-scope reaffirmation

Listed in `proposal.md` §"Out of scope". Two items deserve a
design-level note:

- **SRV-RWRT-002 (L3 MIME fallback).** When the rewritten path
  doesn't resolve, `serve-handler` derives Content-Type from the
  rewritten extension and continues. IrServe's `resolve()` returns
  `NotFound`, dispatcher returns 404. Cleanest landing site is
  Stage 7+ when L3 cache + MIME polish ships.
- **Absolute-URL rewrite destinations.** `Matcher::compile` calls
  `normalize_destination` which preserves protocol-bearing
  destinations verbatim. The dispatcher then calls `resolve` with
  the verbatim string, which fails to canonicalize against the
  filesystem root → `EscapedRoot` or `NotFound` → 404. This
  matches the reference's behavior (its `lstat(absolutePath)` also
  fails) and needs no new code.
