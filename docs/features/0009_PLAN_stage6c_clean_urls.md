# Stage 6c — cleanUrls (extensionless resolution + 301)

## Context

Stage 6b closed the routing-normalization layer (`trailingSlash` 301 + silent multi-slash collapse), leaving explicit comment stubs in the dispatcher for phases 4, 6, 7, 8 (see `crates/irserve-core/src/dispatch.rs:35,53-55`). Stage 6c is the next un-defer slice in the Stage-6 roadmap (`docs/stage6_l1_l2_capabilities.md:51`): it **wires phases 4 and 8** of the 13-phase pipeline (`openspec/changes/001-port-minimal-static-server/design.md` §4) and un-defers `SRV-ROUT-001`, `SRV-ROUT-002`, plus partially `SRV-ROUT-006` (cleanUrls↔trailingSlash composition; full closure of SRV-ROUT-006 waits for 6d/6e).

The capability spec (`openspec/specs/routing/spec.md`) is already `verified` for SRV-ROUT-001/002 — no contract text changes needed, only oracle-list extensions and a per-stage entry in `decisions.md` (D-011, mirroring D-009/D-010).

After 6c, the following becomes observable:
- `GET /index.html`, `GET /about.html`, `GET /<dir>/index.html` → 301 to the extension-stripped form (phase 4).
- `GET /about` → 200 from `/about/index.html`, or `/about.html` (phase 8, index-first).
- The config form `cleanUrls: ["/docs/**"]` constrains both effects via globs.
- The README claims «`_smoke#index_html_redirect` and `mime-defaults#html` flip from L1-divergent to L0-clean» and «unblocks `default-port-l0`» become true.

## Constraints (decided in planning)

- **Full scope in 6c:** both `cleanUrls: bool` and `cleanUrls: string[]` (globs) are implemented. This closes SRV-ROUT-001/002 in full and revives the existing `cleanurls-array.json`. Dependency: one new crate (`globset`, latest stable verified via context7).
- **Compose probes flip to L0-clean in 6c:** `prec-cleanurls-trailing.json`, `prec-cleanurls-trailing-false.json`, and the cleanUrls-dependent anchors `multislash-collapse#double_slash_segment` / `internal_double_slash` (currently reference-only per D-010) flip into `runner.l0.clean` in slice 3.
- **Default = `true`.** When `serve.json` is absent or the `cleanUrls` field is unset, behavior = `cleanUrls: true` (matches `serve-handler/src/index.js:256-274` and the spec scenario «default config and fixture has `index.html` → 301 → /index»). The effective-getter is encapsulated in a single function so 6c–6e read from one source of truth.
- **3-slice slicing per the 6b precedent:** one green-state commit per phase + meta slice. Standing memory rule: ask before each `git commit`.
- **Stop-the-line on divergence:** if a probe against the reference shows unexpected behavior (see open questions below), do not paper over — open a Q-NNN/D-NNN before fixing.
- **Contract text unchanged.** The spec delta `005-clean-urls/specs/routing/spec.md` is a MODIFIED extension of oracle lists only, for the new ORC-IDs and the flipped anchors.

## Critical files

### Read / modify

