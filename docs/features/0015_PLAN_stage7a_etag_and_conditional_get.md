# Stage 7a — ETag + 304 conditional GET

## Context

Stage 7a is the first L3 sub-stage per `docs/stage7_l3_capabilities.md` and is "Next" in the README stage map. It delivers a single SRV — **SRV-CACHE-001** (P0): file responses carry a strong `ETag` header by default, and a request with matching `If-None-Match` short-circuits to `304 Not Modified` with no body. Stages 7b (Last-Modified + `--no-etag` + If-Modified-Since) and 7c (Range) build on the same response-emission slot, so the structural decisions here propagate.

Today: `etag: Option<bool>` is already parsed in `ServeConfig` (`crates/irserve-core/src/config.rs:48`) but never read. `file_response` at `crates/irserve-core/src/dispatch.rs:706-714` sets only `Content-Type`. The probe `etag-roundtrip.json` exists with a static `If-None-Match` value pinned to the reference's sha1 (`"3638b78821a961fcf35969f0bc67cc5944d64a0b"`); under `target=irserve` this would never match a freshly-computed hash, so ORC-043 cannot flip to dual-target without a probe-runner change.

Deliverable: an OpenSpec change package `openspec/changes/011-etag-conditional/`, a new capability spec `openspec/specs/http-cache/spec.md`, a `D-017` decisions entry codifying the hash choice and divergence from spec ID-only contract, the wired implementation in `irserve-core`, and an extended probe runner that supports cross-request value reuse so ORC-042/043 both run under both targets.

## Decisions taken with the user

| # | Decision | Rationale |
|---|---|---|
| D1 | **Hash function: `sha1(extname + '-' + fileContents)`** — exact mirror of `serve-handler/src/index.js:24-36`. | Capture-replay neutralises the value-divergence argument; mirroring keeps the spec one sentence and the contract surface minimum. sha1 in this use-case (ETag, not security) is fine. Still recorded as **D-017** since hash choice is implementation-defined per inventory note. |
| D2 | **Probe round-trip: capture-replay in `tools/probe/run.mjs`.** Extend case schema so a request can reference a header from a prior response (e.g. `"if-none-match": {"$fromResponse": {"request": "first_get", "header": "etag"}}`). | Universal — works for ref and irserve identically. Reusable for 7b (If-Modified-Since), 7c (If-Range). One investment, multi-stage payback. |
| D3 | **`etag` default when unset in `serve.json`: `true`** — mirror `vercel/serve` CLI default (`!args['--no-etag']` at `vercel/serve/source/main.ts`). | irServe IS the CLI; SRV-CACHE-001 inventory entry already states "ETag is sent by default"; flipping to false would contradict that. |

## Decisions taken without asking (small technical points)

1. **ETag computation point: in dispatcher, before `file_response`.** Dispatcher reads `If-None-Match` from `req.headers()`, computes ETag from `bytes` once, decides 200 vs 304. Avoids a wasted `Response` allocation and a header-lookup-then-rewrite when 304. `file_response` gets a new `etag: Option<&str>` parameter (set when `serve_config.etag != Some(false)`, else `None`).
2. **ETag format: strong, double-quoted hex** — `"<40 hex chars>"`. Mirrors reference exactly (`src/index.js:233`).
3. **Range guard preemptively in place.** Per `src/index.js:760` the 304 check is skipped when a `Range` header is present. We do not parse Range yet (7c), but the one-line `if req.headers().get(RANGE).is_none() && …` guard goes in now so 7c does not have to revisit this code.
4. **renderSingle path also gets ETag.** `dispatch.rs:430` (the `--render-single` short-circuit) calls `file_response` too — treat uniformly with regular file responses. Reference does the same.
5. **ETag NOT applied to error/listing/redirect branches in 7a.** Reference applies ETag to custom HTML error pages when `etag=true` (`src/index.js:508`). We defer that to a follow-up and document the divergence as a Note in the spec delta — neither SRV-CACHE-001 scenarios nor ORC-042/043 mandate ETag on error pages, and keeping 7a scope tight reduces blast radius. Directory listings, JSON errors, and 3xx never get ETag in either implementation.
6. **`apply_custom_headers` is NOT modified.** ETag is set in `file_response` before the wrapper at `dispatch.rs:54-70` calls `apply_custom_headers`. Per reference (`Object.assign(defaultHeaders, related)` at `src/index.js:241`), user `headers` rules CAN override the default ETag. Our existing post-merge already gives this behavior for free.
7. **In-memory ETag cache (reference uses `Map<absPath, [mtime, sha]>`): NOT implemented in 7a.** Hash on every request. Files are already buffered (`tokio::fs::read`); cost is bounded by file size, and the project hasn't surfaced a perf signal. Note in `D-017`: caching is a future optimisation, not contractual.
8. **Plan file: `docs/features/0015_PLAN_stage7a_etag_and_conditional_get.md`.** Next number per the `0001..0014` convention.

