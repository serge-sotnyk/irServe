//! Phase-13 custom headers (post-dispatch).
//!
//! Mirrors `serve-handler/src/index.js:194-254` (`getHeaders`):
//! - For each rule, `sourceMatches(source, slasher(relativePath))`
//!   tests the request path against the rule's source pattern.
//! - Multiple matching rules accumulate headers in iteration order
//!   (`index.js:200-210` — the loop never `break`s).
//! - Final merge over default response headers is case-insensitive
//!   last-write-wins (`index.js:245`, `Object.assign(defaultHeaders,
//!   related)`).
//! - `value: null` (SRV-HDR-002) deletes a previously-applied header
//!   per the merged-map prune at `index.js:247-251`. IrServe folds
//!   the accumulate+prune into a single insert/remove pass per item
//!   (last-write-wins per key reproduces the same final state as
//!   the reference's two-stage `Object.assign`-then-prune).
//!
//! Per-branch contract (mirrors `sendError`'s call sites at
//! `index.js:467-524` and the success-site `getHeaders` call at
//! `index.js:746`):
//!
//! - **200 success** — apply, matched against the FINAL resolved
//!   file path (post-cleanUrls / post-rewrite), tracked in
//!   `dispatch_inner` as `lexical_url`.
//! - **JSON-preferring error** — SKIP. Reference returns at
//!   `index.js:477-487` before reaching `getHeaders`.
//! - **HTML error with custom `<status>.html`** — apply, matched
//!   against `/<status>.html` (mirrors `getHeaders(.., errorPage,
//!   stats)` at `index.js:508`). Custom Content-Type from a user
//!   rule may override the default `text/html`.
//! - **HTML error fallback (no custom page)** — apply, matched
//!   against the request path, EXCEPT when the error came from a
//!   path-traversal / malformed-decode 400 (the lexical-escape and
//!   URIError sites pass `skip_fallback_headers=true`). The
//!   fallback then forces `Content-Type: text/html; charset=utf-8`
//!   AFTER the apply pass (mirrors `index.js:520`), so a user rule
//!   cannot override the fallback's content type.
//! - **3xx redirect** — SKIP. Reference's redirect path at
//!   `index.js:586-588` builds the response via
//!   `response.writeHead(redirect.statusCode, { Location: ... })`
//!   without going through `getHeaders`. `apply_custom_headers`
//!   short-circuits on `response.status().is_redirection()`.
//!
//! Note on SRV-HDR-002: the public `serve` CLI's JSON Schema
//! (`@zeit/schemas/deployment/config-static.js`) declares
//! `value: { type: 'string', minLength: 1 }`, rejecting `value: null`
//! at config-load time. So while `serve-handler`'s library code
//! implements null-pruning, it is unreachable through the reference
//! CLI we probe against. IrServe accepts `value: null` as documented
//! and verifies the prune logic via this module's unit tests rather
//! than an oracle probe.

use axum::body::Body;
use axum::http::{HeaderName, HeaderValue, Response};

use crate::config::{HeaderItem, HeaderRule};
use crate::path_pattern::{
    classify_pattern_segment, has_glob_meta, match_segments, path_posix_resolve, slasher,
    CompileError, PatSeg,
};

/// Headers-specific source matcher. Mirrors `serve-handler/src/index.js:38-67`'s
/// `sourceMatches(source, requestPath)` call WITHOUT `allowSegments`
/// — the call site at `index.js:207` is `sourceMatches(source,
/// slasher(relativePath))`, no third argument, so the path-to-regexp
/// branch at `index.js:45-57` is skipped and only `minimatch` runs.
///
/// Codex review round 2 P1: the prior implementation reused
/// `path_pattern::Matcher::compile_no_segments`, which still routed
/// no-glob sources into `Matcher::Literal::try_match`. That method's
/// first comparison is case-INSENSITIVE (path-to-regexp's default
/// `i` flag), so source `/Case` incorrectly matched request `/case`.
/// `HeaderMatcher` re-implements the minimatch-only branch with
/// case-sensitive literal equality (minimatch's default `nocase: false`).
#[derive(Debug)]
enum HeaderMatcher {
    /// Source has no glob meta-characters and no leading `!`. Matches
    /// only exact, case-sensitive equality with `path_posix_resolve`'d
    /// request path.
    Literal(String),
    /// Source has `*`/`?`/`[`/`{` glob meta or `!`-prefix. Matches via
    /// the per-segment minimatch kernel shared with redirects/rewrites
    /// (`PatSeg`/`match_segments`) — applied to the slashed source
    /// segments and the resolved request path's segments.
    Glob {
        segments: Vec<PatSeg>,
        negate: bool,
    },
}

