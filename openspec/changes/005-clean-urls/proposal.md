# Proposal: cleanUrls (extensionless resolution + 301)

## Why

Stage 6b shipped routing normalization (`004-route-normalization`):
phases 3 (silent multi-slash collapse) and 5 (`trailingSlash` 301)
are wired, with explicit comment-stubs in the dispatcher for phases
4, 6, 7, 8. The next un-defer slice in the L1/L2 roadmap
(`docs/stage6_l1_l2_capabilities.md:51`) is `cleanUrls` — phases 4
and 8 of the 13-phase dispatcher.

This change wires:

- **Phase 4** — `cleanUrls` 301 (SRV-ROUT-001). Mirrors the
  `shouldRedirect` cleanUrls branch at
  `serve-handler/src/index.js:121-143`: a single-pass strip of the
  end-anchored regex `(\.html|\/index)$`, followed by a `\/+/ → /`
  collapse on the result (so `//foo.html` → `/foo`), then
  `ensureSlashStart` to re-prepend `/` if the strip emptied the path
  (so `/index` → `/`). Runs on the **decoded** path (the same
  `decoded_path` invariant phase 5 already operates on), before
  phase 5 — matching the reference's coupling at `index.js:130-133`
  ("strip the HTML parts before handling the trailing slash").
- **Phase 8** — `cleanUrls` extensionless resolution (SRV-ROUT-002).
  Mirrors `findRelated` + `getPossiblePaths('.html')` at
  `serve-handler/src/index.js:276-307`: try `<P>/index.html` first
  and `<P>.html` second, serving the first that exists with status
  200. Index-first is probe-confirmed (Q-005 closed by
  `prec-cleanurls-default`).
- **Pre-stat gating** — mirroring the reference's
  `path.extname(relativePath) !== ''` branch at `index.js:608-642`.
  Extensionless paths skip pre-stat and run phase 8 first; only on
  miss do they fall through to the existing `resolve()` (phases
  9–13). Has-extension paths run `resolve()` first (pre-stat); only
  on `NotFound` do they fall back into phase 8. This avoids phase 8
  shadowing a real `/foo.css` via a `/foo.css.html` fallback.

Both `cleanUrls: bool` and `cleanUrls: string[]` (standard globs)
are implemented in this change, including minimatch-style negation
patterns (`!`-prefix; mirrors `slasher` at `glob-slash.js:8` plus
`nonegate: false` minimatch behavior). The array form is precompiled
once at server start into a `Vec<ScopedPattern>`, where each
pattern carries a `negate: bool` flag — `applicable` evaluates
`matcher.is_match(path) ^ negate` per pattern and short-circuits on
the first truthy result, mirroring the reference's iteration at
`index.js:261-268`. Invalid glob patterns are silently skipped
with a stderr warning (server keeps running), mirroring the
reference's behavior at `index.js:38-67` via `minimatch`.

Glob syntax scope is the **standard glob set** — `*`, `**`, `?`,
character classes, brace alternation, and `!`-prefix negation.
Bash-style extglob (`+(a|b)`, `@(a|b)`, `?(a|b)`, `*(a|b)`,
`!(a|b)`), which `minimatch` honors but `globset` does not, is
out of 6c's scope; tracked as Q-012 with reference-only probe
`tools/probe/cases/cleanurls-extglob.json`.

The compose probes `prec-cleanurls-trailing.json` and
`prec-cleanurls-trailing-false.json` — which were
reference-only at 6b per D-010 because phase 4 wasn't yet
implemented — flip to L0-clean in this change. The cleanUrls-
dependent anchors of `multislash-collapse.json` (`double_slash_segment`,
`internal_double_slash`) likewise flip from `divergent` to `clean`.

`tools/probe/run.mjs::applyL0Filter` is extended: `bodyMayDiffer`
now also strips `body.kind`. Reason: raw-mode probes capture
HTTP/1.1 chunked-encoding terminators (`0\r\n\r\n`, 5 bytes) as a
"binary" body against Node's reference, while irserve's
hyper layer uses `Content-Length: 0` for the same empty 301 — a
transport-encoding choice, not an application-body divergence.
Without the extension, the chunked-encoding tail trips the body-kind
comparison on the multi-slash 301 anchors.

## What

