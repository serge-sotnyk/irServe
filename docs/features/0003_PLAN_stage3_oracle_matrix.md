# Stage 3 — Oracle matrix and snapshot regime

## Context

Stage 2 closed the capability-map gaps and produced 52 SRV entries in
`docs/reference/serve/inventory.md`: 38 `accepted`, 4 `adapted`, 6 `deferred`,
3 `candidate` (SRV-ROUT-006, SRV-CORS-001, SRV-CACHE-005), and 1 `unknown`
(SRV-CACHE-003 / Q-009). Every accepted/adapted SRV currently carries
`Oracle test: planned`; none is `verified`. 25 declarative probe cases under
`tools/probe/cases/` already drive the pinned reference (`serve@14.2.6`,
`serve-handler@6.1.7`), but their results are written to `tools/probe/results/`
which is **gitignored** — so today there is no checked-in evidence that any
specific response shape was the one observed.

Stage 2 also recorded an explicit Stage-3 hand-off in
`compatibility-levels.md`:

> Remaining: an oracle case in Stage 3 must assert these status codes against
> the reference before SRV-SEC-001 is promoted to `verified`.

This stage produces the deliverable named in the README stage map (row 3) —
`docs/reference/serve/oracle-matrix.md` — and turns the existing one-sided
probe runner into a reproducible **oracle regime** by introducing a committed
snapshot directory. The snapshots become the artifact-of-record for what the
pinned reference does, so that the Stage 5b Rust harness has a stable target
to diff against.

**Compatibility scope for the matrix** (locked in user interview before this
plan): all active L0–L2 SRVs (MVP target) plus L3 entries on the disputed
zones from the original methodology (ETag / `If-None-Match`,
`Last-Modified` / `If-Modified-Since`, range requests, `Cache-Control`,
custom `headers`, CORS response surface). L4 (symlinks, Windows path quirks,
UDS, named pipes, TLS) stays `deferred`. Out of scope: any Rust code, any
`openspec/changes/*` work, full bodies for L4 SRVs.

## Outcome

After Stage 3:

- `docs/reference/serve/oracle-matrix.md` exists and catalogues every ORC-NNN
  in scope, each row tying a request to its probe case, the SRV(s) it
  verifies, the "must-match" / "may-differ" partition, and a status
  (`pending` / `recorded` / `verified`).
- `tools/probe/snapshots/<id>.json` is a new **committed** directory holding
  one normalized snapshot per probe case. The schema lives at
  `tools/probe/snapshots/_schema.json`.
- `tools/probe/run.mjs` gains a `--snapshot=<update|verify|none>` switch:
  `update` (re)writes snapshots, `verify` re-runs the case and exits non-zero
  on mismatch, `none` keeps the legacy markdown-only report behavior.
  `verify` is the default whenever a snapshot already exists.
- ~7 new probe cases close the residual L0–L3 coverage gaps identified in
  Step 3 of this plan.
- ~30–40 SRVs flip from `accepted` / `adapted` / `candidate` / `unknown` to
  `verified` as their ORC ships with a committed snapshot. SRVs without an
  ORC stay at their previous status; the matrix's "Coverage gaps" section
  records why.
- `inventory.md` entries gain an `Oracle test: ORC-NNN
  (snapshot tools/probe/snapshots/<id>.json#<request-name>)` line in place of
  the `Oracle test: planned` placeholder, for every SRV that became
  `verified`.
- `open-questions.md`: Q-005 closed-by-probe (cleanUrls/index.html
  precedence, including the array-form), Q-010 wire-level part closed
  (raw-mode probe snapshot now committed). Q-006 closed if the new
  multi-slash probe disambiguates; otherwise re-annotated. Q-009 stays open
  unless the conditional-request probes resolve it.
- `compatibility-levels.md` "Coverage gaps" section gains a
  "(status after Stage 3)" pass; items 5 and 6 are reconciled.
- `README.md` stage map row 3 flips `todo → done` and the repo layout block
  mentions `oracle-matrix.md` and `tools/probe/snapshots/`.
