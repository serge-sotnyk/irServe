use std::fs::Metadata;

use axum::http::HeaderValue;

use crate::config::ServeConfig;

/// SRV-CACHE-002 / SRV-CLI-013: under `etag: false` (i.e. the
/// `--no-etag` CLI flag or an explicit `"etag": false` in
/// `serve.json`), the reference emits `Last-Modified` in place of
/// `ETag` (`third_party/serve-handler/src/index.js:227-236`). The two
/// headers are mutually exclusive per request: ETag-on branch returns
/// `None` here, ETag-off branch returns the formatted mtime.
///
/// Format is RFC 7231 IMF-fixdate (`Wed, 06 May 2026 23:39:00 GMT`)
/// via `httpdate::fmt_http_date`. Reference computes the same shape
/// via `stats.mtime.toUTCString()`. Sub-second mtime resolution is
/// not preserved in the wire format — already documented as a
/// Compatibility note on SRV-CACHE-002.
///
/// Returns `None` (no `Last-Modified` header emitted) when:
/// - `serve_config.etag != Some(false)` — ETag path; mutex.
/// - `meta` is absent — the dispatcher could not stat the file.
/// - `meta.modified()` failed — exotic platforms / filesystems
///   without a usable mtime.
/// - the formatted date string cannot be encoded as a header value
///   (defensive — the formatter only produces ASCII IMF-fixdate).
pub fn last_modified_value(
    serve_config: &ServeConfig,
    meta: Option<&Metadata>,
) -> Option<HeaderValue> {
    if serve_config.etag != Some(false) {
        return None;
    }
    let mtime = meta?.modified().ok()?;
    let formatted = httpdate::fmt_http_date(mtime);
    HeaderValue::from_str(&formatted).ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, SystemTime, UNIX_EPOCH};

    fn cfg_etag(value: Option<bool>) -> ServeConfig {
        ServeConfig {
            etag: value,
            ..ServeConfig::default()
        }
    }

    /// Pins the wire format against `httpdate::fmt_http_date`. If the
    /// upstream crate ever changes the shape, this test localises the
    /// breakage to one place.
    #[test]
    fn formats_known_mtime_as_imf_fixdate() {
        // 2021-01-01T00:00:00Z = 1609459200 unix seconds.
        let mtime = UNIX_EPOCH + Duration::from_secs(1_609_459_200);
        let formatted = httpdate::fmt_http_date(mtime);
        assert_eq!(formatted, "Fri, 01 Jan 2021 00:00:00 GMT");
    }

    /// Mutex direction 1: when ETag is on (default or explicit
    /// `true`), Last-Modified is suppressed even when metadata is
    /// available. Mirrors `index.js:227-236` `else` branch.
    #[test]
    fn etag_on_suppresses_last_modified() {
        let tmp = tempfile::NamedTempFile::new().expect("tempfile");
        let meta = tmp.as_file().metadata().expect("metadata");

        // Default config — etag field is `None`, treated as ETag-on.
        let cfg = ServeConfig::default();
        assert!(last_modified_value(&cfg, Some(&meta)).is_none());

        // Explicit `etag: true`.
        let cfg = cfg_etag(Some(true));
        assert!(last_modified_value(&cfg, Some(&meta)).is_none());
    }

    /// Mutex direction 2: under `etag: false`, Last-Modified is
    /// emitted from the file's mtime.
    #[test]
    fn etag_off_emits_last_modified_when_meta_present() {
        let tmp = tempfile::NamedTempFile::new().expect("tempfile");
        let meta = tmp.as_file().metadata().expect("metadata");
        let cfg = cfg_etag(Some(false));

        let value = last_modified_value(&cfg, Some(&meta)).expect("header value");
        let s = value.to_str().expect("ascii");

        // IMF-fixdate shape sanity: ends with " GMT", has commas in
        // expected positions, length matches the canonical 29-char
        // format. Full string equality is volatile (depends on the
        // file's actual mtime), so we shape-check only.
        assert!(s.ends_with(" GMT"));
        assert_eq!(s.len(), 29);
        assert_eq!(&s[3..5], ", ");
    }

    /// `etag: false` + no metadata → graceful degradation, no header.
    #[test]
    fn etag_off_without_meta_returns_none() {
        let cfg = cfg_etag(Some(false));
        assert!(last_modified_value(&cfg, None).is_none());
    }
}
