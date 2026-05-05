// SPDX-License-Identifier: AGPL-3.0-or-later
//! Filesystem watcher over the vault.
//!
//! Wraps `notify::recommended_watcher` and emits a single debounced unit
//! event per quiet window into a tokio channel. Consumers (in lib.rs)
//! spawn a worker that maps each event onto an `Index::reconcile_with_vault`
//! call. The reconciler itself is the diff engine; the watcher just decides
//! "something changed, wake up."

use notify::{Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher as NotifyWatcher};
use std::path::Path;
use std::sync::mpsc as std_mpsc;
use std::thread;
use std::time::{Duration, Instant};
use tokio::sync::mpsc;

#[derive(Debug, thiserror::Error)]
pub enum WatcherError {
    #[error("notify error: {0}")]
    Notify(#[from] notify::Error),
}

/// Holds the underlying `RecommendedWatcher` so it stays alive. Drop this
/// to stop watching.
pub struct Watcher {
    _inner: RecommendedWatcher,
    _shutdown: std_mpsc::Sender<()>,
}

impl Watcher {
    /// Begin watching `root` recursively. Events are coalesced over `debounce`
    /// and delivered as a single unit value on the returned receiver.
    pub fn start(
        root: &Path,
        debounce: Duration,
    ) -> Result<(Self, mpsc::Receiver<()>), WatcherError> {
        // Channel from notify thread → debouncer thread.
        let (raw_tx, raw_rx) = std_mpsc::channel::<()>();
        // Channel from debouncer thread → tokio consumer.
        let (out_tx, out_rx) = mpsc::channel::<()>(8);
        // Shutdown signal so the debouncer thread exits when Watcher drops.
        let (shutdown_tx, shutdown_rx) = std_mpsc::channel::<()>();

        let mut watcher = notify::recommended_watcher(move |res: notify::Result<Event>| {
            if let Ok(event) = res {
                if interesting(&event.kind) {
                    let _ = raw_tx.send(());
                }
            }
        })?;

        watcher.watch(root, RecursiveMode::Recursive)?;

        thread::spawn(move || {
            debounce_loop(raw_rx, out_tx, shutdown_rx, debounce);
        });

        Ok((
            Self {
                _inner: watcher,
                _shutdown: shutdown_tx,
            },
            out_rx,
        ))
    }
}

/// Filter out events that the index doesn't care about (e.g., access-only).
fn interesting(kind: &EventKind) -> bool {
    matches!(
        kind,
        EventKind::Create(_) | EventKind::Modify(_) | EventKind::Remove(_)
    )
}

/// Coalesce raw events into one outgoing event per quiet window.
///
/// Algorithm: on each raw event, record `last_event = now`. After every event
/// (and on a periodic tick), check whether `now - last_event >= debounce`
/// AND we have a pending event; if so, emit one and clear the pending flag.
fn debounce_loop(
    raw_rx: std_mpsc::Receiver<()>,
    out_tx: mpsc::Sender<()>,
    shutdown_rx: std_mpsc::Receiver<()>,
    debounce: Duration,
) {
    let mut pending = false;
    let mut last_event = Instant::now();
    loop {
        // Wait at most `debounce` for the next event.
        match raw_rx.recv_timeout(debounce) {
            Ok(()) => {
                pending = true;
                last_event = Instant::now();
            }
            Err(std_mpsc::RecvTimeoutError::Timeout) => {
                if pending && last_event.elapsed() >= debounce {
                    if out_tx.blocking_send(()).is_err() {
                        return;
                    }
                    pending = false;
                }
            }
            Err(std_mpsc::RecvTimeoutError::Disconnected) => return,
        }
        if shutdown_rx.try_recv().is_ok() {
            return;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;
    use tempfile::tempdir;

    #[tokio::test(flavor = "multi_thread")]
    async fn watcher_emits_event_when_file_created() {
        let dir = tempdir().unwrap();
        let (_watcher, mut rx) =
            Watcher::start(dir.path(), Duration::from_millis(50)).expect("start");

        let path = dir.path().join("hello.md");
        tokio::time::sleep(Duration::from_millis(50)).await;
        std::fs::write(&path, "x").unwrap();

        tokio::time::timeout(Duration::from_secs(2), rx.recv())
            .await
            .expect("event must arrive within 2s")
            .expect("channel must not close");
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn watcher_emits_event_when_file_modified() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("note.md");
        std::fs::write(&path, "v1").unwrap();

        let (_watcher, mut rx) =
            Watcher::start(dir.path(), Duration::from_millis(50)).expect("start");
        tokio::time::sleep(Duration::from_millis(50)).await;
        std::fs::write(&path, "v2").unwrap();

        tokio::time::timeout(Duration::from_secs(2), rx.recv())
            .await
            .expect("modify must trigger an event")
            .expect("channel open");
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn watcher_emits_event_when_file_deleted() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("note.md");
        std::fs::write(&path, "x").unwrap();

        let (_watcher, mut rx) =
            Watcher::start(dir.path(), Duration::from_millis(50)).expect("start");
        tokio::time::sleep(Duration::from_millis(50)).await;
        std::fs::remove_file(&path).unwrap();

        tokio::time::timeout(Duration::from_secs(2), rx.recv())
            .await
            .expect("delete must trigger an event")
            .expect("channel open");
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn watcher_debounces_burst_into_single_event() {
        // Twenty rapid writes within the debounce window must coalesce into
        // exactly one outgoing event. Otherwise reconciliation thrashes.
        let dir = tempdir().unwrap();
        let (_watcher, mut rx) =
            Watcher::start(dir.path(), Duration::from_millis(150)).expect("start");
        tokio::time::sleep(Duration::from_millis(50)).await;

        for i in 0..20 {
            let p = dir.path().join(format!("burst-{i}.md"));
            std::fs::write(&p, "x").unwrap();
        }

        // Wait until well past the debounce window.
        tokio::time::sleep(Duration::from_millis(400)).await;

        // First receive should succeed (the coalesced event).
        let first = rx.try_recv().ok();
        assert!(first.is_some(), "expected one debounced event");

        // Subsequent try_recv should be Empty — no second event.
        let second = rx.try_recv();
        assert!(
            matches!(second, Err(tokio::sync::mpsc::error::TryRecvError::Empty)),
            "burst must coalesce into exactly one event, got {second:?}"
        );
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn watcher_recursive_into_subdirs() {
        // The vault uses notes/YYYY/MM/DD/<id>.md so recursion must go deep.
        let dir = tempdir().unwrap();
        let nested = dir.path().join("a/b/c");
        std::fs::create_dir_all(&nested).unwrap();

        let (_watcher, mut rx) =
            Watcher::start(dir.path(), Duration::from_millis(50)).expect("start");
        tokio::time::sleep(Duration::from_millis(50)).await;
        std::fs::write(nested.join("deep.md"), "x").unwrap();

        tokio::time::timeout(Duration::from_secs(2), rx.recv())
            .await
            .expect("deeply-nested write must produce an event")
            .expect("channel open");
    }
}
