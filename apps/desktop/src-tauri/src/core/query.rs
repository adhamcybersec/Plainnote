// SPDX-License-Identifier: AGPL-3.0-or-later
//! Four-mode tag query algebra.
//!
//! See SPEC §5. Given a selected tag set `S = {T1, T2, ...}`:
//!   * Strict Intersection      A ∩ B          literal AND
//!   * Recursive Intersection   Branch(A) ∩ Branch(B)   ← default
//!   * Strict Union             A ∪ B          literal OR
//!   * Recursive Union          Branch(A) ∪ Branch(B)
//!
//! Each mode is an O(matches) SQL query against the closure table; no
//! recursion at query time. The closure table is populated by
//! `core::tags::ensure_tag` at insert.

use crate::core::ids::NoteId;
use crate::core::index::Index;

#[derive(Debug, thiserror::Error)]
pub enum QueryError {
    #[error("SQLite: {0}")]
    Sqlite(#[from] rusqlite::Error),
    #[error("invalid id: {0}")]
    InvalidId(String),
}

/// All four modes the query engine supports.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum QueryMode {
    StrictIntersection,
    /// SPEC default.
    #[default]
    RecursiveIntersection,
    StrictUnion,
    RecursiveUnion,
}

/// One row returned by `search_by_title_prefix`. Light enough for the
/// autocomplete dropdown — we only need the title to display and the id
/// to insert as `[[<ulid>]]` so the link is rename-stable (ADR forthcoming).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TitleHit {
    pub id: NoteId,
    pub title: String,
}

/// Case-insensitive title-prefix search. Empty prefix returns no rows
/// (autocomplete should not flood the user with the entire vault).
/// Notes with NULL title are excluded — there's nothing to match on.
/// Results are sorted alphabetically by title; `limit` capped at 50.
pub fn search_by_title_prefix(
    index: &Index,
    prefix: &str,
    limit: usize,
) -> Result<Vec<TitleHit>, QueryError> {
    if prefix.is_empty() {
        return Ok(Vec::new());
    }
    let limit = limit.min(50) as i64;
    // LIKE pattern: escape user-controlled `%`, `_`, `\` so a query for "10%"
    // matches a literal `10%` not "any 10-prefixed string".
    let escaped = prefix
        .replace('\\', "\\\\")
        .replace('%', "\\%")
        .replace('_', "\\_");
    let pattern = format!("{escaped}%");
    let mut stmt = index.conn().prepare(
        "SELECT id, title FROM note_index
         WHERE title IS NOT NULL AND title LIKE ?1 ESCAPE '\\'
         COLLATE NOCASE
         ORDER BY title COLLATE NOCASE
         LIMIT ?2",
    )?;
    let rows = stmt.query_map(rusqlite::params![pattern, limit], |row| {
        let id_str: String = row.get(0)?;
        let title: String = row.get(1)?;
        Ok((id_str, title))
    })?;
    let mut out = Vec::new();
    for r in rows {
        let (id_str, title) = r?;
        let id = NoteId::parse(&id_str).map_err(|e| QueryError::InvalidId(e.to_string()))?;
        out.push(TitleHit { id, title });
    }
    Ok(out)
}

/// Run a tag query in `mode` over the index. Returns matching note ids
/// ordered by `created` descending (newest first) so the timeline view can
/// render directly without a re-sort.
pub fn find_notes(
    index: &Index,
    tags: &[String],
    mode: QueryMode,
) -> Result<Vec<NoteId>, QueryError> {
    if tags.is_empty() {
        return Ok(Vec::new());
    }
    match mode {
        QueryMode::StrictIntersection => strict_intersection(index, tags),
        QueryMode::RecursiveIntersection => recursive_intersection(index, tags),
        QueryMode::StrictUnion => strict_union(index, tags),
        QueryMode::RecursiveUnion => recursive_union(index, tags),
    }
}

/// Notes carrying any literal tag in `tags`. Implemented as
/// `WHERE tag_path IN (...)` and DISTINCT to dedupe.
fn strict_union(index: &Index, tags: &[String]) -> Result<Vec<NoteId>, QueryError> {
    let placeholders = (1..=tags.len())
        .map(|i| format!("?{i}"))
        .collect::<Vec<_>>()
        .join(",");
    let sql = format!("SELECT DISTINCT note_id FROM note_tag WHERE tag_path IN ({placeholders})");
    let conn = index.conn();
    let mut stmt = conn.prepare(&sql)?;
    let raw_params: Vec<&dyn rusqlite::ToSql> =
        tags.iter().map(|t| t as &dyn rusqlite::ToSql).collect();
    let ids: Vec<String> = stmt
        .query_map(rusqlite::params_from_iter(raw_params), |row| row.get(0))?
        .filter_map(|r| r.ok())
        .collect();
    order_by_created(index, ids)
}

