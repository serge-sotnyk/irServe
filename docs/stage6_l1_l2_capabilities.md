# Stage 6 — L1 + L2 capabilities (sub-stage breakdown)

This document is the canonical decomposition of Stage 6 into independently
deliverable sub-stages 6a–6h. Stage 6 closes L1 (serve-style configuration
and routing normalization) and L2 (full routing pipeline: cleanUrls,
redirects, rewrites, headers, custom error pages, directory listing) per
[`README.md`](../README.md) compatibility levels. The MVP target is L2;
L3+ is Stage 7+.

This is a roadmap, not a per-sub-stage implementation plan. Each sub-stage
gets its own planning artifact under `docs/features/` when it starts (see
the existing [`docs/features/`](features/) directory for the precedent
established in 5a/5b).

## How a sub-stage runs

Each 6x sub-stage is a self-contained delivery cycle that mirrors the
Stage 5b pattern:

1. **Planning** — fresh session in plan-mode. Read the relevant
   `openspec/specs/<capability>/spec.md`, the SRV entries in
   [`docs/reference/serve/inventory.md`](reference/serve/inventory.md),
   the ORC rows in
   [`docs/reference/serve/oracle-matrix.md`](reference/serve/oracle-matrix.md),
   and existing probe cases under `tools/probe/cases/<capability>-*.json`.
   Open a plan file at `docs/features/000N_PLAN_stage6X_<short_name>.md`.
2. **Implementation** — iterative slices, one commit per green-state
   slice; subagents handle scaffolding and mechanical edits. The main
   agent owns architectural decisions (D-NNN log entries when needed),
   `design.md` authoring, and cross-slice consistency.
3. **OpenSpec change** — a new `openspec/changes/00N-...` change packages
   the sub-stage with proposal/design/tasks plus any required spec
   delta(s). Validates under `npx -y @fission-ai/openspec@latest validate
   --all --strict`.
4. **Codex review rounds** — one commit per round, titled
   `docs(stage-6X): address Codex review round N (priority fixes)`.
5. **Stage map update** — the sub-stage's row in
   [`README.md`](../README.md) flips to `done`.

Sub-stages are independent: 6a must land before everything else (it adds
the config layer all subsequent capabilities depend on), but 6b–6h can be
scheduled flexibly within their dependency constraints. Failed validation
in one sub-stage does not block work on a parallel one.

## Decomposition

| Sub | Name | SRVs delivered | OpenSpec change | Depends on | Closes / touches |
|---|---|---|---|---|---|
| **6a** | `serve.json` loader | SRV-CFG-001, SRV-CFG-002 | `003-load-serve-json` | — | Touches Q-003 (schema-validation error format; D-002 caps wording adherence). |
| **6b** | Routing normalization | SRV-ROUT-003, SRV-ROUT-004, SRV-ROUT-005 | `004-route-normalization` | 6a (for `trailingSlash` config) | Wires phases 3 + 5 of the 13-phase dispatcher. |
| **6c** | cleanUrls (done — `005-clean-urls`) | SRV-ROUT-001, SRV-ROUT-002 | `005-clean-urls` | 6a, 6b | Wires phases 4 + 8. Flipped `_smoke#index_html_redirect` and `mime-defaults#html` from L1-divergent to L0-clean. Also un-deferred the cleanUrls↔trailingSlash compose surface (`prec-cleanurls-trailing*` + `multislash-collapse#double_slash_segment` / `#internal_double_slash`). SRV-ROUT-006's cleanUrls↔redirects/rewrites composition stays deferred until 6d/6e land. |
| **6d** | Configured redirects | SRV-RDIR-001, SRV-RDIR-002, SRV-RDIR-003 | `006-configured-redirects` | 6a, 6c (precedence per SRV-ROUT-006) | Closes Q-007 (relative / scheme-relative / absolute Location forms). Wires phase 6. |
| **6e** | Configured rewrites + `--single` SPA | SRV-RWRT-001, SRV-CLI-008 | `007-configured-rewrites` | 6a, 6d | Wires phase 7. `--single` is implemented as a high-priority rewrite rule per spec text. |
| **6f** | Custom error pages, full L2 security, custom response headers | SRV-FILE-003, SRV-SEC-001 (full surface), SRV-SEC-002, custom-headers SRV (verify ID against `inventory.md` at planning time) | `008-error-pages-and-security` | 6a (custom headers via config) | Custom `<status>.html` per error code; full 400-status surface for SRV-SEC-001 (Stage 5b only wired L0-hygiene 404); single-pass URL decode SRV-SEC-002; `headers` array in `serve.json`. |
| **6g** | Directory listing | SRV-DLST-001, SRV-DLST-002, SRV-DLST-003 | `009-directory-listing` | 6a, 6c (URL trailing slash interaction), 6f (404 fallback when listing disabled) | HTML / JSON content negotiation; `unlisted` and `renderSingle`. Apply D-007 sanitization (relative paths in JSON listing instead of host-absolute). |
| **6h** | CLI fill-in | SRV-CLI-003 (`tcp://`), SRV-CLI-006 (`-p` alias), SRV-CLI-010 (`--cors` — L1 ACAO only), SRV-CLI-014 (`--debug`), SRV-CLI-015 (`--no-request-logging`), SRV-CLI-016 (`--no-port-switching`) | `010-cli-fill-in` | — | Closes Q-001 (`tcp://` host/port defaults). SRV-CLI-009 (`--config`) was closed in 6a as a side-effect of the loader. Several flags are no-ops under D-002. The full L3 CORS response surface stays deferred to Stage 7. |

