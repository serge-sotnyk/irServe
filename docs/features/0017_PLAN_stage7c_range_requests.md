# Stage 7c — Range requests (`206` / `416`)

## Context

Stage 7c is the next "Next" row in `README.md` and the third
sub-stage of Stage 7 (L3 polish) per
`docs/stage7_l3_capabilities.md`. It closes **SRV-CACHE-004**
(P2, `verified`) and flips ORC-044/045/046 from reference-only
to dual-target. No new SRV is introduced; the inventory entry
already exists and lists the reference source line ranges
(`serve-handler/src/index.js:717-734` + `:749-752`).

7c is **independent of 7a/7b** for ordering (per the
`docs/stage7_l3_capabilities.md` "Ordering rationale" section)
but inherits two structural foundations that 7a/7b already
landed:

- The **`Range`-absent guard** at
  `crates/irserve-core/src/dispatch.rs:771`
  (`if req_headers.get(RANGE).is_none()`) wraps both the
  ETag/INM and the Last-Modified/IMS 304 short-circuits. 7c
  fills in the `else` branch of that guard — currently it
  falls through to "return the merged 200 response unchanged",
  which is wrong wire-shape for any Range-bearing request.
- `Metadata` is already threaded through to `build_file_or_304`
  (7b work) so the file size is readily available without
  re-stating.

## Verbatim recon evidence (one investment, reused across slices)

### Reference behavior — `third_party/serve-handler/src/index.js`

```js
// L717-734: Range parsing + status setup
const streamOpts = {};

// TODO ? if-range
if (request.headers.range && stats.size) {
  const range = parseRange(stats.size, request.headers.range);

  if (typeof range === 'object' && range.type === 'bytes') {
    const {start, end} = range[0];

    streamOpts.start = start;
    streamOpts.end = end;

    response.statusCode = 206;
  } else {
    response.statusCode = 416;
    response.setHeader('Content-Range', `bytes */${stats.size}`);
  }
}

// TODO ? multiple ranges

let stream = null;

try {
  stream = await handlers.createReadStream(absolutePath, streamOpts);
} catch (err) {
  return internalError(absolutePath, response, acceptsJSON, current, handlers, config, err);
}

const headers = await getHeaders(handlers, config, current, absolutePath, stats);

// L749-752: 206-only header injection (AFTER getHeaders merge)
// eslint-disable-next-line no-undefined
if (streamOpts.start !== undefined && streamOpts.end !== undefined) {
  headers['Content-Range'] = `bytes ${streamOpts.start}-${streamOpts.end}/${stats.size}`;
  headers['Content-Length'] = streamOpts.end - streamOpts.start + 1;
}

// L760-764: 304 short-circuit (Range-absent guard already in place)
if (request.headers.range == null && headers.ETag && headers.ETag === request.headers['if-none-match']) {
  response.statusCode = 304;
  response.end();
  return;
}

response.writeHead(response.statusCode || 200, headers);
stream.pipe(response);
```

Key non-obvious points (do NOT re-discover during implementation
or Codex review):

1. **416 carries the full file body.** The reference does NOT
   route 416 through `sendError`; it sets `statusCode = 416`
   inline, adds `Content-Range: bytes */<size>`, then falls
   through to `stream.pipe(response)` with an empty `streamOpts`
   (so `createReadStream` reads the whole file). The integration
   test `range request not satisfiable`
   (`test/integration.test.js:1203-1227`) pins:
   `expect(length).toBe(content.length)` AND
   `expect(text).toBe(spec)` (full body). The existing snapshot
   `tools/probe/snapshots/range-request.json#out_of_range` confirms:
   `content-length: "11"`, `content-range: "bytes */11"`,
   body `"abcdefghij\n"`. **Decision-1 below** keeps this on
   irserve too — no D-NNN.

2. **Range pre-empts the 304 short-circuit, even with matching
   `If-None-Match` / `If-Modified-Since`.** The guard at L760
   short-circuits ONLY when `request.headers.range == null`.
   irserve already mirrors this at `dispatch.rs:771`. 7c does
   not touch the guard itself, only fills the `else` branch.

3. **Range headers piggyback on the user-`headers` merge.** The
   `Content-Range` / `Content-Length` for 206 are written into
   `headers` AFTER `getHeaders` (which already merged user
   `serve.json#headers` rules). So a user `headers` rule
   carrying `Content-Length: 999` would be silently overwritten
   by the range-emitted value. We mirror this: range-emitted
   headers are last-write-wins over user rules. (Symmetric with
   how `apply_custom_headers` in irserve hands the merged map to
   the 304 decision — see 7a Codex round-1 P1.)

