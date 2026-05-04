// SPDX-License-Identifier: AGPL-3.0-or-later
//! Pure business logic.
//!
//! Hard rule: no module under `crate::core` may `use tauri::*`.
//! Everything here takes and returns plain types so it can be unit-tested
//! in isolation, without launching a webview.

pub mod frontmatter;
pub mod ids;
pub mod vault;
