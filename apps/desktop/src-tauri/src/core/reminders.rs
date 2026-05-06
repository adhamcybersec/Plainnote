// SPDX-License-Identifier: AGPL-3.0-or-later
//! Reminder CRUD layer.
//!
//! Thin SQL wrapper over the `reminder` table. The scheduler (M6-T2)
//! polls these rows; the Tauri commands (M6-T3) call into here.
//!
//! Times are stored as ISO-8601 strings with `Z` suffix — the same shape
//! used by the rest of the system (frontmatter, note_index). We never
//! store local time. The scheduler converts to absolute Instant via
//! `chrono::DateTime` arithmetic.

use crate::core::ids::NoteId;
use crate::core::index::Index;
use chrono::{DateTime, Utc};
use rusqlite::params;
use ulid::Ulid;

#[derive(Debug, thiserror::Error)]
pub enum ReminderError {
    #[error("SQLite: {0}")]
    Sqlite(#[from] rusqlite::Error),
    #[error("invalid timestamp: {0}")]
    InvalidTimestamp(String),
    #[error("not found: {0}")]
    NotFound(String),
}

/// One reminder as exposed to callers. Mirrors `ReminderV1` over the wire
/// but keeps Tauri types out of the core layer (ADR-003).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Reminder {
    pub id: String, // ULID; reminders are not NoteIds (different namespace)
    pub note_id: Option<NoteId>,
    pub fire_at: String,
    pub fired_at: Option<String>,
    pub cancelled_at: Option<String>,
    pub body: String,
}

/// State filter for `list_reminders`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReminderFilter {
    /// Active = not yet fired AND not cancelled.
    Active,
    /// Already fired (history view).
    Fired,
    /// Cancelled by the user.
    Cancelled,
    /// Everything regardless of state.
    All,
}

/// Insert a new reminder. Returns its generated ULID.
///
/// `fire_at_iso` must be an ISO-8601 string with `Z` (or `+HH:MM`) — we
/// validate by parsing and re-formatting to UTC `Z` so storage stays
/// canonical no matter what the caller submits.
pub fn create_reminder(
    index: &Index,
    note_id: Option<&NoteId>,
    fire_at_iso: &str,
    body: &str,
) -> Result<String, ReminderError> {
    let parsed: DateTime<Utc> = fire_at_iso
        .parse::<DateTime<Utc>>()
        .map_err(|e| ReminderError::InvalidTimestamp(e.to_string()))?;
    let canonical = parsed.format("%Y-%m-%dT%H:%M:%SZ").to_string();
    let id = Ulid::new().to_string();
    index.conn().execute(
        "INSERT INTO reminder (id, note_id, fire_at, body) VALUES (?1, ?2, ?3, ?4)",
        params![id, note_id.map(|n| n.to_string()), canonical, body],
    )?;
    Ok(id)
}

/// Mark a reminder as cancelled. The row is preserved so the scheduler
/// can ignore it without losing the audit trail.
pub fn cancel_reminder(index: &Index, id: &str) -> Result<(), ReminderError> {
    let now = Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string();
    let updated = index.conn().execute(
        "UPDATE reminder SET cancelled_at = ?2
         WHERE id = ?1 AND fired_at IS NULL AND cancelled_at IS NULL",
        params![id, now],
    )?;
    if updated == 0 {
        return Err(ReminderError::NotFound(id.to_string()));
    }
    Ok(())
}

/// Mark a reminder as fired. Called by the scheduler after delivery.
pub fn mark_fired(index: &Index, id: &str) -> Result<(), ReminderError> {
    let now = Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string();
    let updated = index.conn().execute(
        "UPDATE reminder SET fired_at = ?2 WHERE id = ?1 AND fired_at IS NULL",
        params![id, now],
    )?;
    if updated == 0 {
        return Err(ReminderError::NotFound(id.to_string()));
    }
    Ok(())
}

/// List reminders matching `filter`, sorted by `fire_at` ascending.
pub fn list_reminders(
    index: &Index,
    filter: ReminderFilter,
) -> Result<Vec<Reminder>, ReminderError> {
    let where_clause = match filter {
        ReminderFilter::Active => "WHERE fired_at IS NULL AND cancelled_at IS NULL",
        ReminderFilter::Fired => "WHERE fired_at IS NOT NULL",
        ReminderFilter::Cancelled => "WHERE cancelled_at IS NOT NULL",
        ReminderFilter::All => "",
    };
    let sql = format!(
        "SELECT id, note_id, fire_at, fired_at, cancelled_at, body
         FROM reminder
         {where_clause}
         ORDER BY fire_at ASC"
    );
    let mut stmt = index.conn().prepare(&sql)?;
    let rows = stmt.query_map([], |row| {
        let note_id_str: Option<String> = row.get(1)?;
        let note_id = note_id_str.and_then(|s| NoteId::parse(&s).ok());
        Ok(Reminder {
            id: row.get(0)?,
            note_id,
            fire_at: row.get(2)?,
            fired_at: row.get(3)?,
            cancelled_at: row.get(4)?,
            body: row.get(5)?,
        })
    })?;
    Ok(rows.filter_map(|r| r.ok()).collect())
}

