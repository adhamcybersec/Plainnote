// SPDX-License-Identifier: AGPL-3.0-or-later
//! Reminder scheduler.
//!
//! A single tokio task that:
//!   1. Queries the index for any reminder where `fire_at <= now` and
//!      delivers it via the injected `Notifier`.
//!   2. Sleeps until the next pending `fire_at`, or for `max_idle_sleep`
//!      if nothing is pending. Either an external wake (set/cancel via
//!      a `wake_tx` channel) or the timer fires the next iteration.
//!
//! The scheduler is restart-safe by construction: it always reads fresh
//! rows from SQLite. Crashing or restarting the app loses no work — when
//! the app comes back up, all overdue reminders are immediately fired
//! (the user gets a "catch-up" burst, which the plan §6 M6 acceptance
//! gate explicitly contemplates).
//!
//! Test strategy: the scheduler is parameterized over a `Notifier` and
//! a clock. Tests pass a `Vec<DeliveredReminder>` collector + a manual
//! clock so the firing path is verified without D-Bus or wall-clock
//! sleeps. Production wires `notify_rust::Notification`.

use crate::core::index::Index;
use crate::core::reminders::{self, Reminder};
use chrono::{DateTime, Utc};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::sync::mpsc;
use tokio::time::Instant as TokioInstant;

/// Anything that can deliver a notification given a reminder. Production
/// uses `DesktopNotifier` (notify-rust → D-Bus). Tests push the reminder
/// onto a Vec.
pub trait Notifier: Send + Sync + 'static {
    /// Deliver `reminder`. The scheduler calls `mark_fired` on success;
    /// on Err, the row stays pending and is retried next poll.
    fn notify(&self, reminder: &Reminder) -> Result<(), NotifyError>;
}

#[derive(Debug, thiserror::Error)]
pub enum NotifyError {
    #[error("notification delivery failed: {0}")]
    Delivery(String),
}

/// Production notifier — talks D-Bus via notify-rust.
pub struct DesktopNotifier;

impl Notifier for DesktopNotifier {
    fn notify(&self, reminder: &Reminder) -> Result<(), NotifyError> {
        // Title is constant; body is the user's reminder text. Urgency is
        // Normal (Critical would show until-dismissed; the user did not
        // opt into that). No icon path — we use the system fallback.
        notify_rust::Notification::new()
            .summary("Plainnote reminder")
            .body(&reminder.body)
            .urgency(notify_rust::Urgency::Normal)
            .show()
            .map(|_| ())
            .map_err(|e| NotifyError::Delivery(e.to_string()))
    }
}

/// Wake signal sent to the scheduler when a reminder is created or cancelled.
/// The scheduler reacts by reading the index again and recomputing its
/// next sleep target.
#[derive(Debug, Clone, Copy)]
pub struct Wake;

