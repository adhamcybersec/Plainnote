// SPDX-License-Identifier: AGPL-3.0-or-later
//! Pure business logic.
//!
//! Hard rule: no module under `crate::core` may `use tauri::*`.
//! Everything here takes and returns plain types so it can be unit-tested
//! in isolation, without launching a webview.
//!
//! Real modules land starting M1a (`ids`, `frontmatter`, `vault`, `index`, ...).
