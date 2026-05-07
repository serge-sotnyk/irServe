# Stage 2 — Capability map refresh

## Context

Stage 1 (reverse inventory) is done. It produced 48 `SRV-*` entries in
`docs/reference/serve/inventory.md` and a `compatibility-levels.md`
that explicitly logs **6 Stage-2 follow-ups** as coverage gaps:

1. Documented operation precedence (rewrites / redirects / cleanUrls / trailingSlash / static files) — no SRV.
2. `Cache-Control` header behavior — no SRV.
3. `trailingSlash` level placement — inventory L2 vs old skeleton bullet at L1.
4. CORS response-header surface (`access-control-allow-*`) — flag captured (SRV-CLI-010), response surface is not. Probe runner does not even capture these headers.
5. Windows path quirks at L4 — no SRV.
6. Wire-level path traversal probing (Q-010) — current probe runner uses `fetch`, which normalizes `..`, `%2e%2e`, `//` before sending; SRV-SEC-001 cannot be promoted to `verified` until this is fixed.

There is also an open methodology decision: stage 5 in the current README map bundles two distinct deliverables (first implementation proposal **and** Rust scaffold). They have different review costs and should be separated.

Stage 2 closes those gaps without leaking into the deliverables of stages 3 (oracle matrix), 4 (OpenSpec bootstrap), or 5+ (Rust). It also takes one design call recorded in Stage 1: Q-008 (JSON directory listing exposes an absolute `dir` field) is locked as `adapted` with a sanitization rule.

## Outcome

After Stage 2:

- `compatibility-levels.md` has every L0–L3 bullet backed by an `SRV-*` entry; the only remaining gap is the L4 Windows-path block, deliberately left as `deferred`.
- `inventory.md` gains 4 new entries (precedence, Cache-Control, CORS response surface, Windows path quirks).
- `decisions.md` gains D-007 (sanitized JSON listing).
- `open-questions.md` reflects that Q-008 is closed by D-007; Q-010 remains open but with a pointer to the new raw-path probe.
- `tools/probe/run.mjs` captures `access-control-allow-*` headers and supports a raw-path probe mode that does not normalize `..`, `%2e%2e`, or `//`.
- New probe cases provide evidence for the new SRVs.
- `README.md` stage map splits stage 5 into 5a (first implementation proposal) and 5b (Rust scaffold + first vertical slice).

Out of scope: any Rust code, any `openspec/changes/*`, any reverify-pass over the existing 36 `accepted` SRVs (deferred to stage 3), and the full Windows-path SRV body — only a `deferred` placeholder is added.

## Compatibility target

Confirmed: **MVP target = L2; L3 = stretch; L4 = explicitly out of scope** (matches current README and `compatibility-levels.md`). This stage does not change the target. New L3 entries (Cache-Control, CORS response surface) inherit the stretch status; the L4 Windows entry is added with `deferred` status only so the gap is auditable.

## Steps

### 1. README stage map split

File: `README.md`

In the "Stage map" table, replace row 5 with two rows:

| 5a | First implementation proposal (specs delta + design + tasks, no code) | `openspec/changes/001-port-minimal-static-server/` (proposal/design/tasks/specs only) | todo |
| 5b | Rust scaffold + first vertical slice | `Cargo.toml`, `tests/oracle/`, first crate code | todo |

Update the prose underneath (line ~51, "The first concrete Rust crate appears at Stage 5") to point at 5b.

### 2. New inventory entries (`docs/reference/serve/inventory.md`)

Use the existing entry template (Status / Area / Priority / Reference source / Requirement candidate / Scenarios / Compatibility notes / Oracle fixture). Levels and statuses chosen to keep MVP scope tight.

