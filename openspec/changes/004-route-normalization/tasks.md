# Tasks

## 1. Slice 1 — Phase 3 multi-slash collapse + plumbing

- [x] 1.1 `crates/irserve-core/src/normalize.rs` — `pub fn
  collapse_slashes(path: &str) -> Cow<'_, str>` returning
  `Cow::Borrowed` on the no-`//` happy path
- [x] 1.2 `crates/irserve-core/src/normalize.rs` — `#[cfg(test)] mod
  tests` with 10 unit tests covering empty / single slash / leading /
  internal / trailing / mixed runs / percent-encoded slashes (left
  intact at this stage)
- [x] 1.3 `crates/irserve-core/src/lib.rs` — `mod normalize;`
- [x] 1.4 `crates/irserve-core/src/dispatch.rs` — change `dispatch()`
  signature to take `&ServeConfig` (named `_serve_config` until
  slice 2). Hook phase 3 between method gate and resolve. Add
  comment-stubs for phases 4, 5, 6, 7, 8.
- [x] 1.5 `crates/irserve-core/src/server.rs` — `AppState { root,
  serve_config }` struct; update `handler` to pass both into
  `dispatch`
- [x] 1.6 `tools/probe/cases/multislash-collapse.json` — add
  `runner.l0` block: `clean: ["double_slash_root"]`, `divergent:
  ["double_slash_segment", "internal_double_slash"]`
- [x] 1.7 Verify: `cargo build` clean; `cargo test -p irserve-core`
  30/30 green; `cargo test --test oracle` green (12 passed, 26
  skipped, 0 failed); `node tools/probe/run.mjs multislash-collapse
  --target=reference --snapshot=verify` green

## 2. Slice 2 — Phase 5 trailingSlash 301

- [x] 2.1 `crates/irserve-core/src/trailing_slash.rs` — `pub fn
  compute_trailing_slash_redirect(path: &str, cfg: Option<bool>) ->
  Option<String>` mirroring the `shouldRedirect` add/strip branches
- [x] 2.2 `crates/irserve-core/src/trailing_slash.rs` — `#[cfg(test)]
  mod tests` with 14 unit tests covering add / strip / no-op /
  dotfile / extension / dotfile-with-extension / root-edge / nested
- [x] 2.3 `crates/irserve-core/src/lib.rs` — `mod trailing_slash;`
- [x] 2.4 `crates/irserve-core/src/dispatch.rs` — promote
  `_serve_config` to `serve_config`; hook phase 5 between phase-4
  stub and resolve; add `redirect_301(target)` private helper
- [x] 2.5 `tools/probe/cases/trailingslash-add.json` —
  `serve.json: {trailingSlash: true, cleanUrls: false}`, fixture
  `index.html` + `data.txt`. Two anchors:
  `about_no_slash_redirects` (GET /about → 301 /about/),
  `txt_with_extension_no_redirect` (GET /data.txt → 200).
  `runner.l0.clean` lists both; `contentLengthMayDiffer` lists the
  redirect anchor.
- [x] 2.6 `tools/probe/cases/trailingslash-strip.json` —
  `serve.json: {trailingSlash: false, cleanUrls: false}`, fixture
  `index.html` + `about.html`. Two anchors:
  `about_with_slash_redirects` (GET /about/ → 301 /about),
  `about_no_slash_no_redirect` (GET /about.html → 200).
  `runner.l0.clean` lists both; `contentLengthMayDiffer` lists the
  redirect anchor.
- [x] 2.7 Snapshots — record via `node tools/probe/run.mjs <id>
  --target=reference --snapshot=update` for both new cases.
- [x] 2.8 Verify: `cargo test -p irserve-core` 44/44 green;
  `cargo test --test oracle` (40 cases total, 14 passed, 26 skipped,
  0 failed); `node tools/probe/run.mjs --all --target=reference
  --snapshot=verify` (40/40)

## 3. Slice 3 — Change package, research-track edits, README, validate

- [x] 3.1 `openspec/changes/004-route-normalization/proposal.md`
- [x] 3.2 `openspec/changes/004-route-normalization/design.md`
- [x] 3.3 `openspec/changes/004-route-normalization/tasks.md` (this
  document)
- [x] 3.4 `docs/reference/serve/inventory.md` — extend SRV-ROUT-003
  oracle list with ORC-068, ORC-069 and SRV-ROUT-004 with ORC-070,
  ORC-071; add the new probe paths
- [x] 3.5 `docs/reference/serve/oracle-matrix.md` — four new rows in
  the L2 routing section: ORC-068 (`trailingslash-add#about_no_slash_redirects`),
  ORC-069 (`trailingslash-add#txt_with_extension_no_redirect`),
  ORC-070 (`trailingslash-strip#about_with_slash_redirects`),
  ORC-071 (`trailingslash-strip#about_no_slash_no_redirect`)
