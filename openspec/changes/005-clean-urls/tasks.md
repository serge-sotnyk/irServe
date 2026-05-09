# Tasks

## 1. Slice 1 — Phase 4 cleanUrls 301

- [x] 1.1 `Cargo.toml` (workspace) + `crates/irserve-core/Cargo.toml`
  — `globset = "0.4"` (latest stable on crates.io; verified via
  context7 on 2026-05-09)
- [x] 1.2 `crates/irserve-core/src/clean_urls.rs` — **NEW.**
  `CleanUrlsView` enum (Off / On / Scoped(GlobSet)), `from_config`,
  `applicable`, `compute_clean_urls_redirect`, `slasher`,
  `strip_html_or_index_suffix`, `collapse_consecutive_slashes`,
  `ensure_slash_start`
- [x] 1.3 `crates/irserve-core/src/clean_urls.rs` — `#[cfg(test)] mod
  tests` with 12 phase-4 unit tests covering scope (bool / glob in /
  glob out / leading-slash normalization), redirect edges
  (`.html`/`/index`/`/index.html` strip, single-pass not double, `//`
  collapse, trailing-slash blocks match, no-match returns None,
  off short-circuits, scope-guarded redirect)
- [x] 1.4 `crates/irserve-core/src/lib.rs` — `mod clean_urls;` +
  `Error::CleanUrlsGlob(#[from] globset::Error)` variant
- [x] 1.5 `crates/irserve-core/src/server.rs` — `AppState` carries
  `clean_urls_view: CleanUrlsView` built once in `serve()` from
  `config.serve_config.clean_urls`; `handler` propagates it
- [x] 1.6 `crates/irserve-core/src/dispatch.rs` — signature gains
  `&CleanUrlsView`; phase 4 hooked between URL-decode and phase 5
  (replaces the `Phase 4: cleanUrls 301 (Stage 6c)` comment-stub)
- [x] 1.7 Probe flips:
  - `tools/probe/cases/_smoke.json` — `index_html_redirect` from
    `divergent` to `clean`; `contentLengthMayDiffer: ["index_html_redirect"]`
  - `tools/probe/cases/mime-defaults.json` — `html` from
    `divergent` to `clean`; `contentLengthMayDiffer: ["html"]`
  - `tools/probe/cases/prec-cleanurls-default.json` — new
    `runner.l0` block: `clean: ["about_html"]`,
    `contentLengthMayDiffer: ["about_html"]`
  - `tools/probe/cases/cleanurls-array.json` — new `runner.l0`
    block: `clean: ["in_scope_redirect", "out_of_scope_html_direct"]`,
    `contentLengthMayDiffer: ["in_scope_redirect"]`
- [x] 1.8 Verify: `cargo build -p irserve-core` clean;
  `cargo test -p irserve-core` 75/75 green; `cargo test --test
  oracle` (16 passed, 24 skipped, 0 failed; was 14/40 at end of 6b);
  `node tools/probe/run.mjs --all --target=reference --snapshot=verify`
  40/40

## 2. Slice 2 — Phase 8 cleanUrls extensionless resolution

- [x] 2.1 `crates/irserve-core/src/clean_urls.rs` — `pub async fn
  try_clean_urls_resolve(url_path, root, view) -> Option<ResolveOutcome>`
  + private `stat_under_root` helper
- [x] 2.2 `crates/irserve-core/src/clean_urls.rs` — 8 phase-8 unit
  tests (`tempfile` fixtures): index-first hit, `<P>.html` fallback,
  trailing-slash normalizes to same candidates, root-only-tries-index,
  both-miss, off short-circuit, out-of-scope, in-scope glob
- [x] 2.3 `crates/irserve-core/src/resolve.rs` — `#[derive(Debug)]`
  on `ResolveOutcome` so test panics in `clean_urls.rs` can format
  the variant in error messages
