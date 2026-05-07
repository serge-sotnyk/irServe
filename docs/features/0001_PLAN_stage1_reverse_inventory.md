# Stage 1 — Reverse inventory of `vercel/serve`

## Context

`irServe` is an experiment in AI-assisted porting methodology
(`legacy → observable behavior → specs → oracle tests → port`).
Stage 0 produced an empty scaffold: `docs/reference/serve/{inventory,compatibility-levels,decisions,open-questions}.md`,
`openspec/{project.md,AGENTS.md}`, and pinned submodules `third_party/serve@14.2.6`
and `third_party/serve-handler@6.1.7`.

Stage 1 is the **reverse-engineering research layer** — not yet OpenSpec specs and
not implementation. Its sole deliverable is a populated `inventory.md`: a catalogue
of candidate requirements extracted from `serve` / `serve-handler`, each tied to
explicit evidence (README, source path, test name, or ad-hoc oracle probe), each
with a deliberate status from the taxonomy (`candidate / accepted / verified /
adapted / deferred / rejected / unknown`).

Why this stage matters: it is the first place where the anti-hallucination rules
in `AGENTS.md` are enforced under load. Doing it sloppily contaminates every
later stage. Doing it well makes Stages 2–4 (compatibility levels, oracle matrix,
bootstrap baseline) almost mechanical.

**Compatibility scope for this pass: L0–L3.** L4 edge cases (symlinks, Windows
quirks, deep path-traversal corners) are noted but left as `deferred` /
`open-questions.md`. Per AGENTS.md anti-hallucination rule #5, exact terminal UI,
HTML-listing markup, Node middleware API, and bug-for-bug parity are excluded
from inventory and recorded as `rejected` in `decisions.md`.

## Working agreements (chosen, not re-questioning)

- **Authoring**: an explicit subagent writes the inventory; user reviews and
  promotes status (`candidate → accepted` or moves to `decisions.md` /
  `open-questions.md`).
- **Inventory layout**: single monolithic `inventory.md`, sectioned by Area;
  split later only if it exceeds ~1500 lines.
- **Evidence hierarchy** for each SRV: README first, jest test names in
  `third_party/serve-handler/test/` as secondary, source files
  (`serve-handler/src/index.js`, `serve/source/main.ts`) only for precedence /
  MIME / error-handling questions, and ad-hoc probes for behavior that cannot be
  read out of the above.
- **Probing**: Node-based, lives in `tools/probe/`. One `run.mjs` driver +
  declarative `cases/<id>.json` files. Results saved to `tools/probe/results/`
  (gitignored). SRV entries cite `Probe: cases/<id>.json` instead of inlining
  shell output.
- **Granularity**: hybrid as in the PDF — one SRV per *capability* (e.g.
  `cleanUrls`), with one `Requirement candidate` and 1–5 `Scenarios`. CLI flags
  default to one SRV per flag, but compound flags (e.g. `-l/--listen` accepts
  port, `host:port`, or `unix:/path`) get one SRV per distinct mode.
- **`unknown` status**: dual-tracked — entry stays in inventory with
  `Status: unknown`, with a back-reference to a matching `OQ-NNN` in
  `open-questions.md`.

## Plan

### Step A — Tidy `AGENTS.md` and lift methodology into `README.md`

Goal: keep `AGENTS.md` minimal (loaded into every agent context) and move the
authoritative methodology to `README.md` (read by humans and agents on demand).

Changes:

- **`README.md`**: add sections "Stage map", "Anti-hallucination rules",
  "Compatibility levels (summary)" — content lifted verbatim from `AGENTS.md`,
  with the table of stages becoming the canonical version.
- **`AGENTS.md`**: shrink to language policy, anti-hallucination rule #1–7 in
  one-line form, pointers to `README.md#stage-map`,
  `docs/reference/serve/compatibility-levels.md`, `openspec/AGENTS.md`. Drop
  "Getting started", "Project structure" (already in README).
- Verify `CLAUDE.md` still works (`@AGENTS.md` pointer is unchanged).

This step is done **before** the inventory step so the writer-agent reads the
clean `AGENTS.md` + the explicit methodology in README.

### Step B — Probe infrastructure (`tools/probe/`)

Create:

- **`tools/probe/run.mjs`** — single Node script. Signature:
  `node tools/probe/run.mjs <probe-id>` or `--all`.
  Responsibilities:
  1. Pick a random free port (Node `net.createServer().listen(0)` trick).
  2. Spawn `node third_party/serve/build/main.js <args>` against a fixture dir
     described in the case file. Use a temp working dir, suppress clipboard
     (`--no-clipboard`).
  3. Wait for readiness (poll `/`).
  4. Run each request in `cases/<probe-id>.json`'s `requests` array; capture
     status, selected headers (`Content-Type`, `Cache-Control`, `ETag`,
     `Last-Modified`, `Location`), and body shape (length, first 200 bytes for
     HTML, sha256 for binaries).
  5. Write a Markdown report to `tools/probe/results/<probe-id>.md` and a JSON
     to `tools/probe/results/<probe-id>.json`.
  6. Kill the server, clean temp dir.
- **`tools/probe/cases/_schema.json`** — JSON Schema for case files (fixture
  layout, serve args, requests).
- **`tools/probe/cases/_smoke.json`** — first case, sanity check (mirror the
  README smoke test: serve `_tmp/`, GET `/`, expect 200 hello).
