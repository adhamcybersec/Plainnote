// SPDX-License-Identifier: AGPL-3.0-or-later
//! M3 exit-gate integration test for the wikilink graph.
//!
//! Proves the full flow the plan §6 M3 describes:
//!   1. Create note A (titled).
//!   2. Create note B with a body that contains `[[<A's title>]]`.
//!   3. Open A — backlinks include B.
//!   4. Rename A. The title-form `[[<old title>]]` in B's body becomes
//!      dangling (documented in ADR-007); a ULID-form `[[<id>]]`
//!      resolves stably across the rename.
//!
//! This is the "demonstrably correct" backstop for the marquee feature.
//! Drives the same code paths the Tauri commands hit (Vault + Index +
//! reconciler) without launching a webview.

use plainnote_lib::core::frontmatter;
use plainnote_lib::core::ids::NoteId;
use plainnote_lib::core::index::{Index, LinkRow};
use plainnote_lib::core::vault::Vault;
use std::fs;
use tempfile::tempdir;

/// Helper: write a brand-new note with a known title and body, return its id.
fn save(vault: &Vault, title: &str, body: &str) -> NoteId {
    vault.save_note(body.to_string(), Some(title.to_string())).expect("save")
}

/// Helper: rename a note by rewriting its frontmatter on disk.
/// We can't use vault::set_title (no such helper yet — M3 doesn't ship one),
/// so we go through the parse/serialize pair directly. Same path the future
/// rename UI will hit.
fn rename(vault: &Vault, id: &NoteId, new_title: &str) {
    let path = find_path(vault, id);
    let source = fs::read_to_string(&path).expect("read");
    let (mut fm, body) = frontmatter::parse(&source).expect("parse");
    fm.title = Some(new_title.to_string());
    let serialized = frontmatter::write(&fm, body).expect("write");
    fs::write(&path, serialized).expect("rewrite");
}

fn find_path(vault: &Vault, id: &NoteId) -> std::path::PathBuf {
    // Walk notes/<YYYY>/<MM>/<DD>/ shallowly until we find a file whose
    // name starts with the id. Vault layout is fixed at three nested levels
    // (year/month/day) so a hand-rolled walk is enough — pulling in
    // walkdir for one test is overkill.
    fn walk(dir: &std::path::Path, id_prefix: &str) -> Option<std::path::PathBuf> {
        let entries = std::fs::read_dir(dir).ok()?;
        for entry in entries.flatten() {
            let p = entry.path();
            if p.is_dir() {
                if let Some(found) = walk(&p, id_prefix) {
                    return Some(found);
                }
            } else if p.file_name()
                .and_then(|n| n.to_str())
                .map(|n| n.starts_with(id_prefix))
                .unwrap_or(false)
            {
                return Some(p);
            }
        }
        None
    }
    let notes_dir = vault.root_path().join("notes");
    walk(&notes_dir, &id.to_string()).unwrap_or_else(|| panic!("no file for id {id}"))
}

#[test]
fn backlinks_resolve_through_title_and_id() {
    let dir = tempdir().expect("tempdir");
    let vault = Vault::open(dir.path()).expect("vault");

    // 1. Note A — the link target.
    let a = save(&vault, "Calculus", "the math note\n");

    // 2. Note B links to A by title.
    //    Note C links to A by ULID (rename-stable).
    let b_body = "see [[Calculus]] for context\n".to_string();
    let c_body = format!("see [[{a}]] also\n");
    let b = save(&vault, "References", &b_body);
    let c = save(&vault, "Cross-ref", &c_body);

    // Reconcile so wikilinks resolve into note_link.
    let mut idx = Index::open(&dir.path().join(".index/notes.sqlite")).expect("idx");
    idx.reconcile_with_vault(&vault).expect("reconcile");

    // 3. backlinks of A should contain BOTH B and C.
    let backlinks = idx.backlinks_of_note(&a).expect("backlinks query");
    let source_ids: std::collections::HashSet<&str> =
        backlinks.iter().map(|s| s.as_str()).collect();
    assert!(source_ids.contains(b.to_string().as_str()), "B missing: {source_ids:?}");
    assert!(source_ids.contains(c.to_string().as_str()), "C missing: {source_ids:?}");
}

#[test]
fn ulid_links_survive_rename_title_links_become_dangling() {
    // ADR-007: ULID-form links are rename-stable; title-form links
    // become dangling on rename (documented gap).
    let dir = tempdir().expect("tempdir");
    let vault = Vault::open(dir.path()).expect("vault");

    let a = save(&vault, "Calculus", "the math note\n");
    let b_body = "see [[Calculus]] please\n".to_string();
    let c_body = format!("see [[{a}]] please\n");
    let b = save(&vault, "References", &b_body);
    let c = save(&vault, "Cross-ref", &c_body);

    let mut idx = Index::open(&dir.path().join(".index/notes.sqlite")).expect("idx");
    idx.reconcile_with_vault(&vault).expect("reconcile");

    // Sanity: both backlinks present pre-rename.
    assert_eq!(idx.backlinks_of_note(&a).unwrap().len(), 2);

    // Rename A: "Calculus" → "Differential Calculus".
    rename(&vault, &a, "Differential Calculus");
    idx.reconcile_with_vault(&vault).expect("reconcile after rename");

    let backlinks = idx.backlinks_of_note(&a).expect("backlinks query");
    let source_ids: std::collections::HashSet<&str> =
        backlinks.iter().map(|s| s.as_str()).collect();

    // C linked by ULID — must still resolve.
    assert!(
        source_ids.contains(c.to_string().as_str()),
        "ULID-form link from C must survive rename: {source_ids:?}"
    );
    // B linked by old title — now dangling, no longer in backlinks.
    assert!(
        !source_ids.contains(b.to_string().as_str()),
        "title-form link from B should now be dangling per ADR-007: {source_ids:?}"
    );
}

#[test]
fn dangling_links_have_null_target_id() {
    // A note links to a title that doesn't exist. The reconciler must mark
    // the link dangling rather than hallucinate a target.
    let dir = tempdir().expect("tempdir");
    let vault = Vault::open(dir.path()).expect("vault");
    let _a = save(&vault, "Calculus", "real note\n");
    let b = save(&vault, "Refs", "see [[NoSuchNote]]\n");

    let mut idx = Index::open(&dir.path().join(".index/notes.sqlite")).expect("idx");
    idx.reconcile_with_vault(&vault).expect("reconcile");

    let rows: Vec<LinkRow> = idx.outbound_links_of_note(&b).expect("outbound");
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].target_text, "NoSuchNote");
    assert!(rows[0].target_id.is_none(), "dangling link must have NULL target_id");
}