- `AGENTS.md` "Pointers" list gains a one-line pointer to the matrix.

Out of scope, deliberately deferred:

- Any Rust code, `Cargo.toml`, or `tests/oracle/` scaffolding (Stage 5b).
- OpenSpec spec deltas or change folders (Stage 4).
- Promoting `verified` for L4 SRVs (symlinks, TLS, UDS, Windows paths).
- Re-running probes against a non-pinned `npm install -g serve`.
- Resolving Q-001 / Q-002 / Q-003 / Q-004 / Q-007 / Q-011, which require
  evidence outside the snapshot regime (live network, user-interactive
  flows, TLS certs, or non-Windows runtimes).

## Compatibility target

Unchanged: MVP = L2; L3 = stretch; L4 = explicitly out of scope. The matrix
intentionally covers the **whole** MVP target plus the L3 disputed surface,
so that Stage 4 (OpenSpec bootstrap) inherits a fully-grounded baseline.

## Steps

### 1. Snapshot format design (paper step, no edits)

Lock the on-disk shape that every snapshot file will use. One file per case:
`tools/probe/snapshots/<case-id>.json`. Top-level shape:

```
{
  "$schemaVersion": 1,
  "case": "<case-id>",
  "fixture": { ...echoed from cases/<id>.json (files keys + serveJson) },
  "serveArgs": [...],
  "requests": {
    "<request-name>": {
      "request": { "method": "GET", "path": "/index.html", "mode": "fetch", "headers": {...} },
      "requestLine": "GET /%2e%2e/etc/passwd HTTP/1.1\\r\\n..." (raw mode only),
      "response": {
        "status": 301,
        "headers": {
          "content-type": "text/html; charset=utf-8",
          "location": "/index"
        },
        "headersAny": ["date", "server", "connection"],
        "body": {
          "kind": "text",        // text | binary | empty
          "length": 142,
          "sha256": "…",
          "preview": "<!doctype html>…"   // first 200 bytes utf-8 if text
        }
      }
    },
    ...
  }
}
```

Conventions:

- `headers` records header values that **must** match exactly.
- `headersAny` lists header names that are present-and-recorded but whose
  values may vary between runs (`date`, `server`, possibly `etag` when
  driven by mtime). Such headers MUST be stripped from the diff during
  `verify`.
- `body.preview` is the literal first 200 bytes decoded as UTF-8 (with
  replacement char for invalid sequences) for textual responses; binary
  responses record only `length` + `sha256`.
- Field order is deterministic (sorted keys at every level) so diffs are
  stable.
- Per-request `mode: "raw"` snapshots also include the literal `requestLine`
  string so that the wire-level form (e.g., un-normalized `%2e%2e`) is
  auditable.

The schema file `tools/probe/snapshots/_schema.json` formalizes this.

### 2. Probe runner extension (`tools/probe/run.mjs`)

Add a `--snapshot=<update|verify|none>` CLI flag (mutually exclusive with
each other). Behavior:

- `update`: after capturing responses, write the normalized snapshot to
  `tools/probe/snapshots/<id>.json`, overwriting any prior content.
- `verify`: capture as usual, normalize, then deep-diff against the
  committed snapshot. Mismatches print a unified-style diff and the runner
  exits with code 1.
- `none`: legacy behavior (only writes `results/<id>.{md,json}`).
- Default selection: if `tools/probe/snapshots/<id>.json` exists, `verify`;
  otherwise `none`. `--all` honors per-case auto-selection.

Implementation notes:

- Add a normalization helper that produces the snapshot shape from the raw
  response. The same helper is used both for `update` (write) and `verify`
  (diff target).
- Header allowlist for the `headers` block: existing
  `TRACKED_RESPONSE_HEADERS` (already extended in Stage 2 with the CORS
  surface). Headers outside the allowlist are silently dropped from the
  snapshot but counted in `headersAny` if they exist (so accidentally-leaked
  headers stay auditable).
