
//! Explicit filesystem changes only; paths are revalidated under the library root.
use super::queries::{compact_positions, next_position};
use super::{LibraryError, LibraryIndex, Result};
use crate::domain::MoveResult;
use crate::paths;
use std::io::Write;
use std::path::Path;

/// Move files on disk and update their index rows, adding a numeric suffix for destination collisions.
pub fn move_assets(
    index: &LibraryIndex,
    asset_ids: &[i64],
    to_folder_id: i64,
) -> Result<Vec<MoveResult>> {
    if asset_ids.is_empty() {
        return Ok(Vec::new());
    }

    let target = super::folder_by_id(index, to_folder_id)?;
    if target.rel_path.is_empty()
        && target.parent_id.is_none()
        && to_folder_id != super::root_folder(index)?.id
    {
        return Err(LibraryError::FolderNotFound(to_folder_id));
    }
    let target_dir = paths::resolve_within(&index.root, &target.rel_path)?;
    if !target_dir.is_dir() {
        return Err(LibraryError::Invalid(format!(
            "destination folder is not available: {}",
            target.rel_path
        )));
    }

    let mut results = Vec::with_capacity(asset_ids.len());
    let mut moved_any = false;

    for asset_id in asset_ids {
        let asset = super::queries::asset_by_id(index, *asset_id)?;
        if asset.folder_id == to_folder_id {
            continue;
        }

        let Some(source) = index.asset_path(&asset.rel_path)? else {
            return Err(LibraryError::Invalid(format!(
                "{} does not resolve to a readable path",
                asset.rel_path
            )));
        };
        if !source.is_file() {
            return Err(LibraryError::Invalid(format!(
                "{} is no longer on disk",
                asset.rel_path
            )));
        }

        let (destination, renamed) = paths::disambiguate(&target_dir, &source, &asset.filename)
            .ok_or_else(|| {
                LibraryError::Invalid(format!(
                    "could not find a free name for {} in {}",
                    asset.filename, target.rel_path
                ))
            })?;

        relocate(&source, &destination)?;

        let new_rel = if target.rel_path.is_empty() {
            destination
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_default()
        } else {
            format!(
                "{}/{}",
                target.rel_path,
                destination
                    .file_name()
                    .map(|n| n.to_string_lossy().to_string())
                    .unwrap_or_default()
            )
        };

        let position = next_position(&index.conn, to_folder_id)?;
        index.conn.execute(
            "UPDATE assets
             SET folder_id = ?1, rel_path = ?2, filename = ?3, position = ?4, indexed_at = ?5
             WHERE id = ?6",
            rusqlite::params![
                to_folder_id,
                &new_rel,
                destination
                    .file_name()
                    .map(|n| n.to_string_lossy().to_string())
                    .unwrap_or_default(),
                position,
                chrono::Utc::now().timestamp(),
                *asset_id,
            ],
        )?;

        results.push(MoveResult {
            asset_id: *asset_id,
            from_folder_id: asset.folder_id,
            to_folder_id,
            filename: destination
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_default(),
            rel_path: new_rel,
            renamed,
        });
        moved_any = true;
    }

    if moved_any {
        compact_positions(&index.conn, to_folder_id)?;
    }

    Ok(results)
}

/// Rename a file, falling back to copy-and-delete across filesystem boundaries.
fn relocate(source: &Path, destination: &Path) -> Result<()> {
    match std::fs::rename(source, destination) {
        Ok(()) => Ok(()),
        Err(error) if is_cross_device(&error) => copy_across_devices(source, destination),
        Err(error) => Err(LibraryError::Io(error)),
    }
}

fn is_cross_device(error: &std::io::Error) -> bool {
    error.raw_os_error() == Some(18) // EXDEV on Linux
        || error.raw_os_error() == Some(17) // EEXIST surfaces this way on some Windows setups
        || error.to_string().to_ascii_lowercase().contains("cross-device")
        || error.to_string().to_ascii_lowercase().contains("different disk")
}

/// Copy to a destination-side temporary file and sync it before removing the source.
fn copy_across_devices(source: &Path, destination: &Path) -> Result<()> {
    let temp = destination.with_extension(format!(
        "{}-envision-moving",
        destination
            .extension()
            .map(|e| e.to_string_lossy().to_string())
            .unwrap_or_default()
    ));

    let outcome = (|| -> std::io::Result<()> {
        let mut reader = std::fs::File::open(source)?;
        let mut writer = std::fs::File::create(&temp)?;
        std::io::copy(&mut reader, &mut writer)?;
        writer.flush()?;
        writer.sync_all()?;
        drop(writer);
        std::fs::rename(&temp, destination)
    })();

    if let Err(error) = outcome {
        let _ = std::fs::remove_file(&temp);
        return Err(LibraryError::Io(error));
    }

    std::fs::remove_file(source)?;
    Ok(())
}