/// Pop the next due reminder relative to `now`. Returns the reminder if
/// one is currently due (fire_at <= now), else None. Used by the scheduler
/// to determine "what should I sleep until".
///
/// This does NOT mutate state — the scheduler is responsible for calling
/// `mark_fired` after delivery succeeds. Doing the read separately means
/// a delivery failure leaves the reminder pending for the next poll.
pub fn next_due(index: &Index, now: &DateTime<Utc>) -> Result<Option<Reminder>, ReminderError> {
    let now_iso = now.format("%Y-%m-%dT%H:%M:%SZ").to_string();
    let mut stmt = index.conn().prepare(
        "SELECT id, note_id, fire_at, fired_at, cancelled_at, body
         FROM reminder
         WHERE fired_at IS NULL AND cancelled_at IS NULL AND fire_at <= ?1
         ORDER BY fire_at ASC
         LIMIT 1",
    )?;
    let mut rows = stmt.query(params![now_iso])?;
    if let Some(row) = rows.next()? {
        let note_id_str: Option<String> = row.get(1)?;
        let note_id = note_id_str.and_then(|s| NoteId::parse(&s).ok());
        Ok(Some(Reminder {
            id: row.get(0)?,
            note_id,
            fire_at: row.get(2)?,
            fired_at: row.get(3)?,
            cancelled_at: row.get(4)?,
            body: row.get(5)?,
        }))
    } else {
        Ok(None)
    }
}