- **`tools/probe/README.md`** — 20-line usage doc.
- **`.gitignore`**: add `tools/probe/results/` and `tools/probe/.tmp/`.

Acceptance: `node tools/probe/run.mjs _smoke` exits 0, writes a report
showing GET `/` → 200, body `hello`.

### Step C — Reverse inventory write-up (subagent task)

Spawn a single Explore-class subagent with explicit, narrow instructions. The
agent's allowlist: read `third_party/serve/readme.md`,
`third_party/serve-handler/README.md`, `third_party/serve-handler/src/index.js`,
test names from `third_party/serve-handler/test/`, and `serve/source/main.ts`
(for CLI flag enumeration only). Forbidden: writing OpenSpec changes,
implementing Rust, inventing behavior, reading arbitrary files in the repo.

The agent populates `docs/reference/serve/inventory.md` with these areas
(approximate counts in parentheses are sizing estimates, not quotas):

| Area code | Capability area | ≈ entries |
|---|---|---|
| `CLI`  | command-line flags                              | 10–13 |
| `CFG`  | `serve.json` loading + schema-overview          | 2 |
| `FILE` | static file serving, MIME, 404                  | 4–5 |
| `ROUT` | `cleanUrls`, `trailingSlash`                    | 4–5 |
| `RWRT` | `rewrites`                                      | 2–3 |
| `RDIR` | `redirects`                                     | 2–3 |
| `HDR`  | custom headers                                  | 2 |
| `DLST` | directory listing presence + `unlisted` + `renderSingle` | 3–4 |
| `CACHE`| `etag`, `Last-Modified`, conditional requests   | 3–4 |
| `SEC`  | path traversal, URL decoding                    | 2–3 |
| `SYM`  | symlinks (mostly `deferred` / L4)               | 1–2 |

Each entry follows the format already declared in the skeleton's template:
`SRV-<AREA>-NNN`, `Status`, `Area`, `Priority`, `Reference source`,
`Requirement candidate`, `Scenarios` (GIVEN/WHEN/THEN), `Compatibility notes`,
`Oracle fixture` (placeholder name).

Inventory file header (added in this step): pinned reference versions
(`serve@14.2.6`, `serve-handler@6.1.7`) and submodule SHAs from
`git submodule status`. Out-of-scope areas are listed in `decisions.md`, not
here.

Anything the agent cannot pin to evidence becomes `Status: unknown` *and* gets
an `OQ-NNN` entry in `open-questions.md`. Anything explicitly excluded by
AGENTS.md rule #5 becomes a record in `decisions.md` with status `rejected`.

For precedence questions (rewrites vs redirects vs cleanUrls vs trailingSlash,
encoded path traversal, trailing slash on file vs directory) the agent uses
Step B's probe runner, citing `Probe: cases/<id>.json` in the SRV's
`Reference source` field.

### Step D — User review pass

User reads `inventory.md` end-to-end. Per group:

- Promote `candidate → accepted` for items kept as-is.
- Move clearly-out-of-MVP items into `decisions.md` (status `deferred` or
  `rejected`) with one-line rationale.
- For ambiguous behavior, verify the matching `open-questions.md` entry exists
  and consider running its probe.

This is the gate: Stage 1 is "done" only when no `candidate` rows remain — every
SRV is `accepted`, `deferred`, `rejected`, `unknown` (with OQ ref), or
`verified` (probe ran).

### Step E — Refresh `compatibility-levels.md` (light Stage-2 spillover)

Final Stage-1 polish: walk each L0–L4 bullet and append the SRV-ID(s) that back
it. The file becomes an index from level → SRV, which is what Stage 4
(`000-establish-serve-compatibility-baseline`) will consume. No new bullets
added; only annotation. If a level bullet has no SRV, log it as a Stage-2 task,
do not invent one.

## Critical files

- **Touched**:
  - `AGENTS.md` (shrink)
  - `README.md` (absorb stage map + methodology)
  - `docs/reference/serve/inventory.md` (populate)
  - `docs/reference/serve/decisions.md` (out-of-scope records)
  - `docs/reference/serve/open-questions.md` (OQ-NNN entries)
  - `docs/reference/serve/compatibility-levels.md` (annotate with SRV-IDs)
  - `.gitignore` (probe artifacts)
- **Created**:
  - `tools/probe/run.mjs`
  - `tools/probe/cases/_schema.json`
  - `tools/probe/cases/_smoke.json`
  - `tools/probe/README.md`
- **Read by writer-agent**:
  - `third_party/serve/readme.md`
  - `third_party/serve-handler/README.md`
  - `third_party/serve-handler/src/index.js`
  - `third_party/serve-handler/test/` (file names + describe/it titles only)
  - `third_party/serve/source/main.ts` (CLI option enumeration)

## Verification

- `node tools/probe/run.mjs _smoke` → 200 / body `hello`.
- `inventory.md` contains zero `Status: candidate` rows.
- Every `Status: unknown` row has a matching `OQ-NNN` link.
- Every excluded-by-rule-#5 area has a `decisions.md` record.
- `compatibility-levels.md` L0–L3 bullets each cite ≥1 SRV-ID.
- `git diff --stat` shows changes only under `AGENTS.md`, `README.md`,
  `docs/reference/serve/`, `tools/probe/`, `.gitignore`.
- `AGENTS.md` is shorter than its current 100+ lines and does not duplicate
  README content.
- Spot-check three SRVs by hand: re-derive their evidence (open the cited
  README line / source line / probe report) and confirm it says what the SRV
  claims.
