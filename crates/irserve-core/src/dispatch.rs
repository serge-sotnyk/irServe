use std::borrow::Cow;
use std::fs::Metadata;
use std::path::Path;

use axum::body::Body;
use axum::http::header::{
    HeaderValue, CONTENT_TYPE, ETAG, IF_MODIFIED_SINCE, IF_NONE_MATCH, LAST_MODIFIED, LOCATION,
    RANGE,
};
use axum::http::{HeaderMap, Method, Request, Response, StatusCode};
use percent_encoding::{percent_decode_str, utf8_percent_encode, AsciiSet, CONTROLS};

use crate::clean_urls::{compute_clean_urls_redirect, try_clean_urls_resolve, CleanUrlsView};
use crate::compression;
use crate::config::ServeConfig;
use crate::custom_headers::{apply_custom_headers, HeaderRuleCompiled};
use crate::error::error_response;
use crate::etag::compute_etag;
use crate::last_modified::last_modified_value;
use crate::listing::{
    render as render_listing, DirectoryListingView, RenderResult, UnlistedFilter,
};
use crate::mime::mime_for;
use crate::normalize::collapse_slashes;
use crate::range;
use crate::redirects::{compute_configured_redirects, RedirectRuleCompiled};
use crate::resolve::{resolve, ResolveOutcome};
use crate::rewrites::{compute_configured_rewrites, RewriteRuleCompiled};
use crate::trailing_slash::compute_trailing_slash_redirect;

// The 13-phase dispatcher pulls in one precompiled view per
// capability (cleanUrls, directoryListing, unlisted, redirects,
// rewrites, headers) plus the request, root, and live config —
// pushing the arity over clippy's default-7 threshold. Bundling
// into a context struct is a future-stage refactor candidate; a
// `Stage 7+` factor-out would also collapse the parallel
// `compile_*` setup boilerplate in `server.rs`.
#[allow(clippy::too_many_arguments)]
pub async fn dispatch(
    req: Request<Body>,
    root: &Path,
    serve_config: &ServeConfig,
    clean_urls_view: &CleanUrlsView,
    listing_view: &DirectoryListingView,
    unlisted_filter: &UnlistedFilter,
    redirect_rules: &[RedirectRuleCompiled],
    rewrite_rules: &[RewriteRuleCompiled],
    header_rules: &[HeaderRuleCompiled],
) -> Response<Body> {
    // dispatch_inner returns (response, headers_path):
    //
    // - `Some(path)` — caller (this wrapper) applies `apply_custom_headers`
    //   with that path. Used for plain success / 405 paths whose
    //   reference equivalent flows through `getHeaders` at the success
    //   site (`serve-handler/src/index.js:746`) and/or where the
    //   matching path is the request path itself.
    // - `None` — headers either are not applicable (3xx redirects per
    //   `index.js:586-588` bypass `getHeaders`) or have already been
    //   applied per-branch inside `error_response` (Codex review round
    //   1 P1: JSON errors skip; custom `<status>.html` matches against
    //   the page path; fallback HTML matches against the request path).
    let (response, headers_path) = dispatch_inner(
        req,
        root,
        serve_config,
        clean_urls_view,
        listing_view,
        unlisted_filter,
        redirect_rules,
        rewrite_rules,
        header_rules,
    )
    .await;
    match headers_path {
        Some(p) => apply_custom_headers(response, &p, header_rules),
        None => response,
    }
}

