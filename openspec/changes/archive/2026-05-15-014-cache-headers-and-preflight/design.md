# Design: Cache-Control default surface + OPTIONS (CORS preflight)

This document records the architecture for Stage 7d. 7d is
the fourth L3 sub-stage and lands two SRVs (SRV-CACHE-005
and SRV-CORS-001 — both reach dual-target verification at
end-of-stage). No new SRV is introduced; no `D-NNN` entry
is authored. The architectural foundations (crate layout,
HTTP-stack pins, request lifecycle, 13-phase dispatcher,
oracle harness layer) live in
`openspec/changes/archive/2026-05-15-001-port-minimal-static-server/design.md`;
the Stage-6f user-headers seam at `apply_custom_headers`
(`crates/irserve-core/src/custom_headers.rs:178-221`) and
the Stage-6h CORS overlay at `apply_cors`
(`crates/irserve-core/src/cors.rs:26-37`) — both reused
verbatim. 7d adds NO new module or abstraction: it is a
single-line method-gate widening (slice 1) plus
probe-partition flips (slice 0 and the no-op slice 2).

## §1. OPTIONS routing as GET

The reference's source-of-truth for the OPTIONS-routed-as-GET
behavior is `third_party/serve-handler/src/index.js:548-769`.
The critical fact is that **nowhere in the handler is
`request.method` inspected**. The full pipeline runs
identically for GET / HEAD / OPTIONS / POST / PUT / DELETE
/ any verb. Key proof points (all line numbers in
`serve-handler/src/index.js`):

- **L548-562** — entry, decode URI, no method branching.
- **L591-602** — redirect path; no method branching.
- **L613-642** — stat path; no method branching.
- **L644-664** — directory rendering; no method branching.
- **L666-704** — stats + symlink resolution; no method
  branching.
