# Tasks: Directory listing (HTML + JSON), `unlisted`, `renderSingle`

Six iterative slices, one commit per green slice. Slice 6 is the meta
slice and lands last so spec deltas reflect what was actually shipped.
The 1d4448d kickoff-plan commit predates the package; it is not listed
as a slice.

## Slice 1 — Directory resolve outcome plumbing

- [x] Add `ResolveOutcome::Directory(PathBuf)` variant in
  `crates/irserve-core/src/resolve.rs`. Carries the canonicalized
  absolute filesystem path of the resolved directory, already
  containment-checked against `root`.
- [x] Update `resolve()` to emit `Directory` when `meta.is_dir()` AND
  no `index.html` is found, instead of collapsing to `NotFound`. The
  canonicalize + `starts_with(root)` guard at the bottom of `resolve()`
  applies uniformly across all three `Kind` arms.
- [x] Add `#[derive(Debug)]` on `ResolveOutcome` (already present from
  Stage 6c; no-op on this slice).
- [x] In `crates/irserve-core/src/dispatch.rs`, route the new variant
  to `error_response(StatusCode::NOT_FOUND, ..)` for now (preserves
  existing oracle behavior). Update the rewrite-fallback chain (lines
  257-263) and the has-extension cleanUrls fallback (lines 292-303) to
  treat `Directory` the same as `NotFound`.
- [x] Verify: all 67 existing probe cases stay green under
  `target=irserve`.
- Commit: `feat(stage-6g): slice 1 — directory resolve outcome plumbing`
  (b653646).

## Slice 2 — HTML directory listing + scope view

- [x] Create `crates/irserve-core/src/listing.rs` module with:
  - [x] `DirectoryListingView` enum (`Off` / `On` / `Scoped`) and
    `from_config(&Option<BoolOrGlobs>)` mirroring `CleanUrlsView`'s
    pattern from `clean_urls.rs:23-101`.
  - [x] `applicable(decoded_path)` mirroring `applicable(decodedPath,
    directoryListing)` at `serve-handler/src/index.js:256-274`.
  - [x] `render(...)` orchestration entry point with `RenderResult::
    Direct(Response<Body>)` variant only (Single lands in slice 5).
  - [x] HTML body builder: minimal `<!doctype html>...<h1>Index of
    PATH</h1><ul>...</ul>` shape; dirs-first sort then alphabetic;
    parent `..` link when `dir != root`; per-entry hrefs prefixed
    with the request URL (with trailing `/` ensured).
  - [x] `html_escape_text` and `encode_href_segment` helpers for
    spaces, `?`, `#`, and HTML metacharacters.
- [x] Add `mod listing;` to `lib.rs`.
- [x] In `server.rs`, build a `DirectoryListingView` once in `serve()`
  from `config.serve_config.directory_listing`; thread into
  `dispatch` as a 5th parameter.