## Files to touch

### Primary code
- `crates/irserve-core/src/dispatch.rs`
  - Lines 320-334: 200/304 decision before calling `file_response` (File/Index arm).
  - Line 430: same logic for renderSingle.
  - Lines 706-714: `file_response` signature gains `etag: Option<HeaderValue>`; inserts header when `Some`.
- `crates/irserve-core/src/lib.rs`: add `mod etag;` (new module).
- `crates/irserve-core/src/etag.rs` **(new)**: pure function `compute_etag(path: &Path, bytes: &[u8]) -> String` returning `"\"<40 hex>\""`. Uses `sha1` crate streamed over `extname.as_bytes()`, then `b"-"`, then `bytes`. Unit tests pin the value `"\"3638b78821a961fcf35969f0bc67cc5944d64a0b\""` against fixture `body{color:red}\n` named `asset.css` (the same fixture the reference snapshot uses — proves byte-equality with reference).
- `crates/irserve-core/Cargo.toml`: add `sha1 = "0.10"` dep.
- No CLI changes (defer `--no-etag` to 7b per roadmap).

### Probe harness
- `tools/probe/run.mjs`:
  - Extend the request shape: header values may be either a string OR an object `{"$fromResponse": {"request": "<name>", "header": "<lowercase-header-name>"}}`. Resolve lazily: when issuing request N, look up the captured response for the named prior request and substitute the header value (verbatim string, including quotes). If the named request has no recorded response yet, fail loudly with the case id.
  - Document the extension in `tools/probe/README.md` (one short section, mirroring the existing case-schema notes).
  - The capture step is target-agnostic — works for `target=reference` and `target=irserve` identically.
- `tools/probe/cases/etag-roundtrip.json`:
  - Replace the static `if-none-match: "\"3638b78...\""` with the `$fromResponse` reference. Keep `first_get` unchanged.
- `tools/probe/cases/etag-conditional.json`:
  - Keep as-is (it tests `If-None-Match: "\"deadbeef\""` — the bogus-value branch — which is the same shape in both implementations: 200 OK).
- Re-record snapshots via `--snapshot=update --target=reference` (proves the runner change is value-preserving against reference) — touching the two etag cases only; nothing else.

### OpenSpec / docs
- `openspec/changes/011-etag-conditional/` **(new)**:
  - `proposal.md`, `design.md`, `tasks.md`.
  - Spec delta: ADD `openspec/specs/http-cache/spec.md` (capability does not exist yet — first L3 capability). One requirement keyed to SRV-CACHE-001 with two scenarios (200 + ETag present; 304 on matching `If-None-Match`). Compatibility note: hash function is implementation-defined; irServe mirrors `sha1(extname+'-'+contents)` per D-017 but values are not part of the contract.
- `docs/reference/serve/decisions.md`: append **D-017** — "ETag hash function: mirror reference (sha1 of extname + '-' + file contents), strong quoted format. Round-trip behavior is the contract; in-memory mtime-keyed cache is a future optimisation."
- `docs/reference/serve/oracle-matrix.md`: flip ORC-042 and ORC-043 from `verified` to dual-target (the `Coverage` column gains a note that both rows now run under `target=irserve` and `target=reference`). No new ORCs.
- `docs/features/0015_PLAN_stage7a_etag_and_conditional_get.md` **(new)**: slice-level implementation plan (mirroring `0014_PLAN_stage6h_cli_fill_in.md` style — slice list, what each slice commits, what stays out of scope).
- `README.md`: 7a row → `done` only after Codex review rounds resolve. Done as part of the meta slice.

