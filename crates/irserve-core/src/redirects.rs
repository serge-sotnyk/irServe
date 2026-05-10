//! Phase 6 of the dispatcher pipeline: configured `redirects` from
//! `serve.json`.
//!
//! Mirrors `third_party/serve-handler/src/index.js`:
//! - `shouldRedirect` redirects branch (lines 172-182): first-match-wins
//!   iteration over the rules, returning `{target, statusCode}` for the
//!   first rule whose source matches the (already decoded + collapsed)
//!   request path.
//! - `sourceMatches` (lines 38-67): pattern matching against the request
//!   path, with `path-to-regexp` for path-segment patterns and
//!   `minimatch` as a glob fallback.
//! - `toTarget` (lines 69-89): destination rendering with `:param`
//!   interpolation (the reference applies `encodeURIComponent` to each
//!   captured value via `pathToRegExp.compile`; the surrounding
//!   `encodeURI` over the full target lives in `dispatch.rs`).
//!
//! Slice 1 of Stage 6d covers literal and glob source patterns plus the
//! optional `type` override. Slice 2 adds path-segment params (`:name`)
//! via a custom mini path-to-regexp compiler, plus a `*` token that
//! mirrors the reference's `slashed.replace('*', '(.*)')` pre-pass at
//! `index.js:46`. The Q-007 destination-form probe lands in slice 3.
//!
//! Stage 6e slice 1 extracted the path-pattern matcher kernel into
//! [`crate::path_pattern`] so the upcoming `rewrites` module can reuse
//! the same compilation, matching, and destination-template rendering
//! primitives. This module now wires those primitives into the
//! redirect-specific pipeline (`compile_rules` →
//! `compute_configured_redirects` → `try_match`).

use crate::config::RedirectRule;
use crate::path_pattern::{
    classify_pattern_segment, compile_dest_template, compile_source_regex, de_escape_keep_trailing,
    ends_with_unescaped_backslash, has_glob_meta, has_path_param, literal_matches, match_segments,
    normalize_destination, path_posix_resolve, slasher, GlobFallback, Matcher, PatSeg,
};

// Re-export path_pattern items used inside this module (and by the
// `tests` submodule via `use super::*`) so the test bodies and helper
// fns can reference them by short name without changing test code.
// Codex review note: `pub use` on `pub(crate)` items keeps them
// crate-internal — no public-API surface is changed by the extraction.
pub use crate::path_pattern::CompileError;

/// Precompiled redirect rule. Built once at server start so the
/// dispatcher can do per-request matching without recompiling globs.
#[derive(Debug)]
pub struct RedirectRuleCompiled {
    matcher: Matcher,
    status_code: Option<u16>,
}

/// One redirect rule that failed to compile. Mirrors the reference's
/// behavior in `sourceMatches` (`serve-handler/src/index.js:38-67`),
/// where unparseable patterns are silently treated as never-matching:
/// surface invalid rules to the bin layer for stderr warnings; the
/// server keeps running and other rules continue to work.
///
/// Most malformed sources are recovered before reaching this struct:
/// `classify_pattern_segment` falls back to a Literal segment when
/// globset rejects the de-escaped form (Codex review round 10 P2),
/// so rules with sources like `/u/\[` are kept as literal-`[`
/// matchers rather than dropped. Only patterns that fail
/// `regex::Regex::new` (for `:name`/`*` Pattern matchers) propagate
/// here.
#[derive(Debug)]
pub struct InvalidRedirect {
    pub source: String,
    pub error: CompileError,
}

/// Compile the user-supplied redirect rules into matchers. Invalid
/// patterns are collected into the returned `Vec<InvalidRedirect>` for
/// the bin to surface as warnings; valid rules in the same list
/// continue to work.
pub fn compile_rules(rules: &[RedirectRule]) -> (Vec<RedirectRuleCompiled>, Vec<InvalidRedirect>) {
    let mut compiled = Vec::with_capacity(rules.len());
    let mut invalid = Vec::new();
    for rule in rules {
        match compile_one(rule) {
            Ok(c) => compiled.push(c),
            Err(error) => invalid.push(InvalidRedirect {
                source: rule.source.clone(),
                error,
            }),
        }
    }
    (compiled, invalid)
}

fn compile_one(rule: &RedirectRule) -> Result<RedirectRuleCompiled, CompileError> {
    let slashed = slasher(&rule.source);
    let (negate, body) = match slashed.strip_prefix('!') {
        Some(rest) => (true, rest.to_string()),
        None => (false, slashed),
    };
    let normalized_dest = normalize_destination(&rule.destination);
    // Routing classifier: mirror `serve-handler/src/index.js:38-67`'s
    // `sourceMatches` codepath. The reference always tries
    // `pathToRegExp` first (with `*` → `(.*)` pre-substitution) and
    // falls back to `minimatch` only when the regex returns null.
    //
    // - Sources with `:name` or `*` (and not `!`-prefixed) → Pattern
    //   (regex). Mirrors the path-to-regexp first-pass.
    // - `!`-prefixed sources → Glob (negate=true). The reference's
    //   path-to-regexp first-pass on `!`-bearing patterns produces
    //   a regex that matches paths starting with `!` — which real
    //   requests never do — so it falls through to minimatch's
    //   negation handler. We mirror by routing directly to globset
    //   with the negate flag, treating `:name`-like fragments as
    //   literal characters in the glob (which is what minimatch
    //   does too — Codex review round 2 P1).
    // - Sources with `?`/`[`/`{` glob meta (no `*`, no `:name`,
    //   no `!`) → Glob.
    // - Otherwise → Literal.
    let needs_pattern = (has_path_param(&body) || body.contains('*')) && !negate;
    let matcher = if needs_pattern {
        let regex = compile_source_regex(&body)?;
        let dest_template = compile_dest_template(&normalized_dest);
        // The reference's `sourceMatches` ALWAYS tries minimatch
        // after pathToRegExp returns null (`index.js:47-66`),
        // regardless of source shape. Codex review round 6 P2
        // showed that even non-`*`-bearing sources need the
        // fallback: e.g. `/a/:id/?` against `/a/:id/x` matches in
        // the reference via minimatch's literal-`:id` semantics
        // plus `?` as a single-char glob, but our regex compiles
        // `?` as a literal `\?` that real URLs don't carry. Build
        // glob_fallback for every Pattern matcher.
        // Disable glob_fallback when the source body ends with an
        // unescaped `\`: minimatch turns the trailing `\` into a
        // synthetic `/` suffix in the compiled regex, and
        // `path.posix.resolve` strips trailing slashes from the
        // request path, so the minimatch branch can never match
        // such sources against any resolved request path. The
        // Pattern's primary regex (which preserves the trailing
        // `\` as a literal) still handles the
        // `request-with-literal-backslash` case correctly. Codex
        // review round 11 P1: source `/v/*\` previously
        // over-matched `/v/x` via the glob_fallback's `*` segment
        // (since the per-segment classifier silently dropped the
        // trailing `\`).
        let glob_fallback = if ends_with_unescaped_backslash(&body) {
            None
        } else {
            let segments: Result<Vec<PatSeg>, globset::Error> = body
                .split('/')
                .filter(|s| !s.is_empty())
                .map(classify_pattern_segment)
                .collect();
            Some(GlobFallback {
                segments: segments?,
            })
        };
        Matcher::Pattern {
            regex,
            dest_template,
            glob_fallback,
        }
    } else if has_glob_meta(&body) || negate {
        let segments: Result<Vec<PatSeg>, globset::Error> = body
            .split('/')
            .filter(|s| !s.is_empty())
            .map(classify_pattern_segment)
            .collect();
        Matcher::Glob {
            segments: segments?,
            negate,
            destination: normalized_dest,
        }
    } else {
        // Build TWO match forms, mirroring path-to-regexp and
        // minimatch.
        //
        // `source_ptr` mirrors path-to-regexp: `\X` → `X` (escape
        // consumed); trailing `\` → kept as literal `\`. Codex
        // review round 8 P2 introduced de-escape because minimatch
        // and path-to-regexp both treat `\X` as a literal `X`;
        // round 11 P1 surfaced that the trailing `\` is NOT
        // dropped by path-to-regexp — it's emitted as a literal
        // `\` in the compiled regex, so the regex requires a
        // literal `\` at end of path.
        //
        // `source_mm` is `Some(body)` when the raw source body
        // differs from `source_ptr` AND the body has no trailing
        // unescaped `\`. This mirrors minimatch's empirical
        // behavior: `minimatch('/u/\\f', '/u/\\f')` returns true
        // because the segment-level matcher treats the request's
        // literal `\X` as equivalent to the pattern's `\X` (after
        // the parser's per-segment normalization). Trailing `\`
        // is excluded because minimatch produces a regex with a
        // synthetic trailing `/` that `path.posix.resolve` always
        // strips.
        let source_ptr = de_escape_keep_trailing(&body);
        let source_mm = if body != source_ptr && !ends_with_unescaped_backslash(&body) {
            Some(body.clone())
        } else {
            None
        };
        Matcher::Literal {
            source_ptr,
            source_mm,
            destination: normalized_dest,
        }
    };
    Ok(RedirectRuleCompiled {
        matcher,
        status_code: rule.kind,
    })
}

