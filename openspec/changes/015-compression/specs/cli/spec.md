# Delta for cli

## ADDED Requirements

### Requirement: `-u`/`--no-compression` disables HTTP compression

By default the server SHALL apply HTTP compression to
compressible-MIME responses above a fixed body-size
threshold when the client's `Accept-Encoding` header
admits a supported encoder. With `-u` / `--no-compression`,
the server SHALL NOT apply any compression to any
response: bodies are sent uncompressed and no
`Content-Encoding` is emitted; `Vary: Accept-Encoding` is
also NOT emitted (the negotiation hook is skipped
wholesale). The flag SHALL be accepted in both its short
and long forms, mirroring the reference's
`-u, --no-compression` declaration at
`third_party/serve/source/utilities/cli.ts:53,154,171`
and the per-request gate at
`third_party/serve/source/utilities/server.ts:71-72`
(`if (!args['--no-compression']) await compress(...)`).

The full L3 wire surface — encoder set, negotiation
preference order, threshold, MIME filter, Vary semantics,
skip conditions (HEAD, `Cache-Control: no-transform`,
below-threshold, identity-only, all-`q=0`, already-encoded
non-identity `Content-Encoding` passthrough), 206 /
threshold-gate composition (small ranges pass through
uncompressed with Vary; large ranges encode like 200s)
— is documented as a separate L3 capability spec at
`openspec/specs/http-compression/spec.md` and is not
duplicated in this baseline. The compatibility
divergences from the reference's `compression@1.8.1`
middleware are recorded as **D-020** in
`docs/reference/serve/decisions.md`.

Evidence: SRV-CLI-012 (status: verified, level: L3);
oracle: ORC-058 (`compression-default.json#with_accept_encoding`),
ORC-191..ORC-213 (`compression-raw.json` — 23 anchors
covering the MIME allowlist, negotiation matrix, skip
conditions, user-`Content-Encoding` identity / non-identity
passthrough, and the above-threshold-range encode case).
Promoted via Stage 7e slice 2 (commit `d1b9a15`);
the `-u` flag was wired but no-op pre-7e per D-006,
which transitions from `adapted` (100 % deferred) to
implemented by 7e (D-006 historical entry stays; D-020
records the four surviving divergences from reference).

Implementation:
`crates/irserve/src/main.rs::Cli::no_compression`
(`#[arg(short = 'u', long = "no-compression")]`) parses
the flag; the post-parse override forces
`serve_config.compression = Some(false)` when set.
Absence leaves `serve_config.compression` as `None`
(= compress by default). The seam is a centralized
post-dispatch pass in
`crates/irserve-core/src/server.rs::handler` (between
`apply_cors` and the request log — moved from
`build_file_or_304` by Codex round 1 P2 so directory
listings and error pages also get compression).
`maybe_apply`'s first gate is
`if serve_config.compression == Some(false) { return
response; }`, mirroring the reference's per-request
`if (!args['--no-compression'])` gate at
`third_party/serve/source/utilities/server.ts:71-72`.

#### Scenario: Default behavior — compressible asset compresses with `Vary` set

- GIVEN `serve` (defaults) over a directory containing
  `big.css` whose body is > 1024 bytes (above the
  `compression@1.8.1` default threshold)
- WHEN `GET /big.css` with
  `Accept-Encoding: gzip, deflate, br`
- THEN status is 200
- AND the response carries `Vary: Accept-Encoding`
- AND the response carries `Content-Encoding: br` (the
  preference winner)
- AND the response body is the brotli-encoded form of
  the file

#### Scenario: `--no-compression` flag disables compression wholesale

- GIVEN `serve --no-compression` (or `serve -u`) over a
  directory containing the same `big.css`
- WHEN `GET /big.css` with
  `Accept-Encoding: gzip, deflate, br`
- THEN status is 200
- AND the response does NOT carry `Vary` (the
  negotiation hook was skipped wholesale)
- AND the response does NOT carry `Content-Encoding`
- AND the response body is the raw file contents

#### Scenario: Short alias `-u` is equivalent to `--no-compression`

- GIVEN `serve -u`
- WHEN any request is made
- THEN behavior is identical to `serve --no-compression`