## Ordering rationale

The dependency edges that drive the order:

- **6a first.** Every other sub-stage reads its parameters from
  `serve.json`. Without the loader, 6c–6g cannot ship as observable
  changes.
- **6b before 6c.** cleanUrls assumes the URL path is already normalized
  (multi-slash collapsed, trailing-slash rule applied). Reversing this
  forces an awkward two-step canonicalization later.
- **6c → 6d → 6e.** This is SRV-ROUT-006 verbatim: cleanUrls 301s come
  before configured redirects, which come before rewrites. Implementing
  in any other order would mask precedence bugs.
- **6f, 6g, 6h flexible.** They depend on 6a but are decoupled from each
  other. Schedule by appetite: 6h is the cheapest (mostly mechanical
  flag declarations), 6g has the most subjective surface (HTML markup is
  D-003 implementation-defined), 6f has the most security-relevant
  decisions (SEC-001 wire-level handling per Q-010 closure).

## Out of scope (deferred to Stage 7+)

The following SRVs are **not** addressed in Stage 6 even though they
appear in `inventory.md`:

- **L3 cache surface** — ETag, Last-Modified, conditional GET, Range
  responses, cache-control headers (SRV-CACHE-*).
- **L3 CORS response surface** — full preflight handling, exposed
  headers, max-age. The L1 piece (basic `Access-Control-Allow-Origin: *`
  on `--cors`) lands in 6h; everything else is Stage 7.
- **Compression** — `--no-compression` flag, gzip/deflate threshold and
  content-type rules (D-006 defers entirely to L3).
- **L4 edge** — UDS sockets, Windows named pipes, symlink resolution
  (`fs.realpath` semantics), TLS, Windows-specific path quirks
  (Q-011 closure).
- **Terminal output polish** — banner, colored output, log line format
  (D-002 excludes verbatim matching; rough log emission may land in 6h
  as no-op handling for `--debug` and `--no-request-logging`).

## Methodological signals to watch for

Each sub-stage may surface divergences between `vercel/serve` behavior
and the spec. The Stage 5b precedent gives the playbook:

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

## Estimated effort

Not a commitment — calibrate against actual sub-stage 6a duration.
Stage 5b consumed ~250k tokens for one vertical slice plus harness work.
Stage 6 sub-stages are individually narrower than 5b (no harness build,
no workspace bootstrap), so 60–100k tokens per fresh session is a
reasonable target. Allowing one planning session, one implementation
session, and one review-round session per sub-stage gives roughly
8 × 3 = 24 sessions for Stage 6. Sub-stages with simple scope (6h, 6b)
may collapse to 1–2 sessions; sub-stages with subjective surface (6g,
6f) may need 4.

## Stage map cross-reference

The condensed view of Stage 6 lives in [`README.md`](../README.md) as
sub-rows 6a–6h. This document is its canonical detail; updates to
status, dependencies, or scope land here first and propagate to the
README row.