/// Notes carrying any tag in any input tag's branch (Branch(T1) ∪ Branch(T2) ∪ ...).
/// Implemented as a join through `tag_closure` with `ancestor IN (...)` and DISTINCT.
fn recursive_union(index: &Index, tags: &[String]) -> Result<Vec<NoteId>, QueryError> {
    let placeholders = (1..=tags.len())
        .map(|i| format!("?{i}"))
        .collect::<Vec<_>>()
        .join(",");
    let sql = format!(
        "SELECT DISTINCT nt.note_id
         FROM note_tag nt
         JOIN tag_closure tc ON nt.tag_path = tc.descendant
         WHERE tc.ancestor IN ({placeholders})"
    );
    let conn = index.conn();
    let mut stmt = conn.prepare(&sql)?;
    let raw_params: Vec<&dyn rusqlite::ToSql> =
        tags.iter().map(|t| t as &dyn rusqlite::ToSql).collect();
    let ids: Vec<String> = stmt
        .query_map(rusqlite::params_from_iter(raw_params), |row| row.get(0))?
        .filter_map(|r| r.ok())
        .collect();
    order_by_created(index, ids)
}

/// Notes tagged with **all** the literal tags in `tags`. Implemented as
/// `note_id IN (SELECT note_id ... ?1)` repeated, intersected via INTERSECT.
fn strict_intersection(index: &Index, tags: &[String]) -> Result<Vec<NoteId>, QueryError> {
    let mut sql = String::new();
    for (i, _) in tags.iter().enumerate() {
        if i > 0 {
            sql.push_str(" INTERSECT ");
        }
        sql.push_str(&format!(
            "SELECT note_id FROM note_tag WHERE tag_path = ?{}",
            i + 1
        ));
    }
    let conn = index.conn();
    let mut stmt = conn.prepare(&sql)?;
    let raw_params: Vec<&dyn rusqlite::ToSql> =
        tags.iter().map(|t| t as &dyn rusqlite::ToSql).collect();
    let ids: Vec<String> = stmt
        .query_map(rusqlite::params_from_iter(raw_params), |row| row.get(0))?
        .filter_map(|r| r.ok())
        .collect();
    order_by_created(index, ids)
}

/// Notes that match every input tag's branch (i.e., for each Ti the note
/// has at least one tag inside Ti's subtree). Implemented as INTERSECT of
/// per-tag (SELECT note_id FROM note_tag JOIN tag_closure ...) lookups.
fn recursive_intersection(index: &Index, tags: &[String]) -> Result<Vec<NoteId>, QueryError> {
    let mut sql = String::new();
    for (i, _) in tags.iter().enumerate() {
        if i > 0 {
            sql.push_str(" INTERSECT ");
        }
        sql.push_str(&format!(
            "SELECT nt.note_id
             FROM note_tag nt
             JOIN tag_closure tc ON nt.tag_path = tc.descendant
             WHERE tc.ancestor = ?{}",
            i + 1
        ));
    }
    let conn = index.conn();
    let mut stmt = conn.prepare(&sql)?;
    let raw_params: Vec<&dyn rusqlite::ToSql> =
        tags.iter().map(|t| t as &dyn rusqlite::ToSql).collect();
    let ids: Vec<String> = stmt
        .query_map(rusqlite::params_from_iter(raw_params), |row| row.get(0))?
        .filter_map(|r| r.ok())
        .collect();
    order_by_created(index, ids)
}

