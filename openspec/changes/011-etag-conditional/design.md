# Design: ETag + 304 conditional GET

This document records the architecture for Stage 7a. 7a is the
first L3 sub-stage and lands a single SRV (SRV-CACHE-001) plus a
reusable runner extension. The architectural foundations
(crate layout, HTTP-stack pins, request lifecycle, oracle harness
layer) live in
`openspec/changes/001-port-minimal-static-server/design.md`. The
contract for the new capability lives in
`openspec/specs/http-cache/spec.md`. 7a does not add a new phase
to the 13-phase dispatcher; the ETag / 304 decision sits inside
the existing file-response slot (phase 12).

## 1. Where the ETag is computed

The dispatcher reads the file body, computes the default ETag
once, builds a candidate 200 response, applies user `headers`
rules to it, and then decides 200-vs-304 against the MERGED
response's `ETag`. The whole flow lives in `build_file_or_304`
at `crates/irserve-core/src/dispatch.rs:687`:

```rust
fn build_file_or_304(
    serve_config: &ServeConfig,
    req_headers: &HeaderMap,
    path: &Path,
    bytes: Vec<u8>,
    header_rules: &[HeaderRuleCompiled],
    request_path: &str,
) -> Response<Body> {
    let etag = etag_value(serve_config, path, &bytes);
    let response_200 = file_response(path, bytes, etag);
    let merged = apply_custom_headers(response_200, request_path, header_rules);
    if req_headers.get(RANGE).is_none() {
        if let (Some(inm), Some(effective_etag)) =
            (req_headers.get(IF_NONE_MATCH), merged.headers().get(ETAG))
        {
            if inm == effective_etag {
                return Response::builder().status(304).body(Body::empty())...;
            }
        }
    }
    merged
}
```

This shape matches the reference's `getHeaders`-then-304-check
at `serve-handler/src/index.js:194-254` + `:760-765`:

```js
const headers = Object.assign(defaultHeaders, related);   // :241 — user rules win
// ...status set / file open elided...
if (req.headers['if-none-match'] === headers.ETag && !req.headers.range) {  // :760
    response.statusCode = 304;
    response.end();
    return;
}
```

with three structural notes:

1. **Headers merge runs BEFORE the 304 decision.** Codex review
   round 1 P1 caught a divergence in the prior implementation:
   the 304 check compared `If-None-Match` against the
   default-generated ETag, while the response sent on the wire
   carried the user-override value. Reference behavior at
   `index.js:241, 760` merges first, checks second; irserve now
   does the same. The dispatcher's File/Index and renderSingle
   call sites accordingly return `None` for the outer
   `dispatch` wrapper's `headers_path` slot (the headers pass
   already ran inside `build_file_or_304`).

2. **Compute-then-decide vs the reference's stash-on-stats.**
   The reference computes ETag eagerly during `findRelated` /
   `getETag` (`index.js:227-233`) and stashes it on the `stats`
   object; the 304 branch reads it via `headers.ETag` after the
   `Object.assign` merge. IrServe computes once in
   `build_file_or_304`, lets `file_response` insert it as the
   default header, and reads the merged value back via
   `merged.headers().get(ETAG)`. The per-request cost is the same
   (one sha1 over file bytes), and we avoid threading an extra
   field through `ResolveOutcome`.

3. **No allocation when 304.** The reference's `response.end()` at
   `index.js:763` writes status only; no body. IrServe builds a
   fresh `Response<Body>` with `Body::empty()` and 304 status —
   no `Content-Type`, no `ETag` echo. This mirrors the reference
   exactly: the prior `response.setHeader('Last-Modified', ...)`
   and `response.setHeader('ETag', ...)` calls at
   `index.js:752-755` apply only to the 200 branch, and the 304
   branch short-circuits before reaching the body-write
   (`index.js:768-772`).

`file_response` itself (`dispatch.rs:637`) takes
`etag: Option<HeaderValue>` and inserts the header when `Some`.
The two call sites are:

- The standard File/Index arm at `dispatch.rs:320`-ish — the path
  reached by `ResolveOutcome::File` and `ResolveOutcome::Index`.
- The renderSingle short-circuit at `dispatch.rs:430`-ish — when
  `renderSingle` collapses a one-file directory to a direct file
  response. The reference applies ETag here too
  (`index.js:330-340` resolves to the same `findRelated` path
  that the regular file branch uses), so irserve does likewise.

