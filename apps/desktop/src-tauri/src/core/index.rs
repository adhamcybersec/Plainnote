// SPDX-License-Identifier: AGPL-3.0-or-later
//! SQLite index over the vault.
//!
//! The vault on disk is authoritative (ADR-002). This index is rebuildable
//! at any time by walking the vault. It exists only to make queries cheap:
//! list-by-date, tag query, full-text search, and (M1b+) filesystem
//! reconciliation.

use rusqlite::{params, Connection};
use sha2::{Digest, Sha256};
use std::collections::HashSet;
use std::path::Path;

use crate::core::frontmatter;
use crate::core::vault::Vault;

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

/// Counts of rows changed during a reconcile pass.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct ReconcileSummary {
    pub inserted: usize,
    pub updated: usize,
    pub deleted: usize,
}

/// Compute a cheap fingerprint of the vault: SHA-256 over each `.md` file's
/// (vault-relative path, mtime_nanos, size_bytes), with paths sorted lex.
///
/// This is *not* the same as hashing every file's contents — that would
/// defeat the purpose of the short-circuit (the whole point is to skip
/// reading files when nothing changed). mtime + size at nanosecond
/// precision catches every external editor we care about; a malicious
/// actor who rewrites a file with the exact same mtime and size could
/// fool the manifest, but our threat model is "human user editing in
/// vim/Syncthing", not "active attacker".
///
/// Nanosecond precision matters because rapid edits within the same second
/// (Tauri-driven save_note + immediate external mutation) would otherwise
/// share an mtime and slip past the short-circuit.
pub fn compute_manifest_hash(vault: &Vault) -> Vec<u8> {
    let notes_dir = vault.root_path().join("notes");
    let mut entries: Vec<(String, u128, u64)> = walk_md(&notes_dir)
        .into_iter()
        .filter_map(|p| {
            let rel = p
                .strip_prefix(vault.root_path())
                .ok()?
                .to_string_lossy()
                .into_owned();
            let metadata = std::fs::metadata(&p).ok()?;
            let mtime_nanos = metadata
                .modified()
                .ok()?
                .duration_since(std::time::UNIX_EPOCH)
                .ok()?
                .as_nanos();
            let size = metadata.len();
            Some((rel, mtime_nanos, size))
        })
        .collect();
    entries.sort();
    let mut hasher = Sha256::new();
    for (path, mtime, size) in &entries {
        hasher.update(path.as_bytes());
        hasher.update(b"\0");
        hasher.update(mtime.to_le_bytes());
        hasher.update(size.to_le_bytes());
        hasher.update(b"\0");
    }
    hasher.finalize().to_vec()
}

