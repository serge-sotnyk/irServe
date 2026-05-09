# Decisions log

Intentional deviations from `vercel/serve` behavior (`adapted` status) and explicit non-goals (`rejected` status). Each entry must reference the affected requirement ID(s) and explain the rationale.

## Format

```
## D-<NNN>: <short title>

Date: YYYY-MM-DD
Affected requirements: SRV-<AREA>-<NNN>, ...
Status: adapted | rejected
Reason: <why we deviate>
Impact: <what users / oracle tests should expect>
```

## Entries

## D-001: No Node.js middleware API

Date: 2026-05-07
Affected requirements: (none — entire `serve-handler` API surface)
Status: rejected
Reason: IrServe is a CLI binary written in Rust. The Node-specific `handler(request, response, config, methods)` middleware API has no analog in a Rust HTTP server, and exposing an embeddable library is outside the MVP scope per anti-hallucination rule #5.
Impact: IrServe MUST NOT be used as a drop-in replacement for `serve-handler` in Node code paths. The configuration *file format* (`serve.json`) is in scope; the *programmatic API* is not.

## D-002: No exact terminal output / stdout formatting

Date: 2026-05-07
Affected requirements: SRV-CLI-014 (`-d`/`--debug`), SRV-CLI-015 (`-L`/`--no-request-logging`), SRV-CLI-016 (port-switching warning), SRV-CLI-019 (`--help`/`--version`)
Status: rejected
Reason: The `serve` CLI uses `chalk`, `boxen`, and a specific log format (date prefix, IP, status, ms-elapsed). Mirroring this byte-for-byte adds churn without functional value and is excluded by anti-hallucination rule #5.
Impact: Oracle tests MUST NOT compare stdout/stderr text. Verbosity flags (`-d`, `-L`) are accepted; their effect on logging is implementation-defined.

## D-003: No exact HTML/CSS markup of the directory listing or error pages

