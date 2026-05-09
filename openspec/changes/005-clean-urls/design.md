# Design: cleanUrls (extensionless resolution + 301)

This design is **code-level**. The architectural foundations (crate
layout, HTTP-stack pins, 13-phase request lifecycle, oracle harness
layer) live in `openspec/changes/001-port-minimal-static-server/
design.md` (§§1, 4, 6). The contract for the capability lives in
`openspec/specs/routing/spec.md` (SRV-ROUT-001, SRV-ROUT-002,
SRV-ROUT-006).

## 1. Module map (deltas to the Stage-6b layout)

| Module | Delta | Wires |
|---|---|---|
| `crates/irserve-core/src/clean_urls.rs` | **NEW.** `CleanUrlsView` precompiled view (Off / On / Scoped(GlobSet)); `from_config` / `applicable`; `compute_clean_urls_redirect` (phase 4); `try_clean_urls_resolve` (phase 8). 19 in-module unit tests. | phases 4 + 8 |
| `crates/irserve-core/src/dispatch.rs` | `dispatch()` signature gains `&CleanUrlsView`. Phase 4 hooked between URL-decode and phase 5; phase 8 hooked around the existing `resolve()` with pre-stat gating. `url_path_has_extension(path: &str) -> bool` helper added. | dispatcher |
| `crates/irserve-core/src/server.rs` | `AppState` carries `clean_urls_view: CleanUrlsView` built once in `serve()` from `config.serve_config.clean_urls`. `handler` propagates the view into `dispatch`. | startup |
| `crates/irserve-core/src/lib.rs` | `mod clean_urls;`; `Error::CleanUrlsGlob(#[from] globset::Error)` variant for startup-time invalid-glob surfacing. | n/a |
| `crates/irserve-core/src/resolve.rs` | `#[derive(Debug)]` on `ResolveOutcome` so test panics in `clean_urls.rs` can format the variant in error messages. No behavioral change. | n/a |
| `crates/irserve-core/Cargo.toml` + `Cargo.toml` (workspace) | `globset = "0.4"` (workspace-pinned). | n/a |
| `tools/probe/run.mjs` | `applyL0Filter`'s `bodyMayDiffer` branch now also strips `body.kind` (transport-encoding choice; see §6). | runner |

`config.rs`, `mime.rs`, `notfound.rs`, `normalize.rs`,
`trailing_slash.rs` are untouched.

## 2. Pipeline order

The 13-phase pipeline is now realized as follows in `dispatch.rs`:

```
1-2. Method gate (existing)
0.   URL percent-decode at dispatcher entry (decoded_path)         [Stage 6b]
4.   Phase 4: clean_urls::compute_clean_urls_redirect              [Stage 6c]
        if Some(target) -> return redirect_301(&target)
        — operates on decoded_path (uncollapsed) so `//foo.html`
          strips and collapses identically to literal+encoded forms
5.   Phase 5: trailing_slash::compute_trailing_slash_redirect      [Stage 6b]
3.   Phase 3: normalize::collapse_slashes(decoded_path)            [Stage 6b]
6.   /* Phase 6: configured redirects (Stage 6d) */
7.   /* Phase 7: rewrites + --single (Stage 6e) */
8.   Phase 8: clean_urls::try_clean_urls_resolve (pre-stat-gated)  [Stage 6c]
        — extensionless paths: phase 8 first, then phase 9 fallback
        — has-extension paths: phase 9 first; on NotFound, phase 8
9-13. Resolve -> MIME -> 404 (existing)
```

Phase 4 is positioned **before** phase 5 in keeping with the
reference's coupling at `serve-handler/src/index.js:130-133`:

> By stripping the HTML parts from the decoded path *before*
> handling the trailing slash, we make sure that only *one* redirect
> occurs if both config options are used.

That ordering is what makes `GET /about.html` under `trailingSlash:
true` 301 to `/about` (cleanUrls strip wins) rather than to
`/about.html/` (trailingSlash add) or `/about/` (compose). Compose
probes `prec-cleanurls-trailing#about_html` and
`prec-cleanurls-trailing-false#about_html` pin this behavior.

## 3. `CleanUrlsView` and `applicable`