4. **No size → no range.** The guard `if (request.headers.range
   && stats.size)` ignores Range when the file size is unknown
   (`stats.size === 0` is also falsy, but the only practical
   case is `lstat` returning `null` via the test fixture at
   `test/integration.test.js:1130-1175`). For irserve this is
   degenerate: `tokio::fs::metadata(&p)?.len()` always returns a
   number on supported platforms, and we already read the full
   bytes into `Vec<u8>` before the range hook runs — so
   `bytes.len()` is authoritative. **Decision-2 below** uses
   `bytes.len()` as the total size, ignoring `meta.len()` for
   this decision to avoid TOCTOU between `metadata()` and
   `read()`.

5. **Only the first range from a parsed result is used**
   (`range[0]` at L724). Multiple-range comma lists are
   explicitly NOT mirrored; the reference's `// TODO ? multiple
   ranges` matches our Out-of-Scope item below.

6. **Range parsing is via the `range-parser` npm package**
   (`require('range-parser')` at L16). Its grammar accepts
   `bytes=<s>-<e>`, `bytes=<s>-`, `bytes=-<n>`, and
   comma-separated lists. Returns `-1` for malformed syntax
   (irserve → 416 to mirror reference's else-branch),
   `-2` for unsatisfiable (e.g. `bytes=999-1000` on an 11-byte
   file). Both `-1` and `-2` land in the same reference
   `else` → 416 branch. We don't need to distinguish them.

7. **Wrong unit (`Range: pixels=0-3`) → 416.** The reference's
   `range[0].type !== 'bytes'` check sends it to 416, but
   `range-parser` only emits `'bytes'` for `bytes=...`
   (returns `-1` for anything else). Both paths converge on
   the same 416 branch. Confirmed by reading
   `range-parser/index.js`.

### Reference integration tests verbatim

```js
// L1177-1201: positive case (206)
test('range request', async () => {
  const name = 'docs.md';
  const content = await fs.readFile(path.join(fixturesFull, name));
  const url = await getUrl();
  const response = await fetch(`${url}/${name}`, {
    headers: { Range: 'bytes=0-10' }
  });
  expect(response.headers.get('content-range')).toBe(`bytes 0-10/${content.length}`);
  expect(Number(response.headers.get('content-length'))).toBe(11);
  expect(response.status).toBe(206);
  expect(await response.text()).toBe(content.toString().substr(0, 11));
});

// L1203-1227: 416 + full body
test('range request not satisfiable', async () => {
  const name = 'docs.md';
  const content = await fs.readFile(path.join(fixturesFull, name));
  const url = await getUrl();
  const response = await fetch(`${url}/${name}`, {
    headers: { Range: 'bytes=10-1' }     // start > end
  });
  expect(response.headers.get('content-range')).toBe(`bytes */${content.length}`);
  expect(Number(response.headers.get('content-length'))).toBe(content.length);
  expect(response.status).toBe(416);
  expect(await response.text()).toBe(content.toString());   // FULL BODY
});

