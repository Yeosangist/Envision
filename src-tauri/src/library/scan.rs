
use super::fingerprint::Cancel;
//! Reconcile the derived index with a library directory without modifying its files.
#[cfg(test)]
use super::fingerprint;
use super::queries::compact_positions;
use super::{LibraryError, LibraryIndex, Result};
use crate::domain::to_rel_string;
use crate::images::{self, MediaKind};
use std::collections::HashMap;
use std::time::{Duration, Instant};

#[derive(Debug, Clone, Copy)]
pub struct ScanOptions {
    /// Follow symlinks; disabled by default to avoid cycles and indexing outside the library.
    pub follow_symlinks: bool,
    /// Descend into dot-directories such as `.git` or `.Trash`.
    pub include_hidden: bool,
    pub max_depth: Option<usize>,
}

impl Default for ScanOptions {
    fn default() -> Self {
        Self {
            follow_symlinks: false,
            include_hidden: false,
            max_depth: None,
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct ScanOutcome {
    pub folders: u64,
    pub added: u64,
    pub updated: u64,
    pub removed: u64,
    pub unchanged: u64,
    pub duplicates: u64,
    pub elapsed: Duration,
}

/// A handle for cancelling a running scan.
pub type ScanHandle = Cancel;

pub fn reconcile_index(index: &LibraryIndex, options: ScanOptions, cancel: &super::fingerprint::Cancel) -> Result<ScanOutcome> {
    let started = Instant::now();
    let mut outcome = ScanOutcome::default();

    if cancel.is_cancelled() {
        return Ok(outcome);
    }

    if !index.root.is_dir() {
        return Err(LibraryError::Invalid(format!(
            "library folder is not available: {}",
            index.root.display()
        )));
    }

    let root_folder_id = ensure_folder(index, None, "")?;
    let mut folders: HashMap<String, i64> = HashMap::new();
    folders.insert(String::new(), root_folder_id);

    // Pass 1: discover directories. Parents are created before children so the
    // `parent_id` link is always valid.
    let mut directories: Vec<(String, usize)> = Vec::new();
    let directories_complete = collect_entries(index, options, cancel, |entry| {
        if entry.depth == 0 {
            return;
        }
        if let Some(relative) = entry.relative_directory {
            directories.push((relative, entry.depth));
        }
    });
    if !directories_complete {
        return Ok(outcome);
    }

    directories.sort_by_key(|(path, depth)| (*depth, path.clone()));
    for (relative, _) in &directories {
        if cancel.is_cancelled() {
            return Ok(outcome);
        }
        let (_, parent_rel) = split_relative(relative);
        let parent_id = folders.get(parent_rel.as_str()).copied().unwrap_or(root_folder_id);
        let id = ensure_folder(index, Some(parent_id), relative)?;
        folders.insert(relative.clone(), id);
        outcome.folders += 1;
    }

    // Where a newly discovered image lands in its folder. Seeded up front so the
    // walk closure never has to fall back to an error path.
    let mut next_positions: HashMap<i64, i64> = {
        let mut stmt = index
            .conn
            .prepare("SELECT folder_id, COALESCE(MAX(position), -1) + 1 FROM assets GROUP BY folder_id")?;
        let rows = stmt
            .query_map([], |row| Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?)))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        rows.into_iter().collect()
    };

    // Pass 2: discover images.
    let tx = index.conn.unchecked_transaction()?;
    let images_complete = {
        let mut upsert = tx.prepare(
            "SELECT bytes, mtime, sha256, position, decode_ok, width, height
             FROM assets WHERE rel_path = ?1",
        )?;
        let mut insert = tx.prepare(
            "INSERT INTO assets (folder_id, filename, rel_path, ext, mime, width, height,
                                 bytes, mtime, sha256, decode_ok, position, created_at, indexed_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, NULL, ?10, ?11, ?12, ?12)",
        )?;

        let now = chrono::Utc::now().timestamp();

        collect_entries(index, options, cancel, |entry| {
            let Some(relative) = entry.relative_file else {
                return;
            };
            let metadata = match std::fs::symlink_metadata(&entry.path) {
                Ok(metadata) => metadata,
                Err(_) => return,
            };

            let ext = images::extension_of(&entry.path);
            let Some(kind) = images::classify(&ext) else {
                return;
            };

            let bytes = metadata.len() as i64;
            let mtime = metadata
                .modified()
                .ok()
                .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
                .map(|d| d.as_secs() as i64)
                .unwrap_or(0);

            let parent_rel = relative.rsplit_once('/').map(|(head, _)| head).unwrap_or("");
            let folder_id = folders.get(parent_rel).copied().unwrap_or(root_folder_id);
            let filename = entry
                .path
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_default();

            let existing: Option<(i64, i64, Option<String>, i64, i64, Option<i64>, Option<i64>)> =
                upsert
                    .query_row([&relative], |row| {
                        Ok((
                            row.get(0)?,
                            row.get(1)?,
                            row.get(2)?,
                            row.get(3)?,
                            row.get(4)?,
                            row.get(5)?,
                            row.get(6)?,
                        ))
                    })
                    .ok();

            match existing {
                Some((old_bytes, old_mtime, _sha256, _position, decode_ok, old_width, _old_height)) => {
                    let expected_decodable = (kind == MediaKind::Decodable) as i64;
                    let missing_dimensions = expected_decodable == 1 && old_width.is_none();

                    if old_bytes == bytes
                        && old_mtime == mtime
                        && decode_ok == expected_decodable
                        && !missing_dimensions
                    {
                        outcome.unchanged += 1;
                        return;
                    }

                    let still_decodable = kind == MediaKind::Decodable;
                    let (new_width, new_height) = if still_decodable {
                        match images::dimensions(&entry.path) {
                            Some((w, h)) if w > 0 && h > 0 => (Some(w as i64), Some(h as i64)),
                            _ => (None, None),
                        }
                    } else {
                        (None, None)
                    };

                    let _ = tx.execute(
                        "UPDATE assets
                         SET bytes = ?1, mtime = ?2, sha256 = NULL, hashed_at = NULL,
                             decode_ok = ?3, width = ?4, height = ?5,
                             mime = ?6, indexed_at = ?7
                         WHERE rel_path = ?8",
                        rusqlite::params![
                            bytes,
                            mtime,
                            still_decodable as i64,
                            new_width,
                            new_height,
                            images::mime_for(&ext),
                            now,
                            &relative,
                        ],
                    );
                    outcome.updated += 1;
                }
                None => {
                    let position = *next_positions.entry(folder_id).or_insert(0);
                    next_positions.insert(folder_id, position + 1);

                    let (width, height) = if kind == MediaKind::Decodable {
                        match images::dimensions(&entry.path) {
                            Some((w, h)) if w > 0 && h > 0 => (Some(w as i64), Some(h as i64)),
                            _ => (None, None),
                        }
                    } else {
                        // Formats Envision deliberately does not decode. They
                        // stay indexed and countable, and open externally.
                        (None, None)
                    };

                    insert
                        .execute(rusqlite::params![
                            folder_id,
                            &filename,
                            &relative,
                            &ext,
                            images::mime_for(&ext),
                            width,
                            height,
                            bytes,
                            mtime,
                            (kind == MediaKind::Decodable) as i64,
                            position,
                            now,
                        ])
                        .ok();
                    outcome.added += 1;
                }
            }
        })
    };
    if !images_complete {
        tx.rollback()?;
        return Ok(outcome);
    }
    tx.commit()?;

