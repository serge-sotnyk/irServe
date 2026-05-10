# Tasks

## 1. Slice 1 — Phase 6 wiring (literal + glob, type override)

- [x] 1.1 `crates/irserve-core/src/redirects.rs` — **NEW.**
  `RedirectRuleCompiled`, `Matcher` enum with two variants
  (`Literal` / `Glob`), `InvalidRedirect`, `compile_rules`,
  `compile_one`, `compute_configured_redirects`, `slasher`,
  `has_glob_meta`, `literal_matches`. Source-pattern routing:
  `Glob` for sources with glob meta or `!`-prefix, `Literal`
  otherwise.
- [x] 1.2 `crates/irserve-core/src/redirects.rs` — `#[cfg(test)] mod
  tests` with 16 unit tests covering: literal match (default 301,
  type 302/307, no-match, trailing-slash flexion, missing-leading-slash
  normalization), glob match (single-`*`, `**`, brace alternation,
  `!`-prefix negation), first-match-wins, empty rules noop,
  invalid-glob silent skip, destination passthrough (literal +
  absolute URL).
- [x] 1.3 `crates/irserve-core/src/lib.rs` — `mod redirects;`
  registration.
- [x] 1.4 `crates/irserve-core/src/dispatch.rs` — signature gains
  `redirect_rules: &[RedirectRuleCompiled]`; phase 6 hooked between
  phase 3 (slash-collapse) and phase 8 (cleanUrls resolve), replacing
  the `Phase 6: configured redirects (Stage 6d)` comment-stub.
  Generalize `redirect_301` into `redirect_with_status(target,
  status)` so the `type` override can emit any 3xx; out-of-range
  u16 falls back to 301. Keep `redirect_301` as a thin wrapper for
  the existing call sites.
- [x] 1.5 `crates/irserve-core/src/server.rs` — `AppState` carries
  `redirect_rules: Vec<RedirectRuleCompiled>` built once in
  `serve()` from `config.serve_config.redirects`; `handler`
  propagates it. Stderr warning per skipped invalid rule, mirroring
  the cleanUrls treatment from 6c.
- [x] 1.6 Probe flips:
  - `tools/probe/cases/redirects-types.json` — new `runner.l0`
    block: `clean: ["explicit_302"]`,
    `divergent: ["default_301_segment"]` (slice 2 promotes it),
    `contentLengthMayDiffer: ["explicit_302"]`.
  - `tools/probe/cases/prec-rewrites-redirects.json` — new
    `runner.l0` block: `clean: ["go_root", "page_html_cleanurl_default"]`,
    `contentLengthMayDiffer: ["go_root", "page_html_cleanurl_default"]`.
- [x] 1.7 Verify: `cargo build -p irserve-core` clean;
  `cargo test -p irserve-core redirects` 16/16 green;
  `cargo test --workspace --lib` 107/107 green (no regressions);
  `cargo test --test oracle` 22 passed, 21 skipped, 0 failed (was
  20/22 at end of 6c);
  `node tools/probe/run.mjs --all --target=reference --snapshot=verify`
  43/43.

## 2. Slice 2 — Path-segment params (`:name`, `*` token)

- [x] 2.1 `Cargo.toml` (workspace) + `crates/irserve-core/Cargo.toml`
  — `regex = "1"` (latest stable 1.12; verified via context7 on
  2026-05-09).
- [x] 2.2 `crates/irserve-core/src/redirects.rs` — extend `Matcher`
  with a `Pattern { regex, dest_template }` variant. Add
  `DestTemplate { fragments: Vec<DestFrag> }` with
  `DestFrag::{Literal, Param}`. New `ENCODE_URI_COMPONENT_SET`
  AsciiSet (NON_ALPHANUMERIC minus `- _ . ! ~ * ' ( )`).
- [x] 2.3 `crates/irserve-core/src/redirects.rs` — `compile_source_regex`
  walks the source byte-by-byte: `:name` → `(?P<name>[^/]+)`,
  `*` → `(.*)`, literal runs → `regex::escape`. Anchored
  `^...\/?$`. `compile_dest_template` walks the destination
  byte-by-byte to emit `Literal`/`Param` fragments; lone `:` becomes
  literal text. `has_path_param` detects `:[A-Za-z0-9_]+` in source.
- [x] 2.4 `crates/irserve-core/src/redirects.rs` — `compile_one`
  classifier: routes `:name`-bearing sources to `Pattern`; rejects
  `!`-prefix + `:name` via the new `CompileError::NegatedParam`
  variant (`CompileError` enum with `Glob | Regex | NegatedParam`,
  thiserror-derived).
- [x] 2.5 `crates/irserve-core/src/redirects.rs` —
  `RedirectRuleCompiled::try_match` returns the rendered destination
  string on match. `Pattern` branch runs `regex.captures(path)` and
  calls `dest_template.render(&caps)`, which substitutes captured
  values via `utf8_percent_encode(value, ENCODE_URI_COMPONENT_SET)`.
  Missing-source `:name` in destination renders as empty (fail-open).
- [x] 2.6 `crates/irserve-core/src/redirects.rs` — 18 new unit
  tests for the Pattern variant: single `:id` substitutes, multi
  `:a/:b`, `*` token alongside `:id`, trailing-slash flexion, `:id`
  param does not cross `/`, `encodeURIComponent` on captured value
  (`a;b` → `a%3Bb`, `a b` → `a%20b`), explicit type with pattern,
  pattern + literal first-match-wins, missing-source `:name` emits
  empty, `!`-prefix + `:name` rejected with `NegatedParam`,
  destination extra text around param, DestTemplate fragmentation
  (pure literal / param-only / mixed / lone-colon-as-literal),
  has_path_param detects/ignores.
- [x] 2.7 Probe flip: `tools/probe/cases/redirects-types.json` —
  promote `default_301_segment` from `divergent` to `clean`; keep
  `contentLengthMayDiffer` for both anchors.
- [x] 2.8 Verify: `cargo test -p irserve-core redirects` 34/34
  green (was 16/16); `cargo test --workspace --lib` 125/125;
  `cargo test --test oracle` 22/22 (no new clean count — same case,
  added anchor); `node tools/probe/run.mjs --all --target=reference
  --snapshot=verify` 43/43.

## 3. Slice 3 — Q-007 closure + compose probe + meta + un-defer

- [x] 3.1 `tools/probe/cases/redirects-destination-forms.json` —
  **NEW.** Fixture with four rules covering Q-007: absolute
  (`https://example.com/x`), scheme-relative (`//example.com/x`),
  relative no-leading-slash (`foo/bar`), absolute path baseline
  (`/foo/bar`). Four anchors via four corresponding requests.
- [x] 3.2 `tools/probe/snapshots/redirects-destination-forms.json`
  — recorded via `--snapshot=update --target=reference`. Surprise
  result: scheme-relative `//example.com/x` collapses to
  `Location: /example.com/x` (path.posix.normalize folds `//` to
  `/`). Absolute URLs pass through verbatim. Relative paths get a
  leading `/`.
- [x] 3.3 `crates/irserve-core/src/redirects.rs` —
  `normalize_destination(dest)` and `has_protocol(dest)` helpers
  added. `compile_one` applies `normalize_destination` to the
  rule's destination before storing it (Literal/Glob) or
  pre-parsing it (Pattern). Mirrors `serve-handler/src/index.js:80`'s
  `protocol ? destination : slasher(destination)` exactly,
  including the surprising consecutive-slash collapse.
