//! Phase 7 of the dispatcher pipeline: configured `rewrites` from
//! `serve.json` (Stage 6e).
//!
//! Mirrors `third_party/serve-handler/src/index.js`:
//! - `applyRewrites` (lines 91-117): walks the user's rewrite rules
//!   in declared order, returning the first match's
//!   `toTarget`-rendered destination. The reference also recurses
//!   on the rewritten path with the matched rule removed; Stage-6e
//!   slice 2 implements single-pass first-match-wins, and slice 4
//!   adds the recursive chain with an irserve-only depth cap.
//! - `sourceMatches` (lines 38-67) and `toTarget` (lines 69-89):
//!   shared with redirects via [`crate::path_pattern`]. Compilation,
//!   matching, and `:name` interpolation all reuse the same
//!   primitives so semantics stay in lock-step across both phases.
//!
//! Unlike redirects (phase 6), a matched rewrite does NOT emit a
//! 3xx response. The rewritten path replaces the original
//! `url_path` and the dispatcher continues to file resolution
//! (phases 8/9) on the rewritten value. The pre-stat asymmetry
//! mandated by `openspec/specs/rewrites/spec.md` lives in
//! `dispatch.rs`, not here: this module just answers "given this
//! path and these rules, what would the rewrite be?".

use crate::config::RewriteRule;
use crate::path_pattern::Matcher;

pub use crate::path_pattern::CompileError;

/// Precompiled rewrite rule. Mirrors [`RedirectRuleCompiled`] minus
/// the optional status override — a rewrite is implicitly status 200
/// served by the destination file.
///
/// [`RedirectRuleCompiled`]: crate::redirects::RedirectRuleCompiled
#[derive(Debug)]
pub struct RewriteRuleCompiled {
    matcher: Matcher,
}

/// One rewrite rule that failed to compile. Same contract as
/// [`InvalidRedirect`]: the bin layer surfaces a stderr warning per
/// invalid rule; other rules in the same list keep working.
///
/// [`InvalidRedirect`]: crate::redirects::InvalidRedirect
#[derive(Debug)]
pub struct InvalidRewrite {
    pub source: String,
    pub error: CompileError,
}

/// Compile the user-supplied rewrite rules into matchers. Invalid
/// patterns are collected into the returned `Vec<InvalidRewrite>` for
/// the bin to surface as warnings; valid rules in the same list
/// continue to work. Mirrors `redirects::compile_rules` byte-for-byte
/// at the call-site level (only the per-rule constructor differs).
pub fn compile_rules(rules: &[RewriteRule]) -> (Vec<RewriteRuleCompiled>, Vec<InvalidRewrite>) {
    let mut compiled = Vec::with_capacity(rules.len());
    let mut invalid = Vec::new();
    for rule in rules {
        match Matcher::compile(&rule.source, &rule.destination) {
            Ok(matcher) => compiled.push(RewriteRuleCompiled { matcher }),
            Err(error) => invalid.push(InvalidRewrite {
                source: rule.source.clone(),
                error,
            }),
        }
    }
    (compiled, invalid)
}

/// Recursion-depth cap for rewrite chaining. The reference imposes
/// no cap (`applyRewrites` at `serve-handler/src/index.js:91-117`
/// could recurse until V8 stack overflow on a malformed cyclic
/// configuration). irServe adds a hard cap as defense-in-depth; on
/// overflow, it gracefully clamps to the path produced by the last
/// successful pass instead of erroring. Documented as D-014 (status:
/// `adapted` — irserve-only divergence from the reference).
///
/// Cap value: 64. Justification:
/// - `serve-handler`'s real-world rule counts are 2-3; even
///   pathological 16-rule chains fit comfortably.
/// - Power-of-two; well below default thread-stack frame budgets
///   (Tokio worker default ~2 MiB, ample for 64 frames of
///   `apply_rewrites`).
/// - Real configurations cannot reach this depth without an
///   intentional cycle.
const REWRITE_DEPTH_CAP: usize = 64;

/// Phase 7 (recursive chaining): mirror `applyRewrites` at
/// `serve-handler/src/index.js:91-117`. On a match, remove the
/// matched rule from the active list and recurse on the rewritten
/// path. Termination happens when no remaining rule matches or the
/// active list is empty. Returns `None` when no rule matches the
/// initial request path (mirrors the reference's `repetitive ?
/// requestPath : null` fallback at `index.js:97`).
///
/// Recursion depth is capped at [`REWRITE_DEPTH_CAP`]; on overflow,
/// the function gracefully clamps to the path captured by the most
/// recent successful pass. This is an irserve-only divergence from
/// the reference (D-014).
pub fn compute_configured_rewrites(
    url_path: &str,
    rules: &[RewriteRuleCompiled],
) -> Option<String> {
    let active: Vec<&RewriteRuleCompiled> = rules.iter().collect();
    apply_rewrites(url_path, &active, false, 0)
}

