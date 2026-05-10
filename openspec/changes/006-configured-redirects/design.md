# Design: Configured redirects (phase 6)

## 1. Module map

```
crates/irserve-core/src/
├── dispatch.rs            (modified — phase-6 hook + redirect_with_status)
├── redirects.rs           (NEW)
├── server.rs              (modified — compile_rules at startup; threading)
├── lib.rs                 (modified — register module; re-export RedirectRule was already present)
└── config.rs              (unchanged — RedirectRule already in place)
```

`redirects.rs` is self-contained: the `path-to-regexp`-equivalent
compiler and the destination template parser live inline, since the
combined surface stays under ~400 lines and no other module needs
them. If a third capability (rewrites) ends up duplicating the
compiler in 6e, factor out into `path_pattern.rs` then.

## 2. Phase 6 in the dispatcher

Phase 6 slots between phase 3 (silent multi-slash collapse) and
phase 8 (cleanUrls resolution), operating on the already-normalized
`url_path`:

```
Phase 1–2 (method gate)            ← existing
URL-decode                         ← existing
Phase 4 (cleanUrls 301)            ← Stage 6c
Phase 5 (trailingSlash 301)        ← Stage 6b
Phase 3 (slash-collapse)           ← Stage 6b
Phase 6 (configured redirects)     ← NEW
Phase 7 (rewrites + --single)      — comment stub (Stage 6e)
Phase 8 (cleanUrls resolve)        ← Stage 6c
Phases 9–13 (resolve → MIME → 404) ← existing
```

The reference at `index.js:121-185` builds the same ladder but in
the inverse expression order: cleanUrls → trailingSlash → redirects,
after a single-shot `\/+/ → /` collapse inside the early 301
branches. Here, collapse is factored out into phase 3, so phase 6
receives an already-normalized path. The trailingSlash↔redirect
precedence is pinned by `prec-trailing-redirects#trailing_add_wins_over_redirect`
(ORC-083): trailingSlash always wins because phase 5 returns
before phase 6 is reached.

The dispatch hook is a single `if let Some(...)` statement:

```rust
// Phase 6: configured redirects (Stage 6d).
if let Some((target, status)) = compute_configured_redirects(&url_path, redirect_rules) {
    return redirect_with_status(&target, status);
}
```

`redirect_rules: &[RedirectRuleCompiled]` is threaded into
`dispatch()` as a new parameter alongside the existing
`clean_urls_view`. `server.rs::serve` precompiles the rules at
startup via `redirects::compile_rules` and stores the result in
`AppState` next to `CleanUrlsView`.

## 3. Source-pattern routing (three matchers)

The classifier in `redirects::compile_one` mirrors the reference's
`sourceMatches` codepath at `serve-handler/src/index.js:38-67`,
which always tries `pathToRegExp` first (with `*` → `(.*)`
pre-substitution) and falls back to minimatch only when the regex
returns null. The classifier collapses both legs:

1. **Has `:name` segment OR `*` token** (and no `!`-prefix) →
   `Pattern`. The regex compiler walks the source byte-by-byte:
   - `:name` (where `name` is `[A-Za-z0-9_]+`) → `(?P<name>[^/]+)`.
   - `*` → `(.*)` (anonymous; crosses `/` since `[^/]` is not
     applied).
   - All other runs → `regex::escape(run)`.
   - Final regex anchored `^...\/?$` (matches path-to-regexp's
     default optional trailing slash).
   Routing `*`-bearing sources here (not into `Glob`'s
   `literal_separator(true)`) is what gives `/dir/*` cross-segment
   semantics matching the reference: `/dir/*` matches `/dir/page`
   AND `/dir/sub/page`. Pinned by ORC-084/ORC-085.

