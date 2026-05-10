# Stage 6g — Directory listing (kickoff plan)

## Context

Stage 6g closes L1's directory-listing surface (HTML / JSON content
negotiation, `unlisted`, `renderSingle`) — the last subjective-surface
sub-stage of Stage 6 before 6h's CLI fill-in. The `serve.json` config
fields (`directoryListing`, `unlisted`, `renderSingle`) already exist
in `crates/irserve-core/src/config.rs:43-46` and deserialize cleanly
but are unused by dispatch. Phase 11 of the 13-phase pipeline today
just returns `NotFound` from `resolve.rs:31-35` whenever a directory
has no `index.html`; this stage wires the listing branch.

Reference snapshots already exist for 3 probe cases under
`tools/probe/cases/`: `listing-disabled.json`, `listing-unlisted.json`,
`rendersingle.json` (oracle rows ORC-008..ORC-011). The capability
spec at `openspec/specs/directory-listing/spec.md` is verified-status
already; this stage may emit MOD spec deltas to refine wording (e.g.
the D-007 sanitization shape) but no new requirements.

## Goal

Change package `openspec/changes/009-directory-listing/` (proposal +
design + tasks + spec deltas if needed) plus implementation that
passes all four oracle rows for SRV-DLST-001/002/003 against the
pinned reference. Implementation plan at
`docs/features/0013_PLAN_stage6g_directory_listing.md`.

## Key decisions captured from kickoff interview

1. **HTML markup**: mirror reference structurally (breadcrumb `<h1>`,
   dirs-first sort, parent `..` link, `<ul>` with anchors). No CSS,
   no SVG icons. D-003 keeps body bytes out of the contract.
2. **JSON `directory` / `paths` shape under D-007**: `"."` for root,
   `"sub"` / `"sub/deep"` for nested. No leading or trailing slashes.
   This will need to be spelled out in the design's compatibility note
   (and possibly an explicit Note: line in the spec delta).
3. **`renderSingle` ordering**: mirror the reference literally —
   `files.length === 1` is checked BEFORE the `unlisted` filter. The
   `.DS_Store` + 1-real-file edge case is documented as a Note in
   SRV-DLST-003 and the spec's compatibility section, not as a
   divergence. No new D-NNN.

## Pre-stage out-of-scope list (anti-hallucination rule #10)

These quirks of `renderDirectory` are NOT mirrored; if a probe surfaces
one in review, it lands as a known divergence (`D-NNN, status=adapted`)
rather than a fix:

- Exact byte-level HTML markup (D-003 already covers this).
- File `size` formatting via the `bytes` npm package with
  `unitSeparator: ' '` and `decimalPlaces: 0`. We will format sizes
  in our own way (e.g. `humansize` crate or hand-rolled) and document
  the difference if a snapshot picks up size strings.
- `path.basename(current)` prefixing the `directory` field (sanitized
  away under D-007 per the chosen shape).
- The reference's in-place `delete files[index]` sparse-array pattern
  is an internal implementation detail; we filter eagerly into a
  dense `Vec`.
- `details.ext.split('.')[1] || 'txt'` quirk (no-extension files get
  `ext: 'txt'` upstream). We will document this in the design and
  decide a deterministic mapping (likely empty string `""` for
  no-extension; flag for review).

## Approach

### Slice plan (mirrors 6f's iterative-green-state pattern)

After each green slice, ask before `git commit` (per
[feedback memory](../memory/feedback_iterative_commits.md)).

1. **Slice 1 — Directory outcome plumbing.** Add a `Directory(PathBuf)`
   variant (or equivalent) to the `ResolveOutcome` enum in
   `crates/irserve-core/src/resolve.rs`. When `meta.is_dir()` and no
   `index.html`, return the new variant instead of `NotFound`. In
   `dispatch.rs` near lines 289-335, match the new variant and route
   it to `error_response(404)` for now (preserves existing oracle
   behavior). All 71 existing probe cases stay green.

2. **Slice 2 — HTML listing + `directoryListing` scope.** Add a new
   `crates/irserve-core/src/listing.rs` module. Compile
   `directoryListing: bool | string[]` into a scope-check (reuse
   `BoolOrGlobs` machinery from `clean_urls.rs`). When the new
   `Directory` outcome lands AND the scope check passes, render an
   HTML listing (breadcrumb `<h1>`, dirs-first sort, parent `..`
   when not at root, `<ul>` of links). Emit `Content-Type: text/html;
   charset=utf-8`, status 200. Otherwise fall through to
   `error_response(404)`. Wire `apply_custom_headers` post-pass.
   Probe `listing-disabled` and the HTML half of `listing-unlisted`
   should pass (HTML body is may-differ per ORC-009; only status +
   content-type are must-match).

3. **Slice 3 — JSON listing + D-007 sanitization.** In `listing.rs`,
   branch on `Accept: application/json` (case-insensitive
   `.contains("application/json")` per `serve-handler/src/index.js:556-558`).
   Emit `{"files":[...], "directory": <D-007-shape>, "paths":[...]}`
   with `Content-Type: application/json; charset=utf-8`. The
   `directory` field uses the chosen `"."` / `"sub"` / `"sub/deep"`
   shape (no leading/trailing slashes; `.` for root). The `paths`
   breadcrumb array follows the same sanitization. JSON half of
   `listing-unlisted` should pass (must-match: status, content-type;
   may-differ: body bytes including the sanitized `directory`).

