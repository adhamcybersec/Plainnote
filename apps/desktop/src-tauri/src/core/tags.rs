// SPDX-License-Identifier: AGPL-3.0-or-later
//! Hierarchical tags + closure table maintenance.
//!
//! Tags are forward-slash-separated paths: `learning/mathematics/calculus`.
//! Inserting a leaf tag implicitly inserts every ancestor and the closure
//! relations that connect them. The closure table is what powers the
//! four-mode tag query algebra in `core::query`.

use rusqlite::params;

use crate::core::ids::NoteId;
use crate::core::index::Index;

#[derive(Debug, thiserror::Error)]
pub enum TagError {
    #[error("invalid tag path: {0}")]
    Invalid(String),
    #[error("SQLite: {0}")]
    Sqlite(#[from] rusqlite::Error),
}

/// Validate a tag path: non-empty segments, no leading/trailing slash, no
/// double slashes, no whitespace, no characters that the filesystem-style
/// path concept relies on.
fn validate_path(path: &str) -> Result<Vec<&str>, TagError> {
    if path.is_empty() {
        return Err(TagError::Invalid("empty path".into()));
    }
    if path.starts_with('/') || path.ends_with('/') || path.contains("//") {
        return Err(TagError::Invalid(format!(
            "leading/trailing/double slash: {path:?}"
        )));
    }
    let segments: Vec<&str> = path.split('/').collect();
    for seg in &segments {
        if seg.is_empty() {
            return Err(TagError::Invalid(format!("empty segment in {path:?}")));
        }
        if seg.chars().any(|c| c.is_whitespace() || c == '\0') {
            return Err(TagError::Invalid(format!(
                "whitespace or NUL in segment of {path:?}"
            )));
        }
    }
    Ok(segments)
}

/// Insert `path` and all of its ancestors into the tag tables. Idempotent.
///
/// For `learning/math/calculus` this produces:
///   * tag rows: learning, learning/math, learning/math/calculus
///   * tag_closure rows: every (ancestor, descendant, depth) where the
///     ancestor is a prefix of the descendant. Self-edges with depth 0
///     are included for each tag.
pub fn ensure_tag(index: &mut Index, path: &str) -> Result<(), TagError> {
    let segments = validate_path(path)?;
    let conn = index.conn_mut();
    let tx = conn.transaction()?;

    // Build the cumulative path list: ['learning', 'learning/math', ...]
    let mut cumulative: Vec<String> = Vec::with_capacity(segments.len());
    for i in 0..segments.len() {
        cumulative.push(segments[..=i].join("/"));
    }

    for (i, full) in cumulative.iter().enumerate() {
        let parent = if i == 0 {
            None
        } else {
            Some(cumulative[i - 1].as_str())
        };
        tx.execute(
            "INSERT INTO tag (path, parent) VALUES (?1, ?2)
             ON CONFLICT(path) DO NOTHING",
            params![full, parent],
        )?;
        // Self-edge.
        tx.execute(
            "INSERT INTO tag_closure (ancestor, descendant, depth)
             VALUES (?1, ?1, 0)
             ON CONFLICT(ancestor, descendant) DO NOTHING",
            params![full],
        )?;
        // Edges from each ancestor.
        for (j, anc) in cumulative.iter().enumerate().take(i) {
            tx.execute(
                "INSERT INTO tag_closure (ancestor, descendant, depth)
                 VALUES (?1, ?2, ?3)
                 ON CONFLICT(ancestor, descendant) DO NOTHING",
                params![anc, full, (i - j) as i64],
            )?;
        }
    }

    tx.commit()?;
    Ok(())
}

/// Associate a note with a tag. Ensures the tag (and ancestors) exist.
/// Idempotent: re-adding a tag-note pair is a no-op.
pub fn add_tag_to_note(index: &mut Index, note_id: &NoteId, path: &str) -> Result<(), TagError> {
    ensure_tag(index, path)?;
    index.conn_mut().execute(
        "INSERT INTO note_tag (note_id, tag_path) VALUES (?1, ?2)
         ON CONFLICT(note_id, tag_path) DO NOTHING",
        params![note_id.to_string(), path],
    )?;
    Ok(())
}

/// Remove a (note_id, tag_path) association if it exists. Does not delete
/// the tag itself; call `purge_orphan_tags` for that.
pub fn remove_tag_from_note(
    index: &mut Index,
    note_id: &NoteId,
    path: &str,
) -> Result<(), TagError> {
    index.conn_mut().execute(
        "DELETE FROM note_tag WHERE note_id = ?1 AND tag_path = ?2",
        params![note_id.to_string(), path],
    )?;
    Ok(())
}

/// List the tags explicitly assigned to a note (does *not* include
/// ancestors — those are derived via the closure table at query time).
pub fn list_tags_for_note(index: &Index, note_id: &NoteId) -> Result<Vec<String>, TagError> {
    let mut stmt = index
        .conn()
        .prepare("SELECT tag_path FROM note_tag WHERE note_id = ?1")?;
    let tags: Vec<String> = stmt
        .query_map(params![note_id.to_string()], |row| row.get(0))?
        .filter_map(|r| r.ok())
        .collect();
    Ok(tags)
}

/// Remove every tag that has neither notes referencing it (directly or via
/// any descendant) nor descendants in the closure table. Returns the list
/// of paths that were removed.
///
/// Loops until a pass removes nothing — that way orphan-cascading (a leaf
/// goes, then its parent has no descendants, then its parent's parent...)
/// converges in one call without recursion.
pub fn purge_orphan_tags(index: &mut Index) -> Result<Vec<String>, TagError> {
    let mut all_removed = Vec::new();
    loop {
        let candidates: Vec<String> = {
            let mut stmt = index.conn().prepare(
                "SELECT t.path FROM tag t
                 WHERE NOT EXISTS (
                     SELECT 1 FROM note_tag nt
                     JOIN tag_closure tc ON nt.tag_path = tc.descendant
                     WHERE tc.ancestor = t.path
                 )
                 AND NOT EXISTS (
                     SELECT 1 FROM tag_closure tc2
                     WHERE tc2.ancestor = t.path AND tc2.depth > 0
                 )",
            )?;
            let rows: Vec<String> = stmt
                .query_map([], |row| row.get::<_, String>(0))?
                .filter_map(|r| r.ok())
                .collect();
            rows
        };
        if candidates.is_empty() {
            break;
        }
        let tx = index.conn_mut().transaction()?;
        for path in &candidates {
            tx.execute(
                "DELETE FROM tag_closure WHERE ancestor = ?1 OR descendant = ?1",
                params![path],
            )?;
            tx.execute("DELETE FROM tag WHERE path = ?1", params![path])?;
        }
        tx.commit()?;
        all_removed.extend(candidates);
    }
    Ok(all_removed)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn open_index_with_tag_schema() -> (tempfile::TempDir, crate::core::index::Index) {
        let dir = tempdir().unwrap();
        let idx = crate::core::index::Index::open(&dir.path().join("idx.sqlite")).unwrap();
        (dir, idx)
    }

    fn closure_rows(idx: &crate::core::index::Index) -> Vec<(String, String, i64)> {
        idx.conn()
            .prepare("SELECT ancestor, descendant, depth FROM tag_closure ORDER BY ancestor, depth")
            .unwrap()
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
            .unwrap()
            .filter_map(|r| r.ok())
            .collect()
    }

    #[test]
    fn ensure_tag_creates_tag_for_each_ancestor() {
        let (_dir, mut idx) = open_index_with_tag_schema();
        ensure_tag(&mut idx, "learning/math/calculus").unwrap();

        let tags: Vec<String> = idx
            .conn()
            .prepare("SELECT path FROM tag ORDER BY path")
            .unwrap()
            .query_map([], |r| r.get(0))
            .unwrap()
            .filter_map(|r| r.ok())
            .collect();
        assert_eq!(
            tags,
            vec![
                "learning".to_string(),
                "learning/math".to_string(),
                "learning/math/calculus".to_string(),
            ]
        );
    }

    #[test]
    fn ensure_tag_creates_full_closure() {
        // For a 3-level tag:
        //   self-edges (depth 0):       3
        //   from learning:              2 (math, math/calculus)
        //   from learning/math:         1 (math/calculus)
        // total: 6
        let (_dir, mut idx) = open_index_with_tag_schema();
        ensure_tag(&mut idx, "learning/math/calculus").unwrap();

        let rows = closure_rows(&idx);
        let expected: Vec<(String, String, i64)> = vec![
            ("learning".into(), "learning".into(), 0),
            ("learning".into(), "learning/math".into(), 1),
            ("learning".into(), "learning/math/calculus".into(), 2),
            ("learning/math".into(), "learning/math".into(), 0),
            ("learning/math".into(), "learning/math/calculus".into(), 1),
            (
                "learning/math/calculus".into(),
                "learning/math/calculus".into(),
                0,
            ),
        ];
        assert_eq!(rows, expected);
    }

    #[test]
    fn ensure_tag_records_parent_pointer() {
        let (_dir, mut idx) = open_index_with_tag_schema();
        ensure_tag(&mut idx, "learning/math/calculus").unwrap();

        let parent_of_calculus: Option<String> = idx
            .conn()
            .query_row(
                "SELECT parent FROM tag WHERE path = 'learning/math/calculus'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(parent_of_calculus.as_deref(), Some("learning/math"));

        let parent_of_root: Option<String> = idx
            .conn()
            .query_row("SELECT parent FROM tag WHERE path = 'learning'", [], |r| {
                r.get(0)
            })
            .unwrap();
        assert_eq!(parent_of_root, None);
    }

    #[test]
    fn ensure_tag_is_idempotent_on_re_insert() {
        // Re-inserting the same tag must not duplicate rows or fail.
        let (_dir, mut idx) = open_index_with_tag_schema();
        ensure_tag(&mut idx, "learning/math").unwrap();
        ensure_tag(&mut idx, "learning/math").unwrap();
        ensure_tag(&mut idx, "learning/math").unwrap();

        let count: i64 = idx
            .conn()
            .query_row("SELECT COUNT(*) FROM tag", [], |r| r.get(0))
            .unwrap();
        assert_eq!(count, 2); // learning + learning/math

        let closure_count: i64 = idx
            .conn()
            .query_row("SELECT COUNT(*) FROM tag_closure", [], |r| r.get(0))
            .unwrap();
        assert_eq!(closure_count, 3); // 2 self + 1 (learning, learning/math)
    }

    #[test]
    fn ensure_tag_handles_overlapping_branches() {
        // Inserting two paths that share a prefix must not double-create the
        // shared ancestor rows.
        let (_dir, mut idx) = open_index_with_tag_schema();
        ensure_tag(&mut idx, "learning/math/calculus").unwrap();
        ensure_tag(&mut idx, "learning/math/algebra").unwrap();

        let tags: Vec<String> = idx
            .conn()
            .prepare("SELECT path FROM tag ORDER BY path")
            .unwrap()
            .query_map([], |r| r.get(0))
            .unwrap()
            .filter_map(|r| r.ok())
            .collect();
        assert_eq!(
            tags,
            vec![
                "learning".to_string(),
                "learning/math".to_string(),
                "learning/math/algebra".to_string(),
                "learning/math/calculus".to_string(),
            ]
        );
    }

    #[test]
    fn ensure_tag_rejects_invalid_paths() {
        let (_dir, mut idx) = open_index_with_tag_schema();
        for bad in [
            "",
            "/leading",
            "trailing/",
            "double//slash",
            "with spaces",
            "with\ttab",
            "with\0nul",
            "/",
        ] {
            let result = ensure_tag(&mut idx, bad);
            assert!(
                matches!(result, Err(TagError::Invalid(_))),
                "must reject {bad:?}, got {result:?}"
            );
        }
    }

    #[test]
    fn ensure_tag_handles_single_segment() {
        // A bare top-level tag like 'inbox' is also valid.
        let (_dir, mut idx) = open_index_with_tag_schema();
        ensure_tag(&mut idx, "inbox").unwrap();

        let tags: Vec<String> = idx
            .conn()
            .prepare("SELECT path FROM tag")
            .unwrap()
            .query_map([], |r| r.get(0))
            .unwrap()
            .filter_map(|r| r.ok())
            .collect();
        assert_eq!(tags, vec!["inbox".to_string()]);

        // Self-edge only.
        let closure: Vec<(String, String, i64)> = closure_rows(&idx);
        assert_eq!(closure, vec![("inbox".into(), "inbox".into(), 0)]);
    }

    // ─── note-tag association + delete (M2-T3) ─────────────────────────

    use crate::core::ids::NoteId;

    #[test]
    fn add_tag_to_note_creates_association_and_ensures_tag() {
        let (_dir, mut idx) = open_index_with_tag_schema();
        let id = NoteId::new();
        add_tag_to_note(&mut idx, &id, "learning/math").unwrap();

        let tag_count: i64 = idx
            .conn()
            .query_row("SELECT COUNT(*) FROM tag", [], |r| r.get(0))
            .unwrap();
        assert_eq!(tag_count, 2); // learning + learning/math

        let assoc_count: i64 = idx
            .conn()
            .query_row("SELECT COUNT(*) FROM note_tag", [], |r| r.get(0))
            .unwrap();
        assert_eq!(assoc_count, 1); // only the leaf is associated; ancestors come via closure
    }

    #[test]
    fn add_tag_to_note_is_idempotent() {
        let (_dir, mut idx) = open_index_with_tag_schema();
        let id = NoteId::new();
        add_tag_to_note(&mut idx, &id, "math").unwrap();
        add_tag_to_note(&mut idx, &id, "math").unwrap();

        let count: i64 = idx
            .conn()
            .query_row("SELECT COUNT(*) FROM note_tag", [], |r| r.get(0))
            .unwrap();
        assert_eq!(count, 1);
    }

    #[test]
    fn list_tags_for_note_returns_only_explicitly_assigned() {
        let (_dir, mut idx) = open_index_with_tag_schema();
        let id = NoteId::new();
        add_tag_to_note(&mut idx, &id, "learning/math/calculus").unwrap();
        add_tag_to_note(&mut idx, &id, "work/projectTTK").unwrap();

        let mut tags = list_tags_for_note(&idx, &id).unwrap();
        tags.sort();
        assert_eq!(
            tags,
            vec![
                "learning/math/calculus".to_string(),
                "work/projectTTK".to_string()
            ]
        );
    }

    #[test]
    fn remove_tag_from_note_drops_only_that_association() {
        let (_dir, mut idx) = open_index_with_tag_schema();
        let id = NoteId::new();
        add_tag_to_note(&mut idx, &id, "math").unwrap();
        add_tag_to_note(&mut idx, &id, "physics").unwrap();
        remove_tag_from_note(&mut idx, &id, "math").unwrap();

        let tags = list_tags_for_note(&idx, &id).unwrap();
        assert_eq!(tags, vec!["physics".to_string()]);
    }

    #[test]
    fn purge_orphan_tags_removes_unreferenced_leaves() {
        // After every note that referenced a tag is gone, the tag should be
        // removable. But shared ancestors must remain if other tags need them.
        let (_dir, mut idx) = open_index_with_tag_schema();
        let id = NoteId::new();
        add_tag_to_note(&mut idx, &id, "learning/math/calculus").unwrap();
        add_tag_to_note(&mut idx, &id, "learning/math/algebra").unwrap();

        // Detach calculus from the note.
        remove_tag_from_note(&mut idx, &id, "learning/math/calculus").unwrap();
        let removed = purge_orphan_tags(&mut idx).unwrap();
        // Only `learning/math/calculus` is orphaned (no notes, no descendants).
        // `learning` and `learning/math` are still in use via algebra.
        assert_eq!(removed, vec!["learning/math/calculus".to_string()]);

        // Now detach algebra.
        remove_tag_from_note(&mut idx, &id, "learning/math/algebra").unwrap();
        let removed2 = purge_orphan_tags(&mut idx).unwrap();
        let mut sorted = removed2;
        sorted.sort();
        assert_eq!(
            sorted,
            vec![
                "learning".to_string(),
                "learning/math".to_string(),
                "learning/math/algebra".to_string(),
            ]
        );
    }

    #[test]
    fn purge_keeps_ancestor_when_a_sibling_branch_still_has_notes() {
        // Two siblings under the same ancestor. Detach one note's tag; the
        // other sibling still anchors the ancestor through its own note.
        let (_dir, mut idx) = open_index_with_tag_schema();
        let id_a = NoteId::new();
        let id_b = NoteId::new();
        add_tag_to_note(&mut idx, &id_a, "learning/math/calculus").unwrap();
        add_tag_to_note(&mut idx, &id_b, "learning/math/algebra").unwrap();

        // Untag note A from calculus.
        remove_tag_from_note(&mut idx, &id_a, "learning/math/calculus").unwrap();
        let removed = purge_orphan_tags(&mut idx).unwrap();

        assert_eq!(removed, vec!["learning/math/calculus".to_string()]);
        // learning/math must remain — algebra still descends from it.
        let remaining: Vec<String> = idx
            .conn()
            .prepare("SELECT path FROM tag ORDER BY path")
            .unwrap()
            .query_map([], |r| r.get(0))
            .unwrap()
            .filter_map(|r| r.ok())
            .collect();
        assert!(remaining.contains(&"learning/math".to_string()));
        assert!(remaining.contains(&"learning".to_string()));
        assert!(remaining.contains(&"learning/math/algebra".to_string()));
    }

    use proptest::prelude::*;
    proptest! {
        #[test]
        fn property_inserting_then_reinserting_is_idempotent(
            // Generate plausibly-shaped tag paths: 1-4 segments, alphanumeric.
            depth in 1usize..5,
            // 1..=10 letters per segment; comma to separate segments via map below
            seg_lens in prop::collection::vec(1usize..10, 1..5),
        ) {
            let (_dir, mut idx) = open_index_with_tag_schema();
            // Build a path from seg_lens, capped to `depth`.
            let segments: Vec<String> = seg_lens.iter().take(depth).enumerate()
                .map(|(i, &n)| format!("s{}{}", i, "a".repeat(n.min(9))))
                .collect();
            let path = segments.join("/");

            ensure_tag(&mut idx, &path).unwrap();
            let row_count_first: i64 = idx.conn()
                .query_row("SELECT COUNT(*) FROM tag", [], |r| r.get(0)).unwrap();
            let closure_count_first: i64 = idx.conn()
                .query_row("SELECT COUNT(*) FROM tag_closure", [], |r| r.get(0)).unwrap();

            ensure_tag(&mut idx, &path).unwrap();
            let row_count_second: i64 = idx.conn()
                .query_row("SELECT COUNT(*) FROM tag", [], |r| r.get(0)).unwrap();
            let closure_count_second: i64 = idx.conn()
                .query_row("SELECT COUNT(*) FROM tag_closure", [], |r| r.get(0)).unwrap();

            prop_assert_eq!(row_count_first, row_count_second);
            prop_assert_eq!(closure_count_first, closure_count_second);
        }
    }
}
