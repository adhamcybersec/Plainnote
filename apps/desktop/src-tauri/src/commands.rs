// SPDX-License-Identifier: AGPL-3.0-or-later
//! Thin Tauri command wrappers.
//!
//! This module is deliberately the only place that imports `tauri::*`
//! beyond the entry point. All real logic lives in `crate::core` and is
//! unit-testable without launching a webview (ADR-003).

use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};
use tauri::State;

use crate::audio_backend::CpalHost;
use crate::core::audio::{self, AudioError, CaptureSession};
use crate::core::graph;
use crate::core::ids::NoteId;
use crate::core::index::Index;
use crate::core::query::{self, QueryMode};
use crate::core::reminder_scheduler::Wake;
use crate::core::reminders::{self, ReminderFilter};
use crate::core::stt::{SttError, WhisperEngine};
use crate::core::vault::{NoteSummary, Vault};

/// Coarse user-facing state machine surfaced to the frontend via
/// `get_transcription_state` (M4-T7 RecordingIndicator polls this).
#[derive(Debug, Clone, serde::Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum RecordingState {
    Idle,
    Recording {
        /// Unix epoch ms; the UI computes elapsed locally.
        started_at_ms: u64,
    },
    Transcribing,
    Error {
        message: String,
    },
}

/// Application state shared across Tauri commands.
pub struct AppState {
    pub vault: Vault,
    pub index: Arc<Mutex<Index>>,
    /// Sender to wake the reminder scheduler when reminders are
    /// created/cancelled. Optional so tests that build AppState by hand
    /// don't need to spin up the scheduler.
    pub reminder_wake: Option<tokio::sync::mpsc::UnboundedSender<Wake>>,
    // ─── M4 voice ───
    /// Loaded whisper engine. None until the user supplies a model file at
    /// the resolved default path (or via Settings); set on first successful
    /// load. Wrapped in Mutex<Option<...>> so M4-T9's Settings UI can swap
    /// it without restarting the app.
    pub whisper: Arc<Mutex<Option<Arc<WhisperEngine>>>>,
    /// In-flight capture session. Some(session) between start_recording and
    /// stop_recording_and_transcribe. None at all other times.
    pub capture: Arc<Mutex<Option<CaptureSession>>>,
    /// Coarse user-facing state machine; the frontend polls this for the
    /// RecordingIndicator (M4-T7).
    pub rec_state: Arc<Mutex<RecordingState>>,
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
    /// First-run UX: model file is not yet at the resolved default path
    /// (M4-T11 will use this code to show a download prompt). The `message`
    /// carries the path so the prompt can render it.
    fn model_missing(path: impl Into<String>) -> Self {
        Self {
            code: "model_missing",
            message: path.into(),
        }
    }
    fn no_input_device() -> Self {
        Self {
            code: "no_input_device",
            message: "no default audio input device".into(),
        }
    }
    fn audio(message: impl Into<String>) -> Self {
        Self {
            code: "audio",
            message: message.into(),
        }
    }
    fn stt(message: impl Into<String>) -> Self {
        Self {
            code: "stt",
            message: message.into(),
        }
    }
}

fn now_ms() -> Result<u64, IpcError> {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .map_err(|_| IpcError::invalid("system clock invalid (before unix epoch)"))
}

// ─── M4 voice commands ─────────────────────────────────────────────────────

