# Design: Custom error pages, full L2 security, custom response headers

This document records the architecture for Stage 6f. Unlike 6b–6e, 6f
does NOT wire a new dispatcher *phase*; it generalizes phase 13 (error
response), tightens phases 1 and 10 (decode + containment), and adds a
post-dispatch header-application pass.

## 1. Pipeline placement

Phase numbering from `openspec/changes/archive/2026-05-15-001-port-minimal-static-server/design.md`
§4. 6f touches three phases plus a post-dispatch hook:

| Phase | Stage 5b–6e behavior | Stage 6f change |
|-------|----------------------|-----------------|
| 1 (decode) | `percent_decode_str(...).decode_utf8_lossy()` — silent U+FFFD on malformed `%xx` | Wrap with `try_percent_decode`; `Err` short-circuits to `error_response(400, ..)` |
| 1 (containment) | None — relied on phase 10 fs-check | New `lexical_path_escapes_root` walks `..` segments; on escape, return `error_response(400, ..)` |
| 10 (containment) | `EscapedRoot` collapsed into `NotFound` → 404 | `EscapedRoot` routes to `error_response(400, ..)` (defense-in-depth for symlink/canonicalize escapes) |
| 13 (error response) | `notfound_response(headers)` — hardcoded 404 + literal HTML/JSON bodies | `error_response(status, headers, root)` — status-keyed JSON envelope; for HTML clients, `<status>.html` lookup at served root with generic-fallback |
| post-dispatch | None | `apply_custom_headers(response, path, rules)` runs after every non-3xx response |

The compiled rule structures live in `server.rs` alongside
`RedirectRuleCompiled` / `RewriteRuleCompiled` and are threaded into
`dispatch` the same way (added a 7th parameter `header_rules:
&[HeaderRuleCompiled]`).

## 2. `error.rs` and the `<status>.html` lookup

`crates/irserve-core/src/error.rs` replaces the old `notfound.rs` with
a status-parametric API:

```rust
pub async fn error_response(
    status: StatusCode,
    request_headers: &HeaderMap,
    root: &Path,
) -> Response<Body>
```

Three branches mirror `serve-handler/src/index.js:467-524` (`sendError`):

1. **JSON-preferring client.** `accepts_json(request_headers)` (lifted
   verbatim from `notfound.rs`) returns true. Emit a status-keyed
   envelope from `json_envelope_for(status)`:
   - `400` → `{"error":{"code":"bad_request","message":"Bad Request"}}`
   - `404` → `{"error":{"code":"not_found","message":"The requested path could not be found"}}`
   - other → `{"error":{"code":"server_error","message":"Internal Server Error"}}` (defensive — unreachable today)

   Mirrors `index.js:477-487`.

2. **HTML client + custom page.** `tokio::fs::read(root.join("{status}.html"))`
   succeeds. Emit the file bytes as the response body with `Content-Type:
   text/html; charset=utf-8` and the matching status. Mirrors
   `index.js:490-516` (sans the readstream-pipe — irserve buffers the
   file, which is fine for typical error pages of a few KB).

3. **HTML client + no custom page.** Generic
   `<h1>{status} {reason}</h1>\n` body, where `reason` comes from
   `StatusCode::canonical_reason()`. D-003 disclaims markup parity, so
   the generic shape is in-spec; the `notfound-shape` probe's existing
   `bodyMayDiffer` partition continues to mask divergences.

The lookup uses a `tokio::fs::read` (full-buffer) rather than a stat +
streaming open. Justification: error pages are typically <10 KB, the
read happens only on error responses (rare), and the existing 200-path
`file_response` also buffers via `tokio::fs::read`. No streaming or
range support in 6f's scope.

## 3. Strict %xx decode (phase 1)

```rust
fn try_percent_decode(s: &str) -> Result<Cow<'_, str>, ()> {
    let bytes = s.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' {
            if i + 2 >= bytes.len()
                || !is_ascii_hex(bytes[i + 1])
                || !is_ascii_hex(bytes[i + 2])
            {
                return Err(());
            }
            i += 3;
        } else {
            i += 1;
        }
    }
    Ok(percent_decode_str(s).decode_utf8_lossy())
}
```