Reference: `serve-handler/src/index.js:256-274` (`applicable`),
`./glob-slash.js` (`slasher`).

```rust
pub struct CleanUrlsView { inner: Mode }

enum Mode {
    Off,                  // cleanUrls: false
    On,                   // cleanUrls: true (default when absent)
    Scoped(GlobSet),      // cleanUrls: ["/docs/**", ...]
}

impl CleanUrlsView {
    pub fn from_config(cfg: &Option<BoolOrGlobs>) -> Result<Self, globset::Error> {
        match cfg {
            None => Mode::On,                                    // serve-handler default
            Some(Bool(true)) => Mode::On,
            Some(Bool(false)) => Mode::Off,
            Some(Globs(patterns)) => Mode::Scoped(build_glob_set(patterns)?),
        }
    }
    pub fn applicable(&self, decoded_path: &str) -> bool {
        match &self.inner {
            Off => false, On => true, Scoped(set) => set.is_match(decoded_path),
        }
    }
}
```

Glob normalization (`slasher` mirror): patterns without a leading
`/` get one prepended before they enter `GlobSetBuilder`. We do not
replicate the full `path.posix.normalize` (collapsing `..` etc.) —
cleanUrls patterns shouldn't carry such segments in practice; if
they do, that's a methodological signal for a future Q-NNN entry.

Negation patterns (`!/secret/**`) are passed verbatim to globset.
The reference's iteration logic (line 261-268) also doesn't honor
the negation — it's `for; if sourceMatches; return true; end` — so
behavior matches: a negation-leading pattern will never match
anything and so never enables cleanUrls, just like in the reference.

`globset` precompiles the patterns once; per-request matching is
allocation-free batch matching.

## 4. `compute_clean_urls_redirect` (phase 4)

Reference: `serve-handler/src/index.js:121-143` (the cleanUrls
branch of `shouldRedirect`).

```rust
pub fn compute_clean_urls_redirect(
    decoded_path: &str,
    view: &CleanUrlsView,
) -> Option<String> {
    if !view.applicable(decoded_path) { return None; }
    let stripped = strip_html_or_index_suffix(decoded_path)?;
    let target = if stripped.contains("//") {
        collapse_consecutive_slashes(&stripped)
    } else {
        stripped.to_string()
    };
    Some(ensure_slash_start(&target))
}
```

`strip_html_or_index_suffix` mirrors the **single-pass** semantics
of the reference's `decodedPath.replace(/(\.html|\/index)$/g, '')`:
the `g` flag is a no-op against an end-anchored pattern, so the
function tries `.html` first and `/index` second, returning the
input minus the matched suffix on the first hit. This produces:

| Input             | After strip   | Final (after collapse + ensureSlashStart) |
|-------------------|---------------|--------------------------------------------|
| `/index.html`     | `/index`      | `/index` (single-pass; pinned by `_smoke`) |
| `/foo.html`       | `/foo`        | `/foo`                                     |
| `/dir/index.html` | `/dir/index`  | `/dir/index`                               |
| `/dir/index`      | `/dir`        | `/dir`                                     |
| `/index`          | (empty)       | `/` (ensureSlashStart re-prepends)         |
| `//foo.html`      | `//foo`       | `/foo` (collapse fires)                    |
| `/foo.txt`        | (no match)    | None                                       |
| `/foo.html/`      | (no match)    | None (trailing `/` blocks end-anchor)      |

`ensure_slash_start` mirrors `index.js:119` — if the strip empties
the path, the result still leads with `/`. The double-slash
collapse (`decodedPath.replace(/\/+/g, '/')` at `index.js:137`) is
implemented as a single linear walk in `collapse_consecutive_slashes`.

The `Location` header is run through `encode_uri_target` (already
present in `dispatch.rs::redirect_301` since 6b) so SPACEs become
`%20`, non-ASCII bytes become percent-escaped UTF-8, and reserved
chars pass through. No changes to that helper in 6c.

## 5. `try_clean_urls_resolve` (phase 8)

Reference: `serve-handler/src/index.js:276-307` (`getPossiblePaths`,
`findRelated`).