// L1130-1175: stats.size === null → Range ignored, 200 full body
// (Degenerate case for irserve; documented for context.)
```

### Current irserve hook point — `crates/irserve-core/src/dispatch.rs:758-812`

```rust
fn build_file_or_304(
    serve_config: &ServeConfig,
    req_headers: &HeaderMap,
    path: &Path,
    bytes: Vec<u8>,
    meta: Option<&Metadata>,
    header_rules: &[HeaderRuleCompiled],
    request_path: &str,
) -> Response<Body> {
    let etag = etag_value(serve_config, path, &bytes);
    let last_modified = last_modified_value(serve_config, meta);
    let response_200 = file_response(path, bytes, etag, last_modified);
    let merged = apply_custom_headers(response_200, request_path, header_rules);
    if req_headers.get(RANGE).is_none() {
        // ETag/INM 304 path (7a) ...
        // Last-Modified/IMS 304 path (7b, D-018) ...
    }
    merged                                            // <-- 7c REPLACES this fall-through
}
```

The `else` of the Range-absent guard currently falls through to
`merged` (a 200 with the full file body). **7c replaces this
fall-through with a `range::apply` call** that mutates the
merged response into either 206 (in-range) or 416 (out-of-range,
full body retained). Crucially, the range logic runs AFTER
`apply_custom_headers` (mirroring reference's
`getHeaders`-then-range ordering at L746/L749).

## Decisions taken with the user (AskUserQuestion this session)

- **D1 — 416 body is the full file (mirror reference).** No
  D-NNN. The existing snapshot already pins this on the
  reference side; promoting `out_of_range` to `runner.l0.clean`
  validates byte-equality across both targets. RFC 7233 §4.4
  permits a representation on 416; the reference takes that
  path, and we mirror.

- **D2 — Manual `Range` parser in `crates/irserve-core/src/range.rs`.**
  No new crate. Grammar: `bytes=<s>-<e>` / `bytes=<s>-` /
  `bytes=-<n>`. Returns a small enum `Range::{InRange{start,
  end}, Unsatisfiable}` (NOT bytes-unit → `Unsatisfiable`;
  multiple ranges → `InRange` of first only, see Slice 1).
  Estimated ~80 lines + ~15 unit tests. Mirrors reference's
  `range-parser` behavior at the granularity we care about
  (single range, bytes unit, inclusive end, satisfiability
  bound).

- **D3 — Moderate edge-probe extension in `range-request.json`.**
  Add to the existing 3 anchors: `suffix_last_3` (`bytes=-3`),
  `single_byte` (`bytes=0-0`), `clip_to_end` (`bytes=8-999`,
  partial-overlap — reference clips to file end per
  `range-parser` semantics), `range_on_missing_file` (404
  sanity), `range_with_inm_match` (Range pre-empts 304 — guard
  already in place at L771, this probe pins it under both
  targets). 5 new anchors → 8 total. Closes anti-hallucination
  #8 zone proactively.

## Decisions taken without asking (small technical points)

- **Module layout.** New `crates/irserve-core/src/range.rs`
  exposing:
  ```rust
  pub enum RangeOutcome { InRange { start: u64, end: u64 }, Unsatisfiable }
  pub fn parse_range(value: &str, total: u64) -> Option<RangeOutcome>;
  pub fn apply(merged: Response<Body>, range_header: &HeaderValue, bytes: &[u8], total: u64) -> Response<Body>;
  ```
  `parse_range` returns `None` only when the input doesn't
  start with `bytes=` (mirrors reference: non-bytes unit goes
  through `range-parser`'s `-1` fall-through). All malformed
  / non-satisfiable cases return `Some(Unsatisfiable)` →
  416 path. **Note:** the `apply` helper needs access to the
  ORIGINAL full bytes (for the 206 slice AND for the 416
  full-body re-emission), so the call site in `dispatch.rs`
  keeps `bytes: Vec<u8>` available past the `file_response`
  build. Easiest shape: hand `&bytes` to `apply`, then drop.

- **206 response build.** Take the merged 200 response, swap
  status to `StatusCode::PARTIAL_CONTENT`, replace the body
  with `Body::from(bytes[start..=end].to_vec())`, and add
  `Content-Range: bytes <s>-<e>/<total>` + `Content-Length:
  <e-s+1>` headers via `.insert()` (last-write-wins over any
  user rule, mirroring reference's post-`getHeaders`
  injection). `accept-ranges` is NOT added — already in the
  L0 mask and not contractually required at L0 per ORC-044
  ("must-match: ... `accept-ranges: bytes`" is mask-level, not
  strict-byte; verify the existing snapshot vs. mask logic in
  Slice 0).

  *Wait — re-checking the snapshot at L34 and L62 of
  `tools/probe/snapshots/range-request.json`:*
  `"accept-ranges": "bytes"` IS in the reference response.
  Since `accept-ranges` is in `L0_EXTRA_VOLATILE_HEADERS`
  (already masked), irserve can choose to emit it or not
  without breaking the L0 compare. **Decision: irserve does
  NOT emit `accept-ranges`** — the contract is the 206/416
  status + `Content-Range` + `Content-Length`; `accept-ranges`
  is L1 cosmetic (analogous to `Vary`). If Codex pushes back,
  trivial to add.

- **416 response build.** Take the merged 200, swap status to
  `StatusCode::RANGE_NOT_SATISFIABLE` (416), keep the full
  body (the merged 200's `Body` already holds the full bytes;
  no re-clone needed), add `Content-Range: bytes */<total>`
  header. `Content-Length` is auto-derived by axum from
  `Body::from(bytes)` and will equal `<total>` — matches the
  reference snapshot.

- **`Content-Type` on 206/416.** Preserved from the merged
  200 response unchanged. Reference behavior: `Content-Type`
  comes from `getHeaders` and is not modified by the range
  branch. Mirroring: don't touch it.

- **ETag / Last-Modified on 206/416.** Preserved from the
  merged 200 unchanged. Reference: the 304 short-circuit is
  skipped (Range guard), but the validator headers are still
  on the response. We mirror.

- **Range value parsing — first value only.** If the
  `Range` header has multiple values (multi-value HTTP header,
  rare but possible), use the first via `HeaderValue::to_str`.
  Multi-range commas inside a single value: reference's
  `range-parser` accepts and returns all; we ignore beyond the
  first. Slice 1's `parse_range` peeks for a comma after the
  first range; if present, treats the input as the first range
  only (subsequent ranges silently ignored — mirrors our
  Out-of-Scope item).

- **D-NNN numbering.** None. The user picked Mirror in D1; no
  intentional divergence to record. If implementation surfaces
  an unavoidable divergence (e.g. `bytes=0--3` corner that
  manual parser handles differently from `range-parser`),
  raise during slice 1 and add D-019 then.

- **Plan-file numbering.** `docs/features/0017_PLAN_stage7c_range_requests.md`
  (continues the 0001..0016 sequence; 7b was 0016).

- **Change package.** `openspec/changes/013-range-requests/`
  per the roadmap row.

- **No CLI surface.** Range honoring is unconditional in the
  reference; no `--no-range` flag exists. irserve mirrors.

- **No `serve.json` surface.** No `range` key in the reference
  schema. irserve does not add one.

- **`If-Range` header.** Reference has `// TODO ? if-range` at
  L719 — explicitly unimplemented. We mirror: ignore `If-Range`.
  Listed in Out-of-Scope below.