/// Phase 6: walk the compiled rules in order, return the first matching
/// rule's `(target, status_code)`. Status defaults to 301 when the rule
/// has no `type` override (matches `serve-handler/src/index.js:179`,
/// `statusCode: type || defaultType`).
pub fn compute_configured_redirects(
    url_path: &str,
    rules: &[RedirectRuleCompiled],
) -> Option<(String, u16)> {
    for rule in rules {
        if let Some(target) = rule.try_match(url_path) {
            return Some((target, rule.status_code.unwrap_or(301)));
        }
    }
    None
}

impl RedirectRuleCompiled {
    fn try_match(&self, path: &str) -> Option<String> {
        // Mirror `serve-handler/src/index.js:41`'s
        // `path.posix.resolve(requestPath)`. Both `pathToRegExp.exec`
        // (line 49) AND `minimatch` (line 59) operate on the
        // resolved path. `path.posix.resolve` does more than trim
        // a trailing slash: it ALSO resolves `.` / `..` segments
        // and collapses consecutive slashes (Codex review round 8
        // P1). The round-7 trim alone missed raw-mode requests
        // like `GET /a/./x` and `GET /a/b/../x`, which the
        // reference resolves to `/a/x` before matching.
        let resolved = path_posix_resolve(path);
        let path = resolved.as_str();
        match &self.matcher {
            Matcher::Literal {
                source_ptr,
                source_mm,
                destination,
            } => {
                // Path-to-regexp branch: case-insensitive (default
                // `i` flag in v3.3.0). Codex review round 12 P1.
                if literal_matches(source_ptr, path, false) {
                    return Some(destination.clone());
                }
                // Minimatch fallback for inner-`\X` sources like
                // `/u/\f`: the segment-level minimatch matcher
                // accepts the raw request path that literally
                // contains `\X` (Codex review round 11 P1). We
                // mirror via direct string comparison against the
                // raw source body — this is exact for the no-glob,
                // no-trailing-backslash case which is when
                // `source_mm` is `Some`. Case-SENSITIVE per
                // minimatch's default `nocase: false`.
                if let Some(mm) = source_mm {
                    if literal_matches(mm, path, true) {
                        return Some(destination.clone());
                    }
                }
                None
            }
            Matcher::Glob {
                segments,
                negate,
                destination,
            } => {
                let path_segs: Vec<&str> =
                    path.split('/').filter(|s| !s.is_empty()).collect();
                if match_segments(segments, &path_segs) ^ negate {
                    Some(destination.clone())
                } else {
                    None
                }
            }
            Matcher::Pattern {
                regex,
                dest_template,
                glob_fallback,
            } => {
                if let Some(caps) = regex.captures(path) {
                    return Some(dest_template.render(Some(&caps)));
                }
                if let Some(fb) = glob_fallback {
                    // The trailing-slash trim is now applied at the
                    // top of `try_match` (Codex round 7 P1.1), so
                    // the path here is already path.posix.resolve-d.
                    if fb.matches_strict(path) {
                        // Glob-fallback path: no regex captures
                        // (the regex didn't match). dest_template's
                        // `Param` fragments fail open to empty
                        // strings — but a `:name` in the destination
                        // template paired with a `*`-bearing source
                        // can land here when the request literally
                        // carries the `:name` segment (Codex round
                        // 4 P2's repro), in which case the
                        // destination just emits the literal portion
                        // and an empty for the param.
                        return Some(dest_template.render(None));
                    }
                }
                None
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::path_pattern::{compile_dest_template, has_path_param, has_protocol, path_posix_normalize, DestFrag};

    fn rule(source: &str, destination: &str, kind: Option<u16>) -> RedirectRule {
        RedirectRule {
            source: source.to_string(),
            destination: destination.to_string(),
            kind,
        }
    }

    fn compile(rules: &[RedirectRule]) -> Vec<RedirectRuleCompiled> {
        let (c, invalid) = compile_rules(rules);
        assert!(
            invalid.is_empty(),
            "test helper saw invalid rules; pass them through compile_rules directly: {:?}",
            invalid.iter().map(|i| &i.source).collect::<Vec<_>>()
        );
        c
    }

    // ----- literal source ------------------------------------------

    #[test]
    fn literal_match_default_301() {
        let rules = compile(&[rule("/old", "/new", None)]);
        let (target, status) =
            compute_configured_redirects("/old", &rules).expect("literal must match");
        assert_eq!(target, "/new");
        assert_eq!(status, 301);
    }

    #[test]
    fn literal_match_explicit_type_302() {
        let rules = compile(&[rule("/old", "/new", Some(302))]);
        let (_, status) = compute_configured_redirects("/old", &rules).unwrap();
        assert_eq!(status, 302);
    }

    #[test]
    fn literal_match_explicit_type_307() {
        let rules = compile(&[rule("/old", "/new", Some(307))]);
        let (_, status) = compute_configured_redirects("/old", &rules).unwrap();
        assert_eq!(status, 307);
    }

    #[test]
    fn literal_no_match_returns_none() {
        let rules = compile(&[rule("/old", "/new", None)]);
        assert!(compute_configured_redirects("/other", &rules).is_none());
        assert!(compute_configured_redirects("/old/x", &rules).is_none());
    }

    #[test]
    fn literal_trailing_slash_flexion() {
        // path-to-regexp@3.x compiles "/old" to ^/old/?$, so both
        // `/old` and `/old/` should match; conversely a rule
        // `source: "/old/"` should match both `/old/` and `/old`.
        let rules = compile(&[rule("/old", "/new", None)]);
        assert!(compute_configured_redirects("/old/", &rules).is_some());

        let rules = compile(&[rule("/old/", "/new", None)]);
        assert!(compute_configured_redirects("/old", &rules).is_some());
        assert!(compute_configured_redirects("/old/", &rules).is_some());
    }

    #[test]
    fn literal_source_normalizes_missing_leading_slash() {
        // slasher() prepends `/` so users can write `"old"`.
        let rules = compile(&[rule("old", "/new", None)]);
        assert!(compute_configured_redirects("/old", &rules).is_some());
    }

    #[test]
    fn literal_source_resolves_leading_dotdot() {
        // Codex review round 2 P1: source `slasher` is
        // `path.posix.normalize(path.posix.join('/', value))`, so
        // `../old` joins to `/../old`, normalizes to `/old`. The
        // compiled rule then literal-matches `/old`. Without the
        // join-with-`/` step, the source would compile to a literal
        // match against `/../old` (a path no real request carries).
        let rules = compile(&[rule("../old", "/new", None)]);
        let (target, _) = compute_configured_redirects("/old", &rules)
            .expect("source `../old` should normalize to literal `/old`");
        assert_eq!(target, "/new");
    }

    #[test]
    fn literal_source_resolves_dot_segment() {
        let rules = compile(&[rule("/a/./b", "/new", None)]);
        assert!(compute_configured_redirects("/a/b", &rules).is_some());
    }

    // ----- glob source ---------------------------------------------

    #[test]
    fn star_source_crosses_segments() {
        // After Codex round 1 P1 fix: `*`-bearing sources (no `:name`,
        // no `!`-prefix) route through Pattern (regex), mirroring
        // `serve-handler/src/index.js:46`'s
        // `slashed.replace('*', '(.*)')` pre-pass + `pathToRegExp`.
        // `(.*)` crosses `/` segments, unlike globset's
        // `literal_separator(true)`.
        let rules = compile(&[rule("/dir/*", "/elsewhere", None)]);
        assert!(compute_configured_redirects("/dir/page", &rules).is_some());
        assert!(compute_configured_redirects("/dir/sub/page", &rules).is_some());
        assert!(compute_configured_redirects("/dir/a/b/c", &rules).is_some());
        // The leading `/dir/` literal still anchors the match.
        assert!(compute_configured_redirects("/other/page", &rules).is_none());
    }

    #[test]
    fn double_star_source_crosses_segments() {
        let rules = compile(&[rule("/dir/**", "/elsewhere", None)]);
        assert!(compute_configured_redirects("/dir/page", &rules).is_some());
        assert!(compute_configured_redirects("/dir/sub/page", &rules).is_some());
    }

    #[test]
    fn raw_dot_segment_in_path_is_resolved() {
        // Codex review round 8 P1: reference applies
        // `path.posix.resolve(requestPath)` before BOTH
        // pathToRegExp and minimatch (`index.js:41`). The trim
        // alone (round 7) wasn't enough — `.` and `..` segments
        // also resolve. So a raw request `/a/./x` resolves to
        // `/a/x` and matches a literal `/a/x` rule.
        let rules = compile(&[rule("/a/x", "/hit", None)]);
        let (target, _) = compute_configured_redirects("/a/./x", &rules)
            .expect("`.` segment should resolve away");
        assert_eq!(target, "/hit");
    }

    #[test]
    fn raw_dotdot_segment_in_path_is_resolved() {
        let rules = compile(&[rule("/a/x", "/hit", None)]);
        let (target, _) = compute_configured_redirects("/a/b/../x", &rules)
            .expect("`..` segment should pop the previous segment");
        assert_eq!(target, "/hit");
    }

    #[test]
    fn raw_dot_segment_with_param() {
        let rules = compile(&[rule("/a/:id", "/new/:id", None)]);
        let (target, _) = compute_configured_redirects("/a/./foo", &rules)
            .expect("`.` segment should resolve before regex captures :id");
        assert_eq!(target, "/new/foo");
    }

    #[test]
    fn escaped_doublestar_classifies_as_globstar() {
        // Codex review round 10 P1.1: the `seg == "**"` check now
        // runs on the de-escaped form. Source `\**` (which
        // de-escapes to `**`) should be DoubleStar — minimatch
        // parses `\**` as globstar.
        let rules = compile(&[rule("/a/\\**", "/hit", None)]);
        let (target, _) = compute_configured_redirects("/a/x/y", &rules)
            .expect("`\\**` should match like globstar");
        assert_eq!(target, "/hit");
        assert!(compute_configured_redirects("/a/x", &rules).is_some());
    }

    #[test]
    fn brace_with_dot_alt_rejects_other_dotfile() {
        // Codex review round 10 P1.2: minimatch's `dot: false` is
        // PER-ALTERNATIVE. `{.x,*}` admits `.x` (via the `.x` alt)
        // but rejects `.y` because the only dot-prefixed alt is
        // `.x` literal, and `*` rejects dotfiles.
        let rules = compile(&[rule("/x/{.x,*}", "/hit", None)]);
        // `.x` matches via the dot-prefixed alt.
        assert!(compute_configured_redirects("/x/.x", &rules).is_some());
        // Non-dot path matches via the `*` alt.
        assert!(compute_configured_redirects("/x/abc", &rules).is_some());
        // Other dotfile is REJECTED — `.x` literal alt doesn't
        // match `.y`, and `*` rejects dotfiles per `dot: false`.
        assert!(compute_configured_redirects("/x/.y", &rules).is_none());
    }

    #[test]
    fn unmatched_bracket_source_falls_back_to_literal() {
        // Codex review round 10 P2: the de-escaped form `[` is
        // invalid globset (unmatched bracket). The classifier
        // falls back to a Literal segment with the de-escaped
        // form. So `/u/\[` literal-matches `[` (which a request
        // path can carry as `%5B` decoded to `[`).
        let rules = compile(&[rule("/u/\\[", "/hit", None)]);
        let (target, _) = compute_configured_redirects("/u/[", &rules)
            .expect("`\\[` should literal-match `[`");
        assert_eq!(target, "/hit");
    }

    #[test]
    fn literal_source_with_trailing_backslash_requires_literal_backslash() {
        // Codex review round 11 P1: source `/u/foo\` should match
        // a request path that literally ends in `\` (e.g. decoded
        // from `%5C`), NOT strip the trailing `\` and over-match
        // `/u/foo`. Reference (path-to-regexp v3.3.0) preserves
        // the trailing `\` in the compiled regex as a literal `\`,
        // and minimatch's regex requires a synthetic trailing `/`
        // that `path.posix.resolve` always strips, so only the
        // path-to-regexp branch is active for trailing-`\` sources.
        let rules = compile(&[rule("/u/foo\\", "/literal-trailing-bs", None)]);
        // Path without trailing backslash: must NOT match.
        assert!(compute_configured_redirects("/u/foo", &rules).is_none());
        // Path with literal trailing backslash (decoded from %5C):
        // must match.
        let (target, _) = compute_configured_redirects("/u/foo\\", &rules)
            .expect("trailing-`\\` source should match path with literal trailing `\\`");
        assert_eq!(target, "/literal-trailing-bs");
    }

    #[test]
    fn literal_source_with_inner_escape_matches_both_forms() {
        // Codex review round 11 P1: source `/u/\f` (inner `\X`
        // where X is non-glob-special) matches both `/u/f` (the
        // path-to-regexp de-escaped form) AND `/u/\f` (minimatch's
        // segment-level matcher accepts the raw form, empirically
        // verified via `minimatch('/u/\\f', '/u/\\f') === true`).
        let rules = compile(&[rule("/u/\\f", "/inner-esc", None)]);
        let (target, _) = compute_configured_redirects("/u/f", &rules)
            .expect("de-escaped form should match `/u/f`");
        assert_eq!(target, "/inner-esc");
        let (target, _) = compute_configured_redirects("/u/\\f", &rules)
            .expect("raw form should also match `/u/\\f` (minimatch transparency)");
        assert_eq!(target, "/inner-esc");
    }

    #[test]
    fn star_source_with_trailing_backslash_disables_glob_fallback() {
        // Codex review round 11 P1: source `/v/*\` (Pattern with
        // glob meta `*` plus trailing `\`). The Pattern's primary
        // regex correctly requires a literal `\` at end of path
        // (`(.*)\\\/?$`). The glob_fallback previously dropped the
        // trailing `\` from the pattern segment and ended up with
        // a bare `*` segment that admitted ANY single segment,
        // over-matching `/v/x`. After the round-11 fix, the
        // glob_fallback is disabled when the source body ends with
        // an unescaped `\` (minimatch's compiled regex requires a
        // synthetic trailing `/` that `path.posix.resolve` always
        // strips, so the fallback can never match a resolved path
        // anyway).
        let rules = compile(&[rule("/v/*\\", "/glob-trailing-bs", None)]);
        // Path with literal trailing `\`: matches via Pattern's
        // primary regex, captures `x`.
        let (target, _) = compute_configured_redirects("/v/x\\", &rules)
            .expect("Pattern regex should match path with literal `\\` end");
        assert_eq!(target, "/glob-trailing-bs");
        // Path without trailing `\`: must NOT match (regex
        // requires literal `\`, glob_fallback disabled).
        assert!(compute_configured_redirects("/v/x", &rules).is_none());
    }

    #[test]
    fn literal_source_matches_case_insensitively_via_ptr_branch() {
        // Codex review round 12 P1: path-to-regexp v3.3.0 ships
        // the compiled regex with the `i` flag by default, so a
        // literal source like `/Case` matches a request path
        // `/case`. Empirical: `pathToRegExp('/Case').flags === 'i'`.
        let rules = compile(&[rule("/Case", "/literal-case", None)]);
        let (target, _) = compute_configured_redirects("/case", &rules)
            .expect("Literal source `/Case` should match `/case` (path-to-regexp default `i`)");
        assert_eq!(target, "/literal-case");
        // Same-case still matches.
        let (target, _) = compute_configured_redirects("/Case", &rules).unwrap();
        assert_eq!(target, "/literal-case");
        // Mixed case — anywhere in the source — also matches.
        let rules2 = compile(&[rule("/MyPath/Sub", "/m", None)]);
        assert!(compute_configured_redirects("/mypath/sub", &rules2).is_some());
        assert!(compute_configured_redirects("/MYPATH/SUB", &rules2).is_some());
    }

    #[test]
    fn pattern_source_matches_case_insensitively() {
        // Codex review round 12 P1: the Pattern matcher's regex
        // is now built with `RegexBuilder::case_insensitive(true)`,
        // mirroring path-to-regexp's default `i`. Both `:name`
        // and `*` sources are affected.
        let rules = compile(&[rule("/P/:id", "/param/:id", None)]);
        let (target, _) = compute_configured_redirects("/p/Foo", &rules)
            .expect("`/P/:id` should match `/p/Foo` case-insensitively");
        assert_eq!(target, "/param/Foo");
        let rules2 = compile(&[rule("/S/*", "/star", None)]);
        let (target, _) = compute_configured_redirects("/s/x", &rules2)
            .expect("`/S/*` should match `/s/x`");
        assert_eq!(target, "/star");
    }

    #[test]
    fn literal_source_matches_latin1_case_insensitively() {
        // Codex review round 13 P1: `eq_ignore_ascii_case` (round
        // 12) handled ASCII case-folding only, so source `/Ä`
        // missed request `/ä` while reference (path-to-regexp v3.3.0
        // with default `i` flag) matched. Empirical: `/Ä/i.test('ä')`
        // is true in JS. Round 13 switched to Unicode-aware
        // `to_lowercase`, which folds Latin-1 letters with
        // diacritics — the practical case for German/French/Spanish
        // URLs.
        let rules = compile(&[rule("/Ä", "/umlaut", None)]);
        let (target, _) = compute_configured_redirects("/ä", &rules)
            .expect("Latin-1 `/Ä` should match `/ä` (path-to-regexp `i` flag, simple folds)");
        assert_eq!(target, "/umlaut");
        let rules2 = compile(&[rule("/É/path", "/eacute", None)]);
        assert!(compute_configured_redirects("/é/path", &rules2).is_some());
    }

    #[test]
    fn pattern_source_matches_latin1_case_insensitively() {
        // Same fix applies to Pattern matchers. The Rust regex was
        // already Unicode-aware (`case_insensitive(true)` defaults
        // to Unicode folding), so this test is a control: confirm
        // round 13's documented behavior holds for `:name` and `*`
        // sources too.
        let rules = compile(&[rule("/Ö/:id", "/oslash/:id", None)]);
        let (target, _) = compute_configured_redirects("/ö/Foo", &rules)
            .expect("Latin-1 `/Ö/:id` should match `/ö/Foo`");
        assert_eq!(target, "/oslash/Foo");
        let rules2 = compile(&[rule("/Ü/*", "/uumlaut", None)]);
        assert!(compute_configured_redirects("/ü/x", &rules2).is_some());
    }

    #[test]
    fn glob_source_remains_case_sensitive() {
        // Codex review round 12 P1 control case: minimatch
        // (the Glob matcher and Pattern's glob_fallback) is
        // case-sensitive by default (`nocase: false`). Sources
        // that route through the Glob path remain case-sensitive
        // — the empirical reference behavior is that
        // `minimatch('/g/A', '/G/?')` is false.
        let rules = compile(&[rule("/G/?", "/g", None)]);
        // path-to-regexp parses `/G/?` as literal `?` (not a glob),
        // so the regex match fails for `/g/a`. minimatch sees
        // `?` as single-char glob but is case-sensitive, so
        // `/G/?` against `/g/a` (lowercase `g`) does NOT match.
        // Reference returns 404 here too (control case).
        assert!(compute_configured_redirects("/g/a", &rules).is_none());
        // Same-case path matches via minimatch single-char.
        let (target, _) = compute_configured_redirects("/G/a", &rules)
            .expect("`/G/?` should match `/G/a` (same case, ? glob)");
        assert_eq!(target, "/g");
    }

    #[test]
    fn ends_with_unescaped_backslash_helper() {
        // Sanity-check the helper that drives the round-11 P1
        // glob_fallback gating decision.
        assert!(super::ends_with_unescaped_backslash("/u/foo\\"));
        assert!(super::ends_with_unescaped_backslash("\\"));
        assert!(super::ends_with_unescaped_backslash("/v/*\\"));
        // Even number of trailing backslashes: the last `\` is
        // itself escaped, so the body does NOT end unescaped.
        assert!(!super::ends_with_unescaped_backslash("/u/foo\\\\"));
        assert!(!super::ends_with_unescaped_backslash("/u/foo"));
        assert!(!super::ends_with_unescaped_backslash(""));
    }

    #[test]
    fn wildcard_with_escaped_star_keeps_glob_meta() {
        // Codex review round 9 P1: minimatch 3.1.5 (the version
        // pinned by serve-handler) treats `\X` for X in `*`/`?`/
        // `[`/`{` as transparent — the backslash is stripped but
        // the meta-character keeps its glob meaning. Empirical:
        // `m('/s/foo', '/s/\\*')` returns true. Round 8's
        // `backslash_escape(true)` made globset interpret `\*` as
        // a literal `*`, under-matching real paths. Round 9
        // strips `\` uniformly and keeps the meta meaning.
        let rules = compile(&[rule("/s/\\*", "/hit", None)]);
        let (target, _) = compute_configured_redirects("/s/foo", &rules)
            .expect("`\\*` should match any single segment (the `*` keeps glob meaning)");
        assert_eq!(target, "/hit");
        // Literal `*` in the path also matches (the `*` glob
        // matches the literal char `*`).
        assert!(compute_configured_redirects("/s/*", &rules).is_some());
    }

    #[test]
    fn wildcard_with_escaped_question_keeps_glob_meta() {
        let rules = compile(&[rule("/q/\\?", "/hit", None)]);
        let (target, _) = compute_configured_redirects("/q/a", &rules)
            .expect("`\\?` should match any single character");
        assert_eq!(target, "/hit");
    }

    #[test]
    fn wildcard_with_escaped_bracket_keeps_glob_meta() {
        let rules = compile(&[rule("/br/\\[ab]", "/hit", None)]);
        let (target, _) = compute_configured_redirects("/br/a", &rules)
            .expect("`\\[ab]` should match `a` or `b` (bracket class still active)");
        assert_eq!(target, "/hit");
        let (target, _) = compute_configured_redirects("/br/b", &rules).unwrap();
        assert_eq!(target, "/hit");
    }

    #[test]
    fn wildcard_with_escaped_brace_keeps_glob_meta() {
        let rules = compile(&[rule("/bc/\\{a,b}", "/hit", None)]);
        let (target, _) = compute_configured_redirects("/bc/a", &rules)
            .expect("`\\{a,b}` should expand alternation");
        assert_eq!(target, "/hit");
        let (target, _) = compute_configured_redirects("/bc/b", &rules).unwrap();
        assert_eq!(target, "/hit");
    }

    #[test]
    fn literal_source_de_escapes_backslash_dot() {
        // Codex review round 8 P2: minimatch and path-to-regexp
        // treat `\X` as a literal `X`. A source segment `\.x`
        // should match a request path `.x`. The pre-round-8
        // implementation kept the backslash in `Literal.source`
        // and the comparison failed.
        let rules = compile(&[rule("/g/\\.x", "/hit", None)]);
        let (target, _) = compute_configured_redirects("/g/.x", &rules)
            .expect("`\\.` source should literal-match a `.` path segment");
        assert_eq!(target, "/hit");
    }

    #[test]
    fn wildcard_with_escaped_dot_admits_leading_dot_path() {
        // Source `/g/\.*` is a Wildcard segment (has `*`). The
        // dot rule checks the EFFECTIVE first character — `\.`
        // escapes to literal `.`, so the segment effectively
        // starts with `.` and admits a leading-dot path.
        // Globset's compiled regex for `\.*` matches `.X` (the
        // backslash escapes the dot, then `*` matches anything).
        let rules = compile(&[rule("/g/\\.*", "/hit", None)]);
        let (target, _) = compute_configured_redirects("/g/.x", &rules)
            .expect("`\\.*` should admit a `.x` path segment via the effective-first-char dot rule");
        assert_eq!(target, "/hit");
    }

    #[test]
    fn star_source_rejects_trailing_slash_only_path() {
        // Codex review round 7 P1.1: reference applies
        // `path.posix.resolve(requestPath)` BEFORE both
        // pathToRegExp and minimatch, dropping a single trailing
        // `/` (except `/` itself). irserve now trims at the top
        // of `try_match`. For source `/a/*`, request `/a/` is
        // resolved to `/a`, which the regex (require `/(.*)`)
        // doesn't match — both pathToRegExp and minimatch fail
        // in reference, returning 404.
        let rules = compile(&[rule("/a/*", "/hit", None)]);
        assert!(compute_configured_redirects("/a/", &rules).is_none());
        assert!(compute_configured_redirects("/a", &rules).is_none());
        assert!(compute_configured_redirects("/a/x", &rules).is_some());
    }

    #[test]
    fn param_plus_star_rejects_trailing_slash_only_path() {
        // Same trim behavior with `:name`+`*` source. Reference
        // would resolve `/a/foo/` to `/a/foo`, which fails the
        // regex (need `:id` then `/(.*)`).
        let rules = compile(&[rule("/a/:id/*", "/new/:id", None)]);
        assert!(compute_configured_redirects("/a/foo/", &rules).is_none());
        assert!(compute_configured_redirects("/a/foo", &rules).is_none());
        let (target, _) = compute_configured_redirects("/a/foo/x", &rules)
            .expect(":id+* should match three-segment path");
        assert_eq!(target, "/new/foo");
    }

    #[test]
    fn doublestar_after_param_admits_zero_segments() {
        // Codex review round 7 P1.2: trailing `**` after a
        // `:name` segment must be optional (zero-segment match).
        // path-to-regexp's `(.*)*` parses the trailing `*` as a
        // modifier making the whole segment optional+repeat. Our
        // segment-aware compiler now emits `(?:/(.*))?` for `**`
        // segments, mirroring that behavior. Captured `:id` is
        // substituted into the destination via the regex match.
        let rules = compile(&[rule("/a/:id/**", "/new/:id", None)]);
        let (target, _) = compute_configured_redirects("/a/foo", &rules)
            .expect("**` should admit zero trailing segments");
        assert_eq!(target, "/new/foo");
        let (target, _) = compute_configured_redirects("/a/foo/extra", &rules).unwrap();
        assert_eq!(target, "/new/foo");
        let (target, _) =
            compute_configured_redirects("/a/foo/x/y/z", &rules).unwrap();
        assert_eq!(target, "/new/foo");
    }

    #[test]
    fn doublestar_in_middle_admits_zero_segments_with_literal_after() {
        // Round 7 P1.2 alongside zero-trailing match: `**` in the
        // MIDDLE of a pattern (with a literal segment after it)
        // must accept zero-skip so that `/a/:id/**/z` matches
        // `/a/foo/z`. The regex's `(?:/(.*))?` is optional, so the
        // trailing `/z` literal can match directly after the
        // `:id` capture.
        let rules = compile(&[rule("/a/:id/**/z", "/new/:id", None)]);
        let (target, _) = compute_configured_redirects("/a/foo/z", &rules)
            .expect("** in middle should admit zero segments before /z literal");
        assert_eq!(target, "/new/foo");
        let (target, _) =
            compute_configured_redirects("/a/foo/extra/z", &rules).unwrap();
        assert_eq!(target, "/new/foo");
    }

    #[test]
    fn negated_doublestar_at_end_requires_at_least_one_segment() {
        // Codex review round 7 P1.3: under negation (which routes
        // through `Matcher::Glob` + `match_segments`), the
        // DoubleStar branch mirrors minimatch's stricter `**`
        // semantics — `**` at the END of a pattern requires at
        // least ONE segment to consume. So `!/a/**` against `/a`
        // matches via negation (the inner `/a/**` doesn't match
        // `/a` per minimatch). The asymmetry between positive
        // (regex, permissive) and negation (segment-matcher,
        // strict) mirrors `index.js:46-67`.
        let rules = compile(&[rule("!/a/**", "/hit", None)]);
        let (target, _) = compute_configured_redirects("/a", &rules)
            .expect("negated /a/** should match /a (inner pattern fails to match per minimatch)");
        assert_eq!(target, "/hit");
        let (target, _) = compute_configured_redirects("/a/", &rules)
            .expect("trailing slash trimmed; same as /a");
        assert_eq!(target, "/hit");
        // Inner pattern matches `/a/x` (one trailing segment), so
        // negation rejects.
        assert!(compute_configured_redirects("/a/x", &rules).is_none());
        // Unrelated path also matches negation.
        let (target, _) = compute_configured_redirects("/b", &rules).unwrap();
        assert_eq!(target, "/hit");
    }

    #[test]
    fn doublestar_in_middle_glob_path_admits_zero() {
        // Round 7 P1.3 is asymmetric: only `**` at END of pattern
        // requires ≥1 segment under the segment matcher. `**` in
        // the MIDDLE allows zero-skip. Empirical reference probe
        // (with negation `!/a/**/b`) shows `/a/b`, `/a/x/b`, and
        // `/a/x/y/b` are all rejected by negation, meaning the
        // inner `/a/**/b` matches all three paths. So our
        // segment matcher MUST allow `**` in the middle to
        // consume zero segments for `/a/**/b` against `/a/b` to
        // match.
        let rules = compile(&[rule("!/a/**/b", "/hit", None)]);
        // Inner matches `/a/b` (zero `**` skip), so negation rejects.
        assert!(compute_configured_redirects("/a/b", &rules).is_none());
        // Inner matches `/a/x/b` (one `**` skip), negation rejects.
        assert!(compute_configured_redirects("/a/x/b", &rules).is_none());
        // Inner doesn't match `/a` (no trailing `b`), negation matches.
        let (target, _) = compute_configured_redirects("/a", &rules).unwrap();
        assert_eq!(target, "/hit");
    }

    #[test]
    fn glob_bracket_segment_rejects_dot_via_segment_matcher() {
        // Codex review round 6 P1: `Matcher::Glob` (sources with
        // `?`/`[`/`{` glob meta and no `*`/`:name`) must use the
        // same per-segment matcher as `Pattern`'s `glob_fallback`,
        // not raw globset full-pattern matching. globset matches
        // `[.]y` against `.y` (the bracket class contains `.`), but
        // minimatch with `dot: false` rejects (the pattern segment
        // begins with `[`, a magic char — NOT a literal `.`).
        let rules = compile(&[rule("/g/[.]y", "/hit", None)]);
        assert!(compute_configured_redirects("/g/.y", &rules).is_none());
    }

    #[test]
    fn glob_negation_with_bracket_pattern_flips_correctly() {
        // Same Glob source under `!`-prefix: `!/g/[.]y` matches
        // every path that does NOT match `/g/[.]y` (per minimatch
        // negation). Since `/g/.y` falls into the rejected set
        // (the pattern doesn't admit it), the negation matches and
        // the redirect fires.
        let rules = compile(&[rule("!/g/[.]y", "/hit", None)]);
        let (target, _) = compute_configured_redirects("/g/.y", &rules)
            .expect("negation should match a path the inner pattern rejects");
        assert_eq!(target, "/hit");
    }

    #[test]
    fn brace_alternative_starts_with_dot_admits_leading_dot_path() {
        // Codex review round 6 P1: brace expansion happens before
        // the dot rule. `{.x,y}` segment expands to either `.x`
        // (literal-leading-dot) or `y`; the `.x` alternative
        // permits a `.x` path segment.
        let rules = compile(&[rule("/a/*/b/{.x,y}", "/hit", None)]);
        let (target, _) = compute_configured_redirects("/a/q/b/.x", &rules)
            .expect("`.x` alternative inside braces should admit a `.x` path segment");
        assert_eq!(target, "/hit");
        // The `y` alternative also works for non-dot paths.
        assert!(compute_configured_redirects("/a/q/b/y", &rules).is_some());
        // A different dotfile (`.z`) doesn't match either
        // alternative.
        assert!(compute_configured_redirects("/a/q/b/.z", &rules).is_none());
    }

    #[test]
    fn brace_no_dot_alternative_rejects_dot_path() {
        // Sanity check: `{a,b}` (no dot alternative) follows the
        // standard rule — segment starts with `{` (magic), and no
        // alternative begins with literal `.`, so leading-dot
        // paths are rejected.
        let rules = compile(&[rule("/a/*/b/{x,y}", "/hit", None)]);
        assert!(compute_configured_redirects("/a/q/b/.x", &rules).is_none());
        assert!(compute_configured_redirects("/a/q/b/x", &rules).is_some());
    }

    #[test]
    fn pattern_with_param_and_question_mark_falls_back_to_glob() {
        // Codex review round 6 P2: the `glob_fallback` field now
        // builds for any Pattern matcher, not just `*`-bearing
        // ones. A source like `/a/:id/?` (no `*` but with `?`)
        // would fail under regex (since `?` is treated as a
        // literal `\?`) yet match in the reference via minimatch
        // when the path literally carries `:id`.
        let rules = compile(&[rule("/a/:id/?", "/hit", None)]);
        let (target, _) = compute_configured_redirects("/a/:id/x", &rules)
            .expect(":name + `?` should match a literal :id path via glob fallback");
        assert_eq!(target, "/hit");
    }

    #[test]
    fn dot_pattern_segment_admits_leading_dot_path() {
        // Codex review round 5 P1: minimatch's `dot: false` rejects
        // leading-dot path segments only when the corresponding
        // pattern segment doesn't itself begin with a literal `.`.
        // For source `/a/.*/b/*`, the second pattern segment is `.*`
        // (literal `.` then `*`), so a path segment `.x` IS allowed.
        let rules = compile(&[rule("/a/.*/b/*", "/hit", None)]);
        let (target, _) = compute_configured_redirects("/a/.x/b/y", &rules)
            .expect("`.*` pattern should permit a `.x` segment");
        assert_eq!(target, "/hit");
        // Sanity: a non-dot path segment should NOT match `.*` (its
        // first char must be `.`).
        assert!(compute_configured_redirects("/a/x/b/y", &rules).is_none());
    }

    #[test]
    fn bracket_pattern_segment_does_not_admit_leading_dot() {
        // Codex review round 5 P1: a pattern segment like `[.]y`
        // begins with `[` (a magic character), NOT with a literal
        // `.`, so minimatch's `dot: false` rejects path segments
        // that begin with `.` even though the bracket class
        // contains `.`. This is the rule "the pattern segment must
        // start with a LITERAL dot to permit a leading-dot path".
        let rules = compile(&[rule("/a/*/b/[.]y", "/hit", None)]);
        assert!(compute_configured_redirects("/a/x/b/.y", &rules).is_none());
        // Sanity: a non-dot bracket pattern still matches its
        // single-char class non-leading-dot positions correctly.
        // The pattern `[.]y` literal-match on a segment `.y` is
        // refused per `dot: false`, but on `.` alone it would be
        // refused too — and `y` alone doesn't have the `.` so it
        // can't match `[.]y`. The `[.]y` match space is therefore
        // empty under `dot: false`; this rule effectively never
        // fires unless `dot: true` is set (we mirror reference's
        // default).
    }

    #[test]
    fn doublestar_segment_obeys_dot_rule() {
        // Codex review round 5 P1: minimatch's `**` (globstar) also
        // obeys `dot: false` — a `**` cannot expand to a sequence
        // containing a `.`-segment unless `dot: true` is set. The
        // round-4 implementation used `has_doublestar` as a "skip
        // strict validation" escape hatch, which over-matched
        // dotfiles. The round-5 recursive matcher walks `**`
        // expansions explicitly and aborts when a dot segment
        // would have to be consumed.
        let rules = compile(&[rule("/a/**/b/*", "/hit", None)]);
        // Non-dot expansion via `**` works.
        assert!(compute_configured_redirects("/a/x/b/y", &rules).is_some());
        assert!(compute_configured_redirects("/a/x/y/b/z", &rules).is_some());
        // Dot segment under `**` blocks the match.
        assert!(compute_configured_redirects("/a/.x/b/y", &rules).is_none());
        // Dot segment in trailing `*` slot also blocks.
        assert!(compute_configured_redirects("/a/x/b/.y", &rules).is_none());
    }

    #[test]
    fn multi_star_source_rejects_dot_segments() {
        // Codex review round 4 P1: minimatch's `*` does NOT match a
        // path segment beginning with `.` (the `dot: false` default).
        // globset's `*` does match dotfiles by default, so we apply
        // a post-validation step in `GlobFallback::matches_strict`
        // that rejects when any `*`-bearing pattern segment aligns
        // with a leading-`.` path segment.
        let rules = compile(&[rule("/a/*/b/*", "/multi-hit", None)]);
        // First-`*` segment is `.x` → reject.
        assert!(compute_configured_redirects("/a/.x/b/y", &rules).is_none());
        // Second-`*` segment is `.y` → reject.
        assert!(compute_configured_redirects("/a/x/b/.y", &rules).is_none());
        // Both non-dot → still fires.
        assert!(compute_configured_redirects("/a/x/b/y", &rules).is_some());
    }

    #[test]
    fn pattern_with_param_and_star_falls_back_to_glob() {
        // Codex review round 4 P2: the reference's `sourceMatches`
        // tries pathToRegExp first AND falls back to minimatch even
        // for `:name`-bearing sources. minimatch treats `:name` as
        // literal characters, so a request that literally carries
        // `:name` in the corresponding segment matches via the
        // fallback. Build the glob fallback unconditionally for any
        // `*`-bearing source.
        let rules = compile(&[rule("/a/:id/*/b/*", "/hit", None)]);
        // Request literally containing `:id` matches via glob
        // fallback. Single-trailing-segment per minimatch.
        let (target, _) = compute_configured_redirects("/a/:id/x/b/y", &rules)
            .expect(":id-bearing source should match a literal :id segment via glob fallback");
        assert_eq!(target, "/hit");
        // Realistic path (no literal `:id`) still doesn't match
        // via either branch — pathToRegExp's `[^/]+?` for `:id`
        // would need `\*` literal in trailing position which real
        // URLs don't carry; minimatch needs the literal `:id`.
        assert!(compute_configured_redirects("/a/foo/x/b/y", &rules).is_none());
    }

    #[test]
    fn multi_star_source_matches_via_glob_fallback() {
        // Codex review round 3 P1: in `path-to-regexp@3.3.0`, bare
        // `*` is NOT recognized as a wildcard token — the
        // PATH_REGEXP doesn't capture it. After
        // `slashed.replace('*', '(.*)')`, multi-`*` sources like
        // `/a/*/b/*` become `/a/(.*)/b/*`, where the trailing `*`
        // ends up as a regex-literal `\*` in the compiled regex.
        // The reference's `sourceMatches` (`index.js:59`) then
        // falls through to `minimatch`, which treats each `*` as a
        // single-segment wildcard. We mirror via the
        // `glob_fallback` field on `Pattern`: the regex is tried
        // first (so `:name` captures still work for
        // `:`-bearing-with-`*` sources), and a `globset` matcher
        // catches multi-`*` patterns where the regex misses.
        let rules = compile(&[rule("/a/*/b/*", "/multi-hit", None)]);
        // Single-segment trailing → matches via glob fallback.
        let (target, _) = compute_configured_redirects("/a/x/b/y", &rules)
            .expect("multi-`*` should match single-segment trailing via glob fallback");
        assert_eq!(target, "/multi-hit");
        // Multi-segment trailing → no match (`*` is single-segment
        // in minimatch).
        assert!(compute_configured_redirects("/a/x/b/y/z", &rules).is_none());
        // Multi-segment middle → no match.
        assert!(compute_configured_redirects("/a/x/y/b/z", &rules).is_none());
        // No trailing segment → no match.
        assert!(compute_configured_redirects("/a/x/b", &rules).is_none());
        assert!(compute_configured_redirects("/a/x/b/", &rules).is_none());
    }

    #[test]
    fn glob_brace_alternation() {
        let rules = compile(&[rule("/dir/{a,b}.html", "/elsewhere", None)]);
        assert!(compute_configured_redirects("/dir/a.html", &rules).is_some());
        assert!(compute_configured_redirects("/dir/b.html", &rules).is_some());
        assert!(compute_configured_redirects("/dir/c.html", &rules).is_none());
    }

    #[test]
    fn glob_negation_excludes_path() {
        // `!/secret/**` matches everything outside /secret. Mirrors the
        // cleanUrls negation handling and the shared `sourceMatches`
        // codepath in the reference.
        let rules = compile(&[rule("!/secret/**", "/elsewhere", None)]);
        assert!(compute_configured_redirects("/about", &rules).is_some());
        assert!(compute_configured_redirects("/secret/foo", &rules).is_none());
    }

    // ----- ordering / first-match-wins ------------------------------

    #[test]
    fn first_match_wins() {
        let rules = compile(&[
            rule("/dir/*", "/first", None),
            rule("/dir/*", "/second", None),
        ]);
        let (target, _) = compute_configured_redirects("/dir/x", &rules).unwrap();
        assert_eq!(target, "/first");
    }

    #[test]
    fn empty_rules_is_noop() {
        let rules = compile(&[]);
        assert!(compute_configured_redirects("/anything", &rules).is_none());
    }

    // ----- compile_rules error handling -----------------------------

    #[test]
    fn compile_rules_falls_back_to_literal_on_globset_error() {
        // Codex review round 10 P2: when globset rejects the
        // de-escaped pattern (e.g. unmatched `[`), classify falls
        // back to a Literal segment with the de-escaped form. This
        // makes `/u/\[` literal-match a `[` request path. The
        // round-1..9 behavior had this rule silently dropped at
        // compile time. Both rules compile now; the
        // `bad-but-falls-back` rule literal-matches its raw form
        // (which real URL paths rarely carry).
        let rules = vec![
            rule("/good/*", "/g", None),
            rule("[invalid", "/bad", None),
        ];
        let (compiled, invalid) = compile_rules(&rules);
        assert_eq!(compiled.len(), 2);
        assert_eq!(invalid.len(), 0);
        // Valid rule still works.
        assert!(compute_configured_redirects("/good/x", &compiled).is_some());
        // The "invalid" rule literal-matches its de-escaped form
        // — `slasher_join_normalize` prepends `/` so source
        // `[invalid` becomes `/[invalid`.
        assert!(compute_configured_redirects("/[invalid", &compiled).is_some());
    }

    // ----- destination passthrough ----------------------------------

    #[test]
    fn destination_passthrough_in_slice1() {
        // Slice 1: no `:param` substitution. Destinations are passed
        // through verbatim. The `encode_uri_target` step happens in
        // dispatch.rs at the response-building boundary.
        let rules = compile(&[rule("/old", "/new with space", None)]);
        let (target, _) = compute_configured_redirects("/old", &rules).unwrap();
        assert_eq!(target, "/new with space");
    }

    #[test]
    fn destination_absolute_url_passthrough() {
        let rules = compile(&[rule("/external", "https://example.com/x", None)]);
        let (target, _) = compute_configured_redirects("/external", &rules).unwrap();
        assert_eq!(target, "https://example.com/x");
    }

    // ----- path-segment params (slice 2) ---------------------------

    #[test]
    fn pattern_single_param_substitutes() {
        let rules = compile(&[rule("/old-docs/:id", "/new-docs/:id", None)]);
        let (target, status) =
            compute_configured_redirects("/old-docs/12", &rules).expect(":id must match");
        assert_eq!(target, "/new-docs/12");
        assert_eq!(status, 301);
    }

    #[test]
    fn pattern_param_does_not_cross_slash() {
        // `:id` matches a single path segment, mirroring
        // path-to-regexp's `[^/]+` default.
        let rules = compile(&[rule("/old-docs/:id", "/new-docs/:id", None)]);
        assert!(compute_configured_redirects("/old-docs/12/extra", &rules).is_none());
    }

    #[test]
    fn pattern_multi_param_substitutes() {
        let rules = compile(&[rule("/u/:user/p/:post", "/profile/:user/posts/:post", None)]);
        let (target, _) = compute_configured_redirects("/u/alice/p/42", &rules).unwrap();
        assert_eq!(target, "/profile/alice/posts/42");
    }

    #[test]
    fn pattern_trailing_slash_flexion() {
        // path-to-regexp adds an optional trailing slash by default
        // (`/?$` anchor). `compile_source_regex` mirrors that.
        let rules = compile(&[rule("/old-docs/:id", "/new-docs/:id", None)]);
        assert!(compute_configured_redirects("/old-docs/12", &rules).is_some());
        assert!(compute_configured_redirects("/old-docs/12/", &rules).is_some());
    }

    #[test]
    fn pattern_param_value_is_uri_component_encoded() {
        // `pathToRegExp.compile` applies `encodeURIComponent` to each
        // captured value before the surrounding `encodeURI` (in
        // `dispatch.rs`) sees the full target. We mirror that by
        // pre-encoding the value here. A literal `;` becomes `%3B`
        // (encodeURIComponent encodes `;`; encodeURI does not).
        let rules = compile(&[rule("/old/:id", "/new/:id", None)]);
        let (target, _) = compute_configured_redirects("/old/a;b", &rules).unwrap();
        assert_eq!(target, "/new/a%3Bb");
    }

    #[test]
    fn pattern_param_value_space_encoded() {
        // Probably can't reach this via real HTTP without prior decode,
        // but the encoding contract should be uniform.
        let rules = compile(&[rule("/old/:id", "/new/:id", None)]);
        let (target, _) = compute_configured_redirects("/old/a b", &rules).unwrap();
        assert_eq!(target, "/new/a%20b");
    }

    #[test]
    fn pattern_param_with_explicit_type() {
        let rules = compile(&[rule("/old-docs/:id", "/new-docs/:id", Some(302))]);
        let (target, status) = compute_configured_redirects("/old-docs/7", &rules).unwrap();
        assert_eq!(target, "/new-docs/7");
        assert_eq!(status, 302);
    }

    #[test]
    fn pattern_no_match_falls_through() {
        let rules = compile(&[
            rule("/old-docs/:id", "/new-docs/:id", None),
            rule("/old", "/new", None),
        ]);
        // First rule shouldn't match `/other`; second falls through too.
        assert!(compute_configured_redirects("/other", &rules).is_none());
        // Second rule matches a different literal.
        let (target, _) = compute_configured_redirects("/old", &rules).unwrap();
        assert_eq!(target, "/new");
    }

    #[test]
    fn pattern_star_token_in_combination_with_param() {
        // A source with a `:name` routes to Pattern; an additional `*`
        // token in the same source becomes `(.*)` per the reference's
        // pre-pass at `serve-handler/src/index.js:46`. Sources with
        // `*` but no `:` continue to use globset (slice-1 path);
        // see `glob_single_star_matches_one_segment` for that case.
        let rules = compile(&[rule("/u/:user/*", "/profile/:user", None)]);
        let (target, _) = compute_configured_redirects("/u/alice/posts/42", &rules).unwrap();
        assert_eq!(target, "/profile/alice");
    }

    #[test]
    fn pattern_destination_extra_text_around_param() {
        let rules = compile(&[rule("/items/:id", "/v2/items/:id.html", None)]);
        let (target, _) = compute_configured_redirects("/items/42", &rules).unwrap();
        assert_eq!(target, "/v2/items/42.html");
    }

    #[test]
    fn pattern_negation_combo_falls_to_glob() {
        // Codex review round 2 P1: `!`-prefix + `:name` is NOT
        // rejected — the reference's `sourceMatches` falls through
        // to minimatch on this combo (`serve-handler/src/index.js:59`),
        // and minimatch treats `:name`-like fragments as literal
        // characters in a negated glob. We mirror by routing to
        // Glob with negate=true; the rule matches every path that
        // does NOT literally equal the (slasher-normalized) source.
        let rules = compile(&[rule("!/old-docs/:id", "/new", None)]);
        // Path `/old-docs/12` is NOT literally `/old-docs/:id`, so
        // the negation matches → redirect fires.
        let (target, status) = compute_configured_redirects("/old-docs/12", &rules)
            .expect("negation glob should match a non-literal path");
        assert_eq!(target, "/new");
        assert_eq!(status, 301);
        // The literal source itself, on the other hand, does NOT
        // match (negation excludes it).
        assert!(compute_configured_redirects("/old-docs/:id", &rules).is_none());
    }

    #[test]
    fn pattern_destination_param_missing_in_source_emits_empty() {
        // Defensive: if the user's destination references a `:foo`
        // that doesn't appear in the source, we render the rest of
        // the destination and emit an empty fragment for the missing
        // param. The reference would crash inside `pathToRegExp.compile`
        // at runtime; we fail open to keep the request handler alive.
        let rules = compile(&[rule("/old/:id", "/new/:other", None)]);
        let (target, _) = compute_configured_redirects("/old/12", &rules).unwrap();
        assert_eq!(target, "/new/");
    }

    // ----- compile_dest_template ----------------------------------

    #[test]
    fn dest_template_pure_literal() {
        let t = compile_dest_template("/static/path");
        assert_eq!(t.fragments.len(), 1);
        assert!(matches!(&t.fragments[0], DestFrag::Literal(s) if s == "/static/path"));
    }

    #[test]
    fn dest_template_param_only() {
        let t = compile_dest_template(":id");
        assert_eq!(t.fragments.len(), 1);
        assert!(matches!(&t.fragments[0], DestFrag::Param(n) if n == "id"));
    }

    #[test]
    fn dest_template_mixed() {
        let t = compile_dest_template("/u/:user/p/:post");
        assert_eq!(t.fragments.len(), 4);
        assert!(matches!(&t.fragments[0], DestFrag::Literal(s) if s == "/u/"));
        assert!(matches!(&t.fragments[1], DestFrag::Param(n) if n == "user"));
        assert!(matches!(&t.fragments[2], DestFrag::Literal(s) if s == "/p/"));
        assert!(matches!(&t.fragments[3], DestFrag::Param(n) if n == "post"));
    }

    #[test]
    fn dest_template_lone_colon_is_literal() {
        // A `:` not followed by [A-Za-z0-9_] is literal text.
        let t = compile_dest_template("/foo:");
        assert_eq!(t.fragments.len(), 1);
        assert!(matches!(&t.fragments[0], DestFrag::Literal(s) if s == "/foo:"));

        let t = compile_dest_template("/a:/b");
        assert_eq!(t.fragments.len(), 1);
        assert!(matches!(&t.fragments[0], DestFrag::Literal(s) if s == "/a:/b"));
    }

    // ----- has_path_param helper ----------------------------------

    #[test]
    fn has_path_param_detects_named_segment() {
        assert!(has_path_param("/old/:id"));
        assert!(has_path_param(":root"));
        assert!(has_path_param("/u/:user/posts"));
    }

    #[test]
    fn has_path_param_ignores_lone_colon() {
        assert!(!has_path_param("/foo:"));
        assert!(!has_path_param("/a:/b"));
        assert!(!has_path_param("/no-params"));
    }

    // ----- destination normalization (Q-007) -----------------------

    #[test]
    fn destination_normalize_passes_absolute_url_through() {
        let rules = compile(&[rule("/abs", "https://example.com/x", None)]);
        let (target, _) = compute_configured_redirects("/abs", &rules).unwrap();
        assert_eq!(target, "https://example.com/x");
    }

    #[test]
    fn destination_normalize_collapses_scheme_relative() {
        // Q-007 surprise: reference's `slasher(destination)` is
        // `path.posix.normalize`, which folds `//host/x` to `/host/x`.
        // Pinned by `tools/probe/snapshots/redirects-destination-forms.json`
        // anchor `scheme_relative`.
        let rules = compile(&[rule("/proto", "//example.com/x", None)]);
        let (target, _) = compute_configured_redirects("/proto", &rules).unwrap();
        assert_eq!(target, "/example.com/x");
    }

    #[test]
    fn destination_normalize_resolves_dotdot() {
        // Codex round 1 P2 fix: `path.posix.normalize` resolves `..`
        // segments, not just consecutive slashes. `a/../b` → `/b`.
        let rules = compile(&[rule("/up", "a/../b", None)]);
        let (target, _) = compute_configured_redirects("/up", &rules).unwrap();
        assert_eq!(target, "/b");

        let rules = compile(&[rule("/abs-up", "/x/../y", None)]);
        let (target, _) = compute_configured_redirects("/abs-up", &rules).unwrap();
        assert_eq!(target, "/y");
    }

    #[test]
    fn destination_normalize_drops_dot_segments() {
        let rules = compile(&[rule("/dot", "/x/./y", None)]);
        let (target, _) = compute_configured_redirects("/dot", &rules).unwrap();
        assert_eq!(target, "/x/y");
    }

    #[test]
    fn destination_normalize_dotdot_above_root_is_silent() {
        // Absolute-path `..` above root drops silently per
        // `path.posix.normalize("/../../b")` → "/b" semantics.
        let rules = compile(&[rule("/escape", "/../../b", None)]);
        let (target, _) = compute_configured_redirects("/escape", &rules).unwrap();
        assert_eq!(target, "/b");
    }

    #[test]
    fn destination_normalize_leading_dotdot_resolves() {
        // Codex review round 2 P1: `slasher` is
        // `path.posix.normalize(path.posix.join('/', value))`. The
        // `join('/', '../b')` step prepends `/` BEFORE normalize,
        // turning `../b` into `/../b`, which normalize then folds
        // to `/b` (`..`-above-root drop). Without the join step, a
        // naive `path.posix.normalize('../b')` returns `'../b'`
        // (relative `..` accumulates). Pinned by ORC-086 family.
        let rules = compile(&[rule("/up", "../b", None)]);
        let (target, _) = compute_configured_redirects("/up", &rules).unwrap();
        assert_eq!(target, "/b");
    }

    #[test]
    fn destination_normalize_empty_becomes_root() {
        // Codex review round 2 P1: empty destination joins to `/`,
        // which normalizes to `/`. Without the join-with-`/` step,
        // `path_posix_normalize("")` returns `.`, which would yield
        // a `Location: /.` — divergent from the reference's `/`.
        let rules = compile(&[rule("/empty", "", None)]);
        let (target, _) = compute_configured_redirects("/empty", &rules).unwrap();
        assert_eq!(target, "/");
    }

    #[test]
    fn destination_normalize_prepends_slash_to_relative() {
        let rules = compile(&[rule("/rel", "foo/bar", None)]);
        let (target, _) = compute_configured_redirects("/rel", &rules).unwrap();
        assert_eq!(target, "/foo/bar");
    }

    #[test]
    fn destination_normalize_passes_absolute_path_through() {
        let rules = compile(&[rule("/abs-path", "/foo/bar", None)]);
        let (target, _) = compute_configured_redirects("/abs-path", &rules).unwrap();
        assert_eq!(target, "/foo/bar");
    }

    #[test]
    fn destination_normalize_keeps_pattern_template_intact() {
        // `/old/:id` doesn't have a URL scheme before its first `:`
        // (the prefix `/old/` contains `/`), so normalization runs.
        // It should still produce a usable Pattern destination.
        let rules = compile(&[rule("/old/:id", "/new/:id", None)]);
        let (target, _) = compute_configured_redirects("/old/12", &rules).unwrap();
        assert_eq!(target, "/new/12");
    }

    #[test]
    fn destination_normalize_keeps_pattern_under_https() {
        // A Pattern destination starting with `https://` carries a
        // protocol, so normalization is a no-op; the pattern's `:id`
        // (later in the string) is still parsed by the template
        // compiler.
        let rules = compile(&[rule("/old/:id", "https://example.com/items/:id", None)]);
        let (target, _) = compute_configured_redirects("/old/42", &rules).unwrap();
        assert_eq!(target, "https://example.com/items/42");
    }

    // ----- has_protocol helper ------------------------------------

    #[test]
    fn has_protocol_recognizes_common_schemes() {
        assert!(has_protocol("http://example.com"));
        assert!(has_protocol("https://example.com"));
        assert!(has_protocol("ftp://example.com"));
        assert!(has_protocol("mailto:foo@bar"));
    }

    #[test]
    fn has_protocol_rejects_scheme_relative_and_paths() {
        assert!(!has_protocol("//example.com"));
        assert!(!has_protocol("/foo/bar"));
        assert!(!has_protocol("foo/bar"));
        assert!(!has_protocol(":id/foo"));
        assert!(!has_protocol("/old/:id"));
    }

    // ----- path_posix_normalize -----------------------------------

    #[test]
    fn posix_normalize_collapses_consecutive_slashes() {
        assert_eq!(path_posix_normalize("//example.com/x"), "/example.com/x");
        assert_eq!(path_posix_normalize("a//b///c"), "a/b/c");
    }

    #[test]
    fn posix_normalize_drops_dot_segments() {
        assert_eq!(path_posix_normalize("a/./b"), "a/b");
        assert_eq!(path_posix_normalize("./foo"), "foo");
        assert_eq!(path_posix_normalize("foo/."), "foo");
    }

    #[test]
    fn posix_normalize_resolves_dotdot_relative() {
        assert_eq!(path_posix_normalize("a/../b"), "b");
        assert_eq!(path_posix_normalize("a/b/../c"), "a/c");
    }

    #[test]
    fn posix_normalize_resolves_dotdot_absolute() {
        assert_eq!(path_posix_normalize("/a/../b"), "/b");
        assert_eq!(path_posix_normalize("/a/b/../../c"), "/c");
    }

    #[test]
    fn posix_normalize_dotdot_above_root_drops() {
        // Absolute paths drop excess `..`; matches Node's
        // `path.posix.normalize("/../../b")` → "/b".
        assert_eq!(path_posix_normalize("/../../b"), "/b");
    }

    #[test]
    fn posix_normalize_dotdot_relative_accumulates() {
        // Relative paths preserve `..` when nothing left to pop.
        assert_eq!(path_posix_normalize("../foo"), "../foo");
        assert_eq!(path_posix_normalize("../../foo"), "../../foo");
    }

    #[test]
    fn posix_normalize_empty_is_dot() {
        assert_eq!(path_posix_normalize(""), ".");
    }

    #[test]
    fn posix_normalize_root_stays_root() {
        assert_eq!(path_posix_normalize("/"), "/");
    }

    #[test]
    fn posix_normalize_preserves_trailing_slash() {
        assert_eq!(path_posix_normalize("foo/"), "foo/");
        assert_eq!(path_posix_normalize("/foo/"), "/foo/");
    }
}