- `crates/irserve-core/src/dispatch.rs:35,53-55` — comment stubs for phases 4 and 8; one-line call insertion at each. `redirect_301` + `encode_uri_target` (lines 95–104, 76–93) are reused for the cleanUrls 301.
- `crates/irserve-core/src/resolve.rs:15-53` — `resolve()`: phase 8 either wraps it or extends it (see Architecture).
- `crates/irserve-core/src/config.rs:31-56` — `ServeConfig.clean_urls: Option<BoolOrGlobs>` already present; add the effective-getter and (if needed at compile time) precompiled `GlobSet`.
- `crates/irserve-core/src/lib.rs` — register the new module.
- `crates/irserve-core/Cargo.toml` — add `globset` (or `glob`) after a context7 check.
- `tools/probe/cases/_smoke.json:9-14` — flip `index_html_redirect` from `divergent` to `clean`.
- `tools/probe/cases/mime-defaults.json:18-22` — flip `html` from `divergent` to `clean`.
- `tools/probe/cases/prec-cleanurls-default.json` — add a `runner.l0.clean` block.
- `tools/probe/cases/cleanurls-array.json` — add a `runner.l0.clean` block.
- `tools/probe/cases/prec-cleanurls-trailing.json`, `prec-cleanurls-trailing-false.json` — add a `runner.l0.clean` block (compose tests).
- `tools/probe/cases/multislash-collapse.json` — extend `runner.l0.clean` with `double_slash_segment`, `internal_double_slash`.
- `docs/reference/serve/decisions.md` — append **D-011** (un-defer SRV-ROUT-001/002 + partial SRV-ROUT-006 cleanUrls↔trailingSlash composition).
- `docs/reference/serve/open-questions.md` — mark Q-005 closed (if not already — currently verified-by-probe; verify).
- `docs/stage6_l1_l2_capabilities.md`, `README.md` — flip 6c row → done; update «Try IrServe» (include cleanUrls examples) and «What is NOT yet observable» (drop cleanUrls).

### New

- `crates/irserve-core/src/clean_urls.rs` — capability module:
  - `effective(cfg: &Option<BoolOrGlobs>) -> CleanUrlsView` — normalization (`None → Bool(true)`).
  - `GlobSet` precompilation from `Vec<String>` (lazy/cached at startup, not per request).
  - `compute_clean_urls_redirect(decoded_path: &str, view: &CleanUrlsView) -> Option<String>` — phase 4. Regex/match on `(\.html|/index)$`, strip, double-slash collapse (mirrors `serve-handler/src/index.js:121-143`).
  - `try_clean_urls_resolve(url_path: &str, root: &Path, view: &CleanUrlsView) -> Option<ResolveOutcome>` — phase 8. Index-first: `<P>/index.html`, then `<P>.html`. Returns `None` if neither candidate exists — the dispatcher falls through to the existing `resolve()`.
  - Unit tests: scope checks (bool/array), redirect (`/foo.html`, `/index.html`, `/dir/index.html`, `//foo.html` → collapse), exemptions (path without `.html` and without `/index` → None; out-of-scope glob → None), resolve (index-first, fallback to `<P>.html`, both absent → None, out-of-scope glob → None).
- `openspec/changes/005-clean-urls/{proposal,design,tasks}.md` + `specs/routing/spec.md` MODIFIED delta (oracle list extension only).
- `docs/features/0009_PLAN_stage6c_clean_urls.md` — feature plan (continues 0001..0008).
- (Optional) extra probe `cleanurls-index-self.json` covering specific variants `/foo/index.html` → 301 `/foo` and `/foo/index` → 301 `/foo`, if existing probes turn out not to reach them (verify in slice 1).

## Architecture

### Where in the dispatcher

After `decoded_path` (line 33) and before phase 5 (trailingSlash) — i.e. phase 4 runs **first** among routing stages. Exactly as required by SRV-ROUT-006 (precedence): the cleanUrls 301 wins over the trailingSlash 301, redirects, rewrites, and the resolve short-circuit.

```
Phase 1–2 (method gate)            ← existing
URL-decode                         ← existing (entry of dispatch)
Phase 4 (cleanUrls 301)            ← NEW slice 1
Phase 5 (trailingSlash 301)        ← existing
Phase 3 (slash-collapse)           ← existing
Phase 6 (redirects)                — comment stub (Stage 6d)
Phase 7 (rewrites + --single)      — comment stub (Stage 6e)
Phase 8 (cleanUrls resolve)        ← NEW slice 2
Phases 9–13 (resolve → MIME → 404) ← existing
```

Important: phase 4 operates on the decoded path (like phase 5), **before** slash-collapse, so `//foo.html` correctly resolves to `301 /foo` via the combination «strip `.html`» + «collapse `//` in Location» (matching `serve-handler/src/index.js:135-138`).

