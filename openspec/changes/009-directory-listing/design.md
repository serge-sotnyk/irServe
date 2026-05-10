# Design: Directory listing (HTML + JSON), `unlisted`, `renderSingle`

This document records the architecture for Stage 6g. Unlike 6f's
cross-cutting tightening of phases 1, 10, and 13, 6g wires a single
new dispatcher *phase* (phase 11) plus a self-contained
`crates/irserve-core/src/listing.rs` module. The architectural
foundations (crate layout, HTTP-stack pins, 13-phase request
lifecycle, oracle harness layer) live in
`openspec/changes/001-port-minimal-static-server/design.md` (§§1, 4,
6). The contract for the capability lives in
`openspec/specs/directory-listing/spec.md` (SRV-DLST-001 / 002 / 003).

## 1. Pipeline placement

Phase numbering from `openspec/changes/001-port-minimal-static-server/design.md`
§4. 6g touches one phase plus one resolution-outcome variant:

| Phase | Stage 5b–6f behavior | Stage 6g change |
|-------|----------------------|-----------------|
| 9 (resolve) | `resolve()` collapsed `is_dir() && !index.exists()` into `ResolveOutcome::NotFound` | New `ResolveOutcome::Directory(PathBuf)` carries the canonicalized directory path forward |
| 11 (listing) | unimplemented — comment-stub | `Directory` arm in `dispatch.rs` consults `directoryListing` scope, `renderSingle`, and `unlisted` to choose HTML listing / JSON listing / file short-circuit / 404 |

The compiled scope structures live in `server.rs` alongside
`CleanUrlsView`, `RedirectRuleCompiled`, `RewriteRuleCompiled`, and
`HeaderRuleCompiled`, and are threaded into `dispatch` the same way
(added two parameters: `&DirectoryListingView`, `&UnlistedFilter`).

## 2. `ResolveOutcome::Directory` plumbing

`crates/irserve-core/src/resolve.rs` previously emitted `NotFound`
when `meta.is_dir()` AND `index.html` did not exist as a file. Slice 1
adds a third `Kind::Directory` arm:

```rust
let (resolved_path, kind) = if meta.is_file() {
    (candidate, Kind::File)
} else if meta.is_dir() {
    let index = candidate.join("index.html");
    match tokio::fs::metadata(&index).await {
        Ok(m) if m.is_file() => (index, Kind::Index),
        _ => (candidate, Kind::Directory),
    }
} else {
    return ResolveOutcome::NotFound;
};
```

The canonicalize + containment check at the bottom of `resolve()` runs
on the directory candidate the same way it runs on file candidates,
so `Directory(PathBuf)` is always a containment-checked absolute
filesystem path (`crates/irserve-core/src/resolve.rs:49-61`).

In `dispatch.rs`, every site that previously matched `NotFound` plus
the `Index/File` site that consumes the resolved path now also
considers `Directory`. The rewrite-fallback chain (lines 257-263) and
the has-extension cleanUrls fallback chain (lines 292-303) treat
`Directory` as "not a file match" and continue probing — mirroring
the reference's `findRelated`-then-fallback ladder where a directory
without an index.html does not satisfy the file-extension probe.

## 3. `DirectoryListingView` (scope check)

`listing.rs` mirrors `CleanUrlsView` from `clean_urls.rs:23-101`. The
shape is identical because `directoryListing` and `cleanUrls` share
the `BoolOrGlobs` config type:

```rust
pub struct DirectoryListingView { inner: Mode }

enum Mode {
    Off,                                // directoryListing: false
    On,                                 // true / absent (reference default)
    Scoped(Vec<ScopedPattern>),         // ["/docs/**", "!/secret/**"]
}

struct ScopedPattern {
    matcher: globset::GlobMatcher,      // literal_separator(true)
    negate: bool,                       // `!`-prefix XOR'd with match
}
```

`from_config` returns `(Self, Vec<InvalidGlob>)` — invalid globs are
non-fatal warnings, mirroring the reference's silent-skip behavior at
`serve-handler/src/index.js:38-67`.

`applicable(decoded_path)` mirrors `applicable(decodedPath,
directoryListing)` at `serve-handler/src/index.js:256-274`. The default
(when `directoryListing` is absent) is `On` per reference's truthy
return for non-bool / non-array `configEntry` (`index.js:273`).

