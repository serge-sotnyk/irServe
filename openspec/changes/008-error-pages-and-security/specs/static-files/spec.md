# Delta for static-files

## MODIFIED Requirements

### Requirement: Custom error pages via `<status>.html`

A `<statusCode>.html` file at the served root SHALL be sent as the
body of the matching error response (preserving the error status
code) for clients that do not prefer JSON. Clients that prefer JSON
SHALL still receive the JSON envelope from the "404 for missing
paths" Requirement regardless of any `<status>.html` file. The
mechanism SHALL apply to any error code, not only 404.

Evidence: SRV-FILE-003 (status: verified, level: L1); oracle: ORC-007, ORC-059; D-015.

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

## Compatibility notes

- **D-015 (irserve un-defer).** Stage 6f wires the `<status>.html`
  lookup at the served root inside `error_response`. JSON-preferring
  clients short-circuit before `apply_custom_headers` runs, mirroring
  reference's `index.js:477-487`. HTML clients with a custom page
  receive `apply_custom_headers` matched against `/<status>.html`
  (mirrors reference's `getHeaders(.., errorPage, stats)` at
  `index.js:508`). HTML clients without a custom page receive the
  generic `<h1>STATUS REASON</h1>\n` body and `apply_custom_headers`
  matched against the request path (mirrors reference's
  `getHeaders(.., absolutePath, null)` at `index.js:519`).

- **D-003 (markup parity).** The HTML body of the fallback (no
  `<status>.html`) deliberately diverges from the reference's full
  `errorTemplate` HTML. The contract pins status code and
  `Content-Type`, not body bytes. The `notfound-shape` probe's
  `bodyMayDiffer` partition continues to mask the divergence.