- [x] 3.4 `crates/irserve-core/src/redirects.rs` — 8 new unit tests
  covering destination normalization: absolute URL passthrough,
  scheme-relative collapses, relative gets leading slash, absolute
  path passthrough, Pattern template intact, Pattern under https
  protocol, has_protocol recognizes/rejects.
- [x] 3.5 Probe flip: `tools/probe/cases/redirects-destination-forms.json`
  — `runner.l0` block with all four anchors in `clean` and all
  four in `contentLengthMayDiffer`.
- [x] 3.6 `tools/probe/cases/prec-trailing-redirects.json` —
  **NEW.** Fixture with `trailingSlash: true`, `cleanUrls: false`,
  redirect `/face/mask → /elsewhere`. Single anchor
  `trailing_add_wins_over_redirect` exercising the precedence:
  reference's `slashing` branch returns the trailingSlash 301
  before the redirects loop is reached.
- [x] 3.7 `tools/probe/snapshots/prec-trailing-redirects.json` —
  recorded via `--snapshot=update --target=reference`. Confirms
  `Location: /face/mask/`, status 301.
- [x] 3.8 Probe flip: `tools/probe/cases/prec-trailing-redirects.json`
  — `runner.l0` block: `clean: ["trailing_add_wins_over_redirect"]`,
  `contentLengthMayDiffer: ["trailing_add_wins_over_redirect"]`.
- [x] 3.9 `docs/reference/serve/decisions.md` — append D-012:
  un-defer SRV-RDIR-001/002/003; close Q-007; document the Q-007
  scheme-relative collapse surprise; cite ORC-079 through ORC-083;
  note the inherited Q-012 extglob limitation.
- [x] 3.10 `docs/reference/serve/open-questions.md` — Q-007 marked
  closed; cite ORC-079 through ORC-082 and the
  `redirects-destination-forms.json` snapshot.
- [x] 3.11 `docs/reference/serve/inventory.md` — SRV-RDIR-003
  promoted from `accepted` to `verified`; oracle test field updated
  with ORC-079 through ORC-082; "Open questions" cleared.
- [x] 3.12 `docs/reference/serve/oracle-matrix.md` — append rows
  ORC-079 (`absolute_https`), ORC-080 (`scheme_relative`), ORC-081
  (`relative_no_leading_slash`), ORC-082 (`absolute_path_baseline`),
  ORC-083 (`trailing_add_wins_over_redirect`). Fix pre-existing
  ORC-030 typo (`/docs/12` → `/new-docs/12`).
- [x] 3.13 `openspec/changes/006-configured-redirects/{proposal,design,tasks}.md`
  — change package authored.
- [x] 3.14 `openspec/changes/006-configured-redirects/specs/redirects/spec.md`
  — MODIFIED delta extending oracle lists for SRV-RDIR-001/002/003;
  Q-007 note refreshed (closed).
- [x] 3.15 `openspec/changes/006-configured-redirects/specs/routing/spec.md`
  — MODIFIED delta extending SRV-ROUT-006's oracle list with
  ORC-083.
- [x] 3.16 `README.md` — flip 6d row → done; "Try IrServe" gains
  a redirects example with `serve.json`; "What is NOT yet
  observable" drops `(6d)`.
- [x] 3.17 `docs/stage6_l1_l2_capabilities.md` — flip 6d row →
  done; update "Closes / touches" cell ("Closes Q-007").
- [x] 3.18 `npx -y @fission-ai/openspec@latest validate --all
  --strict` — clean.
- [x] 3.19 Verify: `cargo test --workspace` 167/167 green;
  `cargo test --test oracle` 24 passed (was 22), 21 skipped, 0
  failed; `node tools/probe/run.mjs --all --target=reference
  --snapshot=verify` 45/45.

## 4. Codex review round 1 (P1 + P2 fixes)

- [x] 4.1 **P1 — `*` redirect sources route to Pattern.**
  `redirects::compile_one` classifier extended: any source
  containing `*` (without `!`-prefix) routes through `Pattern`
  (regex), not `Glob`. Mirrors the reference's `pathToRegExp`
  first-pass at `serve-handler/src/index.js:46-49` which rewrites
  `*` to `(.*)` before compilation. Globset still handles
  `?`/`[`/`{` and `!`-prefix sources.
- [x] 4.2 New unit tests in `redirects.rs`:
  `star_source_crosses_segments` (replaces the old
  `glob_single_star_matches_one_segment` which locked the wrong
  semantics — flipped to assert cross-slash matching).
- [x] 4.3 New probe `tools/probe/cases/redirects-glob-source.json`
  + snapshot. Two anchors (`star_matches_single_segment`,
  `star_matches_multi_segment`) lock down the cross-segment
  behavior so a regression on the routing classifier shows up in
  the oracle. ORC-084, ORC-085 added to `oracle-matrix.md`.
- [x] 4.4 **P2 — `path.posix.normalize` `.`/`..` resolution.**
  `redirects::normalize_destination` no longer just calls
  `collapse_slashes`; it now calls a local `path_posix_normalize`
  function that mirrors Node's `path.posix.normalize` (consecutive
  slashes + `.`/`..` resolution + `..`-above-root dropping for
  absolute paths). Closes the gap flagged by Codex: a `destination:
  "a/../b"` rule now emits `Location: /b` matching the reference.
- [x] 4.5 New unit tests in `redirects.rs`:
  `destination_normalize_resolves_dotdot`,
  `destination_normalize_drops_dot_segments`,
  `destination_normalize_dotdot_above_root_is_silent`, plus a
  9-test block for the standalone `path_posix_normalize` function.
- [x] 4.6 Probe `tools/probe/cases/redirects-destination-forms.json`
  extended with a fifth anchor `dotdot_resolved` (`/up` →
  destination `a/../b` → reference `Location: /b`). Snapshot
  updated. ORC-086 added to `oracle-matrix.md`.
- [x] 4.7 **P2 — extglob inconsistency in spec text.** The
  modified `redirects` Requirement (delta in
  `006-configured-redirects/specs/redirects/spec.md` AND canonical
  `openspec/specs/redirects/spec.md`) previously said "extglobs
  SHALL be supported" while a later paragraph said extglob is NOT
  supported (Q-012). Removed the contradiction: explicit "extglob
  NOT supported, see Q-012" wording now appears in the
  Requirement's first paragraph. SRV-RDIR-001's compatibility note
  in `inventory.md` updated to match.
- [x] 4.8 SRV-RDIR-003 spec text updated in both delta and
  canonical to reflect full `path.posix.normalize` semantics
  (including `.`/`..` resolution), not just consecutive-slash
  collapse. Implementation note in the canonical spec now points
  at `path_posix_normalize` rather than `collapse_slashes`.
- [x] 4.9 Verify: `cargo test -p irserve-core redirects` 54/54
  green (was 42); `cargo test --workspace` 145/145; `cargo test
  --test oracle` 25 passed (was 24, +1 from `redirects-glob-source`),
  21 skipped, 0 failed; `node tools/probe/run.mjs --all
  --target=reference --snapshot=verify` 46/46;
  `npx -y @fission-ai/openspec@latest validate --all --strict` —
  14/14.

## 5. Codex review round 2 (P1 fixes)

- [x] 5.1 **P1 — multi-`*` over-match fixed.**
  `compile_source_regex` now mirrors JS
  `String.prototype.replace('*', '(.*)')` (first-only): the first
  `*`-run becomes `(.*)`, subsequent `*`-runs are emitted as regex
  literals (`\*`). Consecutive `*`s collapse to a single one
  (so `**` still matches like `*` per the reference's
  `serve-handler/test/integration.test.js:432` precedent —
  `face/**` matches `/face/me`). Closes the over-match where
  `/a/*/b/*` previously matched `/a/x/b/y/z`.
