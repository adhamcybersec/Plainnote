// SPDX-License-Identifier: AGPL-3.0-or-later
pub mod commands;
pub mod core;

use std::sync::{Arc, Mutex};
use std::time::Duration;
use tauri::Manager;

use crate::commands::AppState;
use crate::core::index::Index;
use crate::core::vault::Vault;
use crate::core::watcher::Watcher;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .setup(|app| {
            // Pick the vault location: $XDG_DATA_HOME/Plainnote/vault on Linux,
            // app_data_dir() on every other platform. The user can change this
            // in Settings (M9); for M1a the path is fixed.
            let data_dir = app
                .path()
                .app_data_dir()
                .expect("no app_data_dir — should never happen on a normal desktop");
            let vault_root = data_dir.join("vault");
            let vault = Vault::open(&vault_root).expect("vault open failed");
            let index_path = vault_root.join(".index/notes.sqlite");
            let mut index = Index::open(&index_path).expect("index open failed");
            // Cold-path reconcile so external edits since last shutdown surface.
            // The manifest-hash short-circuit makes this fast on unchanged vaults.
            let _ = index.reconcile_with_vault(&vault);

            let index_arc = Arc::new(Mutex::new(index));
            app.manage(AppState {
                vault: vault.clone(),
                index: index_arc.clone(),
            });

            // Spawn the filesystem watcher. Every debounced event triggers a
            // reconcile under the index mutex. Watcher itself is owned by the
            // app handle so it lives for the program's lifetime.
            let (watcher, mut rx) = Watcher::start(vault.root_path(), Duration::from_millis(200))
                .expect("watcher start");
            app.manage(WatcherHandle(watcher));

            let vault_for_worker = vault.clone();
            let index_for_worker = index_arc.clone();
            tauri::async_runtime::spawn(async move {
                while rx.recv().await.is_some() {
                    if let Ok(mut idx) = index_for_worker.lock() {
                        let _ = idx.reconcile_with_vault(&vault_for_worker);
                    }
                }
            });

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::ping,
            commands::save_note,
            commands::list_notes,
            commands::read_note,
            commands::list_tags,
            commands::query_notes,
            commands::set_tags,
            commands::search_notes_by_title,
            commands::outbound_links_of,
            commands::backlinks_for,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

/// Holds the Watcher so it stays alive for the lifetime of the Tauri app.
/// Dropping it stops the filesystem listener.
struct WatcherHandle(#[allow(dead_code)] Watcher);