### Phase 4 (slice 1) — cleanUrls 301

Pseudocode:
```rust
fn compute_clean_urls_redirect(decoded: &str, view: &CleanUrlsView) -> Option<String> {
    if !view.applicable(decoded) { return None; }            // bool=false or out-of-scope glob
    // Match (\.html|/index)$, including the implicit /index.html → /index then strip /index again.
    let stripped = strip_html_or_index_suffix(decoded)?;     // None if no match
    let collapsed = collapse_consecutive_slashes(&stripped); // safety against //
    if collapsed == decoded { return None; }                 // no-op (e.g. "/foo.txt")
    Some(collapsed)
}
```

Edge cases (cover with tests + probe verification):
- `/index.html` → `/index` → `/` (double strip; matches `serve-handler` regex). Cross-check against the reference: the existing `_smoke#index_html_redirect` snapshot expects `/index` (single strip), not `/`. ⇒ Use **regex `(\.html|/index)$` with EXACTLY ONE pass**, as in the reference. Verify the «/index.html → /» scenario via a separate probe if one exists (D-NNN or Q-NNN if divergence appears).
- `/foo` (no suffix) → None.
- `/foo.txt` (other extension) → None.
- `//foo.html` → `/foo` (strip + collapse).
- Query string is not preserved in Location (as in the reference, `url.parse().pathname`).

### Phase 8 (slice 2) — extensionless resolution

Pseudocode (hook in the dispatcher before the existing `resolve()`):
```rust
let url_path = collapse_slashes(&decoded_path);
// ...phase 6/7 stubs...
if let Some(outcome) = try_clean_urls_resolve(&url_path, root, &view).await {
    return file_response_or_404(outcome, &req).await;
}
let outcome = resolve(&url_path, root).await;              // phase 9 (existing)
```