#[allow(clippy::too_many_arguments)]
async fn dispatch_inner(
    req: Request<Body>,
    root: &Path,
    serve_config: &ServeConfig,
    clean_urls_view: &CleanUrlsView,
    listing_view: &DirectoryListingView,
    unlisted_filter: &UnlistedFilter,
    redirect_rules: &[RedirectRuleCompiled],
    rewrite_rules: &[RewriteRuleCompiled],
    header_rules: &[HeaderRuleCompiled],
) -> (Response<Body>, Option<String>) {
    // Phase 1–2: method gate. 405 carries the request's raw URI path
    // forward so that `apply_custom_headers` matches against it
    // (decode hasn't run yet).
    //
    // OPTIONS flows through the static pipeline like GET/HEAD per
    // SRV-CORS-001 — the reference (`serve-handler/src/index.js`)
    // never inspects `request.method`, so an `OPTIONS /asset.css`
    // walks phases 3..13 and yields the same response shape (200 +
    // file body / 304 / 404 / etc.) plus the `--cors` overlay
    // applied post-dispatch. No D-NNN: mirror semantics, not
    // adaptation.
    if !matches!(*req.method(), Method::GET | Method::HEAD | Method::OPTIONS) {
        let resp = Response::builder()
            .status(StatusCode::METHOD_NOT_ALLOWED)
            .body(Body::empty())
            .expect("405 response should always build");
        return (resp, Some(req.uri().path().to_string()));
    }

    // Decode the URI path once at the dispatcher entry. Mirrors the
    // reference's `decodedPath` invariant (`serve-handler/src/index.js:561`).
    // Subsequent phases operate on this decoded form so that encoded
    // forms like `%2F%2F` collapse identically to literal `//`.
    //
    // Phase-1 strict syntactic + UTF-8 gate (SRV-SEC-002): a malformed
    // `%xx` OR a byte sequence that decodes to invalid UTF-8 yields 400,
    // mirroring `decodeURIComponent`'s URIError branch at
    // `index.js:561-567`. Short-circuit BEFORE any phase 2+ runs. The
    // 400 response goes through `error_response` which applies custom
    // headers per-branch internally; the `None` headers_path tells the
    // wrapper not to apply twice.
    let raw_path = req.uri().path().to_string();
    let decoded_path = match try_percent_decode(&raw_path) {
        Ok(p) => p,
        Err(_) => {
            // Reference's URIError catch passes `'/'` as absolutePath
            // to sendError (`index.js:563`), so the fallback HTML
            // `getHeaders` call runs against a degenerate relative
            // path that no realistic user rule intentionally targets.
            // We mirror that absence with `skip_fallback_headers=true`,
            // but the custom `<status>.html` branch still applies user
            // rules — reference invokes `getHeaders(.., errorPage,
            // stats)` at `index.js:508` whether or not the URIError
            // path was outside root. Codex review round 3 P1.
            let resp = error_response(
                StatusCode::BAD_REQUEST,
                req.headers(),
                root,
                header_rules,
                &raw_path,
                true,
            )
            .await;
            return (resp, None);
        }
    };

    // Phase-10 lexical containment check (SRV-SEC-001). Mirrors the
    // reference's `isPathInside(path.join(current, relativePath), current)`
    // gate at `serve-handler/src/index.js:570-580`: a `..` segment that
    // would pop above the served root yields 400, before any filesystem
    // I/O. The check is purely lexical to match `path.posix.join`'s
    // normalization (e.g. leading `//` collapses to `/` and is benign).
    //
    // Empirically, reference's fallback `getHeaders(.., absolutePath,
    // null)` for traversal 400 fails to match common rules like `**`
    // because `path.relative(current, outsideAbsolutePath)` produces a
    // `../`-prefixed string whose slasher-normalized form is brittle
    // (D-015). Skip the fallback branch's header application, but
    // keep the custom `<status>.html` branch on — Codex review round 3
    // P1: a user-supplied `400.html` MUST still receive its rule-set
    // headers (reference's `getHeaders(.., errorPage, stats)` at
    // `index.js:508` runs regardless).
    if lexical_path_escapes_root(&decoded_path) {
        let resp = error_response(
            StatusCode::BAD_REQUEST,
            req.headers(),
            root,
            header_rules,
            &decoded_path,
            true,
        )
        .await;
        return (resp, None);
    }

    // Phase 4: cleanUrls 301 (SRV-ROUT-001). Runs on the decoded
    // (uncollapsed) path, before phase 5, matching `shouldRedirect`'s
    // ordering at `serve-handler/src/index.js:121-143`. Wins over
    // trailingSlash, redirects, rewrites, and the existing-file
    // pre-stat (per SRV-ROUT-006 scenario "cleanUrls 301 wins over
    // existing-file short-circuit"). 3xx redirects skip custom-header
    // application — reference's redirect path at `index.js:586-588`
    // builds the response via `response.writeHead(redirect.statusCode,
    // { Location: ... })` without going through `getHeaders`.
    if let Some(target) = compute_clean_urls_redirect(&decoded_path, clean_urls_view) {
        return (redirect_301(&target), None);
    }

    // Phase 5: trailingSlash 301 (SRV-ROUT-003 / SRV-ROUT-004), with the
    // multi-slash override from SRV-ROUT-005 (`index.js:158-160`).
    // Operates on the decoded (uncollapsed) path so the override
    // trigger is the input's `//` content.
    if let Some(target) =
        compute_trailing_slash_redirect(&decoded_path, serve_config.trailing_slash)
    {
        return (redirect_301(&target), None);
    }

    // Phase 3: silent multi-slash collapse for resolve-and-onwards
    // stages (SRV-ROUT-005, Q-006 closed). Pure pre-routing transform;
    // never emits a redirect on its own — when `trailingSlash` is set,
    // the redirect for `//` is emitted by phase 5 above.
    let url_path = collapse_slashes(&decoded_path);

    // Phase 6: configured redirects (Stage 6d). First-match-wins
    // iteration over the precompiled rules, mirroring `shouldRedirect`'s
    // redirects branch at `serve-handler/src/index.js:172-182`. The
    // status code defaults to 301 when the rule has no `type` override
    // (`index.js:179`, `statusCode: type || defaultType`).
    if let Some((target, status)) = compute_configured_redirects(&url_path, redirect_rules) {
        return (redirect_with_status(&target, status), None);
    }
    // Phase 7: configured rewrites + `--single` SPA fallback
    // (Stage 6e). Phase 7 interleaves with phases 8/9 per the
    // pre-stat asymmetry mandated by `openspec/specs/rewrites/spec.md`
    // and mirrored from `serve-handler/src/index.js:608-642`:
    //
    //   * Has-extension + file exists → pre-stat short-circuits;
    //     rewrites are NEVER consulted (`index.js:608-616`).
    //   * Has-extension + file missing → apply rewrites; the
    //     rewritten path's resolution wins over `<P>.html` cleanUrls
    //     candidates (mirrors `findRelated`'s
    //     `rewrittenPath ? [rewrittenPath] : getPossiblePaths(...)`
    //     branch).
    //   * Extensionless → apply rewrites unconditionally; a matching
    //     rewrite serves its destination even when the extensionless
    //     original exists as a regular file (per spec
    //     `openspec/specs/rewrites/spec.md:27-29`).
    //
    // Phase 8 (cleanUrls resolution, SRV-ROUT-002) and phase 9
    // (final resolve) are wired below. Phase 8 only runs as the
    // post-rewrite fallback when no rewrite matched — when a rewrite
    // matches, only the rewritten path is tried (mirrors
    // `findRelated`'s "rewrittenPath wins, no further candidates"
    // semantics).
    let url_has_extension = url_path_has_extension(&url_path);

    // `lexical_url` tracks the URL form of the path that ultimately
    // resolved (or attempted to resolve). For successes it feeds
    // `apply_custom_headers` as the matching path, mirroring
    // reference's `path.relative(current, absolutePath)` lookup inside
    // `getHeaders` (where `absolutePath` was updated by `findRelated`
    // to the resolved candidate's lexical path, NOT canonicalized).
    // Codex review round 3 P2: the prior implementation used the
    // canonicalized `PathBuf`, which on Windows folds `/ASSET.CSS`'s
    // case to `/asset.css` and admits matches that reference's
    // case-sensitive minimatch rejects.
    let url_path_str: String = url_path.clone().into_owned();
    let trimmed = url_path_str.trim_start_matches('/').trim_end_matches('/');
    let cleanurls_index_url = if trimmed.is_empty() {
        "/index.html".to_string()
    } else {
        format!("/{}/index.html", trimmed)
    };
    let cleanurls_flat_url = if trimmed.is_empty() {
        // try_clean_urls_resolve never produces a `<P>.html` candidate
        // for the empty-P root; this string is unreachable as a
        // lexical-url winner, but we set it for completeness.
        url_path_str.clone()
    } else {
        format!("/{}.html", trimmed)
    };

    let (outcome, lexical_url): (ResolveOutcome, String) = if !url_has_extension {
        // Extensionless. Apply rewrites first; if matched, resolve
        // the rewritten path. If the rewritten path doesn't resolve,
        // fall back to the ORIGINAL path's resolve — mirrors the
        // reference's final `lstat(absolutePath)` at
        // `serve-handler/src/index.js:634-642` after `findRelated`
        // returns null. The fallback is the original path ONLY (no
        // cleanUrls candidates).
        if let Some(target) = compute_configured_rewrites(&url_path, rewrite_rules) {
            match resolve(&target, root).await {
                // Stage 6g review round 1 P1 fix: a rewrite that
                // resolves to a directory propagates as a successful
                // resolution to that directory. Reference at
                // `serve-handler/src/index.js:644` sends any final
                // `stats.isDirectory()` into `renderDirectory`,
                // including stats produced by the rewrite-target
                // path. Only NotFound / EscapedRoot trigger the
                // original-path fallback.
                ResolveOutcome::NotFound | ResolveOutcome::EscapedRoot => {
                    (resolve(&url_path, root).await, url_path_str.clone())
                }
                other => (other, target),
            }
        } else {
            // No rewrite matched. The reference's `findRelated` here
            // consults cleanUrls candidates (`<P>/index.html` then
            // `<P>.html`) for extensionless requests. The variant
            // distinguishes which candidate succeeded:
            //   ResolveOutcome::Index → `<P>/index.html`
            //   ResolveOutcome::File  → `<P>.html`
            match try_clean_urls_resolve(&url_path, root, clean_urls_view).await {
                Some(o @ ResolveOutcome::Index(_)) => (o, cleanurls_index_url.clone()),
                Some(o @ ResolveOutcome::File(_)) => (o, cleanurls_flat_url.clone()),
                Some(o) => (o, url_path_str.clone()),
                None => (resolve(&url_path, root).await, url_path_str.clone()),
            }
        }
    } else {
        // Has-extension. Pre-stat the original path first; if it
        // resolves, serve it (rewrites NEVER consulted). On miss,
        // apply rewrites; if a rewrite matched, resolve its
        // destination. If no rewrite matched, fall back to the
        // existing cleanUrls candidate.
        match resolve(&url_path, root).await {
            // Stage 6g review round 1 P1 fix: only `NotFound`
            // triggers the rewrite-then-cleanUrls fallback chain.
            // A directory resolution propagates as the successful
            // outcome — reference at
            // `serve-handler/src/index.js:644` sends any final
            // `stats.isDirectory()` into `renderDirectory`, even
            // when the request URL had a `.txt`-style suffix and
            // the directory's literal name carries that extension.
            ResolveOutcome::NotFound => {
                if let Some(target) = compute_configured_rewrites(&url_path, rewrite_rules) {
                    let r = resolve(&target, root).await;
                    (r, target)
                } else {
                    match try_clean_urls_resolve(&url_path, root, clean_urls_view).await {
                        Some(o @ ResolveOutcome::Index(_)) => (o, cleanurls_index_url.clone()),
                        Some(o @ ResolveOutcome::File(_)) => (o, cleanurls_flat_url.clone()),
                        Some(o) => (o, url_path_str.clone()),
                        None => (ResolveOutcome::NotFound, url_path_str.clone()),
                    }
                }
            }
            other => (other, url_path_str.clone()),
        }
    };

    match outcome {
        ResolveOutcome::File(p) | ResolveOutcome::Index(p) => {
            // SRV-CACHE-002 / Stage 7b slice 2: metadata is fetched
            // alongside the file bytes so `build_file_or_304` can
            // emit `Last-Modified` from the file's mtime under
            // `etag: false`. A stat failure here gracefully degrades
            // to "no Last-Modified header"; a read failure still
            // surfaces as 404 below. The metadata call is the
            // cheaper of the two and races with the read only on
            // pathological concurrent-modification timing — the
            // mtime might lag the bytes by a microsecond, which is
            // immaterial at IMF-fixdate's whole-second resolution.
            let meta = tokio::fs::metadata(&p).await.ok();
            match tokio::fs::read(&p).await {
                Ok(bytes) => (
                    // SRV-CACHE-001 / Codex round 1 P1: 304 check must see
                    // user-merged headers, so `build_file_or_304` runs the
                    // headers overlay itself and we return `None` here to
                    // tell the outer `dispatch` wrapper "headers pass is
                    // done." Mirrors reference's getHeaders-then-304
                    // ordering at `serve-handler/src/index.js:241, 760`.
                    build_file_or_304(
                        serve_config,
                        req.headers(),
                        req.method(),
                        &p,
                        bytes,
                        meta.as_ref(),
                        header_rules,
                        &lexical_url,
                    ),
                    None,
                ),
                Err(_) => {
                    let resp = error_response(
                        StatusCode::NOT_FOUND,
                        req.headers(),
                        root,
                        header_rules,
                        &decoded_path,
                        false,
                    )
                    .await;
                    (resp, None)
                }
            }
        }
        // Stage 6g phase 11: directory listing branch. Mirrors
        // `serve-handler/src/index.js:325-374, 644-680`:
        //
        //   * If `directoryListing` scope rejects AND `renderSingle`
        //     is off, return 404 (`index.js:336` early-`return {}`).
        //   * Otherwise read the directory; if `renderSingle` is on
        //     AND the raw entry count is exactly 1 AND that entry is
        //     a file, serve the file directly. Reference reroutes
        //     this through the file-serving site at `index.js:746`,
        //     so custom headers DO apply (matched against the file's
        //     URL).
        //   * Otherwise render the listing (HTML or JSON per Accept)
        //     after `unlisted` filtering. Listing responses bypass
        //     `apply_custom_headers` — reference returns BEFORE the
        //     `getHeaders` site (`index.js:644-672`), the same way
        //     3xx redirects skip headers.
        ResolveOutcome::Directory(absolute) => {
            let render_single = serve_config.render_single.unwrap_or(false);
            let listing_applicable = listing_view.applicable(&decoded_path);
            if !listing_applicable && !render_single {
                let resp = error_response(
                    StatusCode::NOT_FOUND,
                    req.headers(),
                    root,
                    header_rules,
                    &decoded_path,
                    false,
                )
                .await;
                return (resp, None);
            }
            match render_listing(
                &absolute,
                &decoded_path,
                root,
                req.headers(),
                unlisted_filter,
                render_single,
            )
            .await
            {
                Ok(RenderResult::Direct(resp)) => {
                    // When `directoryListing` is off but `renderSingle`
                    // is on, we still ran the renderer to give the
                    // count-of-1 short-circuit a chance to fire. If
                    // it didn't (count != 1, or the lone entry is a
                    // directory), the listing must NOT be emitted —
                    // mirrors reference's `applicable + renderSingle`
                    // at `serve-handler/src/index.js:336-374` where
                    // the post-loop `return {directory: output}`
                    // path is reached only when listing IS
                    // applicable.
                    if listing_applicable {
                        (resp, None)
                    } else {
                        let resp = error_response(
                            StatusCode::NOT_FOUND,
                            req.headers(),
                            root,
                            header_rules,
                            &decoded_path,
                            false,
                        )
                        .await;
                        (resp, None)
                    }
                }
                Ok(RenderResult::Single { path, bytes }) => {
                    // SRV-CACHE-002 / Stage 7b slice 2: same metadata
                    // fetch as the File/Index arm. The renderSingle
                    // path read bytes via the listing renderer
                    // (`listing::render`) and never stat'd the file;
                    // we re-stat here so `build_file_or_304` has the
                    // mtime under `etag: false`. The renderer could
                    // surface metadata via its return type later if
                    // the extra syscall becomes a hot-path concern.
                    let meta = tokio::fs::metadata(&path).await.ok();
                    // Headers-path mirrors reference's `getHeaders(..,
                    // absolutePath, stats)` at `index.js:746`. After
                    // a rewrite (`/old → /docs`), reference overrode
                    // `absolutePath` to the rewritten file's resolved
                    // path at `index.js:649-665`, then `getHeaders`
                    // ran on it — so a rule for `/docs/photo.png`
                    // matches and `/old/photo.png` does not.
                    //
                    // Build the headers-path from `lexical_url` (the
                    // URL form the dispatcher tracked through the
                    // rewrite/resolve chain) plus the file's basename.
                    // `lexical_url` is the rewrite TARGET when a
                    // rewrite fired (e.g. `/docs`) and the request
                    // URL otherwise (preserving raw user-supplied
                    // casing on Windows: `/MEDIA/` stays `/MEDIA/`).
                    // Codex review round 2 P2 used the canonicalized
                    // FS path here, which on Windows folded `/MEDIA/`
                    // to `/media/` and admitted matches the reference
                    // would reject — round 3 P2 reverts to the
                    // lexical form.
                    let prefix = if lexical_url.ends_with('/') {
                        lexical_url.clone()
                    } else {
                        format!("{}/", lexical_url)
                    };
                    let filename = path.file_name().and_then(|s| s.to_str()).unwrap_or("");
                    let url = format!("{prefix}{filename}");
                    (
                        // Codex round 1 P1: same rationale as the
                        // File/Index arm above — `build_file_or_304`
                        // applies user `headers` rules itself so the
                        // 304 check sees the merged ETag.
                        build_file_or_304(
                            serve_config,
                            req.headers(),
                            req.method(),
                            &path,
                            bytes,
                            meta.as_ref(),
                            header_rules,
                            &url,
                        ),
                        None,
                    )
                }
                Err(_) => {
                    let resp = error_response(
                        StatusCode::NOT_FOUND,
                        req.headers(),
                        root,
                        header_rules,
                        &decoded_path,
                        false,
                    )
                    .await;
                    (resp, None)
                }
            }
        }
        ResolveOutcome::NotFound => {
            let resp = error_response(
                StatusCode::NOT_FOUND,
                req.headers(),
                root,
                header_rules,
                &decoded_path,
                false,
            )
            .await;
            (resp, None)
        }
        // Defense-in-depth: the lexical check at phase 1 already
        // short-circuits `..`-escaping requests; this branch covers
        // symlink/canonicalize-based escapes. Skip the fallback HTML
        // branch's header application (mirrors lexical-escape) but
        // keep the custom `<status>.html` branch on — same reasoning
        // as the lexical-escape site (Codex review round 3 P1).
        ResolveOutcome::EscapedRoot => {
            let resp = error_response(
                StatusCode::BAD_REQUEST,
                req.headers(),
                root,
                header_rules,
                &decoded_path,
                true,
            )
            .await;
            (resp, None)
        }
    }
}

