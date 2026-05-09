# Stage 6d — Configured redirects

## Context

Stage 6c closed `cleanUrls` (phases 4 + 8 of the 13-phase dispatcher). Explicit comment-stubs for phases 6 (redirects) and 7 (rewrites) remain at `crates/irserve-core/src/dispatch.rs:65-66`. Per the roadmap (`docs/stage6_l1_l2_capabilities.md:52`), the next un-defer slice is Stage 6d, OpenSpec change `006-configured-redirects`. It un-defers **SRV-RDIR-001** (default 301 + path-segment interpolation), **SRV-RDIR-002** (`type` override), **SRV-RDIR-003** (absolute-URL destinations), and **closes Q-007** (the `Location` header forms for absolute / scheme-relative / relative destination values).

The contract `openspec/specs/redirects/spec.md` is already `verified` for all three requirements — the requirement texts do not change; only a MODIFIED delta on the oracle lists and a status flip for SRV-RDIR-003 (`accepted` → `verified`) are expected.

After 6d, the following becomes observable:

- `GET /old` with `redirects: [{source: "/old", destination: "/new"}]` → `301 Location: /new`.
- A `type` field (any 3xx) overrides the response status: `302`, `307`, `308`, etc.
- `:param` segments in `source` are interpolated into `destination` (`/old-docs/:id` → `/new-docs/:id`).
- Glob-form sources (`/dir/*`, `/dir/**`) match via `globset` — the standard glob set without extglob (inheriting Q-012's limitation).
- Absolute / scheme-relative / relative `destination` values behave the same as the reference (forms pinned by a probe and snapshot, Q-007 closes).
- `prec-rewrites-redirects.json` (anchor `go_root`) and `redirects-types.json` flip into `runner.l0.clean`. New probe cases land: `prec-trailing-redirects.json` for the trailingSlash↔redirect precedence, and `redirects-destination-forms.json` for Q-007.

## Constraints (decided in planning)

- **Pattern matcher:** a custom mini `path-to-regexp` (token parser for `:name` / `*` / literals → `regex::Regex`) for the path-segment form; `globset` (already a workspace dep) for glob sources. The reference uses `path-to-regexp` v3.3.0 with a `minimatch` fallback (`serve-handler/src/index.js:38-67`); no off-the-shelf Rust analogue exists (see Q-012). Extglob is not supported — we inherit the same limitation as cleanUrls; this must be flagged explicitly in `proposal.md`.
- **Zero-arg path-segment fallback:** if a source has no `:`-params and no `*`, the matcher must collapse to a literal — matching the reference's behavior (`pathToRegExp("/old", []) = ^/old/?$`).
- **Destination interpolation:** string substitution `:name` → captured value. The reference additionally URI-encodes captured values via `pathToRegExp.compile` (encodeURIComponent per segment), then applies `encodeURI` to the entire target (`index.js:586`). We reproduce both layers — otherwise `/old/12 34` would lose the space in Location. Pin this as a methodological item in `design.md`.
- **`type` outside `StatusCode` range:** if `serde` accepts `type: 999`, `axum::http::StatusCode::from_u16` will reject it — fall back to 301 with a stderr warning (mirrors "accept any 3xx; range-checking not specified").
- **Order of evaluation:** first-match-wins (`index.js:172-182`), as in the reference. With no rules, phase 6 is a noop.
- **Decoded vs collapsed path:** phase 6 operates on `url_path` (the post-phase-3 collapsed form), because the reference also operates on the normalized `decodedPath` after the early 301 phases (`index.js:121` accepts the already-decoded path; collapse happens inside the early 301 branches). Pin in `design.md`.
- **3-slice slicing:** literal+glob → path-segment params → Q-007/compose/meta. Standard memory rule applies: ask before each `git commit`.
- **Stop-the-line on divergence:** if the Q-007 probe shows, for example, that the reference renders scheme-relative via direct passthrough, record a D-NNN before patching the code.
- **Contract delta:** `openspec/specs/redirects/spec.md` is touched only for ORC lists and the Q-007 note. Requirement texts are not edited.

## Critical files

### Read / modify

- `crates/irserve-core/src/dispatch.rs:65-66` — comment-stubs for phase 6/7. Replace `// Phase 6: configured redirects (Stage 6d).` with a call to `compute_configured_redirects` plus an early `return redirect_with_status(...)`.
- `crates/irserve-core/src/dispatch.rs:153-162` — generalize `redirect_301` into `redirect_with_status(target, status)` (or add a second constructor without breaking existing call sites).
- `crates/irserve-core/src/config.rs:65-72` — `RedirectRule` already in place (`source`, `destination`, `kind: Option<u16>`); no additions.
- `crates/irserve-core/src/lib.rs` — register the new modules.
- `crates/irserve-core/Cargo.toml` + workspace `Cargo.toml` — add `regex` (workspace dep; verify version via `context7-mcp` before editing).
- `tools/probe/cases/redirects-types.json` — add a `runner.l0.clean` block (anchor-by-anchor: `explicit_302` after slice 1, `default_301_segment` after slice 2).
- `tools/probe/cases/prec-rewrites-redirects.json` — add `runner.l0.clean` for `go_root` (slice 1). `page_html_cleanurl_default` already works post-6c (cleanUrls); its 6d behavior is unchanged — verify the existing `runner.l0.clean` block already covers it or extend.
- `docs/reference/serve/decisions.md` — append **D-012** in the D-011 shape: un-defers SRV-RDIR-001/002/003 + closes Q-007 + explicitly records the "no extglob" limitation (inherited from Q-012).
- `docs/reference/serve/open-questions.md` — Q-007: `Resolution: closed by snapshot tools/probe/snapshots/redirects-destination-forms.json. ORC-NNN, ...`.
- `docs/reference/serve/inventory.md` — flip SRV-RDIR-003 status `accepted` → `verified` (after the Q-007 snapshot lands).
- `docs/reference/serve/oracle-matrix.md` — append new ORC-rows for each new anchor (Q-007 destination-forms, trailingSlash↔redirects compose).
- `docs/stage6_l1_l2_capabilities.md` — flip 6d → done; record "Closes Q-007" with a snapshot pointer.
- `README.md` — flip 6d row → done; in "Try IrServe" add an example with `redirects` in `serve.json`; in "What is NOT yet observable" drop `(6d)`.

### New

- `crates/irserve-core/src/redirects.rs` — capability module:
  - `pub fn compute_configured_redirects(url_path: &str, rules: &[RedirectRuleCompiled]) -> Option<(String, u16)>` — returns `(target, status)` for the first matching rule, otherwise None.
  - `pub fn compile_rules(rules: &[RedirectRule]) -> (Vec<RedirectRuleCompiled>, Vec<InvalidRule>)` — pre-compiled at server start; invalid source/destination patterns emit a stderr warning and are skipped (mirrors the reference, which silently ignores `path-to-regexp` errors via try/catch inside `sourceMatches`).
  - Unit tests: literal match / no match / type override / 301 default / `:param` single & multi / `*` wildcard / out-of-glob / first-match-wins / `:param` substitution into destination / encodeURI on target / fallback for `type=999`.
- `crates/irserve-core/src/path_pattern.rs` (or inline in `redirects.rs` if it stays under ~150 lines) — mini-compiler for source patterns: `:name` → `(?P<name>[^/]+)`, `*` → `(.*)`, literals via `regex::escape`; compile into `regex::Regex`; alongside, store the parameter-name list for destination substitution. Unit tests for the compiler.
- `tools/probe/cases/prec-trailing-redirects.json` — fixture with `trailingSlash: true` + `redirects: [{source: "/face/mask", destination: "/elsewhere"}]`. Anchor: `GET /face/mask` → expectation from the reference (per `index.js:145-168` vs `:172-182` — trailingSlash 301 wins → `/face/mask/`). Snapshot via `--snapshot=update --target=reference`. Add a new ORC row to `oracle-matrix.md` under SRV-ROUT-006.
- `tools/probe/cases/redirects-destination-forms.json` — fixture with three rules:
  - `{source: "/abs", destination: "https://example.com/x"}`
  - `{source: "/proto", destination: "//example.com/x"}`
  - `{source: "/rel", destination: "foo/bar"}`
  Requests: `GET /abs`, `GET /proto`, `GET /rel`. Snapshot via the reference; this snapshot is the answer to Q-007 and defines what irserve must do. Possible surprise — flag in `design.md`: "expected divergence handled by D-012 if reference normalizes the relative form via `slasher`".
- `openspec/changes/006-configured-redirects/`:
  - `proposal.md` — Why (un-defer phase 6) / What (literal + path-segment + glob; `:param` interpolation; absolute-URL passthrough; trailingSlash precedence) / Out of scope (extglob — Q-012; rewrites — 6e).
  - `design.md` — module map (`redirects.rs` + `path_pattern.rs`), regex/globset choice, dispatcher order, `type` fallback handling, rationale for the Q-007 probe. ~280-380 lines.
  - `tasks.md` — slices 1/2/3 in the 6c format (`T-1.1 .. T-3.x`).
  - `specs/redirects/spec.md` MODIFIED — update oracle lists (ORC-030, ORC-031, new ORC rows from 6d), update the Q-007 note (closed).
  - `specs/routing/spec.md` MODIFIED (if SRV-ROUT-006 closes fully now that the redirects↔trailingSlash compose probe lands — otherwise a partial update only).
- `docs/features/0010_PLAN_stage6d_configured_redirects.md` — this plan, persisted into the existing 0001..0009 namespace.

## Architecture

### Where in the dispatcher

Phase 6 slots between phase 3 (collapse) and phase 8 (cleanUrls resolution), operating on the already-normalized `url_path`:

```
Phase 1–2 (method gate)            ← existing
URL-decode                         ← existing
Phase 4 (cleanUrls 301)            ← Stage 6c
Phase 5 (trailingSlash 301)        ← Stage 6b
Phase 3 (slash-collapse)           ← Stage 6b
Phase 6 (configured redirects)     ← NEW slice 1 (literal+glob), slice 2 (params)
Phase 7 (rewrites + --single)      — comment stub (Stage 6e)
Phase 8 (cleanUrls resolve)        ← Stage 6c
Phases 9–13 (resolve → MIME → 404) ← existing
```

The reference at `index.js:121-185` builds the same ladder but in the inverse expression order: cleanUrls → trailingSlash → redirects, after a single-shot `\/+/ → /` collapse inside the early 301 branches. Here, collapse is factored out into phase 3, so phase 6 receives an already-normalized path.

### Slice 1 — Phase 6 wiring (literal + glob)

Pseudocode:

```rust
pub fn compute_configured_redirects(
    url_path: &str,
    rules: &[RedirectRuleCompiled],
) -> Option<(String, u16)> {
    for rule in rules {
        if let Some(target) = rule.try_match(url_path) {
            let status = rule.status_code.unwrap_or(301);
            return Some((target, status));
        }
    }
    None
}
```

In slice 1 `RedirectRuleCompiled` carries only two matcher variants:

- `Literal(String)` — exact equality `source == path`. For sources without `:` and without glob meta-characters.
- `Glob(GlobMatcher)` — `globset::Glob`. For sources with `*`, `**`, `?`, `[...]`, `{a,b}`.

`destination` is rendered as-is in slice 1 (no substitutions), wrapped in `encode_uri_target`.

`dispatch.rs:65` is patched to:

```rust
// Phase 6: configured redirects (Stage 6d).
if let Some((target, status)) = compute_configured_redirects(&url_path, redirect_rules) {
    return redirect_with_status(&target, status);
}
```

`redirect_rules: &[RedirectRuleCompiled]` is threaded into `dispatch()` as a new parameter (alongside `clean_urls_view`).

### Slice 2 — Path-segment params

Add a third variant:

- `Pattern { regex: regex::Regex, param_names: Vec<String>, dest_template: DestTemplate }` — for sources with `:name`. Compiler:
  - Splits `source` on `/` into segments.
  - Per segment: if it starts with `:` — emit `(?P<name>[^/]+)`; otherwise escape the literal via `regex::escape`.
  - `*` → `(.*)`, no name.
  - Anchors the regex `^...$` (optionally with trailing-slash flexion — verify against the reference; likely `^...\/?$`).
  - In parallel compiles the destination into a `DestTemplate` (a vector of fragments: literal or `Param(name)`).
- `try_match` reads named groups and substitutes them into `dest_template`. Each value is run through `utf8_percent_encode` with an `encodeURIComponent`-style ASCII set (a new `ASCII_SET`, distinct from `ENCODE_URI_SET` — encodes `: / ? @` etc.). The fully-rendered target then passes through `encode_uri_target` once more.

### Slice 3 — Q-007 + compose + meta

- Run the new `redirects-destination-forms.json` probe against the reference, record the snapshot.
- Based on the snapshot, write the Q-007 section in `design.md`: how each form is transformed. If the reference uses `slasher` for the relative form, mirror the same normalization.
- Run `prec-trailing-redirects.json` against the reference (snapshot), then add `runner.l0.clean` once irserve is green.
- Append D-012; flip SRV-RDIR-003 to `verified`; update oracle-matrix.
- Author `openspec/changes/006-configured-redirects/`; run `npx -y @fission-ai/openspec@latest validate --all --strict`.
- Update `README.md`, `docs/stage6_l1_l2_capabilities.md`.

## Slices

### Slice 1 — Phase 6 wiring (literal + glob, type override)

**Code:**

- `crates/irserve-core/src/redirects.rs` (literal + glob variants, `compile_rules`, `compute_configured_redirects`).
- `dispatch.rs` — generalize `redirect_301` into `redirect_with_status`; insert the phase 6 call.
- `lib.rs` — register the module.
- Unit tests: literal hit/miss, type 302/307, glob `/dir/*`, glob negation `!`-prefix (decide here — otherwise add a D-NNN footnote), first-match-wins, fallback for `type=999`.

**Probes:**

- `tools/probe/cases/redirects-types.json` — add a `runner.l0.clean` block with anchor `explicit_302` (literal `/old` → 302).
- `tools/probe/cases/prec-rewrites-redirects.json` — add `runner.l0.clean` with anchor `go_root` (literal redirect wins over rewrite).

**Verify:**

- `cargo test -p irserve-core`.
- `cargo test --test oracle` — both anchors green.
- `node tools/probe/run.mjs --all --target=reference --snapshot=verify`.
- Manual: `cargo run -- --listen 3010 _tmp` with a fixture; `curl -i http://127.0.0.1:3010/old` → `302 Location: /new`.

**Ask before commit.**

### Slice 2 — Path-segment params (`:id`, `*`)

**Code:**

- Add `regex` workspace dep (after `context7-mcp` lookup for the current version).
- Extend `redirects.rs` (or factor into `path_pattern.rs`) with the `Pattern` variant: source + destination compiler, `try_match` with substitution.
- Unit tests: `:id` single, `:a/:b` multi, `/old-docs/:id` → `/new-docs/:id`, `*` wildcard, regex anchoring, encodeURIComponent on param value, encodeURI on the final target.

**Probes:**

- `tools/probe/cases/redirects-types.json` — extend `runner.l0.clean` to include `default_301_segment` (`/old-docs/12` → `/new-docs/12`).

**Verify:**

- `cargo test -p irserve-core`.
- `cargo test --test oracle`.
- Manual: `curl -i http://127.0.0.1:3010/old-docs/42` → `301 Location: /new-docs/42`.

**Ask before commit.**

### Slice 3 — Q-007 closure + compose probe + meta + un-defer

**Probes:**

- New `tools/probe/cases/redirects-destination-forms.json` — three rules (absolute, scheme-relative, relative), three requests. Snapshot via `--snapshot=update --target=reference` → committed under `tools/probe/snapshots/`. Then add `runner.l0.clean` for the anchors where irserve agrees with the reference. For divergent anchors (if any), keep a reference-only snapshot and pin the divergence in D-012.
- New `tools/probe/cases/prec-trailing-redirects.json` — `serveJson: {trailingSlash: true, redirects: [{source: "/face/mask", destination: "/elsewhere"}]}`. Request `GET /face/mask`. The reference (`integration.test.js:485-523`) shows trailingSlash wins → `301 Location: /face/mask/`. Snapshot, then `runner.l0.clean`.

**Meta:**

- `docs/reference/serve/decisions.md` — append D-012 in the D-011 shape: list of ORC-IDs, reference file:line citations, list of SRV-IDs (RDIR-001/002/003), the "no extglob (parallel to Q-012)" note, and the explicit Q-007 outcome.
- `docs/reference/serve/open-questions.md` — Q-007: `Resolution: closed by snapshot ...`. Enumerate the new ORC-IDs.
- `docs/reference/serve/inventory.md` — SRV-RDIR-003 status `accepted` → `verified`.
- `docs/reference/serve/oracle-matrix.md` — append rows for `redirects-destination-forms` (3 anchors) and `prec-trailing-redirects` (1 anchor).
- `openspec/changes/006-configured-redirects/`:
  - `proposal.md` — Why / What / Out-of-scope.
  - `design.md` — module map (`redirects.rs`, `path_pattern.rs`), regex+globset choice, rationale for the two-layer URI encoding, phase-6 slot, test strategy. ~280-380 lines.
  - `tasks.md` — slices 1/2/3 in the 6c format (T-1.1 .. T-3.x).
  - `specs/redirects/spec.md` MODIFIED — update oracle lists, Q-007 note (closed).
  - `specs/routing/spec.md` MODIFIED — update SRV-ROUT-006 oracle list (if all compose combinations are now covered).
- `README.md` — flip row 6d → done; "Try IrServe": example with `serve.json` redirects (literal + `:id`); "What is NOT yet observable": drop `(6d)`.
- `docs/stage6_l1_l2_capabilities.md` — flip 6d → done; update "Closes / touches" cells.
- `npx -y @fission-ai/openspec@latest validate --all --strict` — must pass clean.

**Verify:**

- `cargo test --workspace` — full green.
- `cargo test --test oracle` (both targets).
- OpenSpec validate clean.

**Ask before commit; signal readiness for Codex review.**

## Open questions to resolve during implementation

- **Trailing-slash policy in regex source.** The reference's `path-to-regexp@3.x` accepts an optional trailing slash by default (`^/old/?$`). Pin the behavior with a probe (`/old/` against rule `source: "/old"`).
- **Negation in glob source.** The reference supports `!`-prefix via minimatch. `globset` itself does not have per-pattern negation — emulate it inside `compile_rules` (carry a `negate: bool` alongside the matcher, mirroring cleanUrls). Decide in slice 1.
- **Empty `redirects: []`.** Equivalent to the field being absent — phase 6 noop. Unit-test it.
- **Encoded path matching.** The reference matches the decoded path. By the time we hit phase 6, `url_path` is already decoded — pin it with a unit test.
- **`regex` workspace dep version.** Before editing `Cargo.toml` — `context7-mcp resolve-library-id "regex rust"` → `query-docs`.

## Verification

- `cargo test -p irserve-core` — `redirects` + `path_pattern` units + regressions.
- `cargo test --test oracle` — `redirects-types`, `prec-rewrites-redirects#go_root`, `redirects-destination-forms`, `prec-trailing-redirects` green against `--target=irserve`.
- `node tools/probe/run.mjs --all --target=reference --snapshot=verify` — reference snapshots untouched.
- `npx -y @fission-ai/openspec@latest validate --all --strict` — clean.
- Manual smoke (after slice 2):

  ```bash
  cargo run -- --listen 3010 _tmp
  # _tmp/serve.json: {"redirects":[{"source":"/old","destination":"/new","type":302},{"source":"/old-docs/:id","destination":"/new-docs/:id"}]}
  curl -i http://127.0.0.1:3010/old           # 302, Location: /new
  curl -i http://127.0.0.1:3010/old-docs/42   # 301, Location: /new-docs/42
  ```

## Hard stops (per template)

- `third_party/` — read-only.
- Existing snapshots are touched only if the reference behavior actually changed → that is a methodological signal, discuss before patching.
- `openspec/specs/redirects/spec.md` — requirement texts unchanged; only oracle lists and a MODIFIED delta. The contract changes only via a D-NNN entry or a MODIFIED delta after explicit discussion. A Q-NNN entry closes only on probe measurements.
- `git commit` / `git push` — none without explicit per-slice approval.
