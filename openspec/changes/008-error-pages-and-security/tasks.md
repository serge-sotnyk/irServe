# Tasks: Custom error pages, full L2 security, custom response headers

Six iterative slices, one commit per green slice. Slice 6 is the meta
slice and lands last so spec deltas reflect what was actually shipped.

## Slice 1 — `error.rs` + custom `<status>.html`

- [x] Create `crates/irserve-core/src/error.rs` with
  `error_response(status, headers, root)`. JSON-keyed envelope map
  (`not_found`, `bad_request`, fallback). HTML branch: `tokio::fs::read`
  on `<root>/<status>.html`; on miss, generic
  `<h1>STATUS REASON</h1>\n` body.
- [x] Drop `crates/irserve-core/src/notfound.rs` (contents moved).
- [x] Update `lib.rs`: `mod error;` replaces `mod notfound;`.
- [x] Update `dispatch.rs`: both 404 emission sites at L160 + L162
  switch to `error_response(StatusCode::NOT_FOUND, ..)`.
- [x] Add `runner.l0.clean: ["missing_with_custom_404"]` to
  `tools/probe/cases/error-page-custom.json`.
- [x] Add `runner.l0.{clean, contentLengthMayDiffer}` to
  `tools/probe/cases/notfound-custom.json` (JSON branch needs
  contentLengthMayDiffer — reference omits Content-Length on chunked
  JSON 404; axum emits it).
- [x] Verify: ORC-007, ORC-059 green under `target=irserve`.
- Commit: `feat(stage-6f): custom <status>.html error pages (SRV-FILE-003)`.

## Slice 2 — Strict %xx decode

- [x] Add `try_percent_decode` and `is_ascii_hex` helpers in
  `dispatch.rs` (private fns at module bottom).
- [x] Replace the `percent_decode_str(raw_path).decode_utf8_lossy()`
  call site with the new validator; `Err` short-circuits to
  `error_response(StatusCode::BAD_REQUEST, ..)`.