/// Mirror of Node's `path.extname(p)` for our URL-path use case.
///
/// Node's `path.posix.extname` strips trailing slashes from `p`
/// before computing the basename, so `path.posix.extname('/foo.txt/')`
/// returns `'.txt'` (NOT empty). Codex review round 1 P2: the
/// previous implementation used `rsplit('/').next()` which yields
/// the empty trailing segment, mis-classifying trailing-slash paths
/// as extensionless and letting rewrites fire on directory-like
/// requests where the reference does not.
///
/// An extension is a non-empty `.xxx` substring after the last `/`
/// (post-trailing-slash trim) that is NOT the leading character of
/// the basename. Dotfiles (`/.bashrc`) have no extension; nested
/// dotfiles with extensions (`/.bashrc.bak`) do.
fn url_path_has_extension(path: &str) -> bool {
    let basename = match path.rsplit('/').find(|b| !b.is_empty()) {
        Some(b) => b,
        None => return false, // root or all-empty
    };
    // A leading-dot basename whose ONLY dot is the leading one has no
    // extension. Skip char index 0 when scanning for an extension dot.
    basename.char_indices().skip(1).any(|(_, ch)| ch == '.')
}

/// Single-pass URL decode with strict syntactic AND UTF-8 validation.
/// Returns `Err` if any `%` is not followed by exactly two ASCII-hex
/// chars, OR if the resulting byte sequence is not valid UTF-8.
///
/// Reference: `serve-handler/src/index.js:561-567` —
/// `try { relativePath = decodeURIComponent(...) } catch (URIError) { 400 }`.
/// `decodeURIComponent` throws URIError on both malformed `%xx` syntax
/// AND on byte sequences that are valid escapes but invalid UTF-8
/// (e.g. `/%FF` — a single 0xFF byte that does not start a valid
/// UTF-8 sequence). Codex review round 1 P2 surfaced that the prior
/// `decode_utf8_lossy()` silently mapped invalid UTF-8 to U+FFFD,
/// admitting requests that the reference rejects with 400.
fn try_percent_decode(s: &str) -> Result<Cow<'_, str>, ()> {
    let bytes = s.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' {
            if i + 2 >= bytes.len() || !is_ascii_hex(bytes[i + 1]) || !is_ascii_hex(bytes[i + 2]) {
                return Err(());
            }
            i += 3;
        } else {
            i += 1;
        }
    }
    percent_decode_str(s).decode_utf8().map_err(|_| ())
}

fn is_ascii_hex(b: u8) -> bool {
    b.is_ascii_hexdigit()
}

/// Returns true if the URL path's `..` segments pop above the served
/// root. Mirrors the reference's lexical check via
/// `path.posix.join(root, decoded).startsWith(root)`: empty segments
/// (from `//` runs) and `.` segments are no-ops; `..` decrements depth
/// and below-zero depth is the escape signal. Does no filesystem I/O.
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

/// Mirrors JavaScript's `encodeURI` (the function the reference applies
/// to redirect targets at `serve-handler/src/index.js:586`):
///
/// * Unreserved (kept as-is): `A-Z a-z 0-9 - _ . ! ~ * ' ( )`
/// * Reserved (kept as-is): `; , / ? : @ & = + $ #`
/// * Encoded: SPACE `"` `%` `<` `>` `\` `^` `` ` `` `{` `|` `}` `[` `]`
///   + control chars + every non-ASCII byte (each UTF-8 byte → `%xx`).
const ENCODE_URI_SET: &AsciiSet = &CONTROLS
    .add(b' ')
    .add(b'"')
    .add(b'%')
    .add(b'<')
    .add(b'>')
    .add(b'\\')
    .add(b'^')
    .add(b'`')
    .add(b'{')
    .add(b'|')
    .add(b'}')
    .add(b'[')
    .add(b']');

pub(crate) fn encode_uri_target(target: &str) -> String {
    utf8_percent_encode(target, ENCODE_URI_SET).to_string()
}

fn redirect_301(target: &str) -> Response<Body> {
    redirect_with_status(target, 301)
}

/// Build a 3xx redirect response with the given status code. Mirrors
/// `serve-handler/src/index.js:586-587` which writes
/// `{Location: encodeURI(redirect.target)}` with whatever status the
/// rule resolved to. Out-of-range or otherwise invalid status codes
/// fall back to 301; this mirrors the "accept any 3xx; range-checking
/// is not specified" stance of the spec
/// (`openspec/specs/redirects/spec.md` SRV-RDIR-002 Note).
fn redirect_with_status(target: &str, status: u16) -> Response<Body> {
    let encoded = encode_uri_target(target);
    let location =
        HeaderValue::from_str(&encoded).unwrap_or_else(|_| HeaderValue::from_static("/"));
    let status_code = StatusCode::from_u16(status).unwrap_or(StatusCode::MOVED_PERMANENTLY);
    Response::builder()
        .status(status_code)
        .header(LOCATION, location)
        .body(Body::empty())
        .expect("redirect response should always build")
}

fn file_response(
    path: &Path,
    bytes: Vec<u8>,
    etag: Option<HeaderValue>,
    last_modified: Option<HeaderValue>,
) -> Response<Body> {
    let mut builder = Response::builder().status(StatusCode::OK);
    if let Some(mime) = mime_for(path) {
        builder = builder.header(CONTENT_TYPE, HeaderValue::from_static(mime));
    }
    // SRV-CACHE-001 / SRV-CACHE-002: reference emits exactly one of
    // ETag / Last-Modified per file response — the mutex is enforced
    // by `etag_value` and `last_modified_value` returning opposite-
    // gated `Some`. We still write whichever value(s) the caller
    // hands us so that user `headers` rules (applied later by
    // `apply_custom_headers`) can override or supplement them.
    if let Some(etag) = etag {
        builder = builder.header(ETAG, etag);
    }
    if let Some(last_modified) = last_modified {
        builder = builder.header(LAST_MODIFIED, last_modified);
    }
    builder
        .body(Body::from(bytes))
        .expect("file response should always build")
}

