# Design: Last-Modified + --no-etag + If-Modified-Since

This document records the architecture for Stage 7b. 7b is the
second L3 sub-stage and lands two SRVs (SRV-CACHE-002,
SRV-CACHE-003) plus one CLI SRV (SRV-CLI-013), closing Q-009 in
the process. The architectural foundations (crate layout,
HTTP-stack pins, request lifecycle, oracle harness layer) live
in `openspec/changes/001-port-minimal-static-server/design.md`;
the Stage-7a foundations (`build_file_or_304`, the headers
merge-before-decide ordering, the `Range`-absent guard, the
probe-runner `$fromResponse` capture-replay) live in
`openspec/changes/011-etag-conditional/design.md` and are
reused verbatim. The contract for the extended capability lives
in `openspec/specs/http-cache/spec.md`; the wire-level scenarios
specific to 7b are in `specs/http-cache/spec.md` of this change
package. 7b does not add a new phase to the 13-phase dispatcher;
the Last-Modified / IMS 304 decision sits inside the same
file-response slot (phase 12) that 7a's ETag / INM 304 path uses.

## §1. The ETag / Last-Modified mutex

The reference's source-of-truth is the gate at
`third_party/serve-handler/src/index.js:227-236`:

```js
if (etag) {
  // ... compute sha, set defaultHeaders['ETag']
} else {
  defaultHeaders['Last-Modified'] = stats.mtime.toUTCString();
}
```

Exactly one of the two headers is set on the default-header
object per file response, gated on `config.etag`. IrServe
mirrors this by routing both headers through opposite-gated
value helpers on the SAME predicate (`serve_config.etag ==
Some(false)`):

- `etag_value` at `crates/irserve-core/src/dispatch.rs:703-708`
  returns `None` iff `etag == Some(false)`, else
  `Some(compute_etag(...))`.
- `last_modified_value` at
  `crates/irserve-core/src/last_modified.rs:27-37` returns
  `None` unless `etag == Some(false)`, else (when metadata
  resolves) `Some(httpdate::fmt_http_date(mtime))`.

`build_file_or_304` at `dispatch.rs:763-765` calls both
helpers in sequence with the same `serve_config`, so exactly
one of `(etag, last_modified)` is `Some` per request — the
mutex.

