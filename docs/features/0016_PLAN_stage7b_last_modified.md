# Stage 7b — `Last-Modified` + `--no-etag` + `If-Modified-Since`

## Context

Stage 7b is the next "Next" row in `README.md` and
`docs/stage7_l3_capabilities.md`. It builds on Stage 7a
(`011-etag-conditional`, just landed):

- 7a established the `etag` module, `build_file_or_304`
  short-circuit, and the probe runner's `$fromResponse`
  capture-replay extension.
- 7a wired the `serve.json` `etag: bool` field through to
  the dispatcher but did **not** add the `--no-etag` CLI flag.

7b closes:

- **SRV-CLI-013** — `--no-etag` CLI flag (P1, `accepted`).
- **SRV-CACHE-002** — under `--no-etag` / `etag: false`,
  emit `Last-Modified` (RFC 7231 IMF-fixdate UTC of mtime)
  in place of `ETag` (P1, `accepted`).
- **SRV-CACHE-003** — `If-Modified-Since` handling (P1,
  currently `unknown` until probe-closure).
- **Q-009** — IMS handling under `--no-etag` (open).

Verbatim recon evidence (one investment, reused across implementation
and Codex review rounds):

1. **Last-Modified is mutually exclusive with ETag in reference.**
   `serve-handler/src/index.js:227-236`:

   ```js
   if (etag) {
     // ... compute sha, set defaultHeaders['ETag']
   } else {
     defaultHeaders['Last-Modified'] = stats.mtime.toUTCString();
   }
   ```

   Reference emits **exactly one** of ETag or Last-Modified per file
   response, gated on `config.etag`.

2. **Reference has zero IMS handling.** Grep across
   `third_party/serve-handler/src/` and `third_party/serve/src/`
   for `if-modified-since`/`ifModifiedSince`/`IfModifiedSince` returns
   no hits. The 304 short-circuit at `index.js:760-764` only handles
   `If-None-Match`. So a conditional GET against the reference under
   `--no-etag` returns **200 with the full body**, regardless of IMS.
   This is the suspected behavior for Q-009; probes confirm.

3. **`--no-etag` mapping.** `serve/source/utilities/cli.ts:155`
   parses `--no-etag: Boolean` (no short alias).
   `serve/source/utilities/config.ts:140`:
   `config.etag = !args['--no-etag']`. Default ETag is on.

## Decisions taken with the user (AskUserQuestion this session)

- **D1 — IMS 304 short-circuit (D-018).** irserve adapts: under
  `etag: false` (i.e. `--no-etag` or `serve.json#etag = false`),
  a request whose `If-Modified-Since` ≥ file mtime gets a 304
  response with no body, no `Content-Type`, no `Last-Modified` echo
  (mirrors the 304 shape from ETag/INM at `serve-handler/src/index.js:761-764`).
  Reference always returns 200; the divergence is documented in
  D-018 + the spec delta's Compatibility note.
