# Stage 7d — `Cache-Control` default + `OPTIONS` (CORS preflight)

## Context

Stage 7d is the next "todo" row in the README stage map and the
fourth L3 sub-stage per [`docs/stage7_l3_capabilities.md`](../../../repos/sotnyk/irServe/docs/stage7_l3_capabilities.md).
It closes two loose ends from the L3 surface:

- **SRV-CACHE-005** (P2, level L3) — verification-only — the contract is
  "no default `Cache-Control`; the header appears verbatim only when a
  user `headers` rule sets it". Empirically irserve already behaves
  this way (grep across `crates/` returns zero `cache-control` /
  `Cache-Control` references; the only entry point is the user
  `headers` rule via `apply_custom_headers`, Stage 6f).
- **SRV-CORS-001** (P1, level L3) — `--cors` four-header surface +
  OPTIONS preflight semantics. The four-header surface is already
  live since Stage 6h (`apply_cors`, post-dispatch). The remaining
  hole is OPTIONS handling: irserve today returns **405** for OPTIONS
  (`dispatch.rs:94` rejects everything outside GET/HEAD); the
  reference does **not** check method at all and routes OPTIONS
  through the full static-file pipeline like a GET. Reference
  snapshot (`tools/probe/snapshots/cors-preflight.json`) pins
  status=200 + 7-byte body + ETag + content-type + the four CORS
  headers.

ChangeID: `openspec/changes/014-cache-headers-and-preflight/`.

Output: dual-target verification across ORC-047..052 (Cache-Control)
and ORC-056 (preflight) in [`docs/reference/serve/oracle-matrix.md`](../../../repos/sotnyk/irServe/docs/reference/serve/oracle-matrix.md);
spec deltas in the change package; README stage row flip to `done`.

## Architectural decision — OPTIONS routing

**Mirror reference (route OPTIONS as GET).** Decided in plan-mode
(user pick). This means:

- No `D-NNN` entry for 7d (mirroring → no divergence to record).
  Same shape as 7c which also added no D-NNN.
- The OPTIONS request walks phases 3..13 untouched; ETag, Range, user
  `headers` rules, and `apply_cors` all reach the response naturally.
- Side-effect: an OPTIONS with a matching `If-None-Match` will
  short-circuit to 304 by the same code path as GET — matching
  reference (its 304 check at `serve-handler/src/index.js:760` does
  not gate on method either).
- Side-effect: HEAD has always emitted a body in irserve; not a 7d
  concern. The reference behaves the same.

Two alternatives considered and rejected: (b) 204 preflight
short-circuit — friendlier but a deliberate divergence requiring a
D-NNN and contradicting the pinned reference snapshot; (c) status
quo at 405 — contradicts the OPTIONS scenario already listed in
SRV-CORS-001 (`inventory.md:1228-1272`).

## Plan

### Slice 0 — SRV-CACHE-005 verification + runner partitions

Cache-Control is pure verification. Irserve emits zero default
Cache-Control today; the 6-anchor probe `cache-control-default.json`
already pins the reference surface (200/304/404/listing both HTML
and JSON, plus a rule-applies positive case). Need to verify the
same probe under `target=irserve` and add a `runner.l0.clean`
partition.

- Read `tools/probe/cases/cache-control-default.json` end-to-end
  and confirm the 6 anchors as listed in the inventory:
  `default_file_no_rule`, `default_listing_html`,
  `default_listing_json`, `default_404_html`, `default_404_json`,
  `rule_applies_cache_control`.
- Run `node tools/probe/run.mjs cache-control-default
  --target=irserve --snapshot=verify`. Expected: all 6 anchors
  pass against the reference snapshot. If any anchor diverges
  (very unlikely — irserve emits no Cache-Control), stop and
  triage before continuing.
- Add `runner.l0.clean: [<all 6 anchor names>]` to the probe case
  (mirroring the `runner.l0.clean` shape used by `cors-applied`
  per the 6h precedent, and by `last-modified-roundtrip` per the
  7b precedent — paste the exact JSON shape from one of those).
- Update [`docs/reference/serve/oracle-matrix.md`](../../../repos/sotnyk/irServe/docs/reference/serve/oracle-matrix.md):
  flip ORC-047..052 from reference-only to dual-target by
  amending the rightmost-column status / runner column per the
  convention used in `oracle-matrix.md` (look at how 7c flipped
  ORC-044..046 — same surgery).