## Slice plan

Iterative, one green commit per slice (per memory: ask before each `git commit`).

1. **Slice 1 — `etag` module + unit tests.** Add `crates/irserve-core/src/etag.rs` with `compute_etag` + `sha1` dep. Unit test pins the byte-equality with reference for the `asset.css` fixture. No wiring yet. `cargo test -p irserve-core` green.
2. **Slice 2 — wire `file_response` + 200 emission.** Modify `file_response` signature, both call sites (File/Index + renderSingle) read `serve_config.etag` (default true), compute ETag, pass into `file_response`. No 304 logic yet. Existing oracle tests stay green (ETag is in `L0_EXTRA_VOLATILE_HEADERS`, so value is masked). Snapshot under `target=irserve`: ETag present but value masked — verified via `cargo test --test oracle`.
3. **Slice 3 — 304 short-circuit.** In both call sites: read `If-None-Match`; if `Range` absent AND ETag matches the request header verbatim, return a fresh 304 `Response` (no body, no `Content-Type`, no `ETag` echo — mirrors `src/index.js:761-764`). Unit tests in `etag.rs` or a new `#[cfg(test)] mod tests` in `dispatch.rs` cover: (a) match → 304, (b) mismatch → 200, (c) Range present + match → still 200 (precursor for 7c).
4. **Slice 4 — probe runner capture-replay.** Extend `run.mjs` request schema + resolver. Update `etag-roundtrip.json` to use `$fromResponse`. Re-record reference snapshot for that case to confirm the runner change is value-preserving. `node tools/probe/run.mjs etag-roundtrip --target=reference --snapshot=verify` green; same with `--target=irserve` green (now ORC-043 actually verifies 304).
5. **Slice 5 — spec / proposal / D-017 / oracle-matrix flip / plan file / docs.** Delegated to a subagent per the kickoff-template subagent-delegation pattern. Briefing: this plan file + slice 1-4 commit log + reference change `010-cli-fill-in` for style. Main agent reviews and edits if needed.

After slice 5 lands, hand to Codex; each review round → one commit titled `docs(stage-7a): address Codex review round N (P{priorities} fixes)`.

## Out of scope for Stage 7a (mandatory pre-stage list per anti-hallucination #10)

These will NOT land in 7a; if any surfaces in review as a "missing feature," the response is "deferred to <stage> by plan, file an issue":

- `--no-etag` CLI flag and `Last-Modified` emission — **7b**.
- `If-Modified-Since` handling (Q-009 closure) — **7b**.
- ETag on custom HTML error pages — out of scope; the reference does it (`src/index.js:508`), we defer; documented as a Compatibility note.
- Range header parsing and 206/416 — **7c**. Only the `Range`-absent guard for the 304 check lands now.
- In-memory ETag cache (mtime-keyed). Hash-on-each-request only.
- Weak ETags (`W/"..."`). Reference does not emit weak ETags.
- Comma-separated `If-None-Match` lists, wildcard `*`. Reference does literal string equality only (`src/index.js:760`); we mirror.

## Verification

End-to-end:

```powershell
# Unit tests for the etag module and dispatcher 304 logic
cargo test -p irserve-core etag

# Oracle harness — must stay green for all cases, including
# etag-roundtrip and etag-conditional under target=irserve
cargo test -p irserve --test oracle

# Manual smoke (after slice 3): expect 200+ETag, then 304 on replay
cargo run -- --listen 3010 _tmp &
ETAG=$(curl -sI http://127.0.0.1:3010/asset.css | grep -i '^etag:' | awk '{print $2}' | tr -d '\r')
curl -i -H "If-None-Match: $ETAG" http://127.0.0.1:3010/asset.css   # expect 304
curl -i -H 'If-None-Match: "deadbeef"' http://127.0.0.1:3010/asset.css  # expect 200
```

The unit test in `etag.rs` proves the implementation produces the **same** sha1 string as the reference's pinned snapshot, eliminating one class of "but does it really mirror" review pushback in advance.

## OpenSpec validation

```powershell
npx -y @fission-ai/openspec@latest validate --all --strict
```

Runs as part of slice 5. Must pass before opening the diff for Codex review.