## Files to touch

**New:**
- `crates/irserve-core/src/range.rs` — manual parser +
  `apply` helper. ~80 lines + ~15 unit tests.
- `openspec/changes/013-range-requests/proposal.md`
- `openspec/changes/013-range-requests/design.md`
- `openspec/changes/013-range-requests/tasks.md`
- `openspec/changes/013-range-requests/specs/http-cache/spec.md`
  — ADDED Requirement for SRV-CACHE-004; possibly MODIFIED
  on the Range-guard prose in the existing Last-Modified
  requirement (the "Stage 7c precursor" Scenario at
  `openspec/specs/http-cache/spec.md:280-290` can be
  generalized once Range emission lands — keep that Scenario
  but add a sibling for the active 206/416 surface).
- `docs/features/0017_PLAN_stage7c_range_requests.md` —
  implementation-time plan (mirrors `0016_PLAN_stage7b_*.md`
  structure: Context / Decisions taken with user / Decisions
  taken without asking / Files to touch / Slice plan / Out of
  scope / Hard stops / Verification).

**Modified:**
- `crates/irserve-core/src/lib.rs` — `mod range;`.
- `crates/irserve-core/src/dispatch.rs`:
  - Replace the fall-through `merged` at the end of
    `build_file_or_304` (~L811) with a Range-aware branch:
    when `req_headers.get(RANGE)` is `Some(v)`, call
    `range::apply(merged, v, &bytes, bytes.len() as u64)`;
    else return `merged`.
  - Plumb `bytes` (or `Bytes`) through past `file_response`
    so it's available to `range::apply`. Likely cleanest:
    clone `bytes` before handing to `file_response`, or
    refactor `file_response` to take `&[u8]` + return a
    builder that the caller finalizes. Decided in Slice 2.
  - ~8 new unit tests in `dispatch::tests`: in-range first/
    tail/suffix, out-of-range 416, range_on_404 (must NOT
    branch into range — 404 is structurally upstream of
    `build_file_or_304`; pin as sanity), range_with_inm_match
    (Range suppresses 304, returns 206 not 304),
    range_with_ims_match_under_no_etag (same, 7b interaction).
- `tools/probe/cases/range-request.json`:
  - Add 5 anchors (D3): `suffix_last_3`, `single_byte`,
    `clip_to_end`, `range_on_missing_file`,
    `range_with_inm_match`.
  - Add `runner.l0.clean` partition with all 8 anchor names.
  - Keep `volatileHeaders: ["last-modified"]` (already there).
- `tools/probe/snapshots/range-request.json`:
  - Re-record with `--target=reference --snapshot=update` to
    pin the 5 new anchors. Existing 3 stay byte-identical.
- `tools/probe/run.mjs`:
  - **Likely no change needed.** `accept-ranges` and `etag`
    are already in `L0_EXTRA_VOLATILE_HEADERS`. `content-range`
    is NOT in the mask — that's the contractual header. Verify
    in Slice 0 whether `content-length` mismatches between
    target=irserve and target=reference need a per-anchor
    `contentLengthMayDiffer` overlay (likely NOT — both
    targets emit the same value from the same `<e-s+1>` /
    `<total>` formula).
- `docs/reference/serve/inventory.md`:
  - SRV-CACHE-004 status stays `verified` (already).
  - Optional: append a "Probe: extended in Stage 7c with
    suffix/single-byte/clip/404/INM-interaction anchors" line
    under the Probe bullet.
- `docs/reference/serve/oracle-matrix.md`:
  - Existing ORC-044/045/046 flip from reference-only to
    dual-target (update the target column / verification
    column per the matrix's conventions; look at the
    7b ORC-167..172 pattern for precedent).
  - Add 5 new ORC rows for the new anchors (one per anchor),
    all dual-target.
