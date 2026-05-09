# Open questions

Behavior of `vercel/serve` that requires runtime probing or additional research before it can become an accepted requirement. Entries here have status `unknown` or `needs-oracle` in `inventory.md`.

## Format

```
## Q-<NNN>: <short title>

Affected area: <capability area>
Suspected behavior: <what we currently assume>
How to verify:
- <oracle probe to run, or source/test to inspect>
Resolution: <fill in once answered, then move the requirement to inventory.md>
```

## Entries

## Q-001: Default port/host on `tcp://` URI without explicit values

Affected area: cli
Suspected behavior: A `tcp://hostname` (no port) defaults to port 3000; `tcp://:1234` (no host) defaults to `localhost`. Source code suggests this; not probed.
How to verify:
- Probe: spawn `serve -l tcp://127.0.0.1` and `serve -l tcp://:3010`, confirm reachable via the inferred default.
Resolution: open.

## Q-002: HTTP compression — content-type set and minimum body size

Affected area: cli (SRV-CLI-012)
Suspected behavior: `serve` uses the `compression` middleware with defaults. Default threshold is roughly 1 KiB and content-types are gzippable text-like ones.
How to verify:
- Inspect actual `Content-Encoding` headers (the probe runner currently does not track `content-encoding`; extend the runner or use `curl -i`).
Resolution: open.

## Q-003: Schema validation error format and exit codes

Affected area: config (SRV-CFG-001)
Suspected behavior: `serve` exits non-zero with an AJV error message on schema-invalid configs. Exact message format is implementation-specific.
How to verify:
- Probe: feed serve a config violating each field's schema; capture exit code and stderr.
Resolution: open. IrServe will not match exact wording (see D-002).

## Q-004: MIME-type bindings beyond the probed set

Affected area: static-files (SRV-FILE-004)
Suspected behavior: Anything in the `mime-types` Node package's table.
How to verify:
- Extend `tools/probe/cases/mime-defaults.json` with more extensions if a specific binding becomes contentious; until then, document the probed set as the contract and treat the rest as best-effort.
Resolution: open.

## Q-005: Precedence between `<dir>/index.html` and `<dir>.html`

Affected area: routing (SRV-ROUT-002)
Suspected behavior: When both `about.html` and `about/index.html` exist, `GET /about` (cleanUrls on) serves `about/index.html`. Probe `prec-cleanurls-default` confirms this for the default config.
How to verify:
- Verified by probe `tools/probe/cases/prec-cleanurls-default.json` (default cleanUrls).
- Verified by probe `tools/probe/cases/cleanurls-array.json` (array-form cleanUrls): paths inside the configured glob receive 301 to extensionless and 200 on extensionless GET; paths outside the glob serve `.html` directly with no rewrite (200) and 404 on extensionless GET.
Resolution: closed by snapshots `tools/probe/snapshots/prec-cleanurls-default.json` and `tools/probe/snapshots/cleanurls-array.json`. ORC-013, ORC-021, ORC-022, ORC-023, ORC-024.

## Q-006: Multi-slash collapse when `trailingSlash` is unset

Affected area: routing (SRV-ROUT-005)
Suspected behavior: Source line `src/index.js:158-160` triggers slash-collapse only inside the `slashing` branch (`typeof trailingSlash === 'boolean'`). With `trailingSlash` unset, `GET /a//b` may pass through verbatim.
How to verify:
- Probe `tools/probe/cases/multislash-collapse.json` issues raw-socket requests with literal `//` segments under default config (no `trailingSlash` set).
Resolution: closed by snapshot `tools/probe/snapshots/multislash-collapse.json`. Observed: serve normalizes consecutive slashes silently — `GET //` returns 200 (root index), `GET //docs/guide.html` and `GET /docs//guide.html` both 301 to `/docs/guide` (cleanUrls 301 fires after the slash collapse). ORC-025, ORC-026, ORC-027.

## Q-007: External-URL redirect destinations — relative vs scheme-relative

Affected area: redirects (SRV-RDIR-003)
Suspected behavior: Absolute URLs (`https://example.com/x`) are passed through; scheme-relative (`//example.com/x`) and relative paths are normalized via `slasher`.
How to verify:
- Probe: configure a redirect to each form and observe `Location` header.
Resolution: open.

## Q-008: Directory listing JSON shape — is it part of the contract?

Affected area: directory-listing (SRV-DLST-001)
Suspected behavior: The probe `listing-unlisted` shows the JSON listing leaks absolute filesystem paths in its `dir` field. README does not specify the shape; the only documentation is the existence of HTML/JSON content negotiation.
How to verify:
- User decision required. Options: (a) match the JSON shape including the `dir` leak (bug-for-bug); (b) define a minimal IrServe JSON listing schema that omits absolute paths; (c) drop JSON listings entirely from MVP.
Resolution: closed (option b) by `D-007`. IrServe preserves the JSON-listing shape and content-negotiation behavior but renders `dir` relative to the served root.

