//! Shared path-pattern matcher kernel.
//!
//! This module owns the source-pattern compilation, matching, and
//! destination-template rendering primitives that the reference
//! implementation lives in `serve-handler/src/index.js:38-89`
//! (`sourceMatches` + `toTarget`). Both `redirects` (Stage 6d) and
//! `rewrites` (Stage 6e) build on top of these primitives, so the
//! kernel is factored out into its own module to avoid duplication
//! and keep matching semantics in lock-step across the two consumers.
//!
//! Module surface is `pub(crate)` — these items are crate-internal
//! plumbing for the dispatcher pipeline and are not part of the
//! crate's public API. The single exception is `CompileError`, which
//! is re-exposed via `redirects::CompileError` (and, in slice 2,
//! `rewrites::CompileError`) for callers that need to distinguish
//! invalid-pattern failures.

use globset::{GlobBuilder, GlobMatcher};
use percent_encoding::{utf8_percent_encode, AsciiSet, CONTROLS};

#[derive(Debug)]
pub(crate) enum Matcher {
    /// Source has no glob meta-characters and no `:param` segments.
    /// Stores TWO comparison forms because the reference's
    /// `sourceMatches` runs both path-to-regexp and minimatch:
    ///
    /// - `source_ptr` mirrors path-to-regexp's interpretation:
    ///   non-trailing `\X` consumes both characters and emits `X`,
    ///   trailing `\` is preserved as a literal `\`. Mirrors the
    ///   regex `^/old/?$` behavior, so we also accept an optional
    ///   trailing slash on either side.
    /// - `source_mm` mirrors minimatch's segment-level
    ///   interpretation: `\X` (for any X except `*`/`?`/`{`) is
    ///   parsed as escape + literal X, so the segment-string used
    ///   for matching is the same as `source_ptr`. The split on `\`
    ///   inside a segment is what makes minimatch *also* match
    ///   request paths that literally contain `\X` — empirically,
    ///   `minimatch('/u/\\f', '/u/\\f')` is true. We capture this
    ///   by additionally accepting the raw, un-de-escaped source
    ///   string when present (`source_mm = Some(raw)` when raw
    ///   differs from `source_ptr`).
    ///
    /// A trailing unescaped `\` in the source has no corresponding
    /// minimatch match against any `path.posix.resolve`-d request
    /// path (minimatch turns the trailing `\` into a synthetic `/`
    /// suffix that the resolved path strips), so we set
    /// `source_mm = None` in that case to avoid spurious matches.
    /// Codex review round 11 P1 surfaced both halves of this
    /// asymmetry.
    Literal {
        source_ptr: String,
        source_mm: Option<String>,
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
pub(crate) struct DestTemplate {
    pub(crate) fragments: Vec<DestFrag>,
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
pub(crate) struct GlobFallback {
    pub(crate) segments: Vec<PatSeg>,
}

#[derive(Debug)]
pub(crate) enum PatSeg {
    /// Pure literal (no glob meta-characters).
    Literal(String),
    /// Single-segment glob with at least one of `*`, `?`, `[`, `{`.
    /// `matcher` matches the full segment; `dot_matcher` is `Some`
    /// when at least one of the segment's top-level brace
    /// alternatives begins with a literal `.` — and contains a
    /// matcher built from ONLY those alternatives. Codex review
    /// round 10 P1.2: with a single boolean `starts_with_dot`,
    /// patterns like `{.x,*}` admitted any dotfile because globset's
    /// `*` alt matches dotfiles even though minimatch only allows
    /// the dot-prefixed alt to do so. The split-matcher design lets
    /// us route dotfile paths to the dot-restricted matcher and
    /// non-dot paths to the full matcher, mirroring minimatch's
    /// per-alternative `dot: false` rule.
    Wildcard {
        matcher: GlobMatcher,
        dot_matcher: Option<GlobMatcher>,
    },
    /// Globstar — matches zero or more path segments, with each
    /// expanded segment subject to the same `dot: false` rule.
    DoubleStar,
}

impl GlobFallback {
    pub(crate) fn matches_strict(&self, path: &str) -> bool {
        let path_segments: Vec<&str> =
            path.split('/').filter(|s| !s.is_empty()).collect();
        match_segments(&self.segments, &path_segments)
    }
}

pub(crate) fn match_segments(pat: &[PatSeg], path: &[&str]) -> bool {
    match pat.first() {
        None => path.is_empty(),
        Some(PatSeg::Literal(lit)) => match path.first() {
            Some(seg) if seg == lit => match_segments(&pat[1..], &path[1..]),
            _ => false,
        },
        Some(PatSeg::Wildcard {
            matcher,
            dot_matcher,
        }) => match path.first() {
            None => false,
            Some(seg) => {
                // minimatch's `dot: false` is per-alternative.
                // For dotfile paths, route to `dot_matcher` (built
                // from only the dot-prefixed brace alternatives, or
                // `None` if no such alt exists). For non-dot paths,
                // use the full `matcher`. Codex review round 10
                // P1.2.
                let chosen = if seg.starts_with('.') {
                    match dot_matcher {
                        Some(m) => m,
                        None => return false,
                    }
                } else {
                    matcher
                };
                if chosen.is_match(seg) {
                    match_segments(&pat[1..], &path[1..])
                } else {
                    false
                }
            }
        },
        Some(PatSeg::DoubleStar) => {
            // `**` matches a sequence of path segments with
            // `dot: false`: none of the consumed segments may begin
            // with `.`. Codex review round 7 P1.3: the empirical
            // contract from minimatch (per
            // `third_party/serve-handler/src/index.js:59`) is that
            // `**` at the END of a pattern requires at least ONE
            // segment to consume — `/a/**` does NOT match `/a` via
            // minimatch. `**` in the MIDDLE of a pattern (followed
            // by more pattern segments) still allows zero-skip so
            // that `/a/**/b` matches `/a/b`. The asymmetry shows
            // up via negation: `!/a/**` against `/a` fires (inner
            // didn't match), where round-6 incorrectly let `**`
            // consume zero and the negation flipped wrong. Note
            // that the POSITIVE case for `**`-bearing sources
            // routes through `Pattern`'s `regex` (which uses
            // path-to-regexp's `(.*)*` semantics — permissive,
            // matches zero), so `/a/**` against `/a` still fires
            // 301; the asymmetry mirrors the reference.
            let is_last = pat.len() == 1;
            let mut skip = if is_last { 1 } else { 0 };
            if is_last && path.is_empty() {
                return false;
            }
            // Pre-validate dot rule on the initial mandatory skip.
            for seg in &path[..skip] {
                if seg.starts_with('.') {
                    return false;
                }
            }
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

pub(crate) fn classify_pattern_segment(seg: &str) -> Result<PatSeg, globset::Error> {
    let de_escaped = de_escape(seg);
    // Codex review round 10 P1.1: the `**` check must happen on the
    // de-escaped form. Sources like `\**` produce the same
    // de-escaped string (`**`) and minimatch parses them as
    // globstar. (The `\*\*` case minimatch parses as TWO single-
    // segment globs rather than globstar — a quirk we don't
    // mirror; documented as a known divergence in D-012.)
    if de_escaped == "**" {
        return Ok(PatSeg::DoubleStar);
    }
    let has_glob = de_escaped
        .chars()
        .any(|c| matches!(c, '*' | '?' | '[' | '{'));
    if !has_glob {
        return Ok(PatSeg::Literal(de_escaped));
    }
    // If globset rejects the de-escaped pattern (e.g. unbalanced
    // `[` or `{`), fall back to a Literal segment with the
    // de-escaped form. Codex review round 10 P2 surfaced this for
    // sources like `/u/\[` — the de-escaped `[` is invalid
    // globset, but minimatch matches a literal `[` request path.
    let glob = match GlobBuilder::new(&de_escaped)
        .literal_separator(true)
        .build()
    {
        Ok(g) => g,
        Err(_) => return Ok(PatSeg::Literal(de_escaped)),
    };
    let dot_matcher = build_dot_only_matcher(&de_escaped)?;
    Ok(PatSeg::Wildcard {
        matcher: glob.compile_matcher(),
        dot_matcher,
    })
}

/// Build a `GlobMatcher` that only matches the dot-prefixed brace
/// alternatives within a segment, or `None` if no alternative
/// begins with a literal `.`. Used at match time to route dotfile
/// paths through a stricter matcher, mirroring minimatch's
/// per-alt `dot: false` rule. Codex review round 10 P1.2.
fn build_dot_only_matcher(seg: &str) -> Result<Option<GlobMatcher>, globset::Error> {
    let dot_alts = collect_dot_starting_alternatives(seg);
    if dot_alts.is_empty() {
        return Ok(None);
    }
    let pattern = if dot_alts.len() == 1 {
        dot_alts.into_iter().next().expect("len==1")
    } else {
        format!("{{{}}}", dot_alts.join(","))
    };
    let glob = match GlobBuilder::new(&pattern)
        .literal_separator(true)
        .build()
    {
        Ok(g) => g,
        Err(_) => return Ok(None),
    };
    Ok(Some(glob.compile_matcher()))
}

/// Return the alternatives within `seg` that begin with a literal
/// `.`. For a non-brace segment, returns `[seg]` if it starts with
/// `.`, else empty. For a brace segment, splits the top-level
/// alternatives and recurses.
fn collect_dot_starting_alternatives(seg: &str) -> Vec<String> {
    if seg.starts_with('.') {
        return vec![seg.to_string()];
    }
    if !seg.starts_with('{') {
        return Vec::new();
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
        return Vec::new();
    };
    let inner = &seg[1..close];
    let suffix = &seg[close + 1..];
    let mut out = Vec::new();
    for alt in split_top_level_alternatives(inner) {
        for nested in collect_dot_starting_alternatives(alt) {
            out.push(format!("{}{}", nested, suffix));
        }
    }
    out
}

/// Split a brace-inner string on top-level commas. Nested braces are
/// kept intact so `{a,{b,c}}` splits to `["a", "{b,c}"]`, allowing
/// callers (e.g. `collect_dot_starting_alternatives`) to recurse.
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
pub(crate) enum DestFrag {
    Literal(String),
    Param(String),
}

/// Mirrors JavaScript's `encodeURIComponent`. It encodes everything
/// except the unreserved set `A-Z a-z 0-9 - _ . ! ~ * ' ( )` (per
/// MDN). This is what `pathToRegExp.compile` applies to each captured
/// value before the rendered target is later passed through
/// `encodeURI` (`dispatch::encode_uri_target`).
pub(crate) const ENCODE_URI_COMPONENT_SET: &AsciiSet = &CONTROLS
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

#[derive(Debug, thiserror::Error)]
pub enum CompileError {
    #[error("invalid glob: {0}")]
    Glob(#[from] globset::Error),
    #[error("invalid path pattern: {0}")]
    Regex(#[from] regex::Error),
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
pub(crate) fn normalize_destination(dest: &str) -> String {
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
pub(crate) fn slasher_join_normalize(value: &str) -> String {
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
pub(crate) fn path_posix_normalize(s: &str) -> String {
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

/// Mirrors Node's `path.posix.resolve` for absolute-path inputs:
/// runs `path_posix_normalize` (resolves `.` / `..`, collapses
/// consecutive slashes) and then drops a single trailing `/`
/// (except for the bare root `/`). The reference's `sourceMatches`
/// (`serve-handler/src/index.js:41`) calls
/// `path.posix.resolve(requestPath)` to produce the path that
/// both pathToRegExp and minimatch see. We mirror at the top of
/// `try_match` so all matchers see the resolved form.
pub(crate) fn path_posix_resolve(path: &str) -> String {
    let normalized = path_posix_normalize(path);
    if normalized != "/" && normalized.ends_with('/') {
        normalized[..normalized.len() - 1].to_string()
    } else {
        normalized
    }
}

/// Strip backslash-escapes from a string. Mirrors minimatch /
/// path-to-regexp behavior where `\X` is a literal `X` (the
/// backslash is the escape character, not part of the literal
/// match). Used at compile time to de-escape `Literal`-variant
/// source bodies, so that source `\.x` literal-matches request
/// `.x`. Codex review round 8 P2 surfaced this gap — the
/// round-7 implementation kept the backslash in `Literal.source`
/// and the comparison against the request path failed.
///
/// Codex round 9 unified the handling: ALL segments are
/// de-escaped before classification, so the same backslash logic
/// applies to Literal, Wildcard, and DoubleStar variants.
///
/// A trailing unescaped `\` (with no character to escape) is
/// silently dropped by this function. The Literal-class compiler
/// uses the trailing-preserving variant
/// (`de_escape_keep_trailing`) instead, but per-segment classifiers
/// (used by Glob and the Pattern glob_fallback) keep the original
/// drop semantics — globset has no representation for a
/// trailing-only `\`, and the glob_fallback's contract for
/// trailing-`\`-bearing sources is "never match" (enforced at
/// compile time by skipping the fallback when the body ends with
/// an unescaped `\`).
pub(crate) fn de_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars();
    while let Some(c) = chars.next() {
        if c == '\\' {
            if let Some(next) = chars.next() {
                out.push(next);
            }
            // Trailing `\` with nothing after: drop silently.
        } else {
            out.push(c);
        }
    }
    out
}

/// Like `de_escape`, but preserves a trailing unescaped `\` as
/// a literal `\`. Mirrors path-to-regexp's behavior in v3.3.0:
/// the `\\.` regex token captures escape pairs, but a trailing
/// `\` with nothing after doesn't match the token and falls
/// through to the literal accumulator, which `escapeString`s it
/// to `\\` in the final regex (matching a literal `\` in the
/// request path).
///
/// Codex review round 11 P1: source `/u/foo\` should match a
/// request path that literally ends in `\` (e.g. decoded from
/// `%5C`), not strip the `\` and over-match `/u/foo`.
pub(crate) fn de_escape_keep_trailing(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\\' {
            if let Some(&next) = chars.peek() {
                chars.next();
                out.push(next);
            } else {
                // Trailing `\` with nothing to escape: keep as
                // literal `\`. Mirrors path-to-regexp's literal
                // accumulator branch.
                out.push('\\');
            }
        } else {
            out.push(c);
        }
    }
    out
}

/// Returns `true` when `s` ends with an unescaped `\` — i.e. an
/// odd number of trailing backslashes. Used to decide whether the
/// Pattern matcher's `glob_fallback` should be active: minimatch
/// turns a trailing `\` into a synthetic `/` suffix in the
/// compiled regex, and `path.posix.resolve` always strips trailing
/// slashes from the request path, so a trailing-`\`-bearing source
/// never matches via minimatch. Codex review round 11 P1.
pub(crate) fn ends_with_unescaped_backslash(s: &str) -> bool {
    let trailing_bs = s.chars().rev().take_while(|c| *c == '\\').count();
    trailing_bs % 2 == 1
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
pub(crate) fn has_protocol(dest: &str) -> bool {
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

impl DestTemplate {
    pub(crate) fn render(&self, captures: Option<&regex::Captures>) -> String {
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
///
/// `case_sensitive: false` mirrors path-to-regexp v3.3.0's default
/// `i` flag on the compiled regex (verified by inspecting `flags`
/// on the returned RegExp). Used for `Literal.source_ptr` matching.
/// `case_sensitive: true` is for `Literal.source_mm`, which mirrors
/// minimatch's default case-sensitive comparison. Codex review
/// round 12 P1.
///
/// Case-insensitive comparison uses Rust's Unicode-aware
/// `to_lowercase` (same semantics as
/// `regex::RegexBuilder::case_insensitive(true)`'s default), so
/// `/Ä` matches `/ä`, mirroring JS regex `i` flag's Latin-1
/// folding. The residual divergence is on Unicode special folds
/// (Kelvin sign U+212A → ASCII `k`, ﬃ ligature → `ffi`, etc.):
/// JS regex `i` *without* the `u` flag does NOT apply these folds,
/// but Rust's Unicode default DOES. Documented as a known
/// divergence in D-012 (Codex review round 13 P1) — bug-for-bug
/// parity here would require shipping a custom JS-specific
/// case-folding table for limited real-world value (URLs almost
/// never contain special-fold codepoints).
pub(crate) fn literal_matches(source: &str, path: &str, case_sensitive: bool) -> bool {
    let eq = |a: &str, b: &str| -> bool {
        if case_sensitive {
            a == b
        } else {
            // Unicode-aware lowercase. Matches Rust regex's
            // `case_insensitive(true)` semantics so the Literal
            // and Pattern matchers behave identically on the
            // path-to-regexp branch.
            a.to_lowercase() == b.to_lowercase()
        }
    };
    if eq(source, path) {
        return true;
    }
    if !source.ends_with('/') && path.len() == source.len() + 1 && path.ends_with('/') {
        return eq(&path[..source.len()], source);
    }
    if !path.ends_with('/') && source.len() == path.len() + 1 && source.ends_with('/') {
        return eq(&source[..path.len()], path);
    }
    false
}

/// Heuristic: a source contains glob meta-characters and should be
/// compiled with `globset`. Mirrors the meta-character set of
/// minimatch's standard glob syntax (the cleanUrls-side decision in
/// Q-012 noted that extglob is not supported, so `+(...)`,
/// `@(...)`, `?(...)`, `*(...)`, `!(...)` patterns are handled as
/// the `?`/`*`-bearing strings their leading character implies).
pub(crate) fn has_glob_meta(source: &str) -> bool {
    source
        .chars()
        .any(|c| matches!(c, '*' | '?' | '[' | '{'))
}

/// Heuristic: a source contains a `:name` segment (where `name` is a
/// non-empty `[A-Za-z0-9_]+`). Such sources route through the regex
/// path-segment matcher. Mirrors the reference's `pathToRegExp`
/// first-pass at `serve-handler/src/index.js:46-49`.
pub(crate) fn has_path_param(source: &str) -> bool {
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

/// Compile a source pattern into a `regex::Regex` mirroring
/// `slashed.replace('*', '(.*)')` + `pathToRegExp(normalized, keys)`
/// from `serve-handler/src/index.js:46-49`.
///
/// Compilation walks segments (split on `/`), then characters within
/// each non-`**` segment. Codex review round 7 P1.2 made this
/// segment-aware so that `**` segments compile to OPTIONAL groups
/// `(?:/(.*))?` — mirroring path-to-regexp v3's `(.*)*` token (the
/// `*` modifier makes the segment optional+repeat). With this,
/// `/a/:id/**` matches `/a/foo` (the trailing `**` segment can be
/// skipped) without needing a glob-fallback rescue.
///
/// Per-segment rules:
/// - `**` → `(?:/(.*))?` (optional multi-segment) when first such
///   substitution. Subsequent `**` segments emit literal `/**`
///   characters (mirroring JS first-only `String.replace('*',
///   '(.*)')` plus path-to-regexp's parsing of remaining stars).
/// - `:name` segments → `(?P<name>[^/]+)` (single segment, no
///   slashes).
/// - `*` (within a non-`**` segment) → first becomes `(.*)`,
///   subsequent become regex literals `\*`. Consecutive `*`s in the
///   same segment (e.g. `**` standalone segment is handled above;
///   inside other text like `prefix**` is rare) collapse to a
///   single `*` for the substitution decision.
/// - All other characters → regex-escaped literals.
/// - The regex is anchored `^...\/?$`, matching path-to-regexp's
///   default optional trailing slash.
pub(crate) fn compile_source_regex(slashed: &str) -> Result<regex::Regex, regex::Error> {
    let mut pattern = String::from("^");
    let mut star_replaced = false;
    let mut segments = slashed.split('/').peekable();
    // Skip the leading-empty segment from the `/` prefix; trailing
    // empties (from a trailing `/`) are also filtered as we walk.
    if segments.peek() == Some(&"") {
        segments.next();
    }
    for seg in segments {
        if seg.is_empty() {
            continue;
        }
        if seg == "**" {
            if !star_replaced {
                // Optional multi-segment match. The leading `/` is
                // inside the optional group, so a missing trailing
                // segment (e.g. path `/a` for source `/a/**`) is
                // accepted by the regex (positive case in
                // `Pattern`). The negation case routes through
                // `Matcher::Glob` → `match_segments` whose
                // DoubleStar branch is stricter (requires ≥1
                // segment when `**` is the last pattern element);
                // that asymmetry mirrors the reference's
                // pathToRegExp-vs-minimatch divergence.
                pattern.push_str("(?:/(.*))?");
                star_replaced = true;
            } else {
                pattern.push('/');
                pattern.push_str(&regex::escape("**"));
            }
            continue;
        }
        // Non-`**` segment: emit `/` then walk segment characters.
        pattern.push('/');
        let bytes = seg.as_bytes();
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
    }
    pattern.push_str("/?$");
    // Path-to-regexp v3.3.0 ships its compiled regex with the `i`
    // flag by default, so source `/Case` matches `/case`. Mirror via
    // `RegexBuilder::case_insensitive(true)`. Codex review round 12
    // P1.
    //
    // Round 13 P1 surfaced that Rust's Unicode-default folding
    // diverges from JS regex `i` (without the `u` flag) on special
    // folds: Kelvin sign U+212A → ASCII `k` matches in Rust but NOT
    // in JS. JS's `i` flag covers ASCII + simple Latin-1 folds
    // only. Documented as a known divergence in D-012 — bug-for-bug
    // parity would require a custom JS-specific case-folding table
    // for limited real-world value (URLs almost never contain
    // special-fold codepoints). The practical Latin-1 case (`/Ä`
    // matches `/ä`) is supported by both Rust and JS folding.
    regex::RegexBuilder::new(&pattern)
        .case_insensitive(true)
        .build()
}

/// Pre-parse a destination string into literal + `:name` fragments.
/// Mirrors what `pathToRegExp.compile(destination)` does in the
/// reference, except that we render at match time using the captures
/// from `compile_source_regex` rather than building a separate compile
/// step.
pub(crate) fn compile_dest_template(dest: &str) -> DestTemplate {
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
pub(crate) fn slasher(pattern: &str) -> String {
    if let Some(rest) = pattern.strip_prefix('!') {
        format!("!{}", slasher_join_normalize(rest))
    } else {
        slasher_join_normalize(pattern)
    }
}