```rust
pub async fn try_clean_urls_resolve(
    url_path: &str,
    root: &Path,
    view: &CleanUrlsView,
) -> Option<ResolveOutcome> {
    if !view.applicable(url_path) { return None; }

    let trimmed = url_path.trim_start_matches('/').trim_end_matches('/');

    // Candidate 1: <P>/index.html (always tried).
    let p1 = if trimmed.is_empty() {
        root.join("index.html")
    } else {
        root.join(trimmed).join("index.html")
    };
    if let Some(canonical) = stat_under_root(&p1, root).await {
        return Some(ResolveOutcome::Index(canonical));
    }

    // Candidate 2: <P>.html (skipped for empty <P>, mirroring the
    // `path.basename(item) !== '.html'` filter at index.js:279).
    if trimmed.is_empty() { return None; }
    let p2 = root.join(format!("{trimmed}.html"));
    if let Some(canonical) = stat_under_root(&p2, root).await {
        return Some(ResolveOutcome::File(canonical));
    }

    None
}
```

`stat_under_root` does `metadata` → `is_file` check → `canonicalize`
→ `starts_with(root)` guard, mirroring the existing `resolve.rs`
escape-root logic. Any candidate outside `root` returns `None` (not
a leak).

The trailing-slash trim (`trim_end_matches('/')`) mirrors
`getPossiblePaths`'s handling of `/about/`: `path.join('/about/',
'index.html')` and `'/about/'.replace(/\/$/g, '.html')` both
produce the same candidate set as for `/about`. So `/about` and
`/about/` flow through phase 8 identically.

For `P = ""` (request to `/`), only `<P>/index.html` is tried —
matching `getPossiblePaths`'s filter on `path.basename(item) !==
'.html'` which drops the bare `/.html` second candidate.

## 6. Pre-stat gating in the dispatcher

Reference: `serve-handler/src/index.js:608-642`.

```rust
let url_has_extension = url_path_has_extension(&url_path);

let outcome = if !url_has_extension {
    // Extensionless: phase 8 first, then phase 9 fallback.
    match try_clean_urls_resolve(&url_path, root, clean_urls_view).await {
        Some(o) => o,
        None => resolve(&url_path, root).await,
    }
} else {
    // Has-ext: phase 9 first; if NotFound, fall to phase 8.
    match resolve(&url_path, root).await {
        ResolveOutcome::NotFound => try_clean_urls_resolve(&url_path, root, clean_urls_view)
            .await
            .unwrap_or(ResolveOutcome::NotFound),
        other => other,
    }
};
```

`url_path_has_extension` mirrors Node's `path.extname(p)` for our
URL-path use case: an extension is a non-empty `.xxx` substring
after the last `/` that is NOT the leading character of the
basename. Trailing-slash paths (`/foo.txt/`) and dotfiles
(`/.bashrc`) have no extension; nested dotfiles with extensions
(`/.bashrc.bak`) do. Implementation walks the basename's
`char_indices().skip(1)`.

The gating guarantees:

- `/foo.css` (has-ext, exists): served directly via `resolve()`. Phase
  8 not reached, so a stray `/foo.css.html` cannot shadow the file.
- `/foo.css` (has-ext, missing, `<P>.html` present): `resolve()` →
  NotFound → phase 8 fires → serves `<P>.html`. Matches `index.js:
  618-632`.
- `/about` (no-ext, dir+index.html): phase 8 first → `<P>/index.html`
  hit. Same outcome `resolve()` would have produced via dir-index.
- `/about` (no-ext, only `<P>.html`): phase 8 first → `<P>.html`
  hit.
- `/about` (no-ext, only flat `<P>` file, `<P>.html` also exists):
  phase 8 first → `<P>.html` wins. Matches the reference's "no
  pre-stat for extensionless".
- `/about` (no-ext, neither `<P>/index.html` nor `<P>.html`, flat
  `<P>` exists): phase 8 → None → phase 9 → File. Matches the
  reference's `index.js:634-642` fallback lstat.

## 7. `bodyMayDiffer` extension to strip `body.kind`

The compose probe `multislash-collapse#double_slash_segment` and
`#internal_double_slash` capture raw HTTP response bytes
(`mode: "raw"`). The reference's Node http for `response.end()`
with no args emits an HTTP/1.1 chunked-encoding terminator
(`0\r\n\r\n`, 5 bytes binary). axum/hyper uses
`Content-Length: 0` and emits no body bytes. Both responses are
zero application bytes; the divergence is purely transport-level.

