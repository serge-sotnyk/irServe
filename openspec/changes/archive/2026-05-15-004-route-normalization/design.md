# Design: Routing normalization (multi-slash + trailingSlash)

This design is **code-level**. The architectural foundations (crate
layout, HTTP-stack pins, 13-phase request lifecycle, oracle harness
layer) live in `openspec/changes/archive/2026-05-15-001-port-minimal-static-server/
design.md` (§§1, 4, 6). The contract for the capability lives in
`openspec/specs/routing/spec.md` (SRV-ROUT-003, SRV-ROUT-004,
SRV-ROUT-005, SRV-ROUT-006).

## 1. Module map (deltas to the Stage-6a layout)

| Module | Delta | Wires |
|---|---|---|
| `crates/irserve-core/src/normalize.rs` | **NEW.** `pub fn collapse_slashes(path: &str) -> Cow<'_, str>` returning `Cow::Borrowed` on the no-`//` happy path. 10 in-module unit tests. | phase 3 |
| `crates/irserve-core/src/trailing_slash.rs` | **NEW.** `pub fn compute_trailing_slash_redirect(path: &str, cfg: Option<bool>) -> Option<String>`. 14 in-module unit tests. | phase 5 |
| `crates/irserve-core/src/dispatch.rs` | `dispatch()` signature gains `&ServeConfig`. Phases 3 and 5 hooked in. `redirect_301(target)` private helper. Comment-stubs mark phases 4, 6, 7, 8. | dispatcher |
| `crates/irserve-core/src/server.rs` | `AppState { root, serve_config }` carries the config; `handler` propagates both into `dispatch`. | startup |
| `crates/irserve-core/src/lib.rs` | `mod normalize;`, `mod trailing_slash;`. | n/a |
| `tools/probe/cases/multislash-collapse.json` | `runner.l0.{clean, divergent}` block added; the case file's `requests` are unchanged. | runner |

`resolve.rs`, `mime.rs`, `notfound.rs`, `config.rs` are untouched.

## 2. Pipeline order

The 13-phase pipeline (`001-port-minimal-static-server/design.md` §4)
is now realized as follows in `dispatch.rs`:

```
1-2. Method gate (existing)
0.   URL percent-decode at dispatcher entry (decoded_path)      [Stage 6b]
4.   /* Phase 4: cleanUrls 301 (Stage 6c) */
5.   Phase 5: trailing_slash::compute_trailing_slash_redirect    [Stage 6b]
        if Some(target) -> return redirect_301(&target)
        — operates on decoded_path (uncollapsed) so the multi-slash
          override sees the input's `//` content
3.   Phase 3: normalize::collapse_slashes(decoded_path)         [Stage 6b]
        — silent for resolve-and-onwards; redirect-on-collapse
          when trailingSlash is set is emitted by phase 5 above
6.   /* Phase 6: configured redirects (Stage 6d) */
7.   /* Phase 7: rewrites + --single (Stage 6e) */
8.   /* Phase 8: cleanUrls resolution (Stage 6c) */
9-13. Resolve -> MIME -> 404 (existing)
```

Phases 3 and 5 are coupled in the reference's `shouldRedirect`
slashing branch (`index.js:158-160`): when `trailingSlash` is set
AND the decoded path contains `//`, the redirect target is the
slash-collapsed form regardless of the add/strip branches. Splitting
"collapse silently" and "trailingSlash redirect" naively (collapse
first, then redirect on the collapsed path) loses this coupling and
silently routes `/test//` under `trailingSlash: true` instead of
emitting the reference's 301 → `/test/`. The implementation therefore
runs phase 5 on the **uncollapsed** decoded path; the silent collapse
applies only to the path that flows into phases 9–13.

Comment-stubs (not Rust code) mark the phases that are not yet
implemented. The 6c insertion is a single function call between the
URL-decode and phase 5.

## 3. `collapse_slashes` semantics

Reference: `serve-handler/src/index.js:158-160` (`decodedPath.replace(/\/+/g, '/')`).

- Input: the **decoded** URI path (after `percent_decode_str` at
  dispatcher entry). The function itself does not decode.
- If the input contains no `//`, return `Cow::Borrowed(path)` (zero
  allocations on the happy path — typical case).
- Otherwise, walk the chars and emit at most one `/` for each run of
  consecutive `/` characters; preserve all non-`/` characters.
- The function never emits a 301 on its own; it is a pure pre-routing
  transform. When `trailingSlash` is set, the redirect-on-`//` is
  emitted by `compute_trailing_slash_redirect` (which calls into this
  function for the target string) — see §4.