Date: 2026-05-07
Affected requirements: SRV-DLST-001, SRV-FILE-002 (HTML branch), SRV-FILE-003 (HTML branch)
Status: rejected
Reason: The directory listing and the default error page are styled HTML templates inside `serve-handler`. Their visual design is not a contractual interface; only the *existence*, status code, and `Content-Type` of the response are. (See anti-hallucination rule #5.)
Impact: Oracle tests MUST NOT compare HTML body bytes for listings or for the no-`<status>.html` error path. The JSON branches of both (which carry structured data) ARE in scope.

## D-004: No bug-for-bug parity with `serve`

Date: 2026-05-07
Affected requirements: (all)
Status: rejected
Reason: Anti-hallucination rule #5. Where `serve` exhibits a behavior that looks unintentional (e.g. the JSON listing leaking absolute filesystem paths in its `dir` field — see Q-008; or the multi-slash-collapse only firing when `trailingSlash` is set — see Q-006), IrServe is free to diverge with a documented adaptation.
Impact: Future SRVs that catch a quirk should record an `adapted` status here when IrServe chooses not to mirror it. This is a meta-decision; it does not by itself rule out matching `serve`.

## D-005: Clipboard side effect is not implemented

Date: 2026-05-07
Affected requirements: SRV-CLI-011 (`-n`/`--no-clipboard`)
Status: rejected
Reason: Writing the bound URL to the system clipboard on startup is environment-coupled (display server, OS-specific clipboard daemons), interferes with non-interactive use, and is not part of the HTTP contract.
Impact: IrServe MUST accept the `-n`/`--no-clipboard` flag without error so existing scripts keep working, but it has no observable effect because IrServe never touches the clipboard.

## D-006: HTTP compression is L3-priority, not MVP-mandatory

Date: 2026-05-07
Affected requirements: SRV-CLI-012 (`-u`/`--no-compression`)
Status: adapted
Reason: `serve` uses the `compression` connect/express middleware with default settings. Implementing equivalent behavior in Rust adds dependencies (`flate2`, content-type sniffing, threshold logic) that are not justified for an L0–L2 MVP. The `--no-compression` flag stays in the CLI.
Impact: Until L3 work begins, IrServe MAY ignore `--no-compression` and serve responses uncompressed regardless. The `Vary: Accept-Encoding` header MAY still be omitted in this early phase; oracle tests for compression are deferred until the implementation lands.

## D-007: Sanitized JSON directory listing

Date: 2026-05-07
Affected requirements: SRV-DLST-001
Status: adapted
Reason: `serve`'s JSON directory listing exposes a `dir` field that contains the absolute filesystem path of the listed directory on the host machine (verified by probe `listing-unlisted`; tracked as Q-008). That value leaks deployment topology — username, deployment root, container layout — to any client that requests `Accept: application/json` against a directory URL. The README does not specify the field, so this is an implementation accident upstream rather than a contractual surface; `D-004` (no bug-for-bug parity) authorizes the divergence.
Impact: IrServe SHALL preserve the JSON-listing content-negotiation behavior (`Accept: application/json` returns `application/json; charset=utf-8` with the `{"files":[...], "directory":..., "paths":...}` shape) but the `dir` field — and any other field carrying a host-absolute path — MUST be rendered relative to the served root (e.g. `"."` for the root itself, `"sub"` for a `sub/` subdirectory). Closes Q-008.

## D-008: First-slice strict-L0 cutoff

Date: 2026-05-08
Affected requirements: SRV-CFG-001, SRV-CFG-002, SRV-FILE-003, SRV-ROUT-001, SRV-ROUT-002, SRV-ROUT-003, SRV-ROUT-004, SRV-ROUT-005, SRV-ROUT-006, SRV-RDIR-001, SRV-RDIR-002, SRV-RDIR-003, SRV-RWRT-001, SRV-DLST-001, SRV-DLST-002, SRV-DLST-003, SRV-CLI-003, SRV-CLI-006, SRV-CLI-008, SRV-CLI-009, SRV-CLI-010, SRV-CLI-011, SRV-CLI-014, SRV-CLI-015, SRV-CLI-016 (deferred from the first IrServe release; not removed from the contract)
Status: adapted
Reason: The first implementation proposal (`openspec/changes/001-port-minimal-static-server`, Stage 5a) deliberately scopes the initial Rust port to strict L0 — exactly eight SRVs (SRV-CLI-001, SRV-CLI-002, SRV-CLI-007, SRV-CLI-019, SRV-FILE-001, SRV-FILE-002, SRV-FILE-004, SRV-FILE-005). The slice exists to prove the methodology end-to-end against the existing oracle harness on the smallest possible code surface, not to ship a useful product. This is a release-scoping decision, not a behavioral divergence from `serve`.
Impact: The first IrServe release responds to L0 inputs only. L1+ inputs (e.g. `-l tcp://...`, `serve.json`-driven `cleanUrls`, configured redirects, directory listings) yield strict CLI rejection or absent-feature behavior, not parity with `serve`. No SRV bodies change. Subsequent changes (`002-…` onward) are sequenced to deliver L1 and L2 behavior; the deferred SRVs above stay `verified`/`accepted` in the contract throughout. Per-SRV un-deferral is recorded in subsequent D-NNN entries (D-009 un-defers SRV-CFG-001 / SRV-CFG-002 in Stage 6a).

## D-009: Stage 6a un-defers `serve.json` loading

Date: 2026-05-09
Affected requirements: SRV-CFG-001, SRV-CFG-002, SRV-CLI-009 (un-deferred from D-008's list)
Status: adapted
Reason: Change `003-load-serve-json` (Stage 6a) lands the configuration loader, the `-c/--config` CLI flag (closing SRV-CLI-009), and the `public` field surface. SRV-CFG-001's oracle list grows by ORC-064 (public field), ORC-065 (missing explicit), ORC-066 (malformed JSON), ORC-067 (explicit overrides default with observable `public` effect). SRV-CLI-009 was already `verified` via ORC-006; this change un-defers it from D-008 because the flag is now actually wired in IrServe. Stage 6h's row in `docs/stage6_l1_l2_capabilities.md` is updated to drop SRV-CLI-009 from its scope. The remaining schema-map fields (cleanUrls, redirects, rewrites, headers, etc.) are parsed into the typed configuration but their per-area behavior is delivered by sub-stages 6b–6g; D-008 still defers those per-area SRVs (SRV-ROUT-*, SRV-RDIR-*, SRV-RWRT-*, SRV-DLST-*, SRV-FILE-003). Q-003 (validation error format) stays open per D-002.
Impact: IrServe now reads `serve.json` / `now.json#now.static` / `package.json#static` per the SRV-CFG-001 lookup chain, accepts `-c/--config <PATH>` to override the implicit chain, exits non-zero on malformed JSON or missing explicit `--config`, and applies the `public` field to the served root. Per-field behavior beyond `public` is parsed-and-stored, not yet observable. Bug-for-bug AJV error wording is excluded by D-002. The reference's TypeError on a `now.json` lacking a top-level `now` key is replaced by clean fall-through to `package.json`; this micro-divergence is authorized by D-004.

## D-010: Stage 6b un-defers routing normalization (multi-slash + trailingSlash)

Date: 2026-05-09 (amended after Codex review rounds 1 and 2 to reflect the as-implemented wiring)
Affected requirements: SRV-ROUT-003, SRV-ROUT-004, SRV-ROUT-005 (un-deferred from D-008's list)
Status: adapted
Reason: Change `004-route-normalization` (Stage 6b) wires the routing-normalization layer of the 13-phase dispatcher. The implementation mirrors `serve-handler/src/index.js:121-185, 561, 586` rather than treating phases 3 and 5 as independent steps: the URI path is percent-decoded once at dispatcher entry; the trailingSlash decision (SRV-ROUT-003, SRV-ROUT-004) is taken on the **uncollapsed** decoded path so the multi-slash override at `index.js:158-160` is observable; the silent multi-slash collapse (SRV-ROUT-005, Q-006 closed) runs only for the path that flows into the resolve stage. When `trailingSlash` is set and the decoded path contains `//`, the redirect target is the slash-collapsed form (`encodeURI`-encoded for the `Location` header, mirroring `index.js:586`), regardless of what the add/strip branches would otherwise compute. Two pure-isolation probes (`trailingslash-add`, `trailingslash-strip`) exercise the redirect surface with `cleanUrls: false` so the cleanUrls 301 (Stage 6c) cannot mask or compose with phase 5; the compose probes (`prec-cleanurls-trailing`, `prec-cleanurls-trailing-false`) and the cleanUrls-dependent anchors of `multislash-collapse` (`double_slash_segment`, `internal_double_slash`) stay reference-only — they are skipped or marked `divergent` in `runner.l0` until Stage 6c lands phase 4. SRV-ROUT-003's oracle list grows by ORC-068, ORC-069, ORC-072, ORC-073, ORC-075, ORC-076; SRV-ROUT-004's by ORC-070, ORC-071, ORC-074; SRV-ROUT-005's by ORC-072, ORC-073, ORC-074. SRV-ROUT-006 (precedence) stays deferred because phases 4, 6, 7, 8 are not yet implemented.
Impact: IrServe now (a) percent-decodes the URI path at dispatcher entry (`crates/irserve-core/src/dispatch.rs`); (b) emits a 301 with the slash-collapsed form when `trailingSlash` is set and the decoded path contains `//` (literal or via `%2F%2F`), overriding the add/strip targets; (c) otherwise emits the trailingSlash 301 to `path + "/"` (add) or to `path[..len-1]` (strip), with dotfile/extension exemptions on the add branch only — matching the reference's `path.parse(p).{name, ext}` shape verbatim, including dotfile-with-extension cases like `/.bashrc.bak` (exempt because the basename has a non-leading dot); (d) silently collapses `//` for the resolve-and-onwards path when `trailingSlash` is unset (silent contract preserved per Q-006 / ORC-025); (e) sets the `Location` header through an `encodeURI`-equivalent encoder (`encode_uri_target` in `dispatch.rs`) so spaces become `%20`, non-ASCII bytes become percent-encoded UTF-8, and reserved-but-safe chars (`?`, `=`, `&`, `:`, `@`, `+`, `$`, `,`, `#`, `/`, `;`) pass through untouched. axum/hyper's auto-emitted `content-length: 0` on the empty 301 body is masked via `runner.l0.contentLengthMayDiffer` since the reference's Node http response omits it; both responses have identical (zero) body bytes. The remaining routing SRVs (SRV-ROUT-001/002 cleanUrls, SRV-ROUT-006 precedence) stay deferred per D-008.

Deferred to Stage 6f (SRV-SEC-001 / SRV-SEC-002, not 6b's scope): malformed percent-escape rejection. The dispatcher uses `percent_encoding::percent_decode_str(...).decode_utf8_lossy()`, which preserves invalid `%xx` sequences (e.g. `%zz`) as literal bytes rather than producing the reference's `400 Bad Request`. As a consequence, when `trailingSlash` is set, a request like `GET /bad%zz` flows through phases 5 and 9–13 unchanged: phase 5's add branch produces `/bad%zz/` (basename has no extension and isn't a dotfile), `encode_uri_target` re-encodes the literal `%` to `%25`, and the response is `301 Location: /bad%zz/` percent-escaped to `/bad%25zz/`. The reference rejects the same request with 400 (its `decodeURIComponent` raises `URIError`, which the handler maps to `400 Bad Request`). This divergence is contracted by ORC-041 (`tools/probe/cases/traversal-raw-encoded.json#raw_malformed_percent`, status: verified, must-match: 400) which intentionally has no `runner.l0` block and therefore auto-skips under `target=irserve`. Promotion to a strict decoder (e.g. `percent_decode_str(...).decode_utf8()` plus a 400 short-circuit on `Err`) and adding the `runner.l0` block to that probe are SRV-SEC-001 work for Stage 6f, not 6b. Likewise, the `traversal-encoded` probe stays reference-only until SRV-SEC-002 (single-pass-decode + dot-segment containment) is wired in 6f.

## D-011: Stage 6c un-defers cleanUrls (phases 4 + 8)

Date: 2026-05-09 (amended after Codex review rounds 1 and 2 to reflect the as-implemented array-form semantics)
Affected requirements: SRV-ROUT-001, SRV-ROUT-002 (un-deferred from D-008's list); SRV-ROUT-006 (partially un-deferred — cleanUrls↔trailingSlash composition observable; cleanUrls↔redirects/rewrites stays deferred until 6e)
Status: adapted
Reason: Change `005-clean-urls` (Stage 6c) wires phases 4 and 8 of the 13-phase dispatcher. Phase 4 (`crates/irserve-core/src/clean_urls.rs::compute_clean_urls_redirect`) emits a 301 to the extension-stripped form for any request matching the end-anchored regex `(\.html|\/index)$` when `cleanUrls` is enabled, with the `//` collapse on the stripped target and `ensureSlashStart` re-prepend mirroring `serve-handler/src/index.js:121-143` exactly. Single-pass strip semantics (`/index.html` → `/index`, NOT `/`) match the reference's regex with the redundant `g` flag against an end-anchor, pinned by the existing `_smoke#index_html_redirect` snapshot. Phase 8 (`crates/irserve-core/src/clean_urls.rs::try_clean_urls_resolve`) tries `<P>/index.html` first and `<P>.html` second, mirroring `findRelated` + `getPossiblePaths('.html')` at `index.js:276-307`; index-first order is probe-confirmed (Q-005 closed by `prec-cleanurls-default`).

The dispatcher gates phase 8 on `url_path_has_extension(&url_path)` so the reference's pre-stat behavior at `index.js:608-642` is faithfully reproduced: extensionless paths skip pre-stat and run phase 8 first (so an existing `<P>.html` is preferred over a bare extensionless `<P>` file, matching SRV-ROUT-006's "no pre-stat for extensionless" clause); has-extension paths run `resolve()` first and only fall back to phase 8 on `NotFound`, preventing `<P>.html` from shadowing an existing `<P>` file. Both phase 4 and phase 8 honor scope via a precompiled `CleanUrlsView` (`Off | On | Scoped(Vec<ScopedPattern>)`) built once at server start from `serve.json#cleanUrls`; each `ScopedPattern` carries a compiled `globset::GlobMatcher` (built with `literal_separator(true)` so `*` does not cross `/`) plus a `negate: bool` for minimatch-style `!`-prefix negation. `globset` was added at workspace pin `0.4` (latest stable verified via context7 on 2026-05-09). `applicable` collapses `//` in the request path before matching (mirrors `path.posix.resolve(requestPath)` inside `sourceMatches` at `index.js:38-67`) and iterates `matcher.is_match(path) ^ negate`, returning `true` on the first truthy result. Invalid glob patterns are silently skipped with a stderr warning emitted from `server.rs::serve` (server keeps running), mirroring the reference's silent-never-match behavior at `index.js:38-67` via `minimatch`.

Compose probes deferred at 6b per D-010 (`prec-cleanurls-trailing.json`, `prec-cleanurls-trailing-false.json`, and the `double_slash_segment` / `internal_double_slash` anchors of `multislash-collapse.json`) flip to L0-clean in this change, closing the cleanUrls↔trailingSlash composition surface. The cleanUrls↔redirects compose surface stays deferred to 6d (`006-configured-redirects`); cleanUrls↔rewrites to 6e (`007-configured-rewrites`). Until those land, SRV-ROUT-006 is `partially un-deferred` — its trailingSlash composition is verified against irserve, its redirects/rewrites composition is not.

`tools/probe/run.mjs::applyL0Filter` was extended: the `bodyMayDiffer` mask now also strips `body.kind`. Reason: raw-mode probes capture HTTP/1.1 chunked-encoding terminators (`0\r\n\r\n`, 5 bytes binary) as a "binary" body against Node's reference, while irserve's hyper layer uses `Content-Length: 0` for the same empty 301 — a transport-encoding choice, not an application-body divergence. Without the extension, the chunked-encoding tail trips the body-kind comparison on `multislash-collapse#double_slash_segment` and `#internal_double_slash`. The semantic widening is "body content (kind, length, bytes, preview) may differ; only headers and status are contractual at L0". The only pre-existing user (`notfound-shape#missing_html`) already had matching `text` kind on both sides, so stripping `kind` is a no-op there.

Impact: IrServe now (a) emits 301 to the extension-stripped form for `.html`, `/index`, and `.../index.html` requests when `cleanUrls` is enabled; (b) resolves extensionless requests via the index-first `<P>/index.html`-then-`<P>.html` candidate chain, with array-form `cleanUrls` globs gating both behaviors; (c) honors the reference's pre-stat ordering so `/foo.css` with both `/foo.css` and `/foo.css.html` present serves the existing `/foo.css` and does not get shadowed; (d) participates correctly in the cleanUrls↔trailingSlash compose surface (phase 4 fires before phase 5, mirroring `index.js:130-133`'s "strip the HTML parts before handling the trailing slash" coupling). The remaining routing SRVs (SRV-ROUT-006 redirects/rewrites composition) stay deferred per D-008 until 6d/6e land. README's "What is NOT yet observable" list is shrunk by cleanUrls; "Try IrServe" is refreshed with cleanUrls examples. ORC-002/012/013/014/017/020/021/022/023/024/026/027 flipped from "verified against reference only" to "verified against both reference and irserve" without renumbering.

Codex review round 1 amendments (P1 + P2): the array-form scope check was tightened against minimatch semantics — globs are now built with `GlobBuilder::new(...).literal_separator(true)` so a single `*` does not cross `/`, and `applicable` collapses `//` in the request path before `is_match` (mirrors `path.posix.resolve(requestPath)` inside `sourceMatches` at `index.js:38-67`). `oracle-matrix.md` ORC-017's must-match field was corrected from `Location: /about/` to `Location: /about` to match the canonical snapshot at `tools/probe/snapshots/prec-cleanurls-trailing.json#about_html` (cleanUrls strip wins over the trailingSlash add per the phase-4-before-5 coupling).

Codex review round 3 amendments (P1 + P2):
- **P1 — extglob (`+(...)`, `@(...)`, `?(...)`, `*(...)`, `!(...)`)
  scoped explicitly out.** The reference's `sourceMatches` calls
  `minimatch` at `index.js:59`, which honors Bash-style extended
  glob constructs. `sourceMatches` is shared between `applicable`
  (cleanUrls scope, `index.js:265`) and `toTarget` (redirects/rewrites,
  `index.js:70`), so extglob support is structurally identical on
  both branches. The pinned reference's only dedicated extglob
  test exercises redirects (`serve-handler/test/integration.test.js:449`,
  `redirects: ["face/+(mask1|mask2)/ideal"]`); the cleanUrls array
  test at `integration.test.js:705` uses a plain `/directory**`
  glob. cleanUrls-side extglob is therefore structurally implied
  but not upstream-tested — captured by the new
  `tools/probe/cases/cleanurls-extglob.json` snapshot in this repo.
  IrServe's `globset::GlobBuilder` does NOT support extglob and as
  of context7 lookup on 2026-05-09 no surveyed Rust crate
  (`globset 0.4.18`, `fast-glob 1.0.1`, `glob-match 0.2.1`,
  `wax 0.7.0`) does either. Rather than pull in regex-translation
  machinery for a power-user surface no committed probe exercises,
  the divergence is recorded as **Q-012** (open) with a
  reference-only probe `tools/probe/cases/cleanurls-extglob.json`
  capturing the gap: `cleanUrls: ["/public/+(page|other).html"]`,
  `GET /public/page.html` → reference 301 → `/public/page`, irserve
  serves the file directly (cleanUrls scope check misses because
  globset treats the extglob constructs as literal characters). The
  array-form contract claim in `005-clean-urls/specs/routing/spec.md`
  reads "standard globs" — `*`, `**`, `?`, character classes
  `[...]`, brace alternation `{a,b}`, and `!`-prefix negation —
  not full minimatch parity. Future closure of Q-012 (option a:
  manual regex translation; option b: a future Rust crate with
  extglob; option c: explicit `adapted` D-NNN) is out of 6c's
  scope.
- **P2 — round-2 stale text fixed.** The first paragraph of D-011
  (above) and the §1 module map + §3 pseudocode in
  `openspec/changes/005-clean-urls/design.md` referenced the
  pre-round-2 shape (`Mode::Scoped(GlobSet)`,
  `Result<Self, globset::Error>`, `GlobSetBuilder`,
  `Error::CleanUrlsGlob` startup-error). Updated in round 3 to
  match the as-implemented shape (`Mode::Scoped(Vec<ScopedPattern>)`,
  `(Self, Vec<InvalidGlob>)`, per-pattern `GlobMatcher` with
  `negate: bool`, silent-skip + stderr warning). The round-2
  amendment paragraph below was already correct; the stale
  references in the original D-011 paragraph were what tripped
  Codex.

Codex review round 2 amendments (P1 + P2): two more reference-faithfulness gaps closed.
- **Negation patterns now honored.** `slasher` preserves a leading `!` (mirrors `glob-slash.js:8`); `Mode::Scoped` stores `Vec<ScopedPattern>` where each pattern carries a `negate: bool`; `applicable` evaluates `matcher.is_match(path) ^ negate` per pattern and returns `true` on the first true result (mirrors minimatch's per-pattern negate with `nonegate: false` plus the reference's `applicable` iteration at `index.js:261-268`). A sole `!/secret/**` therefore enables cleanUrls for every path outside `/secret/**`, matching the reference. Backed by ORC-077 (`cases/cleanurls-negation.json`, 4 anchors).
- **Invalid globs are silent, not fatal.** `CleanUrlsView::from_config` was changed from `Result<Self, globset::Error>` to `(Self, Vec<InvalidGlob>)`: unparseable patterns are collected into the warnings vector, server startup continues, and the bin layer (`server.rs::serve`) emits one stderr warning per skipped pattern. The `Error::CleanUrlsGlob` variant was removed. This mirrors the reference's behavior at `index.js:38-67` via `minimatch` (unparseable patterns silently never match; server keeps running). Backed by ORC-078 (`cases/cleanurls-invalid-glob.json#html_no_redirect`).
