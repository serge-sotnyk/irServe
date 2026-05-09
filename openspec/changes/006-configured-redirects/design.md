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

The classifier in `redirects::compile_one`:

1. **Has `:name` segment** (and no `!`-prefix) → `Pattern`. The
   regex compiler walks the source byte-by-byte:
   - `:name` (where `name` is `[A-Za-z0-9_]+`) → `(?P<name>[^/]+)`.
   - `*` → `(.*)` (anonymous; crosses `/` since `[^/]` is not
     applied).
   - All other runs → `regex::escape(run)`.
   - Final regex anchored `^...\/?$` (matches path-to-regexp's
     default optional trailing slash).
   This routing mirrors `serve-handler/src/index.js:46-49`'s
   `slashed.replace('*', '(.*)') + pathToRegExp(normalized, keys)`
   first-pass.

2. **Has glob meta or `!`-prefix** → `Glob`. Compiled via
   `globset::GlobBuilder::new(body).literal_separator(true).build()`.
   `negate: bool` mirrors the `!`-prefix XOR pattern shared with
   cleanUrls. A literal `!`-prefixed source (no glob meta in the
   body) still routes through `Glob` so the negation flag applies
   uniformly through the same XOR.

3. **Otherwise** → `Literal`. Trailing-slash flexion handled by
   `literal_matches`: matches if `source == path`, or if `source`
   has no trailing slash and `path` has one extra trailing slash, or
   vice versa. Mirrors `pathToRegExp("/old", []) = ^/old/?$`.

A source with both `:name` and `!`-prefix is rejected at compile
time via `CompileError::NegatedParam` (see below).

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
- Otherwise, `collapse_slashes(dest)` (mirrors `path.posix.normalize`'s
  consecutive-slash collapse, the only piece of `posix.normalize`
  that actually impacts redirect destinations in practice).
- If the result starts with `/`, return as-is; else prepend `/`.

`has_protocol` checks for a valid URL scheme prefix: `[A-Za-z][A-Za-z0-9+.\-]*`
followed by `:`. Mirrors the truthy branch of
`url.parse(dest).protocol` in Node.

The Q-007 surprise is exactly the consecutive-slash collapse: a
scheme-relative `//example.com/x` is normalized to `/example.com/x`
and emitted as a same-origin redirect, not as a true scheme-relative
URL. Pinned by ORC-080
(`redirects-destination-forms#scheme_relative`). If a future user
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
    #[error("path-segment params (:name) cannot combine with !-prefix negation")]
    NegatedParam,
}
```

`Glob` covers a malformed `globset::Glob`. `Regex` covers a
malformed compiled regex from `compile_source_regex` (rare in
practice — most user-error sources still compile, just to a regex
that doesn't match anything). `NegatedParam` rejects `!`-prefix +
`:name` upfront because path-to-regexp has no negation flag and
the reference's minimatch fallback for that combo treats `:name`
as literal characters anyway. Surfaced as a stderr warning at
startup; the rule is dropped from the compiled list.

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

- `redirects-destination-forms.json` — Q-007 closure (4 anchors).
- `prec-trailing-redirects.json` — trailingSlash↔redirect
  precedence (1 anchor).

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

2. **`!`-prefix + `:name` combination.** During slice 2 the
   classifier needed to choose between Pattern (regex) and Glob
   (globset) routing. `!`-prefix forces Glob (via the cleanUrls
   precedent), but `:name` requires Pattern. Decision: reject the
   combination at compile time via `CompileError::NegatedParam` and
   surface as a stderr warning. The reference's minimatch fallback
   for that combo would never match any real path either, so this
   is a clarification rather than a divergence.

## 9. Hard stops

- `third_party/` — read-only.
- Existing snapshots are touched only if reference behavior actually
  changed. None are touched in 6d.
- `openspec/specs/redirects/spec.md` — requirement texts unchanged;
  only oracle lists updated and the Q-007 note refreshed.
- `openspec/specs/routing/spec.md` — only SRV-ROUT-006's oracle list
  grows (ORC-083 added).
