
PRAGMA journal_mode = WAL;
PRAGMA foreign_keys = ON;

CREATE TABLE IF NOT EXISTS folders (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    parent_id   INTEGER REFERENCES folders(id) ON DELETE CASCADE,
    name        TEXT    NOT NULL,
    rel_path    TEXT    NOT NULL,
    created_at  INTEGER NOT NULL
);

CREATE UNIQUE INDEX IF NOT EXISTS idx_folders_rel_path
    ON folders(rel_path COLLATE {collation});
CREATE INDEX IF NOT EXISTS idx_folders_parent ON folders(parent_id);

CREATE TABLE IF NOT EXISTS assets (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    folder_id   INTEGER NOT NULL REFERENCES folders(id) ON DELETE CASCADE,

    filename    TEXT    NOT NULL,
    -- Path relative to the library root, '/'-separated.
    rel_path    TEXT    NOT NULL,

    ext         TEXT    NOT NULL,
    mime        TEXT,
    width       INTEGER,
    height      INTEGER,
    bytes       INTEGER NOT NULL,
    mtime       INTEGER NOT NULL,

    sha256      TEXT,
    hashed_at   INTEGER,

    decode_ok   INTEGER NOT NULL DEFAULT 1,

    -- Display order within `folder_id`. This is database-only state: reordering
    -- the grid never touches a file, a name, or a directory.
    position    INTEGER NOT NULL DEFAULT 0,

    created_at  INTEGER NOT NULL,
    indexed_at  INTEGER NOT NULL
);

CREATE UNIQUE INDEX IF NOT EXISTS idx_assets_rel_path
    ON assets(rel_path COLLATE {collation});
CREATE INDEX IF NOT EXISTS idx_assets_folder_position
    ON assets(folder_id, position);
CREATE INDEX IF NOT EXISTS idx_assets_sha256
    ON assets(sha256);

CREATE TABLE IF NOT EXISTS meta (
    key   TEXT PRIMARY KEY,
    value TEXT NOT NULL
);

PRAGMA user_version = 1;