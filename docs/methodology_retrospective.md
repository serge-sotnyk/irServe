# irServe methodology retrospective

A post-mortem of the AI-assisted porting experiment captured in this repo. The methodology — not the binary — was always the deliverable; this document records what worked, what cost more than expected, and what to carry into the next port.

## Outcome

- **Binary**: `irserve` v0.1.0, MVP target (L2) shipped, L3 stretch goal also shipped. Verified end-to-end by 81 oracle probes against the pinned `vercel/serve` reference plus 362 unit tests. L4 (symlinks, TLS, Windows path quirks) deferred per `compatibility-levels.md`.
- **Contract**: 11 OpenSpec capability specs in `openspec/specs/`; 16 archived change packages in `openspec/changes/archive/` (000 baseline + 001-015 implementation deltas).
- **Methodology evidence**: 52-entry SRV inventory, 213-row oracle matrix, 21 `D-NNN` decisions, 13 `Q-NNN` open-question entries (12 closed, 1 deferred).

The experiment shipped its target. The cost and shape of the shipping are the more interesting story.

## Anti-hallucination rules: per-rule verdict

Rules are stated in `README.md` §"Anti-hallucination rules" (numbered 1-10). Verdict labels:

- **worked** — the rule prevented a class of error that would otherwise have landed.
- **saved-tokens** — the rule shortened a stage by avoiding a known failure mode.
- **fired-late** — the rule was right but discovered after the failure, not before.
- **under-specified** — the rule existed but was too vague to enforce; tightened mid-experiment.

| # | Rule | Verdict | Citation |
|---|---|---|---|
| 1 | Cite evidence | worked | Status taxonomy in `inventory.md` (e.g. SRV-CACHE-003 `verified` cites probe + line-numbers) is auditable end-to-end. |
| 2 | Status taxonomy | worked | Without `adapted`/`deferred`/`verified` distinction, D-018 / D-019 / D-020 would have been silent divergences. |
| 3 | Do not invent | worked | All 13 `Q-NNN` entries had to be probe-closed before behavior was claimed. Stage 7b held off on IMS handling until Q-009 was probe-closed in slice 0. |
| 4 | README < tests < source < oracle | worked | Q-007 (redirect destination forms) and Q-009 (IMS) both resolved against probe snapshots, not reading. |
| 5 | MVP non-goals (D-001..D-004) | worked | Repeated push to mirror exact terminal output / HTML markup was rebuffed; saved an estimated 2-3 stages of bikeshedding. |
| 6 | Verify against pinned reference | worked | `npm install -g serve` was excluded as evidence early; saved the class-of-bug where global vs vendored serve diverge silently. |
| 7 | Stage discipline (inventory → specs → impl) | worked | Stage 5b couldn't start until 5a's proposal was signed off. The temptation to skip 5a for the "obvious" L0 surface was resisted. |
| 8 | Empirical-before-implement | **fired-late** | Authored *after* Stage 6d had run 15 review rounds on path-to-regexp + minimatch quirks. Subsequent stages (6e, 7e) front-loaded the probes; cost dropped. Worth promoting to rule #1 in the next port. |
| 9 | Stop after 3+ rounds on same aspect | **fired-late** | Same Stage-6d incident. The parity-scope-declaration pattern that emerged (codified in D-019 for ranges, D-020 for compression) is the load-bearing artifact. |
| 10 | Pre-stage out-of-scope list | **fired-late** | Authored after Stage 7e plan-mode learned that compression-middleware's defaults were under-documented. Stages without explicit out-of-scope lists were systematically more expensive. |

The three "fired-late" rules (8, 9, 10) account for an outsized share of the total review-round budget. They are corollaries of the same root cause: **third-party libraries under-document their defaults**, and reading source plus README is not sufficient. Mirroring requires probing.

## Cost shape

Numbers are informal (estimated from session experience, not a persisted token log):

| Stage cluster | Sessions | Notes |
|---|---|---|
| Stages 1-4 (research, specs bootstrap) | ~6-8 | Largely linear. Stage 3 oracle-matrix authoring was the densest. |
| Stage 5a + 5b (first proposal + first slice) | ~3 | 5a took longer than expected because the 13-phase pipeline architecture had to be settled. |
| Stages 6a-6h (L2 build) | ~15 | Stage 6d alone consumed ~3 sessions across 15 Codex review rounds. The other seven sub-stages averaged 1.5 sessions each. |
| Stages 7a-7e (L3 build) | ~10 | Stage 7e was 11 rounds; 7b was 7 rounds; 7a/7c/7d converged in 2-4 rounds each. |

**Codex review rounds per stage** (full distribution, from commit log):

```
15  stage-6d (configured redirects)        -- path-to-regexp + minimatch quirks
11  stage-7e (compression)                  -- compression@1.8.1 undocumented defaults
 7  stage-7b (last-modified + IMS)          -- D-018 adaptation scope
 5  stage-6f (error pages + security + headers)
 4  stage-5a, 6c, 7a, 7c
 3  stage-3, 6b, 6g, 6h
 1-2 stages-1, 2, 4, 5b, 6a, 6e, 7d
```

