# Stage 6b — Routing normalization (trailingSlash + multi-slash)

## Context

Stage 6a closed the `serve.json` loader and surfaced eleven typed fields in
`ServeConfig`, but only `public` is actually applied to request handling
(see D-009 in `docs/reference/serve/decisions.md:83-89`). The other fields
— `trailingSlash`, `cleanUrls`, redirects, rewrites, headers, etc. — are
parsed-but-unused, deferred per D-008.

Stage 6b is the next un-defer slice in the L1/L2 roadmap
(`docs/stage6_l1_l2_capabilities.md:50`). It wires phases **3** (silent
multi-slash collapse) and **5** (`trailingSlash` 301) of the 13-phase
dispatcher (`openspec/changes/001-port-minimal-static-server/design.md`
§4), un-deferring **SRV-ROUT-003**, **SRV-ROUT-004**, **SRV-ROUT-005**.
Phase 4 (cleanUrls), phases 6–8 (redirects/rewrites/cleanUrls resolution)
stay deferred for 6c–6e; the dispatcher gets explicit comment-stubs at
those points so 6c lands as a single function call insertion.

The capability spec (`openspec/specs/routing/spec.md`) is already
`verified` for these SRVs — no behavioral spec text changes here, only an
oracle-list extension if new probe cases are added, and a per-stage
un-defer record in `decisions.md`.

## Constraints (decided in planning)

- **Compose probes deferred to 6c.** `prec-cleanurls-trailing*.json` mixes
  `cleanUrls` + `trailingSlash`; in 6b irserve only validates pure
  `trailingSlash` and pure multi-slash. Compose cases stay reference-only
  until phase 4 lands. (User decision in planning chat.)
- **Decoded path is the dispatcher invariant.** URL-decode moves to
  dispatcher entry, ahead of phase 3, mirroring `serve-handler/src/index.js:561`.
  This ensures `%2F%2F` → `//` → `/` collapses to match the reference.
- **Slicing follows the 6a precedent: three slices**, one green-state
  commit each. Per the standing feedback memory, ask before each commit.
- **Spec text unchanged.** Routing requirements are already `verified`.
  The change package's spec delta (if any) only extends the oracle list
  with new ORC IDs for added probe cases.

## Critical files

- `crates/irserve-core/src/dispatch.rs:11-28` — current orchestrator;
  needs signature change to thread `&ServerConfig` and the phase-3/5
  insertions.
- `crates/irserve-core/src/resolve.rs:15-53` — phases 9–11; will receive
  an already-decoded path (drop any future decode there).
- `crates/irserve-core/src/config.rs:31-49` — `ServeConfig::trailing_slash:
  Option<bool>` already loaded at 6a; only consumed here.
- `crates/irserve-core/src/lib.rs` — register two new modules.
- `crates/irserve/src/main.rs` — propagate `ServerConfig` into `dispatch`
  if not already wired.
- `tools/probe/run.mjs` — adapter; verify nothing in `L0_DEFERRED_FLAGS`
  blocks the new probe cases.
- `tools/probe/cases/multislash-collapse.json`,
  `tools/probe/snapshots/multislash-collapse.json` — existing,
  reference-verified, just enable for `--target=irserve`.
- `docs/reference/serve/decisions.md` — append D-010.
- `docs/stage6_l1_l2_capabilities.md`, `README.md` — flip 6b row.
- New: `crates/irserve-core/src/normalize.rs`,
  `crates/irserve-core/src/trailing_slash.rs`.
- New: `tools/probe/cases/trailingslash-add.json`,
  `tools/probe/cases/trailingslash-strip.json` (+ snapshots recorded via
  `--snapshot=update --target=reference`).
- New: `openspec/changes/004-route-normalization/{proposal,design,tasks}.md`
  + `specs/routing/spec.md` MODIFIED delta (oracle list extension only).
- New: `docs/features/0008_PLAN_stage6b_route_normalization.md`
  (continues the 0001..0007 convention).

## Architecture (per Plan-agent recommendation)

**Phase 3** — `normalize::collapse_slashes(path: &str) -> Cow<'_, str>`
in a new `normalize.rs`. Returns input untouched when no `//` is present
(zero allocations on the happy path). Silent — never produces a 301.

**Phase 5** — `trailing_slash::compute_trailing_slash_redirect(path: &str,
cfg: Option<bool>) -> Option<String>` in a new `trailing_slash.rs`.
Mirrors the `shouldRedirect` shape of
`third_party/serve-handler/src/index.js:121-185`:

- `Some(true)` + path doesn't end with `/` + has no extension + name
  doesn't start with `.` → `Some(format!("{path}/"))`
