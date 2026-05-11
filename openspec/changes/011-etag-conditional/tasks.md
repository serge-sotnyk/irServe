# Tasks: ETag + 304 conditional GET

Five iterative slices, one commit per green slice. Slice 5 is the
meta slice and lands last so spec deltas reflect what was actually
shipped.

## Slice 1 — `etag` module + unit tests

- [x] New module `crates/irserve-core/src/etag.rs` exposing
  `compute_etag(path: &Path, bytes: &[u8]) -> String` returning
  the strong-quoted form `"\"<40 hex>\""`.
- [x] Hash is `sha1(extname.as_bytes() ++ b"-" ++ bytes)` —
  exact mirror of `serve-handler/src/index.js:24-36`.
- [x] `crates/irserve-core/Cargo.toml` gains `sha1 = "0.10"`.
- [x] `crates/irserve-core/src/lib.rs` declares `mod etag;`.
- [x] Unit tests pin the value
  `"\"3638b78821a961fcf35969f0bc67cc5944d64a0b\""` for fixture
  `asset.css` with body `body{color:red}\n` (byte-equality with
  the reference snapshot used by `etag-roundtrip.json`).
- [x] Verify: `cargo test -p irserve-core etag` green; oracle
  harness untouched.
- Commit: `feat(stage-7a): slice 1 — etag module mirroring reference formula`
  (9521ec4).

## Slice 2 — Wire `file_response` + 200 emission

- [x] `file_response` at `crates/irserve-core/src/dispatch.rs`
  gains an `etag: Option<HeaderValue>` parameter; inserts the
  `ETag` header when `Some`.
- [x] `etag_value(serve_config, path, bytes) -> Option<HeaderValue>`
  helper returns `None` when `serve_config.etag == Some(false)`
  (the only disabling form); otherwise `Some(compute_etag(...))`.
  Mirrors `vercel/serve`'s CLI default
  (`config.etag = !args['--no-etag']` at
  `third_party/serve/source/main.ts`).
- [x] Both file-response call sites updated: the standard
  File/Index arm at `dispatch.rs:320`-ish and the renderSingle
  short-circuit at `dispatch.rs:430`-ish.
- [x] No 304 logic yet. ETag present on 200 responses; existing
  oracle tests stay green because `ETag` is in
  `L0_EXTRA_VOLATILE_HEADERS` (value masked at verify).
- [x] Verify: `cargo test --workspace` green;
  `cargo test --test oracle` green under both targets.
- Commit: `feat(stage-7a): slice 2 — emit ETag header on 200 file responses`
  (63330cd).

## Slice 3 — 304 short-circuit

- [x] New `build_file_or_304(serve_config, req_headers, path,
  bytes, header_rules, request_path) -> Response<Body>` helper
  at `dispatch.rs:687`. Builds the candidate 200 (via
  `file_response`), runs `apply_custom_headers` on it, then
  fires 304 iff (a) the merged response carries an `ETag` header
  (i.e. either the default was emitted or a user rule supplied
  one), (b) no `Range` header (Stage 7c precursor guard, mirrors
  reference's `req.headers.range` check at
  `serve-handler/src/index.js:760`), (c) `If-None-Match` matches
  the MERGED `ETag` verbatim. Codex review round 1 P1 reshaped
  this to match reference's `getHeaders`-then-304-check ordering
  at `index.js:241, 760`.
- [x] 304 response: no body, no `Content-Type`, no `ETag` echo.
  Mirrors `serve-handler/src/index.js:761-764`.
- [x] Both call sites at `dispatch.rs:320` and `:430` switched
  to `build_file_or_304` and return `None` for the outer
  wrapper's `headers_path` slot (custom-headers pass is now
  inside `build_file_or_304`).
- [x] Eight unit tests in `dispatch::tests` after Codex rounds
  1 and 2 (originally five in slice 3; round 1 added two
  override tests, round 2 added one for `etag: false` + user
  rule combination): match → 304, mismatch → 200, Range present
  + match → 200 (7c precursor), ETag default-disabled → no
  default + no 304, no `If-None-Match` → 200 + ETag, custom
  `ETag: "custom"` rule drives the 304 decision (round 1 P1),
  `ETag: null` delete suppresses 304 (round 1 P1, SRV-HDR-002),
  `etag: false` config + user `ETag` rule still 304s on replay
  of the user-supplied value (round 2 P3).