/// Create a real directory beneath the library root and a folder row for it.
pub fn create_folder(index: &LibraryIndex, parent_id: i64, name: &str) -> Result<i64> {
    let name = sanitize_folder_name(name)?;
    let parent = super::folder_by_id(index, parent_id)?;
    let rel = if parent.rel_path.is_empty() {
        name.clone()
    } else {
        format!("{}/{name}", parent.rel_path)
    };

    if paths::resolve_within(&index.root, &rel).is_err() {
        return Err(LibraryError::Invalid("invalid folder name".into()));
    }
    if super::folder_by_rel(index, &rel)?.is_some() {
        return Err(LibraryError::Invalid(format!("“{name}” already exists")));
    }

    let dir = paths::resolve_within(&index.root, &rel)?;
    std::fs::create_dir_all(&dir)?;

    index.conn.execute(
        "INSERT INTO folders (parent_id, name, rel_path, created_at) VALUES (?1, ?2, ?3, ?4)",
        rusqlite::params![parent_id, name, rel, chrono::Utc::now().timestamp()],
    )?;

    Ok(index.conn.last_insert_rowid())
}

/// Rename a real directory and re-point every row beneath it.
pub fn rename_folder(index: &LibraryIndex, folder_id: i64, name: &str) -> Result<()> {
    let name = sanitize_folder_name(name)?;
    let folder = super::folder_by_id(index, folder_id)?;
    if folder.rel_path.is_empty() {
        return Err(LibraryError::Invalid(
            "the library root cannot be renamed".into(),
        ));
    }
    if folder.name == name {
        return Ok(());
    }

    let (_, old_parent_rel) = split_parent(&folder.rel_path);
    let new_rel = if old_parent_rel.is_empty() {
        name.clone()
    } else {
        format!("{old_parent_rel}/{name}")
    };

    if super::folder_by_rel(index, &new_rel)?.is_some() {
        return Err(LibraryError::Invalid(format!("“{name}” already exists")));
    }

    let prefix = format!("{}/", folder.rel_path);

    let old_dir = paths::resolve_within(&index.root, &folder.rel_path)?;
    let new_dir = paths::resolve_within(&index.root, &new_rel)?;
    std::fs::create_dir_all(new_dir.parent().unwrap_or(&index.root))?;
    std::fs::rename(&old_dir, &new_dir)?;

    let suffix_from = prefix.len() + 1;
    let like_pattern = format!("{}%", escaped_prefix(&prefix));

    let tx = index.conn.unchecked_transaction()?;
    {
        tx.execute(
            "UPDATE folders SET name = ?1, rel_path = ?2 WHERE id = ?3",
            rusqlite::params![&name, &new_rel, folder_id],
        )?;

        let mut descendants: Vec<(i64, String)> = {
            let mut stmt = tx.prepare(
                "SELECT id, rel_path FROM folders
                 WHERE id <> ?1 AND rel_path LIKE ?2 ESCAPE '\\'",
            )?;
            let rows = stmt
                .query_map(rusqlite::params![folder_id, &like_pattern], |row| {
                    Ok((row.get(0)?, row.get(1)?))
                })?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            rows
        };
        descendants.sort_by_key(|(_, rel)| rel.len());

        let mut rename_folder = tx.prepare(
            "UPDATE folders SET rel_path = ?1 || '/' || substr(rel_path, ?2), name = ?3
             WHERE id = ?4",
        )?;
        for (id, rel) in &descendants {
            let suffix = rel.get(suffix_from - 1..).unwrap_or_default();
            let segment = suffix.split('/').next().unwrap_or(suffix).to_string();
            rename_folder.execute(rusqlite::params![&new_rel, suffix_from as i64, segment, id])?;
        }

        let mut rename_asset = tx.prepare(
            "UPDATE assets SET rel_path = ?1 || '/' || substr(rel_path, ?2)
             WHERE rel_path LIKE ?3 ESCAPE '\\'",
        )?;
        rename_asset.execute(rusqlite::params![
            &new_rel,
            suffix_from as i64,
            &like_pattern
        ])?;
    }
    tx.commit()?;

    Ok(())
}