The bimodal distribution — most stages converge in 1-4 rounds, two outliers (6d, 7e) double-digit — is the single most important methodology signal. Both outliers share the same root cause: mirroring a non-trivial third-party library. The pre-stage out-of-scope list (rule #10) and probe-first (rule #8), when applied honestly, prevent the outlier shape.

## Subagent delegation: what worked

Two delegation patterns paid off, each saving ~10-15k tokens per stage:

1. **Spec prose after green slices.** Once implementation slices were committed, writing the OpenSpec change package (proposal / design / tasks / spec delta) was handed off to a fresh subagent with a structured brief — slice plan + commit log + relevant `D-NNN` entries + a peer change package to mirror in style. The main agent reviewed and edited only when needed. The subagent did not require the full implementation conversation in context.
2. **Cross-cutting reference reconnaissance in plan-mode.** When a stage mirrored 2+ functions of a third-party library, a plan-mode `Explore` agent was tasked with returning the **verbatim code** of every relevant branch with line numbers — not "where it lives". Re-using one transcript across implementation + review rounds avoided re-opening the same source 3-4 times. Empirical surprises (e.g. "`sendError` skips `getHeaders` for JSON errors") landed in plan-mode rather than in review round 2.

The main agent retained: architectural forks, `D-NNN` decisions, methodology signals, Codex review reasoning. Single-file edits and lookups recoverable via `git log` were never worth delegating.

## Process changes worth keeping for the next port

Five practices to carry forward:

1. **Probe-first when mirroring a library** (rule 8). Author 5-10 edge-case probes against the library *before* writing the Rust equivalent. Documentation under-reports quirks; defaults change between versions; platform-specific branches surprise readers. The cost is ~1 session up-front; the savings are the difference between a 4-round and a 15-round stage.
2. **Mandatory pre-stage out-of-scope list** (rule 10). Each stage plan must enumerate concrete quirks that, if encountered, will land as `D-NNN` known divergences. Discovering scope-creep through review rounds is a signal that this list was missing or under-specified.
3. **Pause-after-3-rounds + parity-scope declaration** (rule 9). When reviewer findings keep landing on the same aspect for three rounds running, do not start the fourth fix. Pause and declare in `D-NNN` + spec compatibility note what is and is not in-scope. D-019 (range multi-segment) and D-020 (compression bit-level divergence) were both successful exits from this pattern.
4. **Status taxonomy as a load-bearing artifact**. `candidate / accepted / verified / adapted / deferred / rejected / unknown` is more than bookkeeping. It is what makes silent divergence impossible — every `adapted` entry has a `D-NNN`; every `unknown` lives in `open-questions.md` until probed. Don't let a status field default to "true".
5. **Stage discipline through OpenSpec change packages**. Each green stage producing its own change package (proposal + design + tasks + spec delta) forced the implementation to be *finished* in a way that "feature branch merged" does not. The archive directory is the audit trail.

## What did NOT work / overhead

- **Reading source as a substitute for probing.** Stage 6d ran 15 review rounds because `path-to-regexp`'s default `i` flag and `minimatch`'s Windows-only path-sep replacement were not visible from reading the libraries' source plus their READMEs. Both surfaced via Codex review on irserve output that diverged. Probe-first (rule #8, written *after* this stage) would have surfaced them in plan-mode for ~1 session of cost instead of 3.
- **Compression middleware parity.** Stage 7e ran 11 rounds because `compression@1.8.1`'s defaults (threshold, MIME table, q-rank behavior, framing) had to be re-discovered by raw-socket probing. Q-002 closure in slice 0 — explicit pre-stage probing — was the right move; the remaining 11 rounds were architectural-seam corrections (move compression past dispatch; already-encoded gate; 206 threshold).
- **Stage 6d cleanup**. The three "fired-late" rules were not authored until Stage 6d had already burned its budget. The rules then became cheap to apply but expensive to discover. Future projects should adopt rules 8/9/10 *at the start*, not as midstream patches.
- **Codex review rounds as a unit of process**. They are not free: each round is one commit, one re-read of the diff, one cycle of reviewer + author state. Stages with 5+ rounds should be a signal to pause the stage and re-scope, not to keep iterating. The Stage 6d "we are close, one more round" trap is real.

## Closing

The methodology is reproducible. The next port should:

1. Author rules 8, 9, 10 as part of the kickoff template.
2. Front-load probes whenever the target mirrors a third-party library.
3. Track Codex round count as a stage health metric, with a soft cap that triggers re-scoping.
4. Keep the status taxonomy load-bearing — never let `candidate` ship to `verified` without an artifact.

The L4 surface (symlinks, TLS, Windows quirks) remains an option for a future iteration; the methodology cost of adding it is now estimable: ~3-5 sessions per cluster, weighted by how much Q-011 surface lands in plan-mode versus review.