// SRV-CACHE-001 / D-017 / D3 (plan 0015): irserve's CLI default is
// `etag=true`, matching `vercel/serve/source/main.ts` which sets
// `config.etag = !args['--no-etag']` before invoking the handler. Only
// an explicit `"etag": false` in `serve.json` disables the default-
// generated ETag — user `headers` rules can still set `ETag` and a 304
// fires on a match against the user-supplied value, mirroring the
// reference's gate-then-`Object.assign` flow at
// `serve-handler/src/index.js:227-241`. To suppress ETag emission
// entirely, set `"etag": false` AND ensure no `headers` rule sets it.
// Hash formula mirrors `serve-handler/src/index.js:24-36`.
fn etag_value(serve_config: &ServeConfig, path: &Path, bytes: &[u8]) -> Option<HeaderValue> {
    if serve_config.etag == Some(false) {
        return None;
    }
    HeaderValue::from_str(&compute_etag(path, bytes)).ok()
}

// SRV-CACHE-001 + SRV-CACHE-003 (D-018): build either a 200 file
// response or a 304 short-circuit, mirroring
// `serve-handler/src/index.js:227-254` + `:760-765` for the ETag/INM
// path and extending with an If-Modified-Since path on the
// `Last-Modified` branch (irserve-only adaptation per D-018; the
// reference has no IMS handling — `tools/probe/snapshots/last-modified-roundtrip.json`
// pins the 200-with-full-body behavior under `target=reference`).
//
// User `serve.json#headers` rules CAN override the default ETag /
// Last-Modified — the reference's flow is `getHeaders` (defaults +
// customHeaders merged via `Object.assign(defaultHeaders, related)`
// at `index.js:241`), THEN the 304 check at `:760` reads the MERGED
// `headers.ETag`. Codex round 1 P1 (Stage 7a) reshaped the irserve
// helper to match this ordering; the Last-Modified branch reuses
// the same merge-then-decide structure.
//
// ETag/INM 304 fires iff (a) the merged response carries an `ETag`
// header — either the default sha1 (when `serve_config.etag !=
// Some(false)`) OR one supplied by a user `headers` rule, even when
// `etag: false` disabled the default (Codex round 2 P3, Stage 7a);
// (b) the request carries no `Range` header (Stage 7c precursor —
// mirrors `req.headers.range == null` at `index.js:760`); and (c)
// the request's `If-None-Match` matches the merged response's
// `ETag` verbatim (strong-quoted string equality; no weak/strong
// distinction, no comma-list, no `*`).
//
// Last-Modified/IMS 304 (D-018, Stage 7b slice 3, irserve-only)
// fires iff (a') `serve_config.etag == Some(false)` (Codex round 1
// P2 — the gate narrows D-018's scope to the `--no-etag` /
// `etag: false` path; under ETag-on, IMS is ignored even when a
// user `headers` rule supplied a `Last-Modified` on the merged
// response) AND (b) above AND (d) the merged response carries a
// `Last-Modified` header (the default emission under `etag: false`,
// optionally overridden by a user `headers` rule) AND (e) the
// request's `If-Modified-Since` parses as an HTTP-date AND the
// merged `Last-Modified` also parses AND (f) `IMS >= merged_LM`
// (whole-second comparison naturally falls out of `httpdate`'s
// round-trip: the formatter writes whole-second IMF-fixdate; the
// parser reads it back as a `SystemTime` at that whole-second).
// Malformed IMS is treated as absent (no 304), mirroring RFC 9111
// §13.1.3 recipient guidance. The IMS check reads the MERGED value
// so under `etag: false` a user-supplied `Last-Modified` override
// drives the decision.
//
// 304 response carries no body, no `Content-Type`, no `ETag` /
// `Last-Modified` echo — mirrors reference's
// `response.statusCode = 304; response.end()` (no `writeHead`).
#[allow(clippy::too_many_arguments)]
fn build_file_or_304(
    serve_config: &ServeConfig,
    req_headers: &HeaderMap,
    req_method: &Method,
    path: &Path,
    bytes: Vec<u8>,
    meta: Option<&Metadata>,
    header_rules: &[HeaderRuleCompiled],
    request_path: &str,
) -> Response<Body> {
    // SRV-CACHE-004 (Stage 7c): clone the file bytes ONLY when a
    // Range header is present, so the no-Range path keeps its
    // single-allocation shape. The clone is needed because
    // `file_response` takes `bytes: Vec<u8>` by value (moves it
    // into the body); `range::apply` needs the original bytes
    // back to slice for 206 or to retransmit for 416.
    let total = bytes.len() as u64;
    let range_value = req_headers.get(RANGE).cloned();
    let bytes_for_range = range_value.as_ref().map(|_| bytes.clone());

    let etag = etag_value(serve_config, path, &bytes);
    let last_modified = last_modified_value(serve_config, meta);
    // Snapshot the pre-compression bytes for `compression::maybe_apply`
    // below. We can't recover them from the `Body` after `file_response`
    // moves them in, and the Range branch already takes its own clone
    // above (`bytes_for_range`).
    let bytes_for_compression = bytes.clone();
    let response_200 = file_response(path, bytes, etag, last_modified);
    let merged = apply_custom_headers(response_200, request_path, header_rules);

    let Some(range_value) = range_value else {
        // No Range — try 304 short-circuits, else fall through to 200.
        // SRV-CACHE-001: ETag/INM 304 path (Stage 7a).
        if let (Some(inm), Some(effective_etag)) =
            (req_headers.get(IF_NONE_MATCH), merged.headers().get(ETAG))
        {
            if inm == effective_etag {
                return not_modified_response();
            }
        }
        // SRV-CACHE-003 / D-018: Last-Modified/IMS 304 path
        // (Stage 7b slice 3, irserve-only). Codex round 1 P2:
        // gated on `serve_config.etag == Some(false)`. When ETag
        // is on (default or `Some(true)`), IMS is ignored even
        // if a user `serve.json#headers` rule supplied a
        // `Last-Modified` on the merged response — the 304 trigger
        // under ETag-on is exclusively `If-None-Match` per
        // SRV-CACHE-001. This narrows D-018's scope to the
        // `--no-etag` / `etag: false` path (matches the inventory
        // rule for SRV-CACHE-003 and the plan's "Decisions taken
        // without asking" entry — the prior symmetric-with-ETag
        // implementation widened the divergence from reference
        // unnecessarily).
        if serve_config.etag == Some(false) {
            if let (Some(ims_raw), Some(lm_raw)) = (
                req_headers.get(IF_MODIFIED_SINCE),
                merged.headers().get(LAST_MODIFIED),
            ) {
                if let (Ok(ims_str), Ok(lm_str)) = (ims_raw.to_str(), lm_raw.to_str()) {
                    if let (Ok(ims), Ok(lm)) = (
                        httpdate::parse_http_date(ims_str),
                        httpdate::parse_http_date(lm_str),
                    ) {
                        if ims >= lm {
                            return not_modified_response();
                        }
                    }
                }
            }
        }
        // SRV-CLI-012 (Stage 7e): compression negotiation. Slots in
        // AFTER the 304 short-circuits (no point compressing a body
        // we won't send) and AFTER `apply_custom_headers` so the
        // final merged Content-Type / Cache-Control drive the
        // compressible / no-transform decisions. Range pre-empts
        // compression — the `let Some(range_value)` tail below
        // bypasses this call when a Range header is present, mirroring
        // the reference's middleware ordering (compression sees the
        // 206 body framing as opaque and skips).
        return compression::maybe_apply(
            merged,
            &bytes_for_compression,
            req_headers,
            req_method,
            serve_config,
        );
    };

    // SRV-CACHE-004: Range present → emit 206 (in-range) or 416
    // (out-of-range, full body retained per the reference). Range
    // pre-empts the 304 short-circuit per `serve-handler/src/index.js:760`
    // (`request.headers.range == null && ...`); the let-else above
    // ensures we never reach the 304 branches under Range.
    //
    // Codex round 1 P1: mirror reference's
    // `if (request.headers.range && stats.size)` guard at
    // `serve-handler/src/index.js:720` — when the file is zero bytes,
    // the reference skips Range parsing entirely and returns the
    // normal (empty-body) 200 with no `Content-Range`. Previously
    // irserve always entered `range::apply` and `total == 0` is
    // pinned as `Unsatisfiable`, so a Range request against a 0-byte
    // file produced 416 + `Content-Range: bytes */0` — a wire-
    // observable divergence. Falling through to `merged` here mirrors
    // the reference's normal 200 path; the 304 short-circuits stay
    // bypassed for Range-bearing requests as before (we are inside
    // the `let Some(range_value)` tail, so the let-else `else` arm
    // never runs).
    if total == 0 {
        return merged;
    }
    let bytes_for_range = bytes_for_range.expect("bytes cloned when range present");
    range::apply(merged, &range_value, &bytes_for_range, total)
}

fn not_modified_response() -> Response<Body> {
    Response::builder()
        .status(StatusCode::NOT_MODIFIED)
        .body(Body::empty())
        .expect("304 response should always build")
}