- `README.md`:
  - Flip Stage 7c row to `done` in the stage map (line ~75).
  - Update the "Try IrServe (post-6h)" prose: add Range
    coverage to the listed capabilities; add a curl demo.
  - Trim the "What is NOT yet observable" footer (line ~307):
    remove `Range requests (206/416)`.

**Untouched (do not modify):**
- `third_party/serve`, `third_party/serve-handler` — pinned
  oracles.
- Existing snapshots for non-range cases (ETag, Last-Modified,
  redirects, etc.) — no reference-behavior change; stay
  byte-identical.
- `openspec/specs/http-cache/spec.md` is modified only
  through the change package's ADDED/MODIFIED delta and
  validated via `npx -y @fission-ai/openspec@latest validate
  --all --strict`.

## Slice plan

Four iterative slices, one commit per green-state slice.
**Ask the user before each `git commit`** (persistent memory
rule `feedback_iterative_commits`). Slice 3 is the meta slice
and lands last; delegate spec prose to a subagent per the
kickoff template.

### Slice 0 — Probe extension + reference snapshot pin

- Extend `tools/probe/cases/range-request.json` with 5 new
  anchors (per D3). Each anchor's `request` block follows the
  existing pattern.
- Add `runner.l0.clean: [in_range_first_4, in_range_tail,
  out_of_range, suffix_last_3, single_byte, clip_to_end,
  range_on_missing_file, range_with_inm_match]`. No `divergent`
  array (mirror only, per D1 + D-018 confirmation).
- Record snapshot: `node tools/probe/run.mjs range-request
  --target=reference --snapshot=update`.
- **Inspection step (mandatory):** read the updated snapshot
  and look for surprises:
  - Does `bytes=8-999` (clip_to_end) return 206 with
    `Content-Range: bytes 8-10/11` (clipped) or 416?
    Hypothesis: 206 with clipping, per `range-parser`'s
    standard behavior on a partially-overlapping range.
  - Does `bytes=-3` (suffix) return 206 with `Content-Range:
    bytes 8-10/11`? Hypothesis: yes.
  - Does `range_with_inm_match` return 206 (Range pre-empts
    INM)? Hypothesis: yes per reference L760 guard.
  - Does `range_on_missing_file` return 404 with the missing-
    file body (no range processing — 404 is structurally
    upstream)? Hypothesis: yes.
- Update `docs/reference/serve/oracle-matrix.md`: add 5 new
  ORC rows for the new anchors (still reference-only at this
  point — promoted to dual-target in slice 2).
- Verify: `node tools/probe/run.mjs range-request
  --target=reference --snapshot=verify` green; no Rust changes
  so `cargo` untouched.
- Commit: `probe(stage-7c): extend range-request fixture with edge cases (suffix, single-byte, clip, 404, INM-interaction)`

### Slice 1 — `range` parser module (no dispatch wiring)

- New `crates/irserve-core/src/range.rs`:
  - `pub enum RangeOutcome { InRange { start: u64, end: u64 }, Unsatisfiable }`
  - `pub fn parse_range(value: &str, total: u64) -> Option<RangeOutcome>`
    — returns `None` only on non-`bytes=` unit prefix
    (mirroring the reference's non-bytes fall-through;
    `Some(Unsatisfiable)` otherwise on malformed/empty).
  - `pub fn apply(merged: Response<Body>, range_header: &HeaderValue, bytes: &[u8], total: u64) -> Response<Body>` —
    parses, then either mutates `merged` into 206 (slice the
    body, set status, set headers) or 416 (keep body, set
    status, set `Content-Range`). Public so dispatch.rs can
    call it.