/// Begin a recording. Lazy-loads `WhisperEngine` from the default model path
/// the first time it's called (so a user without the model only hits the
/// error here, not at app startup). Captures over `CpalHost` at the host's
/// preferred config; downmix + resample happen in `stop_recording_and_transcribe`.
///
/// Capacity cap: 300 s. At 44.1 kHz × 2 ch × 4 bytes = ~105 MB pre-allocated;
/// acceptable for a voice-memo use case and far below any RAM ceiling on
/// modern desktops. See ADR-010 / M4-T6 plan note.
#[tauri::command]
pub async fn start_recording(state: State<'_, AppState>) -> Result<(), IpcError> {
    {
        let rec = state.rec_state.lock().map_err(|_| IpcError::locked())?;
        if !matches!(*rec, RecordingState::Idle | RecordingState::Error { .. }) {
            return Err(IpcError::invalid("already recording or transcribing"));
        }
    }

    // Lazy-load whisper if not yet loaded. We do this *before* opening the
    // audio device so a model_missing error never leaves a half-open stream.
    // Honors the user's override path (M4-T9) stored under meta key
    // `whisper.model_path`; empty/missing falls back to default_model_path().
    {
        let mut slot = state.whisper.lock().map_err(|_| IpcError::locked())?;
        if slot.is_none() {
            let model_path = read_model_path_override(&state)?
                .unwrap_or_else(crate::core::stt::default_model_path);
            match WhisperEngine::from_model_file(&model_path) {
                Ok(eng) => {
                    *slot = Some(Arc::new(eng));
                }
                Err(SttError::ModelMissing { path }) => {
                    return Err(IpcError::model_missing(path.to_string_lossy().into_owned()));
                }
                Err(SttError::NonUtf8ModelPath { .. }) => {
                    return Err(IpcError::invalid("model path is not valid UTF-8"));
                }
                Err(other) => return Err(IpcError::stt(other.to_string())),
            }
        }
    }

    // Hard cap: 5 minutes. See module note above.
    const MAX_SECONDS: f32 = 300.0;
    let host = CpalHost::new();
    let session = audio::start_capture(&host, MAX_SECONDS).map_err(|e| match e {
        AudioError::NoInputDevice => IpcError::no_input_device(),
        other => IpcError::audio(other.to_string()),
    })?;

    {
        let mut slot = state.capture.lock().map_err(|_| IpcError::locked())?;
        *slot = Some(session);
    }
    {
        let mut rec = state.rec_state.lock().map_err(|_| IpcError::locked())?;
        *rec = RecordingState::Recording {
            started_at_ms: now_ms()?,
        };
    }
    Ok(())
}

/// Stop the active session, downmix + resample, run whisper inference on a
/// blocking task pool (per ADR-010 / ADR-008), return the transcript.
///
/// On error, leaves `rec_state` as `Error { message }` so the UI can surface
/// it; the next `start_recording` resets to `Idle`.
#[tauri::command]
pub async fn stop_recording_and_transcribe(state: State<'_, AppState>) -> Result<String, IpcError> {
    // Confirm we're actually recording.
    {
        let rec = state.rec_state.lock().map_err(|_| IpcError::locked())?;
        if !matches!(*rec, RecordingState::Recording { .. }) {
            return Err(IpcError::invalid("not recording"));
        }
    }

    // Take the session out of state so we own it for the drain.
    let session = {
        let mut slot = state.capture.lock().map_err(|_| IpcError::locked())?;
        slot.take()
    };
    let session = session.ok_or_else(|| IpcError::invalid("no active capture session"))?;

    let drained = match session.stop_and_drain() {
        Ok(r) => r,
        Err(e) => {
            let msg = e.to_string();
            let mut rec = state.rec_state.lock().map_err(|_| IpcError::locked())?;
            *rec = RecordingState::Error {
                message: msg.clone(),
            };
            return Err(IpcError::audio(msg));
        }
    };

    // Flip to Transcribing for the duration of the inference call.
    {
        let mut rec = state.rec_state.lock().map_err(|_| IpcError::locked())?;
        *rec = RecordingState::Transcribing;
    }

    // Clone Arc<WhisperEngine> so we can move it into the blocking task.
    let engine: Arc<WhisperEngine> = {
        let slot = state.whisper.lock().map_err(|_| IpcError::locked())?;
        slot.as_ref().cloned().ok_or_else(|| {
            IpcError::stt("whisper engine missing — start_recording should have loaded it")
        })?
    };

    let pcm16k = audio::to_whisper_format(&drained.samples, &drained.config);

    // Whisper inference is CPU-heavy and synchronous; run it on the blocking
    // pool so it doesn't starve the IPC runtime. ADR-010 / ADR-008.
    let transcript_res = tauri::async_runtime::spawn_blocking(move || engine.transcribe(&pcm16k))
        .await
        .map_err(|e| IpcError::stt(format!("blocking task join error: {e}")))?;

    match transcript_res {
        Ok(text) => {
            let mut rec = state.rec_state.lock().map_err(|_| IpcError::locked())?;
            *rec = RecordingState::Idle;
            Ok(text)
        }
        Err(e) => {
            let msg = e.to_string();
            let mut rec = state.rec_state.lock().map_err(|_| IpcError::locked())?;
            *rec = RecordingState::Error {
                message: msg.clone(),
            };
            Err(IpcError::stt(msg))
        }
    }
}

/// UI poll endpoint (~100 ms while recording). Synchronous getter.
#[tauri::command]
pub fn get_transcription_state(state: State<'_, AppState>) -> Result<RecordingState, IpcError> {
    state
        .rec_state
        .lock()
        .map(|s| s.clone())
        .map_err(|_| IpcError::locked())
}

/// Meta key under which the optional whisper-model path override lives.
/// Empty string / unset = no override; fall back to `default_model_path()`.
const WHISPER_MODEL_PATH_KEY: &str = "whisper.model_path";