#[cfg(test)]
mod tests {
    use super::{build_file_or_304, encode_uri_target, url_path_has_extension, Method};
    use crate::config::{HeaderItem, HeaderRule, ServeConfig};
    use crate::custom_headers::{compile_rules, HeaderRuleCompiled};
    use axum::http::header::{
        HeaderValue, ETAG, IF_MODIFIED_SINCE, IF_NONE_MATCH, LAST_MODIFIED, RANGE,
    };
    use axum::http::{HeaderMap, StatusCode};
    use std::path::PathBuf;

    fn cfg_etag(value: Option<bool>) -> ServeConfig {
        ServeConfig {
            etag: value,
            ..ServeConfig::default()
        }
    }

    // The reference ETag for `body{color:red}\n` named `asset.css`,
    // pinned by the `etag-roundtrip` probe and the unit test in
    // `etag.rs`. Reused here to drive 304 round-trips.
    const ASSET_CSS_ETAG: &str = "\"3638b78821a961fcf35969f0bc67cc5944d64a0b\"";
    const ASSET_CSS_BYTES: &[u8] = b"body{color:red}\n";

    fn asset_path() -> PathBuf {
        PathBuf::from("asset.css")
    }

    fn compile_etag_override(source: &str, value: Option<&str>) -> Vec<HeaderRuleCompiled> {
        let rules = vec![HeaderRule {
            source: source.to_string(),
            headers: vec![HeaderItem {
                key: "ETag".to_string(),
                value: value.map(|s| s.to_string()),
            }],
        }];
        let (compiled, invalid) = compile_rules(&rules);
        assert!(invalid.is_empty(), "rule should compile: {invalid:?}");
        compiled
    }

    fn compile_last_modified_override(
        source: &str,
        value: Option<&str>,
    ) -> Vec<HeaderRuleCompiled> {
        let rules = vec![HeaderRule {
            source: source.to_string(),
            headers: vec![HeaderItem {
                key: "Last-Modified".to_string(),
                value: value.map(|s| s.to_string()),
            }],
        }];
        let (compiled, invalid) = compile_rules(&rules);
        assert!(invalid.is_empty(), "rule should compile: {invalid:?}");
        compiled
    }

    #[test]
    fn etag_match_returns_304() {
        let cfg = cfg_etag(None);
        let mut h = HeaderMap::new();
        h.insert(IF_NONE_MATCH, HeaderValue::from_static(ASSET_CSS_ETAG));
        let resp = build_file_or_304(
            &cfg,
            &h,
            &Method::GET,
            &asset_path(),
            ASSET_CSS_BYTES.to_vec(),
            None,
            &[],
            "/asset.css",
        );
        assert_eq!(resp.status(), StatusCode::NOT_MODIFIED);
        // 304 response carries no ETag echo and no Content-Type —
        // mirrors `serve-handler/src/index.js:761-764` which calls
        // `response.statusCode = 304; response.end()` without going
        // through `writeHead(headers)`.
        assert!(resp.headers().get(ETAG).is_none());
        assert!(resp
            .headers()
            .get(axum::http::header::CONTENT_TYPE)
            .is_none());
    }

    #[test]
    fn etag_mismatch_returns_200_with_etag() {
        let cfg = cfg_etag(None);
        let mut h = HeaderMap::new();
        h.insert(IF_NONE_MATCH, HeaderValue::from_static("\"deadbeef\""));
        let resp = build_file_or_304(
            &cfg,
            &h,
            &Method::GET,
            &asset_path(),
            ASSET_CSS_BYTES.to_vec(),
            None,
            &[],
            "/asset.css",
        );
        assert_eq!(resp.status(), StatusCode::OK);
        assert_eq!(resp.headers().get(ETAG).unwrap(), ASSET_CSS_ETAG);
    }

    #[test]
    fn range_present_skips_304_even_on_match() {
        // SRV-CACHE-001 + SRV-CACHE-004: reference's
        // `serve-handler/src/index.js:760` guards the 304 check with
        // `request.headers.range == null`. Stage 7c now emits 206
        // when the Range is satisfiable — the test pins that the 304
        // short-circuit is pre-empted and a partial body comes back
        // instead.
        let cfg = cfg_etag(None);
        let mut h = HeaderMap::new();
        h.insert(IF_NONE_MATCH, HeaderValue::from_static(ASSET_CSS_ETAG));
        h.insert(RANGE, HeaderValue::from_static("bytes=0-3"));
        let resp = build_file_or_304(
            &cfg,
            &h,
            &Method::GET,
            &asset_path(),
            ASSET_CSS_BYTES.to_vec(),
            None,
            &[],
            "/asset.css",
        );
        assert_eq!(resp.status(), StatusCode::PARTIAL_CONTENT);
        assert_eq!(
            resp.headers()
                .get(axum::http::header::CONTENT_RANGE)
                .unwrap(),
            "bytes 0-3/16",
        );
    }

    #[test]
    fn etag_disabled_never_304() {
        let cfg = cfg_etag(Some(false));
        let mut h = HeaderMap::new();
        h.insert(IF_NONE_MATCH, HeaderValue::from_static(ASSET_CSS_ETAG));
        let resp = build_file_or_304(
            &cfg,
            &h,
            &Method::GET,
            &asset_path(),
            ASSET_CSS_BYTES.to_vec(),
            None,
            &[],
            "/asset.css",
        );
        assert_eq!(resp.status(), StatusCode::OK);
        // No ETag header when disabled.
        assert!(resp.headers().get(ETAG).is_none());
    }

    /// SRV-CACHE-002 / Stage 7b slice 2: under `etag: false`, when
    /// the dispatcher passes a real `Metadata`, the 200 response
    /// carries `Last-Modified` (IMF-fixdate shape) and no `ETag`.
    /// Mirrors the `else` branch at
    /// `serve-handler/src/index.js:234-236`. IMS handling is slice 3.
    #[test]
    fn etag_off_emits_last_modified_from_meta() {
        let cfg = cfg_etag(Some(false));
        let tmp = tempfile::NamedTempFile::new().expect("tempfile");
        let meta = tmp.as_file().metadata().expect("metadata");
        let h = HeaderMap::new();
        let resp = build_file_or_304(
            &cfg,
            &h,
            &Method::GET,
            &asset_path(),
            ASSET_CSS_BYTES.to_vec(),
            Some(&meta),
            &[],
            "/asset.css",
        );
        assert_eq!(resp.status(), StatusCode::OK);
        assert!(resp.headers().get(ETAG).is_none());
        let lm = resp
            .headers()
            .get(axum::http::header::LAST_MODIFIED)
            .expect("Last-Modified header")
            .to_str()
            .expect("ascii");
        // IMF-fixdate shape sanity (full string is volatile across
        // runs; the underlying formatter is unit-tested in
        // last_modified.rs).
        assert!(lm.ends_with(" GMT"));
        assert_eq!(lm.len(), 29);
    }

    /// Mutex: ETag-on path emits no `Last-Modified` even when
    /// `Metadata` is available. Mirrors the `if (etag)` branch at
    /// `serve-handler/src/index.js:227-233` — the helper sets
    /// `defaultHeaders['ETag']` and never reaches the LM assignment.
    #[test]
    fn etag_on_suppresses_last_modified_even_with_meta() {
        let cfg = cfg_etag(None); // default → ETag on
        let tmp = tempfile::NamedTempFile::new().expect("tempfile");
        let meta = tmp.as_file().metadata().expect("metadata");
        let h = HeaderMap::new();
        let resp = build_file_or_304(
            &cfg,
            &h,
            &Method::GET,
            &asset_path(),
            ASSET_CSS_BYTES.to_vec(),
            Some(&meta),
            &[],
            "/asset.css",
        );
        assert_eq!(resp.status(), StatusCode::OK);
        assert!(resp.headers().get(ETAG).is_some());
        assert!(resp.headers().get(LAST_MODIFIED).is_none());
    }

    // ---- Stage 7b slice 3: SRV-CACHE-003 / D-018 IMS 304 tests ----
    //
    // Reference returns 200 for every IMS variant under `--no-etag`
    // (closure of Q-009 by `tools/probe/snapshots/last-modified-roundtrip.json`).
    // irserve adapts (D-018) to a 304 short-circuit when
    // `If-Modified-Since` ≥ merged `Last-Modified`, mirroring the
    // ETag/INM round-trip shape: no body, no `Content-Type`, no
    // `Last-Modified` echo. The merged-LM comparison (rather than
    // raw mtime) keeps the path symmetric with ETag/INM so a user
    // `headers` rule that overrides `Last-Modified` also drives the
    // 304 decision.

    /// IMS = the LM the response just emitted → 304. Round-trip via
    /// first-call-then-replay mirrors how a real client interacts
    /// with the server (it sends back what it received).
    #[test]
    fn ims_exact_match_returns_304() {
        let cfg = cfg_etag(Some(false));
        let tmp = tempfile::NamedTempFile::new().expect("tempfile");
        let meta = tmp.as_file().metadata().expect("metadata");

        let first = build_file_or_304(
            &cfg,
            &HeaderMap::new(),
            &Method::GET,
            &asset_path(),
            ASSET_CSS_BYTES.to_vec(),
            Some(&meta),
            &[],
            "/asset.css",
        );
        let captured_lm = first
            .headers()
            .get(LAST_MODIFIED)
            .expect("first response should carry Last-Modified")
            .clone();

        let mut h = HeaderMap::new();
        h.insert(IF_MODIFIED_SINCE, captured_lm);
        let resp = build_file_or_304(
            &cfg,
            &h,
            &Method::GET,
            &asset_path(),
            ASSET_CSS_BYTES.to_vec(),
            Some(&meta),
            &[],
            "/asset.css",
        );
        assert_eq!(resp.status(), StatusCode::NOT_MODIFIED);
        // Mirror ETag 304 shape: no echo, no Content-Type.
        assert!(resp.headers().get(LAST_MODIFIED).is_none());
        assert!(resp
            .headers()
            .get(axum::http::header::CONTENT_TYPE)
            .is_none());
    }