- Commit (after explicit approval per
  `feedback_iterative_commits` memory):
  `probe(stage-7d): promote cache-control-default to dual-target (ORC-047..052)`.

No Rust changes in this slice. If verification reveals an irserve
divergence (e.g. an unexpected default cache header sneaking in
from axum or hyper defaults), stop and surface — that becomes a
real implementation slice.

### Slice 1 — Allow OPTIONS through the dispatcher; promote ORC-056

The only code change in 7d. Reference snapshot pins
`cors-preflight#preflight_options` → 200 + file body + 4 CORS
headers + ETag + content-type + accept-ranges.

- Edit [`crates/irserve-core/src/dispatch.rs:94-100`](../../../repos/sotnyk/irServe/crates/irserve-core/src/dispatch.rs)
  to allow OPTIONS through the method gate alongside GET / HEAD:
  ```rust
  if !matches!(*req.method(), Method::GET | Method::HEAD | Method::OPTIONS) {
      // 405 path unchanged
  }
  ```
  The exact syntax should match the existing pattern in the file
  (the current gate is `if req.method() != Method::GET &&
  req.method() != Method::HEAD`). Phrasing is at agent discretion;
  the contract is "OPTIONS no longer hits 405".
- Add a focused dispatcher-level unit test in
  `dispatch::tests`: `options_routes_as_get_for_existing_file`
  (asserts 200 + body present + ETag header present + the four
  CORS headers via the full handler stack). A second test
  `options_returns_404_for_missing_path` confirms OPTIONS goes
  through the normal 404 branch (mirrors reference). A third
  test `options_with_if_none_match_returns_304` pins the
  side-effect short-circuit.
- Run `cargo test --test oracle` and the new dispatcher unit
  tests; both must be green.
- Run `node tools/probe/run.mjs cors-preflight --target=irserve
  --snapshot=verify`. Expected: `preflight_options` matches the
  reference snapshot byte-for-byte (status, headers, body sha).
- Add `runner.l0.clean: ["preflight_options"]` to
  `tools/probe/cases/cors-preflight.json`.
- Promote ORC-056 in [`docs/reference/serve/oracle-matrix.md`](../../../repos/sotnyk/irServe/docs/reference/serve/oracle-matrix.md)
  from reference-only to dual-target.
- Commit (after approval):
  `feat(stage-7d): route OPTIONS through the static pipeline (SRV-CORS-001)`.

### Slice 2 — Audit remaining cors-* probes for partition coverage

The roadmap names only ORC-056 explicitly, but the three other
`cors-*` probe cases (`cors-on-redirect`, `cors-response-surface`,
`cors-user-override`) and `cors-applied` may still be
reference-only despite the four-header surface being live since
Stage 6h. This slice is a one-shot audit, no Rust change.

- For each of `cors-applied`, `cors-on-redirect`,
  `cors-response-surface`, `cors-user-override`:
  - Read the case file to enumerate anchors.
  - Run under `target=irserve --snapshot=verify`. Record pass /
    fail per anchor.
  - For each green anchor, add it to `runner.l0.clean` in the
    case file.
  - For each failing anchor: triage. If the divergence is real,
    *stop and surface to the user* before doing anything else —
    do not silently weaken the contract per anti-hallucination
    rule #2. If the divergence is purely volatile (mtime, body
    hash on a 404 page), use a `*MayDiffer` overlay per the 6h /
    7b precedent.