impl HeaderMatcher {
    fn compile(source: &str) -> Result<Self, CompileError> {
        let slashed = slasher(source);
        let (negate, body) = match slashed.strip_prefix('!') {
            Some(rest) => (true, rest.to_string()),
            None => (false, slashed),
        };
        if has_glob_meta(&body) || negate {
            let segments: Result<Vec<PatSeg>, globset::Error> = body
                .split('/')
                .filter(|s| !s.is_empty())
                .map(classify_pattern_segment)
                .collect();
            Ok(HeaderMatcher::Glob {
                segments: segments?,
                negate,
            })
        } else {
            // Literal source. `:name` lands here too — minimatch sees
            // `:` as a regular char, so `/api/:id` matches only the
            // literal request path `/api/:id`.
            Ok(HeaderMatcher::Literal(body))
        }
    }

    fn matches(&self, path: &str) -> bool {
        // `path.posix.resolve(requestPath)` at `index.js:41` collapses
        // `..` segments and trailing slashes before pattern comparison.
        let resolved = path_posix_resolve(path);
        match self {
            // Case-sensitive equality mirrors minimatch's default
            // `nocase: false`. Codex review round 2 P1.
            HeaderMatcher::Literal(src) => src.as_str() == resolved.as_str(),
            HeaderMatcher::Glob { segments, negate } => {
                let path_segs: Vec<&str> = resolved
                    .split('/')
                    .filter(|s| !s.is_empty())
                    .collect();
                let matched = match_segments(segments, &path_segs);
                if *negate {
                    !matched
                } else {
                    matched
                }
            }
        }
    }
}

#[derive(Debug)]
pub struct HeaderRuleCompiled {
    matcher: HeaderMatcher,
    headers: Vec<HeaderItem>,
}

#[derive(Debug)]
pub struct InvalidHeaderRule {
    pub source: String,
    pub error: CompileError,
}

/// Compile the user-supplied header rules. Invalid source patterns are
/// surfaced for stderr warnings; remaining rules continue to apply.
/// Mirrors the redirect / rewrite compile contract.
pub fn compile_rules(
    rules: &[HeaderRule],
) -> (Vec<HeaderRuleCompiled>, Vec<InvalidHeaderRule>) {
    let mut compiled = Vec::with_capacity(rules.len());
    let mut invalid = Vec::new();
    for rule in rules {
        match HeaderMatcher::compile(&rule.source) {
            Ok(matcher) => compiled.push(HeaderRuleCompiled {
                matcher,
                headers: rule.headers.clone(),
            }),
            Err(error) => invalid.push(InvalidHeaderRule {
                source: rule.source.clone(),
                error,
            }),
        }
    }
    (compiled, invalid)
}