- [x] Add `runner.l0` to `tools/probe/cases/traversal-raw-encoded.json`
  with `clean: ["raw_malformed_percent"]` and
  `bodyMayDiffer: ["raw_malformed_percent"]` (irserve emits a
  generic 400 body; reference's full `errorTemplate`).
- [x] Verify: `raw_malformed_percent` (ORC-041) green under
  `target=irserve`.
- Commit: `feat(stage-6f): strict %xx decode (SRV-SEC-002 partial; malformed → 400)`.

## Slice 3 — Full SRV-SEC-001 surface (lexical containment)

- [x] Add `lexical_path_escapes_root` helper to `dispatch.rs`. Walks
  decoded path's segments; tracks depth; on `..` below zero, return
  true.
- [x] Insert the check after the strict-decode and before any phase
  4+. On true, short-circuit to `error_response(BAD_REQUEST, ..)`.
- [x] Split the `match outcome` arms in `dispatch.rs` so
  `ResolveOutcome::EscapedRoot` routes to 400 (defense-in-depth for
  symlink/canonicalize escapes); `NotFound` stays 404.
- [x] Expand `traversal-raw-encoded.json` `runner.l0` to all 4 anchors
  (`clean` + `bodyMayDiffer`).
- [x] Add `runner.l0` to `traversal-encoded.json` with all 4 anchors
  (`clean`; first 3 also `bodyMayDiffer`; the 4th — `encoded_in_subpath`
  — has byte-exact body since fetch-mode normalization yields a
  successful 200 on `/secret.txt`).
- [x] Verify: ORC-038/039/040/041 (raw) and ORC-034/035/036/037 (fetch)
  green under `target=irserve`.
- Commit: `feat(stage-6f): full SRV-SEC-001 surface (lexical containment → 400)`.

## Slice 4 — Custom headers (SRV-HDR-001)

- [x] Create `crates/irserve-core/src/custom_headers.rs` with
  `HeaderRuleCompiled`, `InvalidHeaderRule`, `compile_rules`, and
  `apply_custom_headers`.
- [x] Add `mod custom_headers;` to `lib.rs`.
- [x] In `server.rs`: compile header rules at startup via
  `compile_header_rules` (alias); add `header_rules` field to
  `AppState`; thread `&[HeaderRuleCompiled]` into `dispatch`.
- [x] Refactor `dispatch.rs`: rename `dispatch` → `dispatch_inner`;
  add a thin `dispatch` wrapper that computes
  `path_for_headers` once and applies `apply_custom_headers` after
  `dispatch_inner` returns.
- [x] `apply_custom_headers` skips 3xx redirects
  (`response.status().is_redirection()`).
- [x] Add `runner.l0` to `tools/probe/cases/headers-applied.json`
  (`clean: ["css_get"]`).
- [x] Add `runner.l0` to `tools/probe/cases/headers-custom.json`
  (`clean: ["html_get"]` + `contentLengthMayDiffer` since axum emits
  `Content-Length: 0` on empty 301).
- [x] Author `tools/probe/cases/headers-on-error.json` with
  `not_found_carries_x_test` anchor (`bodyMayDiffer`). Record reference
  snapshot first.
- [x] Author `tools/probe/cases/headers-accumulate.json` with
  `css_carries_both` anchor (clean). Record reference snapshot first.
- [x] Verify: ORC-053, ORC-031 (headers-custom), ORC-159
  (headers-on-error), ORC-160 (headers-accumulate) green under
  `target=irserve`.
- Commit: `feat(stage-6f): custom response headers — accumulate + override (SRV-HDR-001)`.

## Slice 5 — `value: null` null-prune (SRV-HDR-002)

- [x] Widen `HeaderItem::value` from `String` to `Option<String>` in
  `crates/irserve-core/src/config.rs`. Existing fixtures with string
  values continue to parse via serde.
- [x] In `apply_custom_headers`: split first-pass insert/replace from
  second-pass null-prune. The first pass skips `value: None` entries
  (deferred to pass 2); the second pass walks matched rules again and
  removes any header whose key matches a `value: None` entry.
- [x] Add 8 unit tests in `custom_headers::tests` covering: empty
  rules pass through, single rule inserts, multiple rules accumulate,
  case-insensitive override, redirects skip application, null-prune
  deletes prior header, null-prune doesn't delete when rule doesn't
  match, null-prune is case-insensitive on key.
- [x] Verify: 8/8 unit tests pass; full oracle suite remains green.
- Commit: `feat(stage-6f): SRV-HDR-002 — \`value: null\` deletes a prior header`.

## Slice 6 — Spec deltas + decisions log + meta (this slice)

- [x] Author `openspec/changes/008-error-pages-and-security/`:
  - [x] `proposal.md`
  - [x] `design.md`
  - [x] `tasks.md` (this file)
  - [ ] `specs/static-files/spec.md` — MODIFIED Requirement: SRV-FILE-003
    oracle list extended for irserve coverage (no requirement-text
    change).
  - [ ] `specs/security/spec.md` — MODIFIED Requirements: un-defer
    ORC-038/039/040/041 (SRV-SEC-001) and ORC-034/035/036/037
    (SRV-SEC-002) into the irserve-clean partition.
  - [ ] `specs/headers/spec.md` — ADDED capability with two
    Requirements: SRV-HDR-001 (glob source + accumulate +
    case-insensitive override; 3xx-skip), SRV-HDR-002 (`value: null`
    deletes after accumulate; reference-CLI unreachable per @zeit
    schema).
- [ ] `docs/reference/serve/decisions.md`: append `D-015` (un-defer
  6f SRVs; methodology signals on traversal-raw snapshot resync,
  3xx-skip, 400-no-headers, schema-rejects-null).
- [ ] `docs/reference/serve/inventory.md`: refresh oracle/L0 status
  notes for SRV-FILE-003, SRV-SEC-001, SRV-SEC-002, SRV-HDR-001,
  SRV-HDR-002.
- [ ] `docs/reference/serve/oracle-matrix.md`: append ORC-159
  (headers-on-error) and ORC-160 (headers-accumulate); update
  ORC-007/031/034-041/053/059 status notes.
- [ ] `README.md`: flip Stage 6f row to `done`; update "What is NOT
  yet observable" + "Try IrServe" with custom-error-page +
  custom-headers + path-traversal-400 examples.
- [ ] `docs/stage6_l1_l2_capabilities.md`: flip 6f row to `done`.
- [ ] Run `npx -y @fission-ai/openspec@latest validate --all --strict`.
- Commit: `docs(stage-6f): spec deltas + decisions log + meta`.