- Cross-update relevant ORC rows (ORC-053..055, ORC-057 if they
  exist for these probes per agent 3's report) in
  `oracle-matrix.md`.
- Commit (after approval):
  `probe(stage-7d): dual-target audit for cors-* probes`.

If all four probes happen to already be at L0-clean / dual-target,
this slice collapses into a one-line ack in the change log and no
commit. Empirical signal will decide.

### Slice 3 — Meta slice (spec deltas + plan file)

Delegate per the kickoff-template subagent trigger: structured
brief = slice plan + commit log + a peer change package to mirror.
Peer: `openspec/changes/013-range-requests/` (the most recent
Stage 7 change). Subagent produces:

- `openspec/changes/014-cache-headers-and-preflight/proposal.md` —
  Why / What. References SRV-CACHE-005 and SRV-CORS-001; no
  D-NNN; flips ORC-047..052 and ORC-056 (plus any others
  promoted in slice 2) to dual-target. Mirrors 7c's wording for
  "this change closes...".
- `openspec/changes/014-cache-headers-and-preflight/design.md` —
  architecture. Two §s: §1 OPTIONS routing as GET (paste
  reference verbatim from `serve-handler/src/index.js:548-769`
  with line numbers, the no-method-check pattern, the
  dispatcher seam at `dispatch.rs:94-100`). §2 Cache-Control
  absence (paste `getHeaders` reference verbatim:
  `serve-handler/src/index.js:194-254`, highlight that
  `defaultHeaders` never includes Cache-Control). No new D-NNN.
- `openspec/changes/014-cache-headers-and-preflight/tasks.md` —
  slice-by-slice with `[x]` per the 7c pattern. Reproduces the
  slice plan from this file with checkbox state set from the
  actual commit log at hand-off time.
- Spec deltas (capability spec subdir): per agent 3, the
  `http-cache/spec.md` already names the forthcoming Cache-Control
  addition. Author a `MODIFIED` delta on `http-cache/spec.md`
  adding the Cache-Control absence-of-default requirement (and its
  rule-applies positive case). For CORS, the inventory entry
  SRV-CORS-001 promises the full four-header surface plus the
  OPTIONS routing scenario — author an `ADDED` capability
  `openspec/specs/cors/spec.md` (new capability dir per the L3
  promotion path; the L1 `Access-Control-Allow-Origin: *` baseline
  in `cli/spec.md:158-173` stays as-is and the L3 surface lives in
  `cors/spec.md`). Mirror peer style strictly — quote the §
  structure from `013-range-requests/specs/http-cache/` and follow
  it.
- Authoring conventions: `Compatibility notes:` is the
  spot for absence-of-default semantics (mirroring 6f's
  SRV-HDR-001 / -002 prose). No `adapted` flag on the SRVs
  because no divergence.
- Inventory tweaks: in [`docs/reference/serve/inventory.md`](../../../repos/sotnyk/irServe/docs/reference/serve/inventory.md),
  the SRV-CORS-001 line "Whether IrServe should adopt or diverge"
  in `Open questions:` becomes resolved — strike it, note the
  decision to mirror. SRV-CACHE-005 already has `Open questions:
  None.` — no change.

The main agent reviews the subagent's output and Edits if needed.

Also: drop an `0018_PLAN_stage7d_cache_headers_and_preflight.md`
under `docs/features/` (the in-repo plan, following the 0001..0017
convention). Subagent can copy from this `.claude/plans/`
file with cosmetic cleanup.

- Commit (after approval):
  `docs(stage-7d): add 014-cache-headers-and-preflight change package`.

### Stage close

- Flip the 7d row in [`README.md`](../../../repos/sotnyk/irServe/README.md)
  stage map from `todo` to `done`. Update the post-6h "Try IrServe"
  prose at the bottom of the README if any 7d-observable behavior
  warrants a one-line example (likely just an OPTIONS curl example
  alongside the existing `curl -i http://127.0.0.1:3010/` lines).
  Append a one-line note to "What is NOT yet observable" removing
  default `Cache-Control` and CORS preflight from the list (they
  are now observable / verified).
- Anticipate Codex review rounds per
  `feedback_review_rounds`: each round = one commit
  `docs(stage-7d): address Codex review round N (P{...} fixes)`.

## Critical files

To modify:

- `crates/irserve-core/src/dispatch.rs` (~3-line edit to method
  gate; ~30 lines of new unit tests).
- `tools/probe/cases/cache-control-default.json` (add
  `runner.l0.clean` block).
- `tools/probe/cases/cors-preflight.json` (add `runner.l0.clean`
  block).
- `tools/probe/cases/cors-applied.json` /
  `cors-on-redirect.json` / `cors-response-surface.json` /
  `cors-user-override.json` (audit-conditional, may add
  partitions).
- `docs/reference/serve/oracle-matrix.md` (status column flips
  for ORC-047..052, ORC-056, maybe ORC-053..055/057).
- `docs/reference/serve/inventory.md` (SRV-CORS-001 open-question
  strike).
- `README.md` (stage row + observability prose).
- `docs/features/0018_PLAN_stage7d_cache_headers_and_preflight.md`
  (new).
- `openspec/changes/014-cache-headers-and-preflight/` (new
  directory: proposal / design / tasks / spec deltas).

Not to modify:

- `third_party/serve` and `third_party/serve-handler` (hard stop
  per AGENTS.md).
- `tools/probe/snapshots/cache-control-default.json` and
  `tools/probe/snapshots/cors-preflight.json` — reference
  snapshots stay frozen; only the case-file `runner.l0` block
  changes.

## Reuse / no new abstractions

- `apply_cors` (`crates/irserve-core/src/cors.rs:26-37`) — already
  layered post-dispatch, hits every status code including 405.
  Will naturally apply to OPTIONS 200 once the method gate opens.
- `apply_custom_headers` (`crates/irserve-core/src/custom_headers.rs:178-221`)
  — already applies to non-3xx responses including via the dispatch
  wrapper's headers_path mechanism. OPTIONS will pick it up
  naturally.
- `build_file_or_304` — handles ETag/304/Range; reused verbatim
  for OPTIONS.
- Probe-runner `runner.l0.clean` partition format — copy from
  `cors-applied.json` and/or `last-modified-roundtrip.json`. No
  runner extension needed.

## Verification (end-to-end)

After all slices commit, before the meta slice:

```bash
# Probe verification — both cache and preflight green dual-target.
node tools/probe/run.mjs cache-control-default --target=irserve --snapshot=verify
node tools/probe/run.mjs cors-preflight --target=irserve --snapshot=verify
node tools/probe/run.mjs cors-applied --target=irserve --snapshot=verify
node tools/probe/run.mjs cors-response-surface --target=irserve --snapshot=verify
node tools/probe/run.mjs cors-on-redirect --target=irserve --snapshot=verify
node tools/probe/run.mjs cors-user-override --target=irserve --snapshot=verify

# Full oracle harness — regression-free across the L0-clean set.
cargo test --test oracle

# Manual smoke — OPTIONS behaves like GET.
mkdir -p _tmp && echo 'body{}' > _tmp/asset.css
cargo run -- --cors --listen 3010 _tmp &
curl -i -X OPTIONS \
  -H 'Origin: https://example.com' \
  -H 'Access-Control-Request-Method: GET' \
  http://127.0.0.1:3010/asset.css
# Expect: 200 + body + 4 access-control-* headers + etag + content-type.

# Manual smoke — no default Cache-Control.
curl -sI http://127.0.0.1:3010/asset.css | grep -i cache-control
# Expect: no output.

# Manual smoke — user rule still works (Stage 6f regression check).
# Add serve.json: {"headers":[{"source":"**/asset.css","headers":[
#   {"key":"Cache-Control","value":"public, max-age=60"}]}]}
# (restart)
curl -sI http://127.0.0.1:3010/asset.css | grep -i cache-control
# Expect: cache-control: public, max-age=60
```

## Risks / signals to watch

- **Slice 0 surprise.** If the cache-control probe fails under
  `target=irserve`, it likely means an axum/hyper default is
  sneaking in (e.g. via `tower-http`'s response middleware). That
  promotes 7d from verification-only to a real implementation
  slice. Stop and surface.
- **Slice 1 surprise.** If allowing OPTIONS triggers latent
  assumptions further down the pipeline (e.g. an axum extractor
  that gates on method, or a hard-coded GET branch somewhere in
  the dispatcher we missed), the unit tests will catch it. Plan
  for one extra ~30-minute debugging window.
- **Slice 2 audit.** If any cors-* probe diverges, treat it as a
  Stage 6h regression and surface — don't bury it in a 7d commit.
- **Anti-hallucination rule #9.** This stage is small enough that
  Codex rounds should not pile up. If a single aspect (e.g.
  "OPTIONS + Range interaction" or "OPTIONS + 304") generates
  three consecutive rounds, pause and declare parity scope per
  the rule.

## Out of scope

- HEAD body-suppression. Reference doesn't suppress; irserve
  doesn't either. Not 7d.
- TRACE / CONNECT / PATCH method handling. Still 405. Reference
  also returns weird shapes for these (probably 200 + body via
  the no-method-check path); not a 7d contract.
- The `cors-flag.json` CLI-parsing probe — already covered by
  Stage 6h via SRV-CLI-010 (L1 baseline).
- 7e (compression). Independent sub-stage.
