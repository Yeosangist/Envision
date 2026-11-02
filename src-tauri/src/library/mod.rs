
//! Library registry and the open index handle; image collections are indexed in place.
pub mod fingerprint;
pub mod moves;
pub mod queries;
pub mod scan;
pub mod watcher;

use crate::database::{self, DatabaseError, LIBRARY_MIGRATIONS};
use crate::domain::{self, Folder, Library};
use crate::paths::PathError;
use crate::platform::PlatformError;
use rusqlite::{Connection, OptionalExtension};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use tauri::AppHandle;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum LibraryError {
    #[error("Database error: {0}")]
    Database(#[from] DatabaseError),
    #[error("Sqlite error: {0}")]
    Sqlite(#[from] rusqlite::Error),
    #[error("{0}")]
    Platform(#[from] PlatformError),
    #[error("Thumbnail error: {0}")]
    Thumbnail(#[from] crate::thumbnails::ThumbnailError),
    #[error("No library is open")]
    NoOpenLibrary,
    #[error("Library not found: {0}")]
    NotFound(String),
    #[error("Folder not found: {0}")]
    FolderNotFound(i64),
    #[error("Asset not found: {0}")]
    AssetNotFound(i64),
    #[error("Path error: {0}")]
    Path(#[from] PathError),
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
    #[error("{0}")]
    Invalid(String),
}

pub type Result<T> = std::result::Result<T, LibraryError>;

/// The open library: its resolved root, its derived index, and its thumbnail
/// cache. Dropping this closes the SQLite handle.
pub struct LibraryIndex {
    pub id: String,
    pub root: PathBuf,
    pub conn: Connection,
    pub thumbnails_dir: PathBuf,
}

impl LibraryIndex {
    /// Open (creating if absent) the derived index for a library.
    pub fn open(app: &AppHandle, id: &str, root: PathBuf) -> Result<Self> {
        let (index_path, thumbnails_dir) = crate::platform::ensure_library_dirs(app, id)?;
        let conn = database::open(&index_path, LIBRARY_MIGRATIONS)?;

        Ok(Self {
            id: id.to_string(),
            root,
            conn,
            thumbnails_dir,
        })
    }

    /// Create a handle that can be used from another thread.
    /// This opens a new connection to the same database.
    pub fn clone_handle(&self, app: &AppHandle) -> Result<Self> {
        let index_path = crate::platform::library_index_path(app, &self.id)?;
        let conn = database::open(&index_path, LIBRARY_MIGRATIONS)?;
        Ok(Self {
            id: self.id.clone(),
            root: self.root.clone(),
            conn,
            thumbnails_dir: self.thumbnails_dir.clone(),
        })
    }

    /// Absolute path of an indexed asset, re-derived from the library root.
    /// Returns `None` if the row is gone or its stored path is not safe.
    pub fn asset_path(&self, rel_path: &str) -> Result<Option<PathBuf>> {
        match crate::paths::resolve_within(&self.root, rel_path) {
            Ok(path) => Ok(Some(path)),
            Err(PathError::NotFound(_))
            | Err(PathError::Escapes(_))
            | Err(PathError::NotAllowed(_)) => Ok(None),
        }
    }

    /// Resolve an existing asset only if it remains under the canonical root after symlink resolution.
    pub fn asset_real_path(&self, rel_path: &str) -> Result<Option<PathBuf>> {
        match crate::paths::resolve_existing_within(&self.root, rel_path) {
            Ok(path) => Ok(Some(path)),
            Err(_) => Ok(None),
        }
    }
}


const SELECT_LIBRARY: &str =
    "SELECT id, name, root_path, created_at, last_opened_at FROM libraries";

fn library_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<Library> {
    Ok(Library {
        id: row.get(0)?,
        name: row.get(1)?,
        root_path: row.get(2)?,
        created_at: row.get(3)?,
        last_opened_at: row.get(4)?,
        missing: false,
    })
}

/// Register a canonical root or reuse its existing registration and index.
pub fn add_library(conn: &Connection, name: Option<&str>, root: &Path) -> Result<Library> {
    let root = crate::platform::validate_library_root(root)?;
    let root_string = root.to_string_lossy().to_string();
    let now = chrono::Utc::now().timestamp();
    let id = domain::library_id_for(&root);

    let existing: Option<String> = conn
        .query_row(
            "SELECT id FROM libraries WHERE root_path = ?1",
            [&root_string],
            |row| row.get(0),
        )
        .optional()?;

    let id = match existing {
        Some(id) => {
            conn.execute(
                "UPDATE libraries SET last_opened_at = ?1 WHERE id = ?2",
                rusqlite::params![now, id],
            )?;
            id
        }
        None => {
            let name = name
                .map(|value| value.trim().to_string())
                .filter(|value| !value.is_empty())
                .unwrap_or_else(|| crate::platform::default_library_name(&root));
            conn.execute(
                "INSERT INTO libraries (id, name, root_path, created_at, last_opened_at)
                 VALUES (?1, ?2, ?3, ?4, ?5)",
                rusqlite::params![id, name, root_string, now, now],
            )?;
            id
        }
    };

    Ok(load_library(conn, &id)?.expect("library row was just written"))
}

/// List registrations by recent activity and mark missing roots for relinking or forgetting.
pub fn list_libraries(conn: &Connection) -> Result<Vec<Library>> {
    let mut stmt = conn.prepare(&format!(
        "{SELECT_LIBRARY} ORDER BY COALESCE(last_opened_at, created_at) DESC"
    ))?;
    let mut libraries = stmt
        .query_map([], library_from_row)?
        .collect::<rusqlite::Result<Vec<_>>>()?;

    for library in &mut libraries {
        library.missing = !Path::new(&library.root_path).is_dir();
    }

    Ok(libraries)
}

pub fn load_library(conn: &Connection, id: &str) -> Result<Option<Library>> {
    let mut library = conn
        .query_row(
            &format!("{SELECT_LIBRARY} WHERE id = ?1"),
            [id],
            library_from_row,
        )
        .optional()?;

    if let Some(library) = library.as_mut() {
        library.missing = !Path::new(&library.root_path).is_dir();
    }
    Ok(library)
}

/// Update an existing registration to a validated root after its folder moves.
pub fn relink_library(conn: &Connection, id: &str, new_root: &Path) -> Result<Library> {
    let root = crate::platform::validate_library_root(new_root)?;

    if load_library(conn, id)?.is_none() {
        return Err(LibraryError::NotFound(id.to_string()));
    }

    let taken: Option<String> = conn
        .query_row(
            "SELECT id FROM libraries WHERE root_path = ?1 AND id <> ?2",
            rusqlite::params![root.to_string_lossy().as_ref(), id],
            |row| row.get(0),
        )
        .optional()?;
    if let Some(other) = taken {
        return Err(LibraryError::Invalid(format!(
            "that folder is already registered as a different library ({other})"
        )));
    }

    conn.execute(
        "UPDATE libraries SET root_path = ?1, last_opened_at = ?2 WHERE id = ?3",
        rusqlite::params![
            root.to_string_lossy().as_ref(),
            chrono::Utc::now().timestamp(),
            id
        ],
    )?;

    load_library(conn, id)?.ok_or_else(|| LibraryError::NotFound(id.to_string()))
}

/// Drop a library registration. This removes only Envision's derived data for
/// that id; the user's images are never touched.
pub fn forget_library(app: &AppHandle, conn: &Connection, id: &str) -> Result<()> {
    conn.execute("DELETE FROM libraries WHERE id = ?1", [id])?;
    let _ = std::fs::remove_dir_all(crate::platform::library_dir(app, id)?);
    Ok(())
}

/// Rename a library's display name without changing the folder on disk.
pub fn rename_library(conn: &Connection, id: &str, name: &str) -> Result<Library> {
    let name = name.trim();
    if name.is_empty() {
        return Err(LibraryError::Invalid("a library needs a name".into()));
    }

    conn.execute(
        "UPDATE libraries SET name = ?1 WHERE id = ?2",
        rusqlite::params![name, id],
    )?;

    load_library(conn, id)?.ok_or_else(|| LibraryError::NotFound(id.to_string()))
}


/// Keep cover identity for backend cache-key construction without serializing it to the UI.
struct FolderRow {
    folder: Folder,
    cover_asset_id: Option<i64>,
    cover_sha256: Option<String>,
}

fn folder_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<FolderRow> {
    Ok(FolderRow {
        folder: Folder {
            id: row.get(0)?,
            parent_id: row.get(1)?,
            name: row.get(2)?,
            rel_path: row.get(3)?,
            image_count: row.get(4)?,
            cover_url: None,
        },
        cover_asset_id: row.get(5)?,
        cover_sha256: row.get(6)?,
    })
}

const SELECT_FOLDER: &str = "
    SELECT f.id, f.parent_id, f.name, f.rel_path,
           (SELECT COUNT(*) FROM assets a WHERE a.folder_id = f.id) AS image_count,
           (SELECT a.id FROM assets a
             WHERE a.folder_id = f.id AND a.decode_ok = 1
             ORDER BY a.position ASC, a.id ASC LIMIT 1) AS cover_asset_id,
           (SELECT a.sha256 FROM assets a
             WHERE a.folder_id = f.id AND a.decode_ok = 1
             ORDER BY a.position ASC, a.id ASC LIMIT 1) AS cover_sha256
    FROM folders f";

/// Attach a cover URL only when its thumbnail exists; otherwise the UI uses a placeholder.
fn attach_folder_covers(index: &LibraryIndex, rows: &mut [FolderRow]) {
    for row in rows.iter_mut() {
        row.folder.cover_url = match row.cover_asset_id {
            Some(asset_id) => {
                let key = queries::thumbnail_key(asset_id, row.cover_sha256.as_deref());
                crate::thumbnails::is_cached(&index.thumbnails_dir, &key)
                    .then(|| crate::thumbnails::path_for(&index.thumbnails_dir, &key))
                    .flatten()
                    .map(|path| {
                        crate::platform::thumbnail_url(&index.thumbnails_dir, &index.id, &path)
                    })
            }
            None => None,
        };
    }
}

pub fn root_folder(index: &LibraryIndex) -> Result<Folder> {
    index
        .conn
        .query_row(
            &format!("{SELECT_FOLDER} WHERE f.rel_path = ''"),
            [],
            folder_from_row,
        )
        .map(|row| row.folder)
        .map_err(|error| match error {
            rusqlite::Error::QueryReturnedNoRows => LibraryError::FolderNotFound(0),
            other => other.into(),
        })
}

/// List direct children by name; `None` selects the root's children, not the root itself.
pub fn list_child_folders(index: &LibraryIndex, parent_id: Option<i64>) -> Result<Vec<Folder>> {
    let parent_id = match parent_id {
        Some(id) => id,
        None => root_folder(index)?.id,
    };

    let mut stmt = index.conn.prepare(&format!(
        "{SELECT_FOLDER} WHERE f.parent_id = ?1 ORDER BY f.name COLLATE NOCASE, f.id"
    ))?;

    let mut rows = stmt
        .query_map([parent_id], folder_from_row)?
        .collect::<rusqlite::Result<Vec<_>>>()?;

    attach_folder_covers(index, &mut rows);

    Ok(rows.into_iter().map(|row| row.folder).collect())
}

/// Every folder in the library, used to build breadcrumbs and the folder picker.
pub fn list_all_folders(index: &LibraryIndex) -> Result<Vec<Folder>> {
    let mut stmt = index
        .conn
        .prepare(&format!("{SELECT_FOLDER} ORDER BY f.rel_path"))?;
    let folders = stmt
        .query_map([], folder_from_row)?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    // Covers are deliberately not attached here: this feeds breadcrumbs and the
    // folder picker, neither of which renders one.
    Ok(folders.into_iter().map(|row| row.folder).collect())
}

pub fn folder_by_id(index: &LibraryIndex, id: i64) -> Result<Folder> {
    index
        .conn
        .query_row(
            &format!("{SELECT_FOLDER} WHERE f.id = ?1"),
            [id],
            folder_from_row,
        )
        .map(|row| row.folder)
        .map_err(|error| match error {
            rusqlite::Error::QueryReturnedNoRows => LibraryError::FolderNotFound(id),
            other => other.into(),
        })
}

pub fn folder_by_rel(index: &LibraryIndex, rel: &str) -> Result<Option<Folder>> {
    let folder = index
        .conn
        .query_row(
            &format!("{SELECT_FOLDER} WHERE f.rel_path = ?1"),
            [rel],
            folder_from_row,
        )
        .optional()?;
    Ok(folder.map(|row| row.folder))
}

/// Ancestors of `id`, outermost first, ending with the folder itself.
pub fn folder_ancestry(index: &LibraryIndex, id: i64) -> Result<Vec<Folder>> {
    let mut chain = Vec::new();
    let mut cursor = Some(id);
    let mut guard = 0;

    while let Some(current) = cursor {
        let folder = folder_by_id(index, current)?;
        cursor = folder.parent_id;
        chain.push(folder);
        guard += 1;
        if guard > 512 {
            return Err(LibraryError::Invalid("folder tree contains a cycle".into()));
        }
    }

    chain.reverse();
    Ok(chain)
}


pub struct AppState {
    pub app: AppHandle,
    app_db: Mutex<Connection>,
    open: Mutex<Option<LibraryIndex>>,
    scan: Mutex<Option<scan::ScanHandle>>,
    watch: Mutex<Option<watcher::WatcherHandle>>,
}

impl AppState {
    pub fn new(app: AppHandle, app_db: Connection) -> Self {
        Self {
            app,
            app_db: Mutex::new(app_db),
            open: Mutex::new(None),
            scan: Mutex::new(None),
            watch: Mutex::new(None),
        }
    }

    pub fn with_app_db<T>(&self, f: impl FnOnce(&Connection) -> Result<T>) -> Result<T> {
        let guard = self.app_db.lock().map_err(poisoned)?;
        f(&guard)
    }

    /// Run a synchronous callback under the open-library mutex; never hold it across an await.
    pub fn with_index<T>(&self, f: impl FnOnce(&LibraryIndex) -> Result<T>) -> Result<T> {
        let guard = self.open.lock().map_err(poisoned)?;
        match guard.as_ref() {
            Some(index) => f(index),
            None => Err(LibraryError::NoOpenLibrary),
        }
    }

    /// Open a library, replacing (and closing) whatever was open before.
    pub fn open_library(&self, id: &str) -> Result<Library> {
        let library = self
            .with_app_db(|conn| load_library(conn, id))?
            .ok_or_else(|| LibraryError::NotFound(id.to_string()))?;

        let root = crate::platform::validate_library_root(Path::new(&library.root_path))
            .map_err(|error| LibraryError::Invalid(error.to_string()))?;

        // Close the previous library first so no stale handle or cached
        // absolute path survives the switch.
        {
            let mut open = self.open.lock().map_err(poisoned)?;
            *open = None;
        }
        {
            let mut watch = self.watch.lock().map_err(poisoned)?;
            *watch = None;
        }
        {
            let mut scan = self.scan.lock().map_err(poisoned)?;
            if let Some(handle) = scan.as_mut() {
                handle.cancel();
            }
            *scan = None;
        }

        let index = LibraryIndex::open(&self.app, &library.id, root)?;

        self.with_app_db(|conn| {
            conn.execute(
                "UPDATE libraries SET last_opened_at = ?1 WHERE id = ?2",
                rusqlite::params![chrono::Utc::now().timestamp(), library.id],
            )?;
            Ok(())
        })?;

        *self.open.lock().map_err(poisoned)? = Some(index);
        Ok(Library {
            missing: false,
            ..library
        })
    }

    pub fn close_library(&self) {
        if let Ok(mut open) = self.open.lock() {
            *open = None;
        }
        if let Ok(mut watch) = self.watch.lock() {
            *watch = None;
        }
        if let Ok(mut scan) = self.scan.lock() {
            if let Some(handle) = scan.as_mut() {
                handle.cancel();
            }
            *scan = None;
        }
    }

    pub fn open_library_id(&self) -> Option<String> {
        self.open
            .lock()
            .ok()
            .and_then(|guard| guard.as_ref().map(|i| i.id.clone()))
    }

    pub fn install_watcher(&self, handle: watcher::WatcherHandle) {
        if let Ok(mut slot) = self.watch.lock() {
            *slot = Some(handle);
        }
    }

    pub fn set_scan(&self, handle: scan::ScanHandle) {
        if let Ok(mut slot) = self.scan.lock() {
            *slot = Some(handle);
        }
    }

    pub fn clear_scan(&self) {
        if let Ok(mut slot) = self.scan.lock() {
            *slot = None;
        }
    }

    /// Whether a scan is currently in flight.
    pub fn has_scan(&self) -> bool {
        self.scan.lock().map(|slot| slot.is_some()).unwrap_or(false)
    }

    /// Inspect the in-flight scan, if any.
    pub fn with_scan<T>(&self, f: impl FnOnce(&scan::ScanHandle) -> T) -> T {
        match self.scan.lock() {
            Ok(slot) => match slot.as_ref() {
                Some(handle) => f(handle),
                // No run in progress: a fresh handle reports "not cancelled",
                // which reads correctly as "nothing to cancel".
                None => f(&scan::ScanHandle::new()),
            },
            Err(_) => f(&scan::ScanHandle::new()),
        }
    }
}

fn poisoned<T>(_: T) -> LibraryError {
    LibraryError::Invalid("internal lock was poisoned by a previous panic".into())
}

// Re-exported so command handlers can stay thin.
pub use moves::{create_folder, move_assets, rename_folder, rename_folder_display};
pub use queries::{asset_by_id, duplicate_summary, list_assets, list_duplicates, reorder_assets};

#[cfg(test)]
mod tests {
    use super::*;
    use crate::database::{open_in_memory, APP_MIGRATIONS};

    fn registry() -> Connection {
        open_in_memory(APP_MIGRATIONS).unwrap()
    }

    fn temp_dir(tag: &str) -> PathBuf {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("envision-lib-{tag}-{nanos}"));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn adding_a_library_names_it_after_the_folder() {
        let conn = registry();
        let root = temp_dir("named");

        let library = add_library(&conn, None, &root).unwrap();
        assert_eq!(library.name, root.file_name().unwrap().to_string_lossy());

        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn adding_the_same_root_twice_reuses_one_registration() {
        let conn = registry();
        let root = temp_dir("idempotent");

        let first = add_library(&conn, Some("One"), &root).unwrap();
        let second = add_library(&conn, Some("Two"), &root).unwrap();

        assert_eq!(first.id, second.id);
        assert_eq!(list_libraries(&conn).unwrap().len(), 1);
        assert_eq!(second.name, "One", "an existing name must not be clobbered");

        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn missing_roots_are_reported_rather_than_dropped() {
        let conn = registry();
        let root = temp_dir("vanishing");
        add_library(&conn, None, &root).unwrap();

        std::fs::remove_dir_all(&root).unwrap();

        let libraries = list_libraries(&conn).unwrap();
        assert_eq!(libraries.len(), 1);
        assert!(libraries[0].missing, "a vanished root must be flagged");

        // Relinking restores it without changing identity.
        let moved = temp_dir("vanishing-moved");
        let relinked = relink_library(&conn, &libraries[0].id, &moved).unwrap();
        assert!(!relinked.missing);
        assert_eq!(relinked.id, libraries[0].id);

        std::fs::remove_dir_all(&moved).ok();
    }

    #[test]
    fn relinking_refuses_a_root_owned_by_another_library() {
        let conn = registry();
        let a = temp_dir("relink-a");
        let b = temp_dir("relink-b");
        let first = add_library(&conn, None, &a).unwrap();
        let _second = add_library(&conn, None, &b).unwrap();

        let error = relink_library(&conn, &first.id, &b).unwrap_err();
        assert!(matches!(error, LibraryError::Invalid(_)));

        std::fs::remove_dir_all(&a).ok();
        std::fs::remove_dir_all(&b).ok();
    }

    #[test]
    fn relinking_an_unknown_library_is_an_error() {
        let conn = registry();
        let root = temp_dir("relink-unknown");
        assert!(matches!(
            relink_library(&conn, "nope", &root),
            Err(LibraryError::NotFound(_))
        ));
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn forgetting_a_library_removes_only_the_registration() {
        let conn = registry();
        let root = temp_dir("forget");
        let library = add_library(&conn, None, &root).unwrap();

        forget_library_pathless(&conn, &library.id).unwrap();

        assert!(list_libraries(&conn).unwrap().is_empty());
        assert!(root.is_dir(), "the user's folder must be untouched");

        std::fs::remove_dir_all(&root).ok();
    }

    /// `forget_library` needs an `AppHandle` for the on-disk cache; this helper
    /// exercises just the registration half.
    fn forget_library_pathless(conn: &Connection, id: &str) -> Result<()> {
        conn.execute("DELETE FROM libraries WHERE id = ?1", [id])?;
        Ok(())
    }


    // --- folder tree, including the root query that binds no parameters ---
    /// An index seeded with a root and three nested folders, plus one image in
    /// each so folder counts can be checked too.
    fn seeded_index(tag: &str) -> LibraryIndex {
        let index = crate::library::queries::tests_support::index();
        index
            .conn
            .execute(
                "INSERT INTO folders (id, parent_id, name, rel_path, created_at)
                 VALUES (1, NULL, 'Library', '', 0)",
                [],
            )
            .unwrap();
        for (id, parent, name, rel) in [
            (2, 1, "Trips", "Trips"),
            (3, 2, "Iceland", "Trips/Iceland"),
            (4, 1, "Portraits", "Portraits"),
        ] {
            index
                .conn
                .execute(
                    "INSERT INTO folders (id, parent_id, name, rel_path, created_at)
                     VALUES (?1, ?2, ?3, ?4, 0)",
                    rusqlite::params![id, parent, name, rel],
                )
                .unwrap();
        }
        for (folder, file) in [(1i64, "loose.jpg"), (2, "a.jpg"), (3, "b.jpg")] {
            index
                .conn
                .execute(
                    "INSERT INTO assets (folder_id, filename, rel_path, ext, bytes, mtime,
                                         position, created_at, indexed_at)
                     VALUES (?1, ?2, ?2, 'jpg', 1, 0, 0, 0, 0)",
                    rusqlite::params![folder, file],
                )
                .unwrap();
        }
        let _ = tag;
        index
    }

    /// Regression: root listing must avoid an unbound SQLite parameter and exclude the root itself.
    #[test]
    fn root_children_are_listed_without_binding_a_parameter() {
        let index = seeded_index("root");

        let children = list_child_folders(&index, None).unwrap();
        let names: Vec<&str> = children.iter().map(|folder| folder.name.as_str()).collect();

        assert_eq!(names, vec!["Portraits", "Trips"], "ordered by name");
        assert!(
            !children.iter().any(|folder| folder.rel_path.is_empty()),
            "the root must not be its own child"
        );
    }

    #[test]
    fn child_folders_are_scoped_to_their_parent() {
        let index = seeded_index("child");

        let trips = list_child_folders(&index, Some(2)).unwrap();
        assert_eq!(trips.len(), 1);
        assert_eq!(trips[0].rel_path, "Trips/Iceland");

        assert!(list_child_folders(&index, Some(3)).unwrap().is_empty());
        assert!(list_child_folders(&index, Some(9999)).unwrap().is_empty());
    }

    #[test]
    fn folder_rows_carry_a_direct_image_count() {
        let index = seeded_index("counts");

        let root = root_folder(&index).unwrap();
        assert_eq!(root.image_count, 1, "loose images sit at the root");
        assert_eq!(
            list_child_folders(&index, Some(2)).unwrap()[0].image_count,
            1
        );

        let deep = folder_by_id(&index, 3).unwrap();
        assert_eq!(deep.image_count, 1);
    }


    // --- folder covers ------------------------------------------------------
    /// A folder row with three images, the first of which is undecodable, so
    /// the cover must skip it.
    fn cover_index(tag: &str) -> LibraryIndex {
        let index = seeded_index(tag);
        index
            .conn
            .execute(
                "INSERT INTO assets (folder_id, filename, rel_path, ext, bytes, mtime,
                                     decode_ok, position, created_at, indexed_at)
                 VALUES (2, 'heic.heic', 'Trips/heic.heic', 'heic', 5, 0, 0, 0, 0, 0)",
                [],
            )
            .unwrap();
        index
    }

    #[test]
    fn a_folder_cover_is_the_first_decodable_image_by_position() {
        let index = cover_index("cover-order");

        let rows = {
            let sql = format!("{SELECT_FOLDER} WHERE f.id = ?1");
            let mut stmt = index.conn.prepare(&sql).unwrap();
            let mut rows = stmt
                .query_map([2_i64], folder_from_row)
                .unwrap()
                .collect::<rusqlite::Result<Vec<_>>>()
                .unwrap();
            attach_folder_covers(&index, &mut rows);
            rows
        };

        // Position 0 is an undecodable HEIC; position 1 is the first real image.
        assert_eq!(rows[0].cover_asset_id, Some(2), "skips the HEIC");
        // No thumbnail is cached in this fixture, so nothing is advertised.
        assert_eq!(rows[0].folder.cover_url, None);
    }

    #[test]
    fn no_cover_is_advertised_before_the_thumbnail_exists() {
        let index = cover_index("cover-uncached");

        let children = list_child_folders(&index, Some(2)).unwrap();
        assert_eq!(children[0].cover_url, None);
    }

    #[test]
    fn an_empty_folder_has_no_cover() {
        let index = seeded_index("cover-empty");
        index
            .conn
            .execute(
                "INSERT INTO folders (id, parent_id, name, rel_path, created_at)
                 VALUES (9, 1, 'Empty', 'Empty', 0)",
                [],
            )
            .unwrap();

        let children = list_child_folders(&index, Some(1)).unwrap();
        let empty = children
            .iter()
            .find(|folder| folder.name == "Empty")
            .expect("Empty");

        assert_eq!(empty.image_count, 0);
        assert_eq!(empty.cover_url, None);
    }

    #[test]
    fn folder_listings_never_leak_the_cover_identity() {
        let index = cover_index("cover-shape");

        let folders = list_child_folders(&index, Some(2)).unwrap();
        let json = serde_json::to_string(&folders[0]).unwrap();
        assert!(!json.contains("cover_asset_id"), "{json}");
        assert!(!json.contains("cover_sha256"), "{json}");
        assert!(json.contains("cover_url"), "{json}");
    }

    #[test]
    fn folder_ancestry_runs_outermost_first() {
        let index = seeded_index("ancestry");

        let chain: Vec<String> = folder_ancestry(&index, 3)
            .unwrap()
            .iter()
            .map(|folder| folder.rel_path.clone())
            .collect();

        assert_eq!(chain, vec!["", "Trips", "Trips/Iceland"]);
    }
}
