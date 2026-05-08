# Proposal: Port a minimal static server (strict L0)

## Why

The OpenSpec contract was bootstrapped at Stage 4 (archived as
`changes/archive/2026-05-08-000-establish-serve-compatibility-baseline/`)
and now lives at `openspec/specs/<capability>/spec.md`. It documents 35
Requirements across L0–L2 but says nothing about how the Rust port should
be structured.

Stage 5a delivers the **first implementation proposal**: a textual contract
for the smallest possible Rust port — a strict-L0 static server backed by
exactly eight SRVs. The change carries no Requirement deltas; the contract
above is unchanged. Its value is fixing concrete architectural decisions
(crate layout, HTTP stack, request-lifecycle order) **before** any
`Cargo.toml` exists, so Stage 5b implementation work has a reviewable
target.

The strict-L0 cutoff is recorded as a release-scoping decision in
`docs/reference/serve/decisions.md` D-008.

## What

Implement, in change `002-implement-strict-l0-runtime` (Stage 5b), a Rust
binary `irserve` (workspace bin) backed by `irserve-core` (workspace lib)
that satisfies the following eight SRVs from the archived contract:

| SRV | Status | Level | Spec | Oracle |
|---|---|---|---|---|
| SRV-CLI-001 | accepted | L0 | `cli/spec.md` "Default port and host" | no probe (Stage-5b TODO; see `proposal.md` risks) |
| SRV-CLI-002 | verified | L0 | `cli/spec.md` "Numeric port for `-l`/`--listen`" | ORC-001 |
| SRV-CLI-007 | verified | L0 | `cli/spec.md` "Directory positional argument" | ORC-001 (sc.1–2), ORC-062 (sc.3) |
| SRV-CLI-019 | verified | L0 | `cli/spec.md` "`--help` and `-v`/`--version` exit cleanly" | ORC-060, ORC-061 |
| SRV-FILE-001 | verified | L0 | `static-files/spec.md` "Serve regular files for matching paths" | ORC-001 |
| SRV-FILE-002 | verified | L0 | `static-files/spec.md` "404 for missing paths" | ORC-004, ORC-005 |
| SRV-FILE-004 | verified | L0 | `static-files/spec.md` "Default MIME types" | ORC-003 |
| SRV-FILE-005 | verified | L0 | `static-files/spec.md` "`index.html` resolution for directory paths" | ORC-001 |

