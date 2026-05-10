# Proposal: Directory listing (HTML + JSON), `unlisted`, `renderSingle`

## Why

Stage 6f shipped custom error pages, the full L2 security surface, and
custom response headers (`008-error-pages-and-security`): phases 1, 10,
and 13 of the 13-phase dispatcher are tightened or generalized, plus a
post-dispatch header pass. The next un-defer slice in the L1/L2 roadmap
(`docs/stage6_l1_l2_capabilities.md:55`) is **phase 11** — directory
listing rendering — the last subjective-surface sub-stage of Stage 6
before 6h's CLI fill-in.

This change closes:

- **SRV-DLST-001** — HTML and JSON directory listings with `Accept`
  content negotiation, gated by `directoryListing: bool | string[]`.
  `verified` against the reference but deferred from the strict-L0
  cutoff per `D-008`.
- **SRV-DLST-002** — the hardcoded `[".DS_Store", ".git"]` default
  exclusion set plus user-supplied `unlisted` glob patterns. Listing-
  only filter; the same files remain directly fetchable via their
  explicit URL (existing `resolve.rs` semantics, no change there).
- **SRV-DLST-003** — `renderSingle: true` short-circuits a directory
  request to serve a lone non-HTML file directly when (and only when)
  the unfiltered entry count is exactly one and that entry is a file.

The `serve.json` fields (`directoryListing`, `unlisted`,
`renderSingle`) already exist in `crates/irserve-core/src/config.rs`
and deserialize cleanly; phase 11 was a comment-stub before this
stage. `ResolveOutcome::NotFound` was the only signal a directory-
without-index request could surface to the dispatcher; resolution
silently swallowed the `is_dir() && !index.exists()` case.

## What

- **Phase 11 of the dispatcher.** A new `ResolveOutcome::Directory(
  PathBuf)` variant carries the canonicalized absolute path of a
  resolved directory that has no usable `index.html`. The `Directory`
  arm in `dispatch.rs` consults `directoryListing` scope, the
  `renderSingle` flag, and the `unlisted` filter to choose between an
  HTML listing, a JSON listing, a `renderSingle` file short-circuit,
  or a 404 fall-through. Mirrors `serve-handler/src/index.js:644-680`
  (the directory branch of the request handler) plus
  `index.js:325-465` (`renderDirectory`).

- **`crates/irserve-core/src/listing.rs` (NEW).** Houses the
  `DirectoryListingView` precompiled scope check (`Off` / `On` /
  `Scoped(Vec<ScopedPattern>)` — same shape as `CleanUrlsView`), the
  `UnlistedFilter` (hardcoded defaults + user globs), the HTML and
  JSON renderers, and the `renderSingle` short-circuit. Reuses the
  `BoolOrGlobs` enum from `clean_urls.rs` and the `globset` crate
  pattern. `apply_custom_headers` is NOT layered onto listing
  responses — mirrors the reference's bypass at
  `index.js:644-672`, where the listing branch ends with
  `response.end(directory)` before reaching the success-site
  `getHeaders` call at `index.js:746`.

- **`renderSingle` short-circuit ordering.** The count-of-1 check
  fires BEFORE the `unlisted` filter. A directory containing
  `.DS_Store` plus one real file therefore has count=2 and renders
  as a listing, not as the lone real file. Mirrors reference's
  `canRenderSingle = renderSingle && (files.length === 1)` at
  `index.js:342`, which runs BEFORE the `canBeListed` filter at
  `index.js:387-391`. Documented as a Note on SRV-DLST-003 (kickoff
  interview decision #3); no new D-NNN.

- **JSON `directory` / `paths` shape under D-007.** The chosen
  relative-path form for the JSON listing's `directory` and `paths`
  fields is:
  - root → `"."`
  - nested → `"sub"`, `"sub/deep"` (POSIX separators, no leading `/`,
    no trailing `/`).

  `paths` is empty for the root and otherwise an array of
  `{name, url}` objects whose `url` accumulates segments in the same
  shape (`[{"name":"sub","url":"sub"}, {"name":"deep","url":
  "sub/deep"}]`). Per-entry `relative` keeps the URL form with a
  leading `/` (and trailing `/` for folders) so the field doubles as
  an `href` for JSON consumers. Closes Q-008 (already closed by
  D-007; this implementation makes the contract observable).

- **`renderSingle` interaction with `directoryListing: false`.** When
  `directoryListing` is `false` (or out-of-scope) BUT `renderSingle`
  is `true`, the directory branch still reads entries and applies the
  count-of-1 short-circuit. If renderSingle fires, the file is
  served. If it doesn't (count != 1, or the lone entry is a
  directory), the 404 fall-through fires. This mirrors reference's
  `applicable + renderSingle` interaction at `index.js:336-374`.
  Listing-off + renderSingle-off → 404 (existing behavior).