The validator runs first; on success, the decoder body uses
`decode_utf8_lossy` (preserving the prior semantics for *valid* `%xx`
sequences with non-UTF-8 bytes). The Err branch is unit-typed because
the only meaningful signal is "malformed" — the response is always 400.

Mirrors `decodeURIComponent`'s URIError at `index.js:561-567`. The Node
function throws on truncated escapes (`%`, `%a`) and non-hex digits
(`%zz`, `%g0`); our validator handles all three.

## 4. Lexical containment check (phase 1)

```rust
fn lexical_path_escapes_root(decoded_path: &str) -> bool {
    let mut depth: i32 = 0;
    for seg in decoded_path.split('/') {
        match seg {
            "" | "." => {}
            ".." => {
                depth -= 1;
                if depth < 0 {
                    return true;
                }
            }
            _ => depth += 1,
        }
    }
    false
}
```

Empty segments (from `//` runs) and `.` segments are no-ops; `..`
decrements depth and below-zero depth is the escape signal. Does no
filesystem I/O.

Why purely lexical (and not just relying on phase 10's
`canonicalize`-based check): the reference's `isPathInside(path.join(
current, relativePath), current)` runs at `index.js:574` BEFORE any
filesystem operation, so escape responses are 400 even when the
escaped target doesn't exist. IrServe's previous `EscapedRoot` (in
`resolve.rs:41-48`) only fires when `tokio::fs::canonicalize` succeeds
and lands outside root — which silently returned 404 (NotFound) for
`/.../package.json` paths whose escaped target happens not to exist on
disk. The lexical check matches reference parity for ORC-038/039.

## 5. Custom headers (`custom_headers.rs`)

### Compilation

```rust
pub struct HeaderRuleCompiled {
    matcher: Matcher,
    headers: Vec<HeaderItem>,
}

pub fn compile_rules(rules: &[HeaderRule])
    -> (Vec<HeaderRuleCompiled>, Vec<InvalidHeaderRule>)
```

Reuses `path_pattern::Matcher::compile(source, "/")`. The placeholder
destination `"/"` is stored verbatim and never read — `try_match`'s
return value (`Option<String>`) is consumed only as `is_some()` for the
boolean match decision. No header-specific matcher kernel is needed.

Matches reference's `sourceMatches(source, slasher(relativePath))` at
`index.js:207`. Default `i` flag, dot-rule, backslash handling, and
all D-012 inherited divergences apply unchanged because the kernel is
identical.

### Application

```rust
pub fn apply_custom_headers(
    mut response: Response<Body>,
    request_path: &str,
    rules: &[HeaderRuleCompiled],
) -> Response<Body>
```

Two passes match the reference's `index.js:200-251`:

1. **Accumulate + merge.** For each rule whose source matches
   `request_path`, insert every `(key, Some(value))` pair into the
   response's `HeaderMap`. axum's `HeaderMap::insert` normalizes keys
   case-insensitively, so the last write wins regardless of case
   (mirrors `Object.assign(defaultHeaders, related)`).

2. **Null-prune.** Walk matched rules a second time; for each entry
   whose `value` is `None`, remove the corresponding header from the
   map (case-insensitive). Mirrors the
   `for (key in headers) { if (... null) delete headers[key] }` loop
   at `index.js:247-251`.

The two-pass split is the simplest correct mirror of the reference's
two-stage merge. Combining the loops would let a `value: null` in rule
A delete a `value: "v"` from rule B even if B fires later — which is
NOT what the reference does (the prune runs over the merged map).

### 3xx skip

```rust
if rules.is_empty() || response.status().is_redirection() {
    return response;
}
```

Empirical finding: reference's redirect path at `index.js:586-588`
builds the response via `response.writeHead(statusCode, { Location:
... })` without going through `getHeaders`. So custom headers never
layer onto 301/302/etc. IrServe mirrors. Pinned by
`headers-custom#html_get` (cleanUrls 301 case): the reference snapshot
emits only `location:` and no `x-custom:`, so irserve's apply pass
must skip 3xx to match.