/// Sort the candidate id list by `note_index.created DESC` and parse each
/// into a `NoteId`. This is what makes the result drop straight into the
/// Library timeline.
fn order_by_created(index: &Index, ids: Vec<String>) -> Result<Vec<NoteId>, QueryError> {
    if ids.is_empty() {
        return Ok(Vec::new());
    }
    let placeholders = (1..=ids.len())
        .map(|i| format!("?{i}"))
        .collect::<Vec<_>>()
        .join(",");
    let sql =
        format!("SELECT id FROM note_index WHERE id IN ({placeholders}) ORDER BY created DESC");
    let conn = index.conn();
    let mut stmt = conn.prepare(&sql)?;
    let raw_params: Vec<&dyn rusqlite::ToSql> =
        ids.iter().map(|t| t as &dyn rusqlite::ToSql).collect();
    let rows: Vec<String> = stmt
        .query_map(rusqlite::params_from_iter(raw_params), |row| row.get(0))?
        .filter_map(|r| r.ok())
        .collect();
    rows.into_iter()
        .map(|s| NoteId::parse(&s).map_err(|e| QueryError::InvalidId(e.to_string())))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::tags::add_tag_to_note;
    use tempfile::tempdir;

    /// SPEC §5 fixture: a small multi-tag corpus that exercises every
    /// query-mode contract. Returns (tempdir, index, ids-by-label).
    ///
    /// Tags created (one note each unless noted):
    ///   1. learning/mathematics/calculus → tomasCalculusBook tag too
    ///   2. learning/mathematics/algebra
    ///   3. learning/physics
    ///   4. work/projectTTK
    ///   5. learning/mathematics/calculus + work/projectTTK   ← cross-cut
    ///   6. (untagged)
    fn fixture() -> (
        tempfile::TempDir,
        Index,
        std::collections::HashMap<&'static str, NoteId>,
    ) {
        use crate::core::vault::Vault;
        let dir = tempdir().unwrap();
        let vault = Vault::open(dir.path()).unwrap();

        let mut ids = std::collections::HashMap::new();
        let n1 = vault.save_note("body 1", None).unwrap();
        let n2 = vault.save_note("body 2", None).unwrap();
        let n3 = vault.save_note("body 3", None).unwrap();
        let n4 = vault.save_note("body 4", None).unwrap();
        let n5 = vault.save_note("body 5", None).unwrap();
        let _n6 = vault.save_note("body 6", None).unwrap();

        let mut idx = Index::open(&dir.path().join(".index/notes.sqlite")).unwrap();
        idx.reconcile_with_vault(&vault).unwrap();

        // Wire tags directly via the tag API for unambiguous test setup.
        add_tag_to_note(&mut idx, &n1, "learning/mathematics/calculus").unwrap();
        add_tag_to_note(
            &mut idx,
            &n1,
            "learning/mathematics/calculus/tomasCalculusBook",
        )
        .unwrap();
        add_tag_to_note(&mut idx, &n2, "learning/mathematics/algebra").unwrap();
        add_tag_to_note(&mut idx, &n3, "learning/physics").unwrap();
        add_tag_to_note(&mut idx, &n4, "work/projectTTK").unwrap();
        add_tag_to_note(&mut idx, &n5, "learning/mathematics/calculus").unwrap();
        add_tag_to_note(&mut idx, &n5, "work/projectTTK").unwrap();

        ids.insert("calc", n1);
        ids.insert("algebra", n2);
        ids.insert("physics", n3);
        ids.insert("ttk", n4);
        ids.insert("cross", n5);

        (dir, idx, ids)
    }

    fn as_set(v: Vec<NoteId>) -> std::collections::HashSet<String> {
        v.into_iter().map(|id| id.to_string()).collect()
    }

    // ─── Strict Intersection (M2-T5) ────────────────────────────────────

    #[test]
    fn strict_intersection_with_one_tag_returns_exact_matches() {
        let (_dir, idx, ids) = fixture();
        let result = find_notes(
            &idx,
            &["learning/mathematics/calculus".into()],
            QueryMode::StrictIntersection,
        )
        .unwrap();
        let got = as_set(result);
        let want: std::collections::HashSet<String> =
            [ids["calc"].to_string(), ids["cross"].to_string()]
                .iter()
                .cloned()
                .collect();
        assert_eq!(got, want);
    }

    #[test]
    fn strict_intersection_with_two_literal_tags_returns_only_cross_tagged() {
        // 'learning/mathematics/calculus' ∩ 'work/projectTTK' (literal):
        // only note 5 has *both* literal tags.
        let (_dir, idx, ids) = fixture();
        let result = find_notes(
            &idx,
            &[
                "learning/mathematics/calculus".into(),
                "work/projectTTK".into(),
            ],
            QueryMode::StrictIntersection,
        )
        .unwrap();
        let got = as_set(result);
        let want: std::collections::HashSet<String> =
            [ids["cross"].to_string()].iter().cloned().collect();
        assert_eq!(got, want);
    }

    #[test]
    fn strict_intersection_does_not_match_via_ancestors() {
        // 'learning/mathematics' is an *ancestor* of calculus and algebra.
        // Strict mode must *not* match notes tagged with the children.
        let (_dir, idx, _) = fixture();
        let result = find_notes(
            &idx,
            &["learning/mathematics".into()],
            QueryMode::StrictIntersection,
        )
        .unwrap();
        assert!(
            result.is_empty(),
            "ancestor literal must not pull in descendants in strict mode: {result:?}"
        );
    }

    #[test]
    fn strict_intersection_with_no_tags_returns_empty() {
        let (_dir, idx, _) = fixture();
        let result = find_notes(&idx, &[], QueryMode::StrictIntersection).unwrap();
        assert!(result.is_empty());
    }

    // ─── Recursive Intersection (M2-T6, default) ─────────────────────────

    #[test]
    fn default_query_mode_is_recursive_intersection() {
        assert_eq!(QueryMode::default(), QueryMode::RecursiveIntersection);
    }

    #[test]
    fn recursive_intersection_with_one_ancestor_tag_pulls_descendants() {
        // 'learning/mathematics' should match calc, algebra, AND cross
        // (which is also tagged calculus). Not physics (different branch),
        // not ttk (different root).
        let (_dir, idx, ids) = fixture();
        let result = find_notes(
            &idx,
            &["learning/mathematics".into()],
            QueryMode::RecursiveIntersection,
        )
        .unwrap();
        let got = as_set(result);
        let want: std::collections::HashSet<String> = [
            ids["calc"].to_string(),
            ids["algebra"].to_string(),
            ids["cross"].to_string(),
        ]
        .iter()
        .cloned()
        .collect();
        assert_eq!(got, want);
    }

    #[test]
    fn recursive_intersection_with_two_branches_picks_cross_cut() {
        // SPEC §5 example: 'learning/mathematics' ∩ 'work' (recursive).
        // The note must have *some* tag in mathematics' tree AND *some*
        // tag in work's tree. Only the cross-tagged note 5 qualifies.
        let (_dir, idx, ids) = fixture();
        let result = find_notes(
            &idx,
            &["learning/mathematics".into(), "work".into()],
            QueryMode::RecursiveIntersection,
        )
        .unwrap();
        let got = as_set(result);
        let want: std::collections::HashSet<String> =
            [ids["cross"].to_string()].iter().cloned().collect();
        assert_eq!(got, want);
    }

    #[test]
    fn recursive_intersection_with_root_branch_returns_subtree() {
        let (_dir, idx, ids) = fixture();
        let result =
            find_notes(&idx, &["learning".into()], QueryMode::RecursiveIntersection).unwrap();
        let got = as_set(result);
        // Everyone tagged anywhere under learning: calc, algebra, physics,
        // and the cross note (which has calculus). Not ttk (work-only).
        let want: std::collections::HashSet<String> = [
            ids["calc"].to_string(),
            ids["algebra"].to_string(),
            ids["physics"].to_string(),
            ids["cross"].to_string(),
        ]
        .iter()
        .cloned()
        .collect();
        assert_eq!(got, want);
    }

    #[test]
    fn recursive_intersection_with_disjoint_branches_returns_empty() {
        // 'learning/physics' ∩ 'work/projectTTK' (recursive). No note has
        // tags in both branches.
        let (_dir, idx, _) = fixture();
        let result = find_notes(
            &idx,
            &["learning/physics".into(), "work/projectTTK".into()],
            QueryMode::RecursiveIntersection,
        )
        .unwrap();
        assert!(result.is_empty());
    }

    // ─── Strict Union (M2-T7) ───────────────────────────────────────────

    #[test]
    fn strict_union_with_one_tag_matches_exact_tag_only() {
        let (_dir, idx, ids) = fixture();
        let result = find_notes(
            &idx,
            &["learning/mathematics/calculus".into()],
            QueryMode::StrictUnion,
        )
        .unwrap();
        let got = as_set(result);
        // calc + cross both literally tagged calculus.
        let want: std::collections::HashSet<String> =
            [ids["calc"].to_string(), ids["cross"].to_string()]
                .iter()
                .cloned()
                .collect();
        assert_eq!(got, want);
    }

    #[test]
    fn strict_union_with_two_tags_matches_either() {
        let (_dir, idx, ids) = fixture();
        let result = find_notes(
            &idx,
            &[
                "learning/mathematics/algebra".into(),
                "work/projectTTK".into(),
            ],
            QueryMode::StrictUnion,
        )
        .unwrap();
        let got = as_set(result);
        // algebra (note 2), ttk (note 4), cross (note 5 has projectTTK).
        let want: std::collections::HashSet<String> = [
            ids["algebra"].to_string(),
            ids["ttk"].to_string(),
            ids["cross"].to_string(),
        ]
        .iter()
        .cloned()
        .collect();
        assert_eq!(got, want);
    }

    #[test]
    fn strict_union_does_not_pull_descendants_via_ancestors() {
        // 'learning/mathematics' is no note's literal tag.
        let (_dir, idx, _) = fixture();
        let result = find_notes(
            &idx,
            &["learning/mathematics".into()],
            QueryMode::StrictUnion,
        )
        .unwrap();
        assert!(
            result.is_empty(),
            "ancestor literal must not pull descendants: {result:?}"
        );
    }

    #[test]
    fn strict_union_dedupes_results() {
        // A note tagged with two of the input tags must appear once.
        let (_dir, idx, _) = fixture();
        let result = find_notes(
            &idx,
            &[
                "learning/mathematics/calculus".into(),
                "work/projectTTK".into(),
            ],
            QueryMode::StrictUnion,
        )
        .unwrap();
        let count = result.len();
        let unique = as_set(result).len();
        assert_eq!(count, unique, "results must be deduped");
    }

    // ─── Recursive Union (M2-T7) ────────────────────────────────────────

    #[test]
    fn recursive_union_with_one_root_returns_subtree() {
        let (_dir, idx, ids) = fixture();
        let result = find_notes(&idx, &["learning".into()], QueryMode::RecursiveUnion).unwrap();
        let got = as_set(result);
        let want: std::collections::HashSet<String> = [
            ids["calc"].to_string(),
            ids["algebra"].to_string(),
            ids["physics"].to_string(),
            ids["cross"].to_string(),
        ]
        .iter()
        .cloned()
        .collect();
        assert_eq!(got, want);
    }

    #[test]
    fn recursive_union_with_two_branches_unions_subtrees() {
        // 'learning/mathematics' ∪ 'work' should pull anything in either tree.
        let (_dir, idx, ids) = fixture();
        let result = find_notes(
            &idx,
            &["learning/mathematics".into(), "work".into()],
            QueryMode::RecursiveUnion,
        )
        .unwrap();
        let got = as_set(result);
        // calc (math), algebra (math), ttk (work), cross (both).
        let want: std::collections::HashSet<String> = [
            ids["calc"].to_string(),
            ids["algebra"].to_string(),
            ids["ttk"].to_string(),
            ids["cross"].to_string(),
        ]
        .iter()
        .cloned()
        .collect();
        assert_eq!(got, want);
    }

    #[test]
    fn recursive_union_dedupes_when_branches_overlap() {
        // 'learning' ∪ 'learning/mathematics' overlaps; cross must not appear
        // twice even though both branches match it.
        let (_dir, idx, _) = fixture();
        let result = find_notes(
            &idx,
            &["learning".into(), "learning/mathematics".into()],
            QueryMode::RecursiveUnion,
        )
        .unwrap();
        let count = result.len();
        let unique = as_set(result).len();
        assert_eq!(count, unique);
    }

    // ─── single-tag equivalences (property-style sanity check) ──────────

    #[test]
    fn single_tag_strict_intersection_equals_strict_union() {
        // With exactly one tag, ∩ and ∪ in strict mode must agree.
        let (_dir, idx, _) = fixture();
        let a = find_notes(
            &idx,
            &["work/projectTTK".into()],
            QueryMode::StrictIntersection,
        )
        .unwrap();
        let b = find_notes(&idx, &["work/projectTTK".into()], QueryMode::StrictUnion).unwrap();
        assert_eq!(as_set(a), as_set(b));
    }

    #[test]
    fn single_tag_recursive_intersection_equals_recursive_union() {
        let (_dir, idx, _) = fixture();
        let a = find_notes(
            &idx,
            &["learning/mathematics".into()],
            QueryMode::RecursiveIntersection,
        )
        .unwrap();
        let b = find_notes(
            &idx,
            &["learning/mathematics".into()],
            QueryMode::RecursiveUnion,
        )
        .unwrap();
        assert_eq!(as_set(a), as_set(b));
    }

    // ─── search_by_title_prefix (M3-T5) ─────────────────────────────────

    fn title_fixture() -> (tempfile::TempDir, Index) {
        // Six notes with titles spanning case, prefix overlap, and one
        // note with title=None to confirm NULL exclusion.
        use crate::core::vault::Vault;
        let dir = tempdir().unwrap();
        let vault = Vault::open(dir.path()).unwrap();
        for t in &[
            Some("Calculus"),
            Some("calculus II"),
            Some("Cosmology"),
            Some("Quantum"),
            Some("Bayes' Theorem"),
            None, // untitled note must never appear
        ] {
            vault.save_note("body", t.map(|s| s.to_string())).unwrap();
        }
        let mut idx = Index::open(&dir.path().join(".index/notes.sqlite")).unwrap();
        idx.reconcile_with_vault(&vault).unwrap();
        (dir, idx)
    }

    #[test]
    fn empty_prefix_returns_no_rows() {
        let (_dir, idx) = title_fixture();
        let hits = search_by_title_prefix(&idx, "", 8).unwrap();
        assert!(hits.is_empty(), "got {hits:?}");
    }

    #[test]
    fn prefix_search_is_case_insensitive() {
        let (_dir, idx) = title_fixture();
        let hits = search_by_title_prefix(&idx, "cal", 8).unwrap();
        let titles: Vec<&str> = hits.iter().map(|h| h.title.as_str()).collect();
        // Both `Calculus` and `calculus II` must surface regardless of input case.
        assert!(titles.contains(&"Calculus"), "got {titles:?}");
        assert!(titles.contains(&"calculus II"), "got {titles:?}");
    }

    #[test]
    fn results_are_alphabetic_by_title() {
        // Stable order so the dropdown does not jitter across calls.
        let (_dir, idx) = title_fixture();
        let hits = search_by_title_prefix(&idx, "c", 8).unwrap();
        let titles: Vec<String> = hits.iter().map(|h| h.title.to_lowercase()).collect();
        let mut sorted = titles.clone();
        sorted.sort();
        assert_eq!(titles, sorted);
    }

    #[test]
    fn limit_caps_result_count() {
        let (_dir, idx) = title_fixture();
        let hits = search_by_title_prefix(&idx, "c", 2).unwrap();
        assert!(hits.len() <= 2, "got {} hits", hits.len());
    }

    #[test]
    fn excludes_notes_with_null_title() {
        // The NULL-title note must never contribute to autocomplete; there's
        // nothing to render and the UI would show a blank row.
        let (_dir, idx) = title_fixture();
        let hits = search_by_title_prefix(&idx, "b", 8).unwrap();
        // Only "Bayes' Theorem" matches `b`; the untitled note must not.
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].title, "Bayes' Theorem");
    }

    #[test]
    fn like_metacharacters_are_escaped() {
        // A user typing `%` or `_` must search for those literal chars,
        // not SQL wildcards.
        use crate::core::vault::Vault;
        let dir = tempdir().unwrap();
        let vault = Vault::open(dir.path()).unwrap();
        vault
            .save_note("body", Some("100% pure".to_string()))
            .unwrap();
        vault
            .save_note("body", Some("anyword".to_string()))
            .unwrap();
        let mut idx = Index::open(&dir.path().join(".index/notes.sqlite")).unwrap();
        idx.reconcile_with_vault(&vault).unwrap();
        let hits = search_by_title_prefix(&idx, "100%", 8).unwrap();
        // Without ESCAPE handling, `100%` would match anything starting with
        // `100`. With our LIKE_ESCAPE this returns only the literal-`%` row.
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].title, "100% pure");
    }

    #[test]
    fn results_are_ordered_by_created_descending() {
        // The fixture saves notes in order n1..n6 so timestamps ascend.
        // Querying must return them with the newest first.
        let (_dir, idx, _) = fixture();
        let result =
            find_notes(&idx, &["learning".into()], QueryMode::RecursiveIntersection).unwrap();
        // We just check the relative order via the sort-by-creation-time
        // contract: pull the created column for each id and assert it's
        // non-increasing.
        let conn = idx.conn();
        let mut last: Option<String> = None;
        for id in &result {
            let created: String = conn
                .query_row(
                    "SELECT created FROM note_index WHERE id = ?1",
                    rusqlite::params![id.to_string()],
                    |row| row.get(0),
                )
                .unwrap();
            if let Some(prev) = &last {
                assert!(
                    prev >= &created,
                    "results must be ordered created DESC: prev={prev:?} next={created:?}"
                );
            }
            last = Some(created);
        }
    }
}
