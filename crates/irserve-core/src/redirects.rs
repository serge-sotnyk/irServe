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

use globset::{GlobBuilder, GlobMatcher};
use percent_encoding::{utf8_percent_encode, AsciiSet, CONTROLS};

use crate::config::RedirectRule;
use crate::normalize::collapse_slashes;

/// Precompiled redirect rule. Built once at server start so the
/// dispatcher can do per-request matching without recompiling globs.
#[derive(Debug)]
pub struct RedirectRuleCompiled {
    matcher: Matcher,
    status_code: Option<u16>,
}

#[derive(Debug)]
enum Matcher {
    /// Source has no glob meta-characters and no `:param` segments.
    /// Mirrors the reference's `pathToRegExp("/old", [])` behavior:
    /// the resulting regex `^/old/?$` matches both `/old` and `/old/`,
    /// so we accept an optional single trailing slash on either side.
    /// Destination is rendered verbatim.
    Literal {
        source: String,
        destination: String,
    },
    /// Source contains glob meta-characters (`*`, `?`, `[`, `{`) but no
    /// `:param` segments. Compiled via `globset` with
    /// `literal_separator(true)` so a single `*` does not cross `/`
    /// segments — matching minimatch's default. `negate` mirrors the
    /// `!`-prefix shared with cleanUrls
    /// (`serve-handler/src/glob-slash.js:8` + `sourceMatches` →
    /// `minimatch`). Destination is rendered verbatim — the reference's
    /// minimatch-fallback path also does no positional substitution.
    Glob {
        matcher: GlobMatcher,
        negate: bool,
        destination: String,
    },
    /// Source contains `:name` segments (and possibly `*` tokens).
    /// Compiled into a `regex::Regex` with named capture groups,
    /// mirroring the reference's `path-to-regexp` first-pass at
    /// `serve-handler/src/index.js:46-49`. The destination is a
    /// pre-parsed template that interpolates captured values
    /// (`encodeURIComponent` per value, mirroring
    /// `pathToRegExp.compile`).
    Pattern {
        regex: regex::Regex,
        dest_template: DestTemplate,
    },
}

/// Pre-parsed redirect destination with embedded `:name` placeholders.
/// Built once per rule at server start; rendered per match against the
/// captured groups.
#[derive(Debug)]
struct DestTemplate {
    fragments: Vec<DestFrag>,
}

#[derive(Debug)]
enum DestFrag {
    Literal(String),
    Param(String),
}

/// Mirrors JavaScript's `encodeURIComponent`. It encodes everything
/// except the unreserved set `A-Z a-z 0-9 - _ . ! ~ * ' ( )` (per
/// MDN). This is what `pathToRegExp.compile` applies to each captured
/// value before the rendered target is later passed through
/// `encodeURI` (`dispatch::encode_uri_target`).
const ENCODE_URI_COMPONENT_SET: &AsciiSet = &CONTROLS
    .add(b' ')
    .add(b'"')
    .add(b'#')
    .add(b'$')
    .add(b'%')
    .add(b'&')
    .add(b'+')
    .add(b',')
    .add(b'/')
    .add(b':')
    .add(b';')
    .add(b'<')
    .add(b'=')
    .add(b'>')
    .add(b'?')
    .add(b'@')
    .add(b'[')
    .add(b'\\')
    .add(b']')
    .add(b'^')
    .add(b'`')
    .add(b'{')
    .add(b'|')
    .add(b'}');

/// One redirect rule that failed to compile. Mirrors the reference's
/// behavior in `sourceMatches` (`serve-handler/src/index.js:38-67`),
/// where unparseable patterns are silently treated as never-matching:
/// surface invalid rules to the bin layer for stderr warnings; the
/// server keeps running and other rules continue to work.
#[derive(Debug)]
pub struct InvalidRedirect {
    pub source: String,
    pub error: CompileError,
}

