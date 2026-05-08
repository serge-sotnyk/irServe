# Design: Bootstrap baseline authoring methodology

This is a *propose-only* change: it adds Requirements to currently-empty
capability namespaces. There is no implementation, no Rust, no test
harness. The "design" here is the authoring methodology itself.

## Authoring methodology

### Source of truth

`docs/reference/serve/inventory.md` is the authoritative source for
scenario bodies. Each SRV entry already has GIVEN / WHEN / THEN
scenarios; these are transcribed into the Requirement scenarios with
minimal edits (only enough to remove implementation-language references
to `serve`'s Node source: line numbers, function names, JS module paths).

`docs/reference/serve/oracle-matrix.md` is the authoritative source for
ORC IDs cited in `Evidence:` lines. Each ORC links to a committed probe
snapshot under `tools/probe/snapshots/` — the wire-level evidence that
backs the requirement.

`docs/reference/serve/decisions.md` (D-001 through D-007) governs
intentional deviations. Any SRV with `Status: adapted` carries a
`Note:` line pointing at the decision that justifies it.

### SRV-to-Requirement mapping

One Requirement block per SRV. The block follows the OpenSpec delta
format:

```markdown
### Requirement: <Title transcribed from the SRV one-liner>
The system SHALL <observable behavior, no implementation language>.

Evidence: SRV-<AREA>-<NNN> (status: <status>, level: L<N>); oracle: ORC-<NNN>[, ORC-<NNN>...].

Note: <only present for `accepted`/`adapted` SRVs to flag gaps or decisions>

#### Scenario: <name>
- GIVEN <preconditions>
- WHEN <stimulus>
- THEN <observable outcome>
```

### Cross-capability requirements

SRV-ROUT-006 (operation precedence) cites every interacting capability
in its scenario evidence (cleanUrls, trailingSlash, redirects,
rewrites, static files, the `--single` SPA fallback). It is placed in
`routing` because that is the capability it most centrally describes;
its `Evidence:` line cross-references SRV-ROUT-001..005, SRV-RDIR-001,
SRV-RWRT-001, and SRV-CLI-008 implicitly via inventory.

## Compatibility-level cutoff

The MVP target per `README.md` is L2; L3 is a stretch goal; L4 is
explicitly out of scope. `compatibility-levels.md` states: "Stage 4
(OpenSpec bootstrap change) only includes requirements up to the agreed
compatibility level."

This change therefore admits SRVs at L0, L1, and L2 only. L3-verified
SRVs (ETag, Range, Cache-Control, custom headers, CORS surface,
compression, ETag override, MIME fallback for rewrites) are
intentionally excluded even though they have probe coverage. They will
be added by a separate change once an implementation proposal targets
L3.

## Status taxonomy in scope

| Status | In this change? | Rationale |
|---|---|---|
| `verified` | yes | Has at least one passing oracle snapshot. The strongest evidence. |
| `accepted` | yes | Confirmed as a target; admitted on README/source authority where probes are not feasible (e.g. `--no-clipboard` is unobservable; `--no-port-switching` happy-path-only). |
| `adapted` | yes | Intentional deviation tracked in `decisions.md`. The Requirement reflects IrServe's adapted behavior, not `serve`'s. |
| `deferred` | no | Postponed to L4 / post-MVP. |
| `unknown` | no | Behavior unclear; tracked in `open-questions.md`. |
| `rejected` | no | Will not be supported (currently zero in inventory). |

## Evidence gaps

Status `accepted` SRVs without probe evidence are admitted because
README/source authority is sufficient for the requirement to be a
target. Each Requirement using one of these SRVs carries a `Note:` line
that names the gap so reviewers can audit it from inside the spec.

| SRV | Why admitted without a probe |
|---|---|
| SRV-CLI-001 | Source-level evidence in `serve/source/main.ts` is the default port `3000`. Every probe passes `--listen` so the default-port path is not exercised; promoting requires a probe that omits `--listen`. |
| SRV-CLI-003 | TCP-URI parse is transitive over SRV-CLI-002 (numeric form). No HTTP-level divergence vs. the bare-port form. |
| SRV-CLI-006 | `-p` deprecated alias is transitive over `-l`/`--listen`. |
| SRV-CLI-011 | Clipboard suppression is unobservable via HTTP, and per D-005 IrServe never modifies the clipboard. The flag is accepted as a no-op for CLI compatibility. |
| SRV-CLI-016 | Happy-path is exercised by every probe; the contractual case (refuse to fall back when the port is occupied) requires a probe that occupies the port first. |
| SRV-CFG-002 | Meta-requirement: schema map. Verified piecewise via the per-field SRVs (cleanUrls, trailingSlash, redirects, etc.). |
| SRV-RDIR-003 | External-URL redirects need a probe with an absolute-URL destination; Q-007 still open. |

## Adaptation references

The bootstrap consumes the following decisions verbatim:

- **D-002** — exact terminal output / stdout formatting is not part of
  the contract. Affects SRV-CLI-014 (`--debug`) and SRV-CLI-015
  (`--no-request-logging`); both are `adapted`.
- **D-003** — exact HTML/CSS markup of the directory listing and error
  pages is not in scope. Affects SRV-FILE-002 (HTML branch) and
  SRV-DLST-001 (HTML branch). The Requirement bodies assert status,
  `Content-Type`, and JSON shape only.
- **D-005** — clipboard side-effect is not implemented. Affects
  SRV-CLI-011; flag is accepted, no observable behavior beyond
  acceptance.
- **D-007** — sanitized JSON directory listing. Affects SRV-DLST-001:
  the `dir` field is rendered relative to the served root, diverging
  from `serve`'s absolute-path leak.

D-001, D-004, and D-006 are project-level rejections / adaptations
that are honored implicitly by the scope choices in `proposal.md` and
do not need per-Requirement annotations.

## Open questions still open at bootstrap

The following research questions remain `open` after Stage 3 and do not
gate this change. They are listed for traceability:

- Q-001 — default port/host on `tcp://` URI without explicit values.
- Q-002 — exact set of compressed content types and minimum body size.
- Q-003 — schema validation error format and exit codes.
- Q-004 — MIME-type bindings beyond the probed set.
- Q-007 — external-URL redirect destinations (relative vs. scheme-relative).
- Q-009 — `If-Modified-Since` handling under `--no-etag`.
- Q-011 — Windows symlink/junction parity.

Resolution of any of these is a research follow-up; once resolved, the
relevant SRV is updated in `inventory.md` and a follow-up change
amends the bootstrap. None of them block Stage 4.

## Validation strategy

Validation runs locally before the diff goes to review:

```text
npx @fission-ai/openspec validate --all --strict --concurrency 12
```

The OpenSpec CLI is not committed as a dependency (no root
`package.json`). The install pointer is documented in
`openspec/AGENTS.md`.

Beyond `openspec validate`, three manual cross-checks are performed:

1. Every `Evidence:` line cites an SRV ID present in
   `docs/reference/serve/inventory.md`.
2. Every ORC ID cited in an `Evidence:` line resolves to a row in
   `docs/reference/serve/oracle-matrix.md`.
3. No SRV cited has status `deferred`, `unknown`, or `rejected`, and
   no SRV cited is at level L3 or L4.

These checks are listed as concrete tasks in `tasks.md`.

## Archive plan

This change is intentionally NOT archived in Stage 4. Archiving (the
step that merges deltas from `openspec/changes/000-…/specs/` into
`openspec/specs/<capability>/spec.md`) is a separate user-initiated
step performed after acceptance. Until that step:

- `openspec/specs/` contains only `.gitkeep` files (state unchanged
  from Stage 0).
- `openspec/changes/000-establish-serve-compatibility-baseline/` is
  the live source of truth for these requirements.

Reviewers should not expect populated `openspec/specs/` after this
change lands.