`file_response` itself at `dispatch.rs:666-691` writes
whichever header value(s) the caller hands it. It does NOT
re-enforce the mutex. This matters: user `serve.json#headers`
rules (applied later by `apply_custom_headers`) can still
supplement either header on the response unchanged, exactly
mirroring the reference's `Object.assign(defaultHeaders,
related)` at `index.js:241` which runs unconditionally on top
of whichever default was written. A deployment under
`etag: false` with a user rule supplying `ETag: "custom"` ends
up with BOTH `ETag: "custom"` (from the user rule) AND
`Last-Modified: <mtime>` (from the default emission); a
deployment under default `etag` with a user rule supplying
`Last-Modified: "..."` ends up with both `ETag: "<sha1>"` AND
the user-supplied `Last-Modified`. The mutex lives at the
default-emission seam, not at the wire.

## §2. Where `Last-Modified` is computed

The `last_modified_value` helper at `last_modified.rs:27-37`
takes `(serve_config, meta: Option<&Metadata>)`. The
two-syscall pattern in the dispatcher
(`tokio::fs::metadata(&p).await.ok()` alongside
`tokio::fs::read(&p).await`) at the File/Index arm
(`dispatch.rs:346`) and renderSingle branch
(`dispatch.rs:480`) keeps the diff minimal vs. the 7a code
that read only the bytes. TOCTOU on mtime is immaterial at
whole-second IMF-fixdate resolution — the worst case is a
one-second mismatch that falls into the same 304-vs-200
decision as a strict-equal IMS would.

Format is RFC 7231 IMF-fixdate via `httpdate::fmt_http_date`
(workspace gains `httpdate = "1"`, pinned-stable 1.0.3). The
reference uses `stats.mtime.toUTCString()` — Node's
`Date.prototype.toUTCString` is documented to produce the same
IMF-fixdate shape. A unit test in `last_modified::tests::
formats_known_mtime_as_imf_fixdate` pins the wire string for a
known mtime (1609459200 unix seconds →
`"Fri, 01 Jan 2021 00:00:00 GMT"`), localising any future
upstream-crate shape change to one place.

`last_modified_value` returns `None` when:

- `serve_config.etag != Some(false)` — ETag-on path; mutex
  forces `None`.
- `meta` is `None` — the dispatcher could not stat the file;
  graceful degradation, the response still ships as 200 with
  no `Last-Modified` (the bytes succeeded reading; we simply
  don't have an mtime to advertise).
- `meta.modified()` fails — exotic filesystems without a
  usable mtime; same graceful path.
- the formatted string cannot be encoded as a `HeaderValue` —
  defensive; the formatter only produces ASCII IMF-fixdate so
  this should never fire in practice.

## §3. The IMS 304 short-circuit (D-018 adaptation)

This is the only deliberate divergence from the reference in
Stage 7b. The reference has **zero** IMS handling: grep across
`third_party/serve-handler/src/` and `third_party/serve/src/`
for `if-modified-since` / `ifModifiedSince` / `IfModifiedSince`
returns no hits. The 304 short-circuit at
`serve-handler/src/index.js:760-764` branches only on
`if-none-match`. Pinned empirically by slice 0 in
`tools/probe/snapshots/last-modified-roundtrip.json`: every IMS
variant under `target=reference` (exact match via
`$fromResponse`, far-future, epoch, malformed, on-404) returns
200 with the full body, status unchanged.

irserve adapts: under `etag: false`, a request whose
`If-Modified-Since` parses AND is `>=` the merged
`Last-Modified` short-circuits to 304 with no body, no
`Content-Type`, no `Last-Modified` echo — exactly the
ETag/INM 304 shape from Stage 7a. The decision is recorded as
**D-018**. The rationale:

1. **Anti-hallucination rule #5** explicitly permits adaptation
   over bug-for-bug parity for MVP. Bug-for-bug 200-on-IMS
   would force every client that relies on conditional GETs
   (browsers, CDNs) to re-download every byte; that is wire-
   observable user-facing pessimization with no upside.
2. **SRV-CACHE-003 was `unknown` until slice 0** — there was
   no prior contract to break. Once empirically pinned, the
   reference behavior became `verified` (200 on IMS), and
   irserve's `adapted` status against the same SRV is the
   correct status-taxonomy outcome.
3. **Symmetry with the ETag/INM 304 path** from Stage 7a. The
   merge-before-decide ordering, the `Range`-absent guard, the
   no-body / no-`Content-Type` / no-validator-echo response
   shape all carry over verbatim. The IMS branch sits as a
   sibling of the INM branch in `build_file_or_304` at
   `dispatch.rs:776-792` and reuses the new
   `not_modified_response()` helper at `dispatch.rs:797-802`.
4. **User chose the adaptation** in the plan-mode
   AskUserQuestion this session (D1, "Адаптировать: 304 на
   match"). Recorded as the canonical answer; D-018 captures
   the framing.

The comparison is on the MERGED Last-Modified (after
`apply_custom_headers`), not on the raw mtime. This keeps the
path symmetric with ETag/INM: a user `serve.json#headers` rule
that overrides `Last-Modified` drives the decision (replay of
the override → 304; an IMS predating the override → 200), and
a rule with `value: null` deletes `Last-Modified` from the
merged response → the IMS branch never fires. Two of the seven
new unit tests pin this:
`user_last_modified_null_rule_suppresses_304` and
`user_last_modified_override_drives_304_decision`.

