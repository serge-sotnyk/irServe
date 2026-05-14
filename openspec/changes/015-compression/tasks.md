# Tasks: HTTP compression (`-u` / `--no-compression`)

Five iterative slices, one commit per green slice — slice 0
is the probe-first reverse-engineering against reference,
slice 1 collapsed to a no-op audit, slice 2 is the Rust
implementation, slice 3 collapsed because the cascade audit
was absorbed by slice 0's 21 legacy re-records, and slice 4
is the meta slice (this change package + main agent).

## Slice 0 — Probe-first reverse-engineering against reference (Q-002 closure)

- [x] Extend `tools/probe/run.mjs::TRACKED_RESPONSE_HEADERS`
  to include `'content-encoding'`. The header was previously
  silently stripped by the runner's tracked-set filter,
  hiding the reference's on-the-wire encoding on every
  compressible-MIME / above-threshold anchor in legacy
  probes (`etag-roundtrip`, `last-modified-roundtrip`,
  `range-request`, …).
- [x] Re-record the 21 legacy reference snapshots that
  legitimately carry `Content-Encoding` once the tracker
  was added. Verified via `node tools/probe/run.mjs --all
  --target=reference --snapshot=verify`.
- [x] Author `tools/probe/cases/compression-raw.json` with
  20 raw-socket anchors:
  - **MIME allowlist coverage** (compressible above
    threshold): `big_html_gzip_deflate_br`,
    `big_css_gzip_deflate_br`, `big_js_gzip_deflate_br`,
    `data_json_gzip_deflate_br`,
    `vector_svg_gzip_deflate_br`, `wasm_gzip_deflate_br`.
  - **Below-threshold compressible**: `tiny_html_gzip`
    (Vary present, no `Content-Encoding`).
  - **Per-encoder corner**: `big_html_gzip`,
    `big_html_deflate` (force specific encoder via the
    Accept-Encoding header).
  - **Negotiation corners**: `big_html_identity_only`,
    `big_html_no_accept_encoding`, `big_html_gzip_q0`
    (`q=0` excludes gzip → br wins), `big_html_star_q0`
    (no compression).
  - **Non-compressible MIMEs**: `image_png_gzip`,
    `font_woff2_gzip`, `media_mp4_gzip` (no Vary, no
    `Content-Encoding`).
  - **Skip-condition anchors**: `head_big_html_gzip`
    (HEAD), `options_big_html_gzip` (OPTIONS-as-GET
    composition with 7d),
    `no_transform_big_html_gzip` (user `headers` rule
    setting `Cache-Control: no-transform`),
    `range_big_html_gzip` (Range pre-empts compression).
- [x] Capture snapshot:
  `node tools/probe/run.mjs compression-raw
  --target=reference --snapshot=update`.
- [x] Mask `content-encoding` in
  `L0_EXTRA_VOLATILE_HEADERS` as a temporary slice-0
  measure (lifted in slice 2 once irserve emits
  matching wire surface).
- [x] Close **Q-002** in
  `docs/reference/serve/open-questions.md` with the
  empirical resolution: threshold = 1024 bytes;
  negotiation order = brotli > gzip > deflate; MIME set
  = curated allowlist + regex fallback; skip conditions
  = HEAD / no-transform / below-threshold / identity-only;
  OPTIONS routes through GET pipeline; Range pre-empts
  compression.
- Commit: `probe(stage-7e): pin reference compression surface (Q-002 closure)`
  (`9bd52f0`).

## Slice 1 — Probe-runner extension audit (collapsed)

- [x] Cascade audit confirmed no other case file broke
  under the new `content-encoding` tracker; the 21
  legacy re-records landed inside slice 0. No separate
  commit.

## Slice 2 — Rust implementation (SRV-CLI-012, D-006 → D-020)

