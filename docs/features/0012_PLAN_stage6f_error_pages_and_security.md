# Stage 6f — Custom error pages, full L2 security, custom response headers

> Plan-mode draft. On approval, this becomes
> `docs/features/0012_PLAN_stage6f_error_pages_and_security.md` (next free
> ordinal after `0011_PLAN_stage6e_configured_rewrites.md`).

## Context

Stage 6f is the next sub-stage of Stage 6 (L1+L2 capabilities). It is the
first 6x sub-stage that does NOT wire a new dispatcher *phase*; instead it
**generalizes phase 13 (error response)** and adds a final **post-dispatch
header-application pass**, then tightens **phase 1 decode** and **phase 10
containment** to spec-conforming wire behavior. Three SRV bundles land:

- **SRV-FILE-003** — custom `<statusCode>.html` at the served root replaces
  the built-in HTML body for any error response sent to HTML-accepting
  clients. Spec: `openspec/specs/static-files/spec.md:58-82`. Reference:
  `serve-handler/src/index.js:467-524` (`sendError`).
- **SRV-SEC-001 (full surface) + SRV-SEC-002** — single-pass URL decode
  with strict malformed-`%xx` rejection (400), and root-escape containment
  failures must return **400** (not the 5b L0-hygiene 404). Spec:
  `openspec/specs/security/spec.md:12-90`. Reference: `index.js:561-580`.
- **SRV-HDR-001 + SRV-HDR-002** — `headers: [{source, headers: [{key,
  value}]}]` from `serve.json`. Glob-matched per request path, accumulated
  in order, override default headers of the same name (case-insensitive),
  applied to **every** response (success + 301 + 4xx). `value: null`
  deletes a previously-set header (post-accumulate pass). Reference:
  `serve-handler/src/index.js:194-254` (`getHeaders`). No spec capability
  exists yet — 6f creates `openspec/specs/headers/spec.md`.

Three binding decisions confirmed in plan-mode interview:

1. **HDR-001 + HDR-002 together** — null-prune is ~20 LOC alongside
   accumulate; ship both to fully close the headers feature in one stage.
2. **Slice order: Errors → Security → Headers** — small first slice
   (generalize `notfound_response` → `error_response`, 404-only); then
   tighten phase 1 decode (malformed `%xx` → 400); then full traversal
   400-surface (resync the existing `traversal-raw-encoded.json` irserve
   snapshot); then headers (touches every response, naturally last).
3. **Re-record `traversal-raw-encoded.json` against irserve** after slice
   3 — the reference snapshot already shows 400 across all four anchors;
   irserve simply needs to catch up. Methodology signal: D-015 documents
   the un-defer flip from 5b L0-hygiene 404 to spec-conforming 400.

## Pipeline integration

Phase numbering from `openspec/changes/001-port-minimal-static-server/design.md` §4.

- **Phase 1 — request parsing.** Currently `dispatch.rs:40-41`
  `percent_decode_str(raw_path).decode_utf8_lossy()`: malformed `%xx`
  silently produces U+FFFD. **6f change:** wrap with a percent-syntax
  validator that returns `Err` on any `%` not followed by two hex chars,
  short-circuit to `error_response(400, ...)` before any phase 2+ runs.
- **Phase 10 — containment check.** `resolve.rs:41-48` returns
  `ResolveOutcome::EscapedRoot`. Currently `dispatch.rs:162` collapses
  `NotFound | EscapedRoot` to a 404. **6f change:** split the arms; route
  `EscapedRoot` to `error_response(400, ...)`.
- **Phase 13 — error response.** Currently `notfound.rs:10` always emits
  status 404 with hardcoded `HTML_BODY` / `JSON_ENVELOPE`. **6f change:**
  generalize to `error_response(status, headers, root) -> Response<Body>`:
  - JSON-preferring clients → status-keyed envelope (`not_found`,
    `bad_request`; extensible).
  - HTML clients → if `<status>.html` exists at `root`, read it as bytes
    and return with the matching status; else fall back to a generic
    `<h1>{status} {reason}</h1>\n` body (D-003 disclaims markup parity).