`etag_value(serve_config, path, bytes)` at `dispatch.rs:655`
returns `None` when `serve_config.etag == Some(false)`.
`None` and `Some(true)` both enable default generation, mirroring
`vercel/serve`'s CLI-default-true semantics
(`third_party/serve/source/main.ts` sets `config.etag =
!args['--no-etag']` before invoking the handler; the CLI is
irserve's only entry point, so the default rides on every
invocation that does not opt out via config). `etag: false`
disables only the DEFAULT — user `headers` rules can still set
`ETag` even when `etag: false`, exactly mirroring the
reference's `Object.assign`-after-gate at `index.js:227-241`.
A unit test pins this combination
(`dispatch::tests::custom_etag_override_drives_304_decision` and
the etag-false-with-rule variant).

## 2. The Range guard (Stage 7c precursor)

The 304 short-circuit fires only when the request carries no
`Range` header. Reference behavior at `index.js:760`:

```js
if (req.headers['if-none-match'] === stats.etag && !req.headers.range) { ... }
```

7a does not parse `Range` yet (deferred to 7c), but the one-line
guard `req_headers.get(RANGE).is_none() && ...` goes in now so
that 7c does not have to revisit `build_file_or_304`. Without
this guard, a `Range: bytes=0-3` request with matching
`If-None-Match` would prematurely 304 — wrong per the reference
and wrong per RFC 9110 §13.1.3 (range requests have priority
over conditional-GET 304 short-circuit when the cache validator
matches).

A unit test pins the precursor: with `If-None-Match` matching
and `Range: bytes=0-10` present, the response is 200 with ETag
(not 304). When 7c lands, the same test will continue to pin
the not-304 outcome; 7c will add tests that the response is 206
/ 416 / 200 according to the parsed range.

## 3. Why ETag is NOT applied to listings / redirects / errors

- **Directory listings.** The reference's listing branch
  (`index.js:330-432`) builds the response from
  `directoryTemplate`; `getETag` is not called. Listing bodies
  are also volatile (timestamps, dynamic sorting), so caching
  on ETag would either be wrong or pessimistic. IrServe mirrors:
  `dispatch.rs`'s listing-builder path does not call
  `build_file_or_304` — it goes through `listing_response`.

- **3xx redirects.** The reference's redirect branch at
  `index.js:586-588` calls `response.writeHead(redirect.statusCode,
  { Location: ... })`; no `ETag` is set. IrServe's redirect arms
  (cleanUrls, trailingSlash, configured redirects) likewise emit
  responses with `Location` only, no ETag. This is consistent
  with D-015 finding #2 (3xx redirects skip header application
  in `apply_custom_headers`) — the redirect path is structurally
  separate from the file-response path.

- **JSON error responses.** The reference's JSON-preferring
  client branch at `index.js:477-487` builds a templated JSON
  envelope and returns early, before any `getHeaders` /
  `getETag` call. IrServe mirrors via `error_response`'s
  JSON-Accept branch.

- **Custom HTML error pages.** The reference's
  `sendError` at `index.js:508` calls `getHeaders(.., errorPage,
  stats)` for the custom-`<status>.html` branch and stashes the
  ETag via the same `findRelated` path used for normal file
  responses. **IrServe defers this** to a follow-up — neither
  SRV-CACHE-001's scenarios nor ORC-042 / ORC-043 mandate ETag
  on error pages, and keeping 7a scope tight reduces blast
  radius. The divergence is documented as a Compatibility note
  on the new requirement and is not a contractual mismatch
  (SRV-CACHE-001's scope is "file responses").

- **Fallback HTML error pages.** Reference and irserve both skip
  ETag on the synthetic `<h1>STATUS REASON</h1>` body, consistent
  with the no-`getHeaders` shape of that branch.

## 4. ETag vs user `headers` rules

User `serve.json#headers` rules CAN override or delete the
default ETag, and the override DRIVES the 304 decision. This
mirrors reference's `Object.assign(defaultHeaders, related)` at
`serve-handler/src/index.js:241` followed by the 304 check at
`:760` reading the merged `headers.ETag`.

After Codex review round 1 P1, irserve runs the user-header
overlay BEFORE the 304 decision. `build_file_or_304`:

1. Builds the candidate 200 via `file_response`, with the
   default ETag (when `etag` config is not `Some(false)`).
2. Calls `apply_custom_headers(response_200, request_path,
   header_rules)` — same function the wrapper used to call. User
   rules with `key: "ETag"` replace the default; `value: null`
   deletes it; non-ETag rules layer onto the response unchanged.
3. Reads `merged.headers().get(ETAG)` and compares against the
   request's `If-None-Match`. If they match (and `Range` is
   absent), emits 304; otherwise returns the merged 200.