`try_clean_urls_resolve` logic:
1. If `view.bool == false` or out-of-scope glob → return None.
2. If the path **has an extension** (per `path.parse(P).ext != ""` equivalent) → return None (this stage is for extensionless paths; the existing-file pre-stat in `resolve()` handles its own).
3. Try `<P>/index.html` (if path doesn't end with `/`, append; otherwise use as-is). On hit return `Some(ResolveOutcome::Index(canonical))`.
4. Try `<P>.html`. On hit return `Some(ResolveOutcome::File(canonical))`.
5. Otherwise None.

All attempts go through the same canonicalize + escape-root guard as `resolve()` (DRY: extract a private helper `try_resolve_file(path, root)` or reuse a piece of `resolve.rs`).

The index-first order is confirmed by `prec-cleanurls-default` (Q-005 closed); pin it with a test.

### Glob library

Candidates:
- **`globset`** — performance-oriented, batch-matching, idiomatic for `cleanUrls: string[]` (precompile once into a `GlobSet`). Default pick.
- `glob` — simpler, no batch.

In slice 1, look up the current version via `context7-mcp` (per AGENTS.md). Precompile `GlobSet` once at `ServeConfig` load (or on first access via `OnceLock` — pick by simplicity).

Edge case: `cleanUrls: []` (empty array) — out-of-scope for all paths (≡ `false`)? Verify against the reference probe in slice 1.

## Slices

### Slice 1 — Phase 4 (cleanUrls 301)

**Code:**
- New `crates/irserve-core/src/clean_urls.rs` with `CleanUrlsView`, `effective()`, `compute_clean_urls_redirect()`. Unit tests: bool-on/off, in-scope/out-of-scope glob, suffix variants (`.html`, `/index`, `/index.html`), `//`-collapse in result, no-op on no-match, query-strip (if left in decoded).
- Cargo dep: `globset` (version — context7-resolve-id before adding).
- `lib.rs` — register the module.
- `dispatch.rs:35` — replace the comment stub with:
  ```rust
  if let Some(target) = compute_clean_urls_redirect(&decoded_path, &view) {
      return redirect_301(&target);
  }
  ```
  where `view = clean_urls::effective(&serve_config.clean_urls)` (computed once before phase 4; reused in phase 8).

**Probes:**
- `tools/probe/cases/_smoke.json` — flip `index_html_redirect` from `divergent` to `clean`.
- `tools/probe/cases/mime-defaults.json` — flip `html` from `divergent` to `clean`.
- `tools/probe/cases/prec-cleanurls-default.json` — add a `runner.l0.clean` block including `about_html`, `about_no_slash`, `about_with_slash` (the last two also exercise phase 8 — they may fail until slice 2 lands; alternatively add the whole block in slice 2). **Decision:** in slice 1 include only the redirect anchor (`about_html`); the resolve anchors (`about_no_slash`, `about_with_slash`) join in slice 2.
- `tools/probe/cases/cleanurls-array.json` — add only `in_scope_redirect` and `out_of_scope_html_direct` (redirect + pass-through); `in_scope_extensionless` and `out_of_scope_extensionless_miss` stay for slice 2.

**Verify:**
- `cargo test -p irserve-core` (units).
- `cargo test --test oracle` — all enabled anchors green against irserve.
- `node tools/probe/run.mjs --all --target=reference --snapshot=verify` — reference untouched.
- Manual: `curl -i http://127.0.0.1:3010/index.html` → `301 Location: /index`.

**Ask before commit.**

### Slice 2 — Phase 8 (extensionless resolution)

**Code:**
- In `clean_urls.rs` add `try_clean_urls_resolve(url_path, root, view) -> Option<ResolveOutcome>`. Unit tests with a `tempfile` fixture: index-first hit, fallback to `.html`, both-miss → None, out-of-scope → None, extension in path → None.
- `dispatch.rs:55` — replace the comment stub with the hook before the existing `resolve()`.
- If needed, lift a private `try_resolve_file(abs_path, root) -> Option<PathBuf>` out of `resolve.rs` for DRY.

**Probes:**
- `tools/probe/cases/prec-cleanurls-default.json` — extend `runner.l0.clean` to include `about_no_slash`, `about_with_slash` (now full list).
- `tools/probe/cases/cleanurls-array.json` — extend `runner.l0.clean` to include `in_scope_extensionless`, `out_of_scope_extensionless_miss`.
- (Optional) add `cleanurls-html-fallback.json` — fixture with a single `/about.html` (no `/about/index.html`) to verify the fallback branch in isolation. Snapshot via `--snapshot=update --target=reference`. Only if the existing cases don't cover the second branch.

**Verify:**
- Units + `cargo test --test oracle`.
- Verify the `default-port-l0` probe — it should stop «sidestepping the cleanUrls divergence» (check that its `runner.l0` no longer conflicts, or clean it up).
- Manual: `curl -i http://127.0.0.1:3010/about` against a fixture with `/about/index.html` → `200`, body == content of index.

**Ask before commit.**

### Slice 3 — Compose probes flip + meta + un-defer

**Probes:**
- `tools/probe/cases/prec-cleanurls-trailing.json`, `prec-cleanurls-trailing-false.json` — add a `runner.l0.clean` block (compose: cleanUrls + trailingSlash). Verify that snapshots are already correct (recorded in 6b against the reference) — do not overwrite if not needed.
- `tools/probe/cases/multislash-collapse.json` — add `double_slash_segment`, `internal_double_slash` to `runner.l0.clean` (previously divergent per D-010).

**Meta:**
- `docs/reference/serve/decisions.md` — append **D-011: Stage 6c un-defers cleanUrls (phases 4 + 8)**. Mirror the D-010 shape: list of ORC-IDs, exact file:line references to the reference. Clearly record: SRV-ROUT-001 + SRV-ROUT-002 fully un-deferred; SRV-ROUT-006 — partially (cleanUrls↔trailingSlash composition closed; cleanUrls↔redirects/rewrites stays deferred until 6d/6e).
- `docs/reference/serve/open-questions.md` — finalize Q-005 as closed (if not already).
- `openspec/changes/005-clean-urls/`:
  - `proposal.md` — Why (un-defer phases 4+8) + What (capability surface) + Out-of-scope (rewrites/redirects).
  - `design.md` — module map, regex/glob choices, dispatcher order, test strategy. ~250–350 lines.
  - `tasks.md` — slices 1/2/3 in 6b format (T-1.1 .. T-3.x). Verification steps. Hard stops.
  - `specs/routing/spec.md` MODIFIED — extend oracle lists for SRV-ROUT-001/002 (+ partial SRV-ROUT-006). Requirement texts unchanged.
- `README.md` — flip row 6c → done; update «Try IrServe» (add cleanUrls examples with `serve.json` and default); shrink the «What is NOT yet observable» list.
- `docs/stage6_l1_l2_capabilities.md` — flip row 6c → done.
- `docs/features/0009_PLAN_stage6c_clean_urls.md` — re-save this plan (adapted) per the 0001..0008 convention.
- `npx -y @fission-ai/openspec@latest validate --all --strict` — must pass.

**Verify:**
- `cargo test --workspace` — full green.
- `cargo test --test oracle` against both targets — green.
- OpenSpec validate clean.

**Ask before commit; signal readiness for Codex review.**

## Open questions to resolve during implementation

- **`/index.html` → `/index` vs `/`.** The reference's single-pass regex strip yields `/index`, which matches the existing `_smoke#index_html_redirect` snapshot. Verify whether any probe expects `/` (double strip). If so — D-NNN or Q-NNN.
- **Empty array `cleanUrls: []`.** Equivalent to `false`? Probe-verify against the reference in slice 1; D/Q on divergence.
- **Decoded vs encoded path for glob matching.** The reference decodes first, then matches. Pin in code, verify by probe.
- **Extension detection equivalent to `path.parse(P).ext`.** A hidden dot in the basename (`/.bashrc.bak`) — basename has `.bak`, which is an extension. Mirror reference. D-010 already pinned such edge cases for phase 5. In phase 8 the same logic applies — reuse the helper if present, otherwise add one.
- **Glob library version.** Before editing `Cargo.toml`, query the current stable via `context7-mcp` (`globset`).
- **Index-first canonical contract.** `prec-cleanurls-default` already pins Q-005 as closed; finalize the marking.

## Verification

- `cargo test -p irserve-core` — units for `clean_urls` + regressions for `normalize`/`trailing_slash`/`resolve`.
- `cargo test --test oracle` — all cleanUrls probes (`_smoke`, `mime-defaults`, `prec-cleanurls-default`, `cleanurls-array`, `prec-cleanurls-trailing*`, `multislash-collapse`) green against `--target=irserve`.
- `node tools/probe/run.mjs --all --target=reference --snapshot=verify` — reference snapshots untouched.
- `npx -y @fission-ai/openspec@latest validate --all --strict` — clean.
- Manual smoke (mirrors README post-6b examples):
  - `cargo run -- --listen 3010 _tmp` (fixture: `_tmp/index.html`, `_tmp/about.html`, `_tmp/blog/post.html`).
  - `curl -i http://127.0.0.1:3010/index.html` → 301 `Location: /index`.
  - `curl -i http://127.0.0.1:3010/about.html` → 301 `Location: /about`.
  - `curl -i http://127.0.0.1:3010/about` → 200 (body of `/about.html`).
  - With `_tmp/serve.json` `{"cleanUrls": ["/blog/**"]}`: `curl -i …/about.html` → 200 (out-of-scope), `curl -i …/blog/post.html` → 301 `/blog/post`.

## Hard stops (per template)

- `third_party/` — read-only.
- Existing snapshots are touched only if reference behavior actually changed → that's a methodological signal, discuss before patching.
- The `routing/spec.md` contract changes only through a MODIFIED delta or a D-NNN entry after explicit discussion. A Q-NNN entry closes only on probe measurement.
- No `git commit` / `git push` without explicit per-slice approval.
