use std::path::{Path, PathBuf};

#[derive(Debug)]
pub enum ResolveOutcome {
    File(PathBuf),
    Index(PathBuf),
    NotFound,
    EscapedRoot,
}

enum Kind {
    File,
    Index,
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
            _ => return ResolveOutcome::NotFound,
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
    }
}