/// Apply matching custom headers to a response. Iterates rules in
/// declaration order; for each match, inserts every `(key, value)`
/// pair into the response, replacing any existing header of the same
/// name (case-insensitive via axum's `HeaderMap`).
///
/// 3xx redirect responses are skipped — the reference's redirect path
/// at `serve-handler/src/index.js:586-588` builds the response via
/// `response.writeHead(statusCode, { Location: ... })` without going
/// through `getHeaders`, so custom headers never layer onto redirects.
/// Mirroring this avoids a snapshot divergence on `headers-custom`'s
/// cleanUrls 301 (where reference emits only `location:` and no
/// `x-custom:`).
pub fn apply_custom_headers(
    mut response: Response<Body>,
    request_path: &str,
    rules: &[HeaderRuleCompiled],
) -> Response<Body> {
    if rules.is_empty() || response.status().is_redirection() {
        return response;
    }
    // Single pass: walk rules in order; for each matched item, either
    // insert (Some(value)) or remove (None). Last-write-wins per key
    // is automatic. This mirrors reference's two-stage merge at
    // `index.js:200-251`:
    //
    //   related = Object.assign(related, lastRule's keys, ...);
    //   headers = Object.assign(defaultHeaders, related);
    //   for (k in headers) if (headers[k] === null) delete headers[k];
    //
    // Equivalently: the *final* value at each key (after accumulate)
    // is what's written; if that final value is null, the key is
    // deleted. So an early `null` followed by a later non-null value
    // for the same key results in the non-null value present. Single
    // insert/remove per item, applied left-to-right, matches that
    // semantics — Codex review round 1 P1.
    for rule in rules {
        if rule.matcher.matches(request_path) {
            for item in &rule.headers {
                let Ok(name) = HeaderName::from_bytes(item.key.as_bytes()) else {
                    continue;
                };
                match item.value.as_deref() {
                    Some(value_str) => {
                        if let Ok(value) = HeaderValue::from_str(value_str) {
                            response.headers_mut().insert(name, value);
                        }
                    }
                    None => {
                        response.headers_mut().remove(&name);
                    }
                }
            }
        }
    }
    response
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::StatusCode;

    fn header_rule(source: &str, items: &[(&str, Option<&str>)]) -> HeaderRule {
        HeaderRule {
            source: source.to_string(),
            headers: items
                .iter()
                .map(|(k, v)| HeaderItem {
                    key: k.to_string(),
                    value: v.map(str::to_string),
                })
                .collect(),
        }
    }

    fn build(rules: &[HeaderRule]) -> Vec<HeaderRuleCompiled> {
        let (compiled, invalid) = compile_rules(rules);
        assert!(invalid.is_empty(), "rule compilation produced invalid entries");
        compiled
    }

    fn ok_response() -> Response<Body> {
        Response::builder()
            .status(StatusCode::OK)
            .body(Body::empty())
            .unwrap()
    }

    fn redirect_response() -> Response<Body> {
        Response::builder()
            .status(StatusCode::MOVED_PERMANENTLY)
            .header("location", "/elsewhere")
            .body(Body::empty())
            .unwrap()
    }

    #[test]
    fn empty_rules_pass_through() {
        let rules: Vec<HeaderRuleCompiled> = vec![];
        let resp = apply_custom_headers(ok_response(), "/x", &rules);
        assert!(resp.headers().get("x-anything").is_none());
    }

    #[test]
    fn single_rule_inserts() {
        let rules = build(&[header_rule("**", &[("X-T", Some("yes"))])]);
        let resp = apply_custom_headers(ok_response(), "/x", &rules);
        assert_eq!(resp.headers().get("x-t").unwrap(), "yes");
    }

    #[test]
    fn multiple_rules_accumulate() {
        let rules = build(&[
            header_rule("**", &[("X-One", Some("1"))]),
            header_rule("**/*.css", &[("X-Two", Some("2"))]),
        ]);
        let resp = apply_custom_headers(ok_response(), "/asset.css", &rules);
        assert_eq!(resp.headers().get("x-one").unwrap(), "1");
        assert_eq!(resp.headers().get("x-two").unwrap(), "2");
    }

    #[test]
    fn case_insensitive_override() {
        let rules = build(&[
            header_rule("**", &[("X-K", Some("first"))]),
            header_rule("**", &[("x-k", Some("second"))]),
        ]);
        let resp = apply_custom_headers(ok_response(), "/x", &rules);
        // axum's HeaderMap normalizes; last write wins regardless of case.
        let values: Vec<_> = resp.headers().get_all("x-k").iter().collect();
        assert_eq!(values.len(), 1);
        assert_eq!(values[0], "second");
    }

    #[test]
    fn redirects_skip_application() {
        let rules = build(&[header_rule("**", &[("X-T", Some("yes"))])]);
        let resp = apply_custom_headers(redirect_response(), "/x", &rules);
        assert!(resp.headers().get("x-t").is_none());
    }

    #[test]
    fn null_prune_deletes_prior_header() {
        let rules = build(&[
            header_rule("**", &[("X-Set", Some("v"))]),
            header_rule("**/*.css", &[("X-Set", None)]),
        ]);
        let resp = apply_custom_headers(ok_response(), "/style.css", &rules);
        assert!(
            resp.headers().get("x-set").is_none(),
            "null-prune should delete X-Set on /style.css"
        );
    }

    #[test]
    fn null_prune_does_not_delete_when_rule_does_not_match() {
        let rules = build(&[
            header_rule("**", &[("X-Set", Some("v"))]),
            header_rule("**/*.css", &[("X-Set", None)]),
        ]);
        let resp = apply_custom_headers(ok_response(), "/data.json", &rules);
        assert_eq!(resp.headers().get("x-set").unwrap(), "v");
    }

    #[test]
    fn later_set_value_wins_over_earlier_null_prune() {
        // Codex review round 1 P1: null-prune order. Reference's
        // two-stage merge (Object.assign-then-prune) preserves a key
        // whose FINAL value is non-null, even if an earlier matched
        // rule set it to null. The single-pass insert/remove
        // implementation must give the same result.
        let rules = build(&[
            header_rule("**", &[("X-Set", None)]),
            header_rule("**", &[("X-Set", Some("v"))]),
        ]);
        let resp = apply_custom_headers(ok_response(), "/x", &rules);
        assert_eq!(
            resp.headers().get("x-set").unwrap(),
            "v",
            "later non-null write must win over earlier null entry"
        );
    }

    #[test]
    fn literal_source_is_case_sensitive() {
        // Codex review round 2 P1: minimatch's default `nocase: false`
        // means literal sources match request paths case-sensitively.
        // The prior `Matcher::compile_no_segments` route went through
        // path-to-regexp's case-insensitive `i` flag, so `/Case`
        // wrongly matched `/case`. HeaderMatcher::Literal uses pure
        // string equality after path_posix_resolve normalization.
        let rules = build(&[header_rule("/Case", &[("X-Sensitive", Some("yes"))])]);
        let mismatch = apply_custom_headers(ok_response(), "/case", &rules);
        assert!(
            mismatch.headers().get("x-sensitive").is_none(),
            "case-only differences must NOT match (minimatch default `nocase: false`)"
        );
        let match_ = apply_custom_headers(ok_response(), "/Case", &rules);
        assert_eq!(match_.headers().get("x-sensitive").unwrap(), "yes");
    }

    #[test]
    fn name_segment_in_source_is_literal_not_capture() {
        // Codex review round 1 P1: `:name` source is minimatch-only
        // for headers (reference's `sourceMatches` skips path-to-regexp
        // when `allowSegments` is falsy, `index.js:38-67`). Source
        // `/api/:id` must match only the literal path `/api/:id`, not
        // `/api/42`.
        let rules = build(&[header_rule("/api/:id", &[("X-Param", Some("yes"))])]);
        let no_match = apply_custom_headers(ok_response(), "/api/42", &rules);
        assert!(
            no_match.headers().get("x-param").is_none(),
            "headers source `:name` must NOT capture-match `/api/42`"
        );
        let literal_match = apply_custom_headers(ok_response(), "/api/:id", &rules);
        assert_eq!(
            literal_match.headers().get("x-param").unwrap(),
            "yes",
            "headers source `/api/:id` must match the literal path `/api/:id`"
        );
    }

    #[test]
    fn null_prune_is_case_insensitive_on_key() {
        let rules = build(&[
            header_rule("**", &[("X-Set", Some("v"))]),
            // Prune via differently-cased key.
            header_rule("**", &[("x-SET", None)]),
        ]);
        let resp = apply_custom_headers(ok_response(), "/x", &rules);
        assert!(resp.headers().get("x-set").is_none());
    }
}
