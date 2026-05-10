# Delta for cli

## MODIFIED Requirements

### Requirement: `-s`/`--single` SPA fallback

The `-s`/`--single` flag SHALL serve `/index.html` (status 200, the
file's MIME type) for any request whose path does not resolve to a
file. The fallback SHALL be implemented as a high-priority rewrite —
per `third_party/serve/source/main.ts:78-90`, the synthetic rule
`{source: "**", destination: "/index.html"}` is prepended to the
user's `serve.json` rewrites at config-load time — and SHALL
therefore be overridden by an earlier-matching redirect (per the
operation precedence in the routing capability).

Evidence: SRV-CLI-008 (status: verified, level: L2); oracle:
ORC-029 (`cases/rewrites-segment.json#spa_fallback` — equivalent SPA
pattern via serve.json), ORC-149
(`cases/single-flag.json#spa_root`), ORC-150
(`cases/single-flag.json#spa_deep`), ORC-151
(`cases/single-flag.json#spa_with_existing_html` — pre-stat
asymmetry: has-extension request whose original file exists is
served directly, NOT replaced by `--single`'s catch-all), ORC-152
(`cases/single-with-redirect.json#redirect_wins_over_single` —
phase 6 redirect fires before phase 7 rewrite).

The `single-flag.json` fixture explicitly sets `cleanUrls: false`
to isolate phase-7 behavior — with cleanUrls on (the default),
`spa_with_existing_html` would 301 via phase 4 instead of testing
the pre-stat asymmetry.

#### Scenario: SPA fallback for missing route

- GIVEN `serve --single` over a directory containing `index.html`
- WHEN `GET /no/such/route`
- THEN status is 200 and body is the contents of `index.html`

#### Scenario: Redirect beats SPA fallback

- GIVEN `serve --single` and a `serve.json` with a redirect from `/old` to `/new`
- WHEN `GET /old`
- THEN status is 301 (the redirect, not the SPA fallback)

#### Scenario: SPA fallback at root

- GIVEN `serve --single` over a directory containing `index.html`
- WHEN `GET /`
- THEN status is 200 and body is the contents of `index.html`

#### Scenario: Has-extension existing file is not displaced

- GIVEN `serve --single` and `cleanUrls: false`, with both `/index.html` and `/existing.html` present
- WHEN `GET /existing.html`
- THEN status is 200 and body is the contents of `/existing.html` (the pre-stat asymmetry serves the existing file directly; the `--single` rewrite does not displace it)
