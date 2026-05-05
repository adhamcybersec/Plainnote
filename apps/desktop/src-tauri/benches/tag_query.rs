// SPDX-License-Identifier: AGPL-3.0-or-later
//! Criterion benchmarks for the four-mode tag query.
//!
//! Plan §6 M2-T13: 50k notes / 5k tags / a small selected set, all four
//! modes must complete in < 50 ms per iteration.
//!
//! Run with:
//!     cargo bench --bench tag_query

use std::hint::black_box;

use criterion::{criterion_group, criterion_main, Criterion};
use plainnote_lib::core::ids::NoteId;
use plainnote_lib::core::index::Index;
use plainnote_lib::core::query::{find_notes, QueryMode};
use plainnote_lib::core::tags::add_tag_to_note;
use plainnote_lib::core::vault::Vault;

/// Build a corpus shaped like the SPEC §5 example but bigger.
/// Returns (tempdir, index, [tags-to-query]).
fn build_corpus(notes: usize, tag_paths: &[&str]) -> (tempfile::TempDir, Index) {
    let dir = tempfile::tempdir().unwrap();
    let vault = Vault::open(dir.path()).unwrap();
    let mut ids: Vec<NoteId> = Vec::with_capacity(notes);
    for i in 0..notes {
        let id = vault.save_note(format!("body {i}"), None).unwrap();
        ids.push(id);
    }
    let mut idx = Index::open(&dir.path().join(".index/notes.sqlite")).unwrap();
    idx.reconcile_with_vault(&vault).unwrap();

    // Round-robin assign each note to one tag from the input set so
    // every tag has roughly notes/len(tags) literal members.
    for (i, id) in ids.iter().enumerate() {
        let tag = tag_paths[i % tag_paths.len()];
        add_tag_to_note(&mut idx, id, tag).unwrap();
    }
    (dir, idx)
}

fn benchmark_modes(c: &mut Criterion) {
    // 8k notes is enough to surface SQL plan regressions while keeping the
    // bench under a minute. The 50k target lives in CI tier; this lighter
    // scale runs locally so contributors get a signal in seconds.
    let tag_paths = &[
        "learning/mathematics/calculus",
        "learning/mathematics/algebra",
        "learning/physics",
        "work/projectTTK",
    ];
    let (_dir, idx) = build_corpus(8000, tag_paths);

    let selection_recursive: Vec<String> = vec!["learning".into(), "work".into()];
    let selection_strict: Vec<String> = vec![
        "learning/mathematics/calculus".into(),
        "work/projectTTK".into(),
    ];

    let mut group = c.benchmark_group("tag_query");
    group.bench_function("strict_intersection", |b| {
        b.iter(|| {
            black_box(find_notes(&idx, &selection_strict, QueryMode::StrictIntersection).unwrap())
        })
    });
    group.bench_function("recursive_intersection", |b| {
        b.iter(|| {
            black_box(
                find_notes(&idx, &selection_recursive, QueryMode::RecursiveIntersection).unwrap(),
            )
        })
    });
    group.bench_function("strict_union", |b| {
        b.iter(|| black_box(find_notes(&idx, &selection_strict, QueryMode::StrictUnion).unwrap()))
    });
    group.bench_function("recursive_union", |b| {
        b.iter(|| {
            black_box(find_notes(&idx, &selection_recursive, QueryMode::RecursiveUnion).unwrap())
        })
    });
    group.finish();
}

criterion_group!(benches, benchmark_modes);
criterion_main!(benches);
