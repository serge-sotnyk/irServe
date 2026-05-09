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
    /// Source has `?`/`[`/`{` glob meta or `!`-prefix, but no `*`
    /// or `:param` segments. Stored as a `Vec<PatSeg>` (the same
    /// per-segment representation as `Pattern`'s `glob_fallback`)
    /// so minimatch's `dot: false` semantics apply uniformly.
    /// Codex review round 6 P1: the previous implementation used
    /// a single full-pattern globset matcher, which couldn't
    /// enforce per-segment dot rejection — `[.]y` would
    /// incorrectly admit a `.y` path segment because globset's
    /// brackets don't follow minimatch's leading-dot convention.
    /// `negate` mirrors the `!`-prefix shared with cleanUrls
    /// (`serve-handler/src/glob-slash.js:8` + `sourceMatches` →
    /// `minimatch`). Destination is rendered verbatim — the
    /// reference's minimatch-fallback path also does no positional
    /// substitution.
    Glob {
        segments: Vec<PatSeg>,
        negate: bool,
        destination: String,
    },
    /// Source contains `:name` segments and/or `*` tokens.
    /// Compiled into a `regex::Regex` with named capture groups,
    /// mirroring the reference's `path-to-regexp` first-pass at
    /// `serve-handler/src/index.js:46-49`. The destination is a
    /// pre-parsed template that interpolates captured values
    /// (`encodeURIComponent` per value, mirroring
    /// `pathToRegExp.compile`).
    ///
    /// `glob_fallback` is `Some` when the source has `*` —
    /// `path-to-regexp@3.3.0`'s PATH_REGEXP doesn't recognize bare
    /// `*` as a wildcard token, so after
    /// `slashed.replace('*', '(.*)')` the second-and-later `*`s
    /// pass through to the compiled regex as literal `*`
    /// characters that real URLs never carry. The reference's
    /// `sourceMatches` (`index.js:59`) ALWAYS falls through to
    /// `minimatch` after path-to-regexp returns null — including
    /// for `:name`-bearing sources, where minimatch treats `:name`
    /// as literal characters that match request paths literally
    /// containing `:name`. We mirror by trying the regex first
    /// (so `:name` captures still work for normal requests) and
    /// falling through to a globset-based matcher with
    /// minimatch-strict semantics (segment-count match, leading-
    /// `.` rejection per minimatch's `dot: false` default).
    Pattern {
        regex: regex::Regex,
        dest_template: DestTemplate,
        glob_fallback: Option<GlobFallback>,
    },
}

/// Pre-parsed redirect destination with embedded `:name` placeholders.
/// Built once per rule at server start; rendered per match against the
/// captured groups.
#[derive(Debug)]
struct DestTemplate {
    fragments: Vec<DestFrag>,
}

/// Per-segment minimatch fallback for `*`-bearing source patterns.
/// Replaces a single globset over the full pattern with a vector of
/// per-segment matchers, so that minimatch's `dot: false` default
/// can be enforced precisely: a path segment beginning with `.`
/// matches only when the corresponding pattern segment literally
/// begins with `.`. globset's `*` matches dotfiles, so we cannot
/// rely on a single full-pattern globset match alone.
///
/// Codex review round 5 P1: the round-4 implementation gated dot
/// rejection on "pattern segment contains `*`" and skipped all
/// validation for `**`-bearing patterns. Both were divergent —
/// `[.]y` and other magic-but-not-`*` patterns also obey
/// `dot: false`, and `**` does too. The fix walks pattern segments
/// classified as `Literal | Wildcard | DoubleStar` and applies the
/// dot rule per minimatch's actual semantics.
#[derive(Debug)]
struct GlobFallback {
    segments: Vec<PatSeg>,
}

#[derive(Debug)]
enum PatSeg {
    /// Pure literal (no glob meta-characters).
    Literal(String),
    /// Single-segment glob with at least one of `*`, `?`, `[`, `{`.
    /// `starts_with_dot` is the literal-leading-dot flag — true when
    /// the user-typed pattern segment starts with `.` (e.g. `.*`,
    /// `.?html`, `.foo*`), false for magic-leading segments like
    /// `*foo`, `[.]y`, `?abc`. Mirrors minimatch's
    /// "leading dot will not be matched by GLOB unless the pattern
    /// itself starts with a literal dot".
    Wildcard {
        matcher: GlobMatcher,
        starts_with_dot: bool,
    },
    /// Globstar — matches zero or more path segments, with each
    /// expanded segment subject to the same `dot: false` rule.
    DoubleStar,
}