| SRV | Status before | Status after | Module |
|---|---|---|---|
| SRV-ROUT-001 | verified (deferred from IrServe per D-008) | verified, un-deferred from D-008 | `crates/irserve-core/src/clean_urls.rs::compute_clean_urls_redirect`; dispatcher hookup in `dispatch.rs` |
| SRV-ROUT-002 | verified (deferred from IrServe per D-008) | verified, un-deferred from D-008 | `crates/irserve-core/src/clean_urls.rs::try_clean_urls_resolve`; dispatcher hookup with pre-stat gating in `dispatch.rs` |
| SRV-ROUT-006 | verified (deferred from IrServe per D-008) | partially un-deferred — cleanUrls↔trailingSlash composition observable; cleanUrls↔redirects/rewrites stays deferred to 6d/6e | composed at the dispatcher level; unit-level wiring in `clean_urls.rs` and `trailing_slash.rs` |

The change carries MODIFIED deltas on `specs/routing/spec.md` for
SRV-ROUT-001 and SRV-ROUT-002, adding `Implementation:` paragraphs
that link the spec to the new code. The behavioral Scenarios are
preserved verbatim. Requirement bodies and `Evidence:` oracle lists
are unchanged — no new ORC IDs are introduced (existing ORC-002,
ORC-012, ORC-013, ORC-014, ORC-017, ORC-020, ORC-021, ORC-022,
ORC-023, ORC-024, ORC-026, ORC-027 are all flipped from "verified
against reference" to "verified against both reference and irserve"
without renumbering).

## Scope

### In scope

- `crates/irserve-core/src/clean_urls.rs` — **NEW.** Capability
  module containing:
  - `pub struct CleanUrlsView` and `pub fn from_config(&Option<BoolOrGlobs>)
    -> (Self, Vec<InvalidGlob>)` — precompiled view (Off / On /
    Scoped(Vec<ScopedPattern>) — each pattern carries a
    `globset::GlobMatcher` plus a `negate: bool`). Invalid patterns
    are surfaced via the returned `Vec<InvalidGlob>` rather than
    failing startup.
  - `pub fn applicable(&self, decoded_path: &str) -> bool` — mirrors
    `applicable()` at `index.js:256-274`, with per-pattern XOR
    against the `negate` flag.
  - `pub fn compute_clean_urls_redirect(decoded_path, view) ->
    Option<String>` — phase 4. Single-pass strip, `//` collapse,
    `ensureSlashStart`.
  - `pub async fn try_clean_urls_resolve(url_path, root, view) ->
    Option<ResolveOutcome>` — phase 8. Index-first candidates,
    canonicalize + escape-root guard.
  - 19 in-module unit tests covering scope checks, redirect edge
    cases (single-pass strip vs `/index.html` → `/index`, double-strip
    `/index` → `/`, `//foo.html` collapse, trailing-slash blocks
    match), and resolve cases (index-first hit, `<P>.html` fallback,
    out-of-scope skip, root-only `index.html`, `view_off` short
    circuit).
- `crates/irserve-core/src/dispatch.rs` — `dispatch()` signature
  gains `&CleanUrlsView`. Phase 4 hooked between URL-decode and
  phase 5; phase 8 hooked with pre-stat gating around the existing
  `resolve()` call. `url_path_has_extension(path: &str) -> bool`
  helper added at module scope, mirroring Node's `path.extname` for
  our URL-path use case.
- `crates/irserve-core/src/server.rs` — `AppState` carries the
  precompiled `clean_urls_view`; `handler` propagates it into
  `dispatch`. The view is built once in `serve()` from
  `config.serve_config.clean_urls`.
- `crates/irserve-core/src/lib.rs` — `mod clean_urls;`. (No
  `Error` variant: invalid globs are non-fatal warnings surfaced
  via `(CleanUrlsView, Vec<InvalidGlob>)` from `from_config`.)
- `crates/irserve-core/src/resolve.rs` — `#[derive(Debug)]` on
  `ResolveOutcome` so test panics in `clean_urls.rs` can format it.
- `crates/irserve-core/Cargo.toml` + workspace `Cargo.toml` —
  `globset = "0.4"` (workspace-pinned, latest stable on crates.io;
  verified via context7 on 2026-05-09).