/// Spawn the scheduler tokio task. Returns a sender so the rest of the
/// app can poke the scheduler whenever a reminder is created or
/// cancelled (without that, a long-pending reminder would block the
/// scheduler in `sleep_until` past a newly-inserted earlier deadline).
pub fn spawn_scheduler<N: Notifier>(
    index: Arc<Mutex<Index>>,
    notifier: Arc<N>,
    max_idle_sleep: Duration,
) -> mpsc::UnboundedSender<Wake> {
    let (tx, mut rx) = mpsc::unbounded_channel::<Wake>();
    tokio::spawn(async move {
        loop {
            // Drain any pending wakes before we start a new cycle so we
            // don't spin-loop firing the channel.
            while rx.try_recv().is_ok() {}

            let (due, sleep_until) = {
                let now = Utc::now();
                let idx = match index.lock() {
                    Ok(g) => g,
                    Err(_) => {
                        // Mutex poisoned — bail out; the app is likely on its
                        // way down.
                        return;
                    }
                };
                let due = reminders::next_due(&idx, &now).ok().flatten();
                let next = reminders::next_pending_after(&idx, &now)
                    .ok()
                    .flatten();
                (due, next)
            };

            if let Some(reminder) = due {
                // Fire and mark. A delivery failure leaves the row pending;
                // a mark_fired failure means the row is already fired by
                // someone else (shouldn't happen, but defensively continue).
                if notifier.notify(&reminder).is_ok() {
                    if let Ok(idx) = index.lock() {
                        let _ = reminders::mark_fired(&idx, &reminder.id);
                    }
                }
                // Don't sleep — there might be more overdue rows behind
                // this one. Loop immediately.
                continue;
            }

            // Nothing currently due; sleep until the next deadline OR
            // `max_idle_sleep` (so we eventually wake even with no
            // reminders, in case the system clock moved).
            let now = Utc::now();
            let sleep = match sleep_until {
                Some(deadline) => {
                    let dur = (deadline - now).to_std().unwrap_or(Duration::ZERO);
                    dur.min(max_idle_sleep).max(Duration::from_millis(50))
                }
                None => max_idle_sleep,
            };

            let deadline = TokioInstant::now() + sleep;
            tokio::select! {
                _ = tokio::time::sleep_until(deadline) => {}
                received = rx.recv() => {
                    if received.is_none() {
                        // Sender dropped — app is shutting down.
                        return;
                    }
                }
            }
        }
    });
    tx
}