Parsing uses `httpdate::parse_http_date` on both sides
(request IMS and merged response LM). Comparison is on the
returned `SystemTime` values with `>=`. Whole-second
resolution naturally falls out of the formatter/parser
round-trip — `fmt_http_date` writes whole-second IMF-fixdate,
`parse_http_date` reads it back as `SystemTime` at that
whole-second. Malformed IMS or malformed LM short-circuits
the branch to "no 304" (falls through to the merged 200),
mirroring RFC 9111 §13.1.3's recipient guidance that
unparseable IMS SHOULD be ignored. The `Range`-absent guard
at `dispatch.rs:767` wraps BOTH the INM and IMS branches —
when `Range` is present, neither short-circuit fires (Stage
7c precursor, mirrors `index.js:760`).

## §4. The `--no-etag` CLI flag

`crates/irserve/src/main.rs:82-83` adds
`#[arg(long = "no-etag")] no_etag: bool` to `Cli`. clap
derives the long-only form; no short alias. Mirrors
`third_party/serve/source/utilities/cli.ts:155`'s
`'--no-etag': Boolean,` exactly.

Post-parse override at `main.rs:144-146`:

```rust
if cli.no_etag {
    serve_config.etag = Some(false);
}
```

Runs AFTER the `serve.json` merge. Reference's
`config.ts:140` writes `config.etag = !args['--no-etag']`
unconditionally — the flag's absence force-sets `etag = true`,
overriding whatever `serve.json` said. IrServe takes the
weaker form: when the flag is set, force `etag = false`; when
unset, leave whatever `serve.json#etag` provided in place
(default ETag-on when absent). This preserves the Stage-7a-
established `serve.json#etag: false` contract (a deployment
that opts out via config keeps its opt-out without needing
the CLI flag) at the cost of a small divergence vs. reference
on the rare combo of `serve.json#etag: false` + no CLI flag
(reference would force ETag-on; irserve honors the config).
Documented as a Compatibility note in
`specs/http-cache/spec.md`. The user-observable behavior
matches the documented per-deployment intent — the deployment
asked for `etag: false`, irserve respects it.

No dedicated unit test for the flag — slice 1's commit message
notes the threading is a single boolean override, and
behavioral coverage lands via slices 2 and 3 (Last-Modified
emission and IMS 304). The planned `no-etag-flag.json` smoke
probe was dropped (redundant with
`last-modified-roundtrip.json#first_get` which runs under
`serveArgs: ["--no-etag"]`).

## §5. Probe runner: partition + day-of-week bug

`tools/probe/cases/last-modified-roundtrip.json` carries 6
requests under `serveArgs: ["--no-etag"]`:

- `first_get` — captures `Last-Modified` via the standard
  response capture mechanism.
- `ims_exact` — replays `first_get`'s `last-modified` as
  `if-modified-since` via the Stage-7a `$fromResponse`
  extension.
- `ims_future`, `ims_past`, `ims_malformed` — static IMS
  values (far-future, epoch, garbage).
- `ims_on_404` — IMS against a missing file.

The `runner.l0` partition splits the case three ways:

- `clean: [first_get, ims_past, ims_malformed, ims_on_404]`
  — both targets agree on the response shape (200 / 200 /
  200 / 404). Promoted to dual-target after slice 3.
- `divergent: [ims_exact, ims_future]` — irserve returns
  304, reference returns 200 (D-018 divergence). Recorded
  under `target=reference` only; irserve coverage lives in
  `dispatch::tests::ims_exact_match_returns_304` and
  `ims_future_returns_304`.
- `bodyMayDiffer: [ims_on_404]` — both sides emit 404 but
  the synthetic HTML body content is not contractual (D-002 —
  terminal output / exact HTML excluded from oracle).