- Probe flips:
  - `tools/probe/cases/_smoke.json` — `index_html_redirect`
    moves from `divergent` to `clean`; `contentLengthMayDiffer` lists
    it.
  - `tools/probe/cases/mime-defaults.json` — `html` moves from
    `divergent` to `clean`; `contentLengthMayDiffer` lists it.
  - `tools/probe/cases/prec-cleanurls-default.json` — new
    `runner.l0` block (`clean: [about_html, about_no_slash,
    about_with_slash]`; `contentLengthMayDiffer: [about_html]`).
  - `tools/probe/cases/cleanurls-array.json` — new `runner.l0`
    block (`clean: [in_scope_redirect, in_scope_extensionless,
    out_of_scope_html_direct, out_of_scope_extensionless_miss]`;
    `bodyMayDiffer: [out_of_scope_extensionless_miss]` per the
    notfound-shape precedent; `contentLengthMayDiffer:
    [in_scope_redirect]`).
  - `tools/probe/cases/prec-cleanurls-trailing.json`,
    `tools/probe/cases/prec-cleanurls-trailing-false.json` — new
    `runner.l0` blocks closing the cleanUrls↔trailingSlash compose
    surface that 6b deferred per D-010.
  - `tools/probe/cases/multislash-collapse.json` — `double_slash_segment`
    and `internal_double_slash` move from `divergent` to `clean`;
    `bodyMayDiffer` lists both (chunked-encoding tail).
- `tools/probe/run.mjs::applyL0Filter` — `bodyMayDiffer` now also
  strips `body.kind`. Comment block updated to document the
  transport-encoding rationale.
- Research-track edits: D-011 added in `decisions.md` amending
  D-008's deferred set (drops SRV-ROUT-001, SRV-ROUT-002; notes
  partial un-defer of SRV-ROUT-006).
- README stage-map row 6c → done; "Try IrServe" section refreshed
  with cleanUrls examples; "What is NOT yet observable" list
  shrunk.
- `docs/stage6_l1_l2_capabilities.md` row 6c → done.

### Out of scope

- **Bash-style extglob in `cleanUrls` array patterns**
  (`+(...)`, `@(...)`, `?(...)`, `*(...)`, `!(...)`). The reference
  inherits these from `minimatch`; `globset` does not support
  them. Tracked as Q-012 in
  `docs/reference/serve/open-questions.md`; reference-only probe
  `tools/probe/cases/cleanurls-extglob.json` captures the
  divergence. Closure (manual regex translation, an extglob-capable
  Rust crate, or an `adapted` D-NNN scoping IrServe to standard
  globs) belongs to a future stage.
- Phase 6 (configured redirects): Stage 6d. The compose surface
  cleanUrls↔redirects stays deferred until 6d.
- Phase 7 (rewrites + `--single`): Stage 6e.
- Full SRV-ROUT-006 closure: cleanUrls↔redirects/rewrites
  composition is not yet observable; SRV-ROUT-006 stays
  partially-deferred until 6e lands the last remaining phase.
- Custom error pages, full L2 security, custom response headers:
  Stage 6f.
- Directory listing (HTML / JSON, `unlisted`, `renderSingle`):
  Stage 6g.
- CLI fill-in (`tcp://`, `-p`, `--cors` L1, `--debug`,
  `--no-request-logging`, `--no-port-switching`): Stage 6h.
- L3 cache surface, compression, L4 edge: Stage 7+.
- Bug-for-bug body-kind parity on raw-mode 301 probes. The
  `bodyMayDiffer` mask absorbs the chunked-encoding tail; we don't
  attempt to make irserve's empty-body 301 match Node's chunked
  framing.
- Any change to `third_party/`. No edits to existing snapshots —
  reference behavior is unchanged.

## Findings (methodological signals)

No spec amendments required. Two micro-divergences surfaced and
were absorbed without a new D-NNN entry:

1. **`/foo.css` shadowed by `/foo.css.html` in the most naive
   ordering.** Running phase 8 unconditionally before the existing
   `resolve()` would let a `/foo.css.html` fallback win over an
   actual `/foo.css` file when both exist. The reference's
   `index.js:608-642` flow gates phase 8 on the pre-stat outcome
   (extensionless = skip pre-stat, run findRelated first; has-ext =
   pre-stat first, findRelated only on miss). The implementation
   mirrors this gating exactly via `url_path_has_extension(&url_path)`
   in `dispatch.rs`. No probe currently exercises the `/foo.css` +
   `/foo.css.html` shadowing scenario; the gating is preventive.
2. **Chunked-encoding tail on raw-mode 301 probes.** Reference Node
   http for `response.end()` with no args emits a `0\r\n\r\n`
   chunked terminator (5 bytes binary) for the redirect anchors of
   `multislash-collapse`. axum/hyper uses `Content-Length: 0` and
   no chunked frames. Mediated via the `body.kind` extension to
   `bodyMayDiffer` rather than by changing irserve's redirect emit
   path. Both responses have zero application-level body bytes; the
   divergence is purely transport-level.

## Risks and mitigations

