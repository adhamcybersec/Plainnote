// SPDX-License-Identifier: AGPL-3.0-or-later
//! Thin Tauri command wrappers.
//!
//! This module is deliberately the only place that imports `tauri::*`
//! beyond the entry point. All real logic lives in `crate::core` and is
//! unit-testable without launching a webview (ADR-003).

use std::sync::{Arc, Mutex};
use tauri::State;

use crate::core::ids::NoteId;
use crate::core::index::Index;
use crate::core::vault::{NoteSummary, Vault};

/// Application state shared across Tauri commands.
pub struct AppState {
    pub vault: Vault,
    pub index: Arc<Mutex<Index>>,
}

/// Versioned wire types. Bumping a struct version is part of the IPC
/// contract; never silently change the shape.
#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub struct NoteSummaryV1 {
    pub id: String,
    pub title: Option<String>,
    pub created: String,
    pub updated: String,
    pub preview: String,
}

impl From<NoteSummary> for NoteSummaryV1 {
    fn from(s: NoteSummary) -> Self {
        Self {
            id: s.id.to_string(),
            title: s.title,
            created: s.created,
            updated: s.updated,
            preview: s.preview,
        }
    }
}

#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub struct NoteV1 {
    pub id: String,
    pub title: Option<String>,
    pub created: String,
    pub updated: String,
    pub tags: Vec<String>,
    pub body: String,
}

/// Tagged error code for the frontend; never expose internal error strings
/// directly. The frontend can branch on `code`; `message` is for logging.
#[derive(Debug, serde::Serialize)]
pub struct IpcError {
    pub code: &'static str,
    pub message: String,
}

impl IpcError {
    fn io(message: impl Into<String>) -> Self {
        Self {
            code: "io",
            message: message.into(),
        }
    }
    fn invalid(message: impl Into<String>) -> Self {
        Self {
            code: "invalid",
            message: message.into(),
        }
    }
    fn not_found(message: impl Into<String>) -> Self {
        Self {
            code: "not_found",
            message: message.into(),
        }
    }
    fn locked() -> Self {
        Self {
            code: "locked",
            message: "could not acquire index lock".into(),
        }
    }
}

#[tauri::command]
pub fn ping() -> &'static str {
    "pong"
}

/// Persist a new note. Returns its ULID as a 26-char string.
#[tauri::command]
pub fn save_note(
    body: String,
    title: Option<String>,
    state: State<'_, AppState>,
) -> Result<String, IpcError> {
    let id = state
        .vault
        .save_note(body, title)
        .map_err(|e| IpcError::io(e.to_string()))?;
    let mut idx = state.index.lock().map_err(|_| IpcError::locked())?;
    idx.reconcile_with_vault(&state.vault)
        .map_err(|e| IpcError::io(e.to_string()))?;
    Ok(id.to_string())
}

/// List notes ordered by created date descending.
#[tauri::command]
pub fn list_notes(
    limit: usize,
    state: State<'_, AppState>,
) -> Result<Vec<NoteSummaryV1>, IpcError> {
    let summaries = state
        .vault
        .list_notes_chrono(limit)
        .map_err(|e| IpcError::io(e.to_string()))?;
    Ok(summaries.into_iter().map(NoteSummaryV1::from).collect())
}

/// Read a single note by id.
#[tauri::command]
pub fn read_note(id: String, state: State<'_, AppState>) -> Result<NoteV1, IpcError> {
    let id = NoteId::parse(&id).map_err(|e| IpcError::invalid(e.to_string()))?;
    let note = state.vault.read_note(&id).map_err(|e| {
        let msg = e.to_string();
        if msg.contains("not found") {
            IpcError::not_found(msg)
        } else {
            IpcError::io(msg)
        }
    })?;
    Ok(NoteV1 {
        id: note.frontmatter.id.to_string(),
        title: note.frontmatter.title,
        created: note.frontmatter.created,
        updated: note.frontmatter.updated,
        tags: note.frontmatter.tags,
        body: note.body,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::vault::NoteSummary;

    #[test]
    fn note_summary_v1_serializes_id_as_string() {
        // The wire contract: id is always a string in JSON, never an object.
        // Frontend code parses it as `string`; if we ever leak the inner Ulid
        // type the round-trip breaks silently.
        let summary = NoteSummary {
            id: NoteId::parse("01HXYZ0000000000000000000A").unwrap(),
            title: Some("hello".into()),
            created: "2026-05-04T10:23:11Z".into(),
            updated: "2026-05-04T10:23:11Z".into(),
            preview: "hi".into(),
        };
        let v1: NoteSummaryV1 = summary.into();
        let json = serde_json::to_value(&v1).unwrap();
        assert_eq!(json["id"], "01HXYZ0000000000000000000A");
        assert_eq!(json["title"], "hello");
        assert_eq!(json["preview"], "hi");
    }

    #[test]
    fn ipc_error_serializes_with_code_and_message() {
        let err = IpcError::not_found("nope");
        let json = serde_json::to_value(&err).unwrap();
        assert_eq!(json["code"], "not_found");
        assert_eq!(json["message"], "nope");
    }

    #[test]
    fn ping_returns_pong() {
        // Sanity: the M0 ping/pong baseline survives the M1a refactor.
        assert_eq!(ping(), "pong");
    }
}
