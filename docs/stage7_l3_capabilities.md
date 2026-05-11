# Stage 7 — L3 capabilities (sub-stage breakdown)

This document is the canonical decomposition of Stage 7 into independently
deliverable sub-stages 7a–7e. Stage 7 closes L3 (HTTP polish: cache surface,
range requests, CORS preflight, compression) per [`README.md`](../README.md)
compatibility levels. After Stage 7 the MVP stretch goal is met; L4 edge
work (symlinks, TLS, Windows path quirks) stays explicitly deferred per
[`compatibility-levels.md`](reference/serve/compatibility-levels.md).

This is a roadmap, not a per-sub-stage implementation plan. Each sub-stage
gets its own planning artifact under `docs/features/` when it starts (see
the existing [`docs/features/`](features/) directory for the precedent
established in 5a/5b and 6a/6h).

## How a sub-stage runs

Each 7x sub-stage is a self-contained delivery cycle that mirrors the
Stage 6 pattern:

1. **Planning** — fresh session in plan-mode. Read the relevant
   `openspec/specs/<capability>/spec.md`, the SRV entries in
   [`docs/reference/serve/inventory.md`](reference/serve/inventory.md),
   the ORC rows in
   [`docs/reference/serve/oracle-matrix.md`](reference/serve/oracle-matrix.md),
   and existing probe cases under `tools/probe/cases/<capability>-*.json`.
   Open a plan file at `docs/features/00NN_PLAN_stage7X_<short_name>.md`.
2. **Implementation** — iterative slices, one commit per green-state
   slice; subagents handle scaffolding and mechanical edits. The main
   agent owns architectural decisions (D-NNN log entries when needed),
   `design.md` authoring, and cross-slice consistency.
3. **OpenSpec change** — a new `openspec/changes/0NN-...` change packages
   the sub-stage with proposal/design/tasks plus any required spec
   delta(s). Validates under `npx -y @fission-ai/openspec@latest validate
   --all --strict`.
4. **Codex review rounds** — one commit per round, titled
   `docs(stage-7X): address Codex review round N (priority fixes)`.
5. **Stage map update** — the sub-stage's row in
   [`README.md`](../README.md) flips to `done`.

Sub-stages within Stage 7 are loosely coupled — only 7a → 7b carries a
hard ordering edge (Last-Modified reuses the response-emission point
that ETag occupies first). The remaining sub-stages can be scheduled in
any order within their own dependency constraints.

## Decomposition