#[derive(Debug, thiserror::Error)]
pub enum CompileError {
    #[error("invalid glob: {0}")]
    Glob(#[from] globset::Error),
    #[error("invalid path pattern: {0}")]
    Regex(#[from] regex::Error),
    #[error("path-segment params (:name) cannot combine with !-prefix negation")]
    NegatedParam,
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
    let matcher = if has_path_param(&body) {
        // `:name` segments route to the regex-based matcher first,
        // mirroring `sourceMatches`'s `pathToRegExp` pre-pass at
        // `serve-handler/src/index.js:46-49`. We don't try to combine
        // a `:`-bearing source with `!`-prefix negation: the reference
        // does not exercise this combination and the semantics are
        // unclear (path-to-regexp has no negation flag).
        if negate {
            return Err(CompileError::NegatedParam);
        }
        let regex = compile_source_regex(&body)?;
        let dest_template = compile_dest_template(&normalized_dest);
        Matcher::Pattern {
            regex,
            dest_template,
        }
    } else if has_glob_meta(&body) || negate {
        // Glob meta-characters route to globset (minimatch fallback in
        // the reference). A literal `!`-prefixed source also goes
        // through globset so the negation XOR can apply uniformly.
        let glob = GlobBuilder::new(&body).literal_separator(true).build()?;
        Matcher::Glob {
            matcher: glob.compile_matcher(),
            negate,
            destination: normalized_dest,
        }
    } else {
        Matcher::Literal {
            source: body,
            destination: normalized_dest,
        }
    };
    Ok(RedirectRuleCompiled {
        matcher,
        status_code: rule.kind,
    })
}

/// Mirrors the destination-side branch of
/// `serve-handler/src/index.js:80`:
///
/// ```js
/// const normalizedDest = protocol ? destination : slasher(destination);
/// ```
///
/// where `slasher` is `glob-slash`'s `path.posix.normalize` +
/// leading-slash guarantee. This is what gives the reference its
/// surprising Q-007 behavior: `//example.com/x` collapses to
/// `/example.com/x` (the `//` is folded by `path.posix.normalize`,
/// not preserved as a scheme-relative URL). We mirror exactly,
/// pinned by `tools/probe/snapshots/redirects-destination-forms.json`.
///
/// We approximate `path.posix.normalize` with `collapse_slashes` —
/// fuller normalization (`.`/`..` resolution) is not implemented
/// because real-world redirect destinations don't carry those
/// segments; if a divergence surfaces, escalate to a Q-NNN.
fn normalize_destination(dest: &str) -> String {
    if has_protocol(dest) {
        return dest.to_string();
    }
    let collapsed = collapse_slashes(dest);
    if collapsed.starts_with('/') {
        collapsed.into_owned()
    } else {
        format!("/{collapsed}")
    }
}

/// Mirrors the truthy branch of `url.parse(dest).protocol` in Node:
/// the destination has a protocol when it begins with a valid URL
/// scheme followed by `:`. A scheme is `[A-Za-z][A-Za-z0-9+.\-]*`.
///
/// `:name` template tokens never start at offset 0 (the `:` would
/// have an empty scheme prefix), so this check coexists with the
/// destination template parser without ambiguity. Likewise a path
/// like `/old/:id` has a non-alphabetic char before its first `:`,
/// so it also reads as protocol-less.
fn has_protocol(dest: &str) -> bool {
    let Some(idx) = dest.find(':') else {
        return false;
    };
    let scheme = &dest[..idx];
    if scheme.is_empty() {
        return false;
    }
    let mut chars = scheme.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    if !first.is_ascii_alphabetic() {
        return false;
    }
    chars.all(|c| c.is_ascii_alphanumeric() || c == '+' || c == '-' || c == '.')
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
        match &self.matcher {
            Matcher::Literal {
                source,
                destination,
            } => {
                if literal_matches(source, path) {
                    Some(destination.clone())
                } else {
                    None
                }
            }
            Matcher::Glob {
                matcher,
                negate,
                destination,
            } => {
                if matcher.is_match(path) ^ negate {
                    Some(destination.clone())
                } else {
                    None
                }
            }
            Matcher::Pattern {
                regex,
                dest_template,
            } => regex.captures(path).map(|caps| dest_template.render(&caps)),
        }
    }
}

impl DestTemplate {
    fn render(&self, captures: &regex::Captures) -> String {
        let mut out = String::new();
        for frag in &self.fragments {
            match frag {
                DestFrag::Literal(s) => out.push_str(s),
                DestFrag::Param(name) => {
                    if let Some(m) = captures.name(name) {
                        out.extend(utf8_percent_encode(m.as_str(), ENCODE_URI_COMPONENT_SET));
                    }
                    // If a `:name` in the destination has no matching
                    // capture, emit nothing for that fragment. The
                    // reference's `pathToRegExp.compile` would throw on
                    // a missing prop; we fail open instead so a typo
                    // doesn't crash the request handler.
                }
            }
        }
        out
    }
}

/// Mirrors `pathToRegExp("/old", [])` in `path-to-regexp@3.x`: the
/// resulting regex is `^/old/?$`, so a literal source matches both
/// `/old` and `/old/` (single optional trailing slash). We accept the
/// flexion symmetrically so that requests with or without a trailing
/// slash still hit literal redirect rules.
fn literal_matches(source: &str, path: &str) -> bool {
    if source == path {
        return true;
    }
    if !source.ends_with('/') && path.len() == source.len() + 1 && path.ends_with('/') {
        return path[..source.len()] == *source;
    }
    if !path.ends_with('/') && source.len() == path.len() + 1 && source.ends_with('/') {
        return source[..path.len()] == *path;
    }
    false
}

/// Heuristic: a source contains glob meta-characters and should be
/// compiled with `globset`. Mirrors the meta-character set of
/// minimatch's standard glob syntax (the cleanUrls-side decision in
/// Q-012 noted that extglob is not supported, so `+(...)`,
/// `@(...)`, `?(...)`, `*(...)`, `!(...)` patterns are handled as
/// the `?`/`*`-bearing strings their leading character implies).
fn has_glob_meta(source: &str) -> bool {
    source
        .chars()
        .any(|c| matches!(c, '*' | '?' | '[' | '{'))
}

/// Heuristic: a source contains a `:name` segment (where `name` is a
/// non-empty `[A-Za-z0-9_]+`). Such sources route through the regex
/// path-segment matcher. Mirrors the reference's `pathToRegExp`
/// first-pass at `serve-handler/src/index.js:46-49`.
fn has_path_param(source: &str) -> bool {
    let bytes = source.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b':' {
            let next = bytes.get(i + 1).copied();
            if matches!(next, Some(b) if b.is_ascii_alphanumeric() || b == b'_') {
                return true;
            }
        }
        i += 1;
    }
    false
}