- Sort headers and JSON keys alphabetically before serialization.
- For raw-mode probes, the runner already records the request line in its
  result file; thread that through into the snapshot.
- Update `tools/probe/cases/_schema.json` only if a new request-level field
  is needed (none anticipated yet).

Update `tools/probe/README.md` "Usage" section with one paragraph about the
snapshot mode and one paragraph about how `verify` is the default. Keep the
existing "Limits" section honest — the runner still does **not** compare two
implementations; it only freezes the reference's behavior.

### 3. Coverage gap analysis (paper step)

Walk every active L0–L2 SRV plus every L3 disputed SRV and tag whether an
existing probe case covers it. Expected residual gaps (to be resolved by
new probe cases in Step 4):

| New case | Backs |
|---|---|
| `cli-help-version.json` | SRV-CLI-019 (`--help` / `--version` exit code 0, body non-empty) |
| `cli-positional-error.json` | SRV-CLI-007 third scenario (two positional args → non-zero exit; runner asserts process exit code, not HTTP) |
| `config-explicit.json` | SRV-CLI-009 (`--config <path>` loads non-default file), SRV-CFG-001 |
| `error-page-custom.json` | SRV-FILE-003 (404.html in fixture is served on miss) |
| `range-request.json` | SRV-CACHE-004 (`Range: bytes=0-3` → 206; out-of-range → 416) |
| `cleanurls-array.json` | SRV-ROUT-001 array-form scope, closes Q-005 |
| `multislash-collapse.json` | SRV-ROUT-005, addresses Q-006 |

Process-level cases (`cli-help-version`, `cli-positional-error`) need a
small runner extension: support a case that records `serve`'s exit code and
stdout/stderr summary instead of HTTP responses. If that complicates the
runner more than ~30 lines, the alternative is to add a tiny separate
helper script `tools/probe/run-cli.mjs` and reference it from the matrix.
Decide during implementation; either path is acceptable.

SRVs deliberately **not** getting an ORC in this stage:

- L4 deferred entries (SRV-CLI-004, 005, 017, 018; SRV-SYM-001; SRV-WIN-001).
- SRVs covered only transitively (e.g., SRV-CLI-006 `-p` deprecated alias —
  trivial, redundant).
- SRVs whose only evidence is source-level (e.g., the multi-`-l` additivity
  note on SRV-CLI-002) where a probe would not add fidelity.

The matrix's "Coverage gaps" section enumerates these with one-line
rationales, so the audit trail is clean for Stage 5b.

### 4. New probe cases (`tools/probe/cases/`)

Author the seven case files listed in Step 3. Each follows
`tools/probe/cases/_schema.json`. For the two CLI cases, depending on the
runner-extension decision, either author them as standard cases with a new
`expect: { exit: 0, stdoutContains: "..." }` block (schema update needed)
or author them as `run-cli.mjs` arguments. Keep fixtures minimal; reuse
existing fixture patterns from `_smoke.json` and Stage-2 cases.

### 5. Snapshot capture pass

Run:

```
node tools/probe/run.mjs --all --snapshot=update
```

This (re)materializes every snapshot file under `tools/probe/snapshots/`.
For each new and existing case, manually inspect the snapshot for:

- absolute filesystem paths leaking into bodies or headers (fail → adjust
  fixture or normalization, never edit the snapshot by hand);
- non-deterministic fields landing in `headers` instead of `headersAny`
  (fail → expand the runner's `headersAny` rules);
- raw-mode probes whose `requestLine` differs from the case's intent (fail
  → fix the runner; %2e%2e must round-trip to the wire literally).

Then run `node tools/probe/run.mjs --all --snapshot=verify` once: must
exit 0 across all cases. This proves idempotence of the regime.

### 6. Author `docs/reference/serve/oracle-matrix.md`

File skeleton:

