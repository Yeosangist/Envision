
use std::path::{Path, PathBuf};
use thiserror::Error;

#[derive(Error, Debug)]
pub enum PathError {
    #[error("Path is not allowed: {0}")]
    NotAllowed(String),
    #[error("Path escapes the library root: {0}")]
    Escapes(String),
    #[error("Path does not exist: {0}")]
    NotFound(String),
}

pub fn resolve_within(root: &Path, rel: &str) -> Result<PathBuf, PathError> {
    if rel.starts_with('/') || rel.starts_with('\\') {
        return Err(PathError::NotAllowed(rel.to_string()));
    }
    let bytes = rel.as_bytes();
    if bytes.len() >= 2 && bytes[1] == b':' && bytes[0].is_ascii_alphabetic() {
        return Err(PathError::NotAllowed(rel.to_string()));
    }

    let mut resolved = root.to_path_buf();

    for raw in rel.split(['/', '\\']) {
        if raw.is_empty() {
            continue;
        }
        if raw == "."
            || raw == ".."
            || raw.contains('\0')
            || raw.contains('/')
            || raw.contains('\\')
            || raw.ends_with(' ')   
            || raw.ends_with('.')
        {
            return Err(PathError::NotAllowed(raw.to_string()));
        }
        resolved.push(raw);
    }

    Ok(resolved)
}

pub fn resolve_existing_within(root: &Path, rel: &str) -> Result<PathBuf, PathError> {
    let candidate = resolve_within(root, rel)?;

    let canonical_root = root
        .canonicalize()
        .map_err(|_| PathError::NotFound(root.display().to_string()))?;
    let canonical = candidate
        .canonicalize()
        .map_err(|_| PathError::NotFound(candidate.display().to_string()))?;

    if !canonical.starts_with(&canonical_root) {
        return Err(PathError::Escapes(rel.to_string()));
    }

    Ok(canonical)
}


pub fn unique_destination(dir: &Path, filename: &str) -> Option<PathBuf> {
    let candidate = dir.join(filename);
    if !candidate.exists() {
        return Some(candidate);
    }

    let path = Path::new(filename);
    let stem = path
        .file_stem()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| "image".to_string());
    let ext = path
        .extension()
        .map(|s| format!(".{}", s.to_string_lossy()))
        .unwrap_or_default();

    for counter in 2..10_000 {
        let candidate = dir.join(format!("{stem} ({counter}){ext}"));
        if !candidate.exists() {
            return Some(candidate);
        }
    }

    None
}

pub fn disambiguate(dir: &Path, source: &Path, filename: &str) -> Option<(PathBuf, bool)> {
    let target = dir.join(filename);
    if target == source || !target.exists() {
        return Some((target, false));
    }
    let unique = unique_destination(dir, filename)?;
    Some((unique, true))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn temp_dir(tag: &str) -> PathBuf {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("envision-{tag}-{nanos}"));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn rejects_parent_traversal_in_every_spelling() {
        let root = Path::new("/library");
        for rel in ["../secret.jpg", "a/../../secret.jpg", "..\\secret.jpg", "a/..", ".."] {
            assert!(
                resolve_within(root, rel).is_err(),
                "expected {rel:?} to be rejected"
            );
        }
    }

    #[test]
    fn rejects_absolute_paths() {
        let root = Path::new("/library");
        assert!(resolve_within(root, "/etc/passwd").is_err());
        assert!(resolve_within(root, "//etc/passwd").is_err());
        assert!(resolve_within(root, "\\\\server\\share\\x.jpg").is_err());
        assert!(resolve_within(root, "C:\\Windows\\x.jpg").is_err());
    }

    #[test]
    fn accepts_plain_nested_names() {
        let root = Path::new("/library");
        let resolved = resolve_within(root, "albums/2024/trip.jpg").unwrap();
        assert_eq!(resolved, Path::new("/library/albums/2024/trip.jpg"));
    }

    #[test]
    fn ignores_empty_segments_from_trailing_separators() {
        let root = Path::new("/library");
        let resolved = resolve_within(root, "albums//trip.jpg/").unwrap();
        assert_eq!(resolved, Path::new("/library/albums/trip.jpg"));
    }

    #[test]
    fn resolve_existing_rejects_symlink_escape() {
        let root = temp_dir("paths-symlink");
        let outside = temp_dir("paths-outside");
        let secret = outside.join("secret.jpg");
        fs::write(&secret, b"secret").unwrap();

        #[cfg(unix)]
        {
            let link = root.join("escape");
            std::os::unix::fs::symlink(&outside, &link).unwrap();

            assert!(resolve_existing_within(&root, "escape/secret.jpg").is_err());

            let ok = root.join("inside.jpg");
            fs::write(&ok, b"ok").unwrap();
            assert!(resolve_existing_within(&root, "inside.jpg").is_ok());
        }

        let _ = secret;
        fs::remove_dir_all(&root).ok();
        fs::remove_dir_all(&outside).ok();
    }

    #[test]
    fn unique_destination_appends_counter() {
        let dir = temp_dir("paths-unique");
        fs::write(dir.join("trip.jpg"), b"a").unwrap();

        let first = unique_destination(&dir, "trip.jpg").unwrap();
        assert_eq!(first.file_name().unwrap(), "trip (2).jpg");

        fs::write(dir.join("trip (2).jpg"), b"b").unwrap();
        let second = unique_destination(&dir, "trip.jpg").unwrap();
        assert_eq!(second.file_name().unwrap(), "trip (3).jpg");

        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn unique_destination_handles_extensionless_names() {
        let dir = temp_dir("paths-noext");
        fs::write(dir.join("README"), b"a").unwrap();

        let next = unique_destination(&dir, "README").unwrap();
        assert_eq!(next.file_name().unwrap(), "README (2)");

        fs::remove_dir_all(&dir).ok();
    }
}