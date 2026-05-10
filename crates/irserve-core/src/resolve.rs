use std::path::{Path, PathBuf};

#[derive(Debug)]
pub enum ResolveOutcome {
    File(PathBuf),
    Index(PathBuf),
    /// The URL resolved to an existing directory that has no
    /// `index.html`. Phase 11 of the dispatcher decides whether to
    /// render a directory listing, fire `renderSingle`, or fall
    /// through to 404 (Stage 6g). The carried path is the
    /// canonicalized absolute filesystem path of the directory,
    /// already containment-checked against `root`.
    //
    // Slice 1 plumbing only: the path is unread until Slice 2 wires
    // the listing renderer.
    Directory(#[allow(dead_code)] PathBuf),
    NotFound,
    EscapedRoot,
}

enum Kind {
    File,
    Index,
    Directory,
}

pub async fn resolve(url_path: &str, root: &Path) -> ResolveOutcome {
    let trimmed = url_path.trim_start_matches('/');
    let candidate = if trimmed.is_empty() {
        root.to_path_buf()
    } else {
        root.join(trimmed)
    };

    let meta = match tokio::fs::metadata(&candidate).await {
        Ok(m) => m,
        Err(_) => return ResolveOutcome::NotFound,
    };

    let (resolved_path, kind) = if meta.is_file() {
        (candidate, Kind::File)
    } else if meta.is_dir() {
        let index = candidate.join("index.html");
        match tokio::fs::metadata(&index).await {
            Ok(m) if m.is_file() => (index, Kind::Index),
            _ => (candidate, Kind::Directory),
        }
    } else {
        return ResolveOutcome::NotFound;
    };

    let canonical = match tokio::fs::canonicalize(&resolved_path).await {
        Ok(p) => p,
        Err(_) => return ResolveOutcome::NotFound,
    };

    if !canonical.starts_with(root) {
        return ResolveOutcome::EscapedRoot;
    }

    match kind {
        Kind::File => ResolveOutcome::File(canonical),
        Kind::Index => ResolveOutcome::Index(canonical),
        Kind::Directory => ResolveOutcome::Directory(canonical),
    }
}
