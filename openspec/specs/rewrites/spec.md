# rewrites Specification

## Purpose

Configured URL rewrites from `serve.json` that change which file backs
a request without altering the response URL. Rewrites participate in
the routing precedence pipeline after redirects and before cleanUrls
extensionless resolution; they short-circuit static-file resolution
when matched.

## Requirements

### Requirement: `rewrites` serve a different file with status 200

A `rewrites` entry `{source, destination}` SHALL match the request
path against `source` (minimatch or `path-to-regexp`). On match, the
server SHALL respond with status 200 (no redirect) and SHALL serve the
file at `destination` (with `path-to-regexp` segments interpolated).
The URL bar SHALL NOT change (rewrites are silent).

Whether a rewrite is short-circuited by an existing original-path file
SHALL depend on the path shape:

- If the original request path has a non-empty extension (e.g.
  `/page.html`, `/asset.css`) and the file exists, the file SHALL be
  served directly and the rewrite SHALL NOT apply.
- If the original request path has no extension (e.g. `/about`,
  `/api`), a matching rewrite SHALL serve its destination instead,
  even if the extensionless original path exists as a regular file.

Evidence: SRV-RWRT-001 (status: verified, level: L2); oracle: ORC-028, ORC-029.

Note: This pre-stat asymmetry is part of the routing pipeline; see the
routing capability's "Operation precedence in the request pipeline"
Requirement for the full ordering. MIME-type fallback when rewriting
(content-type follows the destination file's extension) is governed by
a separate L3 Requirement under the rewrites capability and is not
part of this baseline.

#### Scenario: Path-segment rewrite

- GIVEN `serve.json` with rewrite `{ "source": "/projects/:id/edit", "destination": "/edit-project-:id.html" }`
- WHEN `GET /projects/123/edit`
- THEN status is 200
- AND body is the contents of `/edit-project-123.html`

#### Scenario: SPA wildcard rewrite

- GIVEN rewrite `{ "source": "/spa/**", "destination": "/index.html" }`
- WHEN `GET /spa/some/deep/path`
- THEN status is 200
- AND body is the contents of `/index.html`
