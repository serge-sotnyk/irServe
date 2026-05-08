# static-files Specification

## Purpose

File-resolution behavior: serving regular files for matching paths,
404 handling, custom error pages, MIME-type defaults, and `index.html`
resolution for directory paths. This capability defines the terminal
stages of the request pipeline once routing has settled on a target.

## Requirements

### Requirement: Serve regular files for matching paths

The server SHALL return status 200 with the file's content as the
body, and a `Content-Length` header equal to the file size, for any
`GET` whose URL path resolves to a regular file inside the served
root after redirect and rewrite resolution.

Evidence: SRV-FILE-001 (status: verified, level: L0); oracle: ORC-001.

#### Scenario: Existing file

- GIVEN file `/data.json` exists in the served root
- WHEN `GET /data.json`
- THEN status is 200 and body is the byte-exact file content

### Requirement: 404 for missing paths

The server SHALL respond with status 404 when a request cannot be
resolved to a file or a directory listing. When `Accept:
application/json` is honored by content negotiation, the body SHALL
be `{"error":{"code":"not_found","message":"The requested path could
not be found"}}` with `Content-Type: application/json; charset=utf-8`.
Otherwise, the body SHALL be an HTML error page with `Content-Type:
text/html; charset=utf-8`.

Evidence: SRV-FILE-002 (status: verified, level: L0); oracle: ORC-004, ORC-005, ORC-051, ORC-052, ORC-059.

Note: Per `decisions.md` D-003, exact HTML markup of the error body is
not in scope; only the status code and `Content-Type` are part of the
contract. The JSON envelope IS in scope.

#### Scenario: HTML 404

- GIVEN no file at `/missing` in the served root
- WHEN `GET /missing` with no JSON-preferring `Accept`
- THEN status is 404
- AND `Content-Type: text/html; charset=utf-8`

#### Scenario: JSON 404

- GIVEN no file at `/missing`
- WHEN `GET /missing` with `Accept: application/json`
- THEN status is 404
- AND `Content-Type: application/json; charset=utf-8`
- AND body is `{"error":{"code":"not_found","message":"The requested path could not be found"}}`

### Requirement: Custom error pages via `<status>.html`

A `<statusCode>.html` file at the served root SHALL be sent as the
body of the matching error response (preserving the error status
code) for clients that do not prefer JSON. Clients that prefer JSON
SHALL still receive the JSON envelope from the "404 for missing
paths" Requirement regardless of any `<status>.html` file. The
mechanism SHALL apply to any error code, not only 404.

Evidence: SRV-FILE-003 (status: verified, level: L1); oracle: ORC-007, ORC-059.

#### Scenario: Custom 404 served to HTML clients

- GIVEN `404.html` exists in the served root
- WHEN `GET /missing` (no JSON-preferring Accept)
- THEN status is 404
- AND body is the contents of `404.html`

#### Scenario: JSON clients ignore custom HTML page

- GIVEN `404.html` exists in the served root
- WHEN `GET /missing` with `Accept: application/json`
- THEN status is 404
- AND body is the JSON error envelope (not the HTML page)

### Requirement: Default MIME types

The `Content-Type` header for a file response SHALL be determined by
the file's extension. The following bindings SHALL be preserved:

- `.html` → `text/html; charset=utf-8`
- `.js` → `application/javascript; charset=utf-8`
- `.json` → `application/json; charset=utf-8`
- `.css` → `text/css; charset=utf-8`
- `.txt` → `text/plain; charset=utf-8`
- `.svg` → `image/svg+xml`
- `.wasm` → `application/wasm`
- `.png` → `image/png`
- file with no extension → no `Content-Type` header
- file with an unknown extension → no `Content-Type` header

The `charset=utf-8` suffix on text types SHALL be part of the contract.
IrServe MAY ship a different default MIME database, provided the
bindings above are preserved.

Evidence: SRV-FILE-004 (status: verified, level: L0); oracle: ORC-003.

Note: Q-004 tracks the open question of MIME bindings beyond the
probed set. Bindings outside this list are best-effort.

#### Scenario: SVG content-type

- GIVEN `/icon.svg` exists
- WHEN `GET /icon.svg`
- THEN `Content-Type: image/svg+xml`

#### Scenario: Extensionless file has no Content-Type

- GIVEN `/noext` exists with no extension
- WHEN `GET /noext`
- THEN no `Content-Type` header is set

### Requirement: `index.html` resolution for directory paths

The server SHALL return `index.html` as a 200 response for any
request whose path resolves to a directory containing that file
(subject to `cleanUrls` redirects from `/dir/index` and
`/dir/index.html` per the routing capability). When no index file is
present, the directory listing or 404 path SHALL take over (see the
directory-listing capability).

Evidence: SRV-FILE-005 (status: verified, level: L0); oracle: ORC-001.

#### Scenario: Root index.html

- GIVEN `index.html` at the served root
- WHEN `GET /`
- THEN status is 200
- AND body is the contents of `index.html`

#### Scenario: Sub-directory index.html with cleanUrls default

- GIVEN `about/index.html` and `cleanUrls` defaults
- WHEN `GET /about/`
- THEN status is 200
- AND body is the contents of `about/index.html`
