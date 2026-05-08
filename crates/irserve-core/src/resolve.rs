use std::path::{Path, PathBuf};

pub enum ResolveOutcome {
    File(PathBuf),
    Index(PathBuf),
    NotFound,
    #[allow(dead_code)]
    EscapedRoot,
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

    if meta.is_file() {
        return ResolveOutcome::File(candidate);
    }

    if meta.is_dir() {
        let index = candidate.join("index.html");
        if let Ok(m) = tokio::fs::metadata(&index).await {
            if m.is_file() {
                return ResolveOutcome::Index(index);
            }
        }
        return ResolveOutcome::NotFound;
    }

    ResolveOutcome::NotFound
}