2. **Has `?`/`[`/`{` glob meta (no `*`, no `:name`) OR has
   `!`-prefix** → `Glob`. Compiled via
   `globset::GlobBuilder::new(body).literal_separator(true).build()`.
   `negate: bool` mirrors the `!`-prefix XOR pattern shared with
   cleanUrls. A literal `!`-prefixed source (no glob meta in the
   body) still routes through `Glob` so the negation flag applies
   uniformly through the same XOR. The reference's behavior on
   these patterns goes through path-to-regexp first (which usually
   compiles to a regex that fails to match) and then through
   minimatch — for `?`/`[`/`{` patterns IrServe diverges in nuance
   (path-to-regexp's `?` modifier vs minimatch's `?` glob) but the
   probe matrix doesn't currently exercise those corners.
   `!`-negation goes through minimatch in both reference and
   IrServe.

3. **Otherwise** → `Literal`. Trailing-slash flexion handled by
   `literal_matches`: matches if `source == path`, or if `source`
   has no trailing slash and `path` has one extra trailing slash, or
   vice versa. Mirrors `pathToRegExp("/old", []) = ^/old/?$`.

A source with `:name` and `!`-prefix is NOT rejected — it routes
to `Glob` with `negate=true` so the `:name` fragments are treated
as literal characters in the glob (see CompileError below; Codex
review round 2 P1).

### 3.1 Glob meta classification

`has_glob_meta` returns true when the source contains any of `*`,
`?`, `[`, `{`. Mirrors minimatch's standard glob meta-character
set; extglob constructs (`+(...)`, `@(...)`, `?(...)`, `*(...)`,
`!(...)`) are detected only by their leading `?` or `*` and
treated as the corresponding standard glob — `globset` does not
parse the alternation. Inherits Q-012's limitation from cleanUrls.

### 3.2 `:name` detection

`has_path_param` returns true when the source contains a `:`
followed by `[A-Za-z0-9_]+`. A `:` followed by `/` or end-of-string
is treated as literal (so a Pattern matcher is not produced for
sources like `/foo:` or `/a:/b`).

## 4. Destination interpolation

Rendered destinations follow a two-pass encoding model that mirrors
the reference exactly:

1. **Per-value `encodeURIComponent`** at template-render time. Each
   captured value is run through `utf8_percent_encode` with a
   custom `ENCODE_URI_COMPONENT_SET` (NON_ALPHANUMERIC minus the
   six unreserved chars `- _ . ! ~ * ' ( )`). Mirrors
   `pathToRegExp.compile`'s per-prop encoding (`index.js:81-87`).
2. **Final `encodeURI`** at response-build time, via the existing
   `dispatch::encode_uri_target` helper from 6b. Mirrors
   `index.js:586`'s `encodeURI(redirect.target)`.

For a typical alphanumeric `:id` value (e.g. `12`) both passes are
no-ops. For values with reserved chars the two passes can produce
double-encoding artifacts identical to JavaScript's behavior (e.g.
`12 34` → `encodeURIComponent` → `12%2034` → `encodeURI` does NOT
re-encode `%` itself in `dispatch::encode_uri_target`, but DOES
re-encode if the `%` was followed by literal `%xx` already-encoded
content — same as JS spec). The probe matrix doesn't currently
exercise this corner; if it surfaces, the snapshot will pin the
joint behavior.

### 4.1 DestTemplate

```rust
struct DestTemplate { fragments: Vec<DestFrag> }
enum DestFrag { Literal(String), Param(String) }
```

`compile_dest_template` walks the destination string byte-by-byte
looking for `:name` substrings. A `:` not followed by
`[A-Za-z0-9_]+` is appended to the current literal fragment. At
render time, `Param(name)` looks up `captures.name(name)`; a
missing capture renders as empty (fail-open) rather than panicking,
where the reference would throw at request time.

### 4.2 Destination normalization (Q-007)

`normalize_destination(dest)`:
- If `has_protocol(dest)` is truthy, return `dest.to_string()` (the
  reference's `protocol ? destination : slasher(destination)` at
  `index.js:80` short-circuits here).
- Otherwise, `slasher_join_normalize(dest)` — a helper that mirrors
  `path.posix.normalize(path.posix.join('/', value))`: prepends `/`
  before normalization (Codex review round 2 P1 corrected an
  earlier implementation that called `path_posix_normalize` first
  and prepended `/` after, which diverged for leading-`..` and
  empty inputs).

