// SPDX-License-Identifier: AGPL-3.0-or-later
//! Pure business logic.
//!
//! Hard rule: no module under `crate::core` may `use tauri::*`.
//! Everything here takes and returns plain types so it can be unit-tested
//! in isolation, without launching a webview.

pub mod audio;
pub mod frontmatter;
pub mod graph;
pub mod ids;
pub mod index;
pub mod links;
pub mod query;
pub mod reminder_scheduler;
pub mod reminders;
pub mod tags;
pub mod vault;
pub mod watcher;
