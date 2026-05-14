# Stage 7e — HTTP compression (`-u`/`--no-compression` un-no-op)

## Context

Stage 7e is the last `todo` row in the README stage map and the fifth /
final L3 sub-stage per [`docs/stage7_l3_capabilities.md`](../../../repos/sotnyk/irServe/docs/stage7_l3_capabilities.md).
It closes the L3 polish surface so the MVP stretch goal is met; L4 (symlinks,
TLS, Windows quirks) stays explicitly deferred.

Stage 7e closes **SRV-CLI-012** (`-u`/`--no-compression` un-no-op, P2, level L3),
**Q-002** (compressed content-type set + minimum body-size threshold), and
flips **ORC-058** + new sibling rows from reference-only to dual-target. It
also transitions **D-006** from `adapted` (100 % deferred) to `implemented`
(the flag is functional; only enumerated divergences land as a new `D-020`).

The stage is the **biggest methodological risk** in Stage 7 per
[anti-hallucination rule #8](../../../repos/sotnyk/irServe/README.md#anti-hallucination-rules)
(mirroring a third-party library — Node's `compression` middleware — whose
defaults are not all visible from source). Mitigation: probe-first slice 0,
mandatory out-of-scope list (rule #10), early architectural fork-resolution
in plan-mode.

ChangeID: `openspec/changes/015-compression/`.

## Architectural decisions (resolved in plan-mode)

These two forks were resolved with the user before authoring slices. They
shape the spec delta and the implementation surface; revisiting them
mid-implementation triggers anti-hallucination #9.

### D1 — Encoder set: gzip + deflate + brotli (full parity)

Mirror the reference's negotiation order `br > gzip > deflate` per
`compression/index.js:44-45`. No D-NNN on encoder coverage. Crates:
`flate2` (gzip + deflate) + `brotli`.

Rationale: under the user-pick, the cost of one extra crate (`brotli`) is
preferable to the surface of a `D-020 brotli omission` divergence that
clients with `Accept-Encoding: br, gzip` would hit byte-for-byte.

### D2 — MIME filter: curated allowlist + regex fallback

Mirror reference shape, not byte-for-byte database. Implementation:

1. A curated allowlist of common types: `text/*` (all subtypes via prefix
   match), `application/json`, `application/javascript`, `application/wasm`,
   `image/svg+xml`. Source: empirical slice-0 probe of the reference + the
   existing `mime_for` allowlist in `crates/irserve-core/src/mime.rs`.
2. Regex fallback `^text/|\+(?:json|text|xml)$` (case-insensitive) per
   `compressible/index.js:23`. Catches future / long-tail types like
   `application/ld+json`, `application/atom+xml` that the curated list
   doesn't enumerate.

Out: porting the full `mime-db`'s ~150-entry `compressible` table. Any
divergence from the reference on a non-allowlisted type lands as a `D-020`
entry citing the specific MIME and the probe ID. Slice 0 fixes the
allowlist by probing the reference on `txt|html|css|js|json|svg|wasm|png|woff2|mp4`.

### D3 — `D-020` divergence sketch (drafted in plan-mode, finalized post-slice-0)

Anticipated content of the new `D-020` entry (slot confirmed; current
high-water is D-019 per Stage 7c). Two divergences expected:

1. **Compressed responses send `Content-Length`, not chunked.** Reference
   removes `Content-Length` and uses chunked transfer-encoding because the
   compression stream length is unknown at header-write time. irserve has
   the final compressed bytes in memory (static-file model, not streaming)
   so it sends `Content-Length: <compressed-len>` with no `Transfer-Encoding`.
   Body bytes are byte-identical at the body-frame level; only the framing
   diverges. Probe runner's `runRequestRaw` will see this.
2. **No `mime-db` compressible-flag database.** Curated allowlist per D2.
   Concrete out-of-scope MIMEs to enumerate after slice-0 probing.

Whether D-020 needs to also cover **Range + compression interaction**
depends on slice 0's probe of the reference behavior (see Risks).

## Out of scope (mandatory pre-stage list per rule #10)

Stage 7e explicitly DOES NOT mirror:

- The **mime-db `compressible` table** (~150 entries). Curated allowlist +
  regex fallback per D2.
- **q-value parsing nuances** beyond presence/absence per encoding. The
  reference uses `Negotiator` which honors `Accept-Encoding: gzip;q=0` to
  exclude gzip, `*;q=0` to exclude all, etc. We honor `q=0` (exclude) and
  `*` (wildcard-accept) but do not rank q-values between 0 and 1. Probe
  case `compression-negotiation` (slice 0) pins the corners we DO cover;
  the rest is `D-020`-eligible if a review-round surprise lands.
- **Chunked transfer-encoding for compressed responses.** irserve sends
  `Content-Length: <compressed-len>`. See D3 above.
- **Streaming / backpressure** (`stream.on('drain')`, line 140-152 of
  `compression/index.js`). irserve buffers all bytes in memory — the
  static-file model.
- **Already-encoded passthrough.** irserve never sets `Content-Encoding`
  upstream of compression, so the `compression/index.js:183-189` skip is
  vacuously satisfied. If a user `headers` rule ever sets
  `Content-Encoding` we treat as an unsupported edge case (`D-020`).
- **`Cache-Control: no-transform` skipping.** Mirror — added to slice 2.
  But: in irserve, `Cache-Control` is set ONLY by user `headers` rules
  (Stage 7d verified zero default). The check runs against the merged
  response, so the user-rule case is contractual.
- **Per-request encoder enforcement** via the middleware's
  `enforceEncoding` option (line 198-210). The reference doesn't override
  the default `identity`, so this branch is dead in practice.
- **HEAD with compression negotiation.** Reference skips compression on
  HEAD entirely (line 192-195). Mirror.
- **OPTIONS with compression negotiation.** Reference routes OPTIONS as
  GET (Stage 7d) so the static body would be compressed. Mirror —
  exercised by a slice-0 probe.
- **brotli compression-quality tuning.** Use brotli crate defaults; do
  not match the reference's brotli quality level (which is `zlib`'s
  default `BROTLI_PARAM_QUALITY=4` for streams). Body framing differs;
  bytes inside are not contractual.

## Empirical anchors from reference reconnaissance

These are facts established in plan-mode (full transcript in this
document's reconnaissance preamble); record them here so implementation
slices don't re-derive.

- **`compression@1.8.1`** vendored at
  `third_party/serve/node_modules/compression/index.js`.
- Wired in `serve` at
  `third_party/serve/source/utilities/server.ts:8,25,71-72`:
  ```ts
  import compression from 'compression';
  const compress = promisify(compression());
  // ...
  if (!args['--no-compression'])
    await compress(request as ExpressRequest, response as ExpressResponse);
  ```
- CLI flag defined at `cli.ts:53,154,171`: `-u, --no-compression` (long-form),
  `'-u': '--no-compression'` (short alias).
- `serve-handler` does **not** compress (greppable confirmation). Stage 7e
  is a `serve`-CLI-only feature; the handler library is untouched.
- Default threshold = **1024 bytes** (`compression/index.js:76-78`).
- Negotiation order = **`['br','gzip','deflate']` preferred `['br','gzip']`**
  when Node has brotli (Node 11+, the vendored bundle does). Identity is
  always available as fallback.
- `Vary: Accept-Encoding` is **always** set when the middleware runs, even
  if compression is ultimately skipped. (`compression/index.js:175`.) This
  is the existing oracle anchor for ORC-058.
- `Cache-Control: no-transform` skipping at `compression/index.js:293-300`.
- HEAD skipping at `compression/index.js:192-195`.
- `compressible` decision at `compressible/index.js:41-58`: lookup
  `mime-db[type].compressible`, regex fallback `^text/|\+(?:json|text|xml)$/i`.
  Empirical compressible MIMEs: `application/json` ✓,
  `image/svg+xml` ✓, `application/wasm` ✓.
- irServe seam: `crates/irserve-core/src/dispatch.rs::build_file_or_304`
  (lines 767-858). Compression slot **after** `apply_custom_headers`
  (line 789, so the merged final `Content-Type` and `Cache-Control` are
  visible to the negotiator) and **before** the Range handling
  (line 857, so an uncompressed bytes vec is still available for Range
  slicing).
- CLI flag pattern to mirror: `--no-etag` in
  `crates/irserve/src/main.rs:74-83,144-146`.
- Probe runner raw-socket pathway: `tools/probe/run.mjs:403-476`
  (`runRequestRaw`). Reusable from Stage 6f, no new infra required.
- `content-encoding` is **not** in `TRACKED_RESPONSE_HEADERS`
  (`run.mjs:67-85`). Slice 1 adds it.
- Existing artifacts: SRV-CLI-012 inventory entry (status `verified` —
  reference behavior is pinned via `compression-default.json`, but
  irserve still no-ops per D-006); ORC-058 single-anchor verified; Q-002
  open; D-006 status `adapted` with 100 % deferral text. Highest
  D-NNN slot = D-019; next free = **D-020**.

## Plan

### Slice 0 — Reverse-engineering probes against reference

The probe-first / empirical-before-implement slice per anti-hallucination
rule #8. No Rust changes; no irserve runs. Pure reference reconnaissance,
landing as committed evidence in `tools/probe/snapshots/`.

Goal: pin the on-the-wire compression surface so the implementation slice
matches the contract, not the assumed contract. Output: a new
`compression-raw.json` case file plus its snapshot.

Steps:

1. Extend `tools/probe/run.mjs` to track the `content-encoding` response
   header. Locate the `TRACKED_RESPONSE_HEADERS` array
   (`run.mjs:67-85`), add `'content-encoding'`, and verify the existing
   `compression-default.json` snapshot still passes (the existing snapshot
   was captured via `fetch` which decompresses, so `content-encoding` was
   never present on the reference snapshot either — re-snapshot it under
   raw mode if the assertion stiffens). **Commit gate:** `cargo test --test oracle`
   on irserve target must still be green; reference target re-snap if needed.
2. Author `tools/probe/cases/compression-raw.json`. Use `"mode": "raw"`
   per the `traversal-raw-encoded.json` / `multislash-collapse.json`
   precedent (file paths in the reconnaissance log). Fixture: at minimum
   files spanning the threshold and the MIME allowlist:
   - `tiny.html` (< 1024 bytes, should NOT compress, Vary still set).
   - `big.html` (> 1024 bytes, should compress).
   - `big.css` (> 1024 bytes).
   - `big.js` (> 1024 bytes).
   - `data.json` (> 1024 bytes, `application/json`).
   - `image.png` (binary, should NOT compress regardless of size).
   - `font.woff2` (binary, should NOT compress).
   - `media.mp4` (binary, should NOT compress).
   - `vector.svg` (`image/svg+xml`, should compress).
   - `bin.wasm` (`application/wasm`, should compress).
3. Anchor set (raw-mode requests, mirror `traversal-raw-encoded.json`
   style). For each MIME pair `(below_threshold, above_threshold)` send
   `Accept-Encoding: gzip, deflate, br` and `Accept-Encoding: identity`:
   - `tiny_html_gzip` — expect `Vary: Accept-Encoding` present, **no**
     `Content-Encoding`, body uncompressed.
   - `big_html_gzip_deflate_br` — expect `Vary: Accept-Encoding` present,
     `Content-Encoding: br` (brotli wins by reference's preference order),
     body brotli-compressed.
   - `big_html_gzip` — `Content-Encoding: gzip`.
   - `big_html_deflate` — `Content-Encoding: deflate`.
   - `big_html_identity_only` — no `Content-Encoding`, body uncompressed.
   - `big_html_no_accept_encoding` — Negotiator's default for absent
     header (`enforceEncoding=identity` per line 74) so no compression.
     **Probe verifies.**
   - `big_html_gzip_q0` — `Accept-Encoding: gzip;q=0, br` → must NOT pick
     gzip; expect brotli.
   - `big_html_star_q0` — `Accept-Encoding: *;q=0` → no compression.
   - `binary_png_gzip` — no `Content-Encoding`, Vary present.
   - `wasm_gzip` — `Content-Encoding: br` (compressible).
   - `svg_gzip` — `Content-Encoding: br` (compressible).
   - `head_big_html_gzip` — HEAD method, expect no `Content-Encoding`
     (HEAD skip). Vary may or may not be set — slice-0 records reality.
   - `options_big_html_gzip` — OPTIONS, follows GET pipeline post-7d.
     Expect `Content-Encoding: br` (this validates the 7d→7e composition).
   - `no_transform_big_html_gzip` — fixture: a user `headers` rule
     setting `Cache-Control: no-transform` on `big.html`. Expect no
     `Content-Encoding`, Vary present.
   - `range_big_html_gzip` — `Range: bytes=0-15` + `Accept-Encoding: br, gzip`.
     **Critical empirical question — does reference compress 206 responses?**
     Three outcomes are possible: (a) skip compression on Range present;
     (b) compress the sliced body; (c) compress the full body then range
     into compressed bytes (unlikely). Pin reality.
4. Record snapshot via
   `node tools/probe/run.mjs compression-raw --target=reference --snapshot=update`.
5. Inspect the snapshot. Annotate findings inline in this plan file
   (Risks section below) and prepare the slice-2 implementation to
   match.
6. Close Q-002 in `docs/reference/serve/open-questions.md` by amending
   `Resolution:` with the empirically-observed threshold and the
   curated MIME set + regex pattern.
7. Optional micro-edit: if slice 0 reveals reference uses brotli quality
   level we can match cheaply, note it. Otherwise enumerate as out-of-scope.
8. Commit (after explicit approval per `feedback_iterative_commits` memory):
   ```
   probe(stage-7e): pin reference compression surface (Q-002 closure)
   ```

### Slice 1 — Probe runner extension audit

If slice 0's `content-encoding` tracking change landed cleanly, this
slice collapses into the slice 0 commit. Otherwise:

- Verify no other case files broke (`node tools/probe/run.mjs --all
  --target=reference --snapshot=verify` against the existing
  snapshots).
- Re-snapshot any case where the reference now legitimately includes
  `content-encoding` (probably zero in practice — pre-7e, no other
  probe uses raw mode against compressible content with
  `Accept-Encoding` set).
- Commit: `probe(stage-7e): track content-encoding in runner` (if
  separate from slice 0).

Likely outcome: this slice is empty / collapsed.

### Slice 2 — Rust implementation

The single non-trivial code slice. Three landing surfaces:

1. **New module** `crates/irserve-core/src/compression.rs` with:
   - `pub struct CompressionConfig { enabled: bool, threshold: usize }`
     (defaults: enabled, threshold 1024). Constructed from `ServeConfig`
     by the dispatcher caller.
   - `pub fn negotiate(accept_encoding: Option<&HeaderValue>) ->
     Option<Encoding>` returning the chosen encoder (`Brotli` >
     `Gzip` > `Deflate`), `None` for identity-only or no-acceptable.
     Honors `q=0` exclusions and `*` wildcard. q-rank between 0 and 1
     is **not** honored — out-of-scope per #1 in the out-of-scope list.
   - `pub fn is_compressible(content_type: &str) -> bool` — curated
     allowlist + regex fallback per D2.
   - `pub fn encode(bytes: &[u8], encoding: Encoding) -> Vec<u8>` —
     thin wrapper over `flate2::write::GzEncoder`,
     `flate2::write::DeflateEncoder`, and `brotli::CompressorWriter`.
   - Inline `#[cfg(test)]` module with negotiation tests
     (q=0, wildcard, missing header, preference order) and MIME-filter
     tests against the slice-0 probe expectations.
2. **CLI flag** in `crates/irserve/src/main.rs`. Mirror `--no-etag` shape:
   ```rust
   /// SRV-CLI-012: disable HTTP compression. Mirrors
   /// `third_party/serve/source/utilities/cli.ts:53,154,171`
   /// (`-u, --no-compression`). Default is compression-on; this
   /// flag flips the dispatcher's compression config to disabled.
   #[arg(short = 'u', long = "no-compression")]
   no_compression: bool,
   ```
   Post-parse wiring (mirror `cli.no_etag` at line 144-146):
   ```rust
   if cli.no_compression { serve_config.compression = Some(false); }
   ```
   Add `pub compression: Option<bool>` to `ServeConfig` in
   `crates/irserve-core/src/config.rs`. **Do not** add `compression` to
   the `serve.json` schema — reference's `compression` is CLI-only, not
   a config-file field. Field exists in the struct only as a flag-state
   carrier.
3. **Dispatcher integration** at
   `crates/irserve-core/src/dispatch.rs::build_file_or_304`. Insert
   between the `apply_custom_headers` call (line 789) and the Range
   handling (line ~857):
   ```rust
   // Stage 7e — compression negotiation
   let (response, did_compress) = compression::maybe_apply(
       response,
       request_headers,
       serve_config,
   );
   ```
   where `maybe_apply`:
   - Always appends `Vary: Accept-Encoding` if compression is enabled
     (mirror reference's "always set Vary when middleware runs"
     behavior). The flag-disabled path emits nothing — irserve as
     pre-7e.
   - Returns the response unchanged with `did_compress=false` if:
     - method is HEAD or status ≥ 300 (reference skips on these);
     - `Range` header present in the request (per slice-0 empirical
       finding — pin behavior to whatever the reference does);
     - `Cache-Control: no-transform` is in the final response;
     - body size < threshold (1024);
     - content-type not compressible per `is_compressible`;
     - `negotiate(accept_encoding)` returns `None` (identity-only or
       `*;q=0`).
   - Otherwise: compresses the body, sets `Content-Encoding`, sets
     `Content-Length` to the compressed size (per D3 — divergence
     from reference's chunked, recorded as D-020), returns the new
     response.

   The signature must not break Stage 7c's Range pre-emption: if
   `Range` is in the request, `maybe_apply` returns early so the
   subsequent `range::apply` sees the raw bytes for slicing. (If
   slice 0 reveals the reference DOES compress 206 bodies, revisit
   this and revise; pre-build the plan assuming Range-pre-empts-
   compression, then adapt.)

4. **CLI propagation**: extend the `ServeConfig` → dispatcher path. The
   `serve_config` is already threaded through `dispatch` per Stage 7a/7b;
   add the bool flag to its constructor. Mirror `etag` plumbing exactly.

5. **Unit tests in `dispatch::tests`**:
   - `compression_gzip_above_threshold_compresses`
   - `compression_below_threshold_no_encoding`
   - `compression_disabled_via_flag_no_vary`
   - `compression_head_skipped`
   - `compression_no_transform_skipped`
   - `compression_binary_mime_skipped`
   - `compression_range_pre_empts` (assert no `Content-Encoding` on a
     206)
   - `compression_options_compresses_like_get` (Stage 7d composition)

6. **Cargo.toml**: add `flate2 = "1"` and `brotli = "7"` (or whatever
   current crates.io serves at the time) to `crates/irserve-core`.
   Pin precise versions per repo convention; check the existing
   `Cargo.toml`'s style.

7. **Oracle**: run `node tools/probe/run.mjs compression-raw
   --target=irserve --snapshot=verify`. All anchors must pass. If any
   anchor fails, triage:
   - Body-bytes divergence (compressed bytes don't byte-match
     reference's): per D3, body-frame is byte-equal; only framing
     differs. The probe runner's raw mode SHOULD compare body bytes
     after dropping `Content-Length` / `Transfer-Encoding` headers,
     OR use a `*MayDiffer` overlay. Decide based on what the runner
     does (read it; the snapshot's body section is typically a
     content-hash). If it byte-compares, add a `runner.l0.*MayDiffer`
     overlay scoped to compressed-body anchors with a note that the
     body frame is canonical, the wire framing differs by D-020.
   - Negotiation divergence: bug in `negotiate`. Fix.
   - MIME divergence: add the missing MIME to the curated allowlist
     (and to the slice-0 probe). Note in D-020 if reference includes
     something we exclude.

8. Add `runner.l0.clean` block to `compression-raw.json` enumerating
   all anchors that pass byte-equal (plus any `runner.l0.*MayDiffer`
   overlays needed).

9. Add `runner.l0.clean: ["with_accept_encoding"]` to the legacy
   `compression-default.json` (existing single-anchor) — that case
   only checks `Vary`, which is identical in both targets.

10. Promote **ORC-058** in
    [`docs/reference/serve/oracle-matrix.md`](../../../repos/sotnyk/irServe/docs/reference/serve/oracle-matrix.md)
    from reference-only to dual-target. Add new ORC rows for each
    `compression-raw` anchor (mirror how Stage 7c added rows for
    `range-requests`). Numbering: pick contiguous slots after the
    current high-water — the matrix's last assigned slot is the next
    available ORC-N starting from wherever 7c/7d left off.

11. Commit (after approval):
    ```
    feat(stage-7e): implement HTTP compression (SRV-CLI-012, D-006 → D-020)
    ```

### Slice 3 — Audit and stabilize

- Run the full oracle harness (`cargo test --test oracle`). Expected:
  prior 79 passed + new compression-raw anchors all green. If any prior
  probe broke (e.g. an old text-asset case now emits `Vary` /
  `Content-Encoding`), audit the snapshot.
- Audit `etag-roundtrip` / `last-modified-roundtrip` / `range-requests`
  for compression interaction. If the static asset they fetch is > 1024
  bytes of compressible type AND the case requests with `Accept-Encoding`,
  the post-7e irserve response may now include `Vary` / `Content-Encoding`.
  Reference behavior was already this way — so if the snapshot doesn't
  have them, the **runner was stripping them** (via the missing
  `content-encoding` track). Slice 0's tracker addition might therefore
  cascade into snapshot re-records. Pre-mitigate: in slice 0, before
  adding to `TRACKED_RESPONSE_HEADERS`, run `--target=reference
  --snapshot=verify` across `--all` and record the delta.
- Per anti-hallucination rule #2: any failing legacy anchor that turns
  out to be a legitimate post-7e change in target behavior is fine; but
  silently weakening the contract by `*MayDiffer`-overlaying is NOT
  fine. Either pin the new behavior in the snapshot (re-record it) or
  surface the divergence as a D-020 supplement.
- Commit (only if real changes land):
  ```
  probe(stage-7e): cascade audit + re-record post-content-encoding
  ```

### Slice 4 — Meta slice (spec deltas + plan file)

Delegate per the kickoff-template subagent trigger. Peer change package:
`openspec/changes/014-cache-headers-and-preflight/` (most recent; closest
in shape — single SRV-CLI un-no-op + un-deferral of an existing decision).

Structured brief to the subagent:

- This plan file path.
- Commit log from slices 0-3 (`git log --oneline -- main..HEAD` at
  hand-off).
- D-020 final text (drafted from D3 above plus any slice-0 surprises).
- Plan-mode reconnaissance summary (above sections + the empirical
  anchors list).
- Peer package path: `openspec/changes/014-cache-headers-and-preflight/`.

Subagent produces:

- `openspec/changes/015-compression/proposal.md` — Why / What. Closes
  SRV-CLI-012, Q-002. Transitions D-006 `adapted` → `implemented`. Adds
  D-020 known divergences. Flips ORC-058 + new sibling rows to
  dual-target. Mirror 7d's wording.
- `openspec/changes/015-compression/design.md` — architecture. §1
  Reference behavior (paste `server.ts:25,71-72` + `compression/index.js`
  decision points verbatim with line numbers). §2 Negotiation algorithm
  (preference order, q=0 honored, q-rank out-of-scope). §3 MIME filter
  (D2 — curated allowlist + regex). §4 Dispatcher seam (the
  `build_file_or_304` insertion point with file:line refs). §5
  D-020 divergences (framing, mime-db port, q-rank, brotli quality).
  No new D-NNN beyond D-020.
- `openspec/changes/015-compression/tasks.md` — slice-by-slice with
  `[x]` per the 7c / 7d convention. Reproduces this plan's slice
  structure with checkbox state from the commit log.
- Spec deltas:
  - **MODIFIED** `openspec/specs/cli/spec.md` — add SRV-CLI-012
    Requirement (un-no-op the flag; default-on; flips the dispatcher
    config). Mirror style of the SRV-CLI-013 (`--no-etag`) addition
    from Stage 7b.
  - **ADDED** `openspec/specs/http-compression/spec.md` (new capability
    spec dir) — covers the mechanics: encoder set + preference order,
    threshold, MIME filter, Vary semantics, skip conditions, Range
    interaction. Mirror `openspec/specs/cors/spec.md` from Stage 7d
    (the latest "new capability" pattern).
  - **Compatibility notes** section enumerates D-020 divergences with
    references to the new `D-020` entry in `docs/reference/serve/decisions.md`.
- `docs/reference/serve/decisions.md` — append D-020 in the existing
  D-NNN style, dated, status `adapted`. The final text incorporates
  slice-0 findings.
- `docs/reference/serve/open-questions.md` — close Q-002 with
  `Resolution:` line citing the slice-0 snapshot.
- `docs/reference/serve/inventory.md` — SRV-CLI-012 entry: update its
  `Open questions:` line (or remove if it referenced Q-002); flip any
  "MAY ship without compression" hedge in the Compatibility notes; cite
  D-020. Status stays `verified`.
- Drop `docs/features/0019_PLAN_stage7e_compression.md` (post-0018,
  matching the existing 0001..0018 numbering). Subagent copies from
  this `.claude/plans/` file with cosmetic cleanup.

Commit (after approval):
```
docs(stage-7e): add 015-compression change package
```

### Stage close

- Flip the 7e row in `README.md` from `todo` to `done`.
- Update README "Try IrServe" section: add a one-line example showing
  `Content-Encoding: gzip` on a compressed asset response and `Vary:
  Accept-Encoding`. Strike `gzip compression` from the "What is NOT
  yet observable" list (line 330).
- The compatibility levels block (line 111) says L3 is `HTTP polish:
  ETag, Last-Modified, conditional requests, cache behavior` — Stage
  7e completes that line. The README's status block (line 33) gets a
  new `Stage 7e — HTTP compression. Done.` row.
- Anticipate Codex review rounds per `feedback_review_rounds`. Each
  round = one commit
  `docs(stage-7e): address Codex review round N (P{...} fixes)`.

## Critical files

To modify:

- `crates/irserve-core/src/compression.rs` (new module).
- `crates/irserve-core/src/dispatch.rs` (~10-line insertion in
  `build_file_or_304` after line 789; ~80 lines of new unit tests in
  `#[cfg(test)] mod tests`).
- `crates/irserve-core/src/config.rs` (add `compression: Option<bool>`
  field to `ServeConfig` — flag-only, not exposed via `serve.json`).
- `crates/irserve-core/Cargo.toml` (add `flate2`, `brotli`).
- `crates/irserve/src/main.rs` (~10 lines: clap field for
  `-u`/`--no-compression` + post-parse wiring; mirror `--no-etag`).
- `tools/probe/run.mjs` (~1-line addition to `TRACKED_RESPONSE_HEADERS`).
- `tools/probe/cases/compression-raw.json` (new case file).
- `tools/probe/cases/compression-default.json` (add `runner.l0.clean`
  block).
- `tools/probe/snapshots/compression-raw.json` (new reference snapshot
  via `--target=reference --snapshot=update`).
- `docs/reference/serve/oracle-matrix.md` (flip ORC-058 + add new ORC
  rows for `compression-raw` anchors).
- `docs/reference/serve/inventory.md` (SRV-CLI-012 update).
- `docs/reference/serve/open-questions.md` (close Q-002).
- `docs/reference/serve/decisions.md` (append D-020; D-006 may stay as
  historical, or get a "superseded by Stage 7e" addendum — subagent
  picks the convention by mirroring how D-008..D-016 were handled when
  their corresponding sub-stages landed).
- `README.md` (stage map row flip + observability section).
- `docs/features/0019_PLAN_stage7e_compression.md` (new in-repo plan).
- `openspec/changes/015-compression/` (new directory: proposal /
  design / tasks / spec deltas).
- `openspec/specs/cli/spec.md` (MODIFIED — add SRV-CLI-012
  requirement).
- `openspec/specs/http-compression/spec.md` (ADDED — new capability).

Not to modify:

- `third_party/serve` and `third_party/serve-handler` (hard stop per
  AGENTS.md).
- Existing snapshots untouched **unless** the slice-3 cascade audit
  surfaces legitimate post-7e behavior change (then re-record per the
  rule — never silent weakening).

## Reuse / no new abstractions

- `apply_cors` / `apply_custom_headers` — unchanged. Compression slots
  AFTER both so it sees the final merged headers (e.g.
  `Cache-Control: no-transform` from a user rule, final `Content-Type`
  from custom headers override).
- `build_file_or_304` — extended with one `compression::maybe_apply`
  call. Range pre-emption stays at the existing seam (line 791
  `bytes_for_range` clone happens before compression).
- `mime_for` (`crates/irserve-core/src/mime.rs`) — reused for the
  compressibility decision. The compression module's `is_compressible`
  takes the `Content-Type` value as a string and applies the curated
  + regex check; it does NOT re-derive the MIME from the path.
- Raw-socket probe pathway (`run.mjs::runRequestRaw`) — reused
  verbatim. No new probe-runner infra.
- `runner.l0.clean` partition mechanism — reused for the new probe
  case.

## Verification (end-to-end)

After all slices commit, before the meta slice:

```bash
# Targeted probe — compression surface dual-target green.
node tools/probe/run.mjs compression-raw --target=irserve --snapshot=verify
node tools/probe/run.mjs compression-default --target=irserve --snapshot=verify

# Full oracle harness — regression-free across the L0-clean set.
cargo test --test oracle

# Workspace unit tests.
cargo test --workspace --lib

# Manual smoke — observability on a large compressible asset.
mkdir -p _tmp
python -c "print('body{color:red}'*100)" > _tmp/big.css
cargo run -- --listen 3010 _tmp &
curl -i -H 'Accept-Encoding: br, gzip, deflate' \
  http://127.0.0.1:3010/big.css
# Expect: 200, vary: Accept-Encoding, content-encoding: br, body brotli-compressed.

# Manual smoke — flag flips it off.
cargo run -- --no-compression --listen 3010 _tmp &
curl -i -H 'Accept-Encoding: br, gzip, deflate' \
  http://127.0.0.1:3010/big.css
# Expect: 200, no vary header, no content-encoding, body raw.

# Manual smoke — short alias.
cargo run -- -u --listen 3010 _tmp &
# (same as above)

# Manual smoke — sub-threshold body.
echo 'tiny' > _tmp/tiny.css
curl -i -H 'Accept-Encoding: br, gzip' http://127.0.0.1:3010/tiny.css
# Expect: 200, vary: Accept-Encoding present (negotiation ran), no content-encoding (below threshold).

# Manual smoke — binary asset.
# (drop a real png at _tmp/img.png)
curl -i -H 'Accept-Encoding: br, gzip' http://127.0.0.1:3010/img.png
# Expect: 200, vary present, no content-encoding (not compressible).

# Manual smoke — Range pre-empts compression.
curl -i -H 'Accept-Encoding: br' -H 'Range: bytes=0-15' \
  http://127.0.0.1:3010/big.css
# Expect: 206, content-range: bytes 0-15/N, no content-encoding (Range short-circuits compression).
```

## Risks / signals to watch

- **Slice 0 surprise — Range + compression interaction.** If the
  reference compresses 206 responses (option (b) in slice-0 plan), the
  Stage 7c contract that "Range emits exactly the requested byte
  window" gets weird (compressed bytes are no longer at the original
  offsets). Most likely the reference does NOT (Range is checked
  before compression in Express middleware ordering: serve-handler
  emits Range before `compress` returns). But pin empirically. If
  reference DOES compress 206, surface to the user — this is a
  significant architectural call requiring a fresh decision (anti-
  hallucination rule #9 trigger).
- **Slice 0 surprise — HEAD Vary.** Reference's HEAD path is "skip
  before Vary is set" (`compression/index.js:191-195` returns early
  before `vary(res, 'Accept-Encoding')` at line 175). So HEAD on a
  compressible asset should NOT carry `Vary`. Pin and mirror.
- **Slice 0 surprise — OPTIONS Vary.** Reference treats OPTIONS as a
  non-HEAD method, so compression negotiation runs and Vary is set.
  Pin and mirror.
- **Cascade audit blow-up.** The `content-encoding` track addition in
  slice 0 might surface that the reference's existing snapshots are
  missing `Content-Encoding` headers that they should have, because
  the pre-7e probe runner stripped them. Re-record cleanly. If 5+
  snapshots churn, surface — it's a methodology signal.
- **D-020 surface widening.** If review rounds keep landing on the
  same aspect (e.g. q-rank semantics, MIME edge cases, brotli quality),
  anti-hallucination rule #9 kicks in at round 3. Pause; declare
  parity scope; expand D-020 with explicit divergences; ask user.
- **brotli crate version drift.** The `brotli` Rust crate has had
  major-version jumps. Pin to a known-good version (e.g. `7.0` at the
  time of authoring — verify on crates.io) and document it. If the
  API surface differs significantly, swap to `async-compression`'s
  brotli wrapping or to `brotli2` (different crate name).
- **`Content-Length` framing.** Per D3, irserve sends `Content-Length`
  on compressed bodies; reference sends chunked. The probe runner's
  raw-socket path captures both. The snapshot body section is a
  hash + sample of decoded body, not the wire frame, so this SHOULD
  be transparent. Verify in slice 2 — if `runner.l0.clean` doesn't
  cleanly pass, the divergence is observable and needs a `*MayDiffer`
  overlay scoped to framing-headers (transfer-encoding,
  content-length).
- **Stage-7d composition.** OPTIONS now compresses. If the
  `cors-preflight` snapshot was captured without `Accept-Encoding`
  in the request, slice 2 doesn't break it. If it WAS captured with
  `Accept-Encoding`, the snapshot's body / headers change post-7e.
  Read the case file in slice 0 / slice 3 to check.
- **Anti-hallucination rule #9.** Three rounds on the same compression
  aspect triggers the pause-and-declare-parity-scope protocol. The
  most-likely repeat-offender aspects: q-value semantics, MIME-filter
  edge cases, brotli quality / output framing. Pre-empt by enumerating
  them all in the out-of-scope list above.

## Open methodological questions

(Items the user may want to weigh in on at slice boundaries; not
blocking the plan.)

- Should `compression: true|false` in `serve.json` be honored? Reference
  doesn't expose it (CLI-only). irserve could mirror reference (no
  config-file field) or extend with a `serve.json` field as an irserve
  enhancement. **Default**: no `serve.json` field. Surface if review
  asks.
- Should the threshold be tunable via `serve.json`
  (`compressionMinSize`)? Reference doesn't expose it. **Default**:
  hard-code 1024.
- Should `-u` accept a value (e.g. `-u false`) to override? Reference
  is boolean-flag. **Default**: boolean-flag mirror.

## Stage map cross-reference

- `README.md` line 79 — flip status from `todo` to `done`.
- `docs/stage7_l3_capabilities.md` table line 53 (the `7e` row) — flip
  the rightmost column from a description of "what will land" to a
  past-tense "DONE." block citing this stage's commits, mirroring how
  7a/7b/7c rows were rewritten when they closed. Final wording lands
  in the meta slice.
