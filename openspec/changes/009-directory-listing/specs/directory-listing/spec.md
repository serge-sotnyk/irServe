# Delta for directory-listing

## MODIFIED Requirements

### Requirement: Directory listing on/off via `directoryListing`

The server SHALL return a directory listing (status 200) for a
directory request with no usable index file when `directoryListing`
is `true` (the default) or when an array of glob patterns matches
the request path. When `directoryListing` is `false` (or no glob
matches), the request SHALL fall through to a 404 — UNLESS
`renderSingle: true` AND the directory contains exactly one
non-directory entry, in which case the `renderSingle`
short-circuit fires (see SRV-DLST-003). The listing SHALL have
`Content-Type: text/html; charset=utf-8` for HTML clients and
`Content-Type: application/json; charset=utf-8` for clients that
prefer JSON. The JSON shape SHALL be at least `{"files":[...],
"directory":..., "paths":...}`.

Custom response headers (SRV-HDR-001) SHALL NOT layer onto listing
200 responses. Reference's `renderDirectory` ends with
`response.end(directory)` and returns BEFORE the success-site
`getHeaders` call (`serve-handler/src/index.js:644-672` returning
before `index.js:746`); IrServe mirrors this by emitting `None` for
the headers-path on the listing branch in `dispatch_inner`.

Evidence: SRV-DLST-001 (status: verified, level: L1); oracle:
ORC-008, ORC-009, ORC-010.

Note: Per `decisions.md` D-003, exact HTML markup of the listing is
not in scope. Per `decisions.md` D-007, the JSON listing's
`directory`-equivalent field SHALL be rendered relative to the
served root. The chosen relative-path shape is:

- root → `directory: "."`, `paths: []`
- nested → `directory: "sub"` / `"sub/deep"`, `paths` accumulates
  segments as `[{name: "sub", url: "sub"}, {name: "deep", url:
  "sub/deep"}]`.

Separators are POSIX (`/`); no leading or trailing slash on the
top-level `directory` or `paths.url` fields. Per-entry `relative`
keeps the URL form with leading `/` (and trailing `/` for folders)
so the field doubles as an `href` for JSON consumers. Q-008 closed
by D-007.

#### Scenario: Default HTML listing

- GIVEN default config and fixture root has `a.txt` and `b.txt`
- WHEN `GET /` (no JSON-preferring Accept)
- THEN status is 200
- AND `Content-Type: text/html; charset=utf-8`

#### Scenario: Disabled listing falls through to 404

- GIVEN config `{ "directoryListing": false }`
- WHEN `GET /`
- THEN status is 404

#### Scenario: JSON listing content negotiation

- GIVEN default config
- WHEN `GET /` with `Accept: application/json`
- THEN status is 200
- AND `Content-Type: application/json; charset=utf-8`
- AND body contains the keys `files`, `directory`, `paths`
- AND no field carries a host-absolute filesystem path (D-007 sanitization)

#### Scenario: JSON listing at root has `directory: "."` and empty `paths`

- GIVEN default config and `GET /` with `Accept: application/json`
- WHEN the response is parsed
- THEN `directory` is the string `"."`
- AND `paths` is an empty array

#### Scenario: JSON listing under a subdirectory uses POSIX-relative shape

- GIVEN default config, `GET /sub/deep/` with `Accept: application/json`
- WHEN the response is parsed
- THEN `directory` is the string `"sub/deep"`
- AND `paths` is `[{"name": "sub", "url": "sub"}, {"name": "deep", "url": "sub/deep"}]`

### Requirement: `unlisted` and the default-excluded set

The server SHALL omit files matching `.DS_Store`, `.git`, or any
glob in `unlisted` from the directory listing while keeping them
directly fetchable via their explicit URL. The `unlisted` filter
SHALL affect listings only, not file resolution. The default
exclusion set SHALL be exactly `.DS_Store` and `.git`. The
defaults are prepended unconditionally and cannot be opted out
of (mirrors `excluded = ['.DS_Store', '.git', ...unlisted]` at
`serve-handler/src/index.js:330-334`).

Evidence: SRV-DLST-002 (status: verified, level: L1); oracle:
ORC-009, ORC-010.

#### Scenario: Default and configured exclusions

- GIVEN root has `a.txt`, `b.txt`, `secret.txt`, `.DS_Store`, and `.git/HEAD`, with `unlisted: ["secret.txt"]`
- WHEN `GET /` (HTML or JSON listing)
- THEN the response does not name `secret.txt`, `.DS_Store`, or `.git`