- **Post-dispatch — custom-header application.** New seam: every
  `Response` produced by `dispatch` flows through
  `apply_custom_headers(response, request_path, &compiled_header_rules)`
  before being returned to axum. Accumulate matching rules in order;
  case-insensitive merge over default headers; null-prune last.

The compiled rule structures live alongside `RedirectRuleCompiled` /
`RewriteRuleCompiled` in `server.rs` and are threaded into `dispatch`
the same way.

## Slice plan (ask before each commit)

| # | Goal | Files | Verify |
|---|------|-------|--------|
| 1 | Generalize `notfound.rs` → `error.rs` with `error_response(status, headers, root)`. Custom `<status>.html` filesystem lookup at served root for HTML clients; built-in HTML fallback otherwise. JSON branch keyed off `status` with envelope map (`not_found`, plus stub for `bad_request`). Both 404 emission sites in `dispatch.rs:160,162` switch to `error_response(404, ...)`. | new `crates/irserve-core/src/error.rs` (replaces `notfound.rs`); `dispatch.rs` (call sites); `lib.rs` (re-export) | `cargo test -p irserve-core`; `error-page-custom` ORC + `notfound-shape`/`notfound-custom` ORCs stay green. Record irserve snapshot for `error-page-custom.json` and `notfound-custom.json`. |
| 2 | Strict URL decode at dispatch entry. New helper `try_percent_decode(&str) -> Result<String, MalformedPercent>` that pre-validates `%xx` syntax before decode. `dispatch.rs:40-41` short-circuits to `error_response(400, ...)` on `Err`. | `dispatch.rs`; `error.rs` (400 JSON envelope `bad_request`) | new probe `sec-malformed-percent.json` OR re-record the existing `raw_malformed_percent` anchor; ORC-041 green under `target=irserve`. |
| 3 | Full SRV-SEC-001 surface. Split `dispatch.rs:162` so `ResolveOutcome::EscapedRoot` → `error_response(400, ...)`; `NotFound` stays 404. Re-record `tools/probe/snapshots/traversal-raw-encoded.json` via `--snapshot=update --target=irserve` (must equal existing reference snapshot of 400 across all four anchors). Add `runner.l0.clean` block to the case so the matrix accepts irserve. | `dispatch.rs`; `tools/probe/cases/traversal-raw-encoded.json` (`runner.l0` block); snapshot regen | `traversal-raw-encoded` (4 anchors) ORC green under `target=irserve`; `traversal-encoded` fetch-mode 404 ORCs stay green. |
| 4 | Custom headers SRV-HDR-001. Compile `serve_config.headers` globs at startup using shared `path_pattern.rs` (Literal/Glob kernels — no `:name` since reference's `getHeaders` uses minimatch only; verify `index.js:200-210`). Add `apply_custom_headers` pass invoked at every `dispatch` exit path (file 200, 301, 405, 404, 400, custom error). Accumulate matching rules in order; merge over defaults case-insensitively. | new `crates/irserve-core/src/custom_headers.rs`; `dispatch.rs` (single exit wrapper); `server.rs` (compile + thread `&[HeaderRuleCompiled]`); `config.rs` already has `HeaderRule { source, headers: Vec<HeaderItem> }` | new probes `headers-on-error.json`, `headers-accumulate.json`; existing `headers-applied` ORC stays green; `headers-custom` (the 301 case) re-recorded so its 301 response now carries the custom headers. |
| 5 | SRV-HDR-002 null-prune. Loosen `HeaderItem::value` from `String` to `Option<String>` (`#[serde(default)]` + `Option`). After accumulate, scan for `value: None` entries and delete prior headers with that key (case-insensitive). | `crates/irserve-core/src/config.rs` (HeaderItem); `custom_headers.rs` (prune pass) | new probe `headers-null-prune.json` green; serde regression: existing fixtures with `value: "string"` still parse. |
| 6 | Spec deltas + change package + meta. Create `openspec/changes/008-error-pages-and-security/{proposal,design,tasks}.md` + spec deltas. Validate with openspec CLI. Append D-015. Refresh `inventory.md`, `oracle-matrix.md`, `README.md` "Try IrServe" smoke section, `docs/stage6_l1_l2_capabilities.md` row. | as above | `npx -y @fission-ai/openspec@latest validate --all --strict`; full `cargo test --test oracle` clean. |

## Critical files

Read / extend (do not rewrite from scratch):

- `crates/irserve-core/src/notfound.rs` — entire file becomes `error.rs`;
  `JSON_ENVELOPE` / `HTML_BODY` move to status-keyed maps; `accepts_json`
  helper (lines 32-44) lifts verbatim.
- `crates/irserve-core/src/dispatch.rs` — entry decode at L40-41; error
  emission at L160 + L162; need a single-exit wrapper for
  `apply_custom_headers`.
- `crates/irserve-core/src/resolve.rs:41-48` — `ResolveOutcome::EscapedRoot`
  variant exists; only the dispatch-side mapping changes.
- `crates/irserve-core/src/config.rs` — `HeaderRule` and `HeaderItem` are
  already parsed (per README §"Try IrServe"). Slice 5 widens
  `HeaderItem::value` to `Option<String>`.
- `crates/irserve-core/src/server.rs` — startup-time rule compilation:
  precedent at the redirect/rewrite block (compile + thread into
  `dispatch`). Add `compile_header_rules` along the same axis.
- `crates/irserve-core/src/path_pattern.rs` — already exposes
  `compile_source_regex` / Literal+Glob kernels from 6e. Headers reuse
  the Literal/Glob arms only (no `:name` per reference).
- `tools/probe/run.mjs:34-83` — `TRACKED_RESPONSE_HEADERS`,
  `DEFAULT_VOLATILE_HEADERS`, `L0_EXTRA_VOLATILE_HEADERS`. No changes
  expected; new probes declare custom keys via `snapshot.extraTrackedHeaders`
  (precedent: `headers-applied.json:21`).
- `third_party/serve-handler/src/index.js:194-254` (`getHeaders`,
  null-prune at L247-251), `:38-89` (`sourceMatches`), `:467-524`
  (`sendError`, custom-page lookup at L490-516), `:561-580` (decode +
  containment). Do not modify.

## New probe cases

Each: record against reference first per anti-hallucination rule 6.

- `error-page-400.json` — `bad_request_with_custom_400` raw-mode `..`
  request; fixture has `400.html`. Proves `<status>.html` works beyond 404.
- `sec-malformed-percent.json` — three anchors: `single_pct`,
  `bare_pct_eof`, `pct_one_hex` (`/%`, `/%a`, `/%g0`). Proves wire-level
  400 across malformed forms; complements ORC-041's single-anchor coverage.
- `headers-on-error.json` — fixture with `headers: [{source: "**",
  headers: [{key: "X-T", value: "yes"}]}]`. Two anchors: `not_found_carries_x_t`,
  `bad_request_carries_x_t` (raw-mode `..`). Proves headers apply to error
  responses (matches reference at `index.js:519` calling `getHeaders`).
- `headers-accumulate.json` — two source rules matching the same path,
  different keys. Proves accumulate-not-stop semantics
  (`index.js:200-210`).
- `headers-null-prune.json` — rule A sets `X-K: v`; rule B sets `X-K: null`.
  Proves prune pass deletes (`index.js:247-251`).

Existing probes to re-record after wiring:

- `tools/probe/snapshots/error-page-custom.json` — record irserve
  snapshot in slice 1 (case file unchanged).
- `tools/probe/snapshots/notfound-custom.json` — record irserve snapshot
  in slice 1 (case file unchanged; both `_html_custom` and
  `_json_accept` anchors).
- `tools/probe/snapshots/traversal-raw-encoded.json` — re-record irserve
  snapshot in slice 3; case file gains `runner.l0.clean: [<all four>]`.
- `tools/probe/cases/headers-custom.json` — currently the 301 swallows
  the custom headers (the case is testing infra). After slice 4 the 301
  response carries the headers; re-record snapshot accordingly. Decide
  in slice 4 whether to retire or keep as a precedence ORC.

## Spec deltas (in `openspec/changes/008-error-pages-and-security/`)

- `specs/static-files/spec.md` — **MODIFIED** SRV-FILE-003 oracle list:
  add irserve coverage for ORC-007, ORC-059. Requirement text at
  L58-82 stays unchanged (already final).
- `specs/security/spec.md` — **MODIFIED** SRV-SEC-001 + SRV-SEC-002
  oracle lists: un-defer ORC-038/039/040/041 from D-008/D-010 sticking
  state. Requirement text at L12-90 stays unchanged.
- `specs/headers/spec.md` — **ADDED** new capability with two
  Requirements: SRV-HDR-001 (glob source + accumulate + case-insensitive
  override; applies to all responses), SRV-HDR-002 (`value: null`
  deletes after accumulate).

## Decision log entries

Next free numbers (latest is D-014):

- **D-015** — Stage 6f un-defers SRV-FILE-003, the full SRV-SEC-001
  surface (4 raw-mode anchors), SRV-SEC-002 strict decode, SRV-HDR-001,
  and SRV-HDR-002 from D-008 / D-010 / D-011. Cite `index.js:467-524`,
  `:561-580`, `:194-254`. Records the methodology signal that
  `traversal-raw-encoded.json`'s irserve snapshot flips from 404 (5b
  L0-hygiene) to 400 (spec-conforming) — not a regression but a delivery
  of the deferred contract.