impl GlobFallback {
    fn matches_strict(&self, path: &str) -> bool {
        let path_segments: Vec<&str> =
            path.split('/').filter(|s| !s.is_empty()).collect();
        match_segments(&self.segments, &path_segments)
    }
}

fn match_segments(pat: &[PatSeg], path: &[&str]) -> bool {
    match pat.first() {
        None => path.is_empty(),
        Some(PatSeg::Literal(lit)) => match path.first() {
            Some(seg) if seg == lit => match_segments(&pat[1..], &path[1..]),
            _ => false,
        },
        Some(PatSeg::Wildcard {
            matcher,
            starts_with_dot,
        }) => match path.first() {
            None => false,
            Some(seg) => {
                // minimatch `dot: false`: a path segment that begins
                // with `.` is NOT matched by a glob pattern unless
                // the pattern segment itself begins with a literal
                // `.`. `[.]y` first char is `[` (magic), so it does
                // NOT permit leading-dot matches even though the
                // bracket class contains `.`.
                if seg.starts_with('.') && !starts_with_dot {
                    return false;
                }
                if matcher.is_match(seg) {
                    match_segments(&pat[1..], &path[1..])
                } else {
                    false
                }
            }
        },
        Some(PatSeg::DoubleStar) => {
            // `**` matches zero or more path segments. With
            // `dot: false`, none of the consumed segments may begin
            // with `.`. Try increasing skip counts; abort once a dot
            // segment would have to be consumed.
            let mut skip = 0usize;
            loop {
                if match_segments(&pat[1..], &path[skip..]) {
                    return true;
                }
                if skip == path.len() {
                    return false;
                }
                if path[skip].starts_with('.') {
                    return false;
                }
                skip += 1;
            }
        }
    }
}

fn classify_pattern_segment(seg: &str) -> Result<PatSeg, globset::Error> {
    if seg == "**" {
        return Ok(PatSeg::DoubleStar);
    }
    let has_glob = seg
        .chars()
        .any(|c| matches!(c, '*' | '?' | '[' | '{'));
    if !has_glob {
        return Ok(PatSeg::Literal(seg.to_string()));
    }
    let glob = GlobBuilder::new(seg).literal_separator(true).build()?;
    Ok(PatSeg::Wildcard {
        matcher: glob.compile_matcher(),
        starts_with_dot: segment_can_start_with_dot(seg),
    })
}

/// Returns true when at least one expansion of the segment begins
/// with a literal `.`. Mirrors minimatch's brace expansion happening
/// before the dot rule is applied: `{.x,y}` admits a leading-dot
/// path segment because the `.x` alternative starts with `.`. The
/// segment-leading `.` itself (no braces) trivially returns true.
/// Codex review round 6 P1 — the previous implementation used a
/// raw `seg.starts_with('.')` and missed the brace case.
fn segment_can_start_with_dot(seg: &str) -> bool {
    if seg.starts_with('.') {
        return true;
    }
    if !seg.starts_with('{') {
        return false;
    }
    let bytes = seg.as_bytes();
    let mut depth = 0usize;
    let mut close_idx = None;
    for (i, &b) in bytes.iter().enumerate() {
        if b == b'{' {
            depth += 1;
        } else if b == b'}' {
            depth = depth.saturating_sub(1);
            if depth == 0 {
                close_idx = Some(i);
                break;
            }
        }
    }
    let Some(close) = close_idx else {
        return false;
    };
    let inner = &seg[1..close];
    split_top_level_alternatives(inner)
        .iter()
        .any(|alt| segment_can_start_with_dot(alt))
}

