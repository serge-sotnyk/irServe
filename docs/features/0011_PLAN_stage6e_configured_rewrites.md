# Stage 6e — Configured rewrites + `--single` SPA fallback

## Context

Stage 6e is the next sub-stage of Stage 6 (L1+L2 capabilities). It wires
**phase 7** of the 13-phase dispatcher pipeline, immediately after phase 6
(redirects, Stage 6d) and before phase 8 (cleanUrls resolution). It un-defers
two SRVs from `D-008`:

- **SRV-RWRT-001** — configured `rewrites: [{source, destination}]` from
  `serve.json`. On match, the request path is internally rewritten (no 3xx)
  and the destination file is served with status 200. Spec already exists at
  `openspec/specs/rewrites/spec.md`.
- **SRV-CLI-008** — `-s`/`--single` SPA fallback, implemented as a
  high-priority rewrite `{source: "**", destination: "/index.html"}` prepended
  to user rewrites at config-load time, mirroring
  `third_party/serve/source/main.ts:78-90`.

Two binding decisions taken in plan-mode interview:

1. **Recursive chaining** — mirror `serve-handler/src/index.js:91-117`
   (`applyRewrites` removes the matched rule and recurses on the rewritten
   path). Add an irserve-only depth cap (= 64) as defense-in-depth; on
   overflow, gracefully clamp to the path from the last successful pass.
   Documented as `D-013` (un-defer) and `D-014` (cap-divergence from
   reference).
2. **Extract shared `path_pattern.rs`** — slice 1 lifts the Literal/Glob/
   Pattern matcher kernel out of `redirects.rs` into a new module shared by
   redirects and rewrites. The 6d plan explicitly anticipated this.

## Pipeline integration

Reference behavior (`serve-handler/src/index.js:608-642`) interleaves pre-stat
with rewrites:

- **Has-extension + file exists** → pre-stat short-circuits, rewrites NEVER
  consulted, file served direct.
- **Extensionless** → no pre-stat, rewrites always consulted, rewrite wins
  even when the extensionless original exists as a regular file.
- **Has-extension + file missing** → rewrites consulted; rewritten path
  preferred over `<P>.html` cleanUrls candidates.

Phase 7 is wired inside the existing extension-split fork at
`dispatch.rs:96-110`:

- **Extensionless branch:** apply rewrites first, then
  `try_clean_urls_resolve` → `resolve` on the (possibly-rewritten) path.
- **Has-extension branch:** `resolve(url_path)` first; on `NotFound`, apply
  rewrites; if the path changed, `resolve` the rewritten path; else fall to
  `try_clean_urls_resolve(url_path)`.

`apply_rewrites` returns `Option<String>` — `Some` only when an actual
rewrite occurred (so callers can `unwrap_or(url_path)` cleanly).

## Slice plan (ask before each commit)

| # | Goal | Files | Verify |
|---|------|-------|--------|
| 1 | Extract `path_pattern.rs` from `redirects.rs` (no behavior change). Subagent OK. | new `crates/irserve-core/src/path_pattern.rs`; `redirects.rs`, `lib.rs` | `cargo test -p irserve-core` + `cargo test --test oracle` (all 6d ORCs stay green) |
| 2 | Phase 7 wiring + literal/glob rewrite forms (single-pass). Pre-stat asymmetry from day one. | `dispatch.rs`, new `crates/irserve-core/src/rewrites.rs`, `lib.rs`, `server.rs` (compile rules at startup, thread `&[RewriteRuleCompiled]` into dispatch); `config.rs` (`RewriteRule`) | `cargo test --test oracle` for `rewrites-segment#spa_fallback` (ORC-029) |
| 3 | `:name` source patterns + destination interpolation (single-pass). | `crates/irserve-core/src/rewrites.rs` (Pattern variant) | `rewrites-segment#segment_rewrite` (ORC-028) |
| 4 | Recursive chaining + depth cap (= 64). | `rewrites.rs::apply_rewrites`. New probe `rewrites-chain.json` (record reference first) | new chain anchor green |
| 5 | `--single` CLI flag. Inject synthetic rule at config-load time; remove `--single` from `tools/probe/run.mjs:47-54` `L0_DEFERRED_FLAGS`. | `crates/irserve-bin/src/main.rs` (clap), `server.rs` or `config.rs` (rule injection), `run.mjs`. New probes `single-flag.json`, `single-with-rewrites.json`, `single-with-redirect.json`. | new single-flag anchors green |
| 6 | Spec deltas + decisions log + meta. Validate. | `openspec/changes/007-configured-rewrites/{proposal,design,tasks}.md` + `specs/{rewrites,routing,cli}/spec.md`; `docs/reference/serve/decisions.md` (D-013, D-014); `inventory.md`, `oracle-matrix.md`; `README.md`, `docs/stage6_l1_l2_capabilities.md` | `npx -y @fission-ai/openspec@latest validate --all --strict` |

## Critical files

Read / extend (do not rewrite from scratch):