No D-016 anticipated unless slice 4 surfaces a `getHeaders` quirk
(reading `index.js:200-210` ahead of slice 4 should rule it out).

## Out of scope (anti-hallucination rule 10)

1. **L3 cache headers as defaults** (`Cache-Control`, `ETag`,
   `Last-Modified`, conditional GETs) — Stage 7+. Custom headers can set
   these via `serve.json`, but no irserve-side default emission.
2. **HTML body markup parity for error pages** — D-003 stands; the
   default HTML body for non-custom responses is generic
   `<h1>{status} {reason}</h1>\n`; existing `notfound-shape` `bodyMayDiffer`
   partition stays as-is.
3. **`<status>.html` lookup beyond root** — reference checks
   `${current}/${statusCode}.html` only (`index.js:490`). No nested
   subdir lookup, no extension fallback.
4. **`headers.source` patterns beyond Literal/Glob** — `:name` is not
   supported by reference's `getHeaders`. Inherits Q-012 no-extglob
   limitation from 6d/6e.
5. **`HeaderItem::value` types beyond `string | null`** — JSON numbers,
   booleans, arrays in the value position are rejected at config-load
   time (serde error). Reference uses raw string concatenation and
   undefined behavior for non-strings.
6. **Headers ordering vs response builder** — accumulate order is
   per-rule then per-`headers` array within the rule; merge over
   defaults uses last-write-wins case-insensitively. No
   per-`Set-Cookie`-style multi-value semantics in 6f (reference doesn't
   handle this either).