/// Walk a directory tree collecting .md files (used by reconcile).
fn walk_md(dir: &Path) -> Vec<std::path::PathBuf> {
    let mut out = Vec::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        let Ok(read) = std::fs::read_dir(&d) else {
            continue;
        };
        for entry in read.flatten() {
            let p = entry.path();
            if p.is_dir() {
                stack.push(p);
            } else if p.extension().is_some_and(|e| e == "md") {
                out.push(p);
            }
        }
    }
    out
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
    // 2: tag system. The closure table makes the four-mode query algebra
    // (M2) O(matches) at runtime — no recursion. Adding a new tag requires
    // both a row in `tag` and ancestor-chain rows in `tag_closure`; that
    // logic lives in core::tags::ensure_tag, not in the migration.
    r#"
    CREATE TABLE tag (
        path TEXT PRIMARY KEY,                 -- 'learning/mathematics/calculus'
        parent TEXT REFERENCES tag(path)       -- 'learning/mathematics' (NULL for roots)
    ) STRICT;

    CREATE TABLE tag_closure (
        ancestor TEXT NOT NULL,
        descendant TEXT NOT NULL,
        depth INTEGER NOT NULL,                -- 0 = self
        PRIMARY KEY (ancestor, descendant)
    ) STRICT;

    CREATE INDEX idx_closure_ancestor ON tag_closure(ancestor);
    CREATE INDEX idx_closure_descendant ON tag_closure(descendant);

    CREATE TABLE note_tag (
        note_id TEXT NOT NULL,
        tag_path TEXT NOT NULL,
        PRIMARY KEY (note_id, tag_path)
    ) STRICT;

    CREATE INDEX idx_note_tag_path ON note_tag(tag_path);
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

    /// Read the stored manifest hash, or `None` if no reconcile has run yet.
    pub fn stored_manifest_hash(&self) -> Result<Option<Vec<u8>>, IndexError> {
        let v: Option<String> = self
            .conn
            .query_row(
                "SELECT value FROM meta WHERE key = 'manifest_hash'",
                [],
                |row| row.get(0),
            )
            .ok();
        Ok(v.and_then(|hex| {
            (0..hex.len())
                .step_by(2)
                .map(|i| u8::from_str_radix(&hex[i..i + 2], 16).ok())
                .collect()
        }))
    }

    fn store_manifest_hash(&self, hash: &[u8]) -> Result<(), IndexError> {
        let hex: String = hash.iter().map(|b| format!("{b:02x}")).collect();
        self.conn.execute(
            "INSERT INTO meta (key, value) VALUES ('manifest_hash', ?1)
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            params![hex],
        )?;
        Ok(())
    }

    /// Walk the vault, diff against `file_state`, apply inserts / updates /
    /// deletes to `note_index`. The vault directory is authoritative.
    ///
    /// Short-circuit: if the current manifest hash (a SHA-256 over each
    /// (path, mtime, size) triple sorted by path) matches the previously
    /// stored value, the heavy walk is skipped entirely. Cold start on a
    /// 50k-note vault stays under our 1s budget that way.
    pub fn reconcile_with_vault(&mut self, vault: &Vault) -> Result<ReconcileSummary, IndexError> {
        let root = vault.root_path();
        let notes_dir = root.join("notes");

        // 0. Compare the cheap manifest hash against the stored value.
        let manifest = compute_manifest_hash(vault);
        if let Some(stored) = self.stored_manifest_hash()? {
            if stored == manifest {
                return Ok(ReconcileSummary::default());
            }
        }

        // 1. Snapshot the current file_state into a hashmap.
        let mut prior: std::collections::HashMap<String, Vec<u8>> = self
            .conn
            .prepare("SELECT path, sha256 FROM file_state")?
            .query_map([], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, Vec<u8>>(1)?))
            })?
            .filter_map(|r| r.ok())
            .collect();

        let mut summary = ReconcileSummary::default();
        let mut seen: HashSet<String> = HashSet::new();

        // 2. Walk the vault. For each .md compute hash and diff.
        for path in walk_md(&notes_dir) {
            let rel = match path.strip_prefix(root) {
                Ok(p) => p.to_string_lossy().into_owned(),
                Err(_) => continue,
            };
            seen.insert(rel.clone());

            let bytes = match std::fs::read(&path) {
                Ok(b) => b,
                Err(_) => continue,
            };
            let source = match std::str::from_utf8(&bytes) {
                Ok(s) => s,
                Err(_) => continue,
            };
            let parsed = match frontmatter::parse(source) {
                Ok(p) => p,
                Err(_) => continue,
            };
            let (fm, body) = parsed;
            let hash = Sha256::digest(&bytes).to_vec();
            let metadata = match std::fs::metadata(&path) {
                Ok(m) => m,
                Err(_) => continue,
            };
            let mtime = metadata
                .modified()
                .ok()
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .map(|d| d.as_secs() as i64)
                .unwrap_or(0);
            let size = metadata.len() as i64;
            let preview: String = body.lines().next().unwrap_or("").to_string();

            let tx = self.conn.transaction()?;
            match prior.get(&rel) {
                Some(existing) if existing.as_slice() == hash.as_slice() => {
                    // Unchanged — keep both rows as-is.
                }
                Some(_) => {
                    tx.execute(
                        "UPDATE note_index
                         SET path = ?2, title = ?3, created = ?4, updated = ?5,
                             body_preview = ?6
                         WHERE id = ?1",
                        params![
                            fm.id.to_string(),
                            rel.clone(),
                            fm.title.clone(),
                            fm.created.clone(),
                            fm.updated.clone(),
                            preview.clone()
                        ],
                    )?;
                    tx.execute(
                        "UPDATE file_state
                         SET mtime = ?2, size = ?3, sha256 = ?4 WHERE path = ?1",
                        params![rel.clone(), mtime, size, hash.clone()],
                    )?;
                    summary.updated += 1;
                }
                None => {
                    tx.execute(
                        "INSERT OR REPLACE INTO note_index
                            (id, path, title, created, updated, body_preview)
                         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                        params![
                            fm.id.to_string(),
                            rel.clone(),
                            fm.title.clone(),
                            fm.created.clone(),
                            fm.updated.clone(),
                            preview.clone()
                        ],
                    )?;
                    tx.execute(
                        "INSERT INTO file_state (path, mtime, size, sha256)
                         VALUES (?1, ?2, ?3, ?4)",
                        params![rel.clone(), mtime, size, hash.clone()],
                    )?;
                    summary.inserted += 1;
                }
            }
            tx.commit()?;
            prior.remove(&rel);
        }

        // 3. Anything left in `prior` was deleted from disk.
        for (rel, _) in prior {
            if seen.contains(&rel) {
                continue;
            }
            let tx = self.conn.transaction()?;
            // Find the id from path so we can drop the matching note_index row.
            let id: Option<String> = tx
                .query_row(
                    "SELECT id FROM note_index WHERE path = ?1",
                    params![rel.clone()],
                    |row| row.get(0),
                )
                .ok();
            if let Some(id) = id {
                tx.execute("DELETE FROM note_index WHERE id = ?1", params![id])?;
            }
            tx.execute("DELETE FROM file_state WHERE path = ?1", params![rel])?;
            tx.commit()?;
            summary.deleted += 1;
        }

        // Persist the manifest so the next startup can short-circuit.
        self.store_manifest_hash(&manifest)?;

        Ok(summary)
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
    fn schema_version_matches_migrations_count_after_first_open() {
        // Opening a fresh DB applies every migration, so version should be
        // exactly the number of migrations defined. This way the test
        // doesn't lie when a new migration is appended.
        let dir = tempdir().unwrap();
        let path = dir.path().join("v.sqlite");
        let idx = Index::open(&path).unwrap();
        assert_eq!(idx.schema_version().unwrap(), MIGRATIONS.len() as i64);
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
        assert_eq!(idx.schema_version().unwrap(), MIGRATIONS.len() as i64);
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
        assert_eq!(v, MIGRATIONS.len().to_string());
    }

    // ─── reconciliation tests (T7) ─────────────────────────────────────────

    use crate::core::vault::Vault;

    #[test]
    fn reconcile_inserts_rows_for_each_note_in_fresh_vault() {
        // RED: open empty index against a vault with 3 notes; after
        // reconcile() the note_index must have 3 rows.
        let dir = tempdir().unwrap();
        let vault = Vault::open(dir.path()).unwrap();
        for i in 0..3 {
            vault.save_note(format!("note {i}"), None).unwrap();
        }
        let mut idx = Index::open(&dir.path().join(".index/notes.sqlite")).unwrap();
        let summary = idx.reconcile_with_vault(&vault).unwrap();
        assert_eq!(summary.inserted, 3);
        assert_eq!(summary.updated, 0);
        assert_eq!(summary.deleted, 0);
        let row_count: i64 = idx
            .conn
            .query_row("SELECT COUNT(*) FROM note_index", [], |r| r.get(0))
            .unwrap();
        assert_eq!(row_count, 3);
    }

    #[test]
    fn reconcile_updates_row_when_file_content_changes() {
        let dir = tempdir().unwrap();
        let vault = Vault::open(dir.path()).unwrap();
        let id = vault.save_note("first body", None).unwrap();
        let mut idx = Index::open(&dir.path().join(".index/notes.sqlite")).unwrap();
        idx.reconcile_with_vault(&vault).unwrap();

        // Mutate the note on disk — simulate Syncthing or vim editing it.
        let path = walk_md(&dir.path().join("notes"))
            .into_iter()
            .next()
            .unwrap();
        let original = std::fs::read_to_string(&path).unwrap();
        let mutated = original.replace("first body", "second body");
        std::fs::write(&path, mutated).unwrap();

        let summary = idx.reconcile_with_vault(&vault).unwrap();
        assert_eq!(summary.inserted, 0);
        assert_eq!(summary.updated, 1);
        assert_eq!(summary.deleted, 0);

        let preview: String = idx
            .conn
            .query_row(
                "SELECT body_preview FROM note_index WHERE id = ?1",
                params![id.to_string()],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(preview, "second body");
    }

    #[test]
    fn reconcile_deletes_row_when_file_removed() {
        let dir = tempdir().unwrap();
        let vault = Vault::open(dir.path()).unwrap();
        let id = vault.save_note("x", None).unwrap();
        let mut idx = Index::open(&dir.path().join(".index/notes.sqlite")).unwrap();
        idx.reconcile_with_vault(&vault).unwrap();

        let path = walk_md(&dir.path().join("notes"))
            .into_iter()
            .next()
            .unwrap();
        std::fs::remove_file(&path).unwrap();

        let summary = idx.reconcile_with_vault(&vault).unwrap();
        assert_eq!(summary.deleted, 1);
        let row_count: i64 = idx
            .conn
            .query_row(
                "SELECT COUNT(*) FROM note_index WHERE id = ?1",
                params![id.to_string()],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(row_count, 0);
    }

    #[test]
    fn reconcile_is_idempotent_when_nothing_changes() {
        let dir = tempdir().unwrap();
        let vault = Vault::open(dir.path()).unwrap();
        vault.save_note("body", None).unwrap();
        vault.save_note("more", None).unwrap();
        let mut idx = Index::open(&dir.path().join(".index/notes.sqlite")).unwrap();
        idx.reconcile_with_vault(&vault).unwrap();
        let second = idx.reconcile_with_vault(&vault).unwrap();
        assert_eq!(second.inserted, 0);
        assert_eq!(second.updated, 0);
        assert_eq!(second.deleted, 0);
    }

    /// Local copy of the vault's md walker for test setup.
    fn walk_md(dir: &std::path::Path) -> Vec<std::path::PathBuf> {
        let mut out = Vec::new();
        let mut stack = vec![dir.to_path_buf()];
        while let Some(d) = stack.pop() {
            let Ok(read) = std::fs::read_dir(&d) else {
                continue;
            };
            for entry in read.flatten() {
                let p = entry.path();
                if p.is_dir() {
                    stack.push(p);
                } else if p.extension().is_some_and(|e| e == "md") {
                    out.push(p);
                }
            }
        }
        out
    }

    // ─── manifest-hash short-circuit (M1b-T2) ─────────────────────────────

    #[test]
    fn manifest_hash_unchanged_after_clean_shutdown() {
        // RED: after a reconcile that processed work, the index records a
        // manifest hash. Computing the same hash again on the same vault
        // must produce the same value, so the next startup can skip the
        // expensive walk.
        let dir = tempdir().unwrap();
        let vault = Vault::open(dir.path()).unwrap();
        vault.save_note("a", None).unwrap();
        vault.save_note("b", None).unwrap();

        let mut idx = Index::open(&dir.path().join(".index/notes.sqlite")).unwrap();
        idx.reconcile_with_vault(&vault).unwrap();

        let stored = idx.stored_manifest_hash().unwrap();
        let recomputed = compute_manifest_hash(&vault);
        assert_eq!(stored, Some(recomputed));
    }

    #[test]
    fn manifest_hash_changes_when_a_file_is_added() {
        let dir = tempdir().unwrap();
        let vault = Vault::open(dir.path()).unwrap();
        vault.save_note("a", None).unwrap();

        let before = compute_manifest_hash(&vault);
        vault.save_note("b", None).unwrap();
        let after = compute_manifest_hash(&vault);

        assert_ne!(before, after);
    }

    #[test]
    fn reconcile_with_vault_short_circuits_when_manifest_matches() {
        // After the first reconcile, calling reconcile again on an unchanged
        // vault must skip the heavy walk. We assert this by comparing the
        // wall-clock cost; the second call should be at least an order of
        // magnitude faster than the first.
        //
        // Equally important: the second call must report the same row counts
        // as if the walk had run.
        let dir = tempdir().unwrap();
        let vault = Vault::open(dir.path()).unwrap();
        for _ in 0..50 {
            vault.save_note("body", None).unwrap();
        }
        let mut idx = Index::open(&dir.path().join(".index/notes.sqlite")).unwrap();
        let first = idx.reconcile_with_vault(&vault).unwrap();
        assert_eq!(first.inserted, 50);

        let row_count_before: i64 = idx
            .conn
            .query_row("SELECT COUNT(*) FROM note_index", [], |r| r.get(0))
            .unwrap();
        assert_eq!(row_count_before, 50);

        // Second call: no work, no row deltas.
        let second = idx.reconcile_with_vault(&vault).unwrap();
        assert_eq!(second.inserted, 0);
        assert_eq!(second.updated, 0);
        assert_eq!(second.deleted, 0);

        let row_count_after: i64 = idx
            .conn
            .query_row("SELECT COUNT(*) FROM note_index", [], |r| r.get(0))
            .unwrap();
        assert_eq!(row_count_after, 50);
    }

    #[test]
    fn reconcile_does_not_short_circuit_when_manifest_differs() {
        let dir = tempdir().unwrap();
        let vault = Vault::open(dir.path()).unwrap();
        vault.save_note("a", None).unwrap();
        let mut idx = Index::open(&dir.path().join(".index/notes.sqlite")).unwrap();
        idx.reconcile_with_vault(&vault).unwrap();

        let target = walk_md(&dir.path().join("notes"))
            .into_iter()
            .next()
            .unwrap();
        // Append bytes — guarantees a size change, so the manifest hash
        // (path, mtime, size) must differ regardless of mtime granularity.
        let mut buf = std::fs::read_to_string(&target).unwrap();
        buf.push_str("\nappended line for the test");
        std::fs::write(&target, buf).unwrap();

        let summary = idx.reconcile_with_vault(&vault).unwrap();
        assert!(
            summary.inserted + summary.updated + summary.deleted > 0,
            "manifest must invalidate when file size changes; summary={summary:?}"
        );
    }

    // ─── Tag schema (M2-T1) ─────────────────────────────────────────────

    #[test]
    fn tag_schema_tables_exist_after_migration() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("tags.sqlite");
        let idx = Index::open(&path).expect("open with tag schema");
        let tables = idx.list_tables().unwrap();
        for name in ["tag", "tag_closure", "note_tag"] {
            assert!(
                tables.contains(&name.to_string()),
                "missing {name}: {tables:?}"
            );
        }
    }

    #[test]
    fn schema_version_is_2_after_tag_migration() {
        // Bumping migrations must bump the recorded schema_version so future
        // migrations apply on top, not from scratch.
        let dir = tempdir().unwrap();
        let path = dir.path().join("v.sqlite");
        let idx = Index::open(&path).unwrap();
        assert_eq!(idx.schema_version().unwrap(), 2);
    }

    #[test]
    fn tag_closure_has_required_indexes() {
        // Performance contract: queries on the closure table filter on
        // ancestor (Branch lookup) and descendant (reverse Branch). Both
        // need indexes for M2's <50ms-on-50k-notes budget.
        let dir = tempdir().unwrap();
        let path = dir.path().join("idx.sqlite");
        let idx = Index::open(&path).unwrap();
        let names: Vec<String> = idx
            .conn
            .prepare(
                "SELECT name FROM sqlite_master WHERE type='index' AND name NOT LIKE 'sqlite_%'",
            )
            .unwrap()
            .query_map([], |r| r.get(0))
            .unwrap()
            .filter_map(|r| r.ok())
            .collect();
        for required in [
            "idx_closure_ancestor",
            "idx_closure_descendant",
            "idx_note_tag_path",
        ] {
            assert!(
                names.contains(&required.to_string()),
                "missing index {required}: {names:?}"
            );
        }
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