- [x] 5.2 **P1 — `slasher` parity for source AND destination.**
  Both `slasher` (source) and `normalize_destination` (destination)
  now call `slasher_join_normalize`, a helper that mirrors
  `path.posix.normalize(path.posix.join('/', value))`. The
  join-with-`/`-first prepend is critical: a leading `..` or
  empty input first joins to `/..` or `/` BEFORE normalization,
  yielding `/` (`..` above root drops) rather than `/..` or `/.`.
  Closes the source-side gap (`source: "../old"` now matches
  `/old`) and the destination-side gap (`destination: "../b"`
  → `Location: /b`; `destination: ""` → `Location: /`).
- [x] 5.3 **P1 — `!` + `:name` no longer rejected.**
  `CompileError::NegatedParam` variant removed. The classifier
  now routes `!`-prefix sources to `Glob` regardless of `:name`
  presence in the body, mirroring the reference's `sourceMatches`
  fallback (path-to-regexp returns null on `!`-bearing patterns,
  minimatch handles negation, `:name` fragments are treated as
  literal). The rule fires for every path that does NOT literally
  equal the slasher-normalized source.
- [x] 5.4 New unit tests in `redirects.rs`:
  `multi_star_source_only_first_substitutes` (P1.1),
  `literal_source_resolves_leading_dotdot` and
  `literal_source_resolves_dot_segment` (P1.2 source side),
  `destination_normalize_leading_dotdot_resolves` and
  `destination_normalize_empty_becomes_root` (P1.2 destination
  side), `pattern_negation_combo_falls_to_glob` (P1.3, replaces
  the prior `pattern_negation_combo_is_rejected`).
- [x] 5.5 New probes:
  - `tools/probe/cases/redirects-glob-source.json` extended with
    `double_star_matches_single`, `double_star_matches_multi`,
    `multi_star_no_overmatch` anchors. ORC-089, ORC-090, ORC-091.
  - `tools/probe/cases/redirects-destination-forms.json` extended
    with `dotdot_leading_resolved` (`../b` → `/b`) and
    `empty_destination_root` (`""` → `/`) anchors. ORC-087, ORC-088.
  - `tools/probe/cases/redirects-source-slasher.json` (NEW): two
    anchors for source-side `slasher` parity (`../old` matching
    `/old`; `/a/./b` matching `/a/b`). ORC-092, ORC-093.
  - `tools/probe/cases/redirects-negation-source.json` (NEW): one
    anchor for `!`-prefix + `:name` falling through to minimatch.
    ORC-094.
- [x] 5.6 Spec updates:
  - `006-configured-redirects/specs/redirects/spec.md` — removed
    `NegatedParam` paragraph, replaced with the minimatch-fallback
    description; oracle list extended with ORC-084..094.
  - `006-configured-redirects/proposal.md` — corrected
    `normalize_destination` description (uses
    `slasher_join_normalize`, not bare `path_posix_normalize`);
    removed `:name + !-prefix` out-of-scope entry.
  - `006-configured-redirects/design.md` — `CompileError` block
    updated (no more NegatedParam); routing-classifier section
    updated; §4.2 destination-normalization explains the
    `slasher_join_normalize` helper.
- [x] 5.7 `docs/reference/serve/decisions.md` — D-012 updated to
  document the round-2 fixes (multi-`*` + slasher parity +
  `!`+`:name` minimatch fallback).
- [x] 5.8 `docs/reference/serve/oracle-matrix.md` — append rows
  ORC-087 through ORC-094.
- [x] 5.9 `docs/reference/serve/inventory.md` — SRV-RDIR-001
  oracle list extended with new ORC IDs and probe references;
  SRV-RDIR-003 oracle list extended with ORC-087, ORC-088.
- [x] 5.10 Verify: `cargo test -p irserve-core redirects` 59/59
  green (was 54); `cargo test --workspace` 150/150; `cargo test
  --test oracle` 27 passed (was 25, +2 cases from new probes),
  21 skipped, 0 failed; `node tools/probe/run.mjs --all
  --target=reference --snapshot=verify` 48/48 (was 46);
  `npx -y @fission-ai/openspec@latest validate --all --strict` —
  14/14.

## 6. Codex review round 3 (P1 + P2 fixes)

- [x] 6.1 **P1 — `*`-only sources need a minimatch fallback.**
  Round 2's first-only-`*`-replace correctly stopped over-matching
  `/a/*/b/*` against `/a/x/b/y/z`, but it under-matched
  `/a/x/b/y` (which the reference matches via the `minimatch`
  fallback in `sourceMatches` at `index.js:59`). Root cause:
  `path-to-regexp@3.3.0`'s PATH_REGEXP doesn't recognize bare `*`
  as a wildcard token, so the second-and-later `*`s end up as
  regex literals; the reference's two-stage matcher then tries
  minimatch where each `*` is a single-segment wildcard. Fixed by
  adding a `glob_fallback: Option<GlobMatcher>` field to the
  `Pattern` matcher: for `*`-bearing-no-`:name` sources, store a
  `globset::Glob` over the original source pattern. At match time,
  try regex first (so `:name` captures still work for
  `:`+`*` sources); on miss, try the glob (with a trailing-slash
  trim mirroring `path.posix.resolve(requestPath)` so globset's
  zero-or-more `*` lines up with minimatch's one-or-more).
- [x] 6.2 New unit test `multi_star_source_matches_via_glob_fallback`
  replaces the round-2 `multi_star_source_only_first_substitutes`
  test: now asserts the positive single-trailing case (`/a/x/b/y`
  matches) plus four negative cases.
- [x] 6.3 Probe `tools/probe/cases/redirects-glob-source.json`
  extended with `multi_star_matches_single_trailing` (positive
  case missed previously), and the round-2
  `multi_star_no_overmatch` anchor renamed to
  `multi_star_no_overmatch_two_trailing` plus two new negative
  anchors `multi_star_no_overmatch_zero_trailing` and
  `multi_star_no_overmatch_extra_middle`. Snapshot regenerated.
  ORC-091 reference updated to the renamed anchor; new ORC-095,
  ORC-096, ORC-097 added.
- [x] 6.4 **P2 — stale doc references scrubbed.**
  - `docs/reference/serve/decisions.md` D-012 — removed the stale
    paragraph claiming `!`+`:name` is rejected via
    `CompileError::NegatedParam`; updated multi-`*` description
    to mention the round-3 glob fallback alongside the round-2
    first-only substitution.
  - `openspec/changes/006-configured-redirects/design.md` — §7.2
    updated from "Q-007 closure (4 anchors)" to "(7 anchors after
    Codex rounds 1+2)" plus enumeration of the round-1/2/3
    additional probes; §8 stop-the-line item rewritten to
    document the round-2 reversal of `NegatedParam` rejection
    plus a new item 3 documenting the round-3 glob-fallback
    discovery.
  - `openspec/changes/006-configured-redirects/proposal.md` —
    `CompileError` enum description corrected (no more
    `NegatedParam`).
  - `docs/reference/serve/open-questions.md` Q-007 — anchor count
    updated from 4 to 7 with the full enumeration; ORC list
    extended to include ORC-086, ORC-087, ORC-088.
  - `openspec/specs/redirects/spec.md` (canonical) and the
    `006-configured-redirects` delta — SRV-RDIR-003 oracle list
    extended with ORC-087/ORC-088; SRV-RDIR-001 oracle list
    extended through ORC-097.
  - `docs/reference/serve/inventory.md` — SRV-RDIR-001 oracle
    list and probe enumeration synced with the round-3 anchor
    set.
