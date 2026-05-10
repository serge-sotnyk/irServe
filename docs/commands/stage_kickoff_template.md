# Stage kickoff template

Reusable prompt for starting a new sub-stage (currently 6a–6h, Stage 6).
Replace the `{6X — short name}` placeholder with the sub-stage being
planned (e.g. `6a — serve.json loader`); everything else is reusable
verbatim. Keep the prompt terse — the architectural foundations and
roadmap details live in the canonical sources cited below; do not
duplicate them here.

---

Let's plan stage {6X — short name}.

## Canonical sources
- The document `irServeChat.pdf` contains a preliminary discussion of the project 
  idea — what we are building, how, and why. Some details are already a bit outdated, 
  but it still provides a high-level overview.
- Stage 6 roadmap: `docs/stage6_l1_l2_capabilities.md` — SRVs,
  dependencies, what each sub-stage closes. Verify {6X} is "Next" per
  the README and the roadmap.
- Architectural foundations:
  `openspec/changes/001-port-minimal-static-server/design.md`
  (§4 — the 13-phase pipeline, §6 — the oracle harness layer).
- Contract for the capability: `openspec/specs/<capability>/spec.md`
  (mapping in the roadmap).
- Reference notes:
  `docs/reference/serve/{inventory,oracle-matrix,decisions,open-questions}.md`.
  `decisions.md` is a live document — append `D-NNN` entries as
  divergences surface.
- Existing probe cases: `tools/probe/cases/<capability>-*.json` and
  their snapshots.
- Reference oracle: `third_party/serve`, `third_party/serve-handler`.
  Do not modify.

## Goal

A change package `openspec/changes/00N-<name>/` (proposal + design +
tasks + spec delta if needed) plus working code that passes the oracle
harness against the reference for the capability's anchors. The
implementation plan goes into a separate file
`docs/features/000N_PLAN_stage6X_<short_name>.md` per the existing
0001..0006 convention.

## Process

1. **Plan-mode.** Launch Explore agents in parallel (max 3) to scout
   `serve-handler` source and existing probe cases. For
   **cross-cutting stages** (2+ SRVs that mirror functions in the
   same reference family — e.g. Stage 6f touched `sendError` +
   `getHeaders` + `sourceMatches` + `findRelated`), one of the
   agents MUST return the verbatim code of every relevant branch
   with line numbers, not just "where it lives." Empirical
   surprises about reference behavior (e.g. "this branch skips that
   helper for JSON-accepting clients") belong in the plan-mode
   transcript so they don't get rediscovered in implementation or
   Codex review rounds — each rediscovery costs another full read
   of the source file. High-level interview via `AskUserQuestion`
   (1–4 questions, with one `(Recommended)` option listed first).
   The user often answers "your choice" — decide small technical
   points yourself.
2. **Implementation.** Iterative green-state commits. After each
   green slice, ask before running `git commit`. Delegate to
   subagents:
   - mechanical scaffolding (a new module with a pre-fixed signature);
   - `tools/probe/run.mjs` adapter changes and probe case JSON edits;
   - recording new snapshots via
     `--snapshot=update --target=reference`;
   - **slice-6 (meta) spec prose** — after the implementation
     slices commit, delegate writing of `proposal.md` / `design.md`
     / `tasks.md` / MOD spec deltas to a subagent with a structured
     briefing: slice plan + commit log + relevant `D-NNN` entries +
     a peer change package to mirror in style. The main agent
     reviews and Edits if needed. The subagent does not need the
     full implementation conversation in context.
   The main agent retains: architectural forks, D-NNN decisions,
   and interpretation of methodological signals.
3. **Review.** The user hands the diff to Codex. Each round of fixes
   lands as a separate commit titled
   `docs(stage-6X): address Codex review round N (P{priorities} fixes)`.
   Push back if you disagree — pushback is expected.

## Hard stops

- Do not modify `third_party/`.
- Existing snapshots are touched only if reference behavior actually
  changed; that is a methodological signal, discuss before patching.
- The contract changes only through a `MODIFIED` delta or a `D-NNN`
  entry after explicit discussion. `Q-NNN` entries close on probe
  measurements only, not on code.
- Do not commit or push without explicit permission. But propose it 
  when you see that the current portion of code is ready to merge.

## Context hygiene

- Read large files (`run.mjs`, snapshots) selectively via `Grep` plus
  `Read` with `offset`/`limit`. Avoid reading them whole.

## Start

Begin with reconnaissance and confirm {6X} is "Next" per the README
and the roadmap.
