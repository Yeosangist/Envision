
//! Aggregate index and app-data sizes for Settings; user image folders are not measured here.
use crate::domain::{LibraryTotals, Stats};
use rusqlite::{Connection, OpenFlags};
use std::path::Path;

#[derive(Debug, thiserror::Error)]
pub enum StatsError {
    #[error("SQLite error: {0}")]
    Sqlite(#[from] rusqlite::Error),
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
}

pub type Result<T> = std::result::Result<T, StatsError>;

/// Counts SHA-256 groups with at least two assets and the extra copies beyond one.
const DUPLICATES_SQL: &str = "SELECT COUNT(*), COALESCE(SUM(copies - 1), 0) FROM (
         SELECT COUNT(*) AS copies FROM assets
         WHERE sha256 IS NOT NULL
         GROUP BY sha256
         HAVING COUNT(*) > 1
     )";

/// Count images, non-root folders, bytes, duplicate groups, and redundant copies.
pub fn index_totals(path: &Path) -> Result<LibraryTotals> {
    let conn = Connection::open_with_flags(
        path,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )?;

    let images = conn.query_row("SELECT COUNT(*) FROM assets", [], |row| row.get(0))?;
    let folders =
        conn.query_row("SELECT COUNT(*) FROM folders WHERE rel_path <> ''", [], |row| {
            row.get(0)
        })?;
    let bytes = conn.query_row("SELECT COALESCE(SUM(bytes), 0) FROM assets", [], |row| {
        row.get(0)
    })?;
    let (duplicate_groups, redundant) =
        conn.query_row(DUPLICATES_SQL, [], |row| Ok((row.get(0)?, row.get(1)?)))?;

    Ok(LibraryTotals {
        images,
        folders,
        duplicate_groups,
        redundant,
        bytes,
    })
}

/// Recursively total regular files without following symbolic links.
pub fn dir_size(path: &Path) -> Result<u64> {
    if !path.is_dir() {
        return Ok(0);
    }

    let mut total = 0;
    let mut pending = vec![path.to_path_buf()];

    while let Some(dir) = pending.pop() {
        for entry in std::fs::read_dir(&dir)? {
            let entry = entry?;
            let kind = entry.file_type()?;
            if kind.is_dir() {
                pending.push(entry.path());
            } else if kind.is_file() {
                total += entry.metadata()?.len();
            }
        }
    }

    Ok(total)
}

/// Size of one file, or zero when it is not there.
pub fn file_size(path: &Path) -> u64 {
    std::fs::metadata(path).map(|meta| meta.len()).unwrap_or(0)
}

/// Return zeroes for absent or unreadable indexes; log read failures to distinguish them from empty libraries.
pub fn summary(index_path: &Path) -> LibraryTotals {
    if !index_path.is_file() {
        return LibraryTotals::default();
    }

    match index_totals(index_path) {
        Ok(totals) => totals,
        Err(error) => {
            log::warn!("could not read index {}: {error}", index_path.display());
            LibraryTotals::default()
        }
    }
}

/// Skip unopened libraries and mark unreadable indexes so aggregate totals can be reported incomplete.
pub fn accumulate_index(stats: &mut Stats, index_path: &Path) {
    if !index_path.is_file() {
        return;
    }

    match index_totals(index_path) {
        Ok(totals) => {
            stats.images += totals.images;
            stats.folders += totals.folders;
            stats.duplicate_groups += totals.duplicate_groups;
            stats.redundant += totals.redundant;
            stats.library_bytes += totals.bytes;
        }
        Err(error) => {
            // Logged rather than swallowed: a silent zero here looks exactly
            // like an empty library.
            log::warn!("could not read index {}: {error}", index_path.display());
            stats.unreadable += 1;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn temp_dir(tag: &str) -> PathBuf {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("envision-stats-{tag}-{nanos}"));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// A real index with a folder tree, a duplicate pair and a unique file.
    fn seeded_index(dir: &Path) -> PathBuf {
        let path = dir.join("index.db");
        let conn = crate::database::open(&path, crate::database::LIBRARY_MIGRATIONS).unwrap();
        conn.execute_batch(
            "INSERT INTO folders (id, parent_id, name, rel_path, created_at)
                 VALUES (1, NULL, '', '', 0), (2, 1, 'a', 'a', 0), (3, 1, 'b', 'b', 0);
             INSERT INTO assets
                 (folder_id, filename, rel_path, ext, bytes, mtime, sha256, position, created_at, indexed_at)
                 VALUES
                 (1, 'one.jpg',  'one.jpg',  'jpg', 100, 0, 'aa', 0, 0, 0),
                 (1, 'copy.jpg', 'copy.jpg', 'jpg', 100, 0, 'aa', 1, 0, 0),
                 (2, 'two.jpg',  'a/two.jpg', 'jpg', 250, 0, 'bb', 0, 0, 0);",
        )
        .unwrap();
        path
    }

    #[test]
    fn counts_are_taken_over_the_whole_index() {
        let dir = temp_dir("counts");
        let path = seeded_index(&dir);

        let totals = index_totals(&path).unwrap();
        assert_eq!(totals.images, 3);
        // The root is not one of the user's folders.
        assert_eq!(totals.folders, 2);
        assert_eq!(totals.bytes, 450);
        assert_eq!(totals.duplicate_groups, 1);
        assert_eq!(totals.redundant, 1);
    }

    #[test]
    fn an_untouched_index_contributes_nothing_and_is_not_an_error() {
        let dir = temp_dir("empty");
        let mut stats = Stats::default();

        accumulate_index(&mut stats, &dir.join("index.db"));

        assert_eq!(stats, Stats::default());
        assert_eq!(stats.unreadable, 0);
    }

    #[test]
    fn a_library_without_an_index_is_skipped_not_blamed() {
        let dir = temp_dir("no-index");
        let mut stats = Stats::default();
        // Present but not a database: the index file exists, so this one *is*
        // a read failure and must be reported rather than counted as zero.
        std::fs::write(dir.join("index.db"), b"not a database").unwrap();

        accumulate_index(&mut stats, &dir.join("index.db"));

        assert_eq!(stats.images, 0);
        assert_eq!(stats.unreadable, 1);
    }

    #[test]
    fn directory_size_ignores_symlinks() {
        let dir = temp_dir("size");
        let nested = dir.join("nested");
        std::fs::create_dir_all(&nested).unwrap();
        std::fs::write(dir.join("a.bin"), vec![0u8; 10]).unwrap();
        std::fs::write(nested.join("b.bin"), vec![0u8; 32]).unwrap();

        assert_eq!(dir_size(&dir).unwrap(), 42);

        // A link pointing back at the tree cannot make the walk unbounded.
        #[cfg(unix)]
        std::os::unix::fs::symlink(&dir, dir.join("loop")).unwrap();
        assert_eq!(dir_size(&dir).unwrap(), 42);
    }

    #[test]
    fn missing_paths_measure_as_zero() {
        let dir = temp_dir("missing");
        assert_eq!(dir_size(&dir.join("nope")).unwrap(), 0);
        assert_eq!(file_size(&dir.join("nope.db")), 0);
    }
}