The `compile_scoped_pattern` helper is a private duplicate of the
function with the same name in `clean_urls.rs`. Kept duplicated to
avoid coupling: the two capabilities share the `BoolOrGlobs` shape
but their downstream evolutions are independent (a future negation-
default change in one capability should not silently bleed into the
other). If a third capability ends up duplicating the same pattern
in Stage 7+, factoring out into `path_pattern.rs` becomes worthwhile.

## 4. `UnlistedFilter` (defaults + user globs)

```rust
pub struct UnlistedFilter { matchers: Vec<GlobMatcher> }

impl UnlistedFilter {
    pub fn from_config(user_patterns: &[String])
        -> (Self, Vec<InvalidGlob>) { ... }
    pub fn is_excluded(&self, name: &str) -> bool { ... }
}
```

The hardcoded defaults `[".DS_Store", ".git"]` are prepended
unconditionally in `from_config`, mirroring `excluded = ['.DS_Store',
'.git', ...unlisted]` at `serve-handler/src/index.js:330-334`. These
cannot be opted out of in the reference, and IrServe mirrors that
contract.

Match semantics: each pattern is `slasher`-normalized (leading `/`
ensured) and matched against a similarly-normalized filename. A name
is excluded iff at least one matcher matches. `is_excluded` returns
`true` to mean "drop from listing" — inverted relative to the
reference's `canBeListed` boolean (`index.js:309-323`) for ergonomics
on the call site.

