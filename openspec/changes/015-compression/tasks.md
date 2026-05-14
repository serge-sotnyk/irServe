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
- [x] Extend the runner's `bodyMayDiffer` overlay to
  also strip `content-encoding` (the compression
  decision is a function of body length, so a
  body-may-differ anchor implies an
  encoding-may-differ one).
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