1. **Glob library semantics differ from minimatch.** `globset` is a
   Rust glob library; `serve-handler` uses `minimatch` (Node). Four
   alignments are required so the array-form scope check matches
   the reference:
   (a) `GlobBuilder::new(...).literal_separator(true)` so `*` does
   not cross `/` (minimatch's pathname-aware semantics). Without
   this, `/docs/*` would also match `/docs/sub/page.html`, while
   the reference's minimatch only lets `**` cross segments.
   (b) `applicable` collapses `//` in the request path before
   `is_match`, mirroring `path.posix.resolve(requestPath)` inside
   `sourceMatches` at `index.js:38-67`. Without this, raw-mode
   requests like `GET //docs/guide.html` miss otherwise-matching
   `/docs/**` globs.
   (c) Negation patterns (`!`-prefix). `slasher` preserves the
   `!` (mirrors `glob-slash.js:8`); patterns are stored as
   `Vec<ScopedPattern>` with a per-pattern `negate: bool`;
   `applicable` evaluates `matcher.is_match(path) ^ negate` per
   pattern and short-circuits on the first truthy result, mirroring
   minimatch's `nonegate: false` plus `applicable`'s for-loop at
   `index.js:261-268`. A sole `!/secret/**` thus enables cleanUrls
   for every path outside `/secret/**`.
   (d) Invalid patterns are silently skipped (server keeps running)
   rather than fatal. `from_config` returns `(Self, Vec<InvalidGlob>)`;
   the bin emits stderr warnings per skipped pattern. Mirrors the
   reference's silent-never-match behavior at `index.js:38-67` via
   `minimatch`.
   Alignments (a) and (b) landed in Codex review round 1; (c) and
   (d) landed in round 2 with backing probes ORC-077
   (`cleanurls-negation`, 4 anchors covering pure-negation include
   and exclude legs) and ORC-078 (`cleanurls-invalid-glob#html_no_redirect`,
   server-stays-running anchor). Regressions are pinned by 7 unit
   tests in `clean_urls::tests`: `applicable_single_star_does_not_cross_slash`,
   `applicable_double_star_crosses_segments`,
   `applicable_normalizes_double_slash_path`,
   `applicable_negation_excludes_path`,
   `applicable_mixed_positive_and_negation`,
   `from_config_invalid_glob_is_skipped_silently`,
   `from_config_mixed_valid_and_invalid_keeps_valid`.
2. **`/index.html` → `/index` vs `/`.** The reference regex
   `(\.html|\/index)$/g` does a single-pass replace, yielding
   `/index` (not `/`). The existing snapshot `_smoke#index_html_redirect`
   pins `Location: /index`, confirming single-pass semantics.
   Implementation uses ordered `strip_suffix` calls; tested
   explicitly at `clean_urls.rs::redirect_index_html_strips_only_html_suffix`.
3. **Empty array `cleanUrls: []`.** `Mode::Scoped(Vec::new())`
   makes `applicable.iter().any(...)` return `false` for all paths
   — equivalent to `cleanUrls: false`. Matches the reference's
   `applicable()` iteration which returns `false` on an empty
   array.
4. **Path traversal via `<P>.html` candidates.** `try_clean_urls_resolve`
   canonicalizes each candidate and checks `starts_with(root)` (same
   guard `resolve.rs` uses). Any path-traversal attempt via cleanUrls
   resolution falls through to a 404, not a leak. Tested at
   `clean_urls.rs::resolve_off_short_circuits` and indirectly by the
   existing `traversal-encoded` reference probe (which stays
   reference-only — SRV-SEC-002 work for 6f).
5. **`bodyMayDiffer` semantic change.** Extending it to also strip
   `body.kind` is a minor widening of the L0 mask. No existing
   probes are weakened: the only existing user (`notfound-shape#missing_html`)
   already had matching `text` kind on both sides, so stripping
   `kind` is a no-op for that anchor. Documented in the comment
   block at `tools/probe/run.mjs:623-633`.

## Open assumptions

- **A1 — `cleanUrls` defaults to ON in the reference's
  `applicable()` helper.** Verified at
  `serve-handler/src/index.js:273` (`return true` for the
  non-boolean, non-array fall-through). The default surfaces in
  `_smoke#index_html_redirect` (no `serveJson` block in the case
  file → reference still 301s). The implementation encodes this in
  `CleanUrlsView::from_config` (`None → Mode::On`).
- **A2 — Index-first precedence between `<P>/index.html` and
  `<P>.html`.** Verified by `prec-cleanurls-default#about_no_slash`
  (Q-005 closure): with both files present, `GET /about` serves
  `about/index.html`. The implementation encodes this by trying
  `<P>/index.html` before `<P>.html` in `try_clean_urls_resolve`.