- `crates/irserve-core/src/lib.rs`: `mod range;`.
- Unit tests in `range::tests` (~15):
  - `parse: bytes=0-3` → `InRange{0, 3}`.
  - `parse: bytes=8-` → `InRange{8, total-1}` (e.g. `total=11` → `{8,10}`).
  - `parse: bytes=-3` (suffix) → `InRange{total-3, total-1}` = `{8,10}`.
  - `parse: bytes=0-0` (single byte) → `InRange{0,0}`.
  - `parse: bytes=8-999` (partial-overlap clip) → `InRange{8,10}`.
  - `parse: bytes=999-1000` (fully out) → `Unsatisfiable`.
  - `parse: bytes=10-1` (start > end) → `Unsatisfiable`.
  - `parse: bytes=abc` → `Unsatisfiable`.
  - `parse: bytes=` (empty after =) → `Unsatisfiable`.
  - `parse: bytes=0-3, 8-10` (multi-range) → `InRange{0,3}` (first only).
  - `parse: pixels=0-3` → `None` (non-bytes unit). [Hmm —
    actually per recon point 7, reference's `range-parser`
    returns `-1` for non-bytes which lands in the same
    `else` → 416 branch. So returning `Unsatisfiable` is
    closer to reference behavior. Let me flip:
    `parse: pixels=0-3` → `Unsatisfiable`. Then `parse_range`
    returns `None` ONLY when input is empty/non-string —
    rare in practice. Decided: keep `Option` for early-exit
    cleanliness but make every wire-malformed Range string
    return `Some(Unsatisfiable)` for behavioral mirror. The
    `apply` function then doesn't need to distinguish `None`
    from `Unsatisfiable` — `None` could be treated as "no
    range header" upstream, but at the apply layer it's
    indistinguishable from a parse failure. Revisit in
    implementation.]
  - `apply: Range: bytes=0-3 on 11-byte body` → 206 response
    with body `"abcd"`, `Content-Range: bytes 0-3/11`,
    `Content-Length: 4`.
  - `apply: Range: bytes=999-1000 on 11-byte body` → 416
    response with full body, `Content-Range: bytes */11`,
    `Content-Length: 11`.
  - `apply: Range: bytes=10-1 on 11-byte body` → 416 (start > end).
  - `apply preserves Content-Type from merged`.
  - `apply preserves user-rule headers from merged` (e.g.
    `X-Custom: yes` survives the 206 transformation).
- Verify: `cargo test --workspace` green. Oracle harness
  untouched (no dispatch wiring yet — the existing 3
  reference-only anchors stay reference-only, the 5 new
  ones added in slice 0 also stay reference-only).
- Commit: `feat(stage-7c): slice 1 — range parser module + apply helper (unit-tested, not yet wired)`

### Slice 2 — Wire `range::apply` into `build_file_or_304`

- `crates/irserve-core/src/dispatch.rs`:
  - At the existing fall-through site (~L811), replace
    `merged` with:
    ```rust
    match req_headers.get(RANGE) {
      Some(v) => range::apply(merged, v, &bytes, bytes.len() as u64),
      None => merged,
    }
    ```
    Wait — `bytes` was moved into `file_response`. Resolve
    by either:
    (a) cloning `bytes` before `file_response` (extra alloc,
        simple);
    (b) passing `&[u8]` into `file_response` and having it
        clone there (zero extra alloc at the dispatch-level);
    (c) refactoring to keep the `Vec<u8>` past
        `file_response` by returning the bytes back from a
        builder API.
    **Decide (a) for simplicity.** Range requests are the
    only ones that benefit, all other 200 responses don't need
    the clone. We can guard with `if req_headers.get(RANGE).is_some()
    { bytes.clone() } else { Vec::new() }` to skip the clone
    on the common path — but premature. Start with the simple
    clone-always shape, profile if it shows up.
  - Note: the Range branch runs AFTER the 304 short-circuit
    guards (the existing `if req_headers.get(RANGE).is_none() {
    ... }` block). Range requests never enter the 304 block,
    so they pass through unchanged to the fall-through where
    the new match lives. This mirrors reference's L760
    (Range suppresses 304) AND reference's L749 (range
    headers injected AFTER `getHeaders` merge).
- Promote the 8 anchors in `range-request.json` to dual-target:
  - The `runner.l0.clean: [...]` array was already added in
    slice 0, but slice 0 ran `--target=reference` only. Now
    run `node tools/probe/run.mjs range-request --target=irserve
    --snapshot=verify` and confirm green.
  - If `content-length` mismatches surface (unlikely — same
    formula on both sides), add a per-anchor
    `contentLengthMayDiffer` overlay. If status mismatches
    surface (e.g. irserve emits 200 where reference emits 416
    on `bytes=10-1`), debug — likely a parser corner case.
- ~8 new unit tests in `dispatch::tests`:
  - `range_in_range_returns_206_with_partial_body`.
  - `range_tail_returns_206`.
  - `range_suffix_returns_206`.
  - `range_out_of_range_returns_416_with_full_body`.
  - `range_with_etag_inm_match_returns_206_not_304` (Range
    pre-empts 304 — guard mirror).
  - `range_with_ims_match_under_no_etag_returns_206_not_304`
    (same, 7b interaction).
  - `range_preserves_user_custom_headers`.
  - `range_preserves_etag_on_206`.
- Update `docs/reference/serve/oracle-matrix.md`: flip
  ORC-044/045/046 + the 5 new ORC rows from reference-only
  to dual-target. Cite slice-2 promotion.
- Verify:
  - `cargo test --workspace` green.
  - `node tools/probe/run.mjs --all --target=reference --snapshot=verify` green.
  - `node tools/probe/run.mjs --all --target=irserve --snapshot=verify` green; total goes up by 8 (the 3 original promoted + 5 new).
- Commit: `feat(stage-7c): slice 2 — emit 206/416 on Range requests`

