// SPDX-License-Identifier: AGPL-3.0-or-later
//! SQLite index over the vault.
//!
//! The vault on disk is authoritative (ADR-002). This index is rebuildable
//! at any time by walking the vault. It exists only to make queries cheap:
//! list-by-date, tag query, full-text search, and (M1b+) filesystem
//! reconciliation.

use rusqlite::{params, Connection};
use std::path::Path;

#[derive(Debug, thiserror::Error)]
pub enum IndexError {
    #[error("SQLite error: {0}")]
    Sqlite(#[from] rusqlite::Error),
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
}

/// SQLite-backed index. Holds an open connection.
#[derive(Debug)]
pub struct Index {
    conn: Connection,
}

/// Migrations applied in order. Each migration runs at most once. The
/// applied set is recorded in the `meta` table (`schema_version` integer).
/// Adding a new migration = append to the array, never reorder or edit.
const MIGRATIONS: &[&str] = &[
    // 1: initial schema. The `meta` table is created by the bootstrap step
    // before this migration runs; do not redeclare it here.
    r#"
    CREATE TABLE note_index (
        id TEXT PRIMARY KEY,        -- ULID, also the filename stem
        path TEXT NOT NULL,         -- vault-relative path to the .md file
        title TEXT,
        created TEXT NOT NULL,      -- ISO-8601 with 'Z'
        updated TEXT NOT NULL,
        body_preview TEXT NOT NULL DEFAULT ''
    ) STRICT;

    CREATE INDEX idx_note_index_created ON note_index(created DESC);

    CREATE TABLE file_state (
        path TEXT PRIMARY KEY,      -- vault-relative
        mtime INTEGER NOT NULL,     -- seconds since epoch
        size INTEGER NOT NULL,
        sha256 BLOB NOT NULL
    ) STRICT;
    "#,
];

impl Index {
    /// Open (or create) the index at `path`. Runs any pending migrations.
    pub fn open(path: &Path) -> Result<Self, IndexError> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let conn = Connection::open(path)?;
        // Pragmas for safety + perf appropriate to a single-process desktop.
        conn.execute_batch(
            "
            PRAGMA journal_mode = WAL;
            PRAGMA synchronous = NORMAL;
            PRAGMA foreign_keys = ON;
            ",
        )?;
        let mut idx = Self { conn };
        idx.migrate()?;
        Ok(idx)
    }

    fn migrate(&mut self) -> Result<(), IndexError> {
        // Bootstrap: ensure the meta table exists before we can read its version.
        self.conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS meta (key TEXT PRIMARY KEY, value TEXT NOT NULL) STRICT;",
        )?;

        let current: i64 = self
            .conn
            .query_row(
                "SELECT value FROM meta WHERE key = 'schema_version'",
                [],
                |row| row.get::<_, String>(0),
            )
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(0);

        for (i, sql) in MIGRATIONS.iter().enumerate() {
            let target = (i + 1) as i64;
            if current >= target {
                continue;
            }
            let tx = self.conn.transaction()?;
            tx.execute_batch(sql)?;
            tx.execute(
                "INSERT INTO meta (key, value) VALUES ('schema_version', ?1)
                 ON CONFLICT(key) DO UPDATE SET value = excluded.value",
                params![target.to_string()],
            )?;
            tx.commit()?;
        }
        Ok(())
    }

    /// Current applied migration version (1-based; 0 means none applied).
    pub fn schema_version(&self) -> Result<i64, IndexError> {
        let v: String = self
            .conn
            .query_row(
                "SELECT value FROM meta WHERE key = 'schema_version'",
                [],
                |row| row.get(0),
            )
            .unwrap_or_else(|_| "0".to_string());
        Ok(v.parse().unwrap_or(0))
    }

    /// Names of all user tables (for tests / debugging).
    pub fn list_tables(&self) -> Result<Vec<String>, IndexError> {
        let mut stmt = self
            .conn
            .prepare("SELECT name FROM sqlite_master WHERE type = 'table' ORDER BY name")?;
        let names: Result<Vec<String>, _> = stmt.query_map([], |row| row.get(0))?.collect();
        Ok(names?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn open_creates_schema_on_fresh_db() {
        let dir = tempdir().unwrap();
        let path = dir.path().join(".index/notes.sqlite");
        let idx = Index::open(&path).expect("must open fresh");
        let tables = idx.list_tables().unwrap();
        assert!(
            tables.contains(&"note_index".to_string()),
            "missing note_index: {tables:?}"
        );
        assert!(
            tables.contains(&"file_state".to_string()),
            "missing file_state: {tables:?}"
        );
        assert!(
            tables.contains(&"meta".to_string()),
            "missing meta: {tables:?}"
        );
    }

    #[test]
    fn open_creates_parent_directory() {
        // The vault layer hands us .index/notes.sqlite; the .index dir may not
        // exist yet. open() must mkdir -p, not error.
        let dir = tempdir().unwrap();
        let path = dir.path().join("a/b/c/notes.sqlite");
        let _idx = Index::open(&path).expect("must mkdir parents");
        assert!(path.exists());
    }

    #[test]
    fn schema_version_is_1_after_first_open() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("v.sqlite");
        let idx = Index::open(&path).unwrap();
        assert_eq!(idx.schema_version().unwrap(), 1);
    }

    #[test]
    fn second_open_is_a_no_op() {
        // Re-opening must not re-run migrations or fail. Idempotency is
        // critical because every app launch hits this path.
        let dir = tempdir().unwrap();
        let path = dir.path().join("idempotent.sqlite");
        {
            let _idx = Index::open(&path).unwrap();
        }
        let idx = Index::open(&path).expect("re-open must succeed");
        assert_eq!(idx.schema_version().unwrap(), 1);
        let tables = idx.list_tables().unwrap();
        assert!(tables.contains(&"note_index".to_string()));
    }

    #[test]
    fn schema_version_persists_across_opens() {
        // Even if MIGRATIONS gets a future entry, an existing DB must report
        // its current applied version so the next migrate() picks up only
        // pending steps. Not testing future migrations directly — testing the
        // persistence contract.
        let dir = tempdir().unwrap();
        let path = dir.path().join("persist.sqlite");
        {
            let _idx = Index::open(&path).unwrap();
        }
        let idx = Index::open(&path).unwrap();
        let v: String = idx
            .conn
            .query_row(
                "SELECT value FROM meta WHERE key = 'schema_version'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(v, "1");
    }

    #[test]
    fn note_index_indexes_created_for_chrono_query() {
        // Confirm the descending index on created exists; this is the path
        // hot for the M5 timeline view, must not regress.
        let dir = tempdir().unwrap();
        let path = dir.path().join("idx.sqlite");
        let idx = Index::open(&path).unwrap();
        let names: Vec<String> = idx
            .conn
            .prepare(
                "SELECT name FROM sqlite_master WHERE type = 'index' AND name NOT LIKE 'sqlite_%'",
            )
            .unwrap()
            .query_map([], |row| row.get(0))
            .unwrap()
            .filter_map(|r| r.ok())
            .collect();
        assert!(
            names.contains(&"idx_note_index_created".to_string()),
            "missing chrono index: {names:?}"
        );
    }
}