## Q-009: `If-Modified-Since` handling under `--no-etag`

Affected area: http-cache (SRV-CACHE-003)
Suspected behavior: When `--no-etag` is set, the server emits `Last-Modified` but no source-level branch handles `If-Modified-Since`. So a conditional GET probably returns 200 with the full body.
How to verify:
- Probe: serve with `--no-etag`, capture `Last-Modified`, re-issue with `If-Modified-Since: <that-value>`.
Resolution: open.

## Q-010: Wire-level path traversal behavior against a non-normalizing client

Affected area: security (SRV-SEC-001)
Suspected behavior: Source returns 400 (`bad_request`) when the joined path escapes the served root. Node `fetch` in the probe runner normalizes `%2e%2e` and `..` segments client-side, so the request sent over the wire never carries the unnormalized form.
How to verify:
- Probe: replace `fetch` with a raw `net.connect` request that sends the exact bytes `GET /%2e%2e/etc/passwd HTTP/1.1\r\n...`. Capture status.
Resolution: closed by snapshot `tools/probe/snapshots/traversal-raw-encoded.json`. Wire-level findings: literal `..` and `%2e%2e` escapes return 400; `//etc/passwd` returns 404 (double-slash is not, by itself, an escape); malformed `%`-escapes return 400. ORC-038, ORC-039, ORC-040, ORC-041; SRV-SEC-001 promoted to `verified` in Stage 3.

## Q-012: cleanUrls extglob (`+(...)`, `@(...)`, `?(...)`, `*(...)`, `!(...)`) parity

Affected area: routing (SRV-ROUT-001, SRV-ROUT-002 — array-form scope)
Suspected behavior: `serve-handler` calls `minimatch` via `sourceMatches` (`serve-handler/src/index.js:59`), which honors **extglob** constructs by default — Bash-style extended glob patterns where `+(a|b)` matches one or more occurrences of `a|b`, `@(a|b)` matches exactly one, `?(a|b)` matches zero or one, `*(a|b)` matches zero or more, and `!(a|b)` matches anything except. Reference test evidence: `serve-handler/test/integration.test.js:449` exercises extglob in cleanUrls. Probe-confirmed via Codex review round 3 (`cleanUrls: ["/public/+(page|other).html"]`, `GET /public/page.html` → reference `301 Location: /public/page`).
How to verify:
- Probe `tools/probe/cases/cleanurls-extglob.json` (reference-only): `cleanUrls: ["/public/+(page|other).html"]`, fixture `/public/page.html` and `/public/other.html`. Reference snapshot recorded; no `runner.l0` block (auto-skips against irserve). Anchors: `extglob_in_scope_redirect` (`GET /public/page.html` → 301 `/public/page`), `extglob_alternation_other` (`GET /public/other.html` → 301 `/public/other` — confirms `+(page|other)` alternation), `extglob_in_scope_extensionless` (`GET /public/page` → 404 — the extglob pattern requires `.html` so the extensionless resolution leg doesn't fire even on the reference).
Resolution: open. IrServe currently uses `globset::GlobBuilder` which supports `*`, `**`, `?`, character classes `[...]`, and brace alternation `{a,b}` but **not** extglob. Three options for a future close:
- (a) Implement extglob via manual translation to `regex` (mirroring minimatch's algorithm). Significant code; needs careful semantic alignment.
- (b) Adopt a Rust crate that supports extglob natively. As of context7 lookup on 2026-05-09, the surveyed candidates (`globset 0.4.18`, `fast-glob 1.0.1`, `glob-match 0.2.1`, `wax 0.7.0`) do NOT advertise extglob; `wcmatch` (Python) and `node-glob`/`minimatch` (JS) do, but Rust analogues are missing.
- (c) Record an `adapted` D-NNN scoping IrServe to the standard glob set, and document the divergence in the spec text. Most existing real-world `cleanUrls` configurations use plain `**`-style globs (the `serve` README's example is `/!components/**`), so the practical impact is small.
Until closed, the SRV-ROUT-001/002 array-form scope claim in `openspec/specs/routing/spec.md` should be read as "standard globs", not "minimatch-equivalent".

## Q-011: Windows symlink/junction parity

Affected area: symlinks (SRV-SYM-001)
Suspected behavior: `serve-handler` calls `fs.realpath`; behavior on Windows symlinks vs junctions vs reparse points is governed by the OS and Node.
How to verify:
- L4 work, deferred.
Resolution: deferred.
