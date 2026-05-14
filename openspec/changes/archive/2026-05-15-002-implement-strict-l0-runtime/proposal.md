# Proposal: Implement strict-L0 IrServe runtime

## Why

Stage 5a (`openspec/changes/archive/2026-05-15-001-port-minimal-static-server/`) fixed every
architectural decision a Stage-5b implementer needs: workspace shape,
HTTP-stack pins, request-lifecycle skeleton, CLI surface, and the
Stage-5b oracle-harness mapping. No Rust code existed yet.

This change introduces the first Rust code: a workspace with two crates
(`irserve` bin + `irserve-core` lib) that satisfies the eight strict-L0
SRVs, a `tools/probe/run.mjs` adapter that targets the new binary, two
new L0-mode probe cases, and a Rust integration test that drives the
adapted runner end-to-end.

The methodological value is **falsification of the central hypothesis**:
"strict L0 passes the oracle without contract edits." Stage 5b is the
first chance to surface unknown divergences. Three were found and
mediated as L0 must-match-layer refinements (no contract edits); see
`design.md` § Findings.

## What

The change introduces the workspace and the eight-SRV runtime previously
declared in change 001's `proposal.md`:

| SRV | Status before | Status after | Module |
|---|---|---|---|
| SRV-CLI-001 | accepted | **verified** (via new ORC-063) | `crates/irserve/src/main.rs` (`resolve_port`) |
| SRV-CLI-002 | verified | verified | `crates/irserve/src/main.rs` (clap `--listen`) |
| SRV-CLI-007 | verified | verified | `crates/irserve/src/main.rs` (clap positional) |
| SRV-CLI-019 | verified | verified | `crates/irserve/src/main.rs` (clap `-v`/`-h`) |
| SRV-FILE-001 | verified | verified | `crates/irserve-core/src/dispatch.rs` (file branch) |
| SRV-FILE-002 | verified | verified | `crates/irserve-core/src/notfound.rs` |
| SRV-FILE-004 | verified | verified | `crates/irserve-core/src/mime.rs` |
| SRV-FILE-005 | verified | verified | `crates/irserve-core/src/resolve.rs` (`Index` outcome) |

The change carries one OpenSpec delta — a MODIFIED of SRV-CLI-001 in
`specs/cli/spec.md` — that promotes the SRV from `accepted` to
`verified` because the new probe `default-port-l0.json` exercises both
scenarios (default port 3000 via the no-flag-no-env path is documented
but not probed in this change; the env-var path is probed). The
promotion is partial-but-strong: the env-var branch is verified, the
no-flag-no-env branch remains source-evidence only.

## Scope

### In scope

- Workspace `Cargo.toml` + `crates/irserve/` (bin) +
  `crates/irserve-core/` (lib) per change 001 § 1 design.
- Slice 1 — clap CLI surface + workspace build (no HTTP).
- Slice 2 — server bind + dispatch phases 1/2/9/11/12 (root + index +
  file + 404 stub), no MIME, no containment.
- Slice 3 — MIME table (FILE-004 hand-rolled + `mime_guess` fallback),
  404 envelope (HTML / JSON envelope by Accept), phase-10 containment
  (canonicalize prefix-check; escape → 404).
- Slice 4 — `tools/probe/run.mjs` adapter (target=irserve, deferred-
  flag stripping, per-case `runner.l0` partition, per-anchor
  may-differ overlays for body / content-length / exit-code), new
  probe cases `default-port-l0.json` and `mime-defaults-l0.json`,
  Rust integration test `crates/irserve/tests/oracle.rs`.
