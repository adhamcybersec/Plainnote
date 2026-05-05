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
use crate::core::query::{self, QueryMode};
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

/// One row in the flat tag tree returned by `list_tags`. Frontend builds
/// the tree from these by joining on `parent`.
#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub struct TagRowV1 {
    pub path: String,
    pub parent: Option<String>,
    pub note_count: i64,
}

/// List every tag in the index along with its parent and the count of
/// notes whose `note_tag` row matches the tag literally.
#[tauri::command]
pub fn list_tags(state: State<'_, AppState>) -> Result<Vec<TagRowV1>, IpcError> {
    let idx = state.index.lock().map_err(|_| IpcError::locked())?;
    let mut stmt = idx
        .conn()
        .prepare(
            "SELECT t.path, t.parent,
                    (SELECT COUNT(*) FROM note_tag nt WHERE nt.tag_path = t.path) AS note_count
             FROM tag t
             ORDER BY t.path",
        )
        .map_err(|e| IpcError::io(e.to_string()))?;
    let rows = stmt
        .query_map([], |row| {
            Ok(TagRowV1 {
                path: row.get(0)?,
                parent: row.get(1)?,
                note_count: row.get(2)?,
            })
        })
        .map_err(|e| IpcError::io(e.to_string()))?;
    let out: Vec<TagRowV1> = rows.filter_map(|r| r.ok()).collect();
    Ok(out)
}

/// Run a four-mode tag query and return matching note summaries (newest
/// first). Wire mode strings: "strict_intersection", "recursive_intersection"
/// (default), "strict_union", "recursive_union".
#[tauri::command]
pub fn query_notes(
    tags: Vec<String>,
    mode: Option<QueryMode>,
    state: State<'_, AppState>,
) -> Result<Vec<NoteSummaryV1>, IpcError> {
    let mode = mode.unwrap_or_default();
    let idx = state.index.lock().map_err(|_| IpcError::locked())?;
    let ids = query::find_notes(&idx, &tags, mode).map_err(|e| IpcError::io(e.to_string()))?;
    if ids.is_empty() {
        return Ok(Vec::new());
    }

    // Build the summary list directly from note_index — cheap, doesn't read
    // .md bodies. The frontmatter for `tags` would require a vault read; we
    // return only what NoteSummary already exposes in the chronological view.
    let placeholders = (1..=ids.len())
        .map(|i| format!("?{i}"))
        .collect::<Vec<_>>()
        .join(",");
    let sql = format!(
        "SELECT id, title, created, updated, body_preview
         FROM note_index
         WHERE id IN ({placeholders})
         ORDER BY created DESC"
    );
    let mut stmt = idx
        .conn()
        .prepare(&sql)
        .map_err(|e| IpcError::io(e.to_string()))?;
    let id_strs: Vec<String> = ids.iter().map(|i| i.to_string()).collect();
    let raw_params: Vec<&dyn rusqlite::ToSql> =
        id_strs.iter().map(|s| s as &dyn rusqlite::ToSql).collect();
    let summaries: Vec<NoteSummaryV1> = stmt
        .query_map(rusqlite::params_from_iter(raw_params), |row| {
            Ok(NoteSummaryV1 {
                id: row.get(0)?,
                title: row.get(1)?,
                created: row.get(2)?,
                updated: row.get(3)?,
                preview: row.get(4)?,
            })
        })
        .map_err(|e| IpcError::io(e.to_string()))?
        .filter_map(|r| r.ok())
        .collect();
    Ok(summaries)
}

/// One outbound wikilink occurrence with its resolution state.
#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub struct LinkRefV1 {
    pub raw: String,
    pub target_text: String,
    pub alias: Option<String>,
    /// Resolved NoteId if the target exists; None for dangling links.
    pub target_id: Option<String>,
}

/// One backlink occurrence: a note that links *to* the current one.
#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub struct BacklinkV1 {
    /// The source note (the one containing the wikilink).
    pub source_id: String,
    pub source_title: Option<String>,
    pub source_preview: String,
    /// The raw `[[…]]` span in the source body.
    pub raw: String,
}

/// Outbound links from a note — what this note links to.
#[tauri::command]
pub fn outbound_links_of(
    id: String,
    state: State<'_, AppState>,
) -> Result<Vec<LinkRefV1>, IpcError> {
    let id = NoteId::parse(&id).map_err(|e| IpcError::invalid(e.to_string()))?;
    let idx = state.index.lock().map_err(|_| IpcError::locked())?;
    let mut stmt = idx
        .conn()
        .prepare(
            "SELECT raw, target_text, alias, target_id
             FROM note_link
             WHERE source = ?1
             ORDER BY raw",
        )
        .map_err(|e| IpcError::io(e.to_string()))?;
    let rows = stmt
        .query_map(rusqlite::params![id.to_string()], |row| {
            Ok(LinkRefV1 {
                raw: row.get(0)?,
                target_text: row.get(1)?,
                alias: row.get(2)?,
                target_id: row.get(3)?,
            })
        })
        .map_err(|e| IpcError::io(e.to_string()))?;
    let out: Vec<LinkRefV1> = rows.filter_map(|r| r.ok()).collect();
    Ok(out)
}