/// Reject names that would produce an unusable or unsafe directory.
fn sanitize_folder_name(name: &str) -> Result<String> {
    let trimmed = name.trim();
    if trimmed.is_empty() {
        return Err(LibraryError::Invalid("a folder needs a name".into()));
    }
    if trimmed == "." || trimmed == ".." {
        return Err(LibraryError::Invalid(
            "that is not a usable folder name".into(),
        ));
    }
    if trimmed.contains('/') || trimmed.contains('\\') || trimmed.contains('\0') {
        return Err(LibraryError::Invalid(
            "a folder name cannot contain slashes".into(),
        ));
    }
    if trimmed.chars().count() > 200 {
        return Err(LibraryError::Invalid("that folder name is too long".into()));
    }
    Ok(trimmed.to_string())
}

fn split_parent(rel: &str) -> (&str, String) {
    match rel.rsplit_once('/') {
        Some((parent, _)) => (rel, parent.to_string()),
        None => (rel, String::new()),
    }
}

fn escaped_prefix(prefix: &str) -> String {
    prefix
        .replace('\\', "\\\\")
        .replace('%', "\\%")
        .replace('_', "\\_")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::library::scan::tests_support::{temp_index, write_png};
    use crate::library::{folder_by_rel, queries::list_assets};

    #[test]
    fn moving_an_image_updates_both_the_disk_and_the_index() {
        let (index, root) = temp_index("move");
        write_png(&root.join("a.png"), "a");
        std::fs::create_dir_all(root.join("album")).unwrap();
        reconcile(&index);
        let album = folder_by_rel(&index, "album").unwrap().unwrap();

        let asset = list_assets(&index, 1).unwrap().remove(0);
        let results = move_assets(&index, &[asset.id], album.id).unwrap();

        assert_eq!(results.len(), 1);
        assert!(!results[0].renamed);
        assert_eq!(results[0].rel_path, "album/a.png");

        assert!(root.join("album/a.png").is_file(), "file moved on disk");
        assert!(!root.join("a.png").exists(), "source no longer exists");

        let moved = list_assets(&index, album.id).unwrap();
        assert_eq!(moved.len(), 1);
        assert_eq!(moved[0].rel_path, "album/a.png");
        assert_eq!(moved[0].filename, "a.png");
        assert!(list_assets(&index, 1).unwrap().is_empty());

        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn a_name_collision_is_suffixed_rather_than_overwritten() {
        let (index, root) = temp_index("collide");
        write_png(&root.join("a.png"), "first");
        write_png(&root.join("b.png"), "other");
        std::fs::create_dir_all(root.join("album")).unwrap();
        write_png(&root.join("album/a.png"), "existing");
        reconcile(&index);
        let album = folder_by_rel(&index, "album").unwrap().unwrap();

        let top = list_assets(&index, 1).unwrap();
        let source = top.iter().find(|a| a.filename == "a.png").unwrap();

        let before = std::fs::read(root.join("album/a.png")).unwrap();
        let results = move_assets(&index, &[source.id], album.id).unwrap();

        assert!(results[0].renamed);
        assert_eq!(results[0].filename, "a (2).png");
        assert_eq!(
            std::fs::read(root.join("album/a.png")).unwrap(),
            before,
            "the existing file must be untouched"
        );
        assert!(root.join("album/a (2).png").is_file());

        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn moving_into_the_folder_it_already_in_is_a_no_op() {
        let (index, root) = temp_index("noop");
        write_png(&root.join("a.png"), "a");
        reconcile(&index);
        let asset = list_assets(&index, 1).unwrap().remove(0);

        assert!(move_assets(&index, &[asset.id], 1).unwrap().is_empty());
        assert!(root.join("a.png").is_file());

        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn moving_several_images_keeps_them_all() {
        let (index, root) = temp_index("bulk");
        for name in ["a.png", "b.png", "c.png"] {
            write_png(&root.join(name), name);
        }
        std::fs::create_dir_all(root.join("album")).unwrap();
        reconcile(&index);
        let album = folder_by_rel(&index, "album").unwrap().unwrap();
        let ids: Vec<i64> = list_assets(&index, 1)
            .unwrap()
            .iter()
            .map(|a| a.id)
            .collect();

        let results = move_assets(&index, &ids, album.id).unwrap();
        assert_eq!(results.len(), 3);

        let moved = list_assets(&index, album.id).unwrap();
        assert_eq!(moved.len(), 3);
        let positions: Vec<i64> = moved.iter().map(|a| a.position).collect();
        assert_eq!(positions, vec![0, 1, 2], "arrivals get dense positions");

        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn creating_a_folder_makes_a_real_directory() {
        let (index, root) = temp_index("mkdir");
        reconcile(&index);
        let root_id = crate::library::root_folder(&index).unwrap().id;

        let id = create_folder(&index, root_id, "Trip 2024").unwrap();

        assert!(root.join("Trip 2024").is_dir());
        let folder = crate::library::folder_by_id(&index, id).unwrap();
        assert_eq!(folder.name, "Trip 2024");
        assert_eq!(folder.rel_path, "Trip 2024");

        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn creating_the_same_folder_twice_is_refused() {
        let (index, root) = temp_index("mkdir-dup");
        reconcile(&index);
        let root_id = crate::library::root_folder(&index).unwrap().id;
        create_folder(&index, root_id, "Trip").unwrap();

        assert!(create_folder(&index, root_id, "Trip").is_err());

        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn folder_names_that_could_escape_the_root_are_refused() {
        let (index, root) = temp_index("mkdir-evil");
        reconcile(&index);
        let root_id = crate::library::root_folder(&index).unwrap().id;

        for name in ["../escape", "..", ".", "a/b", "a\\b", "", "   "] {
            assert!(
                create_folder(&index, root_id, name).is_err(),
                "expected {name:?} to be refused"
            );
        }
        assert!(!root.parent().unwrap().join("escape").exists());

        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn renaming_a_folder_rewrites_every_descendant_path() {
        let (index, root) = temp_index("rename");
        std::fs::create_dir_all(root.join("old/inner")).unwrap();
        write_png(&root.join("old/a.png"), "a");
        write_png(&root.join("old/inner/b.png"), "b");
        reconcile(&index);

        let old = folder_by_rel(&index, "old").unwrap().unwrap();
        rename_folder(&index, old.id, "new").unwrap();

        assert!(root.join("new").is_dir());
        assert!(root.join("new/a.png").is_file());
        assert!(root.join("new/inner/b.png").is_file());
        assert!(!root.join("old").exists());

        let all = crate::library::list_all_folders(&index).unwrap();
        let paths: Vec<String> = all.iter().map(|f| f.rel_path.clone()).collect();
        assert!(paths.contains(&"new".to_string()));
        assert!(paths.contains(&"new/inner".to_string()));
        assert!(!paths.contains(&"old".to_string()));
        assert!(folder_by_rel(&index, "old/inner").unwrap().is_none());

        let assets = list_assets(&index, 1).unwrap();
        assert_eq!(assets.iter().filter(|a| a.sha256.is_none()).count(), 0);

        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn renaming_the_library_root_is_refused() {
        let (index, root) = temp_index("rename-root");
        reconcile(&index);
        let root_id = crate::library::root_folder(&index).unwrap().id;

        assert!(rename_folder(&index, root_id, "elsewhere").is_err());

        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn a_png_with_underscores_does_not_corrupt_a_like_prefixed_match() {
        let (index, root) = temp_index("prefix");
        std::fs::create_dir_all(root.join("a_b")).unwrap();
        write_png(&root.join("a_b/inside.png"), "inside");
        std::fs::create_dir_all(root.join("aXb")).unwrap();
        write_png(&root.join("aXb/other.png"), "other");
        reconcile(&index);

        let first = folder_by_rel(&index, "a_b").unwrap().unwrap();
        rename_folder(&index, first.id, "renamed").unwrap();

        // "aXb" must not have been swept up by a LIKE on "a_b/".
        assert!(folder_by_rel(&index, "aXb").unwrap().is_some());
        assert!(root.join("aXb/other.png").is_file());

        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn moving_a_missing_file_is_refused_rather_than_silently_dropped() {
        let (index, root) = temp_index("gone");
        write_png(&root.join("a.png"), "a");
        std::fs::create_dir_all(root.join("album")).unwrap();
        reconcile(&index);
        let album = folder_by_rel(&index, "album").unwrap().unwrap();
        let asset = list_assets(&index, 1).unwrap().remove(0);

        std::fs::remove_file(root.join("a.png")).unwrap();

        assert!(move_assets(&index, &[asset.id], album.id).is_err());

        std::fs::remove_dir_all(&root).ok();
    }

    fn reconcile(index: &LibraryIndex) {
        crate::library::scan::reconcile_index(
            index,
            Default::default(),
            &crate::library::fingerprint::Cancel::new(),
        )
        .unwrap();
    }
}