- [x] In `dispatch.rs`, the `Directory` arm now consults
  `listing_view.applicable(&decoded_path)`. If applicable, call
  `render(...)` and return its `Response<Body>` directly (no
  `apply_custom_headers` post-pass — mirrors reference's bypass).
  Otherwise fall through to `error_response(404, ..)`.
- [x] Add unit tests: `applicable_default_on`, `applicable_off`,
  `applicable_scoped_in_glob`, `applicable_scoped_negation`,
  `from_config_invalid_glob_skipped`,
  `render_html_lists_entries_dirs_first`,
  `render_html_includes_parent_link_in_subdir`,
  `render_html_prefix_when_url_lacks_trailing_slash`.
- [x] Verify: ORC-008 (`listing-disabled#root_no_listing`) and the
  HTML half of ORC-009 (`listing-unlisted#listing_html`) green under
  `target=irserve` with `bodyMayDiffer` overlay (D-003).
- Commit: `feat(stage-6g): slice 2 — HTML directory listing + scope view`
  (0878044).

## Slice 3 — JSON directory listing + D-007 sanitization

- [x] Add JSON branch to `listing.rs::render`: detect
  `Accept: application/json` (case-insensitive substring match,
  mirroring `serve-handler/src/index.js:556-558`); when present,
  emit `application/json; charset=utf-8` Content-Type and serialize
  via `serde_json`.
- [x] JSON shape: `{files:[{type, name, base, ext?, relative,
  size?}], directory: <D-007>, paths: [{name, url}]}`.
- [x] D-007 shape implementation:
  - [x] `sanitized_relative_dir(dir, root)` — `dir.strip_prefix(root)`
    rendered with POSIX separators; `"."` for root.
  - [x] `breadcrumb_segments(directory)` — empty for root;
    `[{"sub","sub"}, {"deep","sub/deep"}]` for `"sub/deep"`.
  - [x] `entry_relative_url(rel_dir, name, is_dir)` — URL form with
    leading `/` and trailing `/` for folders.
- [x] `accepts_json` helper mirrors reference's substring check
  case-insensitively.
- [x] Add unit tests: `render_json_emits_application_json_envelope`,
  `render_json_subdir_directory_and_paths_use_chosen_shape`,
  `render_json_dotfile_has_no_ext`, `accepts_json_basic`,
  `accepts_json_substring_match_in_multivalue`,
  `accepts_json_false_when_html_only`, `accepts_json_missing_header_is_false`,
  `breadcrumb_segments_root_is_empty`, `breadcrumb_segments_nested`,
  `entry_relative_url_root_no_double_slash`, `entry_relative_url_nested`.
- [x] Verify: JSON half of ORC-010 (`listing-unlisted#listing_json`)
  green under `target=irserve` with `bodyMayDiffer` overlay (D-007).
- Commit: `feat(stage-6g): slice 3 — JSON directory listing + D-007 sanitization`
  (b2bd8fd).

## Slice 4 — `unlisted` defaults + user globs

- [x] Add `UnlistedFilter` struct in `listing.rs` with hardcoded
  defaults `[".DS_Store", ".git"]` prepended unconditionally — mirrors
  `excluded = ['.DS_Store', '.git', ...unlisted]` at
  `serve-handler/src/index.js:330-334`.
- [x] `UnlistedFilter::from_config(&[String])` returns `(Self,
  Vec<InvalidGlob>)`. Each pattern is `slasher`-normalized (leading
  `/` ensured) and compiled via `globset::GlobBuilder` with
  `literal_separator(true)`.
- [x] `is_excluded(name)` returns true iff at least one matcher
  matches (inverted `canBeListed` boolean from `index.js:309-323`).
- [x] In `listing.rs::render`, apply the filter AFTER reading raw
  entries and AFTER the `renderSingle` count check (slice 5 wires
  the count check; in this slice the filter runs before the
  HTML/JSON branch).
- [x] Thread `&UnlistedFilter` into `dispatch.rs` as a 6th parameter;
  build it once in `server.rs::serve()` from
  `config.serve_config.unlisted`.
- [x] Add unit tests: `unlisted_defaults_exclude_dotfiles`,
  `unlisted_user_pattern_literal_name`, `unlisted_user_pattern_glob`,
  `render_html_filters_unlisted_entries`,
  `render_json_filters_unlisted_entries`.
- [x] Verify: ORC-009 + ORC-010 (HTML + JSON halves of
  `listing-unlisted`) green end-to-end with the user `unlisted:
  ["secret.txt"]` from the fixture, plus the `.DS_Store` and `.git`
  defaults exercised.
- Commit: `feat(stage-6g): slice 4 — unlisted defaults + user globs`
  (6b81a67).

## Slice 5 — `renderSingle` short-circuit

- [x] Add `RenderResult::Single { path: PathBuf, bytes: Vec<u8> }`
  variant alongside the existing `Direct(Response<Body>)`.
- [x] In `listing.rs::render`, after reading raw entries and BEFORE
  applying the `unlisted` filter: when `render_single == true` AND
  `raw_entries.len() == 1` AND `!raw_entries[0].is_dir`, emit
  `RenderResult::Single { path, bytes }` with the file read directly
  via `tokio::fs::read`. Mirrors `canRenderSingle = renderSingle &&
  (files.length === 1)` at `serve-handler/src/index.js:342`, which
  runs BEFORE the `canBeListed` filter at `index.js:387-391`. Kickoff
  interview decision #3.
- [x] Pre-flight in `dispatch.rs`: when `!listing_applicable &&
  !render_single`, short-circuit to 404 BEFORE reading the directory
  (avoids the read in the common case). When either is true, run
  `render_listing(...)` and dispatch on the `RenderResult` variant:
  - `Direct(resp)` → return `(resp, None)` to bypass
    `apply_custom_headers`.
  - `Single { path, bytes }` → emit `(file_response(&path, bytes),
    Some(url))` so the wrapper's `apply_custom_headers` pass fires
    (mirrors reference's redirection through the file-serving
    `getHeaders` site at `index.js:746`). The headers-path is the
    URL form `<request>/<filename>`.
- [x] Thread `serve_config.render_single` into the dispatcher's
  `Directory` arm via `serve_config.render_single.unwrap_or(false)`.
- [x] Add unit tests: `render_single_fires_for_single_file_directory`,
  `render_single_skips_when_count_is_two`,
  `render_single_count_is_pre_filter` (the `.DS_Store` + 1 file
  edge case explicitly documented),
  `render_single_skips_for_single_subdirectory`,
  `render_single_off_renders_listing`.
- [x] Verify: ORC-011 (`rendersingle#single_file`) green under
  `target=irserve` with byte-exact body match (no `bodyMayDiffer`
  overlay needed — the file's raw PNG bytes are contractual).
- Commit: `feat(stage-6g): slice 5 — renderSingle short-circuit`
  (7f4d75f).

## Slice 6 — Spec deltas + meta (this slice)

- [x] Author `openspec/changes/archive/2026-05-15-009-directory-listing/`:
  - [x] `proposal.md` — closes SRV-DLST-001 / 002 / 003; out-of-scope
    list (markup, `bytes`-package size, `path.basename` prefix,
    `details.ext` quirk, `unlisted` negation, sparse-array delete,
    custom headers on listings).
  - [x] `design.md` — pipeline placement, `Directory` plumbing,
    `DirectoryListingView` / `UnlistedFilter` / `render`
    orchestration, D-007 chosen shape, reference-bypass of
    `getHeaders` for listings, `directoryListing: false` +
    `renderSingle: true` interaction, methodological signals
    (no new D-NNN), verification.
  - [x] `tasks.md` (this file).
  - [x] `specs/directory-listing/spec.md` — MODIFIED Requirements:
    SRV-DLST-001 (Note on JSON `"."`/`"sub"` shape), SRV-DLST-002
    (no contract change; evidence updated), SRV-DLST-003 (Note on
    `renderSingle`-before-`unlisted` ordering and the
    `directoryListing: false` interaction).
- [x] No new `D-NNN` entries authored — every divergence flows from
  existing D-003 (markup non-contractual) and D-007 (JSON path
  sanitization). Documented in design's "Methodological signals"
  section.
- [x] No `oracle-matrix.md` rows added — Stage 1's ORC-008..ORC-011
  cover the surface end-to-end. Probe cases stay unchanged.
- [x] `README.md`: flip Stage 6g row to `done`; update "Try IrServe"
  with directory-listing + JSON-listing examples.
- [x] `docs/stage6_l1_l2_capabilities.md`: flip 6g row to `done`.
- [x] Run `npx -y @fission-ai/openspec@latest validate --all
  --strict` and report any failures.
- Commit: `docs(stage-6g): spec deltas + meta`.