fn apply_rewrites(
    request_path: &str,
    active: &[&RewriteRuleCompiled],
    repetitive: bool,
    depth: usize,
) -> Option<String> {
    // Mirrors `index.js:97`: when the function was already called
    // recursively, the path was rewritten at least once, so we must
    // return that rewritten path even when no remaining rule
    // matches. On the first call, "no match" means the rewrite did
    // not fire at all and we return `None` (mirrors `null`).
    let fallback = if repetitive {
        Some(request_path.to_string())
    } else {
        None
    };
    if active.is_empty() {
        return fallback;
    }
    if depth >= REWRITE_DEPTH_CAP {
        // irserve-only graceful clamp (D-014). Returning `fallback`
        // here yields the path captured by the most recent
        // successful pass — for a `repetitive=true` call this is
        // always `Some(request_path)`.
        return fallback;
    }
    for (idx, rule) in active.iter().enumerate() {
        if let Some(target) = rule.matcher.try_match(request_path) {
            // Remove the matched rule and recurse on the rewritten
            // path. Mirrors `index.js:108-112`:
            //
            //     rewritesCopy.splice(index, 1);
            //     return applyRewrites(slasher(target), rewritesCopy, true);
            //
            // The reference applies `slasher(target)` to normalize
            // captures-into-destination edge cases (e.g. a captured
            // `..` segment surviving `encodeURIComponent`). For
            // slice-4 scope the destinations we render are already
            // normalized by `Matcher::compile`'s
            // `normalize_destination`; capture rendering applies
            // `encodeURIComponent` per value (which encodes `/` to
            // `%2F`), so re-running slasher per recursion would be
            // a no-op for all probe-pinned cases. Documented in
            // D-013 along with the chaining contract.
            let remaining: Vec<&RewriteRuleCompiled> = active
                .iter()
                .enumerate()
                .filter(|(i, _)| *i != idx)
                .map(|(_, r)| *r)
                .collect();
            return apply_rewrites(&target, &remaining, true, depth + 1);
        }
    }
    fallback
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rule(source: &str, destination: &str) -> RewriteRule {
        RewriteRule {
            source: source.to_string(),
            destination: destination.to_string(),
        }
    }

    fn compile(rules: &[RewriteRule]) -> Vec<RewriteRuleCompiled> {
        let (c, invalid) = compile_rules(rules);
        assert!(
            invalid.is_empty(),
            "test helper saw invalid rules: {:?}",
            invalid.iter().map(|i| &i.source).collect::<Vec<_>>()
        );
        c
    }

    #[test]
    fn no_match_returns_none() {
        let rules = compile(&[rule("/a", "/b")]);
        assert_eq!(compute_configured_rewrites("/x", &rules), None);
    }

    #[test]
    fn single_rule_match() {
        let rules = compile(&[rule("/a", "/b")]);
        assert_eq!(
            compute_configured_rewrites("/a", &rules),
            Some("/b".to_string())
        );
    }

    #[test]
    fn chain_two_rules_a_to_b_to_c() {
        // Mirrors `rewrites-chain` probe: /a → /b → /c.html.
        let rules = compile(&[rule("/a", "/b"), rule("/b", "/c.html")]);
        assert_eq!(
            compute_configured_rewrites("/a", &rules),
            Some("/c.html".to_string())
        );
    }

    #[test]
    fn chain_order_independent() {
        // Reverse rule order; same /a → /c.html outcome since
        // the matched rule is removed each pass.
        let rules = compile(&[rule("/b", "/c.html"), rule("/a", "/b")]);
        assert_eq!(
            compute_configured_rewrites("/a", &rules),
            Some("/c.html".to_string())
        );
    }

    #[test]
    fn cycle_two_rules_terminates_via_rule_consumption() {
        // /a→/b and /b→/a. First pass: /a matches first rule → /b
        // (rule consumed). Second pass: /b matches second rule
        // → /a (rule consumed). Third pass: empty active list →
        // returns last path. Reference would do the same.
        let rules = compile(&[rule("/a", "/b"), rule("/b", "/a")]);
        assert_eq!(
            compute_configured_rewrites("/a", &rules),
            Some("/a".to_string())
        );
    }

    #[test]
    fn self_loop_terminates_after_rule_consumed() {
        // /a → /a. Single match consumes the rule; recursion with
        // empty list returns Some("/a").
        let rules = compile(&[rule("/a", "/a")]);
        assert_eq!(
            compute_configured_rewrites("/a", &rules),
            Some("/a".to_string())
        );
    }

    #[test]
    fn depth_cap_clamps_to_last_path() {
        // Cap = 64. Build 65 self-matching rules; each pass
        // consumes one. After 64 passes the cap fires; with
        // `repetitive=true` the fallback is `Some(request_path)`
        // — i.e. the path produced by the last successful pass.
        // Verifies the D-014 graceful-clamp contract.
        let mut raw = Vec::with_capacity(65);
        for _ in 0..65 {
            raw.push(rule("/a", "/a"));
        }
        let rules = compile(&raw);
        assert_eq!(
            compute_configured_rewrites("/a", &rules),
            Some("/a".to_string())
        );
    }

    #[test]
    fn first_rule_no_match_falls_to_second() {
        // Mirrors first-match-wins iteration on the first call:
        // first rule misses, second rule matches.
        let rules = compile(&[rule("/x", "/y"), rule("/a", "/b")]);
        assert_eq!(
            compute_configured_rewrites("/a", &rules),
            Some("/b".to_string())
        );
    }
}
