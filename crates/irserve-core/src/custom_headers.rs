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
//!   after accumulate, mirroring `index.js:247-251`.
//!
//! Custom headers apply to every successful and error response. They
//! are skipped on 3xx redirects (mirrors the reference's
//! `response.writeHead(redirect.statusCode, { Location: ... })` path at
//! `index.js:586-588`, which bypasses `getHeaders`).
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
use crate::path_pattern::{CompileError, Matcher};

#[derive(Debug)]
pub struct HeaderRuleCompiled {
    matcher: Matcher,
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
        // Headers have no destination template — pass a placeholder
        // that the matcher will store verbatim and never render.
        match Matcher::compile(&rule.source, "/") {
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
    // First pass: insert/replace per accumulating rules. Mirror
    // reference's `appendHeaders` loop at `index.js:200-210`.
    for rule in rules {
        if rule.matcher.try_match(request_path).is_some() {
            for item in &rule.headers {
                let Ok(name) = HeaderName::from_bytes(item.key.as_bytes()) else {
                    continue;
                };
                let Some(value_str) = item.value.as_deref() else {
                    // Null-pruning is applied in the second pass so that
                    // its delete-after-accumulate semantics matches
                    // `index.js:245-251` (Object.assign first, prune last).
                    continue;
                };
                let Ok(value) = HeaderValue::from_str(value_str) else {
                    continue;
                };
                response.headers_mut().insert(name, value);
            }
        }
    }
    // Second pass: SRV-HDR-002 null-prune. After accumulate + merge,
    // any matched rule whose `value: null` removes a previously-set
    // header with the same key (case-insensitive). Mirrors
    // `index.js:247-251` (`for ... in` loop deleting headers[key] === null
    // entries from the merged map).
    for rule in rules {
        if rule.matcher.try_match(request_path).is_some() {
            for item in &rule.headers {
                if item.value.is_none() {
                    if let Ok(name) = HeaderName::from_bytes(item.key.as_bytes()) {
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