    if cancel.is_cancelled() {
        return Ok(outcome);
    }

    // Pass 3: drop rows whose files are gone.
    outcome.removed = prune_missing(index)?;

    // Re-densify positions in folders whose membership just changed.
    compact_folder_positions(index)?;

    let (_, redundant) = super::queries::duplicate_summary(index)?;
    outcome.duplicates = redundant as u64;

    outcome.elapsed = started.elapsed();
    Ok(outcome)
}

struct Entry {
    path: std::path::PathBuf,
    depth: usize,
    relative_file: Option<String>,
    relative_directory: Option<String>,
}

fn collect_entries<F>(index: &LibraryIndex, options: ScanOptions, cancel: &Cancel, mut visit: F) -> bool
where
    F: FnMut(Entry),
{
    let walker = walkdir::WalkDir::new(&index.root)
        .follow_links(options.follow_symlinks)
        .max_depth(options.max_depth.unwrap_or(usize::MAX))
        .into_iter()
        .filter_entry(|entry| {
            if !options.include_hidden {
                let name = entry.file_name().to_string_lossy();
                if name.starts_with('.') && entry.depth() > 0 {
                    return false;
                }
            }
            true
        });

    let mut entries: Vec<Entry> = Vec::new();

    for entry in walker.filter_map(|entry| entry.ok()) {
        if cancel.is_cancelled() {
            return false;
        }

        let path = entry.path();
        let depth = entry.depth();
        if depth == 0 {
            continue;
        }

        let relative = match path.strip_prefix(&index.root) {
            Ok(relative) => to_rel_string(relative),
            Err(_) => continue,
        };

        let is_file = entry.file_type().is_file();
        if is_file && !images::is_indexable(path) {
            continue;
        }

        entries.push(Entry {
            path: path.to_path_buf(),
            depth,
            relative_file: is_file.then(|| relative.clone()),
            relative_directory: (!is_file).then_some(relative),
        });
    }

    entries.sort_by(|a, b| a.relative_file.cmp(&b.relative_file));
    for entry in entries {
        if cancel.is_cancelled() {
            return false;
        }
        visit(entry);
    }
    !cancel.is_cancelled()
}