- [x] Verify: `cargo test -p irserve-core dispatch::tests`
  green (covers all eight; note that `dispatch::tests::etag`
  with the `::etag` narrowing only matches the four
  `etag_*`-prefixed tests, not the full surface);
  `cargo test --test oracle` still green under both targets.
- Commit: `feat(stage-7a): slice 3 — 304 short-circuit on If-None-Match match`
  (7d32663). Round 1 P1 follow-up restructured the ordering;
  see `4745014`.

## Slice 4 — Probe runner capture-replay

- [x] Extend `tools/probe/run.mjs` and the case schema: request
  header values may be either a string or an object
  `{"$fromResponse": {"request": "<name>", "header": "<lc>"}}`.
  Resolution is lazy at request-issue time; the runner fails
  loudly with the case id when the named request has no
  recorded response yet. Target-agnostic.
- [x] Update `tools/probe/cases/etag-roundtrip.json`: the
  second request's `If-None-Match` references the first
  request's `etag` via `$fromResponse`. The static hex literal
  is removed.
- [x] Add `runner.l0.clean` partitions to both ETag probes
  (`etag-roundtrip.json` and `etag-conditional.json`) so the
  runner runs them under `target=irserve` as well as
  `target=reference`.
- [x] Re-record the reference snapshot for `etag-roundtrip`
  once (the only field that changes is the resolved
  `if-none-match` value, now mirroring the captured first-GET
  ETag rather than a hand-typed literal).
- [x] Document the extension in `tools/probe/README.md`.
- [x] Verify: `node tools/probe/run.mjs etag-roundtrip
  --target=reference --snapshot=verify` green; same under
  `--target=irserve`; full oracle harness 75 / 6 skipped / 0
  failed under `target=irserve` (up from 73 / 8).
- Commit: `feat(stage-7a): slice 4 — probe runner capture-replay for round-trip cases`
  (1b7758c).

## Slice 5 — Spec deltas + meta (this slice)

- [x] Author `openspec/changes/011-etag-conditional/`:
  - [x] `proposal.md` — closes SRV-CACHE-001; out-of-scope list
    mirrors the kickoff plan's 8-item pre-stage list.
  - [x] `design.md` — where ETag is computed, the
    `build_file_or_304` helper, the Range guard (7c precursor),
    why ETag is NOT applied to listings/redirects/errors,
    user-rule override mechanics, probe runner capture-replay,
    methodological signals.
  - [x] `tasks.md` (this file).
  - [x] `specs/http-cache/spec.md` — ADDED Requirement keyed to
    SRV-CACHE-001 with two scenarios (200 + ETag; 304 on
    matching `If-None-Match`) and a Compatibility note pinning
    the round-trip-is-the-contract framing per D-017.
- [x] Mirror the delta into `openspec/specs/http-cache/spec.md`
  (first L3 capability shipped; this is the FINAL spec for the
  new capability after the ADDED delta is merged).
- [x] New **D-017** entry in `docs/reference/serve/decisions.md`:
  hash function = sha1 of `extname + '-' + contents`; round-trip
  behavior is the contract, exact hex value is not;
  in-memory mtime-keyed cache is a future optimization.
- [x] `docs/reference/serve/oracle-matrix.md`: flip ORC-042 and
  ORC-043 to dual-target coverage; cross-link D-017.
- [x] `docs/reference/serve/inventory.md`: no edit — SRV-CACHE-001
  is already `verified` from Stage 1 evidence; the Compatibility
  note already anticipates the irserve-side surface that 7a
  delivers.
- [x] `README.md`: flip 7a row to `done`; drop `ETag` from the
  "What is NOT yet observable" footer; add an ETag / 304
  round-trip curl demo to "Try IrServe".
- [x] Run `npx -y @fission-ai/openspec@latest validate --all
  --strict` and report any failures.
- Commit (pending main agent review): `docs(stage-7a): spec deltas + D-017 + oracle-matrix flip + meta`.

## Validation

- `cargo test --workspace` — green (slices 1-4).
- Oracle harness: `target=irserve total=81 passed=75 skipped=6
  failed=0`; reference 81/81 via `--snapshot=verify` (the two
  promoted requests are ORC-042 / ORC-043 from
  `etag-roundtrip.json`).
- Unit tests added across slices: 4 (etag.rs — byte-equality
  pin, extname-affects-hash, determinism, empty-body) +
  5 (dispatch::tests — 304 decision corners) = 9 new tests.
- `openspec validate --all --strict` — green (slice 5 final
  step).