    /// Client's cached copy is from the future relative to file
    /// mtime → 304. Static far-future IMS sidesteps mtime sensitivity.
    #[test]
    fn ims_future_returns_304() {
        let cfg = cfg_etag(Some(false));
        let tmp = tempfile::NamedTempFile::new().expect("tempfile");
        let meta = tmp.as_file().metadata().expect("metadata");
        let mut h = HeaderMap::new();
        h.insert(
            IF_MODIFIED_SINCE,
            HeaderValue::from_static("Thu, 01 Jan 2099 00:00:00 GMT"),
        );
        let resp = build_file_or_304(
            &cfg,
            &h,
            &Method::GET,
            &asset_path(),
            ASSET_CSS_BYTES.to_vec(),
            Some(&meta),
            &[],
            "/asset.css",
        );
        assert_eq!(resp.status(), StatusCode::NOT_MODIFIED);
    }

    /// Client's cached copy is from before file mtime → 200 with
    /// full body and `Last-Modified`. Epoch IMS suffices.
    #[test]
    fn ims_past_returns_200_with_last_modified() {
        let cfg = cfg_etag(Some(false));
        let tmp = tempfile::NamedTempFile::new().expect("tempfile");
        let meta = tmp.as_file().metadata().expect("metadata");
        let mut h = HeaderMap::new();
        h.insert(
            IF_MODIFIED_SINCE,
            HeaderValue::from_static("Thu, 01 Jan 1970 00:00:00 GMT"),
        );
        let resp = build_file_or_304(
            &cfg,
            &h,
            &Method::GET,
            &asset_path(),
            ASSET_CSS_BYTES.to_vec(),
            Some(&meta),
            &[],
            "/asset.css",
        );
        assert_eq!(resp.status(), StatusCode::OK);
        assert!(resp.headers().get(LAST_MODIFIED).is_some());
        assert!(resp.headers().get(ETAG).is_none()); // mutex
    }

    /// RFC 9111 §13.1.3: recipients SHOULD treat unparseable IMS as
    /// absent. Reference is inert too (no branch). Either way, 200.
    #[test]
    fn ims_malformed_returns_200() {
        let cfg = cfg_etag(Some(false));
        let tmp = tempfile::NamedTempFile::new().expect("tempfile");
        let meta = tmp.as_file().metadata().expect("metadata");
        let mut h = HeaderMap::new();
        h.insert(IF_MODIFIED_SINCE, HeaderValue::from_static("not-a-date"));
        let resp = build_file_or_304(
            &cfg,
            &h,
            &Method::GET,
            &asset_path(),
            ASSET_CSS_BYTES.to_vec(),
            Some(&meta),
            &[],
            "/asset.css",
        );
        assert_eq!(resp.status(), StatusCode::OK);
    }

    /// SRV-CACHE-004 + SRV-CACHE-003: a `Range` request suppresses
    /// ALL 304 short-circuits, ETag/INM and Last-Modified/IMS alike.
    /// Mirrors `serve-handler/src/index.js:760` (`request.headers.range
    /// == null`). Stage 7c lands the actual 206 emission — this test
    /// now pins partial-content output, not a fall-through 200.
    #[test]
    fn range_present_skips_ims_304_even_on_match() {
        let cfg = cfg_etag(Some(false));
        let tmp = tempfile::NamedTempFile::new().expect("tempfile");
        let meta = tmp.as_file().metadata().expect("metadata");
        let mut h = HeaderMap::new();
        h.insert(
            IF_MODIFIED_SINCE,
            HeaderValue::from_static("Thu, 01 Jan 2099 00:00:00 GMT"),
        );
        h.insert(RANGE, HeaderValue::from_static("bytes=0-3"));
        let resp = build_file_or_304(
            &cfg,
            &h,
            &Method::GET,
            &asset_path(),
            ASSET_CSS_BYTES.to_vec(),
            Some(&meta),
            &[],
            "/asset.css",
        );
        assert_eq!(resp.status(), StatusCode::PARTIAL_CONTENT);
    }

    /// A user `serve.json#headers` rule with `Last-Modified: null`
    /// (the SRV-HDR-002 prune form) deletes the default header from
    /// the merged response. With no `Last-Modified` to compare
    /// against, IMS cannot fire a 304. Symmetric with the
    /// Stage 7a Codex round 1 P1 fix for `ETag: null`.
    #[test]
    fn user_last_modified_null_rule_suppresses_304() {
        let cfg = cfg_etag(Some(false));
        let tmp = tempfile::NamedTempFile::new().expect("tempfile");
        let meta = tmp.as_file().metadata().expect("metadata");
        let rules = compile_last_modified_override("**", None);
        let mut h = HeaderMap::new();
        h.insert(
            IF_MODIFIED_SINCE,
            HeaderValue::from_static("Thu, 01 Jan 2099 00:00:00 GMT"),
        );
        let resp = build_file_or_304(
            &cfg,
            &h,
            &Method::GET,
            &asset_path(),
            ASSET_CSS_BYTES.to_vec(),
            Some(&meta),
            &rules,
            "/asset.css",
        );
        assert_eq!(resp.status(), StatusCode::OK);
        assert!(resp.headers().get(LAST_MODIFIED).is_none());
    }

    /// Codex round 1 P2: under default ETag (i.e. `etag == None`
    /// or `Some(true)`), `If-Modified-Since` is ignored even when
    /// a user `serve.json#headers` rule supplies a `Last-Modified`
    /// on the merged response. The 304 trigger under ETag-on is
    /// exclusively `If-None-Match` per SRV-CACHE-001. This pins
    /// the `serve_config.etag == Some(false)` gate that narrows
    /// D-018's scope to the `--no-etag` / `etag: false` path,
    /// matching the inventory rule for SRV-CACHE-003 and the
    /// plan's Decisions-taken-without-asking entry. Pre-fix
    /// (slice-3 symmetric impl) returned 304 here.
    #[test]
    fn etag_on_ignores_ims_even_with_user_lm_rule() {
        let cfg = cfg_etag(None); // default → ETag on
        let tmp = tempfile::NamedTempFile::new().expect("tempfile");
        let meta = tmp.as_file().metadata().expect("metadata");
        let custom = "Mon, 01 Jan 2001 00:00:00 GMT";
        let rules = compile_last_modified_override("**", Some(custom));
        let mut h = HeaderMap::new();
        // IMS = custom: under the symmetric impl this would 304;
        // under the gated impl it must return 200 (ETag-on path).
        h.insert(IF_MODIFIED_SINCE, HeaderValue::from_static(custom));
        let resp = build_file_or_304(
            &cfg,
            &h,
            &Method::GET,
            &asset_path(),
            ASSET_CSS_BYTES.to_vec(),
            Some(&meta),
            &rules,
            "/asset.css",
        );
        assert_eq!(resp.status(), StatusCode::OK);
        // Both validators present on the merged response (default
        // ETag emission + user LM rule); only INM would drive 304
        // here, not IMS.
        assert!(resp.headers().get(ETAG).is_some());
        assert_eq!(
            resp.headers()
                .get(LAST_MODIFIED)
                .unwrap()
                .to_str()
                .unwrap(),
            custom
        );
    }

    /// A user `headers` rule that overrides `Last-Modified` drives
    /// the 304 decision against the override value (not the raw
    /// mtime). Mirrors the ETag-override-drives-304 invariant from
    /// Stage 7a round 1 P1: the contract is "the client got X in
    /// the response; if X ≥ IMS, 304." Tested by setting LM to a
    /// far-past date so the file's own mtime is irrelevant; only
    /// the override matters.
    #[test]
    fn user_last_modified_override_drives_304_decision() {
        let cfg = cfg_etag(Some(false));
        let tmp = tempfile::NamedTempFile::new().expect("tempfile");
        let meta = tmp.as_file().metadata().expect("metadata");
        let custom = "Mon, 01 Jan 2001 00:00:00 GMT";
        let rules = compile_last_modified_override("**", Some(custom));

        // IMS == custom → 304.
        let mut h_match = HeaderMap::new();
        h_match.insert(IF_MODIFIED_SINCE, HeaderValue::from_static(custom));
        let r_match = build_file_or_304(
            &cfg,
            &h_match,
            &Method::GET,
            &asset_path(),
            ASSET_CSS_BYTES.to_vec(),
            Some(&meta),
            &rules,
            "/asset.css",
        );
        assert_eq!(r_match.status(), StatusCode::NOT_MODIFIED);

        // IMS < custom → 200 (client cache predates the override).
        let mut h_old = HeaderMap::new();
        h_old.insert(
            IF_MODIFIED_SINCE,
            HeaderValue::from_static("Sun, 01 Jan 1995 00:00:00 GMT"),
        );
        let r_old = build_file_or_304(
            &cfg,
            &h_old,
            &Method::GET,
            &asset_path(),
            ASSET_CSS_BYTES.to_vec(),
            Some(&meta),
            &rules,
            "/asset.css",
        );
        assert_eq!(r_old.status(), StatusCode::OK);
        // Merged LM should be the override, not the default mtime.
        let lm = r_old
            .headers()
            .get(LAST_MODIFIED)
            .expect("LM")
            .to_str()
            .unwrap();
        assert_eq!(lm, custom);
    }