Silent-collapse behavior holds when `trailingSlash` is unset
(SRV-ROUT-005, Q-006 closure: ORC-025). When `trailingSlash` is set,
the same `//` instead becomes observable as a 301 (ORC-072, ORC-073,
ORC-074); the silent path through phase 3 still runs but its output
is shadowed by the 301 from phase 5.

## 4. `compute_trailing_slash_redirect` semantics

Reference: `serve-handler/src/index.js:121-185` (`shouldRedirect`,
slashing branch). The implementation includes the multi-slash
override at `index.js:158-160` — when `trailingSlash` is set AND the
decoded path contains `//`, the override unconditionally returns
`Some(collapsed)` regardless of the add/strip branches.

```rust
pub fn compute_trailing_slash_redirect(decoded_path: &str, cfg: Option<bool>) -> Option<String> {
    let cfg = cfg?;

    // Multi-slash override (`index.js:158-160`).
    if decoded_path.contains("//") {
        return Some(collapse_slashes(decoded_path).into_owned());
    }

    let is_trailed = decoded_path.ends_with('/');

    if !cfg && is_trailed {
        if decoded_path.len() <= 1 { return None; }   // root edge: '/' would yield ''
        return Some(decoded_path[..decoded_path.len() - 1].to_string());
    }

    if cfg && !is_trailed {
        let basename = decoded_path.rsplit('/').next().unwrap_or("");
        if basename.is_empty() || basename.starts_with('.') { return None; }
        let has_extension = basename
            .char_indices().skip(1).any(|(_, c)| c == '.');
        if has_extension { return None; }
        return Some(format!("{decoded_path}/"));
    }

    None
}
```

Add-branch exemptions (dotfile / extension) mirror Node's
`path.parse(p).{name, ext}` shape:

- `name.startsWith('.')` flags dotfiles like `.htaccess` or
  `.well-known`.
- A non-leading dot anywhere in the basename signals an extension.
  `/foo.tar.gz` has ext `.gz` (skipped); `/.bashrc.bak` has ext
  `.bak` despite leading-dot basename (skipped). `/.htaccess` has
  empty ext but starts-with-dot (skipped).

Strip branch has no dotfile/extension guard — the reference's strip
fires on `/.htaccess/` → `/.htaccess` and `/foo.txt/` → `/foo.txt`.

Status code is **301** verbatim from the reference (`defaultType` at
`index.js:123`).

## 5. Dispatcher signature change

Phase-5 needs the config; phase-3 does not. Rather than thread
`Option<bool>` all the way through, `dispatch()` takes `&ServeConfig`
and reads `serve_config.trailing_slash` at the phase-5 call site.
This positions 6c (which needs `serve_config.clean_urls`), 6d
(`redirects`), 6e (`rewrites`) for one-line additions without further
signature churn.

`server.rs::AppState` was widened to carry both `root: PathBuf` and
`serve_config: ServeConfig`. The `handler` extracts both via `&state.
{root, serve_config}` and passes them positionally to `dispatch`.

## 6. `redirect_301` helper and `encode_uri_target`

```rust
const ENCODE_URI_SET: &AsciiSet = &CONTROLS
    .add(b' ').add(b'"').add(b'%').add(b'<').add(b'>').add(b'\\')
    .add(b'^').add(b'`').add(b'{').add(b'|').add(b'}').add(b'[').add(b']');

pub(crate) fn encode_uri_target(target: &str) -> String {
    utf8_percent_encode(target, ENCODE_URI_SET).to_string()
}

fn redirect_301(target: &str) -> Response<Body> {
    let encoded = encode_uri_target(target);
    let location = HeaderValue::from_str(&encoded)
        .unwrap_or_else(|_| HeaderValue::from_static("/"));
    Response::builder()
        .status(StatusCode::MOVED_PERMANENTLY)
        .header(LOCATION, location)
        .body(Body::empty())
        .expect("301 response should always build")
}
```

`encode_uri_target` mirrors JavaScript's `encodeURI` (the function the
reference applies to redirect targets at
`serve-handler/src/index.js:586`). Without this re-encoding, a
request like `GET /foo%20bar` would decode to `/foo bar` at
dispatcher entry, the trailingSlash add branch would compute target
`/foo bar/`, and `Location` would carry the literal SPACE — diverging
from the reference's `/foo%20bar/`. Likewise non-ASCII bytes
(`/caf%C3%A9` → `/café` after decode) would emit raw UTF-8 in the
header instead of the reference's percent-escaped form.

The encode set is built additively from `percent_encoding::CONTROLS`:
SPACE, `"`, `%`, `<`, `>`, `\`, `^`, `` ` ``, `{`, `|`, `}`, `[`,
`]`. Reserved chars used in URLs (`?`, `=`, `&`, `:`, `@`, `+`, `$`,
`,`, `#`, `/`, `;`) are intentionally NOT in the set — `encodeURI`
preserves them. `utf8_percent_encode` always percent-encodes non-ASCII
bytes regardless of the set, which matches `encodeURI`'s UTF-8
behavior. 7 unit tests in `dispatch.rs` cover safe-set passthrough,
query/reserved chars, SPACE, multi-byte UTF-8, literal `%` →
`%25`, brackets/quotes, control chars.