fn split_relative(relative: &str) -> (String, String) {
    match relative.rsplit_once('/') {
        Some((parent, name)) => (name.to_string(), parent.to_string()),
        None => (relative.to_string(), String::new()),
    }
}

fn ensure_folder(
    index: &LibraryIndex,
    parent_id: Option<i64>,
    rel_path: &str,
) -> Result<i64> {
    if let Some(existing) = super::folder_by_rel(index, rel_path)? {
        return Ok(existing.id);
    }

    let name = if rel_path.is_empty() {
        index
            .root
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| "Library".to_string())
    } else {
        rel_path
            .rsplit('/')
            .next()
            .unwrap_or(rel_path)
            .to_string()
    };

    index.conn.execute(
        "INSERT INTO folders (parent_id, name, rel_path, created_at) VALUES (?1, ?2, ?3, ?4)",
        rusqlite::params![parent_id, name, rel_path, chrono::Utc::now().timestamp()],
    )?;

    Ok(index.conn.last_insert_rowid())
}

/// Remove stale index rows after a full root reconcile; never delete files from disk.
fn prune_missing(index: &LibraryIndex) -> Result<u64> {
    let rows: Vec<(i64, String)> = {
        let mut stmt = index.conn.prepare("SELECT id, rel_path FROM assets")?;
        let rows = stmt
            .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        rows
    };

    let mut gone = Vec::new();
    for (id, rel) in rows {
        let exists = match index.asset_path(&rel) {
            Ok(Some(path)) => path.is_file(),
            _ => false,
        };
        if !exists {
            gone.push(id);
        }
    }

    if !gone.is_empty() {
        let tx = index.conn.unchecked_transaction()?;
        {
            let mut delete = tx.prepare("DELETE FROM assets WHERE id = ?1")?;
            for id in &gone {
                delete.execute([id])?;
            }
        }
        tx.commit()?;
    }

    Ok(gone.len() as u64)
}

/// Drop folders that no longer contain anything and no longer exist on disk.
fn prune_missing_folders(index: &LibraryIndex) -> Result<()> {
    let rows: Vec<(i64, String)> = {
        let mut stmt = index.conn.prepare("SELECT id, rel_path FROM folders")?;
        let rows = stmt
            .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        rows
    };

    let mut gone = Vec::new();
    for (id, rel) in rows {
        if rel.is_empty() {
            continue;
        }
        let exists = match crate::paths::resolve_within(&index.root, &rel) {
            Ok(path) => path.is_dir(),
            Err(_) => false,
        };
        if !exists {
            gone.push(id);
        }
    }

    if !gone.is_empty() {
        let tx = index.conn.unchecked_transaction()?;
        {
            let mut delete = tx.prepare("DELETE FROM folders WHERE id = ?1")?;
            for id in &gone {
                delete.execute([id])?;
            }
        }
        tx.commit()?;
    }

    Ok(())
}