- **SRV-ROUT-006 — Operation precedence.** Area: `routing`, Level 2, Priority P0, Status `candidate`. Captures the documented order: redirects → rewrites → cleanUrls → trailingSlash → static-file resolution. Cite `serve-handler` source (the request pipeline in `src/index.js`) plus existing probes `prec-rewrites-redirects`, `prec-cleanurls-default`, `prec-cleanurls-trailing`, `prec-cleanurls-trailing-false`. Cross-reference compatibility notes in SRV-RDIR-001, SRV-ROUT-001, SRV-RWRT-001 (which currently each restate a slice of this).
- **SRV-CACHE-005 — Cache-Control header behavior.** Area: `caching`, Level 3, Priority P2, Status `candidate`. Documents serve's default (currently observed: no Cache-Control unless set via `headers` rule) and the interaction with custom `headers`. Backed by a new probe `cache-control-default`.
- **SRV-CORS-001 — CORS response header surface under `--cors`.** Area: `cli`/`headers`, Level 3, Priority P1, Status `candidate`. Enumerates the response headers serve adds when `--cors` is on (Access-Control-Allow-Origin, -Methods, -Headers, etc.). Note: the L1 SRV-CLI-010 stays as-is (flag presence at L1); the **response surface** is L3 polish. Backed by new probes `cors-response-surface` and `cors-preflight`.
- **SRV-WIN-001 — Windows path quirks placeholder.** Area: `security`, Level 4, Priority P3, Status `deferred`. Body lists the sub-cases (case insensitivity, separator handling, drive letters, `\\?\` long paths) without scenarios; explicitly out-of-MVP. Exists to make the gap auditable.

Existing entries that need a one-line "see SRV-ROUT-006" in their Compatibility-notes block: SRV-RDIR-001, SRV-ROUT-001, SRV-ROUT-002, SRV-RWRT-001, SRV-CLI-008.

### 3. Reconcile `trailingSlash` placement

File: `docs/reference/serve/compatibility-levels.md`, line 41 note + Level 1 / Level 2 bullets.

Decision: keep `trailingSlash` at **Level 2** (matches inventory). Remove the "should be revisited at Stage 2" note. Reason: the observable effect is a 301 routing decision, not config loading.

### 4. New decision (`docs/reference/serve/decisions.md`)

- **D-007 — adapted: sanitized JSON directory listing.** IrServe will mirror serve's JSON listing shape for Accept-based negotiation but the `dir` field is rendered **relative to the served root** (not the host filesystem). Reason: privacy/security; `D-004` (no bug-for-bug parity) authorizes the divergence. Affects: SRV-DLST-001, Q-008.

### 5. Open questions update (`docs/reference/serve/open-questions.md`)

- **Q-008 → closed-by-decision** with pointer to D-007.
- **Q-010 → annotated** with a pointer to the new raw-path probe (`traversal-raw-encoded`); status stays `open` until the probe runs and SRV-SEC-001 is promoted to `verified` in stage 3.
- All other Q-* entries stay as they are; they belong to stage 3+ work.

### 6. compatibility-levels.md refresh

After steps 1–5, edit the file:

- Level 1 bullet list: ensure CORS-flag is referenced via SRV-CLI-010 (already there).
- Level 2 bullet list: add precedence bullet citing SRV-ROUT-006.
- Level 3 bullet list: add Cache-Control bullet citing SRV-CACHE-005; add CORS response-surface bullet citing SRV-CORS-001.
- Level 4 bullet list: add Windows-paths bullet citing SRV-WIN-001 (deferred).
- "Coverage gaps (Stage-2 follow-ups)" section: rewrite to record which gaps are now closed (1, 2, 3, 4, 5) and which remain (item 6 — Q-010 — which becomes a stage-3 prerequisite once the raw-path probe is wired in).

### 7. Probe runner extensions (`tools/probe/run.mjs`)

- Extend `TRACKED_RESPONSE_HEADERS` (currently lines 32–41) with: `access-control-allow-origin`, `access-control-allow-methods`, `access-control-allow-headers`, `access-control-allow-credentials`, `access-control-expose-headers`, `access-control-max-age`. Trivial change.
- Add a new request mode (e.g. `mode: "raw"` per request in case JSON) that uses Node's `http.request` with a manually constructed request line to avoid fetch's path normalization. Defaults stay on `fetch` for all existing cases. Document the mode in `tools/probe/cases/_schema.json` and `tools/probe/README.md`.
- Update `tools/probe/cases/_schema.json` accordingly.

### 8. New probe cases (`tools/probe/cases/`)

- `cache-control-default.json` — verifies serve's default response has no Cache-Control unless a `headers` rule sets one. Backs SRV-CACHE-005.
- `cors-response-surface.json` — runs serve with `--cors` and asserts the new tracked CORS headers appear with documented values. Backs SRV-CORS-001.
- `cors-preflight.json` — sends an `OPTIONS` request with `Origin` + `Access-Control-Request-*` to capture preflight response. Backs SRV-CORS-001.
- `traversal-raw-encoded.json` — uses `mode: "raw"` to send `GET /%2e%2e/etc/passwd` literally; backs SRV-SEC-001 / closes the wire-level part of Q-010.

For each new case, run `node tools/probe/run.mjs <id>` once to generate the result file under `tools/probe/results/` (gitignored), confirm shape, and reference the case ID from the relevant SRV's Reference-source block.

### 9. External review pause

After all of the above lands in the working tree, **stop before committing** and pass the diff to the external reviewer (Codex). Do not advance until that pass is complete and any corrections are applied.

### 10. Single commit

When the review settles, land the work as one commit on `main`:

```
docs(stage-2): refresh capability map and split stage 5
```

The commit body summarizes new SRVs, D-007, probe runner extensions, and the README stage-map split.

## Critical files

Read-and-edit:

- `README.md` — stage-5 split.
- `docs/reference/serve/inventory.md` — 4 new SRV entries; cross-references in 5 existing entries.
- `docs/reference/serve/compatibility-levels.md` — bullet refresh, gaps section rewrite.
- `docs/reference/serve/decisions.md` — D-007.
- `docs/reference/serve/open-questions.md` — Q-008 closed, Q-010 annotated.
- `tools/probe/run.mjs` — header allowlist + raw-path mode.
- `tools/probe/cases/_schema.json` — schema update.
- `tools/probe/README.md` — document new mode.
- `tools/probe/cases/cache-control-default.json`, `cors-response-surface.json`, `cors-preflight.json`, `traversal-raw-encoded.json` — new cases.

Read-only references:

- `third_party/serve-handler/src/index.js` — request pipeline source for SRV-ROUT-006 evidence.
- `third_party/serve/source/main.ts` — for any CORS / Cache-Control behavior crosscheck.
- Existing probe results under `tools/probe/results/` for behavior cross-reference (regenerate if stale).

## Verification

End-to-end checks the implementing agent must run before handing off for review:

1. `node tools/probe/run.mjs --list` — confirms 4 new probe IDs appear.
2. `node tools/probe/run.mjs cors-response-surface` — confirms `access-control-allow-*` are now captured in the result file.
3. `node tools/probe/run.mjs traversal-raw-encoded` — confirms the request line shows the unnormalized `%2e%2e` form (manual inspection of the result file's request snapshot).
4. `node tools/probe/run.mjs cache-control-default` — confirms expected Cache-Control behavior is documented.
5. Cross-read pass: every L0–L3 bullet in `compatibility-levels.md` cites an `SRV-*` ID that exists in `inventory.md`; every new `SRV-*` cites either source path, probe ID, or open question.
6. `git status` — only the files listed under "Critical files" should be modified; nothing under `openspec/`, `crates/`, `Cargo.toml`, etc.

## Anti-hallucination guardrails for this stage

- New SRVs start as `candidate`. Promotion to `accepted` requires either documented behavior in serve/serve-handler README or a cited probe; promotion to `verified` is a stage-3 task.
- Cache-Control defaults must be observed (probe), not guessed. If the probe shows behavior that differs from what the agent expected, the SRV body is rewritten to match the probe — not the other way around.
- The Windows-path SRV is intentionally a stub. Do not invent scenarios.
- Do not reverify the existing 36 `accepted` SRVs. If a new probe accidentally reveals that an existing SRV is wrong, log it as a new entry under "Coverage gaps" rather than silently editing — keeping the audit trail clean for stage 3.