- **L706-741** — Range parsing (Stage 7c's reference seam);
  no method branching.
- **L749-756** — ETag 304 short-circuit; no method
  branching. The only conditions are
  `request.headers.range == null && headers.ETag &&
  headers.ETag === request.headers['if-none-match']`.
- **L767-769** — final `response.writeHead(statusCode,
  headers)` + `stream.pipe(response)`; no method
  branching. The `stream.pipe(response)` runs
  unconditionally, which is why reference does not
  suppress HEAD bodies either (out of scope, see proposal).

CLI-side at `third_party/serve/source/utilities/server.ts:42-93`:
the `--cors` branch (L65-70) sets exactly four
`access-control-*` headers via `response.setHeader` BEFORE
delegating to `serve-handler`. No method check at the CLI
layer either:

```ts
// L65-70 (verbatim)
if (args['--cors']) {
  response.setHeader('Access-Control-Allow-Origin', '*');
  response.setHeader('Access-Control-Allow-Headers', '*');
  response.setHeader('Access-Control-Allow-Credentials', 'true');
  response.setHeader('Access-Control-Allow-Private-Network', 'true');
}
```

(Note: the reference does NOT emit
`Access-Control-Allow-Methods`,
`Access-Control-Allow-Expose-Headers`, or
`Access-Control-Max-Age` from this branch — only the four
headers above. irserve mirrors. The inventory entry for
SRV-CORS-001 catalogues the same four headers.)

**irserve dispatcher seam.** `crates/irserve-core/src/dispatch.rs:91-108`.

Before Stage 7d:

```rust
if req.method() != Method::GET && req.method() != Method::HEAD {
    let resp = Response::builder()
        .status(StatusCode::METHOD_NOT_ALLOWED)
        .body(Body::empty())
        .expect("405 response should always build");
    return (resp, Some(req.uri().path().to_string()));
}
```

After Stage 7d (slice 1):

```rust
if !matches!(*req.method(), Method::GET | Method::HEAD | Method::OPTIONS) {
    let resp = Response::builder()
        .status(StatusCode::METHOD_NOT_ALLOWED)
        .body(Body::empty())
        .expect("405 response should always build");
    return (resp, Some(req.uri().path().to_string()));
}
```

Everything downstream — phases 3..13,
`apply_custom_headers`, `apply_cors`, and
`build_file_or_304` (ETag / Last-Modified / 304 / Range) —
is **unchanged**. The widening is single-axis: it
re-classifies OPTIONS from "rejected at the gate" to
"flows through the same code path as GET".

Three side-effects of running the full pipeline for
OPTIONS, all of which fall out automatically without code
changes:

1. **OPTIONS + `If-None-Match` match returns 304.** The
   conditional-GET branch at `build_file_or_304` doesn't
   check `req.method()`; the merged `ETag` comparison
   fires for any method that reaches it. Mirrors
   reference's L760 guard, which gates only on
   `request.headers.range == null && headers.ETag ===
   request.headers['if-none-match']`, never on method.

2. **OPTIONS + `Range` honored.** The Stage-7c
   `range::apply` path at `dispatch.rs:832` runs for any
   method. An `OPTIONS /asset.css` with
   `Range: bytes=0-3` emits 206 + partial body. Mirrors
   reference's `Range`-parsing block at
   `index.js:717-741`, which is also method-blind.

3. **OPTIONS on missing path returns 404 with CORS
   overlay.** The dispatcher's File/Index arm at
   `dispatch.rs` reaches the 404 path for any method; the
   `apply_cors` overlay at `crates/irserve-core/src/server.rs:220-229`
   runs post-dispatch on every response, so the four
   CORS headers ride on the 404 too. Mirrors
   reference's `setHeader`-before-`serve-handler`
   ordering.

The methodological note is that **the slice 1 widening is
strictly additive at the wire**: any wire shape an `OPTIONS`
request previously produced (a single 405 response from the
gate) is replaced with whatever shape a `GET` of the same
URI + headers would produce. No existing GET / HEAD
behavior changes.

## §2. Cache-Control absence-of-default

Reference's `getHeaders` at
`third_party/serve-handler/src/index.js:194-254` is the
**only** entry point that can attach a `Cache-Control`
header to a response. Its full body (lightly elided for
brevity, all line numbers verbatim):

```js
// L194-254
const getHeaders = async (handlers, config, current, absolutePath, stats) => {
    const {headers: customHeaders = [], etag = false} = config;
    const related = {};
    const {base} = path.parse(absolutePath);
    const relativePath = path.relative(current, absolutePath);

    if (customHeaders.length > 0) {
        for (let index = 0; index < customHeaders.length; index++) {
            const {source, headers} = customHeaders[index];

            if (sourceMatches(source, slasher(relativePath))) {
                appendHeaders(related, headers);
            }
        }
    }

    let defaultHeaders = {};

    if (stats) {
        defaultHeaders = {
            'Content-Length': stats.size,
            'Content-Disposition': contentDisposition(base, { type: 'inline' }),
            'Accept-Ranges': 'bytes'
        };

        if (etag) {
            /* ETag branch */
        } else {
            /* Last-Modified branch */
        }

        const contentType = mime.contentType(base);

        if (contentType) {
            defaultHeaders['Content-Type'] = contentType;
        }
    }

    const headers = Object.assign(defaultHeaders, related);

    for (const key in headers) {
        if (headers.hasOwnProperty(key) && headers[key] === null) {
            delete headers[key];
        }
    }

    return headers;
};
```

Two structural observations:

1. **`defaultHeaders` (L215-243) never includes
   `Cache-Control`.** The only fields the reference ever
   populates by default are `Content-Length`,
   `Content-Disposition`, `Accept-Ranges`, one of `ETag`
   or `Last-Modified` (Stage 7a / 7b), and `Content-Type`.
   There is no `defaultHeaders['Cache-Control'] = ...`
   line anywhere in the function (or anywhere else in
   the handler that reaches the response).

2. **The header reaches the response only via
   `customHeaders`.** The `Object.assign(defaultHeaders,
   related)` call at L241 is the merge point: any
   `Cache-Control` value a user `serve.json#headers` rule
   matched into `related` lands on the response verbatim.
   The subsequent `value === null` deletion loop (L246-250)
   honors user `value: null` rules — a user can also
   delete a `Cache-Control` they previously added.

**irserve mirror.** A grep across `crates/` for
`cache-control` / `Cache-Control` returns **zero matches**
— there is no default-emission point anywhere in the Rust
code. The only entry point is `apply_custom_headers`
(Stage 6f, SRV-HDR-001) at
`crates/irserve-core/src/custom_headers.rs:178-221`, which
applies user `serve.json#headers` rules verbatim
(supporting `value: null` deletion via the same loop
shape). Verified empirically by
`tools/probe/cases/cache-control-default.json` running
L0-clean dual-target across 6 anchors after slice 0: a
plain file response carries no `Cache-Control`; a
listing carries none; a 404 (HTML or JSON) carries none;
only a path matched by a user rule carries the rule's
literal value.

The 7d contract is therefore "absence-of-default + verbatim
emission on rule match" — pinned by the 6-anchor probe.
There is no `D-NNN` because there is no intentional
divergence: irserve already mirrored the reference since
Stage 6f.

## §3. Why no `D-NNN`

Both SRVs mirror the reference. The only alternatives
considered in plan-mode were:

- **204 No Content preflight short-circuit on OPTIONS.**
  RFC 7231 §4.3.7 / RFC 9110 §9.3.7 suggest a server MAY
  respond to OPTIONS with `204 No Content` (or
  `200 OK` + empty body) without running the full
  resource-resolution pipeline. This is the conventional
  pattern in many web frameworks. The reference rejects
  this approach (no method check anywhere → OPTIONS runs
  the full pipeline). User chose **Mirror** in plan-mode
  AskUserQuestion D1: implement OPTIONS as a pass-through
  to the GET pipeline, no 204 short-circuit. No `D-NNN`
  entry — there is no intentional divergence.

- **Keep the status-quo 405 for OPTIONS.** Considered and
  rejected: SRV-CORS-001 explicitly catalogues OPTIONS
  routing as part of the L3 surface; the inventory open
  question on the SRV (whether to adopt or diverge from
  the no-preflight-short-circuit behavior) needs to be
  closed; mirror is the lowest-risk closure.

- **Adopt a default `Cache-Control`** (e.g.
  `Cache-Control: public, max-age=0`). Considered and
  rejected: reference emits no default, and adding one
  would be a wire-observable divergence requiring a
  `D-NNN` entry and a `divergent` probe partition. User
  chose Mirror; the absence-of-default contract is the
  lowest-friction lock.

Plan-mode AskUserQuestion D1 ("Implement OPTIONS as a
pass-through to the GET pipeline?") resolved to
**Mirror**. No further forks were needed for slice 0
(verification-only) or slice 2 (no-op audit). The last
`D-NNN` is D-018 (Stage 7b's IMS-304 adaptation); Stage
7c added none; Stage 7d adds none.

**Anti-hallucination rule #5 in action.** The plan was
explicit that the absence-of-default Cache-Control surface
was already wire-observable on both targets (the
`cache-control-default.json` probe was already passing
under `target=reference` and `target=irserve` separately;
slice 0 only added the `runner.l0` partition to ratchet it
to dual-target). No re-litigation of the contract was
needed — the empirical evidence was already on disk.

**Anti-hallucination rule #8 in action.** The slice 1
implementation chose the method-gate widening over a
204-shortcut after running an empirical probe
(`cors-preflight.json` with `preflight_options`) against
the reference; the snapshot showed `200 OK` + the file
body + the four CORS headers + ETag + Content-Type +
Accept-Ranges. Status was `200`, NOT `204`. The 204-shortcut
alternative was thereby ruled out by reference behavior
before any irserve code was touched.