fn compact_folder_positions(index: &LibraryIndex) -> Result<()> {
    prune_missing_folders(index)?;

    let folder_ids: Vec<i64> = {
        let mut stmt = index.conn.prepare("SELECT id FROM folders")?;
        let ids = stmt
            .query_map([], |row| row.get(0))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        ids
    };

    for folder_id in folder_ids {
        compact_positions(&index.conn, folder_id)?;
    }
    Ok(())
}

/// Convenience for tests: the number of indexed assets.
#[cfg(test)]
pub(crate) fn asset_count(index: &LibraryIndex) -> Result<i64> {
    Ok(index
        .conn
        .query_row("SELECT COUNT(*) FROM assets", [], |row| row.get(0))?)
}

#[cfg(test)]
pub(crate) mod tests_support {
    use super::*;
    use crate::database::{open_in_memory, LIBRARY_MIGRATIONS};
    use std::path::{Path, PathBuf};

    /// A temporary directory with an in-memory index rooted at it.
    pub(crate) fn temp_index(tag: &str) -> (LibraryIndex, PathBuf) {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!("envision-scan-{tag}-{nanos}"));
        std::fs::create_dir_all(&root).unwrap();

        let index = LibraryIndex {
            id: "test".into(),
            root: root.clone(),
            conn: open_in_memory(LIBRARY_MIGRATIONS).unwrap(),
            thumbnails_dir: PathBuf::from("/tmp/thumbs"),
        };
        (index, root)
    }

    /// Write a tiny but structurally valid 1x1 PNG so dimension probing works.
    pub(crate) fn write_png(path: &Path, tag: &str) {
        const PNG: &[u8] = &[
            0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0x00, 0x00, 0x00, 0x0D, 0x49, 0x48,
            0x44, 0x52, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x06, 0x00, 0x00,
            0x00, 0x1F, 0x15, 0xC4, 0x89, 0x00, 0x00, 0x00, 0x0D, 0x49, 0x44, 0x41, 0x54, 0x08,
            0xD7, 0x63, 0xF8, 0xCF, 0xC0, 0xF0, 0x1F, 0x00, 0x05, 0x00, 0x01, 0xFF, 0x89, 0x99,
            0x3D, 0x1D, 0x00, 0x00, 0x00, 0x00, 0x49, 0x45, 0x4E, 0x44, 0xAE, 0x42, 0x60, 0x82,
        ];
        std::fs::write(path, PNG).map(|_| tag).unwrap();
    }
}

#[cfg(test)]
mod tests {
    use super::tests_support::{temp_index, write_png};
    use super::*;
    use std::path::Path;