/// Notes that link to this one (resolved via target_id, so renames are
/// followed via ULID).
#[tauri::command]
pub fn backlinks_for(id: String, state: State<'_, AppState>) -> Result<Vec<BacklinkV1>, IpcError> {
    let id = NoteId::parse(&id).map_err(|e| IpcError::invalid(e.to_string()))?;
    let idx = state.index.lock().map_err(|_| IpcError::locked())?;
    let mut stmt = idx
        .conn()
        .prepare(
            "SELECT nl.source, ni.title, ni.body_preview, nl.raw
             FROM note_link nl
             JOIN note_index ni ON ni.id = nl.source
             WHERE nl.target_id = ?1
             ORDER BY ni.created DESC",
        )
        .map_err(|e| IpcError::io(e.to_string()))?;
    let rows = stmt
        .query_map(rusqlite::params![id.to_string()], |row| {
            Ok(BacklinkV1 {
                source_id: row.get(0)?,
                source_title: row.get(1)?,
                source_preview: row.get(2)?,
                raw: row.get(3)?,
            })
        })
        .map_err(|e| IpcError::io(e.to_string()))?;
    let out: Vec<BacklinkV1> = rows.filter_map(|r| r.ok()).collect();
    Ok(out)
}

/// Replace the tag set on a note. Triggers reconciliation so the index
/// catches up immediately.
#[tauri::command]
pub fn set_tags(id: String, tags: Vec<String>, state: State<'_, AppState>) -> Result<(), IpcError> {
    let id = NoteId::parse(&id).map_err(|e| IpcError::invalid(e.to_string()))?;
    state
        .vault
        .set_tags(&id, tags)
        .map_err(|e| IpcError::io(e.to_string()))?;
    let mut idx = state.index.lock().map_err(|_| IpcError::locked())?;
    idx.reconcile_with_vault(&state.vault)
        .map_err(|e| IpcError::io(e.to_string()))?;
    Ok(())
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

    #[test]
    fn tag_row_serializes_with_path_parent_count() {
        let row = TagRowV1 {
            path: "learning/math".into(),
            parent: Some("learning".into()),
            note_count: 3,
        };
        let json = serde_json::to_value(&row).unwrap();
        assert_eq!(json["path"], "learning/math");
        assert_eq!(json["parent"], "learning");
        assert_eq!(json["note_count"], 3);
    }

    #[test]
    fn tag_row_serializes_root_with_null_parent() {
        let row = TagRowV1 {
            path: "learning".into(),
            parent: None,
            note_count: 0,
        };
        let json = serde_json::to_value(&row).unwrap();
        assert!(json["parent"].is_null());
    }

    #[test]
    fn query_mode_serializes_as_snake_case() {
        // Frontend will send strings like "recursive_intersection".
        let m = QueryMode::RecursiveIntersection;
        assert_eq!(serde_json::to_value(m).unwrap(), "recursive_intersection");
        let m = QueryMode::StrictUnion;
        assert_eq!(serde_json::to_value(m).unwrap(), "strict_union");
    }

    #[test]
    fn query_mode_deserializes_from_snake_case() {
        let m: QueryMode = serde_json::from_str("\"recursive_union\"").unwrap();
        assert_eq!(m, QueryMode::RecursiveUnion);
    }

    #[test]
    fn link_ref_serializes_with_target_id_null_for_dangling() {
        let r = LinkRefV1 {
            raw: "[[Nope]]".into(),
            target_text: "Nope".into(),
            alias: None,
            target_id: None,
        };
        let json = serde_json::to_value(&r).unwrap();
        assert_eq!(json["raw"], "[[Nope]]");
        assert!(json["target_id"].is_null());
        assert!(json["alias"].is_null());
    }

    #[test]
    fn backlink_serializes_with_source_metadata() {
        let b = BacklinkV1 {
            source_id: "01HXYZ0000000000000000000A".into(),
            source_title: Some("Source".into()),
            source_preview: "preview".into(),
            raw: "[[Target]]".into(),
        };
        let json = serde_json::to_value(&b).unwrap();
        assert_eq!(json["source_id"], "01HXYZ0000000000000000000A");
        assert_eq!(json["source_title"], "Source");
        assert_eq!(json["source_preview"], "preview");
        assert_eq!(json["raw"], "[[Target]]");
    }
}