### Path threading

Header matching needs the request path; the existing dispatcher works
on a `Cow<'_, str>` decoded path internally and consumes `req` early.
The simplest threading is to compute the matching path once at
top-level `dispatch` (a thin wrapper) and call `dispatch_inner` for the
core pipeline:

```rust
pub async fn dispatch(req, ..., header_rules) -> Response<Body> {
    let raw_path = req.uri().path().to_string();
    let path_for_headers: String = match try_percent_decode(&raw_path) {
        Ok(p) => collapse_slashes(&p).into_owned(),
        Err(_) => raw_path.clone(),
    };
    let response = dispatch_inner(req, root, ...).await;
    apply_custom_headers(response, &path_for_headers, header_rules)
}
```

The double-decode (top-level wrapper + `dispatch_inner`'s internal
call) is cheap — URL paths are short — and keeps `dispatch_inner`
unchanged. Falling back to `raw_path` on decode-Err matches the
reference's behavior in the URIError catch (which still calls
`sendError(400)` and indirectly `getHeaders` with an unrelated
`absolutePath`).

## 6. Empirical findings vs the slice plan

Two assumptions in the 6f plan turned out to be wrong; both surfaced
when recording the new probes' reference snapshots (anti-hallucination
rule 6 — record reference first):

- **`headers-on-error#bad_request_carries_x_test`** (planned). Reference
  did NOT emit `x-test` on the 400 path-traversal response. Trace:
  `sendError(400)` calls `getHeaders(.., absolutePath, null)` where
  `absolutePath = path.join(current, '/../package.json')`. For a fixture
  rooted at `/A/B/`, `path.join` resolves to `/A/package.json` (parent),
  outside `current`. `getHeaders` then computes `relativePath =
  path.relative(current, absolutePath) = '../package.json'`. The
  `slasher` call posix-normalizes `path.posix.join('/', '../package.json')`
  = `/package.json`. So sourceMatches happens against `/package.json`,
  not the request's `/../package.json`. Empirically minimatch's
  `**` does not match `/package.json` in this slasher-normalized form
  (or does match but the rule's headers are otherwise dropped). The
  net result: no x-test on the 400. Rather than mirror this opaque
  matcher quirk, 6f drops the bad_request anchor from the probe and
  keeps the 404 anchor (which is empirically validated and behaves
  intuitively).

- **`headers-custom#html_get`** (existing). Reference's cleanUrls 301
  emits ONLY `location: /page` — no `x-custom`. Looking at the redirect
  path: `response.writeHead(redirect.statusCode, { Location:
  encodeURI(redirect.target) })` at `index.js:586-588`, with no
  `getHeaders` call. So custom headers never apply to redirects in the
  reference. IrServe's apply pass adds the `is_redirection()`
  short-circuit so its 301 also emits only `location:`.

Both findings are documented in this design and in `decisions.md`
(D-015) so future maintenance doesn't try to "fix" the divergence by
adding header application to those branches.

## 7. Out-of-scope reaffirmation

Listed in `proposal.md` §"Out of scope". Two items deserve a
design-level note:

- **`<status>.html` for non-error statuses.** The lookup happens only
  inside `error_response`, which is called only on 4xx paths. A 200
  response served via `file_response` does not pass through
  `error_response`; there is no `200.html` substitution. This matches
  reference's `sendError`-only invocation pattern.

- **`HeaderItem::value: null` reachability via reference CLI.** The
  reference's `serve` CLI validates `serve.json` against
  `@zeit/schemas/deployment/config-static.js`, which declares
  `value: { type: 'string', minLength: 1, ... }`. `serve` refuses to
  start with a `value: null` entry. So the SRV-HDR-002 contract is
  unreachable through the reference CLI; irserve verifies the prune
  logic via `custom_headers::tests` (8 unit tests covering
  insert/accumulate/case-insensitive override / 3xx-skip / null-prune
  in three flavors). Cross-referenced from D-015.