- [x] 2.4 `crates/irserve-core/src/dispatch.rs` — pre-stat-gated
  phase 8 hook: extensionless paths run phase 8 first, has-ext paths
  run `resolve()` first then phase 8 on NotFound. Adds
  `url_path_has_extension(path: &str) -> bool` helper at module
  scope mirroring Node's `path.extname`.
- [x] 2.5 Probe flips:
  - `tools/probe/cases/prec-cleanurls-default.json` — extend
    `runner.l0.clean` to include `about_no_slash` and
    `about_with_slash`
  - `tools/probe/cases/cleanurls-array.json` — extend
    `runner.l0.clean` to include `in_scope_extensionless` and
    `out_of_scope_extensionless_miss`; add `bodyMayDiffer:
    ["out_of_scope_extensionless_miss"]` (D-003 default-error-page
    bytes differ)
- [x] 2.6 Verify: `cargo test -p irserve-core` 83/83 green;
  `cargo test --test oracle` (16 passed, 24 skipped, 0 failed —
  anchor coverage within existing probes expanded; no probe count
  change because both flipped probes already passed at 6c slice 1);
  reference 40/40

## 3. Slice 3 — Compose probes flip + meta + un-defer

- [x] 3.1 `tools/probe/run.mjs::applyL0Filter` — `bodyMayDiffer` now
  also strips `body.kind` (transport-encoding choice; comment block
  updated)
- [x] 3.2 Compose probe flips:
  - `tools/probe/cases/prec-cleanurls-trailing.json` — new
    `runner.l0` block: `clean: ["about_no_slash", "about_html",
    "about_with_slash"]`; `contentLengthMayDiffer: ["about_no_slash",
    "about_html"]`
  - `tools/probe/cases/prec-cleanurls-trailing-false.json` — new
    `runner.l0` block: `clean: ["about_no_slash", "about_html",
    "about_with_slash"]`; `contentLengthMayDiffer: ["about_html",
    "about_with_slash"]`
  - `tools/probe/cases/multislash-collapse.json` — extend
    `runner.l0.clean` to include `double_slash_segment` and
    `internal_double_slash`; remove them from `divergent`; add
    `bodyMayDiffer` for both (chunked-encoding tail)
- [x] 3.3 Meta:
  - `docs/reference/serve/decisions.md` — append D-011 (un-defers
    SRV-ROUT-001/002 from D-008's list; partial un-defer of
    SRV-ROUT-006 cleanUrls↔trailingSlash compose)
  - `docs/reference/serve/open-questions.md` — Q-005 already closed
    in 6b; no edit needed in 6c
- [x] 3.4 OpenSpec change package
  `openspec/changes/005-clean-urls/{proposal,design,tasks}.md` +
  `specs/routing/spec.md` MODIFIED delta (Implementation paragraphs
  on SRV-ROUT-001 and SRV-ROUT-002; behavioral Scenarios preserved
  verbatim; no new ORC IDs)
- [x] 3.5 README — flip 6c row → done; "Try IrServe" section
  refreshed with cleanUrls examples; "What is NOT yet observable"
  list shrunk
- [x] 3.6 `docs/stage6_l1_l2_capabilities.md` — flip 6c row → done
- [x] 3.7 `docs/features/0009_PLAN_stage6c_clean_urls.md` — already
  authored at planning time per the kickoff template; committed in
  this slice
- [x] 3.8 Verify: `cargo test --workspace` green; `cargo test
  --test oracle` (18 passed, 22 skipped, 0 failed); reference
  40/40; `npx -y @fission-ai/openspec@latest validate --all
  --strict` clean

## Hard stops (per template)

- `third_party/` is read-only.
- Existing snapshots are touched only if reference behavior actually
  changed — that's a methodological signal; discuss before patching.
- The `routing/spec.md` contract changes only through MODIFIED
  delta or D-NNN entry after explicit discussion. Q-NNN entries
  close on probe measurement only, not on code.
- No `git commit` / `git push` without explicit per-slice approval
  (granted blanket per-slice for this stage by user on 2026-05-09).
