use std::path::{Path, PathBuf};

use serde::Deserialize;

#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    #[error("explicit --config file not found: {0}")]
    ExplicitMissing(PathBuf),
    #[error("could not read configuration from file {path}: {source}")]
    Read {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("could not parse {path} as JSON: {source}")]
    Json {
        path: PathBuf,
        #[source]
        source: serde_json::Error,
    },
    #[error("configuration in {path} is not a JSON object")]
    NotObject { path: PathBuf },
    #[error("configuration in {path} failed validation: {source}")]
    Schema {
        path: PathBuf,
        #[source]
        source: serde_json::Error,
    },
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct ServeConfig {
    pub public: Option<String>,
    pub clean_urls: Option<BoolOrGlobs>,
    pub trailing_slash: Option<bool>,
    #[serde(default)]
    pub rewrites: Vec<RewriteRule>,
    #[serde(default)]
    pub redirects: Vec<RedirectRule>,
    #[serde(default)]
    pub headers: Vec<HeaderRule>,
    pub directory_listing: Option<BoolOrGlobs>,
    #[serde(default)]
    pub unlisted: Vec<String>,
    pub render_single: Option<bool>,
    pub symlinks: Option<bool>,
    pub etag: Option<bool>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
pub enum BoolOrGlobs {
    Bool(bool),
    Globs(Vec<String>),
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RewriteRule {
    pub source: String,
    pub destination: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RedirectRule {
    pub source: String,
    pub destination: String,
    #[serde(rename = "type")]
    pub kind: Option<u16>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HeaderRule {
    pub source: String,
    pub headers: Vec<HeaderItem>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HeaderItem {
    pub key: String,
    /// `value: null` deletes a previously-applied header with the same
    /// key (case-insensitive) — mirrors `serve-handler/src/index.js:247-251`
    /// (SRV-HDR-002). String values insert/replace as expected.
    pub value: Option<String>,
}

/// Outcome of loading `serve.json` (or its deprecated fallbacks).
///
/// `config` carries the parsed configuration. `source` records which
/// file produced it; the bin uses this to decide whether to emit a
/// deprecation warning.
#[derive(Debug, Clone)]
pub struct LoadedConfig {
    pub config: ServeConfig,
    pub source: ConfigSource,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConfigSource {
    Explicit,
    ServeJson,
    NowJson,
    PackageJson,
}

/// Load configuration following the SRV-CFG-001 lookup chain.
///
/// `explicit_path` is the value of `--config <path>` if given. When
/// present, it is the only candidate; missing file is fatal.
///
/// When absent, the implicit chain `serve.json` → `now.json` (key
/// `now.static`) → `package.json` (key `static`) is searched in
/// `served_dir`; the first candidate that yields a usable section
/// wins. Missing implicit files are silently skipped.
///
/// Returns `Ok(None)` when no configuration is found.
pub fn load_serve_json(
    served_dir: &Path,
    explicit_path: Option<&Path>,
) -> Result<Option<LoadedConfig>, ConfigError> {
    if let Some(rel) = explicit_path {
        let path = if rel.is_absolute() {
            rel.to_path_buf()
        } else {
            served_dir.join(rel)
        };
        if !path.exists() {
            return Err(ConfigError::ExplicitMissing(path));
        }
        let value = read_json(&path)?;
        let config = parse_serve_section(&path, value)?;
        return Ok(Some(LoadedConfig {
            config,
            source: ConfigSource::Explicit,
        }));
    }

    for (name, source) in [
        ("serve.json", ConfigSource::ServeJson),
        ("now.json", ConfigSource::NowJson),
        ("package.json", ConfigSource::PackageJson),
    ] {
        let path = served_dir.join(name);
        if !path.exists() {
            continue;
        }
        let value = read_json(&path)?;
        let section = match source {
            ConfigSource::ServeJson | ConfigSource::Explicit => Some(value),
            ConfigSource::NowJson => extract_nested(value, &["now", "static"]),
            ConfigSource::PackageJson => extract_nested(value, &["static"]),
        };
        let Some(section) = section else { continue };
        let config = parse_serve_section(&path, section)?;
        return Ok(Some(LoadedConfig { config, source }));
    }

    Ok(None)
}

fn read_json(path: &Path) -> Result<serde_json::Value, ConfigError> {
    let bytes = std::fs::read(path).map_err(|source| ConfigError::Read {
        path: path.to_path_buf(),
        source,
    })?;
    serde_json::from_slice(&bytes).map_err(|source| ConfigError::Json {
        path: path.to_path_buf(),
        source,
    })
}

fn extract_nested(value: serde_json::Value, keys: &[&str]) -> Option<serde_json::Value> {
    let mut cursor = value;
    for key in keys {
        let serde_json::Value::Object(mut map) = cursor else {
            return None;
        };
        cursor = map.remove(*key)?;
    }
    Some(cursor)
}

fn parse_serve_section(path: &Path, value: serde_json::Value) -> Result<ServeConfig, ConfigError> {
    if !value.is_object() {
        return Err(ConfigError::NotObject {
            path: path.to_path_buf(),
        });
    }
    serde_json::from_value(value).map_err(|source| ConfigError::Schema {
        path: path.to_path_buf(),
        source,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    fn write(dir: &Path, name: &str, content: &str) {
        fs::write(dir.join(name), content).unwrap();
    }

    #[test]
    fn missing_implicit_returns_none() {
        let dir = tempdir().unwrap();
        let result = load_serve_json(dir.path(), None).unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn empty_serve_json_is_ok() {
        let dir = tempdir().unwrap();
        write(dir.path(), "serve.json", "{}");
        let loaded = load_serve_json(dir.path(), None).unwrap().unwrap();
        assert_eq!(loaded.source, ConfigSource::ServeJson);
        assert!(loaded.config.public.is_none());
    }

    #[test]
    fn serve_json_wins_over_package_json() {
        let dir = tempdir().unwrap();
        write(dir.path(), "serve.json", r#"{"public":"site"}"#);
        write(
            dir.path(),
            "package.json",
            r#"{"static":{"public":"should-not-win"}}"#,
        );
        let loaded = load_serve_json(dir.path(), None).unwrap().unwrap();
        assert_eq!(loaded.source, ConfigSource::ServeJson);
        assert_eq!(loaded.config.public.as_deref(), Some("site"));
    }

    #[test]
    fn now_json_extracts_now_static() {
        let dir = tempdir().unwrap();
        write(
            dir.path(),
            "now.json",
            r#"{"now":{"static":{"public":"site"}}}"#,
        );
        let loaded = load_serve_json(dir.path(), None).unwrap().unwrap();
        assert_eq!(loaded.source, ConfigSource::NowJson);
        assert_eq!(loaded.config.public.as_deref(), Some("site"));
    }

    #[test]
    fn now_json_without_now_static_falls_through() {
        let dir = tempdir().unwrap();
        // now.json without `now.static` falls through to package.json.
        write(dir.path(), "now.json", r#"{"name":"x"}"#);
        write(
            dir.path(),
            "package.json",
            r#"{"static":{"public":"pkg-site"}}"#,
        );
        let loaded = load_serve_json(dir.path(), None).unwrap().unwrap();
        assert_eq!(loaded.source, ConfigSource::PackageJson);
        assert_eq!(loaded.config.public.as_deref(), Some("pkg-site"));
    }

    #[test]
    fn now_json_with_now_but_no_static_falls_through() {
        // D-004 micro-divergence: reference would crash with TypeError.
        let dir = tempdir().unwrap();
        write(dir.path(), "now.json", r#"{"now":{"name":"x"}}"#);
        write(
            dir.path(),
            "package.json",
            r#"{"static":{"public":"pkg-site"}}"#,
        );
        let loaded = load_serve_json(dir.path(), None).unwrap().unwrap();
        assert_eq!(loaded.source, ConfigSource::PackageJson);
    }

    #[test]
    fn package_json_without_static_is_skipped() {
        let dir = tempdir().unwrap();
        write(dir.path(), "package.json", r#"{"name":"x"}"#);
        let result = load_serve_json(dir.path(), None).unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn malformed_json_is_fatal() {
        let dir = tempdir().unwrap();
        write(dir.path(), "serve.json", "{not json");
        let err = load_serve_json(dir.path(), None).unwrap_err();
        assert!(matches!(err, ConfigError::Json { .. }));
    }

    #[test]
    fn non_object_root_is_fatal() {
        let dir = tempdir().unwrap();
        write(dir.path(), "serve.json", "[]");
        let err = load_serve_json(dir.path(), None).unwrap_err();
        assert!(matches!(err, ConfigError::NotObject { .. }));
    }

    #[test]
    fn unknown_field_is_rejected() {
        let dir = tempdir().unwrap();
        write(dir.path(), "serve.json", r#"{"unknownField":1}"#);
        let err = load_serve_json(dir.path(), None).unwrap_err();
        assert!(matches!(err, ConfigError::Schema { .. }));
    }

    #[test]
    fn explicit_missing_is_fatal() {
        let dir = tempdir().unwrap();
        let err = load_serve_json(dir.path(), Some(Path::new("missing.json"))).unwrap_err();
        assert!(matches!(err, ConfigError::ExplicitMissing(_)));
    }

    #[test]
    fn explicit_relative_path_resolves_to_served_dir() {
        let dir = tempdir().unwrap();
        write(dir.path(), "alt.json", r#"{"public":"alt"}"#);
        // A different file is also present — must be ignored.
        write(dir.path(), "serve.json", r#"{"public":"default"}"#);
        let loaded = load_serve_json(dir.path(), Some(Path::new("alt.json")))
            .unwrap()
            .unwrap();
        assert_eq!(loaded.source, ConfigSource::Explicit);
        assert_eq!(loaded.config.public.as_deref(), Some("alt"));
    }

    #[test]
    fn explicit_absolute_path_is_used_as_is() {
        let outer = tempdir().unwrap();
        let inner = tempdir().unwrap();
        write(outer.path(), "elsewhere.json", r#"{"public":"abs"}"#);
        let abs = outer.path().join("elsewhere.json");
        let loaded = load_serve_json(inner.path(), Some(&abs)).unwrap().unwrap();
        assert_eq!(loaded.config.public.as_deref(), Some("abs"));
    }

    #[test]
    fn clean_urls_accepts_bool() {
        let dir = tempdir().unwrap();
        write(dir.path(), "serve.json", r#"{"cleanUrls":false}"#);
        let loaded = load_serve_json(dir.path(), None).unwrap().unwrap();
        assert!(matches!(
            loaded.config.clean_urls,
            Some(BoolOrGlobs::Bool(false))
        ));
    }

    #[test]
    fn clean_urls_accepts_globs() {
        let dir = tempdir().unwrap();
        write(dir.path(), "serve.json", r#"{"cleanUrls":["/blog/*"]}"#);
        let loaded = load_serve_json(dir.path(), None).unwrap().unwrap();
        match loaded.config.clean_urls {
            Some(BoolOrGlobs::Globs(g)) => assert_eq!(g, vec!["/blog/*"]),
            other => panic!("expected globs, got {other:?}"),
        }
    }

    #[test]
    fn clean_urls_object_is_rejected() {
        let dir = tempdir().unwrap();
        write(dir.path(), "serve.json", r#"{"cleanUrls":{"foo":1}}"#);
        let err = load_serve_json(dir.path(), None).unwrap_err();
        assert!(matches!(err, ConfigError::Schema { .. }));
    }

    #[test]
    fn redirects_with_type_optional() {
        let dir = tempdir().unwrap();
        write(
            dir.path(),
            "serve.json",
            r#"{"redirects":[{"source":"/a","destination":"/b"},{"source":"/c","destination":"/d","type":302}]}"#,
        );
        let loaded = load_serve_json(dir.path(), None).unwrap().unwrap();
        assert_eq!(loaded.config.redirects.len(), 2);
        assert_eq!(loaded.config.redirects[0].kind, None);
        assert_eq!(loaded.config.redirects[1].kind, Some(302));
    }

    #[test]
    fn headers_nested_shape() {
        let dir = tempdir().unwrap();
        write(
            dir.path(),
            "serve.json",
            r#"{"headers":[{"source":"/a","headers":[{"key":"X","value":"Y"}]}]}"#,
        );
        let loaded = load_serve_json(dir.path(), None).unwrap().unwrap();
        assert_eq!(loaded.config.headers.len(), 1);
        assert_eq!(loaded.config.headers[0].headers[0].key, "X");
    }

    #[test]
    fn rewrite_rule_unknown_field_rejected() {
        let dir = tempdir().unwrap();
        write(
            dir.path(),
            "serve.json",
            r#"{"rewrites":[{"source":"/a","destination":"/b","extra":1}]}"#,
        );
        let err = load_serve_json(dir.path(), None).unwrap_err();
        assert!(matches!(err, ConfigError::Schema { .. }));
    }

    #[test]
    fn etag_field_accepted() {
        let dir = tempdir().unwrap();
        write(dir.path(), "serve.json", r#"{"etag":true}"#);
        let loaded = load_serve_json(dir.path(), None).unwrap().unwrap();
        assert_eq!(loaded.config.etag, Some(true));
    }
}