- [x] 3.6 `docs/reference/serve/decisions.md` — append D-010
  amending D-008's deferred-SRV list (drops SRV-ROUT-003,
  SRV-ROUT-004, SRV-ROUT-005)
- [x] 3.7 `README.md` — Status block: add `Stage 6b — routing
  normalization (trailingSlash, multi-slash). Done.`; Stage map row
  6b `todo` → `done`; "Try IrServe" section refresh covering the new
  observable surface
- [x] 3.8 Validate: `npx -y @fission-ai/openspec@latest validate
  --all --strict` exit 0 (12/12 — round 1 re-verified after fixes)
- [x] 3.9 Validate: `cargo test --test oracle` green (40 cases, 14
  passed / 26 skipped / 0 failed — round 1 re-verified after fixes)
- [x] 3.10 Validate: `cargo build --release` clean
- [x] 3.11 Validate: `git diff --stat` confirms zero `third_party/`
  edits and no edits to existing snapshots

## 4. Spec delta

- [x] 4.1 `openspec/changes/004-route-normalization/specs/routing/
  spec.md` — three MODIFIED requirements:
  - SRV-ROUT-003: extend `Evidence:` oracle list (`ORC-015, ORC-016`
    → `ORC-015, ORC-016, ORC-068, ORC-069`); add `Implementation:`
    paragraph.
  - SRV-ROUT-004: extend `Evidence:` (`ORC-019` → `ORC-019, ORC-070,
    ORC-071`); same `Implementation:` shape.
  - SRV-ROUT-005: extend `Evidence:` (`ORC-025/026/027` →
    `ORC-025/026/027, ORC-072, ORC-073, ORC-074`); requirement text
    grows by one sentence describing the multi-slash override
    coupling with phase 5; add `Implementation:` paragraph; two new
    Scenarios (double-slash + encoded double-slash).
  Behavioral Scenarios and the requirement texts for SRV-ROUT-003
  and SRV-ROUT-004 are preserved verbatim.

## 5. Round 1 fixes (Codex review)

- [x] 5.1 `Cargo.toml` workspace + `crates/irserve-core/Cargo.toml`
  — add `percent-encoding = "2"` dep
- [x] 5.2 `crates/irserve-core/src/dispatch.rs` — call
  `percent_decode_str(req.uri().path()).decode_utf8_lossy()` at
  dispatcher entry; pass decoded (uncollapsed) path to phase 5;
  collapse for phases 9–13 only (P1 + P2 fix)
- [x] 5.3 `crates/irserve-core/src/trailing_slash.rs` — add
  multi-slash override at the top of
  `compute_trailing_slash_redirect`; 5 new unit tests covering the
  override under add / strip / unset / internal `//` / root `//`
  (P1 fix)
- [x] 5.4 `tools/probe/cases/trailingslash-add.json` — add anchors
  `trailing_double_slash_collapses_via_redirect` (`/about//` → 301
  `/about/`) and `encoded_double_slash_collapses_via_redirect`
  (`/about%2F%2F` → 301 `/about/`); extend `runner.l0.clean` and
  `contentLengthMayDiffer`; ORC-072 / ORC-073
- [x] 5.5 `tools/probe/cases/trailingslash-strip.json` — add anchor
  `trailing_double_slash_collapses_via_redirect` (`/about//` → 301
  `/about/`, NOT `/about`); ORC-074
- [x] 5.6 Snapshots — re-record via `--snapshot=update
  --target=reference` for both probes
- [x] 5.7 `docs/reference/serve/oracle-matrix.md` — three new rows
  for ORC-072 / ORC-073 / ORC-074
- [x] 5.8 `docs/reference/serve/inventory.md` — extend SRV-ROUT-005
  oracle list with the new ORC IDs and probe paths
- [x] 5.9 `openspec/changes/004-route-normalization/{proposal,design}.md`
  — document the override coupling and URL-decode invariant
- [x] 5.10 `openspec/changes/004-route-normalization/specs/routing/spec.md`
  — extend SRV-ROUT-005 MODIFIED delta with the override sentence,
  new Evidence IDs, Implementation paragraph, two new Scenarios
- [x] 5.11 Tasks 3.7–3.11 flipped to `[x]` (P3 fix)
- [x] 5.12 Verify: `cargo test -p irserve-core` 49/49 (was 44/44 +
  5 new override tests); `cargo test --test oracle` (40 cases, 14
  passed / 26 skipped / 0 failed); `node tools/probe/run.mjs --all
  --target=reference --snapshot=verify` (40/40); `npx -y
  @fission-ai/openspec@latest validate --all --strict` (12/12)