## Verification

End-to-end:

```bash
cargo test -p irserve-core
cargo test --test oracle              # exercises tools/probe/run.mjs
                                      # against irserve target
npx -y @fission-ai/openspec@latest validate --all --strict
```

Manual smoke after slice 5:

```bash
mkdir -p _tmp && echo hello > _tmp/index.html
echo '<h1>custom-404</h1>' > _tmp/404.html
cat > _tmp/serve.json <<'JSON'
{
  "headers": [
    {"source": "**", "headers": [
      {"key": "X-Powered-By", "value": "irserve"},
      {"key": "X-Default", "value": null}
    ]}
  ]
}
JSON
cargo run -- --listen 3010 _tmp
curl -i http://127.0.0.1:3010/missing                         # 404, body custom-404, X-Powered-By: irserve
curl -i 'http://127.0.0.1:3010/%zz'                           # 400, X-Powered-By: irserve
curl -i 'http://127.0.0.1:3010/../package.json' --path-as-is  # 400 (raw), X-Powered-By: irserve
```

Reference parity: each new probe case shows identical
status/body/headers (within `contentLengthMayDiffer` envelope) between
`--target=reference` and `--target=irserve` snapshots.

## Estimated effort

Per `docs/stage6_l1_l2_capabilities.md` calibration: 60-100k tokens per
fresh session, 3 sessions per sub-stage. 6f has six slices touching three
distinct features but each is small (~50-150 LOC) and the
`path_pattern.rs` infrastructure from 6e covers the matcher work.
Anticipate 1 implementation session + 1-2 review-round sessions.
Stage 6f is flagged in the roadmap as one of the higher-surface
sub-stages for security decisions; budget conservatively.