### Slice 3 — Spec deltas + meta (delegated)

Per the kickoff template's "spec prose subagent" trigger,
delegate authoring of:
- `openspec/changes/013-range-requests/proposal.md`
- `openspec/changes/013-range-requests/design.md`
- `openspec/changes/013-range-requests/tasks.md`
- `openspec/changes/013-range-requests/specs/http-cache/spec.md`
  (ADDED Requirement for SRV-CACHE-004; MODIFIED note on the
  Range-precursor Scenario at
  `openspec/specs/http-cache/spec.md:280-290` to reference
  the live 206/416 surface instead of the deferred precursor)

Briefing for the subagent: slice plan from this file + commit
log from slices 0–2 + the relevant `D-018` framing for the
Range guard (already in 7b spec) + the `012-last-modified/`
package as style precedent. Subagent does NOT need the full
implementation conversation.

Main agent reviews and Edits if needed.

Also in slice 3 (main agent, not subagent):
- **No D-NNN entry** — D1 chose mirror, no divergence.
- `README.md`: flip Stage 7c row to `done`; trim "What is NOT
  yet observable" footer (drop "Range requests (`206`/`416`)"
  — leave Cache-Control default, full L3 CORS preflight, gzip,
  symlinks, TLS); add a Range curl demo to "Try IrServe":
  ```bash
  # Range requests (Stage 7c, SRV-CACHE-004).
  curl -i -H 'Range: bytes=0-3' http://127.0.0.1:3010/asset.css
  # 206 Partial Content; content-range: bytes 0-3/16; body: 4 bytes
  curl -i -H 'Range: bytes=-4' http://127.0.0.1:3010/asset.css
  # 206; suffix form returns last 4 bytes
  curl -i -H 'Range: bytes=999-1000' http://127.0.0.1:3010/asset.css
  # 416 Range Not Satisfiable; content-range: bytes */16; body: full file
  ```
- Run `npx -y @fission-ai/openspec@latest validate --all --strict`
  and report any failures (will fail until the change package
  exists; expected pre-commit, green post-commit).
- Commit (after main-agent review of subagent output):
  `docs(stage-7c): spec deltas + oracle-matrix promotion + meta`

### Codex review rounds

Per the persistent memory rule `feedback_review_rounds`: user
hands the diff to Codex; each round of fixes lands as one
commit titled
`docs(stage-7c): address Codex review round N (P{priorities} fixes)`.
Push back if disagreement — pushback is expected.

If review rounds keep landing on the same fine-grained aspect
(e.g. parser-corner semantics, suffix form, `Range` header
multi-value handling) for **three rounds running**, pause per
anti-hallucination rule #9: declare parity scope, document
divergences in the spec Compatibility note, ask the user
before round 4.

## Out of scope for Stage 7c (mandatory pre-stage list per anti-hallucination #10)

The following are NOT addressed in 7c. Discovery of any of
these in review rounds is a signal that this list was
under-specified; discuss before patching.

1. **Multiple ranges (`Range: bytes=0-3, 8-10`).** Reference
   has `// TODO ? multiple ranges` at L736 and uses only
   `range[0]`. irserve mirrors: first range only, subsequent
   ignored. No `multipart/byteranges` response shape.

2. **`If-Range` header.** Reference has `// TODO ? if-range`
   at L719 — unimplemented. irserve mirrors: `If-Range` is
   ignored. RFC 7233 §3.2 conditional-range semantics deferred.