4. **Slice 4 — `unlisted` defaults + user globs.** Hardcode the
   `[".DS_Store", ".git"]` exclusion set (mirrors `serve-handler/src/index.js:330-334`).
   Concatenate with user `unlisted` from config. Compile each entry
   into a glob matcher (reuse `path_pattern.rs` infrastructure).
   Filter `files[]` AFTER stat-walk, BEFORE rendering. Note: filter
   does NOT affect `renderSingle` count (intentional, per slice 5).
   `listing-unlisted` (both halves) verifies this end-to-end.

5. **Slice 5 — `renderSingle`.** When `renderSingle: true` AND the
   raw stat-walk returned exactly 1 entry AND it's a non-directory,
   short-circuit: serve that file directly (status 200, file's MIME,
   body = file bytes). The check happens BEFORE `unlisted` filtering,
   matching reference. Probe `rendersingle` verifies (must-match:
   status, content-type, body=fixture PNG bytes).

6. **Slice 6 — Meta (delegated to subagent).** After all
   implementation slices commit, delegate `proposal.md` / `design.md`
   / `tasks.md` / spec MOD-deltas to a subagent with: slice plan,
   commit log, this plan file, and `openspec/changes/008-error-pages-and-security/`
   as the peer change package to mirror in style. Main agent reviews
   and Edits.

### Critical files to modify

- `crates/irserve-core/src/resolve.rs` — new `ResolveOutcome::Directory`
  variant; remove the eager 404 when `is_dir() && !index.exists()`.
- `crates/irserve-core/src/dispatch.rs:289-335` — match the new variant
  and call into `listing::render`. Listing responses go through
  `apply_custom_headers` post-pass at line 54 of `dispatch()`.
- `crates/irserve-core/src/listing.rs` — NEW. Houses HTML + JSON
  rendering, `unlisted` filter, `renderSingle` check, and
  `directoryListing` scope compilation.
- `crates/irserve-core/src/lib.rs` — export `listing` module's
  pub API.
- `crates/irserve-core/src/config.rs` — possibly add a compiled view
  type `DirectoryListingView` mirroring `CleanUrlsView` (lines 76-104
  of `clean_urls.rs`); wire in `Config::compile()` (or equivalent).
- `tools/probe/run.mjs` — only if `--target=irserve` snapshot
  generation needs adapter changes (unlikely; existing cases should
  cover, since reference snapshots already exist).

### Files to reuse, not duplicate

- `apply_custom_headers` (`crates/irserve-core/src/custom_headers.rs:186-229`)
  — listing HTML/JSON responses post-pass through this.
- `error_response` (`crates/irserve-core/src/error.rs:36-83`) — when
  listing is disabled (or scope doesn't match) and no `index.html`,
  this gives the 404 + custom `<status>.html` lookup.
- `BoolOrGlobs` enum + `CleanUrlsView::from_config` pattern
  (`crates/irserve-core/src/clean_urls.rs:76-104`) — `directoryListing`
  shares the exact same shape; copy the compilation pattern.
- `path_pattern.rs` glob matchers — `unlisted` reuses these.
- `mime.rs` — `renderSingle` uses MIME lookup for the served file.

## Methodological signals to watch for

- If a probe-snapshot byte differs in a way that's not covered by D-003
  / D-007, escalate to a `D-NNN` decision before fixing. Stage 5b
  precedent.
- If `path.basename(current)` (the served-root folder name) leaks into
  any unsanitized field besides `directory`, expand D-007's wording
  via a MODIFIED spec delta.
- If `bytes` formatting of the `size` field surfaces in a snapshot,
  confirm whether may-differ overlay covers it; otherwise document as
  pre-stage out-of-scope item (already listed above).

## Verification

After each slice:

1. `cargo build -p irserve` — must compile clean.
2. `cargo test --test oracle` — all probe cases (existing 71 + the
   3 listing ones once unblocked) must pass against the pinned
   `third_party/serve` reference. Run with `--snapshot=verify`.
3. Manual smoke (per slice 2+):
   ```powershell
   mkdir _tmp; "hi" | Set-Content _tmp\a.txt; "ho" | Set-Content _tmp\b.txt
   cargo run -- --listen 3010 _tmp
   curl -i http://127.0.0.1:3010/                                       # HTML listing
   curl -i -H "Accept: application/json" http://127.0.0.1:3010/         # JSON listing, sanitized
   ```
4. After all slices, run `npx -y @fission-ai/openspec@latest validate
   --all --strict` once spec deltas are written (slice 6).

## Hard stops (per kickoff template)

- Do not modify `third_party/`.
- Existing probe snapshots (`listing-disabled`, `listing-unlisted`,
  `rendersingle`) are touched only if `--target=reference` produces
  different bytes — i.e. only on legitimate reference behavior shift.
- Contract changes (the spec) only via MOD delta or D-NNN entry after
  explicit user discussion.
- Do not commit/push without explicit per-slice approval.

## Sources to re-consult during implementation

- `serve-handler/src/index.js:325-465` — `renderDirectory`
- `serve-handler/src/index.js:644-680` — directory dispatch branch
- `serve-handler/src/index.js:309-323, 387-391, 330-334` — `canBeListed`
  + the in-place delete + the hardcoded excluded set
- `serve-handler/src/index.js:556-558` — `Accept: application/json`
  branch
- `serve-handler/src/index.js:336-374` — `renderSingle` short-circuit
- `openspec/changes/008-error-pages-and-security/` — peer change
  package to mirror in style for slice 6
- `docs/reference/serve/decisions.md` — D-003, D-007, plus any new
  D-NNN this stage authors
