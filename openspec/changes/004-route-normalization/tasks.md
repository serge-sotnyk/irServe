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

## 6. Round 2 fixes (Codex review)

- [x] 6.1 `crates/irserve-core/src/dispatch.rs` — add
  `ENCODE_URI_SET: &AsciiSet` (CONTROLS + SPACE, `"`, `%`, `<`,
  `>`, `\`, `^`, `` ` ``, `{`, `|`, `}`, `[`, `]`) and
  `pub(crate) fn encode_uri_target(target: &str) -> String` built
  on `percent_encoding::utf8_percent_encode`; call it from
  `redirect_301` before constructing the `HeaderValue` (P2-1 fix,
  mirrors `serve-handler/src/index.js:586` `encodeURI`)
- [x] 6.2 `crates/irserve-core/src/dispatch.rs` — `#[cfg(test)]
  mod tests` with 7 unit tests for `encode_uri_target`: safe-set
  passthrough, query/reserved chars (`?`, `=`, `&`, `:`, `@`, `+`,
  `$`, `,`, `#`), SPACE → `%20`, multi-byte UTF-8 (`café`,
  Cyrillic), literal `%` → `%25`, brackets/quotes, control chars
- [x] 6.3 `tools/probe/cases/trailingslash-add.json` — add anchors
  `space_in_path_reencoded_in_location` (`/foo%20bar` → 301
  `/foo%20bar/`, ORC-075) and `non_ascii_in_path_reencoded_in_location`
  (`/caf%C3%A9` → 301 `/caf%C3%A9/`, ORC-076); extend
  `runner.l0.clean` and `contentLengthMayDiffer`
- [x] 6.4 Snapshots — re-record via `--snapshot=update
  --target=reference` for `trailingslash-add`
- [x] 6.5 `docs/reference/serve/oracle-matrix.md` — two new rows
  for ORC-075 / ORC-076
- [x] 6.6 `docs/reference/serve/inventory.md` — extend SRV-ROUT-003
  oracle list with ORC-072, ORC-073, ORC-075, ORC-076 (catches up
  to round 1's additions plus the new encoding ORCs)
- [x] 6.7 `docs/reference/serve/decisions.md` — amend D-010 in
  place (P2-2 fix): rewrite Reason and Impact to reflect the
  as-implemented wiring (decode at entry; phase 5 on uncollapsed
  decoded path; multi-slash override; `encodeURI` re-encoding for
  `Location`); update affected ORC list
- [x] 6.8 `openspec/changes/004-route-normalization/{proposal,
  design}.md` — document the `Location` encoding step alongside
  the existing decode and override descriptions
- [x] 6.9 `openspec/changes/004-route-normalization/specs/routing/
  spec.md` — extend SRV-ROUT-003 MODIFIED requirement: add
  `Location` re-encoding sentence to the requirement text, append
  ORC-075 / ORC-076 to `Evidence:`, extend `Implementation:` to
  cover `encode_uri_target`, add two new Scenarios (SPACE re-
  encoded; non-ASCII re-encoded)
- [x] 6.10 Verify: `cargo test -p irserve-core` 56/56 (was 49/49 +
  7 new encode tests); `cargo test --test oracle` (40 cases, 14
  passed / 26 skipped / 0 failed); `node tools/probe/run.mjs --all
  --target=reference --snapshot=verify` (40/40); `npx -y
  @fission-ai/openspec@latest validate --all --strict` (12/12);
  `cargo build --release` clean

## 7. Round 3 fixes (Codex review)

- [x] 7.1 `docs/reference/serve/decisions.md` — append a deferral
  paragraph to D-010 (P2 fix): malformed percent-escape rejection
  (e.g. `/bad%zz` → 400) is SRV-SEC-001 territory and stays
  reference-only via the existing no-`runner.l0` block on
  `traversal-raw-encoded` (ORC-041). Document the observable
  consequence under `trailingSlash: true`: irserve passes the
  literal `%zz` through `decode_utf8_lossy`, phase 5 appends `/`,
  `encode_uri_target` re-encodes `%` to `%25`, and the response
  is `301 Location: /bad%25zz/`; the reference returns 400. The
  divergence is bounded — strict decoding belongs to 6f.
- [x] 7.2 `openspec/changes/004-route-normalization/proposal.md`
  — replace the stale "Location set verbatim" sentence (P3 fix)
  with the round-2 truth: `redirect_301` runs the target through
  `encode_uri_target` before the `HeaderValue`. Also flesh out
  the dispatcher description to reflect the round-1 wiring
  (decode at entry; phase 5 on uncollapsed; phase 3 silent for
  resolve flow).
- [x] 7.3 No code changes; no probe changes; no spec-delta
  changes. The strict-decode promotion lands in 6f together with
  enabling `runner.l0` on `traversal-encoded` /
  `traversal-raw-encoded`.
- [x] 7.4 Verify: `cargo test -p irserve-core` 56/56 (no new
  tests; no behavior change); `cargo test --test oracle` (40
  cases, 14 passed / 26 skipped / 0 failed — `traversal-raw-encoded`
  still auto-skips under `target=irserve` per the deferral);
  `node tools/probe/run.mjs --all --target=reference
  --snapshot=verify` (40/40); `npx -y
  @fission-ai/openspec@latest validate --all --strict` (12/12);
  `cargo build --release` clean