The same `slasher_join_normalize` is used by `slasher` (the
source-pattern preprocessor) for parity — the reference's
`glob-slash.slasher` is also `path.posix.normalize(path.posix.join('/', value))`,
applied to the source after `!`-prefix splitting.

`has_protocol` checks for a valid URL scheme prefix: `[A-Za-z][A-Za-z0-9+.\-]*`
followed by `:`. Mirrors the truthy branch of
`url.parse(dest).protocol` in Node.

`path_posix_normalize` semantics (with unit tests in
`redirects::tests::posix_normalize_*`):

- `path_posix_normalize("a/../b") = "b"` (relative).
- `path_posix_normalize("/a/../b") = "/b"` (absolute).
- `path_posix_normalize("/../../b") = "/b"` (`..` above root drops).
- `path_posix_normalize("../foo") = "../foo"` (relative `..`
  accumulates when nothing left to pop — the leading-slash join
  in `slasher_join_normalize` is what causes destinations like
  `../b` to drop the `..` rather than accumulate it).
- `path_posix_normalize("//example.com/x") = "/example.com/x"`
  (the Q-007 scheme-relative collapse).
- `path_posix_normalize("a/./b") = "a/b"` (`.` segments drop).
- `path_posix_normalize("") = "."`, `path_posix_normalize("/") = "/"`.
- Trailing slash preserved when present.

The Q-007 surprise is the consecutive-slash collapse: a
scheme-relative `//example.com/x` is normalized to `/example.com/x`
and emitted as a same-origin redirect, not as a true scheme-relative
URL. Pinned by ORC-080
(`redirects-destination-forms#scheme_relative`). The `..`-resolution
case is pinned by ORC-086
(`redirects-destination-forms#dotdot_resolved`). If a future user
asks for true scheme-relative behavior, escalate to a D-NNN
divergence — the present change preserves reference parity.

## 5. Status code handling (SRV-RDIR-002)

```rust
fn redirect_with_status(target: &str, status: u16) -> Response<Body> {
    let encoded = encode_uri_target(target);
    let location = HeaderValue::from_str(&encoded)
        .unwrap_or_else(|_| HeaderValue::from_static("/"));
    let status_code = StatusCode::from_u16(status).unwrap_or(StatusCode::MOVED_PERMANENTLY);
    Response::builder()
        .status(status_code)
        .header(LOCATION, location)
        .body(Body::empty())
        .expect("redirect response should always build")
}

fn redirect_301(target: &str) -> Response<Body> {
    redirect_with_status(target, 301)
}
```

The `unwrap_or(StatusCode::MOVED_PERMANENTLY)` fall-back covers the
case where `serde` accepted a `type: 999` (or any value outside
HTTP's `100..=999` range that `axum::http::StatusCode::from_u16`
rejects). This mirrors the spec's "accept any 3xx; range-checking
is not specified" stance — the reference does no validation either,
and JS's `response.writeHead(999, ...)` would produce an arbitrary
non-standard status.

## 6. CompileError variants

```rust
#[derive(Debug, thiserror::Error)]
pub enum CompileError {
    #[error("invalid glob: {0}")]
    Glob(#[from] globset::Error),
    #[error("invalid path pattern: {0}")]
    Regex(#[from] regex::Error),
}
```