3. **`Accept-Ranges: bytes` response header on all file
   responses.** Reference emits it (visible in the snapshot)
   but it's already in irserve's `L0_EXTRA_VOLATILE_HEADERS`
   mask. irserve does NOT emit it (per "Decisions taken
   without asking"). If a follow-up sub-stage promotes
   `Accept-Ranges` to contractual, that's a separate change.

4. **Range support beyond static files.** Directory listings,
   3xx redirects, JSON error responses, custom HTML error
   pages: none of these honor `Range`. Same surface as
   ETag/Last-Modified in 7a/7b. The `Range` header is silently
   ignored on these paths (no 206/416 emission).

5. **Range on 404.** A `Range`-bearing request for a missing
   file returns 404, not 416 + 404. The 404 path is
   structurally upstream of `build_file_or_304`; Range parsing
   never runs. Pinned by the `range_on_missing_file` probe.

6. **`Content-Length` on 416 omission strategy.** Reference's
   416 path doesn't explicitly set `Content-Length`, but
   Node's http response infers it from the full body length
   → `content-length: 11`. axum's `Body::from(bytes)` does
   the same. Wire-shape matches; the internal "did we set it
   explicitly" question is non-contractual.

7. **Range on POST / PUT / DELETE.** Reference serves only
   GET/HEAD on static files; non-GET methods produce 405 or
   pass through. Range on a 405 is not a thing. No probe.

8. **Range header with garbage AFTER a valid first range
   (`bytes=0-3, garbage`).** Reference's `range-parser`
   either accepts the first and ignores garbage, or rejects
   wholesale. Manual parser: split on first `,`, parse the
   first segment, ignore the rest. Mirrors the "first range
   only" semantics. Edge probe in Slice 0 may add this if
   easy; otherwise out-of-scope for this stage.

9. **Negative-zero suffix (`bytes=-0`).** RFC 7233 §2.1
   says "If the selected representation is shorter than the
   specified suffix-length, the entire representation is used".
   `range-parser` returns `Unsatisfiable` for `bytes=-0`.
   Manual parser mirrors. No edge probe (rare corner; ask
   reviewer if surfaced).

10. **`Vary: Range` response header.** Reference does NOT
    emit. irserve mirrors. RFC suggests `Vary: Range` for
    cacheability with intermediaries, but neither side adds
    it.

11. **HTTP/2 / HTTP/3 range semantics.** irserve serves
    HTTP/1.1 only (axum default). HTTP/2 multiplexed range
    requests are out of scope.

12. **Range on HEAD.** Reference: HEAD requests reuse the GET
    pipeline and emit headers without body, so `Range: bytes=0-3`
    on HEAD would emit `Content-Range: bytes 0-3/N` and
    `Content-Length: 4` with no body. We currently do not
    have a HEAD-specific test surface; whether axum strips
    the body automatically on HEAD is to be verified. If
    surfaced in review, add a probe and either confirm match
    or record divergence.

## Hard stops (kickoff template §"Hard stops")

- Do not modify `third_party/`.
- Existing snapshots (ETag, Last-Modified, redirects, etc.)
  touched only if reference behavior changed — it has not.
  The 7a/7b snapshots stay byte-identical.
- The 3 existing `range-request.json` anchors' snapshots
  also stay byte-identical (only the 5 new ones are recorded
  in slice 0).
- Contract changes only through MOD deltas after explicit
  user discussion. No D-NNN entries planned for 7c (D1 chose
  mirror).
- Do not commit without explicit permission. Propose when a
  slice is green and ready.

## Verification (end-to-end)

After all slices land:

```bash
# Workspace tests — including new range::tests and
# dispatch::tests for range branches.
cargo test --workspace

# Oracle harness — both targets green.
node tools/probe/run.mjs --all --target=reference --snapshot=verify
node tools/probe/run.mjs --all --target=irserve --snapshot=verify
# Expected: target=irserve total goes up by ~8 (3 existing
# range anchors promoted + 5 new edge anchors added).

# OpenSpec contract validation.
npx -y @fission-ai/openspec@latest validate --all --strict
```

Manual smoke test (mirrors what lands in README "Try IrServe"):

```bash
mkdir -p _tmp && echo 'body{color:red}' > _tmp/asset.css  # 16 bytes incl. LF

cargo run -- --listen 3010 _tmp &

# In-range first 4 bytes
curl -i -H 'Range: bytes=0-3' http://127.0.0.1:3010/asset.css
# Expect: 206 Partial Content
#   content-range: bytes 0-3/16
#   content-length: 4
#   body: "body"

# Suffix form (last 4 bytes)
curl -i -H 'Range: bytes=-4' http://127.0.0.1:3010/asset.css
# Expect: 206
#   content-range: bytes 12-15/16
#   content-length: 4
#   body: "ed}\n"

# Out of range
curl -i -H 'Range: bytes=999-1000' http://127.0.0.1:3010/asset.css
# Expect: 416 Range Not Satisfiable
#   content-range: bytes */16
#   content-length: 16
#   body: full file contents

# Range pre-empts 304 (even with matching ETag)
ETAG=$(curl -sI http://127.0.0.1:3010/asset.css | grep -i ^etag: | cut -d' ' -f2- | tr -d '\r')
curl -i -H "If-None-Match: $ETAG" -H 'Range: bytes=0-3' http://127.0.0.1:3010/asset.css
# Expect: 206 (NOT 304); the Range guard at dispatch.rs:771 suppresses the 304 short-circuit.
```

## Estimated effort

Per `docs/stage7_l3_capabilities.md`'s "Estimated effort"
table: 7c is in the 1-plan + 1-implementation + 1-review
bucket (~60k tokens). Three implementation slices (0, 1, 2)
plus one meta slice (3, delegated). Slice 0 is small (probe
extension, 1-2 hours of clock time). Slice 1 is pure module
code (3-4 hours, mostly tests). Slice 2 is the wiring +
integration tests (2-3 hours). Slice 3 is delegated and
reviewed (~1 hour main-agent time).