#### Scenario: Unlisted file is still fetchable

- GIVEN config `{ "unlisted": ["secret.txt"] }` and `/secret.txt` exists
- WHEN `GET /secret.txt`
- THEN status is 200
- AND body is the byte-exact file content

### Requirement: `renderSingle` serves a lone file in place of a listing

The server SHALL serve a lone non-HTML file directly (status 200,
the file's MIME type) in place of a directory listing when
`renderSingle: true` and a directory contains exactly one non-HTML
file with no usable index. The behavior SHALL be disabled by
default.

`renderSingle` SHALL still fire even when `directoryListing` is
`false` (or out-of-scope). When listing is off and `renderSingle`
is on, the server reads the directory; if the count-of-1 short-
circuit fires, the file is served; otherwise the request falls
through to 404. When BOTH listing is off AND `renderSingle` is
off, the request falls through to 404 without reading the
directory. Mirrors `applicable + renderSingle` at
`serve-handler/src/index.js:336-374`.

Evidence: SRV-DLST-003 (status: verified, level: L2); oracle:
ORC-011.

Note (`renderSingle`-before-`unlisted` ordering): the count-of-1
check SHALL be evaluated against the **unfiltered** directory
entries (mirrors `canRenderSingle = renderSingle && (files.length
=== 1)` at `serve-handler/src/index.js:342`, which runs BEFORE
the `canBeListed` filter at `index.js:387-391`). A directory
containing `.DS_Store` plus one real file therefore has count=2
and does NOT trigger `renderSingle` — it falls through to a
listing (or 404 if listing is off). Kickoff interview decision
#3 explicitly mirrors this literally rather than reordering for
intuition.

Note (custom headers on `renderSingle`): unlike HTML / JSON
listing responses, the `renderSingle` branch DOES go through
`apply_custom_headers`. Reference reroutes through the
file-serving site at `serve-handler/src/index.js:649-665`,
overriding `absolutePath` / `stats` and letting the request flow
back into `getHeaders` at `index.js:746`. IrServe's
`dispatch_inner` emits `Some(<file-url>)` for the
`RenderResult::Single` arm to mirror.

#### Scenario: Single file rendered

- GIVEN `renderSingle: true` and `media/photo.png` is the only entry under `media/`
- WHEN `GET /media/`
- THEN status is 200
- AND `Content-Type: image/png`
- AND body is the byte-exact file content

#### Scenario: `.DS_Store` + 1 real file does NOT trigger `renderSingle`

- GIVEN `renderSingle: true` and `media/` contains `photo.png` and `.DS_Store`
- WHEN `GET /media/`
- THEN the response is a directory listing (status 200, listing content-type), NOT the raw `photo.png` bytes

#### Scenario: `directoryListing: false` + `renderSingle: true` still fires for a lone file

- GIVEN `directoryListing: false`, `renderSingle: true`, and `media/photo.png` is the only entry under `media/`
- WHEN `GET /media/`
- THEN status is 200
- AND `Content-Type: image/png`

#### Scenario: `directoryListing: false` + `renderSingle: false` falls through to 404

- GIVEN `directoryListing: false`, `renderSingle: false` (or absent), and `media/` contains any number of entries
- WHEN `GET /media/`
- THEN status is 404

## Compatibility notes

- **No new `D-NNN` entries.** Stage 6g lands cleanly on top of
  existing decisions. HTML markup body bytes diverge under D-003;
  the JSON `directory` / `paths` sanitization shape lives under
  D-007; the `size` field as raw bytes (no `bytes`-package strings)
  also flows from D-003. Future maintenance touching directory
  listings should consult those two decisions before authoring a
  new one.

- **Reference-bypass of `getHeaders` for listings.** Custom response
  headers do NOT layer onto HTML or JSON listing 200 responses.
  Mirrors reference at `serve-handler/src/index.js:644-672`, which
  ends with `response.end(directory)` and returns before the
  success-site `getHeaders` call at `index.js:746`. The
  `renderSingle` branch DOES go through `apply_custom_headers`
  because reference reroutes through the file-serving site at
  `index.js:649-665`. IrServe's `dispatch_inner` returns
  `(response, None)` for the listing branch (suppressing the
  wrapper's `apply_custom_headers` pass) and `(file_response,
  Some(url))` for the `renderSingle` branch (engaging it).
