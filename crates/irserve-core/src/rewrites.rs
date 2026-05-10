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

/// Phase 7 (single-pass): walk the compiled rewrite rules in order
/// and return the first matching rule's rendered destination. Slice
/// 4 of Stage 6e replaces this with a recursive chain (`applyRewrites`
/// at `serve-handler/src/index.js:91-117` removes the matched rule
/// and recurses on the rewritten path).
pub fn compute_configured_rewrites(
    url_path: &str,
    rules: &[RewriteRuleCompiled],
) -> Option<String> {
    for rule in rules {
        if let Some(target) = rule.matcher.try_match(url_path) {
            return Some(target);
        }
    }
    None
}