Because `build_file_or_304` already runs the headers overlay,
the dispatcher's File/Index and renderSingle call sites return
`None` for the outer `dispatch` wrapper's `headers_path` slot.
The wrapper's `match` accordingly skips the second
`apply_custom_headers` for these branches — see
`dispatch.rs:67`. Other branches (the 405 fallback, error
responses authored by `error_response`, listings) preserve their
prior behavior unchanged.

Two unit tests in `dispatch::tests` pin the override semantics:

- `custom_etag_override_drives_304_decision` — with a `headers`
  rule setting `ETag: "custom"`, the 304 check sees `"custom"`
  and 304s only when the request's `If-None-Match` is `"custom"`
  (the default sha1 no longer matches because the override
  masked it).

- `custom_etag_delete_disables_304` — with a `headers` rule
  setting `ETag: null`, the merged response has no ETag header,
  so the 304 short-circuit never fires; the response is 200.

The `etag: false` config + user rule combination is covered by
the requirement's Compatibility note in
`openspec/specs/http-cache/spec.md` (and mirrored in the delta);
the wire behavior is identical to `etag: true` + user rule
because the only thing `etag: false` changes is whether the
DEFAULT is generated — the merge and 304 check operate on the
final response either way.

## 5. Probe runner capture-replay

The pre-7a `etag-roundtrip.json` pinned a static
`If-None-Match: "\"3638b78821a961fcf35969f0bc67cc5944d64a0b\""`
matching the reference's sha1 of `body{color:red}\n` named
`asset.css`. This worked under `target=reference` but could not
flip to dual-target — irserve and reference both happen to
produce the same hash today (D-017 mirrors the formula
byte-for-byte), but the **contract** is the round-trip behavior,
not the exact hash value, so any test that hard-codes a hex
literal would silently break the moment D-017's formula is
deliberately revised.

Slice 4 extended `tools/probe/run.mjs`'s case schema: a request
header value may be either a string or an object
`{"$fromResponse": {"request": "<name>", "header": "<lc>"}}`.
At request-issue time, the runner looks up the captured response
for `request` and substitutes the named header's value verbatim
(including quotes). When the named request has no recorded
response yet, the runner fails loudly with the case id.

The extension is target-agnostic: it works identically under
`target=reference` and `target=irserve`, and the reference
snapshot was re-recorded once to confirm the runner change is
value-preserving against the pinned reference. The same
mechanism is reusable for 7b (`If-Modified-Since` round-trip,
sourcing `Last-Modified` from a prior response) and 7c
(`If-Range` round-trip, sourcing either `Last-Modified` or
`ETag`). One investment, multi-stage payback.

## 6. Methodological signals

This stage authored **one new D-NNN entry** (D-017) and no
existing-D edits. No other divergences required new decisions —
the choice of hash formula was on the line, the reference's
specific behaviors on listings / redirects / errors were either
mirrored or already covered by deferral notes in
`compatibility-levels.md`.

D-017 captures the hash choice + the round-trip-is-the-contract
framing. Per anti-hallucination rule #4, the source of truth
ordering is `README < tests < source < oracle probe`; the probe
encodes the contract, the hash value is implementation. D-017
makes that explicit so a future stage (7b, an in-memory ETag
cache for performance) does not need to re-litigate the hash
choice.

## 7. Verification

`cargo test -p irserve-core etag` and
`cargo test -p irserve-core dispatch::tests::etag` cover:

- byte-equality of `compute_etag(asset.css, b"body{color:red}\n")`
  against the reference's pinned snapshot;
- the four corners of the 304 decision (match → 304, mismatch →
  200, Range + match → 200, ETag disabled → no 304, no INM →
  200 + ETag).

`cargo test -p irserve --test oracle` exercises ORC-042 / ORC-043
under both targets. The oracle harness moves from `73 passed / 8
skipped` to `75 passed / 6 skipped` (the two etag-roundtrip
requests promoted off the runner's L0 deferral list).

Reference still 81 of 81 green via `node tools/probe/run.mjs
--all --target=reference --snapshot=verify`. The
`etag-roundtrip.json` reference snapshot was re-recorded once
when the runner extension landed, to confirm capture-replay is
value-preserving against the pinned reference; the snapshot
diff was limited to a metadata pass and the resolved
`if-none-match` value (which now mirrors the captured first-GET
ETag rather than a hand-typed literal).

Manual smoke commands are documented in `README.md`'s
"Try IrServe (post-7a)" section.