- `crates/irserve-core/src/dispatch.rs:75` — phase-7 stub.
- `crates/irserve-core/src/redirects.rs` — source of `path_pattern.rs`
  extraction. Helpers to lift verbatim: `compile_source_regex`,
  `compile_dest_template` + `DestTemplate` + `DestFrag`, `slasher`,
  `path_posix_resolve`, `path_posix_normalize`, `slasher_join_normalize`,
  `normalize_destination`, `has_protocol`, `de_escape`,
  `de_escape_keep_trailing`, `ends_with_unescaped_backslash`,
  `literal_matches`, `has_glob_meta`, `has_path_param`,
  `classify_pattern_segment`, `build_dot_only_matcher`, `match_segments`,
  `PatSeg`, `GlobFallback`, `ENCODE_URI_COMPONENT_SET`. The `Matcher` enum
  shape becomes generic `PathPatternKind` (without `destination`); each
  capability wraps it with its own `destination` type and per-rule
  `try_match` returning `Option<String>` (rewrites) or
  `Option<(String, u16)>` (redirects).
- `crates/irserve-core/src/config.rs` — add `RewriteRule { source,
  destination }` next to existing `RedirectRule`.
- `crates/irserve-core/src/server.rs` — compile rewrite rules at startup
  alongside redirect rules; thread `&[RewriteRuleCompiled]` into `dispatch`.
- `tools/probe/run.mjs:47-54` (`L0_DEFERRED_FLAGS`) and the dual
  `serveArgs` plumbing at lines 169 / 173 — `--single` plumbing already
  symmetric; only the deferred-flag entry needs removal in slice 5.
- `third_party/serve-handler/src/index.js:91-117` (`applyRewrites`),
  `:38-89` (`sourceMatches`/`toTarget`), `:608-642` (pre-stat-and-rewrite
  interleave), `:561-685` (full request path); `third_party/serve/source/
  main.ts:78-90` (`--single` injection). Do not modify these.
- Existing probes to keep: `tools/probe/cases/rewrites-segment.json`,
  `tools/probe/cases/prec-rewrites-redirects.json`. Add `runner.l0.clean`
  block to the former in slice 3.

New probe cases (each: record against reference first per
anti-hallucination rule 6):

- `rewrites-chain.json` — `chain_a_to_b_to_c`, `chain_order_independent`.
- `rewrites-prestat.json` — `extensioned_skipped`, `extensionless_wins`.
- `rewrites-extensioned-miss.json` — `extensioned_falls_to_rewrite`.
- `single-flag.json` — `spa_root`, `spa_deep`, `spa_with_existing_html`.
- `single-with-rewrites.json` — `user_rule_with_synthetic_prepended`
  (verifies `**` is prepended FIRST per `main.ts:78-90`).
- `single-with-redirect.json` — `redirect_wins_over_single` (phase 6 fires
  before phase 7).
- `rewrites-cycle.json` — `cycle_a_b`. Record reference first; if reference
  loops/stack-overflows, document as a divergence in D-014 and pin only
  irserve's terminating behavior.

## Spec deltas (in `openspec/changes/007-configured-rewrites/`)

- `specs/rewrites/spec.md` — **ADDED** Requirements: rewrite chaining;
  recursion-depth cap (irserve-only); `--single` SPA fallback. Existing
  Requirement at lines 13-31 stays unchanged.
- `specs/routing/spec.md` — **MODIFIED** SRV-ROUT-006 oracle list: un-defer
  redirects↔rewrites compose corner (per D-012 note "stays deferred to 6e");
  add `single-with-redirect` ORC and `rewrites-prestat` ORCs.
- `specs/cli/spec.md` — **MODIFIED** SRV-CLI-008 oracle list: un-defer from
  D-008; add `single-flag.json` ORCs.

## Decision log entries

Next free numbers (latest is D-012):

- **D-013** — Stage 6e un-defers SRV-RWRT-001 + SRV-CLI-008. Cite
  `index.js:91-117`, `index.js:608-642`, `main.ts:78-90`. Pin
  prepend-position contract for `--single`.
- **D-014** — Recursion-depth cap on rewrite chaining (irserve-only).
  Status `adapted`. Cap = 64; on overflow, clamp to last successful pass
  (no error response). Reference has no cap.

## Out of scope (anti-hallucination rule 10)

1. **SRV-RWRT-002** — L3 MIME fallback when destination missing. Stage 7+.
2. **Extglob in rewrite source** — inherits Q-012 (no extglob in
   `globset`).
3. **Headers config interaction** — Stage 6f.
4. **Absolute-URL or scheme-relative rewrite destinations** — verify
   reference behavior (expected: 404 since target cannot be stat'd as a
   filesystem path); document as no-op guard in spec note, no new D-NNN.
5. **`:name(custom-regex)` source patterns** — same exclusion as 6d;
   `path_pattern.rs` does not support it.

## Verification

End-to-end:

```bash
cargo test -p irserve-core
cargo test --test oracle              # exercises tools/probe/run.mjs
                                      # against irserve target
npx -y @fission-ai/openspec@latest validate --all --strict
```

Manual smoke after slice 5:

```bash
mkdir -p _tmp && echo spa-root > _tmp/index.html
cargo run -- --single --listen 3010 _tmp
curl -i http://127.0.0.1:3010/anything/deep    # 200, body "spa-root"
```

Reference parity: every new probe case must show identical
status/body/headers (within `contentLengthMayDiffer` envelope) between
`--target=reference` and `--target=irserve` snapshots.

## Estimated effort

Per `docs/stage6_l1_l2_capabilities.md` calibration: 60-100k tokens per
fresh session, 3 sessions per sub-stage. 6e has chaining + a refactor +
CLI flag + 6 new probe cases — likely on the upper end (1 implementation
session + 1-2 review-round sessions).