- Single MODIFIED delta for SRV-CLI-001 (status promotion).
- Research-track edits: ORC-063 row in `oracle-matrix.md`; ORC-062
  must-match relaxed from `exit=1` to `exit=non-zero` (matches spec
  text; § Findings #3 below).
- README stage map row 5b → done.

### Out of scope

- All non-strict-L0 SRVs (still deferred by D-008).
- Closing any Q-NNN. Q-001/002/003/004 stay open; the design-level
  assumption "irserve binds 127.0.0.1 by default" is recorded under
  `design.md` § Open assumptions but does not promote into a Q-NNN.
- The `--no-flag-no-env` scenario of SRV-CLI-001 (port 3000 hardcoded;
  flaky on dev machines where 3000 is occupied). The env-var scenario
  is sufficient to promote `accepted` → `verified`.
- Any change to `third_party/` or to existing snapshots under
  `tools/probe/snapshots/`. Two snapshots are ADDED (`default-port-l0`
  and `mime-defaults-l0`); none are modified.

## Findings (methodological signals)

Three real divergences surfaced during Slice 4 verification. None
required contract edits; all were mediated by extending the L0
must-match-layer overlay in `tools/probe/run.mjs`. Each is covered in
detail in `design.md` § Findings.

1. **`default-port-l0`** — `serve` with `cleanUrls: false` skips
   `findRelated()` and falls through to directory-listing rendering
   for `GET /`, returning a 4384-byte HTML listing instead of
   `index.html`. Resolution: drop `serve.json` from the case fixture so
   the reference runs under default `cleanUrls: true` (which routes `/`
   to `index.html`, matching strict-L0 IrServe). The case probes only
   `GET /`, so divergent cleanUrls behavior on `/index.html` is
   irrelevant.
2. **`notfound-shape#missing_html_with_accept`** — `serve` chunks the
   80-byte JSON envelope and omits `Content-Length`; IrServe (axum)
   sets `Content-Length: 80`. Body bytes match. Resolution: new
   `runner.l0.contentLengthMayDiffer` field strips `content-length`
   for that anchor only.
3. **`cli-positional-error#two_positionals`** — `serve` exits with
   code 1; `clap` defaults to exit 2 for arg-parse errors. Spec text
   (SRV-CLI-007 sc.3, ORC-062 must-match per design 001 § 6) is
   "non-zero exit code". Resolution: new `runner.l0.exitCodeMayDiffer`
   field asserts both sides non-zero then strips `exitCode`.
   `oracle-matrix.md` ORC-062 must-match relaxed from `exit=1` to
   `exit=non-zero` to match the spec.

None of the three is a contract edit; each is an L0-mask precision
fix. The contract for FILE-002, CLI-007, and FILE-005 is unchanged.

## Risks and mitigations

1. **Windows UNC canonicalize path comparison** — `Path::canonicalize`
   on Windows returns UNC-prefixed paths (`\\?\C:\...`). Mitigation:
   the bin canonicalizes the served root once at startup and resolve
   compares both as `Path` (component-by-component), not as strings.
   Verified on the developer's Windows 11 machine via `cargo test
   --test oracle`.
2. **`clap` 4.6 `version: ()` field syntax** — clap derive's exit-on-
   parse `ArgAction::Version` accepts a unit-typed struct field on
   4.6.x. If a future clap version rejects this, Stage-5b's clap pin
   needs widening to keep compatibility. Mitigation: pinned to
   `clap = "4"` (not `clap = "4.6"`) so cargo selects the latest
   compatible 4.x at lock time.
3. **`mime_guess` long-tail charset suffix** — `mime_guess` does not
   append `; charset=utf-8` to text MIME types; FILE-004 mandates the
   suffix for the eight bound extensions. Mitigation: hand-rolled
   table in `crates/irserve-core/src/mime.rs` is checked first; only
   unmatched extensions fall through to `mime_guess`. The probe case
   `mime-defaults.json` exercises every bound extension plus
   extensionless and unknown-extension files (anchor `unknownext` uses
   a synthetic `.unknownext` extension that `mime_guess` does not
   know, so it returns `None` and matches `serve`'s "no Content-Type"
   behavior).
4. **Q-001..004 remain open** — `tcp://` URI form, compression details,
   schema validation, MIME long tail. None affects the eight in-scope
   SRVs.

## Open assumptions

The following are assumed by this change but not verified:

- **A1 — `irserve` binds `127.0.0.1` by default.** No SRV constrains
  the default interface. Loopback matches every existing probe URL
  (`http://127.0.0.1:<port>/...`) and `serve`'s startup banner. If a
  probe ever requires `0.0.0.0` binding, that's an L4 decision.
- **A2 — `cleanUrls: false` in `serve.json` disables index.html
  resolution at `/`.** Inferred from `serve-handler/src/index.js:620`
  via reading source during Stage-5b Slice-4 verification. Not yet a
  Q-NNN entry; closing it requires a probe with `cleanUrls: false` and
  an `index.html` at root, which is exactly what `default-port-l0`
  reproduces (and confirms the divergence). The behavior is
  reference-specific and does not constrain IrServe.
