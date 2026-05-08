use std::path::Path;

pub fn mime_for(path: &Path) -> Option<&'static str> {
    let ext = path.extension()?.to_str()?.to_ascii_lowercase();
    match ext.as_str() {
        "html" => Some("text/html; charset=utf-8"),
        "js" => Some("application/javascript; charset=utf-8"),
        "json" => Some("application/json; charset=utf-8"),
        "css" => Some("text/css; charset=utf-8"),
        "txt" => Some("text/plain; charset=utf-8"),
        "svg" => Some("image/svg+xml"),
        "wasm" => Some("application/wasm"),
        "png" => Some("image/png"),
        _ => mime_guess::from_path(path).first_raw(),
    }
}
