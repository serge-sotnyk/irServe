# Proposal: Routing normalization (multi-slash + trailingSlash)

## Why

Stage 6a shipped the `serve.json` loader (`003-load-serve-json`):
`ServeConfig` is parsed and stored, but only the `public` field is
applied to request handling. The remaining schema-map fields stay
parsed-but-unused, deferred per D-008. The next un-defer slice in the
L1/L2 roadmap (`docs/stage6_l1_l2_capabilities.md:50`) is routing
normalization — phases 3 and 5 of the 13-phase dispatcher.

This change wires:

- **Phase 3** — silent multi-slash collapse (SRV-ROUT-005, Q-006
  closed). `/a//b` → `/a/b` before any routing decision; the collapse
  itself never emits a 301.
- **Phase 5** — `trailingSlash` 301 add/strip (SRV-ROUT-003,
  SRV-ROUT-004). `Some(true)` adds `/` to extensionless non-dotfile
  paths; `Some(false)` strips `/` (root `/` exempt because the
  stripped target would be empty); `None` is a no-op.

Two new probes (`trailingslash-add`, `trailingslash-strip`) isolate
the trailingSlash redirect from `cleanUrls` (which defaults to on in
the reference but is explicitly disabled in the new fixtures). The
existing wire-level `multislash-collapse` probe is enabled for
`target=irserve` via a `runner.l0` block whose `clean` set is the pure
collapse anchor (`double_slash_root`); the two cleanUrls-dependent
anchors stay marked `divergent` until 6c lands phase 4.

## What

| SRV | Status before | Status after | Module |
|---|---|---|---|
| SRV-ROUT-003 | verified (deferred from IrServe per D-008) | verified, un-deferred from D-008 (oracle list grows: ORC-068, ORC-069) | `crates/irserve-core/src/trailing_slash.rs`; dispatcher hookup in `dispatch.rs` |
| SRV-ROUT-004 | verified (deferred from IrServe per D-008) | verified, un-deferred from D-008 (oracle list grows: ORC-070, ORC-071) | `crates/irserve-core/src/trailing_slash.rs`; dispatcher hookup in `dispatch.rs` |
| SRV-ROUT-005 | verified (deferred from IrServe per D-008) | verified, un-deferred from D-008 (no oracle list change — `multislash-collapse` `runner.l0` adds the `clean` partition for `target=irserve`) | `crates/irserve-core/src/normalize.rs`; dispatcher hookup in `dispatch.rs` |

The change carries one MODIFIED delta on `specs/routing/spec.md` for
SRV-ROUT-003 / SRV-ROUT-004, extending each requirement's `Evidence:`
oracle list with the new ORC IDs and adding an `Implementation:`
paragraph pointing at the `trailing_slash` module. The behavioral text
and the Scenarios are preserved verbatim. SRV-ROUT-005's evidence list
is unchanged (the new probe wiring is `runner.l0`-level, not a new
ORC). SRV-ROUT-006 (precedence) stays deferred — phases 4, 6, 7, 8 are
not yet implemented.

## Scope

### In scope

- `crates/irserve-core/src/normalize.rs` — `collapse_slashes(path: &str)
  -> Cow<'_, str>` returning `Cow::Borrowed` on the no-`//` happy path
  (zero-allocation). 10 unit tests cover empty / single slash / leading
  / internal / trailing / mixed runs / percent-encoded slashes.
- `crates/irserve-core/src/trailing_slash.rs` —
  `compute_trailing_slash_redirect(path, cfg) -> Option<String>`. 14
  unit tests cover add / strip / no-op / dotfile / extension /
  dotfile-with-extension / root-edge / nested.
- `crates/irserve-core/src/dispatch.rs` — `dispatch()` signature gains
  `&ServeConfig`; phases 3 and 5 hooked in. Explicit comment-stubs
  mark phases 4, 6, 7, 8 for the upcoming sub-stages so the 6c
  insertion is a one-line addition. `redirect_301(target)` helper
  builds the 301 response with `Location` set verbatim.
- `crates/irserve-core/src/server.rs` — `AppState` carries both `root`
  and `serve_config`; `handler` propagates both into `dispatch`.
- `crates/irserve-core/src/lib.rs` — registers `mod normalize` and
  `mod trailing_slash`.
- `tools/probe/cases/multislash-collapse.json` — adds `runner.l0`
  block: `clean: ["double_slash_root"]`, `divergent:
  ["double_slash_segment", "internal_double_slash"]`.
- New probe `tools/probe/cases/trailingslash-add.json` (HTTP-mode):
  `serve.json: {trailingSlash: true, cleanUrls: false}`, fixture has
  `index.html` and `data.txt`. `GET /about` → 301 `/about/` (ORC-068);
  `GET /data.txt` → 200 (ORC-069).
  `runner.l0.contentLengthMayDiffer` masks the redirect anchor's
  empty-body length.
- New probe `tools/probe/cases/trailingslash-strip.json` (HTTP-mode):
  `serve.json: {trailingSlash: false, cleanUrls: false}`, fixture has
  `index.html` and `about.html`. `GET /about/` → 301 `/about`
  (ORC-070); `GET /about.html` → 200 (ORC-071).
  `runner.l0.contentLengthMayDiffer` masks the redirect anchor.
