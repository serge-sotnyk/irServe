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
//! - `value: null` deletion lands in slice 5 (`index.js:247-251`).
//!
//! Custom headers apply to every response, including 4xx error pages
//! (the reference passes through `getHeaders` from `sendError` at
//! `index.js:519` as well as from the success path at `index.js:508`).

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
    for rule in rules {
        if rule.matcher.try_match(request_path).is_some() {
            for item in &rule.headers {
                let Ok(name) = HeaderName::from_bytes(item.key.as_bytes()) else {
                    continue;
                };
                let Ok(value) = HeaderValue::from_str(&item.value) else {
                    continue;
                };
                response.headers_mut().insert(name, value);
            }
        }
    }
    response
}
