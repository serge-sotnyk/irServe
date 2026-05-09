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