/// Earliest future fire_at among active reminders. Used by the scheduler
/// to pick the next sleep deadline.
pub fn next_pending_after(
    index: &Index,
    now: &DateTime<Utc>,
) -> Result<Option<DateTime<Utc>>, ReminderError> {
    let now_iso = now.format("%Y-%m-%dT%H:%M:%SZ").to_string();
    let mut stmt = index.conn().prepare(
        "SELECT fire_at FROM reminder
         WHERE fired_at IS NULL AND cancelled_at IS NULL AND fire_at > ?1
         ORDER BY fire_at ASC
         LIMIT 1",
    )?;
    let mut rows = stmt.query(params![now_iso])?;
    if let Some(row) = rows.next()? {
        let s: String = row.get(0)?;
        let parsed: DateTime<Utc> = s
            .parse()
            .map_err(|e: chrono::ParseError| ReminderError::InvalidTimestamp(e.to_string()))?;
        Ok(Some(parsed))
    } else {
        Ok(None)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn fresh_index() -> (tempfile::TempDir, Index) {
        let dir = tempdir().unwrap();
        let idx = Index::open(&dir.path().join("notes.sqlite")).unwrap();
        (dir, idx)
    }

    #[test]
    fn create_and_list_active() {
        let (_dir, idx) = fresh_index();
        let id = create_reminder(&idx, None, "2026-06-01T10:00:00Z", "drink water").unwrap();
        let active = list_reminders(&idx, ReminderFilter::Active).unwrap();
        assert_eq!(active.len(), 1);
        assert_eq!(active[0].id, id);
        assert_eq!(active[0].body, "drink water");
        assert_eq!(active[0].fire_at, "2026-06-01T10:00:00Z");
        assert!(active[0].fired_at.is_none());
        assert!(active[0].cancelled_at.is_none());
    }

    #[test]
    fn create_canonicalizes_offset_into_z() {
        // Caller submits +03:00 but we always store UTC Z.
        let (_dir, idx) = fresh_index();
        let id = create_reminder(&idx, None, "2026-06-01T13:00:00+03:00", "x").unwrap();
        let r = list_reminders(&idx, ReminderFilter::Active).unwrap();
        assert_eq!(r[0].id, id);
        assert_eq!(r[0].fire_at, "2026-06-01T10:00:00Z");
    }

    #[test]
    fn create_rejects_bad_timestamp() {
        let (_dir, idx) = fresh_index();
        let err = create_reminder(&idx, None, "tomorrow", "x").unwrap_err();
        assert!(matches!(err, ReminderError::InvalidTimestamp(_)));
    }

    #[test]
    fn cancel_excludes_from_active_but_keeps_row() {
        let (_dir, idx) = fresh_index();
        let id = create_reminder(&idx, None, "2026-06-01T10:00:00Z", "x").unwrap();
        cancel_reminder(&idx, &id).unwrap();
        assert_eq!(
            list_reminders(&idx, ReminderFilter::Active).unwrap().len(),
            0
        );
        assert_eq!(
            list_reminders(&idx, ReminderFilter::Cancelled)
                .unwrap()
                .len(),
            1
        );
        assert_eq!(list_reminders(&idx, ReminderFilter::All).unwrap().len(), 1);
    }

    #[test]
    fn cancel_idempotent_returns_not_found_second_time() {
        let (_dir, idx) = fresh_index();
        let id = create_reminder(&idx, None, "2026-06-01T10:00:00Z", "x").unwrap();
        cancel_reminder(&idx, &id).unwrap();
        let err = cancel_reminder(&idx, &id).unwrap_err();
        assert!(matches!(err, ReminderError::NotFound(_)));
    }

    #[test]
    fn mark_fired_moves_to_fired_state() {
        let (_dir, idx) = fresh_index();
        let id = create_reminder(&idx, None, "2026-06-01T10:00:00Z", "x").unwrap();
        mark_fired(&idx, &id).unwrap();
        assert_eq!(
            list_reminders(&idx, ReminderFilter::Active).unwrap().len(),
            0
        );
        let fired = list_reminders(&idx, ReminderFilter::Fired).unwrap();
        assert_eq!(fired.len(), 1);
        assert!(fired[0].fired_at.is_some());
    }

    #[test]
    fn mark_fired_twice_errors() {
        let (_dir, idx) = fresh_index();
        let id = create_reminder(&idx, None, "2026-06-01T10:00:00Z", "x").unwrap();
        mark_fired(&idx, &id).unwrap();
        let err = mark_fired(&idx, &id).unwrap_err();
        assert!(matches!(err, ReminderError::NotFound(_)));
    }

    #[test]
    fn next_due_picks_earliest_overdue() {
        let (_dir, idx) = fresh_index();
        let _later = create_reminder(&idx, None, "2026-06-01T12:00:00Z", "later").unwrap();
        let earlier = create_reminder(&idx, None, "2026-06-01T10:00:00Z", "earlier").unwrap();
        let _future = create_reminder(&idx, None, "2030-01-01T00:00:00Z", "future").unwrap();
        let now: DateTime<Utc> = "2026-06-01T11:00:00Z".parse().unwrap();
        let due = next_due(&idx, &now).unwrap().expect("one due");
        assert_eq!(due.id, earlier);
    }

    #[test]
    fn next_due_returns_none_when_nothing_overdue() {
        let (_dir, idx) = fresh_index();
        create_reminder(&idx, None, "2030-01-01T00:00:00Z", "future").unwrap();
        let now: DateTime<Utc> = "2026-06-01T11:00:00Z".parse().unwrap();
        assert!(next_due(&idx, &now).unwrap().is_none());
    }

    #[test]
    fn next_due_skips_cancelled_and_fired() {
        let (_dir, idx) = fresh_index();
        let cancelled = create_reminder(&idx, None, "2026-06-01T10:00:00Z", "cancelled").unwrap();
        cancel_reminder(&idx, &cancelled).unwrap();
        let fired = create_reminder(&idx, None, "2026-06-01T10:00:00Z", "fired").unwrap();
        mark_fired(&idx, &fired).unwrap();
        let now: DateTime<Utc> = "2026-06-01T11:00:00Z".parse().unwrap();
        assert!(next_due(&idx, &now).unwrap().is_none());
    }

    #[test]
    fn next_pending_after_returns_earliest_future() {
        let (_dir, idx) = fresh_index();
        create_reminder(&idx, None, "2026-06-01T12:00:00Z", "a").unwrap();
        create_reminder(&idx, None, "2026-06-01T15:00:00Z", "b").unwrap();
        let now: DateTime<Utc> = "2026-06-01T11:00:00Z".parse().unwrap();
        let next = next_pending_after(&idx, &now).unwrap().unwrap();
        assert_eq!(
            next.format("%Y-%m-%dT%H:%M:%SZ").to_string(),
            "2026-06-01T12:00:00Z"
        );
    }

    #[test]
    fn list_active_orders_by_fire_at_ascending() {
        let (_dir, idx) = fresh_index();
        let later = create_reminder(&idx, None, "2026-06-02T00:00:00Z", "later").unwrap();
        let earlier = create_reminder(&idx, None, "2026-06-01T00:00:00Z", "earlier").unwrap();
        let active = list_reminders(&idx, ReminderFilter::Active).unwrap();
        assert_eq!(active[0].id, earlier);
        assert_eq!(active[1].id, later);
    }

    #[test]
    fn note_id_round_trips() {
        // The note_id round-trips through the NoteId newtype validation.
        // (Tests that we don't accidentally store a non-Crockford string.)
        let (_dir, idx) = fresh_index();
        let nid = NoteId::parse("01HXYZ0000000000000000000A").unwrap();
        let _id = create_reminder(&idx, Some(&nid), "2026-06-01T10:00:00Z", "x").unwrap();
        let active = list_reminders(&idx, ReminderFilter::Active).unwrap();
        assert_eq!(active[0].note_id, Some(nid));
    }
}