```
# Oracle matrix

## Status
Stage 3 deliverable. Pinned reference: serve@14.2.6, serve-handler@6.1.7.
Snapshots live under `tools/probe/snapshots/`. The matrix is the index;
the snapshots are the evidence.

## Conventions
- ID format: ORC-<NNN> (gap-free, three-digit, allocated in scope order).
- "Verifies" lists every SRV-* whose scenario this oracle satisfies.
- "Probe" cites `cases/<id>.json#<request-name>` (anchor to a single
  request inside a multi-request case) or `cases/<id>.json` for whole-case
  coverage.
- "Layer" is `must-match` (status, location, body for static files,
  selected headers, response framing) or `may-differ` (Date, Server, exact
  HTML markup of directory listing or error pages, ETag value when serve
  uses mtime). See `compatibility-levels.md` for the principle.
- "Status":
  - `pending` — listed, no snapshot yet (matrix shipped before snapshot).
  - `recorded` — snapshot committed, not yet asserted in CI.
  - `verified` — snapshot committed AND `--snapshot=verify` passes locally.

## Entries

### L0 — minimal useful server
| ID | Area | Request | Verifies | Probe | Layer | Status |
| ORC-001 | static-files | GET / on dir with index.html | SRV-CLI-001, SRV-CLI-007, SRV-FILE-001 | `cases/_smoke.json#root` | must-match: 200, body=fixture index | verified |
| ORC-002 | routing | GET /index.html (cleanUrls default) | SRV-ROUT-001, SRV-CLI-001 | `cases/_smoke.json#index_html_redirect` | must-match: 301, location=/index | verified |
| ... |

### L1 — serve-style CLI and configuration
...

### L2 — routing behavior
...

### L3 (disputed) — HTTP polish
...

## Coverage gaps
Items intentionally not covered in Stage 3:
- SRV-CLI-004 (UNIX socket bind) — deferred to L4; oracle requires Linux CI.
- SRV-CLI-005 (Windows named pipe) — deferred to L4.
- SRV-CLI-017 / SRV-SYM-001 (symlinks) — deferred to L4.
- SRV-CLI-018 / TLS (`--ssl-*`) — deferred to L4; oracle requires fixture certs.
- SRV-WIN-001 — placeholder, deferred.
- SRV-CLI-006 (-p deprecated alias) — covered transitively by every -l probe; no dedicated ORC.
- ...

## How the matrix is updated
- Adding behavior: author a probe case, run `--snapshot=update`, append an
  ORC row in the right level, cross-link the SRV(s).
- Promoting status: any ORC whose snapshot passes `--snapshot=verify`
  end-to-end is `verified`.
- Removing entries: matrix entries are append-only within a stage; deletions
  go through Stage 5b/Stage 6 once Rust-side oracle tests exist.