- [x] 6.5 Verify: `cargo test -p irserve-core redirects` 59/59
  green (no test count change; one round-2 test rewritten);
  `cargo test --test oracle` 27 passed (no case-count change —
  the `redirects-glob-source` probe gained 3 anchors but stayed
  one case), 21 skipped, 0 failed; `node tools/probe/run.mjs
  --all --target=reference --snapshot=verify` 48/48; `npx -y
  @fission-ai/openspec@latest validate --all --strict` — 14/14.

## 7. Codex review round 4 (P1 + P2 fixes)

- [x] 7.1 **P1 — leading-dot rejection in glob fallback.**
  globset's `*` matches dotfiles, while minimatch's default
  (`dot: false`) does not. Round 3's `glob_fallback: Option<GlobMatcher>`
  field was missing this constraint, so requests like
  `/a/.x/b/y` against `/a/*/b/*` returned 301 in irserve while
  the reference returns 404. Fixed by wrapping the matcher in a
  new `GlobFallback` struct that carries `pattern_segments` and
  `has_doublestar` metadata; the `matches_strict` method runs
  globset first, then walks pattern segments alongside path
  segments and rejects when any `*`-bearing pattern segment
  aligns with a leading-`.` path segment.
- [x] 7.2 **P2 — glob fallback now builds for `:name`-bearing
  sources too.** Round 3 gated the fallback on `!has_path_param`
  on the assumption that minimatch would never fire for `:name`
  sources. Codex showed this was wrong: requests CAN literally
  carry `:name` segments (URL paths permit `:` unencoded), in
  which case the reference's minimatch falls back and emits a
  301. The gate was removed; the fallback now builds for any
  `*`-bearing source.
- [x] 7.3 New unit tests in `redirects.rs`:
  `multi_star_source_rejects_dot_segments` (P1.1: `/a/*/b/*`
  against `/a/.x/b/y` and `/a/x/b/.y` both miss; positive
  control kept) and `pattern_with_param_and_star_falls_back_to_glob`
  (P2: `/a/:id/*/b/*` against literal `/a/:id/x/b/y` matches via
  fallback; realistic `/a/foo/x/b/y` still misses).
- [x] 7.4 New probes:
  - `tools/probe/cases/redirects-glob-source.json` extended
    with `multi_star_rejects_leading_dot_first` and
    `multi_star_rejects_leading_dot_second` anchors. ORC-098,
    ORC-099 added to oracle-matrix.
  - `tools/probe/cases/redirects-pattern-with-multistar-fallback.json`
    (NEW): one anchor `literal_colon_id_matches_via_minimatch`.
    ORC-100 added.
- [x] 7.5 D-012 in `decisions.md` extended with the round-4
  refinements; `design.md` §8 stop-the-line item 3 updated to
  document both refinements; `inventory.md` SRV-RDIR-001 oracle
  list extended through ORC-100; the `006-configured-redirects`
  delta `specs/redirects/spec.md` SRV-RDIR-001 oracle list also
  extended.
- [x] 7.6 Verify: `cargo test -p irserve-core redirects` 61/61
  green (was 59); `cargo test --test oracle` 28 passed (was 27,
  +1 case from `redirects-pattern-with-multistar-fallback`),
  21 skipped, 0 failed; `node tools/probe/run.mjs --all
  --target=reference --snapshot=verify` 49/49 (was 48); `npx -y
  @fission-ai/openspec@latest validate --all --strict` — 14/14.

## 8. Codex review round 5 (P1 generalized dot-rejection)

- [x] 8.1 **P1 — generalized minimatch `dot: false`.** Round 4's
  `matches_strict` rejected dot path segments only when the
  pattern segment contained `*`, AND skipped all validation when
  any pattern segment was exactly `**` (the `has_doublestar`
  short-circuit). Both heuristics diverged from minimatch:
  - A pattern segment beginning with literal `.` (e.g. `.*`,
    `.foo*`) DOES admit a leading-dot path segment, so the round-4
    rule was too strict for `dot/.*/b/*` patterns.
  - A pattern segment whose magic char is `?`/`[`/`{` instead of
    `*` (e.g. `[.]y`) ALSO rejects leading-dot paths, so the
    round-4 rule was too loose by gating on `*`-presence.
  - `**` (globstar) follows the same rule per minimatch's default
    — it cannot expand to a sequence containing a leading-dot
    segment. The round-4 escape hatch over-matched.
- [x] 8.2 Replaced the bare `Option<GlobMatcher>` glob_fallback
  with a per-segment `Vec<PatSeg>` plus a recursive
  `match_segments` walker. `PatSeg` variants:
  - `Literal(String)` — no glob meta.
  - `Wildcard { matcher: GlobMatcher, starts_with_dot: bool }` —
    has at least one of `*`/`?`/`[`/`{`. `starts_with_dot` is
    `seg.starts_with('.')`, the literal-leading-dot flag.
  - `DoubleStar` — segment is exactly `**`.
  `match_segments` is a textbook minimatch-style recursive
  descent: `Literal` requires equality; `Wildcard` checks the
  dot rule then runs globset over the single segment;
  `DoubleStar` tries successive skip counts, aborting once a
  consumed segment would begin with `.`.
- [x] 8.3 New unit tests in `redirects.rs`:
  `dot_pattern_segment_admits_leading_dot_path`,
  `bracket_pattern_segment_does_not_admit_leading_dot`,
  `doublestar_segment_obeys_dot_rule`. The round-4
  `multi_star_source_rejects_dot_segments` test continues to
  pass under the new matcher.
- [x] 8.4 `tools/probe/cases/redirects-glob-source.json`
  extended with three new anchors covering all three round-5
  corners (`dot_pattern_segment_admits_leading_dot`,
  `bracket_pattern_segment_rejects_leading_dot`,
  `doublestar_rejects_dot_in_expansion`). Snapshot regenerated.
  ORC-101, ORC-102, ORC-103 added to oracle-matrix.
- [x] 8.5 D-012 in `decisions.md` extended; `design.md` §8
  stop-the-line item 4 added; `inventory.md` SRV-RDIR-001 oracle
  list extended through ORC-103; the
  `006-configured-redirects` delta `specs/redirects/spec.md`
  SRV-RDIR-001 oracle list also extended.
- [x] 8.6 Verify: `cargo test -p irserve-core redirects` 64/64
  green (was 61); `cargo test --test oracle` 28 passed (no
  case-count change — `redirects-glob-source` gained 3 anchors
  but stayed one case), 21 skipped, 0 failed; `node
  tools/probe/run.mjs --all --target=reference --snapshot=verify`
  49/49; `npx -y @fission-ai/openspec@latest validate --all
  --strict` — 14/14.

## 9. Codex review round 6 (P1 + P1 + P2 fixes)

- [x] 9.1 **P1 — `Matcher::Glob` now uses the segment matcher.**
  The round-5 segment-based `match_segments` was applied only
  inside `Pattern`'s `glob_fallback`. `Matcher::Glob` (sources
  with `?`/`[`/`{` glob meta and no `*`/`:name`, OR any
  `!`-prefixed source) still ran raw `globset.is_match()` and
  missed minimatch's `dot: false`. Refactored
  `Matcher::Glob` to store `Vec<PatSeg>` (the same shape as
  `GlobFallback.segments`) and call `match_segments`. The
  `negate` XOR continues to apply on top.
