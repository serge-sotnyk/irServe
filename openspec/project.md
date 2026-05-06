# Project context

## Project

IrServe is a Rust CLI static file server inspired by `vercel/serve`. The repository hosts an experiment in AI-assisted porting methodology; the binary is a side product, the methodology is the deliverable.

## Reference implementation

`npm serve` and `serve-handler` (`third_party/serve`, `third_party/serve-handler`) are used as the **reference oracle**. Specs describe IrServe behavior, not `serve` source. Where IrServe deviates intentionally, the deviation is logged in `docs/reference/serve/decisions.md`.

## Goal

Behavior-compatible with `serve` where practical, **not** source-compatible. Node.js middleware API (`serve-handler` as embeddable library) is **out of scope for MVP**.

## Compatibility scope

See `../docs/reference/serve/compatibility-levels.md`. Target for the experiment: Level 2 or Level 3.

## Spec authoring rules

- Use behavior-first requirements; do not describe Rust types or call sites.
- Every scenario uses GIVEN / WHEN / THEN.
- Mark intentional deviations from `serve` explicitly (status `adapted`, plus an entry in `decisions.md`).
- Do not put implementation details in `spec.md`. Implementation choices live in `design.md` of the corresponding change.

## Design rules

- Explain compatibility tradeoffs explicitly.
- Separate Rust architecture decisions from reference behavior.
- Reference the oracle test that backs each behavior claim, when one exists.

## Stage map

The methodology runs in eight stages (0–7). Current overall status is in the root `AGENTS.md`. OpenSpec only owns Stages 4+ (the bootstrap change and subsequent implementation proposals).