- `Some(false)` + path ends with `/` (and isn't `/` itself) →
  `Some(path[..len-1].to_string())`
- `None` (or no transformation needed) → `None`

Status code: **301** (matches `defaultType` in the reference).

**Dispatcher order** (`dispatch.rs`):

1. Method-allow check (existing).
2. URL-decode the path (moved up from `resolve`).
3. Phase 3: collapse slashes.
4. Phase 4 placeholder: `// Phase 4: cleanUrls 301 (Stage 6c)`.
5. Phase 5: trailing-slash redirect → return 301 if `Some(target)`.
6. Phase 6/7 placeholders (Stage 6d/6e).
7. Phases 9–13 (existing): resolve → MIME → 404.

`dispatch()` signature gains `&ServerConfig` (or its `serve_config`
sub-borrow). This is the most invasive plumbing change in 6b; isolated
in slice 1 to keep diffs reviewable.

## Slices

### Slice 1 — Phase 3 + plumbing
- Add `normalize.rs` + unit tests (root `//`, trailing, internal,
  no-`//` happy path, empty).
- Move URL-decode into `dispatch()` entry; trim it out of `resolve()`.
- Change `dispatch()` signature to thread `&ServerConfig` (or just
  `&ServeConfig`); update `irserve` bin call site.
- Wire phase 3 into the dispatcher with explicit comment-stubs for
  phases 4, 6, 7, 8.
- Enable `tools/probe/cases/multislash-collapse.json` for irserve target;
  green via `cargo test --test oracle`.
- Ask before commit.

### Slice 2 — Phase 5 (trailingSlash 301)
- Add `trailing_slash.rs` with the pure decision function + unit tests:
  add (`/about` + true → `/about/`), strip (`/about/` + false → `/about`),
  no-op when `None`, dotfile exemption (`/.well-known` + true → `None`),
  extension exemption (`/foo.txt` + true → `None`), root edge (`/` +
  false → `None`, since stripping yields empty).
- Hook into dispatcher between phase-4 stub and resolve.
- Add `tools/probe/cases/trailingslash-add.json` (config
  `{"trailingSlash": true, "cleanUrls": false}` + fixture
  `about/index.html`, request `/about` → 301 `/about/`) and
  `trailingslash-strip.json` (mirror with `false` + request `/about/` →
  301 `/about`). Record snapshots via `--snapshot=update --target=reference`,
  then verify against irserve.
- Ask before commit.

### Slice 3 — Meta + un-defer
- Append **D-010** to `docs/reference/serve/decisions.md` (mirrors D-009;
  un-defers SRV-ROUT-003/004/005 from D-008's list).
- Author the change package
  `openspec/changes/004-route-normalization/{proposal,design,tasks}.md`
  + `specs/routing/spec.md` MODIFIED delta (extend oracle lists with new
  ORC IDs for the two new probes).
- Update `README.md` "Try IrServe" section + flip the 6b row to `done`
  in both `README.md` and `docs/stage6_l1_l2_capabilities.md`.
- Author `docs/features/0008_PLAN_stage6b_route_normalization.md`.
- Run `npx -y @fission-ai/openspec@latest validate --all --strict`.
- Ask before commit; signal readiness for Codex review.

## Open questions to resolve during implementation

- Default of `cleanUrls` in the reference vs. `serve-handler` library
  call — does `cleanUrls: false` need to be set explicitly in the new
  trailingSlash probe fixtures, or is it the library default? Verify via
  one quick `--snapshot=update --target=reference` dry run; if surprising,
  log a Q-NNN entry rather than guessing.
- Encoded-slash handling (`%2F%2F`) — confirm via probe that decode-then-
  collapse matches the reference. If divergent, adapt with a D-NNN.
- Root-edge for `trailingSlash: false` on `/`. Reference logic
  (`index.js:152` slice) would yield empty-string target. Confirm via
  reference probe and short-circuit to `None` in our function.

## Verification

- `cargo test -p irserve-core` — unit tests for `normalize` and
  `trailing_slash` modules pass.
- `cargo test --test oracle` — `multislash-collapse`,
  `trailingslash-add`, `trailingslash-strip` green against irserve
  target; all existing snapshots still verified.
- `node tools/probe/run.mjs --all --target=reference --snapshot=verify`
  — reference still matches its committed snapshots (no accidental
  reference touch).
- Manual: `cargo run -- --listen 3010 _tmp` → `curl -i
  http://127.0.0.1:3010///` returns the index (collapsed), and against
  a fixture with `serve.json` `{"trailingSlash": true, "cleanUrls":
  false}` and an `about/` directory, `curl -i …/about` returns 301 with
  `Location: /about/`.
- `npx -y @fission-ai/openspec@latest validate --all --strict` clean.

## Hard stops (per template)

- `third_party/` is read-only.
- Existing snapshots are touched only if reference behavior actually
  changed — that's a methodological signal; discuss before patching.
- No commit / push without explicit per-slice approval.