/// Read the override path from `meta`. Returns `Ok(None)` when unset or
/// stored as empty string; `Ok(Some(_))` otherwise. Mirrors the access
/// pattern used by `get_meta` (no dedicated Index accessor exists).
fn read_model_path_override(state: &State<'_, AppState>) -> Result<Option<PathBuf>, IpcError> {
    let idx = state.index.lock().map_err(|_| IpcError::locked())?;
    let stored: Option<String> = idx
        .conn()
        .query_row(
            "SELECT value FROM meta WHERE key = ?1",
            rusqlite::params![WHISPER_MODEL_PATH_KEY],
            |row| row.get(0),
        )
        .ok();
    Ok(stored.filter(|s| !s.is_empty()).map(PathBuf::from))
}

/// Resolved whisper-model path the engine will load on next start.
/// `is_default = true` when no override is in effect (the path is the
/// XDG-resolved default); `false` when the user set an override.
#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub struct EffectiveModelPathV1 {
    pub path: String,
    pub is_default: bool,
}

/// Returns the path the engine will load for the next recording, plus a
/// flag the UI uses to decide whether to render a "Use default" button.
#[tauri::command]
pub fn get_effective_model_path(
    state: State<'_, AppState>,
) -> Result<EffectiveModelPathV1, IpcError> {
    match read_model_path_override(&state)? {
        Some(p) => Ok(EffectiveModelPathV1 {
            path: p.to_string_lossy().into_owned(),
            is_default: false,
        }),
        None => Ok(EffectiveModelPathV1 {
            path: crate::core::stt::default_model_path()
                .to_string_lossy()
                .into_owned(),
            is_default: true,
        }),
    }
}

/// Persist (or clear) the whisper-model path override.
///
/// `path = Some(non-empty)` validates that the path points to an existing
/// regular file and stores it. `path = Some("")` or `None` clears the
/// override. On success, drops the cached `WhisperEngine` so the next
/// `start_recording` reloads from the new path.
#[tauri::command]
pub fn set_model_path(
    path: Option<String>,
    state: State<'_, AppState>,
) -> Result<EffectiveModelPathV1, IpcError> {
    let trimmed = path.as_deref().map(str::trim).filter(|s| !s.is_empty());
    let new_value: String = match trimmed {
        Some(p) => {
            let buf = PathBuf::from(p);
            if !buf.is_file() {
                return Err(IpcError::not_found(format!("not a file: {p}")));
            }
            p.to_string()
        }
        None => String::new(),
    };

    {
        let idx = state.index.lock().map_err(|_| IpcError::locked())?;
        idx.conn()
            .execute(
                "INSERT INTO meta (key, value) VALUES (?1, ?2)
                 ON CONFLICT(key) DO UPDATE SET value = excluded.value",
                rusqlite::params![WHISPER_MODEL_PATH_KEY, new_value],
            )
            .map_err(|e| IpcError::io(e.to_string()))?;
    }

    // Drop the cached engine so the next start_recording reloads.
    {
        let mut slot = state.whisper.lock().map_err(|_| IpcError::locked())?;
        *slot = None;
    }

    get_effective_model_path(state)
}

/// Vault information surfaced in Settings: absolute path to the vault
/// root and a count of indexed notes. Versioned wire type.
#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub struct VaultInfoV1 {
    pub path: String,
    pub note_count: i64,
}

/// Surface the vault root path + note count for the Settings page.
#[tauri::command]
pub fn vault_info(state: State<'_, AppState>) -> Result<VaultInfoV1, IpcError> {
    let path = state.vault.root_path().to_string_lossy().into_owned();
    let idx = state.index.lock().map_err(|_| IpcError::locked())?;
    let note_count: i64 = idx
        .conn()
        .query_row("SELECT COUNT(*) FROM note_index", [], |row| row.get(0))
        .map_err(|e| IpcError::io(e.to_string()))?;
    Ok(VaultInfoV1 { path, note_count })
}

/// Force-reindex the vault. The watcher's manifest-hash short-circuit is
/// bypassed; every .md is re-walked. Returns the reconcile summary.
#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub struct ReindexSummaryV1 {
    pub inserted: usize,
    pub updated: usize,
    pub deleted: usize,
}