    #[test]
    fn no_if_none_match_returns_200() {
        let cfg = cfg_etag(None);
        let h = HeaderMap::new();
        let resp = build_file_or_304(
            &cfg,
            &h,
            &Method::GET,
            &asset_path(),
            ASSET_CSS_BYTES.to_vec(),
            None,
            &[],
            "/asset.css",
        );
        assert_eq!(resp.status(), StatusCode::OK);
        assert_eq!(resp.headers().get(ETAG).unwrap(), ASSET_CSS_ETAG);
    }

    /// Codex round 1 P1: a user `serve.json#headers` rule that sets
    /// `ETag: "custom"` must take effect BEFORE the 304 decision.
    /// Replay of the custom value must 304; replay of the default
    /// sha1 must NOT 304 (the merged response no longer advertises
    /// it). Mirrors `serve-handler/src/index.js:241, 760` ordering.
    #[test]
    fn custom_etag_override_drives_304_decision() {
        let cfg = cfg_etag(None);
        let rules = compile_etag_override("**/*.css", Some("\"custom\""));

        // (a) replay of the user-override value → 304.
        let mut h_custom = HeaderMap::new();
        h_custom.insert(IF_NONE_MATCH, HeaderValue::from_static("\"custom\""));
        let r_custom = build_file_or_304(
            &cfg,
            &h_custom,
            &Method::GET,
            &asset_path(),
            ASSET_CSS_BYTES.to_vec(),
            None,
            &rules,
            "/asset.css",
        );
        assert_eq!(r_custom.status(), StatusCode::NOT_MODIFIED);

        // (b) replay of the default sha1 (which the user rule has
        // overwritten on the wire) → 200, not 304. The merged ETag
        // is `"custom"`, so the default sha1 no longer matches.
        let mut h_default = HeaderMap::new();
        h_default.insert(IF_NONE_MATCH, HeaderValue::from_static(ASSET_CSS_ETAG));
        let r_default = build_file_or_304(
            &cfg,
            &h_default,
            &Method::GET,
            &asset_path(),
            ASSET_CSS_BYTES.to_vec(),
            None,
            &rules,
            "/asset.css",
        );
        assert_eq!(r_default.status(), StatusCode::OK);
        assert_eq!(r_default.headers().get(ETAG).unwrap(), "\"custom\"");
    }

    /// Codex round 2 P3: `"etag": false` in `serve.json` disables only
    /// the DEFAULT-generated ETag. A user `headers` rule keyed by
    /// `ETag` still lands on the response and still drives the 304
    /// decision, mirroring the reference's gate-then-`Object.assign`
    /// flow at `serve-handler/src/index.js:227-241` + `:760`.
    #[test]
    fn etag_false_still_honors_custom_rule_and_304() {
        let cfg = cfg_etag(Some(false));
        let rules = compile_etag_override("**/*.css", Some("\"custom\""));

        // (a) first GET: merged response carries the user-supplied
        //     ETag, NOT the default sha1 (which was suppressed).
        let h_empty = HeaderMap::new();
        let r_first = build_file_or_304(
            &cfg,
            &h_empty,
            &Method::GET,
            &asset_path(),
            ASSET_CSS_BYTES.to_vec(),
            None,
            &rules,
            "/asset.css",
        );
        assert_eq!(r_first.status(), StatusCode::OK);
        assert_eq!(r_first.headers().get(ETAG).unwrap(), "\"custom\"");

        // (b) replay of the user-supplied ETag still 304s, because the
        //     304 check runs against the merged response's ETag.
        let mut h_custom = HeaderMap::new();
        h_custom.insert(IF_NONE_MATCH, HeaderValue::from_static("\"custom\""));
        let r_replay = build_file_or_304(
            &cfg,
            &h_custom,
            &Method::GET,
            &asset_path(),
            ASSET_CSS_BYTES.to_vec(),
            None,
            &rules,
            "/asset.css",
        );
        assert_eq!(r_replay.status(), StatusCode::NOT_MODIFIED);
    }

    /// Codex round 1 P1: when a user rule deletes ETag via
    /// `value: null` (SRV-HDR-002), the 304 short-circuit must NEVER
    /// fire — the merged response has no ETag header to compare.
    #[test]
    fn custom_etag_delete_disables_304() {
        let cfg = cfg_etag(None);
        let rules = compile_etag_override("**/*.css", None);

        let mut h = HeaderMap::new();
        h.insert(IF_NONE_MATCH, HeaderValue::from_static(ASSET_CSS_ETAG));
        let resp = build_file_or_304(
            &cfg,
            &h,
            &Method::GET,
            &asset_path(),
            ASSET_CSS_BYTES.to_vec(),
            None,
            &rules,
            "/asset.css",
        );
        assert_eq!(resp.status(), StatusCode::OK);
        assert!(resp.headers().get(ETAG).is_none());
    }

    #[test]
    fn extension_basic_paths() {
        assert!(url_path_has_extension("/foo.txt"));
        assert!(url_path_has_extension("/dir/foo.css"));
        assert!(!url_path_has_extension("/about"));
        assert!(!url_path_has_extension("/dir/sub"));
    }

    #[test]
    fn extension_root_and_empty() {
        assert!(!url_path_has_extension("/"));
        assert!(!url_path_has_extension(""));
    }

    #[test]
    fn extension_dotfile_no_ext() {
        // `.bashrc` is a dotfile; Node's path.extname returns ''.
        assert!(!url_path_has_extension("/.bashrc"));
    }

    #[test]
    fn extension_nested_dotfile_with_ext() {
        // `.bashrc.bak` has extension `.bak`.
        assert!(url_path_has_extension("/.bashrc.bak"));
    }

    #[test]
    fn extension_trailing_slash_preserves_extension() {
        // Codex round 1 P2: Node's `path.extname('/foo.txt/')` ===
        // '.txt' because trailing slashes are trimmed before
        // computing the basename. The previous implementation used
        // `rsplit('/').next()` which captured the empty trailing
        // segment and mis-classified `/foo.txt/` as extensionless,
        // letting rewrites fire on directory-like requests where
        // the reference does not.
        assert!(url_path_has_extension("/foo.txt/"));
        assert!(url_path_has_extension("/dir/foo.css/"));
    }

    #[test]
    fn extension_trailing_slash_extensionless_stays_extensionless() {
        assert!(!url_path_has_extension("/about/"));
        assert!(!url_path_has_extension("/dir/sub/"));
    }

    #[test]
    fn encode_keeps_safe_ascii_path() {
        assert_eq!(encode_uri_target("/about/"), "/about/");
        assert_eq!(encode_uri_target("/api/v1/users"), "/api/v1/users");
    }

    #[test]
    fn encode_preserves_query_and_reserved_chars() {
        // encodeURI keeps ? = & : @ + $ , # / ; intact.
        assert_eq!(encode_uri_target("/path?a=1&b=2"), "/path?a=1&b=2");
        assert_eq!(
            encode_uri_target("/with:colons@and+pluses,$dollars#frag"),
            "/with:colons@and+pluses,$dollars#frag"
        );
    }

    #[test]
    fn encode_space_to_percent_20() {
        assert_eq!(encode_uri_target("/foo bar/"), "/foo%20bar/");
    }

    #[test]
    fn encode_non_ascii_to_utf8_bytes() {
        // 'é' is U+00E9 → UTF-8 bytes C3 A9.
        assert_eq!(encode_uri_target("/café/"), "/caf%C3%A9/");
        // 'д' is U+0434 → UTF-8 bytes D0 B4.
        assert_eq!(
            encode_uri_target("/привет"),
            "/%D0%BF%D1%80%D0%B8%D0%B2%D0%B5%D1%82"
        );
    }

    #[test]
    fn encode_literal_percent_becomes_percent_25() {
        // A literal `%` left over after decode (e.g. `%25` in the input
        // path) must be re-encoded to `%25` to match `encodeURI`.
        assert_eq!(encode_uri_target("/100%off/"), "/100%25off/");
    }

    #[test]
    fn encode_brackets_and_quotes() {
        assert_eq!(encode_uri_target("/[x]/\"y\""), "/%5Bx%5D/%22y%22");
    }

    #[test]
    fn encode_control_chars() {
        // Newline (0x0A) is a control character.
        assert_eq!(encode_uri_target("/a\nb"), "/a%0Ab");
    }

    // ---- SRV-CACHE-004 (Stage 7c): Range emission through build_file_or_304 ----
    //
    // The Range parser + apply helper is unit-tested in `range::tests`; these
    // integration tests exercise the dispatch-level seam: the bytes-cloning
    // guard, the let-else that pre-empts the 304 short-circuits when Range
    // is present, and the interaction with user `headers` rules and the 7b
    // IMS branch under `etag: false`.

    fn range_header(value: &'static str) -> HeaderMap {
        let mut h = HeaderMap::new();
        h.insert(RANGE, HeaderValue::from_static(value));
        h
    }

