
//! SQLite setup for the app database and each library's rebuildable index.
use rusqlite::{Connection, OptionalExtension};
use std::path::Path;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum DatabaseError {
    #[error("SQLite error: {0}")]
    Sqlite(#[from] rusqlite::Error),
    #[error("Database migration error: {0}")]
    Migration(String),
    #[error("Database I/O error: {0}")]
    Io(#[from] std::io::Error),
}

pub type Result<T> = std::result::Result<T, DatabaseError>;

pub struct Migration {
    pub version: i64,
    pub name: &'static str,
    pub sql: &'static str,
}

/// Schema for the global application database.
pub const APP_MIGRATIONS: &[Migration] = &[Migration {
    version: 1,
    name: "001_app",
    sql: include_str!("../../migrations/001_app.sql"),
}];

/// Schema for a per-library query index.
pub const LIBRARY_MIGRATIONS: &[Migration] = &[Migration {
    version: 1,
    name: "002_library",
    sql: include_str!("../../migrations/002_library.sql"),
}];

/// Use case-insensitive path comparison on macOS and Windows, case-sensitive elsewhere.
pub const fn path_collation() -> &'static str {
    if cfg!(any(target_os = "windows", target_os = "macos")) {
        "NOCASE"
    } else {
        "BINARY"
    }
}

/// Substitute the `{collation}` placeholder used by the migration files.
pub fn render(sql: &str) -> String {
    sql.replace("{collation}", path_collation())
}

/// Open and migrate a database with WAL and a busy timeout.
pub fn open(path: &Path, migrations: &[Migration]) -> Result<Connection> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    let conn = Connection::open(path)?;
    conn.busy_timeout(std::time::Duration::from_secs(5))?;
    conn.pragma_update(None, "journal_mode", "WAL")?;
    conn.pragma_update(None, "foreign_keys", "ON")?;

    migrate(&conn, migrations)?;
    Ok(conn)
}

/// Open a throwaway in-memory database with a schema applied. Used by tests.
#[cfg(test)]
pub fn open_in_memory(migrations: &[Migration]) -> Result<Connection> {
    let conn = Connection::open_in_memory()?;
    conn.pragma_update(None, "foreign_keys", "ON")?;
    migrate(&conn, migrations)?;
    Ok(conn)
}

fn migrate(conn: &Connection, migrations: &[Migration]) -> Result<()> {
    let current: i64 = conn.pragma_query_value(None, "user_version", |row| row.get(0))?;

    let mut ordered: Vec<&Migration> = migrations.iter().collect();
    ordered.sort_by_key(|migration| migration.version);

    for migration in ordered {
        if migration.version <= current {
            continue;
        }
        conn.execute_batch(&render(migration.sql))
            .map_err(|error| {
                DatabaseError::Migration(format!(
                    "{} (v{}) failed: {error}",
                    migration.name, migration.version
                ))
            })?;
        // Stamp after the DDL so an interrupted upgrade re-runs it. Every
        // statement is `IF NOT EXISTS`, so re-running is safe.
        conn.pragma_update(None, "user_version", migration.version)?;
    }

    Ok(())
}

/// Read a setting as text.
pub fn read_setting(conn: &Connection, key: &str) -> Result<Option<String>> {
    let value = conn
        .query_row("SELECT value FROM settings WHERE key = ?1", [key], |row| {
            row.get::<_, String>(0)
        })
        .optional()?;
    Ok(value)
}

/// Write a setting as text.
pub fn write_setting(conn: &Connection, key: &str, value: &str) -> Result<()> {
    conn.execute(
        "INSERT INTO settings (key, value) VALUES (?1, ?2)
         ON CONFLICT(key) DO UPDATE SET value = excluded.value",
        rusqlite::params![key, value],
    )?;
    Ok(())
}