#[tauri::command]
pub fn reindex_vault(state: State<'_, AppState>) -> Result<ReindexSummaryV1, IpcError> {
    let mut idx = state.index.lock().map_err(|_| IpcError::locked())?;
    let summary = idx
        .reconcile_with_vault(&state.vault)
        .map_err(|e| IpcError::io(e.to_string()))?;
    Ok(ReindexSummaryV1 {
        inserted: summary.inserted,
        updated: summary.updated,
        deleted: summary.deleted,
    })
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

/// Read a key from the persistent `meta` table. Returns `None` for unknown
/// keys so the frontend can apply its default. Used for tiny app-wide
/// preferences (e.g. the editor render-mode toggle); not for note content.
#[tauri::command]
pub fn get_meta(key: String, state: State<'_, AppState>) -> Result<Option<String>, IpcError> {
    let idx = state.index.lock().map_err(|_| IpcError::locked())?;
    let value: Option<String> = idx
        .conn()
        .query_row(
            "SELECT value FROM meta WHERE key = ?1",
            rusqlite::params![key],
            |row| row.get(0),
        )
        .ok();
    Ok(value)
}

/// Upsert a key/value pair into the `meta` table. The schema_version key is
/// reserved and rejected so a misbehaving frontend cannot corrupt migrations.
#[tauri::command]
pub fn set_meta(key: String, value: String, state: State<'_, AppState>) -> Result<(), IpcError> {
    if key == "schema_version" {
        return Err(IpcError::invalid("reserved key: schema_version"));
    }
    let idx = state.index.lock().map_err(|_| IpcError::locked())?;
    idx.conn()
        .execute(
            "INSERT INTO meta (key, value) VALUES (?1, ?2)
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            rusqlite::params![key, value],
        )
        .map_err(|e| IpcError::io(e.to_string()))?;
    Ok(())
}

/// One reminder row over the wire. Versioned.
#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub struct ReminderV1 {
    pub id: String,
    pub note_id: Option<String>,
    pub fire_at: String,
    pub fired_at: Option<String>,
    pub cancelled_at: Option<String>,
    pub body: String,
}

/// Filter for `list_reminders`. Matches the Rust enum verbatim.
#[derive(Debug, Default, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReminderFilterV1 {
    #[default]
    Active,
    Fired,
    Cancelled,
    All,
}

impl From<ReminderFilterV1> for ReminderFilter {
    fn from(f: ReminderFilterV1) -> Self {
        match f {
            ReminderFilterV1::Active => Self::Active,
            ReminderFilterV1::Fired => Self::Fired,
            ReminderFilterV1::Cancelled => Self::Cancelled,
            ReminderFilterV1::All => Self::All,
        }
    }
}

impl From<reminders::Reminder> for ReminderV1 {
    fn from(r: reminders::Reminder) -> Self {
        Self {
            id: r.id,
            note_id: r.note_id.map(|n| n.to_string()),
            fire_at: r.fire_at,
            fired_at: r.fired_at,
            cancelled_at: r.cancelled_at,
            body: r.body,
        }
    }
}

/// Schedule a reminder. `note_id` is optional; the reminder fires
/// regardless of whether the source note exists at fire_at.
#[tauri::command]
pub fn set_reminder(
    note_id: Option<String>,
    fire_at: String,
    body: String,
    state: State<'_, AppState>,
) -> Result<String, IpcError> {
    let parsed_note_id = match note_id {
        Some(s) => Some(NoteId::parse(&s).map_err(|e| IpcError::invalid(e.to_string()))?),
        None => None,
    };
    let id = {
        let idx = state.index.lock().map_err(|_| IpcError::locked())?;
        reminders::create_reminder(&idx, parsed_note_id.as_ref(), &fire_at, &body).map_err(|e| {
            match e {
                reminders::ReminderError::InvalidTimestamp(m) => IpcError::invalid(m),
                other => IpcError::io(other.to_string()),
            }
        })?
    };
    if let Some(tx) = &state.reminder_wake {
        let _ = tx.send(Wake);
    }
    Ok(id)
}

/// Mark a reminder cancelled. Idempotent: cancelling an already-cancelled
/// or fired reminder returns `not_found`.
#[tauri::command]
pub fn cancel_reminder(id: String, state: State<'_, AppState>) -> Result<(), IpcError> {
    {
        let idx = state.index.lock().map_err(|_| IpcError::locked())?;
        reminders::cancel_reminder(&idx, &id).map_err(|e| match e {
            reminders::ReminderError::NotFound(m) => IpcError::not_found(m),
            other => IpcError::io(other.to_string()),
        })?;
    }
    if let Some(tx) = &state.reminder_wake {
        let _ = tx.send(Wake);
    }
    Ok(())
}

