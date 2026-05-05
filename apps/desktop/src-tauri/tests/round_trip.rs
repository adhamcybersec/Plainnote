// SPDX-License-Identifier: AGPL-3.0-or-later
//! M1a exit-gate integration test.
//!
//! Proves the smallest end-to-end loop the SPEC asks for:
//!     a user can capture a note, "restart the app", and find it again.
//!
//! We run this at the layer the Tauri commands target (Vault + Index)
//! rather than driving the real webview. tauri-driver-backed E2E lives
//! in M9 polish where the toolchain risk is contained.

use plainnote_lib::core::frontmatter;
use plainnote_lib::core::index::Index;
use plainnote_lib::core::vault::Vault;

use tempfile::tempdir;

#[test]
fn capture_then_restart_then_find_again() {
    let dir = tempdir().expect("tempdir");
    let vault_root = dir.path();

    // ─── Session 1: capture three notes ─────────────────────────────────
    let id1;
    let id2;
    let id3;
    {
        let vault = Vault::open(vault_root).expect("open vault");
        let index_path = vault_root.join(".index/notes.sqlite");
        let mut index = Index::open(&index_path).expect("open index");

        id1 = vault.save_note("first thought", None).expect("save 1");
        std::thread::sleep(std::time::Duration::from_millis(1100));
        id2 = vault
            .save_note("second thought with more text", Some("titled".into()))
            .expect("save 2");
        std::thread::sleep(std::time::Duration::from_millis(1100));
        id3 = vault.save_note("third", None).expect("save 3");

        let summary = index.reconcile_with_vault(&vault).expect("reconcile");
        assert_eq!(summary.inserted, 3, "fresh vault inserts 3 rows");

        // The list call backs the Library timeline view. Newest first.
        let summaries = vault.list_notes_chrono(50).expect("list");
        assert_eq!(summaries.len(), 3);
        assert_eq!(summaries[0].id, id3);
        assert_eq!(summaries[2].id, id1);
    }

    // ─── On disk: verify the user's contract — readable .md files ──────
    let mut md_files = Vec::new();
    walk_md(&vault_root.join("notes"), &mut md_files);
    assert_eq!(md_files.len(), 3);

    let first_raw = std::fs::read_to_string(&md_files[0]).expect("read md");
    assert!(first_raw.starts_with("---\n"));
    assert!(first_raw.contains("id: "));
    assert!(first_raw.contains("created: "));
    assert!(first_raw.contains("updated: "));
    let (fm, _body) = frontmatter::parse(&first_raw).expect("parse on disk");
    assert!(fm.created.ends_with('Z'));

    // ─── Session 2: simulate cold restart — drop and re-open everything
    let id1_seen;
    {
        let vault = Vault::open(vault_root).expect("re-open vault");
        let index_path = vault_root.join(".index/notes.sqlite");
        let mut index = Index::open(&index_path).expect("re-open index");

        // Reconcile must be a no-op: nothing changed since last shutdown.
        let summary = index.reconcile_with_vault(&vault).expect("reconcile 2");
        assert_eq!(summary.inserted, 0);
        assert_eq!(summary.updated, 0);
        assert_eq!(summary.deleted, 0);

        let summaries = vault.list_notes_chrono(50).expect("list 2");
        assert_eq!(summaries.len(), 3, "all three notes survive restart");

        let note = vault.read_note(&id2).expect("read by id");
        assert_eq!(note.body, "second thought with more text");
        assert_eq!(note.frontmatter.title.as_deref(), Some("titled"));

        id1_seen = summaries.iter().any(|s| s.id == id1);
    }
    assert!(id1_seen);

    // ─── Session 3: external edit (simulates Syncthing or vim) ─────────
    let target = md_files
        .iter()
        .find(|p| {
            std::fs::read_to_string(p)
                .map(|s| s.contains("first thought"))
                .unwrap_or(false)
        })
        .expect("find first note's file");
    let original = std::fs::read_to_string(target).unwrap();
    let edited = original.replace("first thought", "first thought (edited externally)");
    std::fs::write(target, edited).expect("rewrite md externally");

    {
        let vault = Vault::open(vault_root).expect("re-open vault for diff");
        let index_path = vault_root.join(".index/notes.sqlite");
        let mut index = Index::open(&index_path).expect("re-open index for diff");
        let summary = index.reconcile_with_vault(&vault).expect("reconcile 3");
        assert_eq!(summary.updated, 1, "external edit reconciles into 1 update");

        let note = vault.read_note(&id1).expect("read updated");
        assert!(note.body.contains("(edited externally)"));
    }

    // ─── Session 4: external delete ─────────────────────────────────────
    std::fs::remove_file(target).expect("rm md");
    {
        let vault = Vault::open(vault_root).expect("re-open vault for delete");
        let index_path = vault_root.join(".index/notes.sqlite");
        let mut index = Index::open(&index_path).expect("re-open index for delete");
        let summary = index.reconcile_with_vault(&vault).expect("reconcile 4");
        assert_eq!(summary.deleted, 1);

        let summaries = vault.list_notes_chrono(50).expect("list final");
        assert_eq!(summaries.len(), 2);
        assert!(!summaries.iter().any(|s| s.id == id1));
    }

    // Sanity: the third note we never touched is still findable.
    {
        let vault = Vault::open(vault_root).expect("final");
        let note = vault.read_note(&id3).expect("read id3");
        assert_eq!(note.body, "third");
    }
}

fn walk_md(dir: &std::path::Path, out: &mut Vec<std::path::PathBuf>) {
    let read = match std::fs::read_dir(dir) {
        Ok(r) => r,
        Err(_) => return,
    };
    for entry in read.flatten() {
        let p = entry.path();
        if p.is_dir() {
            walk_md(&p, out);
        } else if p.extension().is_some_and(|e| e == "md") {
            out.push(p);
        }
    }
}