| Sub | Name | SRVs delivered | OpenSpec change | Depends on | Closes / touches |
|---|---|---|---|---|---|
| **7a** | ETag + 304 conditional GET | SRV-CACHE-001 (P0) | `011-etag-conditional` | — | Flips ORC-042 (`etag-roundtrip#first_get`) and ORC-043 (`etag-roundtrip#second_with_inm`) from reference-only to dual-target. The probe `etag-conditional` adds an `l0` partition. Response-builder gains an `etag()` helper; dispatcher reads `If-None-Match` and short-circuits to 304. Hash function is implementation-defined (reference uses `sha1(extname + '-' + fileContents)`; irserve picks something deterministic — anti-hallucination #4: round-trip behavior is the contract, not the hash). |
| **7b** | `Last-Modified` + `--no-etag` + `If-Modified-Since` | SRV-CACHE-002, SRV-CACHE-003, SRV-CLI-013 | `012-last-modified` | 7a | Closes Q-009 (If-Modified-Since handling under `--no-etag` — currently `unknown`; needs probe-first per anti-hallucination #8). Wires `etag: bool` into `ServeConfig` (currently a `serve.json` field that's parsed-and-ignored). With `--no-etag`, file responses emit `Last-Modified` (RFC 7231 IMF-fixdate UTC) and no `ETag`. The CLI default flip lives in slice 1 of this sub-stage (mirrors Stage 6a's "wire what's already parsed"). |
| **7c** | Range requests (`206`/`416`) | SRV-CACHE-004 (P2) | `013-range-requests` | — (independent of 7a/7b) | Flips ORC-044/045/046 from reference-only to dual-target. Parses `Range: bytes=<start>-<end>`/`<start>-`/`-<suffix>`; on a satisfiable range, emits 206 with `Content-Range: bytes <s>-<e>/<total>` and `Content-Length: <e-s+1>`. On a strictly out-of-range value, emits 416 with `Content-Range: bytes */<total>`. Multiple ranges (`bytes=0-3, 8-11`) are explicitly OUT of scope — reference doesn't implement them either ("TODO ? multiple ranges" in source). |
| **7d** | `Cache-Control` default + `OPTIONS` (CORS preflight) | SRV-CACHE-005 (P2), SRV-CORS-001 (preflight semantics) | `014-cache-headers-and-preflight` | — | Two small loose ends. SRV-CACHE-005 is *verification-only* — the contract is "no default `Cache-Control`; the header appears verbatim only when a user `headers` rule sets it". irserve already behaves this way; flip ORC-047..052 from reference-only to dual-target. SRV-CORS-001 was substantially closed in 6h (all four headers on every status), but the preflight pass-through (reference does NOT short-circuit `OPTIONS` to 204; it serves the file via the static pipeline) needs to be either mirrored or `adapted` with a D-NNN. Flip ORC-056 (`cors-preflight#preflight_options`) to dual-target after the decision lands. |
| **7e** | HTTP compression (`-u`/`--no-compression` un-no-op) | SRV-CLI-012 (P2) | `015-compression` | — | The biggest methodological risk in Stage 7 (anti-hallucination rule #8: mirroring a third-party library — Node's `compression` middleware — with defaults that aren't fully visible from source). D-006 currently defers entirely; this sub-stage promotes it from `adapted` to a real implementation. Closes Q-002 (compressed content-type set + minimum body-size threshold). Flips ORC-058 from reference-only to dual-target. The runner's `fetch` transparently decompresses bodies, so the on-the-wire `Content-Encoding` is unobservable via standard probes; expect raw-mode (`net.Socket`) probes to pin the wire-level shape, mirroring the strategy used for SRV-SEC-001 in Stage 6f. |

## Ordering rationale

The dependency edges that drive the order:

- **7a before 7b.** ETag and Last-Modified share the response-emission
  point; the cleanest cut is to land ETag (the default) first, then
  add the `--no-etag` switch which flips it for `Last-Modified`. Doing
  them in the reverse order forces a throwaway scaffold for the default
  case.
- **7c, 7d, 7e independent.** Range is a pure response-shape change
  with no dependency on ETag (the 206 short-circuit comes before the
  304 short-circuit per reference; we mirror). Cache-Control is mostly
  verification work. Compression is its own beast.
- **Schedule by appetite.** Recommended order if Stage 7 is sequenced:
  7a → 7b → 7c → 7d → 7e. 7e last because it's the riskiest; 7d
  before it because it's cheap closure for two existing SRV families.
  But 7c can move earlier if the dev wants a quick win after 7a/7b.

## Out of scope (deferred to L4 or beyond)

The following SRVs are **not** addressed in Stage 7 even though they
appear in `inventory.md`:

- **Symlinks** — `SRV-CLI-017` (`-S`/`--symlinks`) and `SRV-SYM-001`.
  irserve currently follows symlinks subject to the lexical
  containment check (Stage 6f's SRV-SEC-001 work); reference returns
  404 by default and follows only with `-S`. Inverting the default
  to match would be a contractual flip and likely surfaces Q-011
  (Windows symlink/junction parity). L4 territory — Stage 8 or
  later.
- **TLS** — `SRV-CLI-018` (`--ssl-cert`, `--ssl-key`, `--ssl-pass`).
  HTTPS support is L4. MVP serves HTTP only.
- **Windows path quirks** — `SRV-WIN-001`. Case-insensitivity on
  NTFS, `\` vs `/` separators, drive letters, UNC paths, long-path
  syntax, reserved names. L4. Q-011 is the umbrella.
- **cleanUrls extglob parity** — Q-012 (`+(a|b)`, `@(a|b)`, etc.).
  `globset` does not support extglob; a future close requires either
  a manual translation to `regex` or an adapted D-NNN documenting the
  divergence. Not Stage 7 scope.
- **Terminal output polish** — banner, colored output, exact log line
  format. D-002 keeps these out forever (only the *behavior* — log
  presence vs silence under `--no-request-logging`, exit codes — is
  contractual; wording is not).

## Methodological signals to watch for

Each sub-stage may surface divergences between `vercel/serve` behavior
and the spec. The Stage 6 precedent gives the playbook:

1. **Spec under-specified** — record a new `D-NNN` entry in
   [`docs/reference/serve/decisions.md`](reference/serve/decisions.md)
   adapting or rejecting the specific case, *before* the slice commits.
2. **Spec over-broad must-match** — refine the L0 mask in
   `tools/probe/run.mjs` (e.g. additional `*MayDiffer` overlay) and
   relax the corresponding ORC must-match line. Never silently weaken
   the contract.
3. **Reference quirk** — capture as an `adapted` SRV with a `Note:`
   line citing the source code path; possibly add a Q-NNN entry in
   [`docs/reference/serve/open-questions.md`](reference/serve/open-questions.md)
   if the divergence is not yet probed.

Stage-7-specific signals worth flagging up front:

- **7a / hash function divergence (D-NNN candidate).** The reference
  uses `sha1(extname + '-' + fileContents)`. Mirroring exactly is
  unnecessary — only the 200/304 round-trip is contractual. Pick a
  fast deterministic hash (e.g. blake3 truncated, or sha1 of contents
  without the extname prefix) and document the divergence as a
  `D-NNN` Note. The `etag-roundtrip` probe's reference snapshot pins
  a specific hash value; under `target=irserve` the case will need an
  `*MayDiffer` overlay or a derived "any non-empty etag" comparator
  rather than byte-equal match.

- **7b / If-Modified-Since semantics (Q-009 closure).** Source has no
  explicit branch. Anti-hallucination rule #8 mandates 5-10 probes
  against the reference BEFORE implementation. The probe shape:
  `serve --no-etag`, capture `Last-Modified`, re-issue with
  `If-Modified-Since: <that-value>`. Expected reference behavior is
  *probably* "no 304; returns 200 with full body" (since source lacks
  the branch), but until probed it stays `unknown`. Close Q-009 with
  the probe outcome; do not write the implementation against the
  guess.

- **7d / preflight pass-through vs short-circuit (D-NNN candidate).**
  The reference does NOT short-circuit `OPTIONS`; it sends the
  request through the static-file pipeline like a `GET`. A common
  expectation for CORS preflight is `204 No Content` with the four
  headers and no body. Two options: (a) mirror reference, send the
  body — surprising to clients but matches contract. (b) adapt to
  `204` — friendlier, but a `D-NNN` divergence. Probe-confirm
  reference behavior via `cors-preflight.json` (already authored),
  then decide. The current `OPTIONS` handling in axum may already
  diverge from reference — verify before deciding.

- **7e / compression library emulation (anti-hallucination #8 zone).**
  Node's `compression` middleware has implementation-specific defaults
  (content-type allowlist, ~1 KiB body threshold, `gzip`/`deflate`
  negotiation order). Mirroring requires probe-first reverse-engineering
  via raw-mode `net.Socket` probes (the runner's `fetch` decompresses
  transparently). Q-002 is the umbrella. Pre-stage out-of-scope list
  is **mandatory** here — enumerate what we WILL NOT mirror (brotli,
  per-request encoding overrides via `Accept-Encoding: identity;q=0`,
  the exact body-size threshold within ±10%) BEFORE the first slice.
  If review rounds keep landing on the same sub-aspect (compression
  threshold corner-cases, content-type matchers), trigger
  anti-hallucination rule #9 and pause to declare parity scope.

## Estimated effort

Not a commitment — calibrate against actual sub-stage 7a duration.
Stage 6 sub-stages averaged 60-100k tokens per fresh session; Stage 7
sub-stages are individually narrower than 6f/6g (less cross-cutting,
fewer interacting phases) but 7e is unusually risky. Rough sketch:

- 7a, 7c, 7d: 1 planning + 1 implementation + 1 review session
  each — ~60k tokens.
- 7b: 2 implementation sessions (one for Last-Modified + `--no-etag`,
  one for If-Modified-Since after Q-009 probe-closure) — ~100k tokens.
- 7e: 1 plan + 5-10 reverse-engineering probes against the reference
  + 1-2 implementation sessions + 2-3 review rounds — could exceed
  150k tokens if the compression-middleware parity surface is wider
  than expected.

Rough total: 5 × ~3 sessions = ~15 sessions, weighted toward 7b and 7e.

## Stage map cross-reference

The condensed view of Stage 7 lives in [`README.md`](../README.md) as
sub-rows 7a–7e. This document is its canonical detail; updates to
status, dependencies, or scope land here first and propagate to the
README row.