/// List reminders, filtered. Defaults to Active.
#[tauri::command]
pub fn list_reminders(
    filter: Option<ReminderFilterV1>,
    state: State<'_, AppState>,
) -> Result<Vec<ReminderV1>, IpcError> {
    let f = filter.unwrap_or_default().into();
    let idx = state.index.lock().map_err(|_| IpcError::locked())?;
    let rows = reminders::list_reminders(&idx, f).map_err(|e| IpcError::io(e.to_string()))?;
    Ok(rows.into_iter().map(ReminderV1::from).collect())
}

/// One node in the graph view payload. Versioned wire type.
#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub struct GraphNodeV1 {
    pub id: String,
    pub title: String,
    pub size: f32,
    pub x: f32,
    pub y: f32,
}

/// One edge in the graph view payload.
#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub struct GraphEdgeV1 {
    pub source: String,
    pub target: String,
}

/// Full graph payload returned to the frontend.
#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub struct GraphDataV1 {
    pub nodes: Vec<GraphNodeV1>,
    pub edges: Vec<GraphEdgeV1>,
    /// True when the vault exceeded the 5k-node cap; the UI surfaces a
    /// "graph truncated" notice rather than rendering a soup.
    pub truncated: bool,
}

/// Build the graph (nodes + edges + force-layout coordinates) from the
/// current index. Coordinates are deterministic given the same input.
#[tauri::command]
pub fn graph_data(state: State<'_, AppState>) -> Result<GraphDataV1, IpcError> {
    let idx = state.index.lock().map_err(|_| IpcError::locked())?;
    let g = graph::build_graph(&idx).map_err(|e| IpcError::io(e.to_string()))?;
    Ok(GraphDataV1 {
        nodes: g
            .nodes
            .into_iter()
            .map(|n| GraphNodeV1 {
                id: n.id,
                title: n.title,
                size: n.size,
                x: n.x,
                y: n.y,
            })
            .collect(),
        edges: g
            .edges
            .into_iter()
            .map(|e| GraphEdgeV1 {
                source: e.source,
                target: e.target,
            })
            .collect(),
        truncated: g.truncated,
    })
}

/// One row in the wikilink autocomplete dropdown. Versioned wire type.
#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub struct TitleHitV1 {
    pub id: String,
    pub title: String,
}

/// Wikilink autocomplete: case-insensitive title prefix search.
/// Returns up to `limit` rows (capped at 50 in core::query). Empty
/// `prefix` returns an empty list — the UI must not flood the dropdown
/// with the entire vault.
#[tauri::command]
pub fn search_notes_by_title(
    prefix: String,
    limit: usize,
    state: State<'_, AppState>,
) -> Result<Vec<TitleHitV1>, IpcError> {
    let idx = state.index.lock().map_err(|_| IpcError::locked())?;
    let hits = query::search_by_title_prefix(&idx, &prefix, limit)
        .map_err(|e| IpcError::io(e.to_string()))?;
    Ok(hits
        .into_iter()
        .map(|h| TitleHitV1 {
            id: h.id.to_string(),
            title: h.title,
        })
        .collect())
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

    #[test]
    fn recording_state_serializes_with_kind_tag() {
        let s = serde_json::to_string(&RecordingState::Idle).unwrap();
        assert_eq!(s, r#"{"kind":"idle"}"#);
        let s = serde_json::to_string(&RecordingState::Recording {
            started_at_ms: 1234,
        })
        .unwrap();
        assert_eq!(s, r#"{"kind":"recording","started_at_ms":1234}"#);
        let s = serde_json::to_string(&RecordingState::Transcribing).unwrap();
        assert_eq!(s, r#"{"kind":"transcribing"}"#);
        let s = serde_json::to_string(&RecordingState::Error {
            message: "x".into(),
        })
        .unwrap();
        assert_eq!(s, r#"{"kind":"error","message":"x"}"#);
    }

    #[test]
    fn effective_model_path_v1_serializes_with_snake_case_fields() {
        // Pin the wire shape: frontend reads `is_default` (snake_case).
        let s = serde_json::to_string(&EffectiveModelPathV1 {
            path: "/x.bin".into(),
            is_default: true,
        })
        .unwrap();
        assert_eq!(s, r#"{"path":"/x.bin","is_default":true}"#);
    }

    #[test]
    fn title_hit_serializes_with_id_and_title() {
        let h = TitleHitV1 {
            id: "01HXYZ0000000000000000000A".into(),
            title: "Calculus".into(),
        };
        let json = serde_json::to_value(&h).unwrap();
        assert_eq!(json["id"], "01HXYZ0000000000000000000A");
        assert_eq!(json["title"], "Calculus");
    }
}
