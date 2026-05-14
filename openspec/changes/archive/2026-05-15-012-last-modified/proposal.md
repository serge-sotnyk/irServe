# Proposal: Last-Modified + --no-etag + If-Modified-Since

## Why

Stage 7b is the second sub-stage of Stage 7 (L3 polish) per
`docs/stage7_l3_capabilities.md` and the next "Next" row in
the README stage map. It builds directly on Stage 7a
(`openspec/changes/archive/2026-05-15-011-etag-conditional/`), which established
the `etag` module, the `build_file_or_304` short-circuit, and
the probe-runner `$fromResponse` capture-replay extension. 7b
extends the same response-emission slot — the file-response
arms in `crates/irserve-core/src/dispatch.rs` — with the
mutually exclusive `Last-Modified` branch the reference takes
under `etag: false`, plus the `If-Modified-Since` conditional
short-circuit irserve adapts on top of that branch.

This change closes:

- **SRV-CLI-013** (P1, `accepted` → `verified` via ORC-167) — `--no-etag`
  CLI flag, long-only, no short alias. Mirrors
  `third_party/serve/source/utilities/cli.ts:155`
  (`--no-etag: Boolean`) and the post-parse mapping at
  `source/utilities/config.ts:140`
  (`config.etag = !args['--no-etag']`).
- **SRV-CACHE-002** (P1, `accepted` → `verified` via ORC-167) — under
  `etag: false` (the CLI flag or `serve.json#etag: false`),
  emit `Last-Modified` (RFC 7231 IMF-fixdate, UTC) in place of
  `ETag`. Mirrors the `else` branch of
  `third_party/serve-handler/src/index.js:227-236`'s mutex.
- **SRV-CACHE-003** (P1, `unknown` → `verified`) — `If-Modified-Since`
  handling on file responses. Reference behavior pinned by
  slice-0 probes; irserve adapts to short-circuit 304 (D-018).
- **Q-009** (open → closed in slice 0) — IMS behavior under
  `--no-etag`. Resolved empirically before any Rust code was
  written, per anti-hallucination rule #8 (empirical-before-
  implement when mirroring a third-party library). Result:
  reference has zero IMS branches across `serve-handler/src/`
  and `serve/src/`; every IMS variant (exact-match, future,
  epoch, malformed) returns 200 with the full body. Pinned by
  `tools/probe/snapshots/last-modified-roundtrip.json`.

The structural foundations laid in 7a (merge-before-decide
ordering against the user `headers` overlay; the `Range`-absent
guard as a 7c precursor; the round-trip-is-the-contract framing
of D-017) propagate forward unchanged. No D-017 edits are
required.

## What

- **`--no-etag` CLI flag (slice 1).** New `no_etag: bool` field
  on `Cli` at `crates/irserve/src/main.rs:82-83`
  (`#[arg(long = "no-etag")]`, long-only). Post-parse override
  at `main.rs:144-146` forces `serve_config.etag = Some(false)`
  when set; an unset flag honors whatever `serve.json#etag`
  provided (default ETag-on). Diverges slightly from the
  reference's `force-true-without-flag` shape — see
  Compatibility note in `specs/http-cache/spec.md`.

- **`last_modified` module + emission on 200 (slice 2).** New
  `crates/irserve-core/src/last_modified.rs::last_modified_value`
  returns `Some(HeaderValue)` only when
  `serve_config.etag == Some(false)` AND `meta.modified()`
  succeeds; otherwise `None`. The mutex with ETag is enforced
  upstream in the two value helpers — `etag_value` and
  `last_modified_value` return opposite-gated `Some` on the
  same `serve_config.etag` predicate — so exactly one of the
  two ever reaches `file_response`. Format is RFC 7231
  IMF-fixdate via `httpdate::fmt_http_date` (workspace gains
  `httpdate = "1"`). `file_response` at `dispatch.rs:666-691`
  gains a `last_modified: Option<HeaderValue>` parameter and
  writes both headers independently so user `headers` rules
  applied later by `apply_custom_headers` can override or
  supplement either. `build_file_or_304` at `dispatch.rs:758`
  gains a `meta: Option<&Metadata>` parameter; both call sites
  (File/Index arm at `dispatch.rs:346` and renderSingle branch
  at `dispatch.rs:480`) now `tokio::fs::metadata(&p).await.ok()`
  alongside the bytes read. A metadata failure degrades
  gracefully to "no Last-Modified"; a read failure still
  surfaces as 404.

