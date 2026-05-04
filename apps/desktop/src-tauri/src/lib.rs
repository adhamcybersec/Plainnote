// SPDX-License-Identifier: AGPL-3.0-or-later
pub mod commands;
pub mod core;

use std::sync::Mutex;
use tauri::Manager;

use crate::commands::AppState;
use crate::core::index::Index;
use crate::core::vault::Vault;

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
            let _ = index.reconcile_with_vault(&vault);

            app.manage(AppState {
                vault,
                index: Mutex::new(index),
            });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::ping,
            commands::save_note,
            commands::list_notes,
            commands::read_note,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
