use std::borrow::Cow;

/// Phase 3 of the dispatcher pipeline (SRV-ROUT-005): silently collapse
/// runs of `/` in the URL path to a single `/`. The reference performs
/// this normalization before any redirect or routing decision and never
/// emits a 301 for the collapse itself (Q-006 closed).
///
/// Returns `Cow::Borrowed` when the input contains no `//`, so the
/// happy path costs zero allocations.
pub fn collapse_slashes(path: &str) -> Cow<'_, str> {
    if !path.contains("//") {
        return Cow::Borrowed(path);
    }
    let mut out = String::with_capacity(path.len());
    let mut prev_slash = false;
    for ch in path.chars() {
        if ch == '/' {
            if !prev_slash {
                out.push('/');
            }
            prev_slash = true;
        } else {
            out.push(ch);
            prev_slash = false;
        }
    }
    Cow::Owned(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_unchanged() {
        let out = collapse_slashes("");
        assert_eq!(out, "");
        assert!(matches!(out, Cow::Borrowed(_)));
    }

    #[test]
    fn single_slash_unchanged() {
        let out = collapse_slashes("/");
        assert_eq!(out, "/");
        assert!(matches!(out, Cow::Borrowed(_)));
    }

    #[test]
    fn no_double_slash_returns_borrowed() {
        let out = collapse_slashes("/foo/bar");
        assert_eq!(out, "/foo/bar");
        assert!(matches!(out, Cow::Borrowed(_)));
    }

    #[test]
    fn root_double_slash_collapses_to_single() {
        assert_eq!(collapse_slashes("//"), "/");
    }

    #[test]
    fn root_many_slashes_collapse_to_single() {
        assert_eq!(collapse_slashes("////"), "/");
    }

    #[test]
    fn leading_double_collapses() {
        assert_eq!(collapse_slashes("//foo"), "/foo");
    }

    #[test]
    fn internal_double_collapses() {
        assert_eq!(collapse_slashes("/foo//bar"), "/foo/bar");
    }

    #[test]
    fn trailing_double_collapses_preserving_one() {
        assert_eq!(collapse_slashes("/foo//"), "/foo/");
    }

    #[test]
    fn mixed_runs_collapse() {
        assert_eq!(collapse_slashes("///a///b///"), "/a/b/");
    }

    #[test]
    fn percent_encoded_slashes_left_intact() {
        // Percent-encoded slashes are not decoded at this stage; phase 3
        // operates on the raw URI path. If decoding is added upstream
        // (see open question on `%2F%2F`), this test will need to flip.
        assert_eq!(collapse_slashes("/foo%2F%2Fbar"), "/foo%2F%2Fbar");
    }
}