- **Probe coverage.** The following anchors flip into `runner.l0`
  partitions (reference snapshots already exist; recorded in
  Stage 1):
  - `listing-disabled#root_no_listing` (ORC-008) — already had
    `clean` + `bodyMayDiffer`; no change.
  - `listing-unlisted#{listing_html, listing_json}` (ORC-009 /
    ORC-010) — already had `clean` + `bodyMayDiffer`; the
    `bodyMayDiffer` overlay still covers HTML markup divergence
    (D-003) and the sanitized JSON `directory`/`paths` shape
    (D-007).
  - `rendersingle#single_file` (ORC-011) — `clean` only;
    body-bytes are byte-exact (the file's raw PNG bytes; not a
    listing body).

  Final oracle after this stage: 70 total / 59 passed / 11 skipped /
  0 failed. No new probes authored — Stage 1's three listing probes
  cover the full surface.

- **No new D-NNN entries.** Every divergence in this stage flows
  from existing decisions:
  - HTML markup non-byte-match → `D-003`.
  - JSON `directory`/`paths` sanitization → `D-007`.
  - File `size` field as raw bytes (no `bytes` npm-package
    formatting) → `D-003` (markup is non-contractual; the sanitized
    JSON shape pins fields, not formatting).
  Recorded in design's "Methodological signals" section so future
  maintenance does not introduce a redundant D-NNN.

## Out of scope

- **Exact byte-level HTML markup.** D-003 already disclaims listing
  markup parity. IrServe emits a minimal `<!doctype html><html>
  <head>...<title>Index of <path></title>...<h1>Index of <path></h1>
  <ul>...<li><a href=...>...</a></li>...</ul></body></html>` shape —
  no CSS, no SVG icons, no `<table>` layout. The reference's
  `serve-handler` ships a styled template; matching it byte-for-byte
  is non-contractual.

- **`bytes`-package formatted size strings.** The reference formats
  file sizes via the `bytes` npm package (`unitSeparator: ' '`,
  `decimalPlaces: 0`) producing strings like `"1 kB"`. IrServe emits
  the JSON `size` field as a raw integer (`u64`). The HTML body does
  not include sizes at all in this stage. Tracked under D-003.

- **`path.basename(current)` prefixing of the JSON `directory`
  field.** The reference's `directory: \`${path.basename(current)}/
  ${slashed(relativePath)}\`` shape leaks the served-root folder
  name into every JSON listing response. The chosen `"."` /
  `"sub"` / `"sub/deep"` shape sanitizes that away under D-007.
  The peer `paths` array is sanitized identically.

- **`details.ext.split('.')[1] || 'txt'` quirk.** The reference's
  per-entry shape sets `ext: 'txt'` for files with no extension
  (an upstream quirk where the no-ext branch of the JSON serializer
  defaults to `'txt'`). IrServe emits no `ext` field for files
  without an extension (Rust's `Path::extension` returns `None`,
  `serde(skip_serializing_if = "Option::is_none")` elides it). The
  per-entry shape is non-contractual under D-003 / D-007 (which pin
  status + content-type + the absence of host-absolute paths, not
  the precise key set).

- **Negation patterns in `unlisted`.** A `!`-prefixed entry like
  `"!keep"` would, under reference's minimatch semantics in this
  position, exclude every non-`keep` file (because `unlisted`'s
  semantic is "include this in the excluded set"). That is rarely
  what users want, and the reference's behavior in this corner is
  unintuitive. IrServe treats `unlisted` patterns as positive
  include-this-name patterns only; a `!`-prefixed entry would
  compile as a literal `globset` pattern starting with `!` and
  match nothing. If a probe surfaces this corner in a future
  stage, it lands as a `D-NNN` (status: `adapted`), not a fix.

- **In-place sparse-array delete pattern.** The reference's
  `renderDirectory` mutates `files[]` via `delete files[index]`
  during the `canBeListed` walk (`index.js:387-391`), producing a
  sparse array that subsequent loops handle defensively. IrServe
  filters eagerly into a dense `Vec`. This is an internal
  implementation detail; the wire shape is identical.

- **Custom headers on listing responses.** Mirrors the reference's
  bypass: `index.js:644-672` returns BEFORE the success-site
  `getHeaders` call at `index.js:746`. A `**` headers rule does
  NOT layer `x-custom: yes` onto an HTML or JSON listing response.
  The `renderSingle` branch DOES go through custom headers — the
  reference reroutes through the file-serving path, and IrServe
  mirrors by emitting `Some(url)` from `dispatch_inner` for the
  `RenderResult::Single` case (so the wrapper's
  `apply_custom_headers` pass fires).
