/// Phase 5 of the dispatcher pipeline (SRV-ROUT-003 / SRV-ROUT-004):
/// decide whether the URL path should 301-redirect to honor the
/// `trailingSlash` config option.
///
/// Mirrors the `shouldRedirect` shape of
/// `third_party/serve-handler/src/index.js:121-185`:
///
/// * `Some(true)` + path doesn't end with `/` + has no extension + name
///   doesn't start with `.` → `Some(format!("{path}/"))`
/// * `Some(false)` + path ends with `/` (and isn't `/` itself) →
///   `Some(path[..len-1])`
/// * Otherwise → `None`
///
/// The strip branch ignores the dotfile/extension exemptions — the
/// reference applies them only on the add branch.
pub fn compute_trailing_slash_redirect(path: &str, cfg: Option<bool>) -> Option<String> {
    let cfg = cfg?;
    let is_trailed = path.ends_with('/');

    if !cfg && is_trailed {
        // Strip. Edge: `/` would yield an empty target → suppress.
        if path.len() <= 1 {
            return None;
        }
        return Some(path[..path.len() - 1].to_string());
    }

    if cfg && !is_trailed {
        let basename = path.rsplit('/').next().unwrap_or("");
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
        return Some(format!("{path}/"));
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
}
