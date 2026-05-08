# Delta for security

## ADDED Requirements

### Requirement: Path traversal outside the served root is denied

The server SHALL NOT serve files outside the served root. Wire-level
behavior SHALL be:

- A request whose path decodes to a sequence containing `..` segments
  that escape the served root SHALL return status 400 with a body
  shape consistent with the `bad_request` error template.
- A request whose path is percent-encoded `..` (e.g. `/%2e%2e/...`)
  SHALL follow the same path: single-decode, then containment check,
  then 400.
- A request with a leading `//` (e.g. `//etc/passwd`) SHALL be treated
  as an in-root path that simply does not resolve, yielding 404 (not
  400). The `//` SHALL NOT be treated as an escape on its own.
- A request whose URL contains a malformed `%`-escape (e.g. `/%zz`)
  SHALL return 400.

IrServe MAY use any pipeline as long as the root-escape invariant
holds and the four scenarios below are preserved.

Evidence: SRV-SEC-001 (status: verified, level: L2); oracle: ORC-038, ORC-039, ORC-040, ORC-041.

#### Scenario: Literal `..` segment is rejected

- GIVEN any fixture root
- WHEN a wire-level `GET /../package.json HTTP/1.1` is sent
- THEN status is 400

#### Scenario: Percent-encoded `..` is rejected

- GIVEN any fixture root
- WHEN a wire-level `GET /%2e%2e/package.json HTTP/1.1` is sent
- THEN status is 400

#### Scenario: Leading double slash is benign

- GIVEN any fixture root
- WHEN a wire-level `GET //etc/passwd HTTP/1.1` is sent
- THEN status is 404 (path stays inside root after slash collapse)

#### Scenario: Malformed percent-escape is rejected

- GIVEN any fixture root
- WHEN a wire-level `GET /%zz HTTP/1.1` is sent
- THEN status is 400

### Requirement: URL is decoded once

The path SHALL be URI-decoded exactly once before resolution. Double
decoding (e.g. `%252e` → `%2e` → `.`) SHALL NOT occur. Malformed
escapes SHALL yield 400. Single-decode preserves literal `%2e` in
filenames if any exist.

Evidence: SRV-SEC-002 (status: verified, level: L2); oracle: ORC-034, ORC-035, ORC-036, ORC-037.

#### Scenario: Encoded `..` is normalized at the client (404 inside root)

- GIVEN any fixture root
- WHEN a `fetch`-mode `GET /%2e%2e/etc/passwd` is sent (client normalizes the encoded `..`)
- THEN status is 404 (the resulting path stays inside the root and does not resolve)

#### Scenario: Double-slash before `/etc/passwd` (fetch mode)

- GIVEN any fixture root
- WHEN a `fetch`-mode `GET //etc/passwd` is sent
- THEN status is 404

#### Scenario: Raw `..` segments at fetch normalization layer

- GIVEN any fixture root
- WHEN a `fetch`-mode `GET /../../etc/passwd` is sent
- THEN status is 404

#### Scenario: Single-pass decode is symmetric

- GIVEN fixture has `/secret.txt` (in root, not unlisted)
- WHEN a `fetch`-mode `GET /sub/%2e%2e/secret.txt` is sent (decoded once → `/secret.txt`, which IS inside root)
- THEN status is 200 (single-pass decode does not double-decode and does not over-strip)
