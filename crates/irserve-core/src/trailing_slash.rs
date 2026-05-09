use crate::normalize::collapse_slashes;

/// Phase 5 of the dispatcher pipeline (SRV-ROUT-003 / SRV-ROUT-004,
/// plus the multi-slash interaction from SRV-ROUT-005): decide whether
/// the URL path should 301-redirect to honor the `trailingSlash` config
/// option.
///
/// Mirrors the `shouldRedirect` slashing branch of
/// `third_party/serve-handler/src/index.js:121-185` verbatim, including
/// the multi-slash override: when `trailingSlash` is set AND the
/// (decoded) input path contains `//`, the redirect target is the
/// slash-collapsed form and a 301 is emitted regardless of whether the
/// add or strip branch otherwise applied (`index.js:158-160`). Without
/// this coupling, a request like `GET /test//` under `trailingSlash:
/// true` would be silently rerouted to `/test/` instead of producing
/// the 301 the reference emits.
///
/// Decision table (precedence top to bottom):
/// * `cfg = None`                                       -> `None` (silent)
/// * `cfg = Some(_)` AND path has `//`                  -> `Some(collapsed)`
/// * `cfg = Some(false)` AND path ends with `/` (and isn't `/` itself)
///                                                      -> `Some(stripped)`
/// * `cfg = Some(true)` AND path doesn't end with `/`,
///   has no extension, and basename doesn't start with `.`
///                                                      -> `Some(path + "/")`
/// * Otherwise                                          -> `None`
///
/// The strip branch ignores the dotfile/extension exemptions — those
/// apply only on the add branch (matches the reference).
pub fn compute_trailing_slash_redirect(decoded_path: &str, cfg: Option<bool>) -> Option<String> {
    let cfg = cfg?;

    // Multi-slash override (`index.js:158-160`). When trailingSlash is
    // set and the decoded path contains `//`, the reference unconditionally
    // emits a 301 to the slash-collapsed form, overriding whatever the
    // add/strip branches computed.
    if decoded_path.contains("//") {
        return Some(collapse_slashes(decoded_path).into_owned());
    }

    let is_trailed = decoded_path.ends_with('/');

    if !cfg && is_trailed {
        // Strip. Edge: `/` would yield an empty target -> suppress.
        if decoded_path.len() <= 1 {
            return None;
        }
        return Some(decoded_path[..decoded_path.len() - 1].to_string());
    }

    if cfg && !is_trailed {
        let basename = decoded_path.rsplit('/').next().unwrap_or("");
        if basename.is_empty() {
            return None;
        }
        // Reference uses Node's `path.parse(p)`:
        //   * `name` is the basename without its extension
        //   * `ext` is empty for dotfiles like `.htaccess`
        //   * `name.startsWith('.')` flags any leading-dot basename
        if basename.starts_with('.') {
            return None;
        }
        // `path.parse('foo.tar.gz').ext === '.gz'` — extension is the
        // tail starting from the LAST dot in basename, but only when
        // that dot is not the first character. So a non-leading dot
        // anywhere in the basename signals an extension.
        let has_extension = basename
            .char_indices()
            .skip(1)
            .any(|(_, ch)| ch == '.');
        if has_extension {
            return None;
        }
        return Some(format!("{decoded_path}/"));
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unset_is_noop() {
        assert_eq!(compute_trailing_slash_redirect("/about", None), None);
        assert_eq!(compute_trailing_slash_redirect("/about/", None), None);
    }

    #[test]
    fn unset_does_not_override_on_double_slash() {
        // When trailingSlash is unset, multi-slash collapse is silent
        // (handled by phase 3, not by this function).
        assert_eq!(compute_trailing_slash_redirect("/test//", None), None);
        assert_eq!(compute_trailing_slash_redirect("//", None), None);
    }

    #[test]
    fn add_appends_slash_to_extensionless() {
        assert_eq!(
            compute_trailing_slash_redirect("/about", Some(true)).as_deref(),
            Some("/about/")
        );
    }

    #[test]
    fn add_noop_when_already_trailed() {
        assert_eq!(compute_trailing_slash_redirect("/about/", Some(true)), None);
    }

    #[test]
    fn add_skips_path_with_extension() {
        assert_eq!(compute_trailing_slash_redirect("/foo.txt", Some(true)), None);
        assert_eq!(
            compute_trailing_slash_redirect("/foo.tar.gz", Some(true)),
            None
        );
    }

    #[test]
    fn add_skips_dotfile() {
        assert_eq!(
            compute_trailing_slash_redirect("/.htaccess", Some(true)),
            None
        );
        assert_eq!(
            compute_trailing_slash_redirect("/.well-known", Some(true)),
            None
        );
    }

    #[test]
    fn add_skips_dotfile_with_extension() {
        assert_eq!(
            compute_trailing_slash_redirect("/.bashrc.bak", Some(true)),
            None
        );
    }

    #[test]
    fn add_root_is_noop() {
        assert_eq!(compute_trailing_slash_redirect("/", Some(true)), None);
    }

    #[test]
    fn add_nested_extensionless() {
        assert_eq!(
            compute_trailing_slash_redirect("/api/users", Some(true)).as_deref(),
            Some("/api/users/")
        );
    }

    #[test]
    fn strip_removes_trailing_slash() {
        assert_eq!(
            compute_trailing_slash_redirect("/about/", Some(false)).as_deref(),
            Some("/about")
        );
    }

    #[test]
    fn strip_noop_when_not_trailed() {
        assert_eq!(compute_trailing_slash_redirect("/about", Some(false)), None);
    }

    #[test]
    fn strip_root_is_noop() {
        // `/` would strip to an empty target → suppress (matches the
        // reference's falsy-target branch).
        assert_eq!(compute_trailing_slash_redirect("/", Some(false)), None);
    }

    #[test]
    fn strip_applies_to_dotfile() {
        // The reference's strip branch has no dotfile guard.
        assert_eq!(
            compute_trailing_slash_redirect("/.htaccess/", Some(false)).as_deref(),
            Some("/.htaccess")
        );
    }

    #[test]
    fn strip_applies_to_path_with_extension() {
        assert_eq!(
            compute_trailing_slash_redirect("/foo.txt/", Some(false)).as_deref(),
            Some("/foo.txt")
        );
    }

    #[test]
    fn strip_nested() {
        assert_eq!(
            compute_trailing_slash_redirect("/api/users/", Some(false)).as_deref(),
            Some("/api/users")
        );
    }

    #[test]
    fn override_add_with_trailing_double_slash() {
        // Reference: `/test//` with trailingSlash=true -> 301 /test/.
        // Without the override, the add branch would skip (already
        // trailed) and the request would silently route. The reference
        // index.js:158-160 forces a redirect-to-collapsed when slashing
        // is on and the path has `//`.
        assert_eq!(
            compute_trailing_slash_redirect("/test//", Some(true)).as_deref(),
            Some("/test/")
        );
        assert_eq!(
            compute_trailing_slash_redirect("/test//////", Some(true)).as_deref(),
            Some("/test/")
        );
    }

    #[test]
    fn override_strip_with_trailing_double_slash() {
        // Reference: `/test//` with trailingSlash=false -> 301 /test/
        // (NOT /test, because the override replaces the strip target).
        assert_eq!(
            compute_trailing_slash_redirect("/test//", Some(false)).as_deref(),
            Some("/test/")
        );
    }

    #[test]
    fn override_internal_double_slash_add() {
        // Internal `//` with no trailing — add branch would normally
        // append `/`, but the override replaces the target with the
        // collapsed form (no trailing slash).
        assert_eq!(
            compute_trailing_slash_redirect("/test/foo//bar", Some(true)).as_deref(),
            Some("/test/foo/bar")
        );
    }

    #[test]
    fn override_root_double_slash() {
        // `//` -> collapsed `/`. Returns 301 to root.
        assert_eq!(
            compute_trailing_slash_redirect("//", Some(true)).as_deref(),
            Some("/")
        );
        assert_eq!(
            compute_trailing_slash_redirect("//", Some(false)).as_deref(),
            Some("/")
        );
    }
}
