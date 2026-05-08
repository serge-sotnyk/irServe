# Delta for directory-listing

## ADDED Requirements

### Requirement: Directory listing on/off via `directoryListing`

The server SHALL return a directory listing (status 200) for a
directory request with no usable index file when `directoryListing`
is `true` (the default) or when an array of glob patterns matches
the request path. When `directoryListing` is `false` (or no glob
matches), the request SHALL fall through to a 404. The listing SHALL
have `Content-Type: text/html; charset=utf-8` for HTML clients and
`Content-Type: application/json; charset=utf-8` for clients that
prefer JSON. The JSON shape SHALL be at least `{"files":[...],
"directory":..., "paths":...}`.

Evidence: SRV-DLST-001 (status: verified, level: L1); oracle: ORC-008, ORC-009, ORC-010.

Note: Per `decisions.md` D-003, exact HTML markup of the listing is
not in scope. Per `decisions.md` D-007, the JSON listing's
`directory`-equivalent field SHALL be rendered relative to the
served root (e.g. `"."` for the root itself, `"sub"` for a `sub/`
subdirectory) — diverging from `serve`'s absolute-path leak. The
JSON-listing content-negotiation behavior is otherwise preserved.
Q-008 closed by D-007.

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

### Requirement: `unlisted` and the default-excluded set

The server SHALL omit files matching `.DS_Store`, `.git`, or any
glob in `unlisted` from the directory listing while keeping them
directly fetchable via their explicit URL. The `unlisted` filter
SHALL affect listings only, not file resolution. The default
exclusion set SHALL be exactly `.DS_Store` and `.git`.

Evidence: SRV-DLST-002 (status: verified, level: L1); oracle: ORC-009, ORC-010.

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

Evidence: SRV-DLST-003 (status: verified, level: L2); oracle: ORC-011.

#### Scenario: Single file rendered

- GIVEN `renderSingle: true` and `media/photo.png` is the only entry under `media/`
- WHEN `GET /media/`
- THEN status is 200
- AND `Content-Type: image/png`
- AND body is the byte-exact file content