- [x] New module `crates/irserve-core/src/compression.rs`:
  - `pub const DEFAULT_THRESHOLD: usize = 1024` (mirrors
    `compression/index.js:76-78`).
  - `pub enum Encoding { Brotli, Gzip, Deflate }` with
    `as_str()` for the `Content-Encoding` token.
  - `pub fn negotiate(accept_encoding:
    Option<&HeaderValue>) -> Option<Encoding>` —
    parses `name;q=N` tokens, honors `q=0` exclusion
    and `*` wildcard accept/reject. Preference order
    `br > gzip > deflate` per
    `compression/index.js:44-45`. Q-rank within
    `(0, 1)` NOT honored (D-020 #3).
  - `pub fn is_compressible(content_type: &str) ->
    bool` — curated allowlist
    (`application/json`, `application/javascript`,
    `application/wasm`, `image/svg+xml`) + regex
    fallback `^text/|\+(?:json|text|xml)$/i` per D2.
  - `pub fn maybe_apply(response, bytes, req_headers,
    method, serve_config) -> Response<Body>` — the
    dispatcher seam (see §4 of design).
  - `fn encode(bytes, encoding) -> Vec<u8>` — thin
    wrapper over `flate2::write::GzEncoder` /
    `DeflateEncoder` / `brotli::CompressorWriter`.
  - `#[cfg(test)] mod tests` — unit coverage on
    `negotiate` corners and `is_compressible` against
    the MIMEs in the slice-0 fixture.
- [x] Add `flate2 = "1"` and `brotli = "8"` to
  `crates/irserve-core/Cargo.toml`.
- [x] Add `pub compression: Option<bool>` to
  `ServeConfig` in
  `crates/irserve-core/src/config.rs` with
  `#[serde(skip)]` (flag-only, not exposed via
  `serve.json`).
- [x] Wire `-u` / `--no-compression` in
  `crates/irserve/src/main.rs`:
  - clap field with `#[arg(short = 'u', long =
    "no-compression")] no_compression: bool` mirroring
    `--no-etag`.
  - Post-parse override: `if cli.no_compression {
    serve_config.compression = Some(false); }` —
    mirrors `cli.no_etag` precedent.
- [x] Thread `req.method()` through the dispatcher
  call chain into `build_file_or_304`.
- [x] Dispatcher integration in
  `crates/irserve-core/src/dispatch.rs::build_file_or_304`:
  - Snapshot bytes before `file_response` consumes
    them (the `bytes_for_compression` clone — Range
    already takes its own clone).
  - Insert `compression::maybe_apply(...)` AFTER
    `apply_custom_headers` and BEFORE Range
    pre-emption (Range branch runs upstream so a
    `Range`-bearing request never enters
    `maybe_apply`).
- [x] Drop the slice-0 temporary `content-encoding`
  mask from `L0_EXTRA_VOLATILE_HEADERS` in
  `tools/probe/run.mjs`.
- [x] Add `runner.l0.clean: ["with_accept_encoding"]`
  to the legacy `tools/probe/cases/compression-default.json`
  (single-anchor Vary-only check; identical
  dual-target).
- [x] Add `runner.l0.clean` to
  `tools/probe/cases/compression-raw.json` over all 20
  anchors, plus `bodyMayDiffer` over the 10 compressed
  anchors per D-020 #4.
- [x] Promote **ORC-058** in
  `docs/reference/serve/oracle-matrix.md` from
  reference-only to dual-target. Add 20 new ORC rows
  for the `compression-raw` anchors
  (ORC-191..ORC-210).
- [x] Verify: `cargo test --test oracle` ends at
  **81 passed, 2 skipped, 0 failed** (was 79 / 3 / 0 at
  end of Stage 7d). The two skips are pre-existing
  cases without an L0 partition or with an empty
  partition; not introduced by 7e.
- [x] Verify:
  `node tools/probe/run.mjs compression-raw
  --target=reference --snapshot=verify` green.
  `node tools/probe/run.mjs compression-raw
  --target=irserve --snapshot=verify` green.
- Commit: `feat(stage-7e): implement HTTP compression (SRV-CLI-012)`
  (`d1b9a15`).

## Slice 3 — Audit and stabilize (collapsed)

- [x] Cross-cutting audit of the post-slice-2 oracle
  state confirmed no leftover divergence. The legacy
  `etag-roundtrip` / `last-modified-roundtrip` /
  `range-request` snapshots were re-recorded in slice
  0 alongside the 21 legacy compression-touched ones,
  so the post-slice-2 irserve `Content-Encoding`
  emission lined up dual-target without further
  re-records. No separate commit.

## Slice 4 — Spec deltas + meta (this slice)

- [x] Author `openspec/changes/015-compression/`:
  - [x] `proposal.md` — closes SRV-CLI-012 (functional
    after slice 2), Q-002 (empirically pinned in slice
    0), D-006 → implemented (the flag is now wired),
    adds D-020 (the four deliberate divergences).
    Mirrors 7d's wording style with citations to
    `9bd52f0` and `d1b9a15`.
  - [x] `design.md` — §1 Reference behavior (verbatim
    `server.ts:8,25,71-72` + the empirical anchors
    list); §2 Negotiation algorithm in irserve
    (preference order, q=0 exclusion, q-rank
    divergence); §3 MIME filter (D2 — curated
    allowlist + regex, not the full mime-db port); §4
    Dispatcher seam (`build_file_or_304` insertion
    point, gate ordering); §5 Probe-runner adaptations
    (content-encoding tracking, bodyMayDiffer
    extension, runner.l0.clean blocks); §6 D-020
    divergences (inline summary, canonical entry in
    decisions.md); §7 Why this stage shape (D1/D2/D3
    plan-mode forks).
  - [x] `tasks.md` (this file).
  - [x] `specs/cli/spec.md` — MODIFIED delta that adds
    the SRV-CLI-012 Requirement. Mirrors the SRV-CLI-010
    flag-presence / capability-pointer pattern: the
    flag and its default-on / default-off effect on
    the response shape are stated here; the full L3
    wire surface lives in the new http-compression
    capability spec.
  - [x] `specs/http-compression/spec.md` — ADDED
    capability spec covering the full mechanics
    (encoder set + preference order, threshold, MIME
    filter, Vary semantics, skip conditions, Range
    pre-emption) plus Compatibility notes enumerating
    the D-020 divergences with cross-reference to
    `docs/reference/serve/decisions.md` D-020.
- [x] **Main agent (NOT the subagent):**
  - [x] Append **D-020** to
    `docs/reference/serve/decisions.md` with the four
    deliberate divergences (framing, mime-db port,
    q-rank, body-bytes). Status: `adapted`. Date:
    2026-05-14. Affected requirements: SRV-CLI-012.
    Notes that this supersedes D-006's deferral
    rationale (D-006 stays as historical record, no
    addendum needed).
  - [x] Mirror the deltas into
    `openspec/specs/cli/spec.md` (add SRV-CLI-012
    requirement) and create the new
    `openspec/specs/http-compression/spec.md`.
  - [x] Update `docs/reference/serve/inventory.md`
    SRV-CLI-012 entry — strike the Q-002 line (now
    closed), flip the "MAY ship without compression"
    hedge in Compatibility notes to past tense, cite
    D-020. Status stays `verified`.
  - [x] `README.md`: flip Stage 7e row from `todo` to
    `done`; add `Stage 7e — HTTP compression. Done.`
    to the Status block; add 3-4 lines to the "Try
    IrServe" section demonstrating
    `Content-Encoding: br` / `gzip` and
    `Vary: Accept-Encoding`; strike "gzip compression"
    from the "What is NOT yet observable" list.
  - [x] Update `docs/stage7_l3_capabilities.md` 7e
    row to past-tense DONE with slice citations.
  - [x] Run `npx -y @fission-ai/openspec@latest
    validate --all --strict` and fix any failures
    surfaced (deferred to post-review; re-run at the
    end of every Codex review round).
  - [x] Commit:
    `docs(stage-7e): 015-compression change package + capability + meta`.

## Codex review round 1 (P1 + P2 fixes)

- [x] **P3 — OpenSpec validation.** First Requirement
  in `http-compression/spec.md` had a `When ...,` lead-in
  before `SHALL`; the validator parsed only the leading
  clause and reported "must contain SHALL or MUST".
  Reshape to `The server SHALL ... when ...`. Same edit
  in the change-package delta. `npx -y
  @fission-ai/openspec@latest validate --all` now: 26
  passed / 0 failed.
- [x] **P2 — Vary append semantics.** Reference's
  `vary()` utility appends `Accept-Encoding` to any
  existing `Vary` header; the initial slice 2
  implementation only inserted when missing, which is
  cache-incorrect when a user `headers` rule already
  set e.g. `Vary: Cookie` (downstream caches would key
  only on `Cookie` and serve a brotli body to identity
  clients). Implemented `append_vary_accept_encoding`
  in `crates/irserve-core/src/compression.rs`: appends
  to existing `Vary`, deduplicates `Accept-Encoding`
  case-insensitively, leaves `Vary: *` (wildcard)
  alone. New unit tests
  `maybe_apply_appends_to_existing_vary`,
  `maybe_apply_existing_vary_with_accept_encoding_is_not_duplicated`,
  `maybe_apply_existing_vary_star_is_left_alone`. The
  old `maybe_apply_existing_vary_preserved` test was
  retired — its assertion locked in the wrong
  behavior.
- [x] **P2 — Compression seam widened to listings and
  errors.** The initial slice 2 call to
  `compression::maybe_apply` lived inside
  `build_file_or_304`, so directory-listing and error
  branches bypassed compression. The reference's
  middleware fires on every response. Moved
  `maybe_apply` to a centralized post-dispatch pass
  in `crates/irserve-core/src/server.rs::handler`
  between `apply_cors` and the request log. The pass
  is async (consumes the response body via
  `axum::body::to_bytes`); Range pre-emption stays
  intact via an explicit `StatusCode::PARTIAL_CONTENT`
  short-circuit AFTER Vary is set, so 206 responses
  carry the negotiation hook (`Vary: Accept-Encoding`)
  without re-encoding the sliced body. Reverted the
  `req_method` parameter previously threaded through
  `build_file_or_304` and its 36 test callsites —
  back to the pre-7e shape.
- [x] **P1 — Oracle assertions tightened.** Slice 2
  had: (a) `'vary'` masked in `L0_EXTRA_VOLATILE_HEADERS`
  hiding the Vary contract from every L0 probe, (b)
  the `bodyMayDiffer` overlay also stripping
  `content-encoding`. The two together let the new
  compression-raw ORCs pass even if irserve emitted no
  `Content-Encoding` at all. Fixes:
  - Dropped `'vary'` from `L0_EXTRA_VOLATILE_HEADERS`
    (the centralized seam fix above makes irserve
    emit `Vary` everywhere the reference does, so the
    global mask is no longer needed).
  - Reverted the `bodyMayDiffer` overlay's
    `content-encoding` strip — `bodyMayDiffer` is
    body-only again.
  - Added a new `contentEncodingMayDiffer` partition
    to the runner + the case schema (per-anchor opt-in
    for "body content is implementation-defined per
    D-002 / D-003 so the compression decision is too";
    the 20 legacy 4xx-error / listing anchors that
    rely on a may-differ HTML body got
    `contentEncodingMayDiffer` mirroring their
    `bodyMayDiffer`).
  - Restored compression-raw's 10 compressed anchors
    to `bodyMayDiffer`-only (the body bytes differ at
    the bit level per D-020 #4) — `content-encoding`
    is now a must-match contract there.
- [x] Verify: 359 unit tests pass; oracle 81 passed /
  2 skipped / 0 failed; OpenSpec validate 26 / 26.
- [x] Updated `docs/reference/serve/decisions.md`
  D-020's #5 stub (Vary set-if-missing) to call out
  the round-1 append fix; kept the four numbered
  divergences (#1..#4) unchanged.
- Commit:
  `docs(stage-7e): address Codex review round 1 (P1 + P2 fixes)`.

## Codex review round 2 (P2 + P3 fixes)

- [x] **P2 — Existing `Content-Encoding` from user
  `headers` was not honored.** Initial slice-2
  implementation overwrote any `Content-Encoding` set
  upstream by a user `headers` rule. Reference's
  `compression@1.8.1` middleware at
  `compression/index.js:182-188` reads `encoding =
  res.getHeader('Content-Encoding') || 'identity'` and
  skips when `encoding !== 'identity'`. Added an
  initial truthy-check skip in `maybe_apply` (Codex
  round 2 P2 — superseded by round 3 P2 below).
- [x] **P2 — OpenSpec described pre-round-1
  architecture.** Rewrote `openspec/specs/http-compression/spec.md`'s
  Implementation list + Range Requirement + Compatibility
  notes to reflect the centralized
  `server::handler` seam (vs the old
  `build_file_or_304` integration), Vary append (vs
  set-if-missing), and 206 short-circuit AFTER Vary
  (vs "never enters maybe_apply"). Same edits in the
  change-package delta at
  `openspec/changes/015-compression/specs/http-compression/spec.md`.
- [x] **P3 — Stale doc references to
  `bodyMayDiffer` stripping `content-encoding`.**
  Reworded D-020 #1 and #4 (`docs/reference/serve/decisions.md`),
  the SRV-CLI-012 inventory entry
  (`docs/reference/serve/inventory.md`), ORC-191
  (`docs/reference/serve/oracle-matrix.md`),
  `openspec/changes/015-compression/proposal.md`, and
  `openspec/changes/015-compression/design.md` to
  call out the round-1 P1 overlay-separation
  (`bodyMayDiffer` strips body bytes + `content-length`
  only; `content-encoding` is stripped only via the
  explicit per-anchor `contentEncodingMayDiffer`
  partition).
- Commit:
  `docs(stage-7e): address Codex review round 2 (P2 + P3 fixes)`.

## Codex review round 3 (P2 + P3 fixes)

- [x] **P2 — `Content-Encoding: identity` was
  incorrectly treated as already-encoded.** Round 2 P2's
  initial truthy-check skip (`contains_key`) was a
  divergence from reference, which does
  `encoding = res.getHeader('Content-Encoding') || 'identity';
  if (encoding !== 'identity') skip`. With a user
  `headers` rule setting `Content-Encoding: identity`,
  reference compresses normally (overwriting `identity`
  with the chosen encoder); the round-2 irserve skipped
  and left `identity` on the wire. Refined the gate in
  `maybe_apply` to compare against the literal
  `identity` string (case-sensitive, mirroring JS
  strict equality). New unit test
  `maybe_apply_existing_identity_falls_through_and_compresses`
  pins the round-trip. Two new raw dual-target probes
  in `compression-raw.json`:
  `user_ce_identity_above_threshold` (user-set
  `identity` → compressed to br, header overwritten)
  and `user_ce_br_above_threshold` (user-set `br` →
  middleware skips, header preserved verbatim, body
  served raw). Added ORC-211 and ORC-212.
  Reference snapshots re-recorded (case file now 22
  anchors, +2; the existing
  `maybe_apply_existing_content_encoding_passthrough`
  unit test was renamed
  `maybe_apply_existing_non_identity_content_encoding_passthrough`
  for symmetry with the new identity case).
- [x] **P3 — Stale doc references to old
  pre-round-1 architecture outside OpenSpec.**
  Rewrote `docs/stage7_l3_capabilities.md` row 7e to
  reflect the centralized `server::handler` seam (vs
  the original `build_file_or_304` slice-2 wiring)
  plus the Codex round 1/2/3 evolution; updated
  ORC-206 in `docs/reference/serve/oracle-matrix.md`
  to say "206 DOES enter `maybe_apply`, gets `Vary`
  set, then short-circuits on `PARTIAL_CONTENT`" (vs
  the stale "206 never enters maybe_apply"). Also
  bumped the "20 anchors" counts in inventory.md to
  22 for the post-round-3 case file state.
- [x] Verify: `cargo test --workspace --lib` — 361
  passed (+1 vs round 2); `cargo test --test oracle`
  — 81 passed / 2 skipped / 0 failed (unchanged at
  the case-file level — the 2 new anchors land
  inside the existing `compression-raw` case);
  `npx @fission-ai/openspec validate --all --strict`
  — 26 passed / 0 failed.
- Commit:
  `docs(stage-7e): address Codex review round 3 (P2 + P3 fixes)`.

## Codex review round 4 (P2 fix)

- [x] **P2 — Above-threshold 206 Partial Content
  ranges were not compressed.** Initial slice 2 wiring
  (and the round 1 P2 / round 3 P2 refinements) all
  carried an explicit `if status == PARTIAL_CONTENT`
  short-circuit in `maybe_apply` — a deliberate
  safety measure based on the slice-0 probe of a
  16-byte range (which naturally fails the threshold
  gate anyway). Codex round 4 P2 surfaced empirically
  that the reference has NO status-based 206 skip:
  `serve-handler/src/index.js:749` sets
  `Content-Length` to the range size before
  `writeHead`, and `compression/index.js:177`
  evaluates `chunkLength < threshold` uniformly
  across statuses. A 1200-byte sliced body crosses
  threshold and gets encoded with `Content-Range`
  retained verbatim.
  - Fix: removed the status-based 206 skip from
    `crates/irserve-core/src/compression.rs`. 206
    responses now follow the same gate ordering as
    200s; small ranges naturally fail the threshold
    gate; large ranges encode normally.
  - Renamed the existing unit test
    `maybe_apply_partial_content_keeps_vary_skips_encoding`
    to
    `maybe_apply_partial_content_below_threshold_keeps_vary_skips_encoding`
    (a 16-byte body that fails the threshold gate).
    Added
    `maybe_apply_partial_content_above_threshold_compresses`
    (a 1500-byte body that passes the gate and gets
    encoded; asserts `Content-Encoding: br` is set
    and `Content-Range` is retained verbatim).
  - New raw dual-target probe anchor
    `range_big_html_above_threshold` (Range:
    `bytes=0-1199`, 1200-byte slice) in
    `compression-raw.json`. Added ORC-213.
    `bodyMayDiffer` per D-020 #4. Reference snapshot
    re-recorded — the new anchor pins `206 +
    Content-Range: bytes 0-1199/1494 + Vary +
    Content-Encoding: br + Transfer-Encoding: chunked`
    on reference (vs `206 + Content-Encoding: br +
    Content-Length: <compressed>` on irserve — D-020
    #1 framing already covers this).
  - Updated ORC-206's must-match text to clarify that
    the no-`Content-Encoding` outcome is due to the
    threshold gate, not a status-based skip. Added a
    new Scenario in `openspec/specs/http-compression/spec.md`
    for the above-threshold case. Bumped anchor counts
    22 → 23 in inventory.md.
- [x] Verify: `cargo test --workspace --lib` — 362
  passed (+1 vs round 3); `cargo test --test oracle`
  — 81 passed / 2 skipped / 0 failed (the new anchor
  lands inside `compression-raw`); `npx @fission-ai/openspec
  validate --all --strict` — 26 passed / 0 failed.
- Commit:
  `docs(stage-7e): address Codex review round 4 (P2 fix)`.

## Validation

Latest totals at end of Stage 7e (refreshed after every
Codex round; doc-hygiene rounds do not change the
underlying counts):

- `cargo test --workspace --lib` — green; new unit
  tests under `compression::tests` and updates to
  `dispatch::tests` cover the compression gate corners
  (HEAD, no-transform, below-threshold, identity-only,
  flag-disabled, OPTIONS-as-GET composition, Range
  pre-emption).
- Oracle harness: `target=irserve total=83 passed=81
  skipped=2 failed=0`. Promotion delta vs. end-of-7d:
  `compression-default.json` (1 anchor via slice 2)
  and `compression-raw.json` (20 anchors via slice 2)
  both promoted from skipped to passed.
- `node tools/probe/run.mjs compression-default
  --target=reference --snapshot=verify` — green.
- `node tools/probe/run.mjs compression-default
  --target=irserve --snapshot=verify` — green.
- `node tools/probe/run.mjs compression-raw
  --target=reference --snapshot=verify` — green.
- `node tools/probe/run.mjs compression-raw
  --target=irserve --snapshot=verify` — green.
- `npx -y @fission-ai/openspec@latest validate --all
  --strict` — to be run at end-of-stage. Re-run at
  end of every Codex review round.