`Glob` covers a malformed `globset::Glob`. `Regex` covers a
malformed compiled regex from `compile_source_regex` (rare in
practice — most user-error sources still compile, just to a regex
that doesn't match anything). `!`-prefix combined with `:name` is
NOT rejected (Codex review round 2 P1 reversed the earlier
`NegatedParam` rejection): the combination routes to `Glob` with
`negate=true` and the `:name` fragments are treated as literal
characters in the glob, mirroring the reference's minimatch
fallback at `serve-handler/src/index.js:59`.

## 7. Test strategy

### 7.1 Unit tests (in `crates/irserve-core/src/redirects.rs`)

Coverage per matcher variant:
- Literal: hit, miss, type 302/307, trailing-slash flexion in both
  directions, missing-leading-slash normalization (`slasher`),
  fallback for invalid `type` (`u16` overflow handled at the
  response layer).
- Glob: single-`*` (single segment), `**` (cross-segment), brace
  alternation, `!`-prefix negation, invalid-glob silent skip with
  warning collected.
- Pattern: single `:id`, multi `:a/:b`, `*` token alongside `:id`,
  trailing-slash flexion, `:id` substitution into destination,
  `encodeURIComponent` on the captured value (`a;b` → `a%3Bb`,
  `a b` → `a%20b`), explicit `type` with pattern, no-match falls
  through to next rule, missing-source `:name` in destination
  emits empty fragment, `!`-prefix + `:name` rejected at compile.
- DestTemplate: pure literal, param-only, mixed, lone-`:`-as-literal.
- has_path_param: detects `:name` segments, ignores lone `:`.
- has_protocol: recognizes common schemes, rejects scheme-relative
  and bare paths.
- Destination normalization: passes absolute URL through, collapses
  scheme-relative to `/`, prepends `/` to relative, passes absolute
  path through, keeps Pattern templates intact even when destination
  is `https://...`.

Total: 42 unit tests in `redirects.rs`. Plus `config::tests::redirects_with_type_optional`
(unchanged since 6a) covers `Option<u16>` schema parsing.

### 7.2 Oracle probes

Existing probes `redirects-types`, `prec-rewrites-redirects` get
`runner.l0.clean` blocks (anchors documented in proposal.md). Two
new probes land in slice 3:

- `redirects-destination-forms.json` — Q-007 closure (7 anchors
  after Codex rounds 1+2: absolute URL, scheme-relative,
  relative-no-leading-slash, absolute path baseline, mid-path `..`,
  leading `..`, empty destination).
- `prec-trailing-redirects.json` — trailingSlash↔redirect
  precedence (1 anchor).

After Codex rounds 1, 2, 3, four more probes land:

- `redirects-glob-source.json` (8 anchors) — `*` and `**` source
  routing, multi-`*` minimatch fallback semantics.
- `redirects-source-slasher.json` (2 anchors) — source-side
  `slasher` parity for leading-`..` and `.`-segment.
- `redirects-negation-source.json` (1 anchor) — `!`-prefix +
  `:name` falling through to minimatch.

All redirect anchors mark `contentLengthMayDiffer` (axum vs Node
http chunked encoding, same treatment as 6c's
`cleanurls-array#in_scope_redirect`).

## 8. Stop-the-line decisions during implementation

Two surprises surfaced during implementation that were resolved per
plan:

1. **Q-007 scheme-relative collapse.** The `redirects-destination-forms`
   probe's `scheme_relative` anchor revealed that the reference
   emits `Location: /example.com/x` for destination `//example.com/x`,
   not the expected scheme-relative passthrough. Root cause: the
   reference's `slasher` is `glob-slash`'s
   `path.posix.normalize` plus a leading-slash guarantee, and
   `path.posix.normalize` collapses consecutive slashes. Decision:
   mirror exactly via `redirects::normalize_destination`, document
   the surprise in D-012.

2. **`!`-prefix + `:name` combination.** Slice 2 initially rejected
   this combination at compile time via a (later-removed)
   `CompileError::NegatedParam` variant on the assumption that the
   reference would never match it either. Codex review round 2 P1
   probed and showed that the reference's `sourceMatches` falls
   through to minimatch on these patterns, treating `:name` as
   literal characters and emitting the negated match for any path
   that doesn't literally equal the source. Decision: route
   `!`-prefix + `:name` through `Glob` with `negate=true` (just
   like `!`-prefix without `:name`), drop the `NegatedParam`
   variant, and pin the behavior with
   `redirects-negation-source.json` (ORC-094).

3. **Multi-`*` source / glob fallback.** Slice 1 and round 1
   routed `*`-bearing sources through `Pattern` (regex) only.
   Codex rounds 2 and 3 surfaced the deeper truth: `path-to-regexp@3.3.0`
   doesn't recognize bare `*` as a wildcard — only `(.*)` from the
   JS first-replace step is interpreted as a custom regex segment.
   Multi-`*` sources end up with a regex containing literal `\*`
   that real URLs can't match, and the reference's `sourceMatches`
   falls through to minimatch (single-segment `*` per pattern
   element). Decision: keep `Pattern` (regex) and add a
   `glob_fallback` field on the variant for `*`-bearing sources.
   At match time, try the regex first (so `:name`+`*` sources
   still get path-to-regexp captures), fall back to the glob
   (with a trailing-slash trim mirroring
   `path.posix.resolve(requestPath)` at `index.js:41`) if the
   regex returns no match. Codex round 4 P2 then removed the
   gating on `!has_path_param`: the reference's minimatch
   fallback fires for `:name`-bearing sources too when the
   request literally carries `:name` in the corresponding
   segment, so the fallback now builds unconditionally for any
   `*`-bearing source. Codex round 4 P1 wrapped the
   `GlobMatcher` in a `GlobFallback` struct with
   `pattern_segments` + `has_doublestar` metadata; the
   `matches_strict` method runs the globset match first then
   applies minimatch's `dot: false` default by walking pattern
   segments alongside path segments and rejecting when any
   `*`-bearing pattern segment aligns with a leading-`.` path
   segment. Pinned by `redirects-glob-source.json`: ORC-091
   (multi-trailing 404), ORC-095 (positive single-trailing
   match), ORC-096 (no-trailing 404), ORC-097 (extra-middle
   404), ORC-098/ORC-099 (leading-`.` rejection per
   minimatch `dot: false`); plus
   `redirects-pattern-with-multistar-fallback.json#literal_colon_id_matches_via_minimatch`
   (ORC-100, `:name`+`*` literal-`:name` minimatch hit).

4. **Generalized dot-rejection (Codex round 5 P1).** The round-4
   `matches_strict` used "pattern segment contains `*`" as its
   dot-rejection trigger and skipped all validation when the
   pattern contained `**`. The actual minimatch rule: a path
   segment beginning with `.` matches only when the pattern
   segment ITSELF begins with a literal `.`, regardless of glob
   meta in the rest of the segment. Decision: replace the
   `Option<GlobMatcher>` glob_fallback with a per-segment
   `Vec<PatSeg>` and a recursive `match_segments` walker.
   `PatSeg` is `Literal | Wildcard{matcher, starts_with_dot} |
   DoubleStar`; `Wildcard.starts_with_dot` flags pattern
   segments whose first character is literal `.` (so `.*`
   admits dotfiles, `*` and `[.]y` do not). `DoubleStar`
   recurses with explicit dot-aborting (no segment consumed by
   `**` may begin with `.`). Pinned by ORC-101 (`.*` admits
   leading-dot), ORC-102 (`[.]y` doesn't), ORC-103 (`**`
   doesn't).

5. **Unify Glob with the segment matcher; brace-aware
   `starts_with_dot`; unconditional fallback (Codex round 6).**
   Round 5 applied the segment matcher only inside `Pattern`'s
   `glob_fallback`; `Matcher::Glob` (sources with `?`/`[`/`{`
   glob meta and no `*`/`:name`) still used raw
   `globset.is_match()` and missed dot rejection. Round 6 P1
   refactored `Matcher::Glob` to store `Vec<PatSeg>` and run
   the same `match_segments` walker. Round 6 P1 also fixed
   `starts_with_dot`: the round-5 implementation evaluated the
   flag from the raw segment (so `{.x,y}` looked non-dot
   because it begins with `{`), but minimatch expands braces
   BEFORE applying the dot rule. The new
   `segment_can_start_with_dot` walks brace alternatives
   recursively; any alternative starting with literal `.` flips
   the flag. Round 6 P2 removed the `body.contains('*')` gate
   on `glob_fallback` construction: the reference's
   `sourceMatches` ALWAYS tries minimatch after path-to-regexp
   returns null, regardless of source shape. So `:name`+`?` /
   `:name`+`{}` / `:name`+`[...]` sources also need a fallback
   for the literal-`:name` corner case. Pinned by ORC-104
   (Glob bracket reject), ORC-105 (brace dot-alt admits),
   ORC-106 (brace no-dot-alt rejects), ORC-107 (`:id`+`?`
   minimatch hit), ORC-108 (negation+bracket flips correctly).

6. **Path resolve trim, `**` optional, DoubleStar end-strict
   (Codex round 7).** P1.1: the reference applies
   `path.posix.resolve(requestPath)` before BOTH `pathToRegExp.exec`
   and `minimatch` (`index.js:41`). The round-6 implementation
   only trimmed in the Glob / glob_fallback branches; the Pattern
   regex saw the raw path, so `/a/*` against `/a/` matched
   incorrectly. Decision: trim once at the top of `try_match`.
   P1.2: path-to-regexp's `(.*)*` parsing makes the trailing-`**`
   segment optional+repeat. The round-5 compiler collapsed `**`
   into the same first-only `(.*)` substitution, requiring an
   extra slash segment that real paths don't always carry.
   Decision: refactor `compile_source_regex` to be segment-aware;
   `**` segments emit `(?:/(.*))?` (optional multi-segment).
   P1.3: minimatch's `**` at END of pattern requires ≥1 segment,
   while in MIDDLE it admits zero — asymmetric with pathToRegExp's
   permissive `(.*)*`. The asymmetry shows up via negation: `!/a/**`
   falls to minimatch (because `!`-bearing pattern fails
   pathToRegExp), and minimatch's strict end-`**` semantics
   determine the result. Decision: in `match_segments`'s
   DoubleStar branch, when `**` is the last pattern element
   (`pat.len() == 1`), require `min_skip = 1` and reject empty
   paths. `**` in middle still allows zero-skip. The split
   between routes (positive → Pattern regex permissive, negation
   → Glob segment-matcher strict) mirrors the reference exactly.
   Pinned by ORC-109..117.

7. **Full `path.posix.resolve` and backslash-escape parity
   (Codex round 8).** P1: round-7's trailing-slash trim missed
   `.` / `..` segments. Decision: replace with a full
   `path_posix_resolve` helper that runs `path_posix_normalize`
   (already implemented for destination normalization) followed
   by the trim. Pinned by ORC-118..120. P2: minimatch and
   path-to-regexp treat `\X` as literal `X`. Fix: (a) `de_escape`
   Literal source bodies at compile time so `\.x` literal-matches
   `.x`; (b) compile globset patterns with
   `backslash_escape(true)` for Wildcard variants so `\.*` is
   parsed with `\.` as literal `.`; (c) `segment_can_start_with_dot`
   detects `\.` prefix as effective-leading-dot. Pinned by
   ORC-121 (Literal escape), ORC-122 (Wildcard escape admits
   dotfile).

8. **Backslash transparency for glob meta (Codex round 9).**
   Round 8's `backslash_escape(true)` was correct for `\.` but
   over-generalized: empirical testing of `minimatch@3.1.5`
   (pinned by serve-handler) shows `\*` / `\?` / `\[...]` /
   `\{...}` are TRANSPARENT — the backslash is effectively
   stripped but the meta-character keeps its glob meaning.
   Decision: in `classify_pattern_segment`, `de_escape` the
   segment first, then classify based on the de-escaped form.
   globset compiles the de-escaped pattern with default
   backslash semantics (no `backslash_escape(true)` flag). The
   round-8 `\.`-prefix check in `segment_can_start_with_dot` is
   now redundant — a de-escaped `.X` naturally hits the
   `.`-prefix branch. Pinned by ORC-123 (`\*` keeps glob),
   ORC-124 (`\?`), ORC-125/126 (`\[ab]`), ORC-127 (`\{a,b}`).

## 9. Hard stops

- `third_party/` — read-only.
- Existing snapshots are touched only if reference behavior actually
  changed. None are touched in 6d.
- `openspec/specs/redirects/spec.md` — requirement texts unchanged;
  only oracle lists updated and the Q-007 note refreshed.
- `openspec/specs/routing/spec.md` — only SRV-ROUT-006's oracle list
  grows (ORC-083 added).