- Research-track edits: ORC-068/069/070/071 rows in `oracle-matrix.md`;
  SRV-ROUT-003/004 oracle lists extended in `inventory.md`; D-010
  added in `decisions.md` amending D-008's deferred set (drops
  SRV-ROUT-003, SRV-ROUT-004, SRV-ROUT-005).
- README stage-map row 6b → done; "Try IrServe" section refreshed.

### Out of scope

- Phase 4 (`cleanUrls` 301) and phase 8 (`cleanUrls` resolution):
  Stage 6c. Compose probes (`prec-cleanurls-trailing*`) and the
  cleanUrls-dependent anchors of `multislash-collapse` stay
  reference-only until then.
- Phase 6 (configured redirects): Stage 6d.
- Phase 7 (rewrites + `--single`): Stage 6e.
- SRV-ROUT-006 (operation precedence): full pipeline is not yet
  observable; stays deferred to the sub-stage that lands the last
  remaining phase (6e).
- URL percent-decoding: phase 3 currently operates on the raw URI
  path (`req.uri().path()`). Encoded `%2F%2F` is left intact and
  does NOT collapse. No probe asserts on encoded slashes against
  irserve in 6b; this is tracked as an open question for 6c+ where
  decoding becomes load-bearing for cleanUrls's `.html` matching.
- Bug-for-bug content-length parity on 301 responses. axum/hyper
  emits `content-length: 0` on the empty body; the reference (Node
  http) omits it. Both responses have identical body bytes (zero).
  The new probes use `runner.l0.contentLengthMayDiffer` to mask the
  header on both sides for the redirect anchors only.
- Any change to `third_party/`. No edits to existing snapshots —
  reference behavior is unchanged.

## Findings (methodological signals)

No spec amendments required. Two micro-divergences surfaced and were
absorbed without a new D-NNN:

1. **`content-length: 0` on irserve 301 responses.** axum/hyper
   automatically populates `content-length` for the empty 301 body;
   the reference's Node http response omits the header entirely. Both
   bodies are zero bytes; only the header's presence differs. Mediated
   via `runner.l0.contentLengthMayDiffer` on the redirect anchors of
   the two new probes. No contract change — the spec text never
   pinned the header's presence.
2. **Directory listing under `cleanUrls: false`.** The reference
   serves a directory listing for `GET /` and `GET /about/` when
   `cleanUrls` is off, while irserve's L0 directory→`index.html`
   resolution (SRV-FILE-002) serves `index.html` directly. The new
   probes deliberately route around this by avoiding directory
   anchors with `cleanUrls: false`; the divergence is not in 6b's
   scope and will be addressed when SRV-DLST-* lands in 6g.

## Risks and mitigations

1. **Encoded-slash semantics.** Phase 3 collapses `//` literally and
   does not decode `%2F%2F`. No 6b probe asserts on encoded slashes
   against irserve. If a future probe (6c) shows the reference
   collapses encoded forms via decode-then-normalize, we either move
   URL-decode upstream of phase 3 or log a Q-NNN entry. The current
   `traversal-encoded` and `traversal-raw-encoded` probes have no
   `runner.l0` block so they auto-skip under `target=irserve`.
2. **Root-edge for `trailingSlash: false` on `/`.** The reference's
   logic (`index.js:152`) would yield an empty target string, falsy
   in JS, producing no redirect. The Rust implementation
   short-circuits `path.len() <= 1` in the strip branch to mirror
   this. Unit-tested as `strip_root_is_noop`.
3. **Dotfile-with-extension behavior on the add branch.** Files like
   `.bashrc.bak` have a non-leading dot in the basename and so DO
   have an extension by `path.parse()` semantics; the add branch
   skips them. Unit-tested as `add_skips_dotfile_with_extension`.
4. **`L0_DEFERRED_FLAGS` interactions.** No new flags are introduced
   in 6b. The `trailingSlash` and `cleanUrls` config fields enter via
   `serve.json`, not via CLI flags, so the deferred-flag list is
   untouched. The two new probes use `serveJson` fixture entries
   already supported by the runner.

## Open assumptions

- **A1 — `cleanUrls` defaults to ON in the reference's `applicable`
  helper.** Verified in `serve-handler/src/index.js:256-274` —
  `applicable(path, undefined)` returns `true`. The new probes
  therefore explicitly set `cleanUrls: false` to isolate the
  trailingSlash redirect from the cleanUrls 301; without that,
  ORC-068 / ORC-070 would be cleanUrls-compose probes, contradicting
  the planning decision to defer compose to 6c.
- **A2 — axum/hyper auto-emits `content-length: 0` on empty 301.**
  Verified during probe verification (the diff against the reference
  surfaced exactly this header). Mediated via
  `contentLengthMayDiffer` rather than by stripping the header in
  irserve's response builder, since the header is correct (the body
  IS zero bytes); the divergence is purely the reference's choice
  to omit it.