    #[test]
    fn range_in_range_returns_206() {
        let resp = build_file_or_304(
            &cfg_etag(None),
            &range_header("bytes=0-3"),
            &Method::GET,
            &asset_path(),
            ASSET_CSS_BYTES.to_vec(),
            None,
            &[],
            "/asset.css",
        );
        assert_eq!(resp.status(), StatusCode::PARTIAL_CONTENT);
        assert_eq!(
            resp.headers()
                .get(axum::http::header::CONTENT_RANGE)
                .unwrap(),
            "bytes 0-3/16",
        );
        assert_eq!(
            resp.headers()
                .get(axum::http::header::CONTENT_LENGTH)
                .unwrap(),
            "4",
        );
    }

    #[test]
    fn range_tail_returns_206() {
        let resp = build_file_or_304(
            &cfg_etag(None),
            &range_header("bytes=8-"),
            &Method::GET,
            &asset_path(),
            ASSET_CSS_BYTES.to_vec(),
            None,
            &[],
            "/asset.css",
        );
        assert_eq!(resp.status(), StatusCode::PARTIAL_CONTENT);
        assert_eq!(
            resp.headers()
                .get(axum::http::header::CONTENT_RANGE)
                .unwrap(),
            "bytes 8-15/16",
        );
    }

    #[test]
    fn range_suffix_returns_206() {
        let resp = build_file_or_304(
            &cfg_etag(None),
            &range_header("bytes=-4"),
            &Method::GET,
            &asset_path(),
            ASSET_CSS_BYTES.to_vec(),
            None,
            &[],
            "/asset.css",
        );
        assert_eq!(resp.status(), StatusCode::PARTIAL_CONTENT);
        assert_eq!(
            resp.headers()
                .get(axum::http::header::CONTENT_RANGE)
                .unwrap(),
            "bytes 12-15/16",
        );
    }

    #[test]
    fn range_out_of_range_returns_416() {
        let resp = build_file_or_304(
            &cfg_etag(None),
            &range_header("bytes=999-1000"),
            &Method::GET,
            &asset_path(),
            ASSET_CSS_BYTES.to_vec(),
            None,
            &[],
            "/asset.css",
        );
        assert_eq!(resp.status(), StatusCode::RANGE_NOT_SATISFIABLE);
        assert_eq!(
            resp.headers()
                .get(axum::http::header::CONTENT_RANGE)
                .unwrap(),
            "bytes */16",
        );
    }

    #[test]
    fn range_with_ims_match_under_no_etag_returns_206_not_304() {
        // 7b interaction: under `--no-etag`, IMS matching normally
        // 304s. Range pre-empts that branch (mirrors reference's
        // L760 guard which we generalised to BOTH 304 paths via the
        // let-else in build_file_or_304).
        let cfg = cfg_etag(Some(false));
        let tmp = tempfile::NamedTempFile::new().expect("tempfile");
        let meta = tmp.as_file().metadata().expect("metadata");

        // Capture the Last-Modified first.
        let first = build_file_or_304(
            &cfg,
            &HeaderMap::new(),
            &Method::GET,
            &asset_path(),
            ASSET_CSS_BYTES.to_vec(),
            Some(&meta),
            &[],
            "/asset.css",
        );
        let captured_lm = first
            .headers()
            .get(LAST_MODIFIED)
            .expect("Last-Modified")
            .clone();

        // Replay with IMS + Range.
        let mut h = HeaderMap::new();
        h.insert(IF_MODIFIED_SINCE, captured_lm);
        h.insert(RANGE, HeaderValue::from_static("bytes=0-3"));
        let resp = build_file_or_304(
            &cfg,
            &h,
            &Method::GET,
            &asset_path(),
            ASSET_CSS_BYTES.to_vec(),
            Some(&meta),
            &[],
            "/asset.css",
        );
        // Range pre-empts the IMS 304 — partial body comes back.
        assert_eq!(resp.status(), StatusCode::PARTIAL_CONTENT);
    }

    #[test]
    fn range_preserves_etag_on_206() {
        // ETag rides on 206 — Range parsing runs AFTER
        // `apply_custom_headers`, so the default ETag set by
        // `file_response` survives.
        let resp = build_file_or_304(
            &cfg_etag(None),
            &range_header("bytes=0-3"),
            &Method::GET,
            &asset_path(),
            ASSET_CSS_BYTES.to_vec(),
            None,
            &[],
            "/asset.css",
        );
        assert_eq!(resp.status(), StatusCode::PARTIAL_CONTENT);
        assert_eq!(resp.headers().get(ETAG).unwrap(), ASSET_CSS_ETAG);
    }

    #[test]
    fn range_preserves_user_custom_headers_on_206() {
        // A user `headers` rule setting an `ETag` override on `.css`
        // assets must survive the 206 transformation — Range parsing
        // is downstream of `apply_custom_headers`.
        let rules = compile_etag_override("**/*.css", Some("\"custom\""));
        let resp = build_file_or_304(
            &cfg_etag(None),
            &range_header("bytes=0-3"),
            &Method::GET,
            &asset_path(),
            ASSET_CSS_BYTES.to_vec(),
            None,
            &rules,
            "/asset.css",
        );
        assert_eq!(resp.status(), StatusCode::PARTIAL_CONTENT);
        assert_eq!(resp.headers().get(ETAG).unwrap(), "\"custom\"");
    }

    #[test]
    fn range_416_preserves_etag_and_user_headers() {
        // 416 also rides on the merged response: the full body comes
        // back, but ETag and user-rule headers survive.
        let rules = compile_etag_override("**/*.css", Some("\"custom\""));
        let resp = build_file_or_304(
            &cfg_etag(None),
            &range_header("bytes=999-1000"),
            &Method::GET,
            &asset_path(),
            ASSET_CSS_BYTES.to_vec(),
            None,
            &rules,
            "/asset.css",
        );
        assert_eq!(resp.status(), StatusCode::RANGE_NOT_SATISFIABLE);
        assert_eq!(resp.headers().get(ETAG).unwrap(), "\"custom\"");
    }

    #[test]
    fn range_on_empty_file_returns_200_not_416() {
        // Codex round 1 P1: reference's
        // `if (request.headers.range && stats.size)` guard at
        // `serve-handler/src/index.js:720` skips Range processing
        // when the file is zero bytes, returning the normal 200 with
        // an empty body and no `Content-Range`. irserve mirrors via
        // the `total == 0 → return merged` guard in
        // `build_file_or_304` right before the `range::apply` call.
        let resp = build_file_or_304(
            &cfg_etag(None),
            &range_header("bytes=0-3"),
            &Method::GET,
            &asset_path(),
            Vec::new(),
            None,
            &[],
            "/empty.txt",
        );
        assert_eq!(resp.status(), StatusCode::OK);
        assert!(resp
            .headers()
            .get(axum::http::header::CONTENT_RANGE)
            .is_none());
    }

    #[test]
    fn range_416_user_content_range_rule_wins() {
        // Codex round 1 P2: user `Content-Range` override survives
        // the 416 transformation (mirrors reference's setHeader-
        // before-getHeaders ordering at `index.js:730-732, 746, 767`).
        // Threads through `apply_custom_headers` from a
        // `serve.json#headers` rule with key `Content-Range`.
        let rules = {
            use crate::config::{HeaderItem, HeaderRule};
            use crate::custom_headers::compile_rules;
            let rules = vec![HeaderRule {
                source: "**/*.css".to_string(),
                headers: vec![HeaderItem {
                    key: "Content-Range".to_string(),
                    value: Some("custom-value".to_string()),
                }],
            }];
            let (compiled, invalid) = compile_rules(&rules);
            assert!(invalid.is_empty());
            compiled
        };
        let resp = build_file_or_304(
            &cfg_etag(None),
            &range_header("bytes=999-1000"),
            &Method::GET,
            &asset_path(),
            ASSET_CSS_BYTES.to_vec(),
            None,
            &rules,
            "/asset.css",
        );
        assert_eq!(resp.status(), StatusCode::RANGE_NOT_SATISFIABLE);
        assert_eq!(
            resp.headers()
                .get(axum::http::header::CONTENT_RANGE)
                .unwrap(),
            "custom-value",
        );
    }

    #[test]
    fn range_trailing_garbage_value_emits_206() {
        // Codex round 1 P2: parseInt-style leniency — `bytes=0-3x`
        // parses as 0-3.
        let resp = build_file_or_304(
            &cfg_etag(None),
            &range_header("bytes=0-3x"),
            &Method::GET,
            &asset_path(),
            ASSET_CSS_BYTES.to_vec(),
            None,
            &[],
            "/asset.css",
        );
        assert_eq!(resp.status(), StatusCode::PARTIAL_CONTENT);
        assert_eq!(
            resp.headers()
                .get(axum::http::header::CONTENT_RANGE)
                .unwrap(),
            "bytes 0-3/16",
        );
    }

    #[test]
    fn range_user_last_modified_delete_under_no_etag_still_emits_206() {
        // Belt-and-suspenders: even when a user rule deletes the
        // default `Last-Modified` (so the 7b IMS branch would never
        // 304 anyway), a Range request still emits 206 — the
        // let-else in build_file_or_304 doesn't depend on validator
        // presence.
        let rules = compile_last_modified_override("**/*.css", None);
        let resp = build_file_or_304(
            &cfg_etag(Some(false)),
            &range_header("bytes=0-3"),
            &Method::GET,
            &asset_path(),
            ASSET_CSS_BYTES.to_vec(),
            None,
            &rules,
            "/asset.css",
        );
        assert_eq!(resp.status(), StatusCode::PARTIAL_CONTENT);
        assert!(resp.headers().get(LAST_MODIFIED).is_none());
    }
}
