use std::path::Path;

use sha1::{Digest, Sha1};

/// SRV-CACHE-001 / D-017: ETag value = `"<sha1-hex>"` where the hash is
/// computed over `extname` (with the leading dot, empty when the file
/// has none) + literal `-` + the file bytes, mirroring
/// `third_party/serve-handler/src/index.js:24-36`. Round-trip behavior
/// is the contract; the hash function is implementation-defined per the
/// inventory note on SRV-CACHE-001. irserve mirrors the reference
/// formula so probe values match byte-for-byte.
pub fn compute_etag(path: &Path, bytes: &[u8]) -> String {
    let mut hasher = Sha1::new();
    if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
        hasher.update(b".");
        hasher.update(ext.as_bytes());
    }
    hasher.update(b"-");
    hasher.update(bytes);
    let digest = hasher.finalize();
    let mut hex = String::with_capacity(2 + digest.len() * 2);
    hex.push('"');
    for byte in digest {
        use std::fmt::Write as _;
        let _ = write!(hex, "{byte:02x}");
    }
    hex.push('"');
    hex
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    /// Pins byte-equality with the reference snapshot at
    /// `tools/probe/snapshots/etag-roundtrip.json` — same fixture
    /// (`body{color:red}\n` named `asset.css`), same hash. If this
    /// breaks, either the reference changed or our formula drifted.
    #[test]
    fn matches_reference_for_asset_css() {
        let path = PathBuf::from("asset.css");
        let etag = compute_etag(&path, b"body{color:red}\n");
        assert_eq!(etag, "\"3638b78821a961fcf35969f0bc67cc5944d64a0b\"");
    }

    #[test]
    fn extensionless_file_uses_empty_extname() {
        // Node's path.extname('LICENSE') → '', so hash input is just
        // `-` + contents. Mirror that.
        let path = PathBuf::from("LICENSE");
        let etag_no_ext = compute_etag(&path, b"hello");

        // Verify a different extension produces a different hash for
        // the same body — this is the whole point of the extname
        // prefix in the reference.
        let path_with_ext = PathBuf::from("LICENSE.txt");
        let etag_with_ext = compute_etag(&path_with_ext, b"hello");

        assert_ne!(etag_no_ext, etag_with_ext);
    }

    #[test]
    fn deterministic_across_calls() {
        let path = PathBuf::from("a.css");
        let etag1 = compute_etag(&path, b"x");
        let etag2 = compute_etag(&path, b"x");
        assert_eq!(etag1, etag2);
    }

    #[test]
    fn format_is_quoted_40_lowercase_hex() {
        let path = PathBuf::from("any.txt");
        let etag = compute_etag(&path, b"");
        assert_eq!(etag.len(), 42, "should be 2 quotes + 40 hex chars");
        assert!(etag.starts_with('"') && etag.ends_with('"'));
        let inner = &etag[1..etag.len() - 1];
        assert!(inner.chars().all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase()));
    }
}