The fallback to `"/"` on `HeaderValue::from_str` failure is
defensive: targets pass through `encode_uri_target` first, so the
result is guaranteed to be valid header bytes. The fallback exists
only to keep the function infallible.

axum/hyper auto-emits `content-length: 0` on the empty body; the
reference (Node http) omits the header. We do not strip it from the
response — see Findings.

## 7. Probe wiring

### `multislash-collapse.json`

The case file gains a `runner.l0` block:

```json
"runner": {
  "l0": {
    "clean": ["double_slash_root"],
    "divergent": ["double_slash_segment", "internal_double_slash"]
  }
}
```

`double_slash_root` (`GET //` → 200 root index) is the pure-collapse
anchor. The other two anchors (`//docs/guide.html` → 301
`/docs/guide`, `/docs//guide.html` → 301 `/docs/guide`) require
phase 4 (cleanUrls 301 stripping `.html`); they stay reference-only
under `runner.l0.divergent` until 6c lands phase 4 — at which point
they move from `divergent` to `clean`.

### New: `trailingslash-add.json`

```
serve.json: { "trailingSlash": true, "cleanUrls": false }
fixture:    index.html, data.txt
GET /about       -> 301 /about/                        (ORC-068)
GET /data.txt    -> 200 (extension exempt)             (ORC-069)
runner.l0.contentLengthMayDiffer: ["about_no_slash_redirects"]
```

`cleanUrls: false` is required: the reference's `applicable` helper
(`serve-handler/src/index.js:256-274`) defaults `cleanUrls` to on
when undefined, so without the explicit `false` the redirect target
would be cleanUrls-stripped (e.g. `/about` -> `/about` -> `/about/`
sequence, depending on fixture content). Setting `false` isolates
phase 5 from phase 4 cleanly.

The `data.txt` anchor is the anti-redirect case: a path with an
extension is exempt from the trailing-slash add, so the file is
served directly with `200`.

### New: `trailingslash-strip.json`

```
serve.json: { "trailingSlash": false, "cleanUrls": false }
fixture:    index.html, about.html
GET /about/      -> 301 /about                         (ORC-070)
GET /about.html  -> 200                                (ORC-071)
runner.l0.contentLengthMayDiffer: ["about_with_slash_redirects"]
```

The redirect target `/about` does not need to exist as a file — the
strip branch runs before file resolution; the probe records only the
301. `/about.html` (no trailing slash) is the anti-redirect anchor
verifying phase 5 does not fire when it should not.

The probes deliberately avoid directory anchors (e.g. `GET /` or
`GET /about/` resolving to a directory) because under `cleanUrls:
false` the reference returns a directory listing while irserve's L0
directory→`index.html` (SRV-FILE-002) serves `index.html` directly.
That divergence is SRV-DLST-* territory and lands in 6g.

## 8. Methodological signals — none required spec edits

Two micro-divergences surfaced; both were absorbed without a new
D-NNN entry:

1. **`content-length: 0` on the 301 response (axum/hyper) vs.
   omitted (Node http).** No spec text pins the header's presence;
   `runner.l0.contentLengthMayDiffer` masks it on both sides for the
   two redirect anchors. The body bytes (zero) still must-match.
2. **Directory listing vs. `index.html` under `cleanUrls: false`.**
   Out of 6b's scope (SRV-DLST-* lands in 6g). The new probes route
   around the divergence by not exercising directory anchors with
   `cleanUrls: false`.

D-010 records the un-defer (SRV-ROUT-003, SRV-ROUT-004, SRV-ROUT-005
removed from D-008's deferred set). SRV-ROUT-006 stays deferred
because phases 4, 6, 7, 8 are not yet implemented.
