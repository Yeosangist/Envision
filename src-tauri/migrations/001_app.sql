
PRAGMA journal_mode = WAL;
PRAGMA foreign_keys = ON;

CREATE TABLE IF NOT EXISTS libraries (
    id              TEXT    PRIMARY KEY,
    name            TEXT    NOT NULL,
    root_path       TEXT    NOT NULL,
    created_at      INTEGER NOT NULL,
    last_opened_at  INTEGER
);

-- Paths are compared case-insensitively only where the host filesystem is.
-- `{collation}` is substituted by the Rust migration runner before execution.
CREATE UNIQUE INDEX IF NOT EXISTS idx_libraries_root_path
    ON libraries(root_path COLLATE {collation});

CREATE TABLE IF NOT EXISTS settings (
    key   TEXT PRIMARY KEY,
    value TEXT NOT NULL
);

PRAGMA user_version = 1;