- [x] 9.2 **P1 — brace expansion before the dot rule.**
  `classify_pattern_segment` set `starts_with_dot` from the
  raw segment string, so `{.x,y}` (segment beginning with `{`)
  was treated as not-dot-starting. Minimatch expands braces
  BEFORE applying the dot rule, so the `.x` alternative is a
  literal-leading-dot expansion that admits dot-paths. Added
  `segment_can_start_with_dot` helper that walks brace
  alternatives recursively (including nested braces via
  `split_top_level_alternatives`), flipping the flag when any
  alternative begins with literal `.`.
- [x] 9.3 **P2 — `glob_fallback` for ALL Pattern matchers.**
  The round-3..5 implementation gated `glob_fallback` on
  `body.contains('*')`. Codex showed that `:name`+`?` and
  `:name`+`{}` sources also need the fallback (the
  reference's `sourceMatches` ALWAYS tries minimatch on
  path-to-regexp miss, per `index.js:47-66`). Removed the
  gate; `glob_fallback` now builds for every Pattern matcher.
- [x] 9.4 New unit tests in `redirects.rs`:
  `glob_bracket_segment_rejects_dot_via_segment_matcher`,
  `glob_negation_with_bracket_pattern_flips_correctly`,
  `brace_alternative_starts_with_dot_admits_leading_dot_path`,
  `brace_no_dot_alternative_rejects_dot_path`,
  `pattern_with_param_and_question_mark_falls_back_to_glob`.
- [x] 9.5 New probes:
  - `tools/probe/cases/redirects-glob-edges.json` (4 anchors:
    Glob bracket dot-reject, brace-dot-alt admits, brace-no-dot
    rejects, `:id`+`?` minimatch fallback). ORC-104..107.
  - `tools/probe/cases/redirects-negation-bracket.json` (1
    anchor: negation flips bracket dot rejection). ORC-108.
    Isolated to its own probe because `!`-rules match almost
    any path and would interfere with positive anchors.
- [x] 9.6 D-012 in `decisions.md` extended with all three
  round-6 fixes; `design.md` §8 stop-the-line item 5 added;
  `inventory.md` SRV-RDIR-001 oracle list extended through
  ORC-108; `006-configured-redirects` delta
  `specs/redirects/spec.md` SRV-RDIR-001 oracle list also
  extended.
- [x] 9.7 Verify: `cargo test -p irserve-core redirects` 69/69
  green (was 64); `cargo test --test oracle` 30 passed (was
  28, +2 cases from new probes), 21 skipped, 0 failed; `node
  tools/probe/run.mjs --all --target=reference --snapshot=verify`
  51/51 (was 49); `npx -y @fission-ai/openspec@latest validate
  --all --strict` — 14/14.

## 10. Codex review round 7 (P1 × 3 fixes)