/// Split a brace-inner string on top-level commas. Nested braces are
/// kept intact so `{a,{b,c}}` splits to `["a", "{b,c}"]`, allowing
/// the recursion in `segment_can_start_with_dot` to descend.
fn split_top_level_alternatives(s: &str) -> Vec<&str> {
    let mut parts = Vec::new();
    let mut depth = 0usize;
    let mut start = 0usize;
    for (i, c) in s.char_indices() {
        match c {
            '{' => depth += 1,
            '}' => depth = depth.saturating_sub(1),
            ',' if depth == 0 => {
                parts.push(&s[start..i]);
                start = i + 1;
            }
            _ => {}
        }
    }
    parts.push(&s[start..]);
    parts
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
        let segments: Result<Vec<PatSeg>, globset::Error> = body
            .split('/')
            .filter(|s| !s.is_empty())
            .map(classify_pattern_segment)
            .collect();
        let glob_fallback = Some(GlobFallback {
            segments: segments?,
        });
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
/// where `slasher` is
/// `path.posix.normalize(path.posix.join('/', value))`
/// (`third_party/serve-handler/src/glob-slash.js:6`). The
/// `join('/', value)` step is critical: a leading-`..` input like
/// `../b` first joins to `/../b`, then `path.posix.normalize` drops
/// the `..`-above-root segment, yielding `/b`. An empty destination
/// joins to `/` and normalizes to `/`. We mirror via
/// `slasher_join_normalize` below.
///
/// Q-007 surprises pinned by
/// `tools/probe/snapshots/redirects-destination-forms.json`:
/// `//example.com/x` → `/example.com/x` (`//` collapse), `a/../b`
/// → `/b` (`..` resolution), `../b` → `/b` (leading-`..`-above-root
/// drop), `""` → `/` (empty destination becomes root).
fn normalize_destination(dest: &str) -> String {
    if has_protocol(dest) {
        return dest.to_string();
    }
    slasher_join_normalize(dest)
}

/// Mirrors `path.posix.normalize(path.posix.join('/', value))` from
/// `glob-slash.slasher`. Used both for redirect destinations
/// (`normalize_destination`) and for redirect source patterns
/// (`slasher`, after the `!`-prefix is split out). The
/// `path.posix.join('/', ...)` prepends a `/` BEFORE normalization,
/// which is what causes leading-`..` segments to land above the
/// absolute root and be silently dropped.
fn slasher_join_normalize(value: &str) -> String {
    let joined = if value.starts_with('/') {
        value.to_string()
    } else {
        format!("/{value}")
    };
    path_posix_normalize(&joined)
}

/// POSIX-style path normalization, mirroring Node's
/// `path.posix.normalize`:
///
/// - Consecutive slashes collapse to a single slash.
/// - `.` segments are dropped.
/// - `..` segments pop the previous non-`..` segment; for absolute
///   paths, `..` above the root is silently dropped; for relative
///   paths, `..` accumulates at the front when there's no segment to
///   pop.
/// - Trailing slash is preserved (except for the root `/`, which is
///   itself).
/// - Empty input normalizes to `.`.
///
/// This is what `glob-slash.slasher` runs on the destination (after
/// `path.posix.join('/', value)`) before the leading-slash guarantee.
fn path_posix_normalize(s: &str) -> String {
    if s.is_empty() {
        return ".".to_string();
    }
    let is_absolute = s.starts_with('/');
    let trailing_slash = s.len() > 1 && s.ends_with('/');

    let mut parts: Vec<&str> = Vec::new();
    for seg in s.split('/') {
        match seg {
            "" | "." => continue,
            ".." => {
                let can_pop = parts.last().is_some_and(|p| *p != "..");
                if can_pop {
                    parts.pop();
                } else if !is_absolute {
                    parts.push("..");
                }
                // Absolute path: ".." above root silently dropped.
            }
            other => parts.push(other),
        }
    }

    let mut result = String::new();
    if is_absolute {
        result.push('/');
    }
    result.push_str(&parts.join("/"));
    if trailing_slash && !result.ends_with('/') {
        result.push('/');
    }
    if result.is_empty() {
        result.push('.');
    }
    result
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
                segments,
                negate,
                destination,
            } => {
                // Trim a single trailing `/` (mirror
                // `path.posix.resolve` per `index.js:41`) so
                // segment splitting yields the same shape as
                // minimatch sees.
                let path_for_match = if path != "/" && path.ends_with('/') {
                    &path[..path.len() - 1]
                } else {
                    path
                };
                let path_segs: Vec<&str> =
                    path_for_match.split('/').filter(|s| !s.is_empty()).collect();
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
                    // Mirror `serve-handler/src/index.js:41`'s
                    // `path.posix.resolve(requestPath)` — drop a
                    // single trailing `/` (except for the root `/`)
                    // before minimatch sees the path. Without this,
                    // globset's `*` (zero-or-more) would match an
                    // empty trailing segment and `/a/x/b/` would hit
                    // a `/a/*/b/*` rule, diverging from the
                    // reference's 404. minimatch's `*` requires a
                    // non-empty segment.
                    let path_for_glob =
                        if path != "/" && path.ends_with('/') {
                            &path[..path.len() - 1]
                        } else {
                            path
                        };
                    if fb.matches_strict(path_for_glob) {
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

impl DestTemplate {
    fn render(&self, captures: Option<&regex::Captures>) -> String {
        let mut out = String::new();
        for frag in &self.fragments {
            match frag {
                DestFrag::Literal(s) => out.push_str(s),
                DestFrag::Param(name) => {
                    if let Some(caps) = captures {
                        if let Some(m) = caps.name(name) {
                            out.extend(utf8_percent_encode(m.as_str(), ENCODE_URI_COMPONENT_SET));
                        }
                    }
                    // Missing capture or no captures (glob fallback):
                    // fail open with empty string. The reference's
                    // `pathToRegExp.compile` would throw on a missing
                    // prop; we fail open instead so a typo doesn't
                    // crash the request handler.
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
/// `serve-handler/src/index.js:46-49`, with two fidelity points:
///
/// - JavaScript's `String.prototype.replace('*', '(.*)')` replaces
///   only the FIRST `*` (Codex review round 2 P1). We mirror by
///   tracking `star_replaced`: the first `*`-run becomes `(.*)`,
///   later `*`-runs are emitted as regex literals (`\*`).
/// - Consecutive `*`s collapse to a single `*` in our compiler.
///   Reference test `serve-handler/test/integration.test.js:432`
///   ("set 'redirects' config property to wildcard path") shows
///   `face/**` matching `/face/me`; after the first-only replace
///   the reference passes `face/(.*)*` to path-to-regexp v3, which
///   parses the trailing `*` as a kleene modifier on the previous
///   `(.*)` group — yielding a regex that matches anything under
///   `/face/`. Rather than port the path-to-regexp v3 modifier
///   grammar, we collapse consecutive `*`s to a single one (so
///   `**` → `(.*)`), which produces the same effective match for
///   the patterns the reference test suite exercises.
///
/// Other tokens:
/// - `:name` segments → `(?P<name>[^/]+)` (single segment, no
///   slashes).
/// - All other characters → regex-escaped literals.
/// - The regex is anchored `^...\/?$`, matching path-to-regexp's
///   default optional trailing slash.
fn compile_source_regex(slashed: &str) -> Result<regex::Regex, regex::Error> {
    let mut pattern = String::from("^");
    let bytes = slashed.as_bytes();
    let mut i = 0;
    let mut star_replaced = false;
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
                // Consume consecutive `*`s — collapse to a single
                // wildcard to match the reference's effective
                // `**`-handling (see doc comment above).
                while i + 1 < bytes.len() && bytes[i + 1] == b'*' {
                    i += 1;
                }
                if !star_replaced {
                    pattern.push_str("(.*)");
                    star_replaced = true;
                } else {
                    pattern.push_str(&regex::escape("*"));
                }
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

/// Mirrors `slasher` from `serve-handler/src/glob-slash.js:6`:
///
/// ```js
/// value.charAt(0) === '!'
///     ? '!' + path.posix.normalize(path.posix.join('/', value.substr(1)))
///     : path.posix.normalize(path.posix.join('/', value));
/// ```
///
/// The `!`-prefix is preserved verbatim so the caller can split out
/// the negation flag; the body is run through
/// `slasher_join_normalize` (i.e.
/// `path.posix.normalize(path.posix.join('/', body))`), which
/// ensures a leading `/`, collapses consecutive slashes, and
/// resolves `.`/`..` segments. Codex review round 2 P1: the
/// previous implementation only prepended `/`, so a source like
/// `../old` would compile to a literal-match against `/../old`
/// rather than the reference's `/old`.
fn slasher(pattern: &str) -> String {
    if let Some(rest) = pattern.strip_prefix('!') {
        format!("!{}", slasher_join_normalize(rest))
    } else {
        slasher_join_normalize(pattern)
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