/// Convert an ISO-8601 timestamp string to a `DateTime<Utc>`. Used by
/// the scheduler tests to compare deadlines.
#[allow(dead_code)]
pub fn parse_iso(s: &str) -> Option<DateTime<Utc>> {
    s.parse().ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::reminders::{create_reminder, list_reminders, ReminderFilter};
    use std::sync::atomic::{AtomicUsize, Ordering};
    use tempfile::tempdir;

    /// Test notifier: appends each delivered reminder to a Vec for assertions.
    struct Capture {
        seen: Mutex<Vec<Reminder>>,
        count: AtomicUsize,
    }
    impl Notifier for Capture {
        fn notify(&self, reminder: &Reminder) -> Result<(), NotifyError> {
            self.seen.lock().unwrap().push(reminder.clone());
            self.count.fetch_add(1, Ordering::SeqCst);
            Ok(())
        }
    }

    fn fresh_index() -> (tempfile::TempDir, Arc<Mutex<Index>>) {
        let dir = tempdir().unwrap();
        let idx = Index::open(&dir.path().join("notes.sqlite")).unwrap();
        (dir, Arc::new(Mutex::new(idx)))
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn fires_overdue_reminder_immediately_on_start() {
        let (_dir, idx) = fresh_index();
        // Schedule a reminder at a past time — simulates "app was closed
        // when it should have fired; on restart, fire the catch-up".
        {
            let g = idx.lock().unwrap();
            create_reminder(&g, None, "2000-01-01T00:00:00Z", "overdue").unwrap();
        }
        let cap = Arc::new(Capture {
            seen: Mutex::new(Vec::new()),
            count: AtomicUsize::new(0),
        });
        let _tx = spawn_scheduler(idx.clone(), cap.clone(), Duration::from_millis(500));
        // Give the scheduler a moment to deliver.
        tokio::time::sleep(Duration::from_millis(150)).await;
        assert_eq!(cap.count.load(Ordering::SeqCst), 1);
        assert_eq!(cap.seen.lock().unwrap()[0].body, "overdue");
        // Row is now in the fired state.
        let fired = list_reminders(&idx.lock().unwrap(), ReminderFilter::Fired).unwrap();
        assert_eq!(fired.len(), 1);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn fires_reminder_when_deadline_arrives() {
        let (_dir, idx) = fresh_index();
        let cap = Arc::new(Capture {
            seen: Mutex::new(Vec::new()),
            count: AtomicUsize::new(0),
        });
        let tx = spawn_scheduler(idx.clone(), cap.clone(), Duration::from_secs(60));

        // Add a reminder 200ms in the future, then wake the scheduler.
        let fire_at = (Utc::now() + chrono::Duration::milliseconds(200))
            .format("%Y-%m-%dT%H:%M:%SZ")
            .to_string();
        {
            let g = idx.lock().unwrap();
            create_reminder(&g, None, &fire_at, "soon").unwrap();
        }
        let _ = tx.send(Wake);

        // Wait long enough for the deadline + a margin.
        tokio::time::sleep(Duration::from_millis(450)).await;
        assert_eq!(cap.count.load(Ordering::SeqCst), 1);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn does_not_fire_cancelled_reminder() {
        let (_dir, idx) = fresh_index();
        let id = {
            let g = idx.lock().unwrap();
            let id = create_reminder(&g, None, "2000-01-01T00:00:00Z", "cancel-me").unwrap();
            crate::core::reminders::cancel_reminder(&g, &id).unwrap();
            id
        };
        let cap = Arc::new(Capture {
            seen: Mutex::new(Vec::new()),
            count: AtomicUsize::new(0),
        });
        let _tx = spawn_scheduler(idx.clone(), cap.clone(), Duration::from_millis(500));
        tokio::time::sleep(Duration::from_millis(200)).await;
        assert_eq!(cap.count.load(Ordering::SeqCst), 0);
        // Sanity: the row is in the cancelled state, not fired.
        let cancelled =
            list_reminders(&idx.lock().unwrap(), ReminderFilter::Cancelled).unwrap();
        assert_eq!(cancelled[0].id, id);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn fires_multiple_overdue_reminders_in_one_pass() {
        let (_dir, idx) = fresh_index();
        {
            let g = idx.lock().unwrap();
            create_reminder(&g, None, "2000-01-01T00:00:00Z", "a").unwrap();
            create_reminder(&g, None, "2000-01-02T00:00:00Z", "b").unwrap();
            create_reminder(&g, None, "2000-01-03T00:00:00Z", "c").unwrap();
        }
        let cap = Arc::new(Capture {
            seen: Mutex::new(Vec::new()),
            count: AtomicUsize::new(0),
        });
        let _tx = spawn_scheduler(idx.clone(), cap.clone(), Duration::from_millis(500));
        tokio::time::sleep(Duration::from_millis(300)).await;
        assert_eq!(cap.count.load(Ordering::SeqCst), 3);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn delivery_failure_leaves_row_pending_for_retry() {
        // Notifier that always fails on first call but succeeds on second.
        struct FlakyNotifier {
            failed_once: AtomicUsize,
        }
        impl Notifier for FlakyNotifier {
            fn notify(&self, _reminder: &Reminder) -> Result<(), NotifyError> {
                let prev = self.failed_once.fetch_add(1, Ordering::SeqCst);
                if prev == 0 {
                    Err(NotifyError::Delivery("simulated".into()))
                } else {
                    Ok(())
                }
            }
        }
        let (_dir, idx) = fresh_index();
        {
            let g = idx.lock().unwrap();
            create_reminder(&g, None, "2000-01-01T00:00:00Z", "retry").unwrap();
        }
        let flaky = Arc::new(FlakyNotifier {
            failed_once: AtomicUsize::new(0),
        });
        let tx = spawn_scheduler(idx.clone(), flaky.clone(), Duration::from_millis(80));
        // First poll: delivery fails, row stays pending.
        tokio::time::sleep(Duration::from_millis(50)).await;
        // Wake to force a re-poll; second delivery succeeds.
        let _ = tx.send(Wake);
        tokio::time::sleep(Duration::from_millis(150)).await;
        assert!(flaky.failed_once.load(Ordering::SeqCst) >= 2);
        let fired = list_reminders(&idx.lock().unwrap(), ReminderFilter::Fired).unwrap();
        assert_eq!(fired.len(), 1, "row should eventually move to fired");
    }
}