The existing `bodyMayDiffer` mask strips `body.length`,
`body.sha256`, `body.preview` but kept `body.kind`. With the chunked
terminator captured as `binary` length 5 vs irserve's `empty` length
0, the kind comparison would trip even with `bodyMayDiffer` set.

The fix: extend `bodyMayDiffer` to also strip `body.kind`. The
semantic widening is "body content (kind, length, bytes, preview)
may differ; only headers and status are contractual at L0". The
only pre-existing user (`notfound-shape#missing_html`) already had
matching `text` kind on both sides, so stripping `kind` is a no-op
there.

## 8. Probe-flip ledger

| Probe | Anchor | Before | After | Why |
|---|---|---|---|---|
| `_smoke` | `index_html_redirect` | divergent | clean + contentLengthMayDiffer | phase 4 wired |
| `mime-defaults` | `html` | divergent | clean + contentLengthMayDiffer | phase 4 wired |
| `prec-cleanurls-default` | `about_html` | (no l0 block) | clean + contentLengthMayDiffer | phase 4 wired |
| `prec-cleanurls-default` | `about_no_slash` | (no l0 block) | clean | phase 8 wired (existing resolve dir-index actually handles this; phase 8 confirms the index-first contract via `<P>/index.html` candidate) |
| `prec-cleanurls-default` | `about_with_slash` | (no l0 block) | clean | phase 8 wired |
| `cleanurls-array` | `in_scope_redirect` | (no l0 block) | clean + contentLengthMayDiffer | phase 4 wired (array-form scope) |
| `cleanurls-array` | `in_scope_extensionless` | (no l0 block) | clean | phase 8 wired (array-form scope) |
| `cleanurls-array` | `out_of_scope_html_direct` | (no l0 block) | clean | phase 4 + phase 9 — out-of-scope path served direct as `.html` |
| `cleanurls-array` | `out_of_scope_extensionless_miss` | (no l0 block) | clean + bodyMayDiffer | phase 8 out-of-scope guard (404 from existing 9–13 path; default-error-page bytes differ per D-003) |
| `prec-cleanurls-trailing` | all 3 | reference-only (D-010) | clean + contentLengthMayDiffer (redirects only) | compose: phase 4 + phase 5 |
| `prec-cleanurls-trailing-false` | all 3 | reference-only (D-010) | clean + contentLengthMayDiffer (redirects only) | compose: phase 4 + phase 5 strip |
| `multislash-collapse` | `double_slash_segment` | divergent (D-010) | clean + bodyMayDiffer | phase 4 fires after URL decode; chunked-terminator transport mask |
| `multislash-collapse` | `internal_double_slash` | divergent (D-010) | clean + bodyMayDiffer | same |

Total: 13 anchors un-deferred. No new probe cases or snapshots
recorded — every flip uses existing reference snapshots from earlier
stages.

## 9. Verification

`cargo test -p irserve-core` covers 32 in-module unit tests in the
new `clean_urls.rs` plus all pre-existing tests (88 total green).

`cargo test --test oracle` runs the oracle harness against
`target=irserve` with the flipped probes. After 6c: 18 of 40 probes
passing, 22 skipped (probes lacking a `runner.l0` block — all
deferred to 6d–6h or Stage 7+), 0 failed. Reference:
40/40 still green via `node tools/probe/run.mjs --all
--target=reference --snapshot=verify`.

Manual smoke (mirrors README post-6b examples):

- `cargo run -- --listen 3010 _tmp` (fixture: `_tmp/index.html`,
  `_tmp/about.html`, `_tmp/blog/post.html`).
- `curl -i http://127.0.0.1:3010/index.html` → 301 `Location: /index`.
- `curl -i http://127.0.0.1:3010/about.html` → 301 `Location: /about`.
- `curl -i http://127.0.0.1:3010/about` → 200 (body of `/about.html`).
- With `_tmp/serve.json` `{"cleanUrls": ["/blog/**"]}`:
  `curl -i …/about.html` → 200 (out-of-scope);
  `curl -i …/blog/post.html` → 301 `/blog/post`.