- **IMS 304 short-circuit (slice 3, D-018).** A second
  conditional branch in `build_file_or_304` at
  `dispatch.rs:780-809`, sibling to the existing ETag/INM
  branch. Parses both `If-Modified-Since` and the MERGED
  `Last-Modified` via `httpdate::parse_http_date`; on
  `ims >= lm` (and `Range`-absent), returns 304 via a new
  shared `not_modified_response()` helper at `dispatch.rs:814`.
  Malformed IMS or malformed LM falls through to 200 (RFC 9111
  §13.1.3). Symmetric with the ETag path: reads the MERGED
  Last-Modified after `apply_custom_headers`, so a user
  `serve.json#headers` rule overriding or deleting
  `Last-Modified` drives the decision (override → 304 on
  replay of the override value; `null` delete → no 304
  possible). Seven new unit tests in `dispatch::tests` pin
  the IMS surface; the ETag tests from 7a were updated to
  pass `None` for the new `meta` argument (they exercise the
  ETag-on path and don't need a real mtime). This branch is
  an irserve adaptation — the reference returns 200 here.
  Decision recorded as **D-018**.

- **Probes + Q-009 closure (slice 0).** New
  `tools/probe/cases/last-modified-roundtrip.json` (6 requests
  under `serveArgs: ["--no-etag"]`) covers `first_get`,
  `ims_exact` (via `$fromResponse` capture-replay of the
  first response's `last-modified`), `ims_future`, `ims_past`,
  `ims_malformed`, `ims_on_404`. Snapshot recorded under
  `target=reference` pins the 200-with-full-body shape on
  every IMS variant — empirically closing Q-009. The
  `runner.l0` partition splits the case three ways:
  `clean: [first_get, ims_past, ims_malformed, ims_on_404]`
  (irserve and reference agree); `divergent: [ims_exact,
  ims_future]` (irserve 304, reference 200 — D-018 divergence,
  reference-only coverage); `bodyMayDiffer: [ims_on_404]`
  (both sides emit 404 but the synthetic HTML body content is
  not contractual — D-002). Six new ORC rows (ORC-167..ORC-172)
  record the surface; ORC-167/170/171/172 are dual-target after
  slice 3 (clean partition), ORC-168/169 stay reference-only
  (divergent partition — irserve diverges to 304 per D-018) with
  cross-linked irserve coverage via `dispatch::tests::ims_*`.

- **Documentation updates (slice 4, this change package +
  main agent).** New change package
  `openspec/changes/archive/2026-05-15-012-last-modified/` with this proposal +
  design + tasks + an ADDED Requirement on http-cache + a
  MODIFIED note on the Stage 7a Requirement. **D-018** in
  `docs/reference/serve/decisions.md` (added by the main
  agent in slice 4, NOT here). README's stage-7b row flipped
  to `done`. The "Try IrServe" curl-based demo gains a
  `--no-etag` + Last-Modified + IMS-304 round-trip. Inventory
  flip for SRV-CACHE-003 (`unknown` → `verified`) landed in
  slice 0; SRV-CACHE-002 and SRV-CLI-013 were promoted
  `accepted` → `verified` in round-3 Codex fixes once ORC-167
  was identified as exercising both end-to-end (under
  `serveArgs: ["--no-etag"]`, Last-Modified emitted, no ETag).
  Per README §Anti-hallucination rules #2, an SRV transitions
  to `verified` when an Oracle test demonstrates the behavior;
  ORC-167 is that test.

## Out of scope

Mirrors `docs/features/0016_PLAN_stage7b_last_modified.md`'s
pre-stage out-of-scope list (anti-hallucination rule #10):

1. **`If-Unmodified-Since`.** RFC 7232 §3.4 conditional
   request header. Reference does not handle it; irserve does
   not either. No P-class SRV; out of scope for L3.

2. **`Vary: Accept-Encoding` / `Vary` on `Last-Modified`.**
   Compression is Stage 7e territory. Not surfaced here.

3. **Weak `Last-Modified` comparison semantics.** RFC 7232
   §2.2.2 distinguishes strong/weak validators. irserve does
   whole-second comparison only (the IMF-fixdate wire format's
   natural resolution); no weak-comparison logic, no special
   handling of cache-busting clients.

4. **Sub-second mtime precision.** Already documented as a
   Compatibility note on SRV-CACHE-002. IMF-fixdate wire
   format carries seconds; sub-second drift is not preserved.
   Round-trip via `httpdate::fmt_http_date` then
   `parse_http_date` is exact at whole-second granularity.

5. **`Last-Modified` on directory listings, 3xx redirects,
   JSON error responses, custom HTML error pages.** Same
   surface as Stage 7a's ETag deferrals — none of these
   branches emit `Last-Modified` either. `Last-Modified` is a
   file-response header; the listing / redirect / error arms
   are structurally separate from the `build_file_or_304`
   path. The custom-HTML-error-page divergence from Stage 7a
   (reference applies ETag/LM there via `findRelated`;
   irserve does not) carries forward unchanged.

6. **`Last-Modified` cache between requests.** Reference
   maintains a `Map<absPath, [mtime, sha]>` for the ETag path
   (`index.js:22`, consulted at `:228-231`); for `Last-Modified`
   no cache is needed (mtime fetch is one syscall via
   `tokio::fs::metadata`). The two-syscall pattern
   (`metadata` + `read`) is the smallest diff vs. the
   existing 7a code and TOCTOU on mtime is immaterial at
   whole-second IMF-fixdate resolution. No optimization here;
   captured in D-017's "implementation-defined, round-trip is
   the contract" framing.

7. **`Range` + IMS interaction beyond the guard.** RFC 7233
   §3.3 says a 304 takes precedence over 206 when the cache
   validator matches; for now the `Range`-absent guard at
   `dispatch.rs:771` suppresses both the ETag/INM and the
   Last-Modified/IMS 304 short-circuits — same shape as 7a.
   Stage 7c may revisit when Range parsing lands.

8. **Multiple IMS values / IMS list parsing.** RFC 7232 §3.3
   says one value only. irserve reads the first header value
   verbatim; malformed-style values (including comma lists)
   fall through to 200 via `httpdate::parse_http_date`'s
   strict reject path.

9. **`Date` response header.** Outside Stage 7 scope per the
   roadmap. axum may or may not emit it; not contractual.

10. **Symlink mtime semantics.** irserve uses
    `tokio::fs::metadata()` (follows symlinks); reference uses
    `lstat` (does not). L4 / Q-011 territory; stays deferred.