This change carries `proposal.md`, `design.md`, `tasks.md`, and a single
**traceability-only** spec delta (`specs/cli/spec.md`) that MODIFIES
SRV-CLI-001's Requirement to add an `Implementation:` line pointing at
this change. The MODIFIED preserves observable behavior verbatim — only
metadata is added. Rationale: OpenSpec's validator (`openspec validate
--strict`) requires every change to carry at least one delta; a
traceability-only MODIFIED is the minimum-disruption way to satisfy
that constraint while keeping the Stage 5a artifact a design-first
proposal. The Stage-5b implementation change (`002-…`) will reference
the same eight SRVs and add Rust artifacts.

## Scope

### In scope

- Workspace skeleton design (two crates: `irserve` bin, `irserve-core` lib).
- HTTP-stack and dependency choices fixed in `design.md` with version
  pins.
- Request-lifecycle skeleton matching SRV-ROUT-006 even though only the
  static-file branch is wired in this slice.
- CLI surface for the L0 flags (positional dir, `-l`/`--listen`
  numeric, `-h`/`--help`, `-v`/`--version`, plus `-n`/`--no-clipboard`
  accepted as a no-op per the D-005 invariant).
- Anchor-level mapping of each in-scope SRV to existing probe cases
  under `tools/probe/cases/`, partitioned into L0-clean and
  L1-divergent. Stage-5b's harness adapts the runner (does not run
  unmodified — see `design.md` § 6 and `tasks.md` § 6.2 for the
  explicit adapter requirements).
- D-008 entry in `docs/reference/serve/decisions.md` (release-scoping).
- One traceability-only MODIFIED delta in `specs/cli/spec.md` that adds
  an `Implementation:` line to SRV-CLI-001's Requirement; observable
  behavior is preserved verbatim (see `design.md` § 8 for the rationale).

### Out of scope (explicit non-goals)

The 27 SRVs deferred by D-008 are **explicitly out of scope** for this
slice. Each name pairs with the SRV it defers and the change that is
expected to deliver it (numbering tentative, sequencing decided per
stage):

- `serve.json` loading and the schema map — SRV-CFG-001, SRV-CFG-002.
- Custom `<status>.html` error pages — SRV-FILE-003.
- `cleanUrls` 301 / extensionless resolution — SRV-ROUT-001, SRV-ROUT-002.
- `trailingSlash` add / strip — SRV-ROUT-003, SRV-ROUT-004.
- Multi-slash normalization — SRV-ROUT-005.
- Operation precedence wiring beyond the static-file terminal stage —
  SRV-ROUT-006.
- Configured `redirects` — SRV-RDIR-001, SRV-RDIR-002, SRV-RDIR-003.
- Configured `rewrites` — SRV-RWRT-001.
- Directory listing (HTML/JSON), `unlisted`, `renderSingle` — SRV-DLST-001,
  SRV-DLST-002, SRV-DLST-003.
- CLI flags beyond strict L0 — SRV-CLI-003 (`tcp://` URI), SRV-CLI-006
  (`-p` alias), SRV-CLI-008 (`--single`), SRV-CLI-009 (`--config`),
  SRV-CLI-010 (`--cors`), SRV-CLI-014 (`--debug`), SRV-CLI-015
  (`--no-request-logging`), SRV-CLI-016 (`--no-port-switching`).
  *Exception:* SRV-CLI-011 (`--no-clipboard`) is **accepted as a
  no-op** at L0 because the D-005 invariant ("IrServe MUST accept the
  flag without error") is unconditional — see `design.md` § 5
  "D-005 exception". Probe coverage of SRV-CLI-011 stays deferred.
- All L3 SRVs (custom `headers`, `http-cache`/ETag/Last-Modified/Range,
  CORS response surface, compression, MIME fallback for rewrites). Already
  excluded from the bootstrap and stay excluded.
- All L4 SRVs (UDS, Windows pipe, symlinks, TLS, Windows path quirks).
- No Rust code, no `Cargo.toml`, no `tests/` artifacts in this stage; they
  arrive in Stage 5b.
- No changes under `tools/probe/` or `third_party/`.

## Risks and mitigations

1. **`tower-http::ServeDir` short-circuits the request lifecycle.** Using
   it as the L0 engine would force a rewrite at L1/L2 because
   `cleanUrls`, `trailingSlash`, `redirects`, and `rewrites` (per
   SRV-ROUT-006) must run **before** static-file resolution.
   *Mitigation:* `design.md` mandates a custom dispatcher from the start,
   even though only the static-file terminal stage is wired in L0. The
   pipeline shape (numbered phases) is laid out so future stages fill in
   no-op slots.

2. **`mime_guess` defaults differ from `serve`'s `mime-types` package.**
   FILE-004 mandates `; charset=utf-8` on text MIME types; `mime_guess`
   does not emit that suffix by default.
   *Mitigation:* `design.md` requires a hand-rolled MIME table for the
   eight bindings explicitly listed in `static-files/spec.md` "Default
   MIME types"; `mime_guess` is fallback for the long tail (Q-004 stays
   open).

3. **Default-port path (SRV-CLI-001) has no probe.** Every existing probe
   passes `--listen <port>` explicitly, so the default-port code path is
   not exercised by the oracle harness. `cli/spec.md` notes this gap.
   *Mitigation:* Stage 5b adds a probe case (working name
   `default-port.json`) that omits `--listen` to promote SRV-CLI-001 from
   `accepted` to `verified`. Recorded as a `tasks.md` § 5b-prep follow-up;
   not blocking for change 001 acceptance.

## Open assumptions

The following Q-NNN entries in `docs/reference/serve/open-questions.md`
remain `open` after Stage 5a; the design touches their area but cannot
close them without a probe (per anti-hallucination rule #3 — Q-NNN close
on oracle measurements only):

- **Q-001** (`tcp://` default port/host) — SRV-CLI-003 is L1 and out of
  scope; Q-001 is irrelevant to this slice.
- **Q-002** (compression set / threshold) — D-006 already defers
  compression; no L0 impact.
- **Q-004** (MIME bindings beyond the probed set) — `design.md` pins the
  probed set as the contract and treats the remainder as best-effort via
  `mime_guess`. Q-004 stays open; closing it requires extending
  `tools/probe/cases/mime-defaults.json` with more extensions.

The Stage 5a contract therefore explicitly assumes:

- `tcp://` URIs are rejected by the strict-L0 CLI (consistent with D-008).
- Compression is absent and `Vary: Accept-Encoding` is not emitted (per
  D-006).
- MIME bindings outside the eight in `static-files/spec.md` "Default MIME
  types" follow `mime_guess`'s defaults; clients depending on the long
  tail SHOULD treat this as best-effort.