/// Delete a setting row, preserving the distinction between unset and blank.
pub fn delete_setting(conn: &Connection, key: &str) -> Result<()> {
    conn.execute("DELETE FROM settings WHERE key = ?1", [key])?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn placeholder_is_always_substituted() {
        let rendered = render("CREATE INDEX i ON t(c COLLATE {collation});");
        assert!(!rendered.contains("{collation}"));
        assert!(rendered.contains(path_collation()));
    }

    #[test]
    fn schema_contains_no_unsubstituted_placeholders() {
        for migration in APP_MIGRATIONS.iter().chain(LIBRARY_MIGRATIONS) {
            assert!(
                !render(migration.sql).contains("{collation}"),
                "{} left a placeholder unresolved",
                migration.name
            );
        }
    }

    #[test]
    fn app_schema_creates_registry_and_settings() {
        let conn = open_in_memory(APP_MIGRATIONS).unwrap();
        let version: i64 = conn
            .pragma_query_value(None, "user_version", |r| r.get(0))
            .unwrap();
        assert_eq!(version, 1);

        conn.execute(
            "INSERT INTO libraries (id, name, root_path, created_at) VALUES ('a', 'A', '/a', 0)",
            [],
        )
        .unwrap();
        assert_eq!(read_setting(&conn, "theme").unwrap(), None);
        write_setting(&conn, "theme", "dark").unwrap();
        assert_eq!(
            read_setting(&conn, "theme").unwrap().as_deref(),
            Some("dark")
        );
    }

    #[test]
    fn library_schema_has_the_tables_the_indexer_needs() {
        let conn = open_in_memory(LIBRARY_MIGRATIONS).unwrap();

        let table_names = ["folders", "assets", "meta"];
        for table in table_names {
            let exists: i64 = conn
                .query_row(
                    "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = ?1",
                    [table],
                    |row| row.get(0),
                )
                .unwrap();
            assert_eq!(exists, 1, "table {table} is missing");
        }

        // The columns the scan and query layers actually bind by position.
        let asset_columns: Vec<String> = {
            let mut stmt = conn.prepare("PRAGMA table_info(assets)").unwrap();
            let rows = stmt
                .query_map([], |row| row.get::<_, String>(1))
                .unwrap()
                .collect::<rusqlite::Result<Vec<_>>>()
                .unwrap();
            rows
        };
        for column in [
            "id",
            "folder_id",
            "filename",
            "rel_path",
            "ext",
            "mime",
            "width",
            "height",
            "bytes",
            "mtime",
            "sha256",
            "decode_ok",
            "position",
        ] {
            assert!(
                asset_columns.iter().any(|name| name == column),
                "assets is missing {column}"
            );
        }
    }

    #[test]
    fn library_schema_rejects_duplicate_paths() {
        let conn = open_in_memory(LIBRARY_MIGRATIONS).unwrap();
        let now = 0;
        conn.execute(
            "INSERT INTO folders (id, parent_id, name, rel_path, created_at)
             VALUES (1, NULL, '', '', ?1)",
            [now],
        )
        .unwrap();

        let insert = |rel: &str| {
            conn.execute(
                "INSERT INTO assets (folder_id, filename, rel_path, ext, bytes, mtime,
                                     created_at, indexed_at)
                 VALUES (1, 'a.jpg', ?1, 'jpg', 1, 0, 0, 0)",
                [rel],
            )
        };

        insert("a.jpg").unwrap();
        assert!(insert("a.jpg").is_err(), "duplicate path must be rejected");
    }

    #[test]
    fn deleting_a_folder_cascades_to_its_assets() {
        let conn = open_in_memory(LIBRARY_MIGRATIONS).unwrap();
        conn.execute(
            "INSERT INTO folders (id, parent_id, name, rel_path, created_at)
             VALUES (1, NULL, 'album', 'album', 0)",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO assets (folder_id, filename, rel_path, ext, bytes, mtime,
                                 created_at, indexed_at)
             VALUES (1, 'a.jpg', 'album/a.jpg', 'jpg', 1, 0, 0, 0)",
            [],
        )
        .unwrap();

        conn.execute("DELETE FROM folders WHERE id = 1", [])
            .unwrap();
        let remaining: i64 = conn
            .query_row("SELECT COUNT(*) FROM assets", [], |row| row.get(0))
            .unwrap();
        assert_eq!(remaining, 0);
    }

    #[test]
    fn reopening_an_upgraded_database_is_a_no_op() {
        let dir = std::env::temp_dir().join(format!(
            "envision-db-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));

        let path = dir.join("nested").join("index.db");
        drop(open(&path, LIBRARY_MIGRATIONS).unwrap());
        drop(open(&path, LIBRARY_MIGRATIONS).unwrap());
        drop(open(&path, LIBRARY_MIGRATIONS).unwrap());

        let conn = open(&path, LIBRARY_MIGRATIONS).unwrap();
        let version: i64 = conn
            .pragma_query_value(None, "user_version", |r| r.get(0))
            .unwrap();
        assert_eq!(version, 1);

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn journal_mode_is_wal() {
        let dir = std::env::temp_dir().join(format!(
            "envision-wal-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let conn = open(&dir.join("index.db"), LIBRARY_MIGRATIONS).unwrap();
        let mode: String = conn
            .query_row("PRAGMA journal_mode", [], |row| row.get(0))
            .unwrap();
        assert_eq!(mode.to_ascii_lowercase(), "wal");
        drop(conn);
        std::fs::remove_dir_all(&dir).ok();
    }
}