- [x] 10.1 **P1.1 — `path.posix.resolve(requestPath)` trim
  before regex.** The reference applies
  `path.posix.resolve(requestPath)` BEFORE both `pathToRegExp.exec`
  (`index.js:49`) and `minimatch` (`index.js:59`). The round-6
  impl trimmed only in Glob and glob_fallback branches; the
  Pattern regex saw the raw path. Repro: `/a/*` against `/a/`
  matched in irserve while reference returned 404 (after trim,
  the regex needs `/(.*)` which `/a` can't satisfy). Fix: trim
  once at the top of `try_match`.
- [x] 10.2 **P1.2 — `**` after `:name` admits zero segments.**
  path-to-regexp v3 parses `(.*)*` (the JS-replaced `**`) as a
  custom regex segment plus an `*` modifier, making the whole
  segment optional+repeat. So `/a/:id/**` matches `/a/foo` (zero
  trailing segments) with `id=foo`. The round-5 compiler
  collapsed `**` into the same first-only `(.*)` substitution,
  requiring the slash segment, and the glob fallback couldn't
  rescue realistic paths because it treated `:id` literally.
  Fix: refactor `compile_source_regex` to be segment-aware. Walk
  segments (split on `/`); `**` segments emit `(?:/(.*))?`
  (optional multi-segment); other segments walk char-by-char as
  before. The first-only `*`-replace flag is shared across
  segments.
- [x] 10.3 **P1.3 — DoubleStar at END of pattern requires ≥1
  segment in segment matcher.** The reference's minimatch
  fallback (`index.js:59`) is stricter than pathToRegExp's
  `(.*)*` here: `/a/**` does NOT match `/a` via minimatch, even
  though it does via pathToRegExp. The asymmetry shows up via
  negation: `!/a/**` falls to minimatch (because `!`-bearing
  pattern fails pathToRegExp), and the inner minimatch
  semantics determine the negation result. Empirical probe
  confirms: `!/a/**` against `/a` → 301, against `/a/x` → 404,
  against `/b` → 301. `**` in MIDDLE of pattern still allows
  zero-skip per the empirical probe of `!/a/**/b` (matches
  `/a/b` via inner minimatch). Fix: in `match_segments`'s
  DoubleStar branch, when `**` is the last pattern element
  (`pat.len() == 1`), require `min_skip = 1` and reject empty
  paths.
- [x] 10.4 New unit tests in `redirects.rs` covering all three
  corners: `star_source_rejects_trailing_slash_only_path`,
  `param_plus_star_rejects_trailing_slash_only_path`,
  `doublestar_after_param_admits_zero_segments`,
  `doublestar_in_middle_admits_zero_segments_with_literal_after`,
  `negated_doublestar_at_end_requires_at_least_one_segment`,
  `doublestar_in_middle_glob_path_admits_zero`. Six new tests
  bringing the redirects suite to 75.
- [x] 10.5 New probes:
  - `tools/probe/cases/redirects-resolve-and-doublestar.json`
    (5 anchors, ORC-109..113) covering the resolve trim and
    `**`-after-param positive cases.
  - `tools/probe/cases/redirects-negation-doublestar.json`
    (4 anchors, ORC-114..117) covering negation+end-`**` strict
    behavior. Isolated probe because `!`-rules match almost any
    path and would interfere with positive anchors.
- [x] 10.6 D-012 in `decisions.md` extended with all three
  round-7 fixes; `design.md` §8 stop-the-line item 6 added;
  `inventory.md` SRV-RDIR-001 oracle list extended through
  ORC-117 (using `ORC-084..ORC-117` shorthand for the long
  range); `006-configured-redirects` delta
  `specs/redirects/spec.md` SRV-RDIR-001 oracle list
  enumerated through ORC-117.
- [x] 10.7 Verify: `cargo test -p irserve-core redirects` 75/75
  green (was 69); `cargo test --test oracle` 32 passed (was
  30, +2 cases from new probes), 21 skipped, 0 failed; `node
  tools/probe/run.mjs --all --target=reference --snapshot=verify`
  53/53 (was 51); `npx -y @fission-ai/openspec@latest validate
  --all --strict` — 14/14.

## 11. Codex review round 8 (P1 + P2 fixes)

- [x] 11.1 **P1 — full `path.posix.resolve`.** Round 7's
  trailing-slash trim missed `.` / `..` segments. Reference
  applies `path.posix.resolve` (`index.js:41`) which ALSO
  resolves dot/dotdot and collapses consecutive slashes. Raw
  requests like `/a/./x` and `/a/b/../x` resolve to `/a/x` in
  reference and match a literal `/a/x` rule; irserve was
  returning 404. Fix: replace the simple trim at the top of
  `try_match` with a full `path_posix_resolve` helper
  (`path_posix_normalize` followed by trailing-slash trim).
- [x] 11.2 **P2 — backslash escape in source patterns.**
  minimatch and path-to-regexp treat `\X` as a literal `X`.
  irserve had three places that needed updates:
  - `Matcher::Literal` compared raw bytes, so source `\.x`
    didn't match path `.x`. Fix: `de_escape` the body at
    compile time.
  - `Matcher::Glob` and `Pattern.glob_fallback` use globset
    via `classify_pattern_segment`. globset's GlobBuilder
    interprets `\` as literal backslash by default; enabling
    `backslash_escape(true)` makes it treat `\X` as escape.
    Fix: chain `.backslash_escape(true)` onto the
    GlobBuilder.
  - `segment_can_start_with_dot` checked `seg.starts_with('.')`
    only, missing the `\.` case. The dot rule's effective-
    first-char treatment in minimatch looks past escape chars.
    Fix: add a `seg.starts_with("\\.")` check.
- [x] 11.3 New unit tests in `redirects.rs`:
  `raw_dot_segment_in_path_is_resolved`,
  `raw_dotdot_segment_in_path_is_resolved`,
  `raw_dot_segment_with_param`,
  `literal_source_de_escapes_backslash_dot`,
  `wildcard_with_escaped_dot_admits_leading_dot_path`. Five
  new tests bringing the redirects suite to 80.
- [x] 11.4 New probe `tools/probe/cases/redirects-resolve-and-escape.json`
  (5 anchors): three raw-mode anchors for `path.posix.resolve`
  parity (`.` segment, `..` segment, `.` + `:name`); two
  fetch-mode anchors for `\` escape parity (Literal `\.x`,
  Wildcard `\.*` admits dotfile). Raw-mode anchors mark
  `bodyMayDiffer` because reference's raw-mode redirect
  responses include the HTTP/1.1 chunked encoding terminator
  (5-byte `0\r\n\r\n`) while irserve's hyper layer uses
  Content-Length: 0 — same divergence as `multislash-collapse`
  per round-3 D-011 amendments. ORC-118..122 added.
- [x] 11.5 D-012 in `decisions.md` extended with both round-8
  fixes; `design.md` §8 stop-the-line item 7 added;
  `inventory.md` SRV-RDIR-001 oracle list extended through
  ORC-122 (using `ORC-084..ORC-122` shorthand);
  `006-configured-redirects` delta `specs/redirects/spec.md`
  SRV-RDIR-001 oracle list collapsed to the same shorthand
  (40 anchors total — `ORC-030` plus the contiguous 39-anchor
  range `ORC-084..ORC-122`. Codex round 9 P3 corrected the
  earlier "39 anchors total" miscount).
- [x] 11.6 Verify: `cargo test -p irserve-core redirects` 80/80
  green (was 75); `cargo test --test oracle` 33 passed (was
  32, +1 case from new probe), 21 skipped, 0 failed; `node
  tools/probe/run.mjs --all --target=reference --snapshot=verify`
  54/54 (was 53); `npx -y @fission-ai/openspec@latest validate
  --all --strict` — 14/14.

## 12. Codex review round 9 (P1 + P3 fixes)

- [x] 12.1 **P1 — backslash transparency for glob meta.** Round
  8's `GlobBuilder::backslash_escape(true)` was over-broad.
  Empirical probe of `minimatch@3.1.5` (the version pinned by
  serve-handler — see `_minimatch_test.js`-style direct
  invocation) shows `\X` is transparent for X in `*`/`?`/`[`/`{`:
  the backslash is stripped but the meta-character keeps its
  glob meaning. So `/s/\*` matches `/s/foo` (the `*` is still
  wildcard). Round 8's flag made globset interpret `\*` as
  literal `*`, under-matching real paths. Round 9 fix: remove
  the `backslash_escape(true)` flag and `de_escape` each
  segment before passing to globset / Literal classification.
  This unifies handling: `\X` → `X` for all X, the dot rule
  works naturally on the de-escaped first-char.
- [x] 12.2 The `\.`-prefix branch in `segment_can_start_with_dot`
  is now redundant (de-escaped `.X` hits the existing
  `.`-prefix branch). Removed for clarity.
- [x] 12.3 New unit tests in `redirects.rs`:
  `wildcard_with_escaped_star_keeps_glob_meta`,
  `wildcard_with_escaped_question_keeps_glob_meta`,
  `wildcard_with_escaped_bracket_keeps_glob_meta`,
  `wildcard_with_escaped_brace_keeps_glob_meta`. Existing round-8
  tests (`literal_source_de_escapes_backslash_dot`,
  `wildcard_with_escaped_dot_admits_leading_dot_path`) continue
  to pass under the new uniform de-escape.
- [x] 12.4 New probe `tools/probe/cases/redirects-backslash-meta.json`
  (5 anchors): `\*` matches via wildcard glob, `\?` matches
  single char, `\[ab]` matches bracket class members `a` and
  `b`, `\{a,b}` expands alternation. ORC-123..127 added to
  `oracle-matrix.md`.
- [x] 12.5 **P3 — anchor count fix.** The round-8 wording in
  the spec delta and tasks.md said "39 anchors total" for the
  range `ORC-030, ORC-084..ORC-122`. The range `084..122` IS
  39 anchors, plus `ORC-030` makes it 40. Updated to "40
  anchors total" in both files.
- [x] 12.6 D-012 in `decisions.md` extended with the round-9
  correction; `design.md` §8 stop-the-line item 8 added;
  `inventory.md` SRV-RDIR-001 oracle list extended through
  ORC-127; `006-configured-redirects` delta
  `specs/redirects/spec.md` updated to reflect the 40-anchor
  total over `ORC-084..ORC-127`.
- [x] 12.7 Verify: `cargo test -p irserve-core redirects` 84/84
  green (was 80); `cargo test --test oracle` 34 passed (was
  33, +1 case from new probe), 21 skipped, 0 failed; `node
  tools/probe/run.mjs --all --target=reference --snapshot=verify`
  55/55 (was 54); `npx -y @fission-ai/openspec@latest validate
  --all --strict` — 14/14.

## 13. Codex review round 10 (P1 + P1 + P2 + P3 fixes)

- [x] 13.1 **P1.1 — escaped globstar.** The `seg == "**"` check
  in `classify_pattern_segment` ran BEFORE round-9's de-escape,
  so `\**` (which de-escapes to `**`) was classified as a
  single-segment Wildcard instead of DoubleStar. Empirically
  `minimatch('/a/x/y', '/a/\\**')` → true (globstar). Fix: move
  de-escape before the `==` check. The closely-related `\*\*`
  case is documented as a known divergence (minimatch parses it
  as two single-segment globs because escape positions of
  consecutive stars matter; mirroring requires a fuller
  minimatch parser).
- [x] 13.2 **P1.2 — per-alt dot rule for braces.** Round-6's
  single `starts_with_dot: bool` over-permitted brace patterns:
  `{.x,*}` admitted any dotfile via the `*` alt because
  globset's `*` matches dotfiles regardless. Empirically
  minimatch's `dot: false` is per-alternative — only the
  dot-prefixed alt admits dotfile paths. Fix: replace
  `starts_with_dot` with `dot_matcher: Option<GlobMatcher>` —
  `Some` when at least one alt begins with `.`, containing a
  matcher built from ONLY those dot-prefixed alts. At match
  time, dotfile paths route to `dot_matcher` (None → reject);
  non-dot paths use the full matcher. New helper
  `collect_dot_starting_alternatives` walks brace alts
  recursively. Removed the now-redundant
  `segment_can_start_with_dot` helper.
- [x] 13.3 **P2 — globset-error fallback to Literal.** The
  round-9 unified de-escape produces invalid globset patterns
  for sources like `\[` (de-escaped `[` is unmatched bracket).
  The rule was silently dropped. Reference matches the literal
  `[` request path. Fix: in `classify_pattern_segment`, on
  globset compile error, fall back to a Literal segment with
  the de-escaped form. Updates the
  `compile_rules_skips_invalid_glob` test to
  `compile_rules_falls_back_to_literal_on_globset_error`.
- [x] 13.4 New unit tests in `redirects.rs`:
  `escaped_doublestar_classifies_as_globstar` (P1.1),
  `brace_with_dot_alt_rejects_other_dotfile` (P1.2),
  `unmatched_bracket_source_falls_back_to_literal` (P2).
  Plus the `compile_rules_falls_back_to_literal_on_globset_error`
  rewrite. Three new tests; one updated; redirects suite at 87.
- [x] 13.5 New probe `tools/probe/cases/redirects-edge-corners.json`
  (5 anchors, ORC-128..132): `\**` globstar, brace per-alt dot
  rule (3 anchors), `\[` literal fallback.
- [x] 13.6 **P3 — anchor count fix.** The round-9 wording in
  the spec delta said "40 anchors total" for the range
  `ORC-030, ORC-084..ORC-127`. The range `084..127` IS 44
  anchors; plus `ORC-030` is 45. Round 10 extends through
  ORC-132 — 49 + 1 = 50 anchors total. Updated wording in the
  spec delta and `tasks.md`.
- [x] 13.7 D-012 in `decisions.md` extended with all three
  round-10 fixes; `design.md` §8 stop-the-line item 9 added;
  `inventory.md` SRV-RDIR-001 oracle list extended through
  ORC-132; `006-configured-redirects` delta
  `specs/redirects/spec.md` updated to "50 anchors" over the
  `ORC-084..ORC-132` range.
- [x] 13.8 Verify: `cargo test -p irserve-core redirects` 87/87
  green (was 84); `cargo test --test oracle` 35 passed (was
  34, +1 case from new probe), 21 skipped, 0 failed; `node
  tools/probe/run.mjs --all --target=reference --snapshot=verify`
  56/56 (was 55); `npx -y @fission-ai/openspec@latest validate
  --all --strict` — 14/14.

## 14. Codex review round 11 (P1 + P3 fixes)

- [x] 14.1 **P1 — backslash handling vs requests with literal
  `\` (decoded from `%5C`).** The round-8 `de_escape` silently
  dropped a trailing unescaped `\`, and the per-segment
  `de_escape` in `classify_pattern_segment` did the same for
  the Pattern matcher's glob_fallback. Three concrete
  divergences against requests with literal backslashes
  (decoded from `%5C`):
  - source `/u/foo\` matched `/u/foo` (over-match) but failed
    to match `/u/foo\` (under-match);
  - source `/u/\f` matched `/u/f` only, missing minimatch's
    segment-level transparency that empirically matches
    `/u/\f` too (`minimatch('/u/\\f', '/u/\\f') === true`);
  - source `/v/*\` over-matched `/v/x` via the glob_fallback
    (segment `*\` de-escaped to bare `*`).

  Fix: the Literal variant now stores TWO match forms —
  `source_ptr` (de-escape with trailing `\` PRESERVED,
  mirroring path-to-regexp's `(\\.)`-then-literal-accumulator
  behavior in v3.3.0) and `source_mm: Option<String>` (the raw
  body, populated when the body has `\` AND no trailing
  unescaped `\`, mirroring minimatch's segment-level
  transparency that accepts request paths literally containing
  `\X`). For the Pattern matcher's glob_fallback: a new helper
  `ends_with_unescaped_backslash` gates the fallback off when
  the source body ends with an odd number of trailing
  backslashes; minimatch's compiled regex requires a synthetic
  trailing `/` suffix that `path.posix.resolve` always strips,
  so the fallback can never match a resolved path anyway. The
  Pattern's primary regex was already correct (it preserves
  the trailing `\` as a literal `\` in its compiled regex), so
  requests with literal trailing `\` still match.
- [x] 14.2 New helper `de_escape_keep_trailing` alongside the
  existing `de_escape`. The latter is retained for per-segment
  classifiers in Glob and the now-gated glob_fallback (where
  the trailing-`\` case is excluded by the new gating).
- [x] 14.3 Four new unit tests in `redirects.rs`:
  - `literal_source_with_trailing_backslash_requires_literal_backslash`
    — pins the over-match-`/u/foo` and under-match-`/u/foo\`
    asymmetry that round 11 fixes.
  - `literal_source_with_inner_escape_matches_both_forms` —
    pins the dual-form acceptance for `/u/\f`.
  - `star_source_with_trailing_backslash_disables_glob_fallback`
    — pins the gating of glob_fallback for `/v/*\`.
  - `ends_with_unescaped_backslash_helper` — sanity-checks
    the helper that drives the gating decision.

  Redirects suite goes 87 → 91.
- [x] 14.4 New probe
  `tools/probe/cases/redirects-backslash-paths.json` (6
  anchors, ORC-133..138). Three rules (literal trailing `\`,
  inner `\f`, glob `*\`) × six request shapes covering the
  four divergences plus their positive-side controls. Reference
  snapshot captured with `--target=reference --snapshot=update`;
  irserve verified clean against the reference snapshot.
- [x] 14.5 **P3 — stale comments about invalid-glob being
  dropped.** Round 10's globset-error fallback to Literal
  means sources like `/u/\[` are now recovered, not dropped —
  but the warning text in `server.rs::serve` still said
  "invalid glob", `redirects.rs`'s `InvalidRedirect` doc
  comment still said "silently dropped" without noting the
  recovery, and `proposal.md`'s "Compile-time error handling"
  bullet said the same. Updated all three call-sites to
  reflect that only `regex::Error` cases (the rare path-
  pattern compilation failure) propagate to `InvalidRedirect`,
  and the `Glob` variant remains in the `CompileError` enum
  for forward compatibility but is unreachable from the
  current segment-level classifier.
- [x] 14.6 D-012 in `decisions.md` extended with the round-11
  fix narrative; oracle-matrix.md ORC-133..138 added;
  inventory.md SRV-RDIR-001 oracle list extended through
  ORC-138; `006-configured-redirects` delta
  `specs/redirects/spec.md` updated to "56 anchors" over the
  `ORC-084..ORC-138` range.
- [x] 14.7 Verify: `cargo test -p irserve-core redirects`
  91/91 green (was 87); `cargo test --test oracle` 36 passed
  (was 35, +1 case from new probe), 21 skipped, 0 failed;
  `node tools/probe/run.mjs --all --target=reference
  --snapshot=verify` 57/57 (was 56); `npx -y
  @fission-ai/openspec@latest validate --all --strict` —
  14/14.

## 15. Codex review round 12 (P1 fix; P2 pushed back as known divergence)

- [x] 15.1 **P1 — case-insensitive matching on the
  path-to-regexp branch.** Empirical:
  `pathToRegExp('/Case').flags === 'i'` — path-to-regexp v3.3.0
  ships its compiled regex with the `i` flag by default, so
  reference matches `/Case → /literal-case` against request
  `/case`, `/P/:id → /param/:id` against `/p/Foo`, and
  `/S/* → /star` against `/s/x`. The previous IrServe was
  case-sensitive throughout. Fix: `literal_matches` takes a
  new `case_sensitive: bool` parameter; the Literal arm of
  `try_match` calls it with `false` for `source_ptr` (path-to-
  regexp branch) and `true` for `source_mm` (minimatch branch).
  `compile_source_regex` builds the Pattern matcher's regex
  via `regex::RegexBuilder::case_insensitive(true)`. The Glob
  matcher and the Pattern matcher's `glob_fallback` remain
  case-sensitive — minimatch's default is `nocase: false`, so
  both routes mirror minimatch.
- [x] 15.2 Three new unit tests in `redirects.rs`:
  - `literal_source_matches_case_insensitively_via_ptr_branch`
    — `/Case` matches `/case`, `/Case`, plus a multi-segment
    `/MyPath/Sub` against various casings.
  - `pattern_source_matches_case_insensitively` — `/P/:id`
    matches `/p/Foo` and the captured `Foo` is interpolated
    verbatim (case-insensitive matching does not normalize
    the captured value); `/S/*` matches `/s/x`.
  - `glob_source_remains_case_sensitive` — control case:
    `/G/?` against `/g/a` is 404 (path-to-regexp parses `?`
    as literal so doesn't match; minimatch is case-sensitive).
    Same-case `/G/a` matches via minimatch single-char glob.

  Redirects suite goes 91 → 94.
- [x] 15.3 New probe
  `tools/probe/cases/redirects-case-insensitivity.json` (5
  anchors, ORC-139..143). Four positive cases (Literal
  lowercase, Literal same-case, Pattern `:name`, Pattern `*`)
  plus one negative control (Glob-only `/G/?` against `/g/a`).
  Reference snapshot captured with
  `--target=reference --snapshot=update`; irserve verified
  clean against the reference snapshot.
- [x] 15.4 **P2 — segment-internal trailing `\` (pushed back
  as platform-specific known divergence).** Codex's
  reproduction `source: "/g/?\\/bar"` against
  `/g/a%5C/bar` shows reference matches via minimatch but
  IrServe doesn't. Empirical investigation (verified by
  inspecting `Minimatch.matchOne` directly returning false
  while `m.match()` returns true) traced the divergence to
  `minimatch.js:742-745`:
  `if (path.sep !== '/') { f = f.split(path.sep).join('/') }`.
  This is filesystem-aware behavior — minimatch on Windows
  treats `\` as a path separator and converts it to `/`
  BEFORE segment matching. On Linux the same source/request
  is 404 in reference too. Mirroring would require
  `cfg!(target_os = "windows")` conditional code that
  produces platform-divergent test behavior (same source/
  request 404s on Linux but 301s on Windows) — an
  anti-pattern for the IrServe codebase. The case is also
  narrow (non-final glob segment with trailing `\`).
  Documented as a known divergence in D-012 alongside `\*\*`
  (per-alt parsing) and `{a\,b,c}` (brace-with-escaped-
  comma). If a future user reports this, the fix would be a
  Windows-only `\` → `/` normalization in `match_segments`,
  scoped and clearly labeled.
- [x] 15.5 D-012 in `decisions.md` extended with the
  round-12 P1 fix narrative AND the round-12 P2 push-back
  reasoning; oracle-matrix.md ORC-139..143 added;
  inventory.md SRV-RDIR-001 oracle list extended through
  ORC-143; `006-configured-redirects` delta
  `specs/redirects/spec.md` updated to "61 anchors" over the
  `ORC-084..ORC-143` range.
- [x] 15.6 Verify: `cargo test -p irserve-core redirects`
  94/94 green (was 91); `cargo test --test oracle` 37 passed
  (was 36, +1 case from new probe), 21 skipped, 0 failed;
  `node tools/probe/run.mjs --all --target=reference
  --snapshot=verify` 58/58 (was 57); `npx -y
  @fission-ai/openspec@latest validate --all --strict` —
  14/14.

## 16. Codex review round 13 (P1 fix; partial — special folds documented as known divergence)

- [x] 16.1 **P1 — Latin-1 vs Unicode special-fold asymmetry.**
  Round 12 used `eq_ignore_ascii_case` for `Literal.source_ptr`
  (ASCII-only) and `RegexBuilder::case_insensitive(true)` for
  `Pattern.regex` (Rust's Unicode-default folding). Two
  empirically-verified divergences from path-to-regexp v3.3.0's
  default `i` flag (a JS regex `i` without `u`):
  - **Latin-1 under-match (Literal):** `/Ä/i.test('ä') === true`
    in JS, but `eq_ignore_ascii_case('/Ä', '/ä')` is false in
    Rust. Source `/Ä` missed request `/ä`.
  - **Special-fold over-match (Pattern):** Kelvin sign
    `K` (U+212A) folds to ASCII `k` in Rust's Unicode default
    but NOT in JS without `u` flag (`new RegExp('K', 'i').test('k')
    === false`). Source `/K/:id` (Kelvin K) wrongly matched
    `/k/Foo` in IrServe.

  Fix scope (per project rule 9 — "3+ consecutive review rounds
  on the same subsystem → declare compat level"): fix the
  Latin-1 case (the practical real-world case for German/French/
  Spanish URLs); document special-fold over-match as a known
  divergence. Bug-for-bug parity would require shipping a
  custom JS-specific case-folding table for limited real-world
  value.
- [x] 16.2 Implementation: switch Literal's case-insensitive
  comparison from `eq_ignore_ascii_case` to Unicode-aware
  `to_lowercase()`. This unifies Literal and Pattern semantics
  on the path-to-regexp branch — both now use Rust's Unicode
  default folding. The `compile_source_regex` path is unchanged
  (already Unicode-aware via `RegexBuilder`); doc-comment
  updated to reflect the residual divergence.
- [x] 16.3 Two new unit tests in `redirects.rs`:
  - `literal_source_matches_latin1_case_insensitively` —
    `/Ä` matches `/ä`, `/É/path` matches `/é/path`. Pins the
    Round-13 fix.
  - `pattern_source_matches_latin1_case_insensitively` —
    `/Ö/:id` matches `/ö/Foo` and the captured value `Foo` is
    preserved verbatim (case-folding does not normalize
    captures). Control: confirms Pattern handles Latin-1 the
    same way Literal does.

  Redirects suite goes 94 → 96.
- [x] 16.4 New probe
  `tools/probe/cases/redirects-latin1-case.json` (4 anchors,
  ORC-144..147). Three rules (Literal `/Ä`, Literal mid-path
  `/É/path`, Pattern `/Ö/:id`) × four request shapes covering
  Latin-1 case folding. Reference snapshot captured with
  `--target=reference --snapshot=update`; irserve verified
  clean.
- [x] 16.5 D-012 in `decisions.md` extended with the round-13
  P1 narrative and the explicit residual divergence on
  Unicode special folds (Kelvin sign, ﬃ ligature, etc.);
  oracle-matrix.md ORC-144..147 added; inventory.md
  SRV-RDIR-001 oracle list extended through ORC-147;
  `006-configured-redirects` delta `specs/redirects/spec.md`
  updated to "65 anchors" over the `ORC-084..ORC-147` range.
- [x] 16.6 Verify: `cargo test -p irserve-core redirects`
  96/96 green (was 94); `cargo test --test oracle` 38 passed
  (was 37, +1 case from new probe), 21 skipped, 0 failed;
  `node tools/probe/run.mjs --all --target=reference
  --snapshot=verify` 59/59 (was 58); `npx -y
  @fission-ai/openspec@latest validate --all --strict` —
  14/14.
