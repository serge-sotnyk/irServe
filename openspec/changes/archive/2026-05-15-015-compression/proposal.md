# Proposal: HTTP compression (`-u` / `--no-compression`)

## Why

Stage 7e is the fifth and final sub-stage of Stage 7 (L3
polish) per `docs/stage7_l3_capabilities.md`, the last
`todo` row in the README stage map, and the closure of the
L3 compression surface left unaddressed since Stage 1.
Until 7e, irserve recognised `-u` / `--no-compression` only
as a clap-accepted no-op per **D-006** (HTTP compression
is L3-priority, not MVP-mandatory) — responses were always
sent uncompressed regardless of the flag and the
`Vary: Accept-Encoding` invariant was not emitted on
compressible MIMEs.

7e is also the **biggest methodological risk** in Stage 7
per anti-hallucination rule #8 (mirroring a third-party
library — Node's `compression@1.8.1` middleware — whose
defaults are not all visible from source). Mitigation lived
in the slice plan at
`docs/features/0019_PLAN_stage7e_compression.md`:
probe-first slice 0 reverse-engineering against the
reference, mandatory pre-stage out-of-scope list (rule
#10), early architectural fork-resolution in plan-mode
(D1 — encoder set: brotli + gzip + deflate full parity;
D2 — MIME filter: curated allowlist + regex fallback, not
the full `mime-db` `compressible` table; D3 — known
divergences pre-sketched as the future `D-020` entry).

This change closes:

- **SRV-CLI-012** (P2, status: `verified`, level: L3) —
  the `-u` / `--no-compression` flag is now functional.
  Default is compression-on; under default `Accept-Encoding`
  on a compressible MIME above the 1024-byte threshold the
  response carries `Content-Encoding: <br | gzip | deflate>`
  with the body compressed and `Vary: Accept-Encoding`
  set. Setting `-u` / `--no-compression` disables the
  feature wholesale — no `Vary` emitted, body uncompressed,
  exactly the pre-7e behavior. Mirrors reference's
  `if (!args['--no-compression']) await compress(...)` at
  `third_party/serve/source/utilities/server.ts:71-72` and
  the underlying `compression@1.8.1` middleware surface
  pinned in slice 0 via 20 raw-socket anchors.

- **Q-002** (status: `closed` 2026-05-14) — compressed
  content-type set + minimum body-size threshold. Resolved
  empirically by slice 0's `tools/probe/cases/compression-raw.json`
  + `tools/probe/snapshots/compression-raw.json`. The set
  is the curated allowlist (`text/*` prefix,
  `application/json`, `application/javascript`,
  `application/wasm`, `image/svg+xml`) plus a regex
  fallback `^text/|\+(?:json|text|xml)$` (case-insensitive).
  The threshold is 1024 bytes — the
  `compression@1.8.1` default, never overridden by `serve`.

- **D-006** transitions from `adapted` (100 % deferred —
  the flag was wired but no-op) to **implemented**. The
  D-006 historical entry stays in
  `docs/reference/serve/decisions.md` as the audit record
  of when the deferral was in force; its rationale
  ("`compression` middleware adds dependencies not
  justified for an L0–L2 MVP") is now superseded by Stage
  7e closing the L3 stretch goal.

7e **does add `D-020`** (`docs/reference/serve/decisions.md`)
to record the four deliberate divergences from the
reference's wire surface that survive into the irserve
implementation — see "What" below and the canonical
`D-020` entry. The previous high-water mark was `D-019`
(Stage 7c multi-range parity scope); next free slot was
`D-020`.

## What

- **Slice 0 — probe-first reverse-engineering against
  reference (`9bd52f0`).** Extended
  `tools/probe/run.mjs::TRACKED_RESPONSE_HEADERS` with
  `'content-encoding'` so the on-the-wire encoding is
  observable on raw-socket probes (Node's `fetch`
  transparently decompresses bodies, hiding
  `Content-Encoding` from `fetch`-mode probes). Authored
  `tools/probe/cases/compression-raw.json` — 20 raw-socket
  anchors spanning the MIME allowlist (compressible:
  `tiny.html`, `big.html`, `big.css`, `big.js`,
  `data.json`, `vector.svg`, `bin.wasm`; non-compressible:
  `image.png`, `font.woff2`, `media.mp4`), the negotiation
  matrix (`gzip, deflate, br`, `gzip`, `deflate`,
  `identity`, no `Accept-Encoding`, `gzip;q=0, br`,
  `*;q=0`), the skip conditions (HEAD, OPTIONS,
  `Cache-Control: no-transform`, `Range: bytes=0-15`).
  Captured the reference snapshot via
  `node tools/probe/run.mjs compression-raw
  --target=reference --snapshot=update`. Re-recorded 21
  legacy reference snapshots that the silently-stripped
  `content-encoding` header was newly visible on. Closed
  Q-002 in `docs/reference/serve/open-questions.md` with
  the empirical findings cited above.

- **Slice 1 — probe-runner extension audit (collapsed).**
  Cascade audit confirmed no other case file broke under
  the new `content-encoding` tracker; the 21 legacy
  re-records landed inside slice 0. No separate commit.

- **Slice 2 — Rust implementation (`d1b9a15`).** New
  module `crates/irserve-core/src/compression.rs`:
  `negotiate` (parses `Accept-Encoding` tokens with `q=0`
  exclusion and `*` wildcard accept/reject; preference
  order `br > gzip > deflate`), `is_compressible`
  (curated allowlist + regex fallback per D2),
  `maybe_apply` (the dispatcher seam), and `encode` (thin
  wrapper over `flate2::write::GzEncoder` /
  `flate2::write::DeflateEncoder` / `brotli::CompressorWriter`).
  Added `flate2 = "1"` and `brotli = "8"` to
  `crates/irserve-core/Cargo.toml`. Added
  `pub compression: Option<bool>` to `ServeConfig` in
  `crates/irserve-core/src/config.rs` with
  `#[serde(skip)]` — flag-only, not exposed via
  `serve.json`. Wired `-u` / `--no-compression` in
  `crates/irserve/src/main.rs` mirroring the `--no-etag`
  shape; post-parse override forces
  `serve_config.compression = Some(false)` when the flag
  is set. Threaded `req.method()` through the dispatcher
  call chain. Dispatcher integration in
  `crates/irserve-core/src/dispatch.rs::build_file_or_304`:
  `compression::maybe_apply` slots AFTER
  `apply_custom_headers` (so merged `Content-Type` and
  `Cache-Control` are visible to the negotiator) and
  BEFORE Range handling (so an uncompressed bytes vec is
  still available for slicing — Range pre-empts
  compression entirely). Probe runner: dropped the slice-0
  temporary `content-encoding` mask from
  `L0_EXTRA_VOLATILE_HEADERS`; extended `bodyMayDiffer` to
  also strip `content-encoding` (the compression decision
  is a function of body length vs threshold, so a
  body-may-differ anchor implies an encoding-may-differ
  one). Added `runner.l0.clean` blocks to
  `compression-default.json` (legacy 1-anchor) and
  `compression-raw.json` (new, with `bodyMayDiffer` on the
  10 compressed anchors per D-020's body-bytes
  divergence). Promoted **ORC-058** in
  `docs/reference/serve/oracle-matrix.md` from
  reference-only to dual-target and added 20 new ORC rows
  for the compression-raw anchors (ORC-191..ORC-210).
  (Current-state counts after Codex rounds 3 and 4:
  23 anchors, 12 in `bodyMayDiffer`, ORC-191..ORC-213
  — see D-020 / capability spec / inventory.)
  `cargo test --test oracle` ends at **81 passed, 2
  skipped, 0 failed** (was 79 / 3 / 0 at end of Stage 7d).

- **Slice 3 — audit and stabilize (collapsed).**
  Cross-cutting audit of the post-slice-2 oracle state
  confirmed no leftover divergence: the legacy
  `etag-roundtrip.json` / `last-modified-roundtrip.json` /
  `range-request.json` snapshots were re-recorded in
  slice 0 alongside the other 21, so the post-slice-2
  irserve `Content-Encoding` emission lined up
  dual-target without further changes. No separate
  commit.

- **Slice 4 — documentation (this change package + main
  agent).** New change package
  `openspec/changes/archive/2026-05-15-015-compression/` with this proposal
  + design + tasks + a MODIFIED delta on the `cli`
  Requirement set (adds the SRV-CLI-012 Requirement) + an
  ADDED capability spec `http-compression` covering the
  full mechanics (encoder set, threshold, MIME filter,
  Vary semantics, skip conditions, and the 206 /
  threshold-gate composition with Stage 7c — small
  ranges pass through uncompressed with Vary; large
  ranges encode like 200s, per the Codex round 4 P2
  refinement) and the D-020 divergences. Appends **D-020** to
  `docs/reference/serve/decisions.md`. Updates
  `docs/reference/serve/inventory.md` SRV-CLI-012 entry
  (closes Q-002, flips the "MAY ship without
  compression" hedge to past tense, cites D-020).
  README's stage-7e row flips to `done`; the "What is
  NOT yet observable" footer drops `gzip compression`.
  `docs/stage7_l3_capabilities.md` 7e row rewrites to
  past-tense DONE with slice citations.

## Out of scope

These are the divergences enumerated as `D-020` (the
deliberate ones — every other behavior mirrors reference):

1. **`Content-Length` framing on compressed responses.**
   irserve sends `Content-Length: <compressed-len>` with
   no `Transfer-Encoding: chunked`. Reference removes
   `Content-Length` and uses chunked transfer-encoding
   because its `compression` middleware is stream-based
   and doesn't know the final length at header-write
   time. irserve has the final compressed bytes in memory
   (static-file model, not streaming) and can declare
   `Content-Length` cleanly. Wire-observable; bodies
   inside the framing carry the same compressed content
   for a given encoder + same input.

2. **No `mime-db` `compressible` table port.** irserve
   uses a curated allowlist of common types (`text/*`
   prefix match, `application/json`,
   `application/javascript`, `application/wasm`,
   `image/svg+xml`) plus the regex fallback
   `^text/|\+(?:json|text|xml)$/i` per
   `compressible/index.js:23`. The reference's
   `compressible@2.0.18` package consults
   `mime-db@1.33.0`'s `compressible: true` flag for
   ~150 MIMEs that the curated allowlist + regex do not
   enumerate (e.g. `application/postscript`,
   `application/xml-dtd`). Any future divergence on a
   long-tail MIME is a separate `D-NNN` opportunity but
   has no current production-traffic surface.

3. **q-rank within `(0, 1)` not honored.** irserve
   treats `q=0` as exclusion and any non-zero / absent
   `q` as accept; values strictly between 0 and 1 are
   NOT ranked. The reference's `Negotiator` package
   honors fractional q-values to pick the most-preferred
   encoder. Real-world `Accept-Encoding` values seen on
   the wire are all-equal-priority (e.g.
   `gzip, deflate, br`), so this is a corner case
   without observable production drift.

4. **Compressed body bytes are NOT byte-identical to
   reference's.** Both sides produce valid encodings of
   the same logical body, but encoder defaults differ at
   the bit level — Node `zlib` uses
   `Z_DEFAULT_COMPRESSION` (= 6) for gzip / deflate,
   matching `flate2`'s default; Node brotli's stream
   defaults (quality 4 / window 22 via the
   `compression` middleware) and the `brotli` crate's
   defaults are nominally the same numbers but
   implementation variations may still differ at the
   bit level. The probe runner's per-anchor
   `bodyMayDiffer` overlay strips body bytes +
   `content-length` from the L0 contract on the 10
   compressed anchors in `compression-raw.json`.
   `content-encoding` is NOT stripped by
   `bodyMayDiffer` — only by the explicit per-anchor
   `contentEncodingMayDiffer` partition, which the 10
   compressed anchors do NOT opt into (Codex round 1
   P1 separated the overlays: encoder CHOICE is
   contractual, encoder OUTPUT is not).

Additionally — explicitly out of scope for 7e (not new
divergences, already established):

5. **`Vary: Accept-Encoding` IS appended (Codex round 1
   P2 fix).** Initial Stage 7e implementation only set
   `Vary` when no `Vary` header was present, which is
   cache-incorrect when a user `headers` rule already
   set e.g. `Vary: Cookie` (downstream caches would
   key only on Cookie and serve a brotli body to
   identity clients). Round 1 P2 implements full
   append semantics in `append_vary_accept_encoding`:
   existing `Vary: <field>` becomes
   `Vary: <field>, Accept-Encoding`; wildcard `*` is
   left alone; case-insensitive deduplication. No
   longer a divergence.

6. **`HEAD` body suppression.** Neither side suppresses
   the HEAD body at the dispatcher; the HTTP layer
   (axum / hyper on irserve, Node http on reference)
   strips it on the wire. Compression-side: HEAD enters
   `maybe_apply`, gets `Vary` set, and short-circuits
   before encode (mirroring
   `compression/index.js:192-195`). Out of scope for 7e
   as a stage in its own right; inherited from the
   existing Stage 7d HEAD aside.

7. **Streaming / backpressure.** Reference's
   `stream.on('drain')` semantics at
   `compression/index.js:140-152` are not mirrored.
   irserve buffers all bytes in memory — the
   static-file model.

8. **Already-encoded passthrough** for upstream
   `Content-Encoding` (reference's
   `compression/index.js:182-188` skip). Codex
   round 2 P2 implemented in `maybe_apply`, refined
   by round 3 P2 — when the merged response (post
   `apply_custom_headers`) already carries a
   `Content-Encoding` header AND the value is NOT the
   literal `identity`, the centralized compression
   pass returns with `Vary` set but does NOT re-encode
   the body. Reference reads
   `encoding = res.getHeader('Content-Encoding') || 'identity';
   if (encoding !== 'identity') skip`; irserve
   mirrors. `Content-Encoding: identity` is treated as
   no encoding applied and falls through to the
   encode step. No longer a divergence.

9. **Per-request encoder enforcement** via
   `compression()`'s `enforceEncoding` option. The
   reference's `serve` CLI doesn't override the default
   `identity`, so this branch is dead in practice and
   not mirrored.

10. **Brotli compression-quality tuning.** Both sides
    use crate defaults. Body bytes diverge per D-020 #4
    above; quality matching is not contractual.

## Risks

- **Anti-hallucination rule #9 (declared parity scope).**
  The four `D-020` divergences are the declared scope
  boundaries for 7e. Subsequent review rounds landing
  on these aspects do NOT trigger rule #9 (they are
  already documented divergences); review rounds landing
  on a new aspect (e.g. MIME edge case not enumerated)
  would. Pre-stage out-of-scope list (per rule #10) is
  baked into both this proposal and the canonical
  `http-compression` capability spec's Compatibility
  notes.

- **Body-bytes drift.** The probe runner's per-anchor
  `bodyMayDiffer` overlay strips body bytes +
  `content-length` from the L0 contract on the 10
  compressed anchors; `content-encoding` stays
  must-match. If a future encoder upgrade changes the
  byte output (e.g. `flate2` major version bump), the
  L0 contract stays green and the divergence stays
  scoped; only an empirical re-record is needed. If
  the same encoder upgrade somehow changed the chosen
  encoder name (extremely unlikely), the
  `content-encoding` must-match would surface that.

- **Stage 7d composition.** OPTIONS now compresses (the
  `options_big_html_gzip` anchor in `compression-raw.json`
  carries `Content-Encoding: br`). The Stage 7d
  `cors-preflight.json` snapshot was captured WITHOUT
  `Accept-Encoding` so its 7d-pinned shape (200 + file
  body + four CORS headers) is unchanged.

- **Encoder defaults drift between Node and Rust.**
  `D-020` #4 documents this as not contractual.
  Future-proofed against `flate2` / `brotli` crate
  updates and Node `zlib` updates without re-litigation.