- **D2 — Probe sequencing.** Slice 0 is a dedicated reference-probe
  commit closing Q-009 before any Rust code is written
  (anti-hallucination rule #8). Slice 0 lands the probes
  + snapshots + the inventory/open-questions flip.
- **D3 — CLI flag form.** Mirror reference exactly:
  `--no-etag` long-only, no short alias.

## Decisions taken without asking (small technical points)

- **Last-Modified mutex with ETag mirrors reference.** Methodology
  rule #4 (reference is the arbiter for behavior); SRV-CACHE-002's
  current `accepted` text explicitly says "instead of `ETag`".
  irserve never emits both headers simultaneously on a file response.
- **Format = HTTP-date (IMF-fixdate, UTC).** Use the `httpdate` crate
  (or `chrono`'s `to_rfc2822`-like helper if already a dep) to format
  `SystemTime` → `Wed, 06 May 2026 23:39:00 GMT`. Reference uses
  `Date#toUTCString()`. Round to whole-second resolution (sub-second
  mtimes are not preserved in the wire format — already noted in
  SRV-CACHE-002's Compatibility section).
- **mtime source.** `std::fs::metadata(path)?.modified()` —
  follows symlinks (matches irserve's current behavior; symlink
  parity is L4 / Q-011 and stays deferred). Reference uses `lstat`
  but that detail is L4 territory.
- **Module layout.** New `crates/irserve-core/src/last_modified.rs`
  parallel to `etag.rs`, exposing
  `last_modified_value(meta: &Metadata) -> Option<HeaderValue>` and
  a private `format_http_date(SystemTime) -> String`. Keeps the
  `etag` module focused.
- **`build_file_or_304` extension.** Rename to
  `build_file_or_conditional_304` and have it accept
  `(serve_config, req_headers, path, bytes, meta, header_rules, request_path)`.
  Decision tree at the 304 site (mirrors reference's mutex):
  - If `serve_config.etag != Some(false)`: ETag path (existing 7a
    behavior — `If-None-Match` drives 304).
  - Else: Last-Modified path — emit `Last-Modified` from `meta`,
    then check `If-Modified-Since` ≥ mtime → 304 (D-018 adaptation).
  - `Range` header still suppresses the 304 short-circuit
    (7c precursor, mirrors reference `index.js:760`).
- **User `headers` rules + Last-Modified.** Same merge-then-decide
  ordering as ETag: candidate 200 → `apply_custom_headers` →
  read merged `Last-Modified` → compare to IMS. A user rule that
  deletes (`null`) or overrides `Last-Modified` drives the
  decision. (Mirrors the round-1 P1 reshape from 7a.)
- **Probe runner.** No code changes needed — `$fromResponse` from
  7a covers `Last-Modified` → `If-Modified-Since` verbatim.
  Add `lastModifiedMayDiffer` overlay support to the runner's L0
  volatile-header mask (parallel to `etagMayDiffer`) for cases
  where the snapshot comparator should ignore the literal
  Last-Modified string but assert presence/shape.
- **D-NNN numbering.** Stage 7b lands **D-018** (IMS 304 adaptation).
  No D-017 edits.
- **Plan file numbering.** `docs/features/0016_PLAN_stage7b_last_modified.md`
  (continues the 0001..0015 sequence; 7a was 0015).
- **Change package.** `openspec/changes/012-last-modified/` per the
  roadmap row.

## Files to touch

**New:**
- `crates/irserve-core/src/last_modified.rs` — formatter + value helper.
- `tools/probe/cases/last-modified-roundtrip.json` — close Q-009 (slice 0).
- `tools/probe/cases/no-etag-flag.json` — CLI-level smoke (slice 1).
- `tools/probe/snapshots/last-modified-roundtrip.json` (recorded).
- `tools/probe/snapshots/no-etag-flag.json` (recorded).
- `openspec/changes/012-last-modified/{proposal,design,tasks}.md`.
- `openspec/changes/012-last-modified/specs/http-cache/spec.md` —
  ADDED Requirement(s) for SRV-CACHE-002 + SRV-CACHE-003;
  MODIFIED if any 7a requirement text needs touching (likely none).
- `openspec/changes/012-last-modified/specs/cli/spec.md` —
  ADDED scenario for SRV-CLI-013 if needed (or just bullet in cli
  spec depending on existing structure).
- `docs/features/0016_PLAN_stage7b_last_modified.md` — implementation
  plan for the working session (mirrors `0015_PLAN_stage7a_*.md`
  structure: Context / Decisions taken with user / Decisions taken
  without asking / Files to touch / Slice plan / Out of scope).

**Modified:**
- `crates/irserve/src/main.rs` — add `--no-etag: bool` flag; override
  `serve_config.etag = Some(false)` post-parse when set (mirrors
  reference's `config.etag = !args['--no-etag']`).
- `crates/irserve-core/src/lib.rs` — `mod last_modified;`.
- `crates/irserve-core/src/dispatch.rs` — extend / rename
  `build_file_or_304` → `build_file_or_conditional_304`; thread
  `&Metadata` to it from the two call sites
  (File/Index arm, renderSingle branch); add Last-Modified branch
  + IMS 304 logic; new unit tests in `dispatch::tests`.
- `crates/irserve-core/src/file_response.rs` (or wherever the
  bytes+path read happens) — surface mtime alongside bytes if not
  already.
- `crates/irserve-core/Cargo.toml` — add `httpdate = "1"` (or
  equivalent) if not already present.
- `tools/probe/run.mjs` — add `Last-Modified` to L0 volatile-header
  mask similar to ETag handling; thread `lastModifiedMayDiffer`
  overlay where needed.
- `tools/probe/README.md` — note the new overlay in the volatile-
  header section.
- `docs/reference/serve/decisions.md` — append **D-018**.
- `docs/reference/serve/open-questions.md` — close Q-009 with
  resolution: reference returns 200 + full body for any IMS;
  irserve adapts per D-018.
- `docs/reference/serve/inventory.md` — flip SRV-CACHE-003 from
  `unknown` to `verified` (reference) / `adapted` (irserve).
  No change to SRV-CACHE-002 (already `accepted`) or SRV-CLI-013
  (already `accepted`).
- `docs/reference/serve/oracle-matrix.md` — new ORC rows for
  Last-Modified emission under `--no-etag`, IMS-with-match → 304
  under irserve only, IMS-with-match → 200 under reference,
  IMS-malformed-date behavior. Cross-link D-018.
- `README.md` — flip Stage 7b row to `done`; update the "What is
  NOT yet observable" footer (drop `Last-Modified`, drop IMS
  semantics); add `--no-etag` + Last-Modified curl demo to "Try
  IrServe".

**Untouched (do not modify):**
- `third_party/serve`, `third_party/serve-handler` — pinned oracles.
- Existing `etag-roundtrip.json` / `etag-conditional.json` cases
  and snapshots (no reference-behavior change at the ETag layer).
- `openspec/specs/http-cache/spec.md` is only modified through
  the change package's ADDED delta, validated via
  `openspec validate --all --strict`.

## Slice plan

Five iterative slices, one commit per green-state slice. **Ask the
user before each `git commit`** (per the persistent memory rule
"feedback_iterative_commits"). Slice 5 is the meta slice and lands
last; delegate spec prose to a subagent per the kickoff template's
hand-off list.

### Slice 0 — Reference probes closing Q-009

- New `tools/probe/cases/last-modified-roundtrip.json` with 5–7
  requests under `--no-etag` (config = `{"etag": false}`),
  exercising:
  1. `first_get` — capture `Last-Modified`, expect no `ETag` header.
  2. `ims_exact` — IMS = captured Last-Modified, expect **200** under
     reference (Q-009 closure).
  3. `ims_future` — IMS = a date well past mtime, expect 200.
  4. `ims_past` — IMS = `Thu, 01 Jan 1970 00:00:00 GMT`, expect 200.
  5. `ims_malformed` — IMS = `not-a-date`, expect 200 (reference
     never branches on IMS at all, so malformed values are inert).
  6. `ims_with_etag_on` — IMS sent with config `{etag: true}`,
     expect 200 + `ETag` (no `Last-Modified`; mutex per reference).
  7. `ims_on_404` — IMS against a missing file, expect 404 (sanity).
- Record snapshots under `target=reference` via
  `node tools/probe/run.mjs last-modified-roundtrip --target=reference --snapshot=update`.
- `runner.l0.clean` partition lists only requests 1–3 + 6 (the
  ones that flip to dual-target after Stage 7b implementation;
  malformed/404 stay reference-only as they are not the L3 surface).
- Update `docs/reference/serve/open-questions.md` — close Q-009
  with the probe outcome.
- Update `docs/reference/serve/inventory.md` — flip SRV-CACHE-003
  to `verified` (reference behavior is now empirically pinned).
- Update `docs/reference/serve/oracle-matrix.md` — add ORC rows
  for the new requests (reference-only initially; promoted in
  slice 4).
- Verify: `node tools/probe/run.mjs last-modified-roundtrip
  --target=reference --snapshot=verify` green; no Rust changes
  yet so `cargo` untouched.
- Commit: `probe(stage-7b): close Q-009 — reference IMS handling under --no-etag`

### Slice 1 — `--no-etag` CLI flag + threading

- `crates/irserve/src/main.rs`: add
  `#[arg(long = "no-etag")] no_etag: bool` to `Cli`.
- Post-parse: if `cli.no_etag`, override `serve_config.etag =
  Some(false)` (a value already loaded from `serve.json`'s
  `etag: false` wins identically — irrelevant; if `serve.json`
  set `etag: true` explicitly, the CLI flag still wins, mirroring
  reference's `args['--no-etag']` being the final word).
- `tools/probe/cases/no-etag-flag.json` — minimal smoke: spawn
  irserve with `--no-etag`, request a file, assert no `ETag`,
  presence of `Last-Modified` (assert presence, value
  `lastModifiedMayDiffer`).
- Six new unit tests (or as needed) in dispatcher / main covering:
  CLI flag set → `serve_config.etag = Some(false)`; CLI flag
  unset + `serve.json` etag absent → default true; etc.
- Verify: `cargo test --workspace` green; oracle harness green
  under both targets (existing cases untouched).
- Commit: `feat(stage-7b): slice 1 — --no-etag CLI flag wired through to ServeConfig`

### Slice 2 — `last_modified` module + emission on 200

- New `crates/irserve-core/src/last_modified.rs` exposing
  `last_modified_value(meta: &Metadata) -> Option<HeaderValue>`
  (returns `None` if `meta.modified()` fails — paranoid path,
  shouldn't happen on the platforms we support). Use `httpdate`
  crate for IMF-fixdate formatting.
- Unit tests pin a known mtime → fixed wire string.
- Thread `&Metadata` through `file_response` and the call sites
  at dispatch.rs (currently only `bytes` is threaded; add `meta`).
- Extend `etag_value` + `last_modified_value` selection logic:
  emit ETag iff `serve_config.etag != Some(false)`, else emit
  Last-Modified. **Mutex** like reference.
- Update existing ETag unit tests where needed (the `etag: false`
  case should now also assert `Last-Modified` presence).
- Verify: `cargo test --workspace` green; oracle harness green
  under both targets.
- Commit: `feat(stage-7b): slice 2 — emit Last-Modified header under --no-etag`

### Slice 3 — IMS 304 short-circuit (D-018 adaptation)

- Rename `build_file_or_304` → `build_file_or_conditional_304`
  in `dispatch.rs`. Extend logic per the decision tree in
  "Decisions taken without asking" above.
- Parse IMS as HTTP-date via `httpdate::parse_http_date`;
  malformed IMS → ignore (treat as no IMS, return 200). Mirrors
  RFC 9111 §13.1.3 (recipients SHOULD ignore unparseable IMS).
- 304 fires when: (a) `serve_config.etag == Some(false)`,
  (b) no `Range` header, (c) merged response carries
  `Last-Modified` (after `apply_custom_headers`), (d) IMS parses,
  (e) `floor(file_mtime, second) ≤ ims_value` (round file mtime
  to whole seconds to match HTTP-date resolution).
- 304 response: no body, no `Content-Type`, no `Last-Modified`
  echo (mirrors ETag/INM 304 shape from 7a).
- Promote requests 1–3 + 6 from `last-modified-roundtrip.json`
  to `runner.l0.clean`. Under `target=irserve` the IMS round-trip
  now returns 304 instead of 200 — that's the D-018 divergence,
  and the snapshot needs split handling: `runner.l0.clean.*` cases
  whose response shape differs between targets get the
  `responseMayDiffer` pattern (or equivalent split-snapshot
  mechanism — verify what 7a established for the
  `etag-roundtrip` second request; if there is no precedent,
  use the existing `etagMayDiffer`/`lastModifiedMayDiffer`
  overlays plus a status overlay).

  **Open implementation detail to resolve in slice 3 itself:**
  the probe runner may need a `statusMayDiffer` overlay
  if not already present, for the IMS-match case (reference=200,
  irserve=304). If so, add it minimally and document in
  `tools/probe/README.md`.

- ~6–8 new unit tests in `dispatch::tests`: IMS-exact-match → 304,
  IMS-future → 304, IMS-past → 200, IMS-malformed → 200,
  Range present + IMS-match → 200 (7c precursor), user
  `Last-Modified: null` rule deletes header → no 304 possible,
  user `Last-Modified` override drives the 304 decision against
  the override value.
- Verify: `cargo test --workspace` green; oracle harness green
  under both targets with the new snapshots; `target=irserve`
  total count goes up by 4 (probes 1, 2, 3, 6 promoted).
- Commit: `feat(stage-7b): slice 3 — 304 short-circuit on If-Modified-Since (D-018)`

### Slice 4 — Spec deltas + meta (delegated)

Per the kickoff template's "spec prose subagent" trigger,
delegate authoring of:
- `openspec/changes/012-last-modified/proposal.md`
- `openspec/changes/012-last-modified/design.md`
- `openspec/changes/012-last-modified/tasks.md`
- `openspec/changes/012-last-modified/specs/http-cache/spec.md`
  (ADDED Requirement(s) for SRV-CACHE-002 + SRV-CACHE-003 with
  Compatibility note documenting D-018; possibly MODIFIED of
  the 7a requirement to note that the ETag and Last-Modified
  paths are mutually exclusive)
- `openspec/changes/012-last-modified/specs/cli/spec.md`
  (ADDED scenario for SRV-CLI-013)

Briefing for the subagent: slice plan from this file + commit
log from slices 0–3 + D-018 entry + the 011-etag-conditional
package as style precedent. Subagent does NOT need the full
implementation conversation in context.

Main agent reviews and Edits if needed.

Also in slice 4 (main agent, not subagent):
- Append **D-018** to `docs/reference/serve/decisions.md` with
  the IMS 304 adaptation rationale (cite reference's zero-IMS-branch
  finding from slice 0 probes, anti-hallucination rule #5 framing
  for the divergence).
- `README.md`: flip Stage 7b row to `done`; trim the "What is NOT
  yet observable" footer; add `--no-etag` + `Last-Modified` +
  IMS 304 curl demo to "Try IrServe".
- Run `npx -y @fission-ai/openspec@latest validate --all --strict`
  and report any failures.
- Commit (after main-agent review of subagent output):
  `docs(stage-7b): spec deltas + D-018 + oracle-matrix promotion + meta`

### Codex review rounds

Per the persistent memory rule "feedback_review_rounds": user
hands the diff to Codex; each round of fixes lands as one commit
titled `docs(stage-7b): address Codex review round N (P{priorities} fixes)`.
Push back if disagreement — pushback is expected.

If review rounds keep landing on the same fine-grained aspect
(e.g. mtime resolution, IMS parser corners, mutex edge cases)
for **three rounds running**, pause per anti-hallucination rule
#9: declare parity scope, document divergences in BOTH D-018 and
the spec Compatibility note, ask the user before round 4.

## Out of scope for Stage 7b (mandatory pre-stage list per anti-hallucination #10)

The following are NOT addressed in 7b. Discovery of any of these
in review rounds is a signal that this list was under-specified;
discuss before patching.

1. **`If-Unmodified-Since`** — RFC 7232 §3.4 conditional request
   header. Reference does not handle it; irserve does not either.
   No P-class SRV; out of scope for L3.
2. **`Vary: Accept-Encoding` / Vary on `Last-Modified`** —
   compression is 7e territory.
3. **Weak Last-Modified comparison semantics** — RFC 7232 §2.2.2
   distinguishes strong/weak validators. irserve does whole-second
   comparison only (mirrors HTTP-date wire resolution); no weak
   comparison logic.
4. **Sub-second mtime precision** — already noted in SRV-CACHE-002.
5. **`Last-Modified` on directory listings, 3xx redirects, JSON
   error responses, custom HTML error pages** — same surface as
   ETag's 7a deferrals. None of these get `Last-Modified` either.
6. **`Last-Modified` cache between requests** — reference has the
   ETag cache (`Map<absPath, [mtime, sha]>`); for Last-Modified
   no cache is needed (mtime fetch is one syscall). No change.
7. **Range + IMS interaction beyond the guard** — RFC 7233 §3.3
   says a 304 takes precedence over 206. The Range guard
   suppresses both ETag/INM and Last-Modified/IMS short-circuits
   in 7b (7c precursor). 7c may revisit.
8. **Multiple IMS values / IMS list parsing** — RFC 7232 says one
   value only. irserve treats first value, ignores subsequent
   comma-separated values, malformed-style.
9. **`Date` response header** — outside Stage 7 scope per the
   roadmap. axum may or may not emit it today; not contractual.
10. **Symlink mtime semantics** — irserve uses `metadata()`
    (follows symlinks), reference uses `lstat` (does not).
    L4 / Q-011 territory.

## Hard stops (kickoff template §"Hard stops")

- Do not modify `third_party/`.
- Existing snapshots (ETag, etc.) touched only if reference
  behavior changed — it has not. The 7a snapshots stay byte-identical.
- Contract changes only through MOD deltas or D-NNN entries
  after explicit user discussion. D-018 is the only new D-NNN.
- Do not commit without explicit permission. Propose when slice
  is green and ready.

## Verification (end-to-end)

After all slices land:

```bash
# Workspace tests
cargo test --workspace

# Oracle harness — both targets green
node tools/probe/run.mjs --all --target=reference --snapshot=verify
node tools/probe/run.mjs --all --target=irserve --snapshot=verify
# Expected: target=irserve total goes up by ~4 (last-modified-roundtrip
# requests 1, 2, 3, 6 promoted to dual-target via runner.l0.clean).

# OpenSpec contract validation
npx -y @fission-ai/openspec@latest validate --all --strict
```

Manual smoke test (mirrors what lands in README "Try IrServe"):

```bash
mkdir -p _tmp && echo 'body{color:red}' > _tmp/asset.css

# Default — ETag, no Last-Modified
cargo run -- --listen 3010 _tmp &
curl -sI http://127.0.0.1:3010/asset.css | grep -iE '^(etag|last-modified):'
# Expect: ETag present, no Last-Modified

# --no-etag — Last-Modified, no ETag
cargo run -- --no-etag --listen 3010 _tmp &
LM=$(curl -sI http://127.0.0.1:3010/asset.css | grep -i ^last-modified: | cut -d: -f2- | tr -d '\r')
curl -i -H "If-Modified-Since:$LM" http://127.0.0.1:3010/asset.css
# Expect: 304, no body (D-018 — diverges from reference's 200)

# serve.json etag: false (no CLI flag) — same Last-Modified surface
echo '{"etag": false}' > _tmp/serve.json
cargo run -- --listen 3010 _tmp &
curl -sI http://127.0.0.1:3010/asset.css | grep -iE '^(etag|last-modified):'
```