/// Compile a `:name`-bearing source pattern into a `regex::Regex` with
/// named capture groups. Mirrors `slashed.replace('*', '(.*)')` +
/// `pathToRegExp(normalized, keys)` from
/// `serve-handler/src/index.js:46-49`:
///
/// - `:name` segments → `(?P<name>[^/]+)` (single segment, no slashes).
/// - `*` token → `(.*)` (anonymous, may cross segments).
/// - All other characters → regex-escaped literals.
/// - The regex is anchored `^...\/?$`, matching path-to-regexp's
///   default optional trailing slash.
fn compile_source_regex(slashed: &str) -> Result<regex::Regex, regex::Error> {
    let mut pattern = String::from("^");
    let bytes = slashed.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b':' => {
                let start = i + 1;
                let mut end = start;
                while end < bytes.len()
                    && (bytes[end].is_ascii_alphanumeric() || bytes[end] == b'_')
                {
                    end += 1;
                }
                if end == start {
                    pattern.push_str(&regex::escape(":"));
                    i += 1;
                } else {
                    let name = std::str::from_utf8(&bytes[start..end])
                        .expect("ascii name slice is valid utf-8");
                    pattern.push_str("(?P<");
                    pattern.push_str(name);
                    pattern.push_str(">[^/]+)");
                    i = end;
                }
            }
            b'*' => {
                pattern.push_str("(.*)");
                i += 1;
            }
            _ => {
                let start = i;
                while i < bytes.len() && bytes[i] != b':' && bytes[i] != b'*' {
                    i += 1;
                }
                let run = std::str::from_utf8(&bytes[start..i])
                    .expect("byte run from utf-8 source is valid utf-8");
                pattern.push_str(&regex::escape(run));
            }
        }
    }
    pattern.push_str("/?$");
    regex::Regex::new(&pattern)
}