**Methodological signal — the day-of-week bug.** Slice 0
authored `ims_future`'s IMS as `"Fri, 01 Jan 2099 00:00:00 GMT"`.
2099-01-01 is actually a Thursday. Under `target=reference` the
mismatch was inert (reference does not parse IMS at all, so
the day-of-week could be anything). The bug surfaced only in
slice 3 when the irserve unit test mirroring the same date hit
`httpdate::parse_http_date`'s strict RFC 7231 day-of-week check
and got `Err` → fell through to 200 instead of the expected
304. Fix: `Fri` → `Thu` in both `ims_future` and `ims_on_404`
in the probe case, plus the matching unit test. Snapshot
re-recorded once (only the `if-modified-since` request-header
string changed; the response stays 200 since the reference is
IMS-inert). **Lesson:** when authoring static HTTP-date literals
in tests/probes, double-check the day-of-week against the
calendar. A reference whose parser is lax (or absent) cannot
catch the mistake; only a strict parser on the irserve side
will.

## §6. Methodological signals

This stage authored **one new D-NNN entry** (D-018) and no
existing-D edits. D-017's "round-trip-is-the-contract"
framing carries forward unchanged.

**D-018** captures three things: (1) the IMS-304 adaptation,
anchored in anti-hallucination rule #5 and the SRV-CACHE-003
status-taxonomy framing; (2) the symmetry with the ETag/INM
304 path from Stage 7a; (3) the user's explicit answer to the
plan-mode AskUserQuestion (D1, Recommended option).

**Plan-mode AskUserQuestion answers** for the session:

- **D1 — IMS 304 short-circuit.** Adopted: irserve adapts.
  Anchors D-018.
- **D2 — Probe sequencing.** Slice 0 is a dedicated
  reference-probe commit closing Q-009 before any Rust code
  is written. Anti-hallucination rule #8 in action.
- **D3 — CLI flag form.** `--no-etag` long-only, no short
  alias, mirrors reference exactly.

**Q-009 closure via probe-first** (anti-hallucination rule
#8). The plan had a strong prior — three points of source-
level evidence pointed to "reference is IMS-inert" — but the
SRV-CACHE-003 row stayed `unknown` until the probes ran. Once
the snapshot pinned the 200-with-full-body shape, the status
flipped to `verified` and the irserve adaptation could be
authored against a known contract instead of an assumption.

## §7. Verification

Slice-by-slice green at each commit; full verification at
end-of-stage (slice 3 commit):

- `cargo test --workspace --lib` — **278/278** unit tests
  green (was 271 after slice 2; +7 from slice 3's IMS
  branch). New tests across stages: 4 (`last_modified.rs` —
  shape pin + mutex direction 1 + direction 2 + meta-absent
  graceful path) + 2 (`dispatch::tests` slice 2 — LM emission
  surface + ETag-on suppression of LM) + 7 (`dispatch::tests`
  slice 3 — IMS-exact-match → 304, IMS-future → 304, IMS-past
  → 200, IMS-malformed → 200, Range present + IMS-match →
  200, user `Last-Modified: null` suppresses 304, user
  `Last-Modified` override drives the 304 decision) = 13 new
  tests across slices 2–3.
- `cargo test --test oracle` — **82 total, 76 passed, 6
  skipped, 0 failed** (was 75 / 7 after slice 1; the four
  `last-modified-roundtrip` clean-partition requests promoted
  to dual-target).
- `node tools/probe/run.mjs last-modified-roundtrip
  --target=reference --snapshot=verify` — green (snapshot
  pinned in slice 0, day-of-week fix applied in slice 3).
- `node tools/probe/run.mjs last-modified-roundtrip
  --target=irserve --snapshot=verify` — green with the L0
  partition applied (clean requests round-trip; divergent
  requests are recorded under `target=reference` only).
- `cargo run -- --help` — lists `--no-etag` with the
  SRV-CLI-013 doc comment.
- `npx -y @fission-ai/openspec@latest validate --all --strict`
  — slice 4 final step (this change package).