    #[test]
    fn entry_collection_stops_when_cancelled_during_visitation() {
        let (index, root) = temp_index("cancel-walk");
        write_png(&root.join("a.png"), "a");
        write_png(&root.join("b.png"), "b");
        let cancel = Cancel::new();
        let mut visited = 0;

        let completed = collect_entries(&index, ScanOptions::default(), &cancel, |_| {
            visited += 1;
            cancel.cancel();
        });

        assert!(!completed);
        assert_eq!(visited, 1);
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn scans_a_flat_directory() {
        let (index, root) = temp_index("flat");
        write_png(&root.join("a.png"), "a");
        write_png(&root.join("b.png"), "b");

        let outcome = reconcile_index(&index, ScanOptions::default(), &super::fingerprint::Cancel::new()).unwrap();
        assert_eq!(outcome.added, 2);
        assert_eq!(outcome.removed, 0);
        assert_eq!(asset_count(&index).unwrap(), 2);

        let assets = super::super::queries::list_assets(&index, 1).unwrap();
        assert_eq!(assets.len(), 2);
        assert!(assets.iter().all(|a| a.decode_ok));
        assert_eq!(assets[0].width, Some(1));

        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn builds_a_nested_folder_tree_with_counts() {
        let (index, root) = temp_index("nested");
        write_png(&root.join("top.png"), "top");
        std::fs::create_dir_all(root.join("album/2024")).unwrap();
        write_png(&root.join("album/a.png"), "a");
        write_png(&root.join("album/2024/b.png"), "b");

        reconcile_index(&index, ScanOptions::default(), &super::fingerprint::Cancel::new()).unwrap();

        let folders = super::super::list_all_folders(&index).unwrap();
        assert_eq!(folders.len(), 3, "root plus album plus album/2024");

        let album = super::super::folder_by_rel(&index, "album").unwrap().unwrap();
        assert_eq!(album.image_count, 1);
        let deep = super::super::folder_by_rel(&index, "album/2024").unwrap().unwrap();
        assert_eq!(deep.image_count, 1);
        assert_eq!(super::super::root_folder(&index).unwrap().image_count, 1);

        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn ignores_non_images_and_hidden_directories() {
        let (index, root) = temp_index("ignore");
        write_png(&root.join("keep.png"), "keep");
        std::fs::write(root.join("notes.txt"), b"hello").unwrap();
        std::fs::write(root.join("movie.mp4"), b"video").unwrap();
        std::fs::create_dir_all(root.join(".hidden")).unwrap();
        write_png(&root.join(".hidden/secret.png"), "secret");
        write_png(&root.join(".dotfile.png"), "dot");

        let outcome = reconcile_index(&index, ScanOptions::default(), &super::fingerprint::Cancel::new()).unwrap();
        assert_eq!(outcome.added, 1, "only keep.png should be indexed");

        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn indexes_heic_but_marks_it_undecodable() {
        let (index, root) = temp_index("heic");
        std::fs::write(root.join("photo.heic"), b"ftypheic").unwrap();
        std::fs::write(root.join("modern.avif"), b"ftypavif").unwrap();

        reconcile_index(&index, ScanOptions::default(), &super::fingerprint::Cancel::new()).unwrap();

        let assets = super::super::queries::list_assets(&index, 1).unwrap();
        assert_eq!(assets.len(), 2, "external formats stay visible");
        assert!(assets.iter().all(|a| !a.decode_ok));
        assert!(assets.iter().all(|a| a.width.is_none()));

        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn indexes_a_mislabelled_jpeg_with_real_dimensions() {
        // A JPEG on disk named `.png`. The extension is a claim about the file,
        // not a fact about it, and dimensions must come from the bytes.
        let (index, root) = temp_index("mislabelled");
        let path = root.join("liar.png");
        image::DynamicImage::ImageRgb8(
            image::RgbImage::from_raw(300, 150, vec![11u8; 300 * 150 * 3]).unwrap(),
        )
        .save_with_format(&path, image::ImageFormat::Jpeg)
        .unwrap();

        reconcile_index(&index, ScanOptions::default(), &super::fingerprint::Cancel::new()).unwrap();

        let assets = super::super::queries::list_assets(&index, 1).unwrap();
        assert_eq!(assets.len(), 1);
        assert!(assets[0].decode_ok);
        assert_eq!(
            (assets[0].width, assets[0].height),
            (Some(300), Some(150)),
            "dimensions must come from the real pixels, not the extension"
        );

        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn a_rescan_repairs_dimensions_that_were_never_read() {
        let (index, root) = temp_index("repair");
        write_png(&root.join("a.png"), "a");
        reconcile_index(&index, ScanOptions::default(), &super::fingerprint::Cancel::new()).unwrap();

        index
            .conn
            .execute("UPDATE assets SET width = NULL, height = NULL", [])
            .unwrap();
        let stranded = super::super::queries::list_assets(&index, 1).unwrap();
        assert_eq!(stranded[0].width, None, "precondition: dimensions are missing");

        reconcile_index(&index, ScanOptions::default(), &super::fingerprint::Cancel::new()).unwrap();

        let repaired = super::super::queries::list_assets(&index, 1).unwrap();
        assert_eq!(
            (repaired[0].width, repaired[0].height),
            (Some(1), Some(1)),
            "a rescan must re-read the header of a decodable asset with no dimensions"
        );

        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn rescanning_is_idempotent_and_preserves_order() {
        let (index, root) = temp_index("idempotent");
        for name in ["a.png", "b.png", "c.png"] {
            write_png(&root.join(name), name);
        }

        reconcile_index(&index, ScanOptions::default(), &super::fingerprint::Cancel::new()).unwrap();

        let ids: Vec<i64> = super::super::queries::list_assets(&index, 1)
            .unwrap()
            .iter()
            .map(|a| a.id)
            .collect();
        let flipped = vec![ids[2], ids[0], ids[1]];
        super::super::queries::reorder_assets(&index, 1, &flipped).unwrap();

        let outcome = reconcile_index(&index, ScanOptions::default(), &super::fingerprint::Cancel::new()).unwrap();
        assert_eq!(outcome.added, 0);
        assert_eq!(outcome.removed, 0);
        assert_eq!(outcome.unchanged, 3, "nothing should have changed");

        let order: Vec<String> = super::super::queries::list_assets(&index, 1)
            .unwrap()
            .iter()
            .map(|a| a.filename.clone())
            .collect();
        assert_eq!(order, vec!["c.png", "a.png", "b.png"]);

        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn removing_a_file_removes_its_row_on_rescan() {
        let (index, root) = temp_index("removed");
        write_png(&root.join("a.png"), "a");
        write_png(&root.join("b.png"), "b");
        reconcile_index(&index, ScanOptions::default(), &super::fingerprint::Cancel::new()).unwrap();

        std::fs::remove_file(root.join("b.png")).unwrap();

        let outcome = reconcile_index(&index, ScanOptions::default(), &super::fingerprint::Cancel::new()).unwrap();
        assert_eq!(outcome.removed, 1);
        assert_eq!(asset_count(&index).unwrap(), 1);

        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn changing_a_file_clears_its_fingerprint_but_keeps_its_position() {
        let (index, root) = temp_index("touched");
        write_png(&root.join("a.png"), "a");
        write_png(&root.join("b.png"), "b");
        reconcile_index(&index, ScanOptions::default(), &super::fingerprint::Cancel::new()).unwrap();

        index
            .conn
            .execute("UPDATE assets SET sha256 = 'deadbeef'", [])
            .unwrap();

        // Make the file observably different and ensure a different mtime.
        std::fs::write(root.join("b.png"), vec![0xAB; 2048]).unwrap();
        let later = std::time::SystemTime::now() + std::time::Duration::from_secs(5);
        set_mtime(&root.join("b.png"), later);

        let outcome = reconcile_index(&index, ScanOptions::default(), &super::fingerprint::Cancel::new()).unwrap();
        assert_eq!(outcome.updated, 1);

        let assets = super::super::queries::list_assets(&index, 1).unwrap();
        let b = assets.iter().find(|a| a.filename == "b.png").unwrap();
        assert_eq!(b.position, 1, "a touched file must not jump in the grid");
        let a = assets.iter().find(|a| a.filename == "a.png").unwrap();
        assert!(a.sha256.is_some(), "an untouched file keeps its fingerprint");
        assert_eq!(assets.iter().filter(|a| a.sha256.is_some()).count(), 1);

        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn a_deleted_folder_disappears_from_the_tree() {
        let (index, root) = temp_index("folder-gone");
        std::fs::create_dir_all(root.join("album")).unwrap();
        write_png(&root.join("album/a.png"), "a");
        reconcile_index(&index, ScanOptions::default(), &super::fingerprint::Cancel::new()).unwrap();
        assert_eq!(super::super::list_all_folders(&index).unwrap().len(), 2);

        std::fs::remove_dir_all(root.join("album")).unwrap();
        reconcile_index(&index, ScanOptions::default(), &super::fingerprint::Cancel::new()).unwrap();

        assert_eq!(super::super::list_all_folders(&index).unwrap().len(), 1);
        assert_eq!(asset_count(&index).unwrap(), 0);

        std::fs::remove_dir_all(&root).ok();
    }

    #[cfg(unix)]
    fn set_mtime(path: &Path, time: std::time::SystemTime) {
        let file = std::fs::OpenOptions::new().write(true).open(path).unwrap();
        file.set_modified(time).unwrap();
    }

    #[cfg(not(unix))]
    fn set_mtime(_path: &Path, _time: std::time::SystemTime) {}
}
