
//! Resolve Envision's derived data under the platform application-data directory.
use std::path::{Path, PathBuf};
use tauri::{AppHandle, Manager};

#[derive(Debug, thiserror::Error)]
pub enum PlatformError {
    #[error("Could not resolve the application data directory: {0}")]
    AppData(String),
    #[error("{0}")]
    Io(#[from] std::io::Error),
}

/// Root of Envision's per-machine state.
pub fn app_data_dir(app: &AppHandle) -> Result<PathBuf, PlatformError> {
    app.path()
        .app_data_dir()
        .map_err(|error| PlatformError::AppData(error.to_string()))
}

/// The global registry of libraries plus desktop preferences.
pub fn app_db(app: &AppHandle) -> Result<PathBuf, PlatformError> {
    Ok(app_data_dir(app)?.join("settings.db"))
}

/// Root of every per-library derived directory: the index and thumbnail caches
/// for all registered libraries. Derived state only — never a user's images.
pub fn libraries_dir(app: &AppHandle) -> Result<PathBuf, PlatformError> {
    Ok(app_data_dir(app)?.join("libraries"))
}

/// Root of the derived per-library directory.
pub fn library_dir(app: &AppHandle, library_id: &str) -> Result<PathBuf, PlatformError> {
    Ok(libraries_dir(app)?.join(library_id))
}

/// The rebuildable query index for one library.
pub fn library_index_path(app: &AppHandle, library_id: &str) -> Result<PathBuf, PlatformError> {
    Ok(library_dir(app, library_id)?.join("index.db"))
}

/// Thumbnail cache for one library, keyed by content hash so that duplicate
/// files share a single decode.
pub fn library_thumbnails_dir(app: &AppHandle, library_id: &str) -> Result<PathBuf, PlatformError> {
    Ok(library_dir(app, library_id)?.join("thumbnails"))
}

/// Log directory for one library.
pub fn library_logs_dir(app: &AppHandle, library_id: &str) -> Result<PathBuf, PlatformError> {
    Ok(library_dir(app, library_id)?.join("logs"))
}

/// Create every directory Envision needs for a library, returning the paths it
/// created.
pub fn ensure_library_dirs(
    app: &AppHandle,
    library_id: &str,
) -> Result<(PathBuf, PathBuf), PlatformError> {
    let index = library_index_path(app, library_id)?;
    let thumbnails = library_thumbnails_dir(app, library_id)?;
    library_logs_dir(app, library_id)?;

    std::fs::create_dir_all(&thumbnails)?;
    if let Some(parent) = index.parent() {
        std::fs::create_dir_all(parent)?;
    }

    Ok((index, thumbnails))
}

/// Require an existing directory and return its canonical path for stable identity.
pub fn validate_library_root(root: &std::path::Path) -> Result<PathBuf, PlatformError> {
    let canonical = root.canonicalize().map_err(|error| {
        PlatformError::AppData(format!("{}: {error}", root.display()))
    })?;

    if !canonical.is_dir() {
        return Err(PlatformError::AppData(format!(
            "{} is not a directory",
            canonical.display()
        )));
    }

    Ok(canonical)
}

/// Human-facing library name derived from a folder, used when the user does not
/// supply one.
pub fn default_library_name(root: &std::path::Path) -> String {
    root.file_name()
        .map(|name| name.to_string_lossy().to_string())
        .filter(|name| !name.is_empty())
        .unwrap_or_else(|| "Library".to_string())
}

/// Build a platform-specific Tauri URL without exposing filesystem paths to the webview.
pub fn asset_url(path: &std::path::Path) -> String {
    let encoded = percent_encode(&path.to_string_lossy());
    if cfg!(any(target_os = "windows", target_os = "android")) {
        format!("http://asset.localhost/{encoded}")
    } else {
        format!("asset://localhost/{encoded}")
    }
}

/// Percent-encode UTF-8 bytes outside the URL unreserved set.
fn percent_encode(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    for byte in input.as_bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(*byte as char)
            }
            other => out.push_str(&format!("%{other:02X}")),
        }
    }
    out
}

/// Path prefix the dev server mounts its thumbnail middleware on. Must match
/// `vite.config.ts`.
pub const DEV_THUMB_PREFIX: &str = "/__thumbs";

/// Use Vite same-origin URLs in debug because WebKitGTK blocks `asset://` from localhost; release uses the asset protocol.
pub fn thumbnail_url(thumbnails_dir: &Path, library_id: &str, path: &Path) -> String {
    #[cfg(debug_assertions)]
    if let Ok(relative) = path.strip_prefix(thumbnails_dir) {
        return format!(
            "{DEV_THUMB_PREFIX}/{}/{}",
            library_id,
            relative.to_string_lossy().replace('\\', "/")
        );
    }

    let _ = (thumbnails_dir, library_id);
    asset_url(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn asset_urls_use_the_platform_convention() {
        let url = asset_url(Path::new("/home/me/Pictures/a.jpg"));

        if cfg!(any(target_os = "windows", target_os = "android")) {
            assert!(url.starts_with("http://asset.localhost/"), "{url}");
        } else {
            assert!(url.starts_with("asset://localhost/"), "{url}");
        }
    }

    #[test]
    fn asset_urls_encode_the_whole_path() {
        let url = asset_url(Path::new("/home/me/My Photos/a b.jpg"));

        // Every path separator is escaped, so the only slashes left are the
        // three in the scheme and authority: "asset://localhost/".
        assert_eq!(url.matches('/').count(), 3, "{url}");
        assert!(!url.contains(' '), "{url}");
        assert!(
            url.ends_with("%2Fhome%2Fme%2FMy%20Photos%2Fa%20b.jpg"),
            "{url}"
        );
    }

    #[test]
    fn percent_encoding_leaves_unreserved_characters_alone() {
        assert_eq!(percent_encode("aZ0-_.~"), "aZ0-_.~");
        assert_eq!(percent_encode("/a b"), "%2Fa%20b");
        assert_eq!(percent_encode("é"), "%C3%A9");
        assert_eq!(percent_encode("a+b"), "a%2Bb");
    }

    #[test]
    fn library_names_never_come_back_empty() {
        assert_eq!(default_library_name(Path::new("/")), "Library");
        assert_eq!(default_library_name(Path::new("/photos")), "photos");
    }

    /// Debug builds must address the dev server; release builds the asset
    /// protocol. Both have to stay in step with `vite.config.ts`.
    #[test]
    fn thumbnail_urls_target_the_dev_server_in_debug_builds() {
        let dir = Path::new("/data/libraries/abc123/thumbnails");
        let path = dir.join("v2").join("ab").join("abc123.jpg");

        let url = thumbnail_url(dir, "abc123", &path);

        if cfg!(debug_assertions) {
            assert_eq!(url, "/__thumbs/abc123/v2/ab/abc123.jpg");
        } else {
            assert!(url.starts_with("asset://localhost/") || url.starts_with("http://asset.localhost/"));
        }
    }

    /// A thumbnail that somehow sits outside its library's cache directory
    /// must not produce a dev URL that escapes it.
    #[test]
    fn a_path_outside_the_cache_falls_back_to_the_asset_protocol() {
        let dir = Path::new("/data/libraries/abc123/thumbnails");
        let outside = Path::new("/data/libraries/other/secret.jpg");

        let url = thumbnail_url(dir, "abc123", outside);

        if cfg!(debug_assertions) {
            assert!(
                !url.starts_with(DEV_THUMB_PREFIX),
                "a path outside the cache must not become a dev URL: {url}"
            );
        }
    }
}
