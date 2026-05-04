// SPDX-License-Identifier: AGPL-3.0-or-later
//! Thin Tauri command wrappers.
//!
//! This module is deliberately the only place that imports `tauri::*`.
//! Real logic lives in `crate::core` and is unit-testable without Tauri.

#[tauri::command]
pub fn ping() -> &'static str {
    "pong"
}