Negation patterns (`!`-prefix) in `unlisted` are NOT honored. The
reference's minimatch-with-negation semantics in this position are
unintuitive (a sole `!keep` would exclude every non-`keep` file
because the slot's contract is "names to exclude"). Real `unlisted`
configurations are positive include-this-name patterns. If a probe
surfaces a `!`-prefixed entry in a future stage, document as a `D-NNN`
divergence rather than a fix.

## 5. `render` orchestration

The top-level entry point in `listing.rs`:

```rust
pub async fn render(
    dir: &Path,
    decoded_url: &str,
    root: &Path,
    request_headers: &HeaderMap,
    unlisted_filter: &UnlistedFilter,
    render_single: bool,
) -> Result<RenderResult, std::io::Error>

pub enum RenderResult {
    Direct(Response<Body>),
    Single { path: PathBuf, bytes: Vec<u8> },
}
```

The two-variant return shape is what lets the dispatcher distinguish
"listing emitted; bypass `apply_custom_headers`" from "single-file
short-circuit; rejoin the file-serving headers pass":

- `Direct` carries a complete `Response<Body>` (HTML or JSON listing).
  The dispatcher returns it as-is and emits `None` for `headers_path`,
  matching reference's listing branch which bypasses `getHeaders`
  per `serve-handler/src/index.js:644-672`.
- `Single` carries the lone file's path + bytes. The dispatcher
  builds a file response via the existing `file_response` helper and
  emits `Some(url)` for `headers_path`, matching reference's
  redirection through the success-site `getHeaders` call at
  `index.js:746`.

The orchestration order inside `render` is critical:

1. `read_sorted_entries(dir)` — read raw entries, sorted dirs-first
   then alphabetic (mirrors reference's sort at
   `serve-handler/src/index.js:402-413`).
2. `if render_single && raw_entries.len() == 1 && !raw_entries[0].
   is_dir` — fire the `Single` short-circuit. Count is **pre-filter**.
3. `apply_unlisted_filter(raw_entries, filter)` — drop excluded names
   from the rendered list.
4. `if accepts_json(headers) { json_response(...) } else
   { html_response(...) }`.

Why steps 2 and 3 are not swapped: reference at `index.js:342` checks
`canRenderSingle = renderSingle && (files.length === 1)` BEFORE the
`canBeListed` filter at `index.js:387-391`. A directory containing
`.DS_Store` plus one real file therefore has count=2 at step 2 and
falls through to the listing branch — even though the rendered
listing only shows the one real file. Mirroring this literally is a
kickoff interview decision (#3) and is documented as a Note on
SRV-DLST-003 in the spec delta. The pinned unit test
`render_single_count_is_pre_filter` in `listing.rs::tests` guards
against future refactors that might "fix" this counter-intuitive
order.

## 6. JSON `directory` / `paths` shape (D-007)

The chosen shape (kickoff interview decision #2) is:

| Position    | Shape                                      |
|-------------|--------------------------------------------|
| root        | `directory: "."`, `paths: []`              |
| `/sub/`     | `directory: "sub"`, `paths: [{name:"sub", url:"sub"}]` |
| `/sub/deep/`| `directory: "sub/deep"`, `paths: [{name:"sub", url:"sub"}, {name:"deep", url:"sub/deep"}]` |

POSIX separators only (Windows `\` is folded to `/` via
`replace('\\', '/')` on the `strip_prefix` result). No leading `/`,
no trailing `/`. The `paths` segments accumulate URL fragments
without leading `/` either, matching the chosen `directory` shape.

Per-entry `relative` is the URL form with a leading `/` and (for
folders) a trailing `/`:

| Position in tree | `relative` field |
|------------------|------------------|
| root entry `a.txt` | `"/a.txt"` |
| root folder `sub` | `"/sub/"` |
| nested entry `docs/api/index.json` | `"/docs/api/index.json"` |

The leading `/` is what lets the field double as an `href` for JSON
consumers. Reference's per-entry `relative` field uses the same URL-
shape leading `/`; the divergence is only in the top-level `directory`
and `paths` fields (where the reference leaks
`path.basename(current)` plus the absolute `dir` field).

Implementation: `sanitized_relative_dir(dir, root)` calls
`dir.strip_prefix(root)` and renders POSIX. `breadcrumb_segments`
walks the directory string by `/` and accumulates segment URLs.
`entry_relative_url(rel_dir, name, is_dir)` formats the per-entry
href.

The containment check upstream (`resolve()` returns `EscapedRoot` for
paths outside `root`) means `strip_prefix` never fails in practice.
The `Err(_)` arm of `sanitized_relative_dir` defensively returns
`"."` so a buggy refactor cannot leak a host-absolute path on the
JSON wire.

## 7. Reference-bypass of `getHeaders` for listings

Empirical finding documented in slice 2: reference's
`renderDirectory` at `serve-handler/src/index.js:644-672` ends with

```js
response.statusCode = 200;
response.setHeader('Content-Type', contentType);
response.end(directory);
return;
```

— and `return`s **before** the success-site `getHeaders` call at
`index.js:746`. Custom response headers therefore do NOT layer onto
listing 200 responses. This mirrors how 3xx redirects also bypass
`getHeaders` (reference at `index.js:586-588`).

IrServe's wiring:

- `dispatch_inner` returns `(response, None)` for the
  `RenderResult::Direct` arm. The `None` tells the top-level
  `dispatch` wrapper not to call `apply_custom_headers`.
- For `RenderResult::Single`, `dispatch_inner` returns
  `(file_response(&path, bytes), Some(url))`. The `url` is the URL
  form of the served file (`<request>/<filename>`). Reference reroutes
  the renderSingle branch through the file-serving site at
  `index.js:649-665` — overriding `absolutePath` and `stats` and
  letting the request flow back into `getHeaders` at `index.js:746`.
  IrServe's `Some(url)` triggers the `apply_custom_headers` pass,
  matching against the resolved file URL.

A consequence: a `**` headers rule paired with a directory-listed
URL emits `x-custom: yes` only when the directory matched
`renderSingle`, not when it produced an HTML or JSON listing. This is
contractual (mirrors reference) but easy to misread; the design's
"out of scope" section in the proposal calls it out.

## 8. `directoryListing: false` + `renderSingle: true` interaction

The kickoff plan flags this as a corner that needed a deliberate
choice. Reference's `applicable + renderSingle` at
`serve-handler/src/index.js:336-374`:

```js
if (!applicable(relativePath, configuredListing) && !renderSingle) {
    return {};
}
// renderSingle still gets a chance even when listing is off:
const files = await fs.readdir(absolutePath);
const canRenderSingle = renderSingle && (files.length === 1);
if (canRenderSingle) { ... return; }
// fall through to listing OR (if listing not applicable) 404
```

The dispatcher mirrors this:

```rust
let listing_applicable = listing_view.applicable(&decoded_path);
if !listing_applicable && !render_single {
    return error_response(404, ...);
}
match render_listing(...) {
    Ok(RenderResult::Single { path, bytes }) => /* serve file */,
    Ok(RenderResult::Direct(resp)) => {
        if listing_applicable {
            /* emit listing (HTML or JSON) */
        } else {
            /* listing off + renderSingle didn't fire → 404 */
        }
    }
    Err(_) => /* read failure → 404 */,
}
```

Two layered short-circuits keep the contract explicit:

1. **Pre-flight**: when both `listing_applicable` and `render_single`
   are false, return 404 without reading the directory. Common case
   (listing off, no renderSingle).
2. **Post-render**: when `render_listing` returns `Direct` AND
   `listing_applicable` is false, fall through to 404. This is the
   case where `renderSingle` was enabled but did not fire (count !=
   1, or the lone entry is a directory). Without this check, the
   dispatcher would emit a listing response despite
   `directoryListing: false`.

Both short-circuits are implemented in
`crates/irserve-core/src/dispatch.rs`'s `Directory` arm.

## 9. Methodological signals

This stage authored **no new D-NNN entries**. Every divergence flows
from existing decisions:

- HTML markup non-byte-match → `D-003` (no exact HTML/CSS markup of
  the listing or error pages). The `bodyMayDiffer` overlay on
  `listing-unlisted#listing_html` continues to mask body bytes; only
  status + content-type are contractual.
- JSON `directory` / `paths` sanitization → `D-007` (sanitized JSON
  directory listing). The chosen `"."` / `"sub"` / `"sub/deep"`
  shape is one specific point inside D-007's "rendered relative to
  the served root" envelope; it is documented as a Note on
  SRV-DLST-001 in the spec delta rather than a new decision.
- File `size` field as raw bytes (no `bytes`-package strings) →
  `D-003`. The JSON shape pins fields, not formatting — the shape is
  per-listing-implementation under the broad markup-non-contractual
  umbrella.

The absence of new D-NNN entries is itself a signal: 6g is the first
post-bootstrap stage that lands cleanly on top of existing
decisions. Future maintenance touching listing should consult D-003
and D-007 before authoring a new decision.

## 10. Out-of-scope reaffirmation

Listed in `proposal.md` §"Out of scope". Two items deserve a
design-level note:

- **`details.ext.split('.')[1] || 'txt'` quirk.** Reference's
  per-entry `ext` field defaults to `'txt'` for files without an
  extension (a JSON-serializer quirk in `serve-handler`). IrServe's
  `Path::extension().and_then(|s| s.to_str())` returns `None` for
  no-ext files, and `serde(skip_serializing_if = "Option::is_none")`
  elides the field. Pinned by the unit test
  `render_json_dotfile_has_no_ext`. If a probe surfaces this in
  Stage 7+, escalate to a `D-NNN` (status: `adapted`) rather than
  emit a placeholder.

- **Negation patterns in `unlisted`.** `UnlistedFilter::from_config`
  feeds raw `slasher`-normalized strings into `GlobBuilder`; a
  `!`-prefixed entry would compile as a literal `!`-prefixed glob
  and match nothing. Reference's minimatch with `nonegate: false`
  would (oddly) treat `!keep` as "exclude every file except `keep`",
  inverting the slot's contract. The pre-stage out-of-scope list
  declared this divergence; if a future probe surfaces it, document
  as `D-NNN`.

## 11. Verification

`cargo test -p irserve-core` covers the in-module unit tests added
in `listing.rs` (32 tests after slice 5):

- `applicable_*` — scope semantics for `Off` / `On` / `Scoped` /
  invalid-glob.
- `render_html_*` / `render_json_*` — content-type, dirs-first
  sort, parent `..` link, prefix when URL lacks trailing slash,
  D-007 chosen shape for nested dirs, dotfile no-ext.
- `unlisted_*` — defaults, user literal name, user glob.
- `render_single_*` — fires for count=1 file, skips for count=2,
  pre-filter count (the `.DS_Store` + 1 file case), skips for
  single subdirectory, off-flag renders listing.
- `accepts_json_*` — `Accept: application/json` substring + case-
  insensitive.
- `breadcrumb_segments_*`, `entry_relative_url_*` — D-007 shape
  helpers in isolation.

`cargo test --test oracle` runs the full harness against
`target=irserve` with the three listing probes flipped:
70 probes total / 59 passed / 11 skipped / 0 failed.

Reference still 70 of 70 green via `node tools/probe/run.mjs --all
--target=reference --snapshot=verify`.

Manual smoke (per slice plan):

```powershell
mkdir _tmp; "hi" | Set-Content _tmp\a.txt; "ho" | Set-Content _tmp\b.txt
mkdir _tmp\sub
cargo run -- --listen 3010 _tmp
curl -i http://127.0.0.1:3010/                                       # HTML listing
curl -i -H "Accept: application/json" http://127.0.0.1:3010/         # JSON listing, sanitized
curl -i http://127.0.0.1:3010/sub/                                   # nested listing — directory: "sub"
```