/// Pre-parse a destination string into literal + `:name` fragments.
/// Mirrors what `pathToRegExp.compile(destination)` does in the
/// reference, except that we render at match time using the captures
/// from `compile_source_regex` rather than building a separate compile
/// step.
fn compile_dest_template(dest: &str) -> DestTemplate {
    let mut fragments: Vec<DestFrag> = Vec::new();
    let bytes = dest.as_bytes();
    let mut literal_start = 0;
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b':' {
            let name_start = i + 1;
            let mut name_end = name_start;
            while name_end < bytes.len()
                && (bytes[name_end].is_ascii_alphanumeric() || bytes[name_end] == b'_')
            {
                name_end += 1;
            }
            if name_end > name_start {
                if i > literal_start {
                    let lit = std::str::from_utf8(&bytes[literal_start..i])
                        .expect("literal run from utf-8 source is valid utf-8")
                        .to_string();
                    fragments.push(DestFrag::Literal(lit));
                }
                let name = std::str::from_utf8(&bytes[name_start..name_end])
                    .expect("ascii name slice is valid utf-8")
                    .to_string();
                fragments.push(DestFrag::Param(name));
                i = name_end;
                literal_start = i;
                continue;
            }
        }
        i += 1;
    }
    if literal_start < bytes.len() {
        let lit = std::str::from_utf8(&bytes[literal_start..])
            .expect("trailing literal from utf-8 source is valid utf-8")
            .to_string();
        fragments.push(DestFrag::Literal(lit));
    }
    DestTemplate { fragments }
}

/// Mirrors `slasher` from `serve-handler/src/glob-slash.js:8`: ensures
/// the source has a leading `/`. The `!`-prefix is preserved so the
/// caller can split out the negation flag.
fn slasher(pattern: &str) -> String {
    if let Some(rest) = pattern.strip_prefix('!') {
        let normalized = if rest.starts_with('/') {
            rest.to_string()
        } else {
            format!("/{rest}")
        };
        format!("!{normalized}")
    } else if pattern.starts_with('/') {
        pattern.to_string()
    } else {
        format!("/{pattern}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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

    // ----- glob source ---------------------------------------------

    #[test]
    fn glob_single_star_matches_one_segment() {
        let rules = compile(&[rule("/dir/*", "/elsewhere", None)]);
        assert!(compute_configured_redirects("/dir/page", &rules).is_some());
        // `*` does not cross `/` boundaries (literal_separator(true)).
        assert!(compute_configured_redirects("/dir/sub/page", &rules).is_none());
    }

    #[test]
    fn glob_double_star_crosses_segments() {
        let rules = compile(&[rule("/dir/**", "/elsewhere", None)]);
        assert!(compute_configured_redirects("/dir/page", &rules).is_some());
        assert!(compute_configured_redirects("/dir/sub/page", &rules).is_some());
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
    fn compile_rules_skips_invalid_glob() {
        let rules = vec![
            rule("/good/*", "/g", None),
            rule("[invalid", "/bad", None),
        ];
        let (compiled, invalid) = compile_rules(&rules);
        assert_eq!(compiled.len(), 1);
        assert_eq!(invalid.len(), 1);
        assert_eq!(invalid[0].source, "[invalid");
        // Valid rule still works.
        assert!(compute_configured_redirects("/good/x", &compiled).is_some());
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
    fn pattern_negation_combo_is_rejected() {
        // `!`-prefix + `:name` is an unsupported combination; the rule
        // is dropped with a CompileError::NegatedParam variant.
        let rules = vec![rule("!/old-docs/:id", "/new", None)];
        let (compiled, invalid) = compile_rules(&rules);
        assert_eq!(compiled.len(), 0);
        assert_eq!(invalid.len(), 1);
        assert!(matches!(invalid[0].error, CompileError::NegatedParam));
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
}