```

Total expected ORC count: ~50–70 rows (fewer if multi-request probes get
one ORC per probe rather than per request — decide while populating; the
PDF example uses one ORC per request, which is finer-grained but more
verbose, so default to per-request granularity).

### 7. Promote SRV statuses (`docs/reference/serve/inventory.md`)

For every SRV referenced from at least one ORC where Status reaches
`verified` in Step 6:

- Replace `Status: accepted` / `adapted` / `candidate` / `unknown` with
  `Status: verified`. Adapted entries keep their adapted flavor — change
  to `verified` only the status keyword; preserve the body's adaptation
  notes.
- Replace `Oracle test: planned` with
  `Oracle test: ORC-NNN (snapshot tools/probe/snapshots/<id>.json#<request-name>)`.
  If the SRV is verified by multiple ORCs, list them comma-separated.
- For the three `candidate` SRVs (SRV-ROUT-006, SRV-CORS-001,
  SRV-CACHE-005): if covered, jump straight to `verified`; if not, jump to
  `accepted` and explain in the matrix's Coverage gaps why no ORC was
  produced.
- For SRV-CACHE-003 (`unknown`): keep `unknown` unless the conditional
  probes (`etag-conditional`, `range-request`) resolve Q-009; otherwise
  upgrade to `verified` and close Q-009.

SRVs without ORC keep their previous status. Do **not** retroactively
revise the body of a `verified` SRV against the snapshot in this stage —
if the snapshot reveals divergence from the SRV scenarios, log it as an
inconsistency in the matrix's Coverage gaps with a Stage-5b TODO. Anti-
hallucination rule: snapshots win, but rewriting SRV scenarios is a
separate, deliberate task and must not be silently merged into Stage 3.

### 8. Open questions update (`docs/reference/serve/open-questions.md`)

- **Q-005** (cleanUrls / index.html precedence, array-form): closed-by-probe
  with pointer to `cleanurls-array` snapshot. Status `closed`.
- **Q-006** (multi-slash collapse): if `multislash-collapse` snapshot
  unambiguously settles it → `closed`; otherwise add a one-line pointer to
  the snapshot and keep `open` with the residual ambiguity recorded.
- **Q-009** (`If-Modified-Since` under `--no-etag`): if `etag-conditional`
  variant covers the no-etag case → `closed`; otherwise `open` with
  pointer.
- **Q-010** (wire-level path traversal): wire-level part now closed (snap
  exists); the SRV-SEC-001 promotion is the bookkeeping. Mark as
  `closed-by-snapshot tools/probe/snapshots/traversal-raw-encoded.json`.
- All other Q-* stay as they are; they are out of Stage 3 scope.

### 9. Refresh `compatibility-levels.md`

- Top of file: add a one-liner under "Compatibility principle" pointing at
  `oracle-matrix.md` as the index of behaviors verified against the
  reference.
- "Coverage gaps (status after Stage 2)" section: rename to "Coverage gaps
  (status after Stage 3)". For each of the six items, append a Stage-3
  closure line where applicable. Item 6 is closed (SEC-001 verified). Item
  5 stays deferred.

### 10. README and AGENTS pointers

- `README.md`: stage map row 3 → `done`. Add `oracle-matrix.md` to the
  bullet under `docs/reference/serve/` in the repo-layout block, and
  mention `tools/probe/snapshots/` next to `tools/probe/`.
- `AGENTS.md`: in the Pointers list, add a one-line pointer to
  `docs/reference/serve/oracle-matrix.md`. Do **not** restate the matrix
  contents here; the pointer is enough.

### 11. External review pause

After all of the above lands in the working tree, **stop before
committing** and pass the diff to Codex. Do not advance until that pass is
complete and any corrections are applied.

### 12. Single commit

When the review settles, land the work as one commit on `main`:

```
docs(stage-3): oracle matrix and snapshot regime
```

Body summarizes: oracle-matrix.md, snapshots directory and schema, runner
snapshot mode, ~7 new probe cases, ~30–40 SRV promotions to `verified`,
README stage-map flip.

## Critical files

Read-and-edit:

- `README.md` — stage map row 3 + repo-layout pointer.
- `AGENTS.md` — one-line pointer to oracle-matrix.md.
- `docs/reference/serve/inventory.md` — status flips and `Oracle test:`
  lines for ~30–40 SRVs.
- `docs/reference/serve/compatibility-levels.md` — gaps section refresh +
  matrix pointer.
- `docs/reference/serve/open-questions.md` — close Q-005, Q-010 wire-level
  part; annotate Q-006 / Q-009.
- `tools/probe/run.mjs` — `--snapshot=` mode + normalization helper +
  optional process-level case support.
- `tools/probe/README.md` — document snapshot mode.
- `tools/probe/cases/_schema.json` — only if process-level cases need a
  new field.

Created:

- `docs/reference/serve/oracle-matrix.md` — the deliverable.
- `tools/probe/snapshots/_schema.json` — snapshot schema.
- `tools/probe/snapshots/<id>.json` — one per probe case (~32 files).
- `tools/probe/cases/cli-help-version.json`,
  `cli-positional-error.json`, `config-explicit.json`,
  `error-page-custom.json`, `range-request.json`, `cleanurls-array.json`,
  `multislash-collapse.json`.
- (Optional) `tools/probe/run-cli.mjs` — only if Step 3's process-level
  cases are factored out instead of folded into `run.mjs`.

Read-only references (consult, do not edit):

- `third_party/serve-handler/test/integration.test.js` — 54 reference tests
  to cross-check ORC scenarios against (cite test names where they exactly
  match an ORC's intent).
- `third_party/serve-handler/test/fixtures/` — fixture-shape inspiration.
- `third_party/serve/tests/cli.test.ts`, `config.test.ts`, `server.test.ts`
  — for CLI-flag and config-loading evidence.
- `third_party/serve/source/main.ts`, `source/utilities/server.ts` — flag
  enumeration and listen-default reference.
- Existing `tools/probe/results/<id>.{md,json}` (gitignored, regenerable)
  — sanity reference for "what the wire actually looked like" before
  snapshot normalization.

## Verification

End-to-end checks the implementing agent runs before handoff for review:

1. `node tools/probe/run.mjs --list` includes the existing 25 cases plus
   the 7 new ones (32 total).
2. `node tools/probe/run.mjs --all --snapshot=update` exits 0; every
   `tools/probe/snapshots/<id>.json` is written.
3. `node tools/probe/run.mjs --all --snapshot=verify` (the same run on a
   fresh clone of the working tree) exits 0 — proves the snapshot regime
   is idempotent against the pinned reference.
4. Spot-test mismatch path: hand-edit one `headers.location` value in a
   snapshot, run `verify` for that case → exit code 1 with a readable diff.
   Restore the snapshot.
5. Cross-read pass: every L0–L2 active SRV either appears in at least one
   ORC's `Verifies` column or appears in the matrix's "Coverage gaps"
   section with a one-line rationale. Same for the L3 disputed surface
   (HDR-001/002, CACHE-001/002/004/005, CORS-001, RWRT-002 if covered,
   SEC-001 wire-level).
6. Cross-read pass: every SRV whose status flipped to `verified` carries an
   `Oracle test: ORC-NNN (snapshot ...)` line that resolves to a snapshot
   file actually present on disk.
7. `git status` — only files listed under "Critical files" (read-and-edit
   + created) are touched. Nothing under `openspec/`, `crates/`,
   `Cargo.toml`, etc.
8. `compatibility-levels.md` "Coverage gaps" reflects Stage-3 closures;
   item 6 is marked closed by SEC-001 verification.
9. `README.md` row 3 reads `done`. The repo-layout block mentions
   `oracle-matrix.md` and `tools/probe/snapshots/`.

## Anti-hallucination guardrails for this stage

- Snapshots are **recorded**, never hand-edited. If a snapshot looks wrong,
  the fix is upstream of the snapshot — adjust the probe case, the runner,
  or the fixture. Editing a snapshot by hand to "fix the diff" defeats the
  oracle.
- Promotion to `verified` requires the **conjunction** of (a) a probe case,
  (b) a committed snapshot, (c) a `--snapshot=verify` pass against that
  snapshot. Two-out-of-three is not enough. In particular, "the probe
  output looks plausible" is not enough.
- For non-deterministic response fields (Date, Server, sometimes ETag),
  use `headersAny` honestly. Do not normalize a value to a literal in the
  snapshot if the value is volatile across runs.
- For raw-mode probes, the snapshot's `requestLine` is part of the
  contract: it asserts that the test actually went on the wire with the
  unsanitized form. If the runner accidentally normalizes (Node URL
  parser, fetch fallback, etc.), that is a runner bug, not a snapshot
  exception.
- If a snapshot reveals that an existing SRV scenario is wrong, do **not**
  silently update the SRV body. Log the divergence in
  `oracle-matrix.md` "Coverage gaps" as a Stage-5b TODO and keep the SRV's
  current scenario unchanged for this commit. Stage 3's contract is
  `freeze observed behavior`, not `revise interpretations`.
- The matrix does **not** introduce a new compatibility level or a new
  SRV-NNN id. New requirements coming out of probing become Stage-5b/6
  work, not silent additions here.
