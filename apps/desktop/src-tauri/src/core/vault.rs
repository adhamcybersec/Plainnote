// SPDX-License-Identifier: AGPL-3.0-or-later
//! The vault — files-on-disk source of truth for notes.

use std::fs;
use std::io::{self, Write};
use std::path::Path;

#[derive(Debug, thiserror::Error)]
pub enum VaultError {
    #[error("I/O error: {0}")]
    Io(#[from] io::Error),
    #[error("vault path does not exist: {0}")]
    MissingParent(String),
}

/// Atomically write `bytes` to `path`.
///
/// Protocol:
///   1. Create a sibling tempfile in the same directory with a random suffix.
///   2. Write all bytes; `flush()` and `sync_all()` (fsync the data + metadata).
///   3. `rename(tmp, path)` — POSIX guarantees atomicity on the same filesystem.
///   4. `sync_all()` on the parent directory — ensures the rename survives a
///      crash before the dirent reaches disk.
///
/// On crash mid-write the target either has the previous content (if it
/// existed) or doesn't exist at all. There is never a partial file at `path`.
pub fn atomic_write(path: &Path, bytes: &[u8]) -> Result<(), VaultError> {
    let parent = path
        .parent()
        .ok_or_else(|| VaultError::MissingParent(path.display().to_string()))?;
    if !parent.exists() {
        return Err(VaultError::MissingParent(parent.display().to_string()));
    }

    // Random suffix is enough; we don't need cryptographic uniqueness.
    let nonce = ulid::Ulid::new();
    let tmp_path = parent.join(format!(
        ".{}.tmp.{}",
        path.file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("plainnote"),
        nonce
    ));

    // Write + fsync the data file. Drop the handle to release the FD before
    // rename (Windows compatibility; harmless on Linux).
    {
        let mut file = fs::File::create(&tmp_path)?;
        file.write_all(bytes)?;
        file.flush()?;
        file.sync_all()?;
    }

    // POSIX-atomic on the same filesystem.
    fs::rename(&tmp_path, path)?;

    // Persist the directory entry. fsync the parent dir so a crash here
    // can't lose the rename. Best-effort on platforms that don't support it.
    if let Ok(dir) = fs::File::open(parent) {
        let _ = dir.sync_all();
    }

    Ok(())
}

#[cfg(test)]
mod atomic_write_tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn atomic_write_then_read_returns_same_bytes() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("hello.md");
        atomic_write(&path, b"hello, world").unwrap();
        let read_back = std::fs::read(&path).unwrap();
        assert_eq!(read_back, b"hello, world");
    }

    #[test]
    fn atomic_write_replaces_existing_file() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("note.md");
        atomic_write(&path, b"first").unwrap();
        atomic_write(&path, b"second").unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), b"second");
    }

    #[test]
    fn atomic_write_leaves_no_tempfile_on_success() {
        // After a successful write the directory must contain only the
        // target file. A leftover `.tmp.*` would confuse Syncthing and
        // accumulate junk over time.
        let dir = tempdir().unwrap();
        let path = dir.path().join("note.md");
        atomic_write(&path, b"data").unwrap();
        let entries: Vec<_> = std::fs::read_dir(dir.path())
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        assert_eq!(
            entries,
            vec!["note.md".to_string()],
            "found unexpected files: {entries:?}"
        );
    }

    #[test]
    fn atomic_write_errors_when_parent_missing() {
        // Writing to /a/b/c/note.md when /a/b/c does not exist must error,
        // not silently create the directory tree. The vault layer is
        // responsible for creating directories explicitly.
        let dir = tempdir().unwrap();
        let path = dir.path().join("does/not/exist/note.md");
        let err = atomic_write(&path, b"data").unwrap_err();
        assert!(matches!(err, VaultError::MissingParent(_)));
    }

    #[test]
    fn atomic_write_round_trips_arbitrary_bytes_including_nul() {
        // Notes are markdown but body content may contain any UTF-8 + zeros
        // (e.g. embedded base64 of small images down the road). The writer
        // must not truncate at NUL.
        let dir = tempdir().unwrap();
        let path = dir.path().join("blob");
        let payload: Vec<u8> = (0u8..=255).collect();
        atomic_write(&path, &payload).unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), payload);
    }

    #[test]
    fn atomic_write_overwrite_is_atomic_under_concurrent_readers() {
        // Best-effort: the read either sees the OLD content or the NEW one,
        // never partial. This test runs the write 32 times against a reader
        // loop and asserts the reader never observes anything but the two
        // expected payloads.
        use std::sync::{
            atomic::{AtomicBool, Ordering},
            Arc,
        };
        use std::thread;

        let dir = tempdir().unwrap();
        let path = dir.path().join("race.md");
        atomic_write(&path, b"AAAAAAAAAAAAAAAAAAAA").unwrap();

        let stop = Arc::new(AtomicBool::new(false));
        let stop_reader = Arc::clone(&stop);
        let path_reader = path.clone();

        let reader = thread::spawn(move || {
            while !stop_reader.load(Ordering::Relaxed) {
                if let Ok(buf) = std::fs::read(&path_reader) {
                    assert!(
                        buf == b"AAAAAAAAAAAAAAAAAAAA" || buf == b"BBBBBBBBBBBBBBBBBBBB",
                        "saw torn write: {:?}",
                        String::from_utf8_lossy(&buf)
                    );
                }
            }
        });

        for _ in 0..32 {
            atomic_write(&path, b"BBBBBBBBBBBBBBBBBBBB").unwrap();
            atomic_write(&path, b"AAAAAAAAAAAAAAAAAAAA").unwrap();
        }

        stop.store(true, Ordering::Relaxed);
        reader.join().unwrap();
    }
}
