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

// ─── Vault: filesystem layer that owns notes/ ──────────────────────────────

use crate::core::frontmatter::{self, Frontmatter};
use crate::core::ids::NoteId;
use chrono::{DateTime, Datelike, Utc};

/// A loaded note: parsed frontmatter plus body.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Note {
    pub frontmatter: Frontmatter,
    pub body: String,
}

/// A row for the chronological list view (read-cheap; no body).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NoteSummary {
    pub id: NoteId,
    pub title: Option<String>,
    pub created: String,
    pub updated: String,
    pub preview: String,
}

/// Filesystem-backed vault rooted at a directory.
#[derive(Debug, Clone)]
pub struct Vault {
    root: std::path::PathBuf,
}

#[derive(Debug, thiserror::Error)]
pub enum VaultOpError {
    #[error("vault: {0}")]
    Io(#[from] VaultError),
    #[error("frontmatter: {0}")]
    Frontmatter(#[from] frontmatter::FrontmatterError),
    #[error("note not found: {0}")]
    NotFound(NoteId),
    #[error("UTF-8 error: {0}")]
    Utf8(#[from] std::string::FromUtf8Error),
}

impl Vault {
    /// Open a vault rooted at `root`. Creates `notes/` if missing.
    pub fn open(root: impl Into<std::path::PathBuf>) -> Result<Self, VaultOpError> {
        let root: std::path::PathBuf = root.into();
        fs::create_dir_all(root.join("notes")).map_err(VaultError::Io)?;
        Ok(Self { root })
    }

    /// Filesystem path for the note id, partitioned by `created` date.
    fn note_path(&self, id: &NoteId, created: &str) -> Result<std::path::PathBuf, VaultOpError> {
        let dt = parse_iso8601_z(created).ok_or(VaultOpError::Frontmatter(
            frontmatter::FrontmatterError::DisallowedYaml(
                "timestamp must be ISO-8601 with trailing Z",
            ),
        ))?;
        Ok(self.root.join(format!(
            "notes/{:04}/{:02}/{:02}/{}.md",
            dt.year(),
            dt.month(),
            dt.day(),
            id
        )))
    }

    /// Save a new note. Generates a fresh id and stamps created/updated to now.
    pub fn save_note(
        &self,
        body: impl Into<String>,
        title: Option<String>,
    ) -> Result<NoteId, VaultOpError> {
        let id = NoteId::new();
        let now = format_iso8601_z(Utc::now());
        let fm = Frontmatter {
            id,
            created: now.clone(),
            updated: now,
            title,
            tags: vec![],
            links: vec![],
            attachments: vec![],
        };
        let body = body.into();
        let path = self.note_path(&id, &fm.created)?;
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(VaultError::Io)?;
        }
        let serialized = frontmatter::write(&fm, &body)?;
        atomic_write(&path, serialized.as_bytes())?;
        Ok(id)
    }

    /// Read a note by id. Walks the date-partitioned dirs to find it.
    pub fn read_note(&self, id: &NoteId) -> Result<Note, VaultOpError> {
        let path = self
            .find_note_path(id)?
            .ok_or(VaultOpError::NotFound(*id))?;
        let bytes = fs::read(&path).map_err(VaultError::Io)?;
        let source = String::from_utf8(bytes)?;
        let (fm, body) = frontmatter::parse(&source)?;
        Ok(Note {
            frontmatter: fm,
            body: body.to_string(),
        })
    }

    /// List notes ordered by created date descending. Body excluded for cheap I/O.
    pub fn list_notes_chrono(&self, limit: usize) -> Result<Vec<NoteSummary>, VaultOpError> {
        let mut summaries = Vec::new();
        let notes_dir = self.root.join("notes");
        for entry in walk_md_files(&notes_dir) {
            let bytes = match fs::read(&entry) {
                Ok(b) => b,
                Err(_) => continue,
            };
            let source = match String::from_utf8(bytes) {
                Ok(s) => s,
                Err(_) => continue,
            };
            let (fm, body) = match frontmatter::parse(&source) {
                Ok(p) => p,
                Err(_) => continue,
            };
            summaries.push(NoteSummary {
                id: fm.id,
                title: fm.title.clone(),
                created: fm.created.clone(),
                updated: fm.updated.clone(),
                preview: body.lines().next().unwrap_or("").to_string(),
            });
        }
        summaries.sort_by(|a, b| b.created.cmp(&a.created));
        summaries.truncate(limit);
        Ok(summaries)
    }

    fn find_note_path(&self, id: &NoteId) -> Result<Option<std::path::PathBuf>, VaultOpError> {
        let needle = format!("{id}.md");
        for entry in walk_md_files(&self.root.join("notes")) {
            if entry.file_name().is_some_and(|n| n == needle.as_str()) {
                return Ok(Some(entry));
            }
        }
        Ok(None)
    }
}

fn walk_md_files(dir: &Path) -> Vec<std::path::PathBuf> {
    // Iterative depth-first, collecting all .md files. Vaults are small
    // enough that streaming doesn't earn its complexity.
    let mut out = Vec::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        let Ok(read) = fs::read_dir(&d) else { continue };
        for entry in read.flatten() {
            let p = entry.path();
            if p.is_dir() {
                stack.push(p);
            } else if p.extension().is_some_and(|e| e == "md") {
                out.push(p);
            }
        }
    }
    out
}

fn parse_iso8601_z(s: &str) -> Option<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(s)
        .ok()
        .map(|d| d.with_timezone(&Utc))
}

fn format_iso8601_z(dt: DateTime<Utc>) -> String {
    dt.format("%Y-%m-%dT%H:%M:%SZ").to_string()
}

#[cfg(test)]
mod vault_tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn save_then_read_returns_same_note() {
        let dir = tempdir().unwrap();
        let v = Vault::open(dir.path()).unwrap();
        let id = v
            .save_note("hello body", Some("title".to_string()))
            .unwrap();
        let note = v.read_note(&id).unwrap();
        assert_eq!(note.frontmatter.id, id);
        assert_eq!(note.frontmatter.title.as_deref(), Some("title"));
        assert_eq!(note.body, "hello body");
    }

    #[test]
    fn save_persists_into_date_partitioned_path() {
        let dir = tempdir().unwrap();
        let v = Vault::open(dir.path()).unwrap();
        let id = v.save_note("content", None).unwrap();
        let now = Utc::now();
        let expected = dir.path().join(format!(
            "notes/{:04}/{:02}/{:02}/{}.md",
            now.year(),
            now.month(),
            now.day(),
            id
        ));
        assert!(expected.exists(), "expected note at {expected:?}");
    }

    #[test]
    fn read_note_returns_not_found_for_unknown_id() {
        let dir = tempdir().unwrap();
        let v = Vault::open(dir.path()).unwrap();
        let unknown = NoteId::new();
        let err = v.read_note(&unknown).unwrap_err();
        assert!(matches!(err, VaultOpError::NotFound(_)));
    }

    #[test]
    fn list_notes_chrono_orders_by_created_descending() {
        let dir = tempdir().unwrap();
        let v = Vault::open(dir.path()).unwrap();
        let _id1 = v.save_note("first", None).unwrap();
        std::thread::sleep(std::time::Duration::from_millis(1100)); // crosses 1s boundary
        let id2 = v.save_note("second", None).unwrap();
        let summaries = v.list_notes_chrono(10).unwrap();
        assert_eq!(summaries.len(), 2);
        assert_eq!(summaries[0].id, id2, "newest first");
    }

    #[test]
    fn list_notes_chrono_respects_limit() {
        let dir = tempdir().unwrap();
        let v = Vault::open(dir.path()).unwrap();
        for _ in 0..5 {
            v.save_note("body", None).unwrap();
        }
        let summaries = v.list_notes_chrono(3).unwrap();
        assert_eq!(summaries.len(), 3);
    }

    #[test]
    fn list_notes_chrono_includes_first_line_preview() {
        let dir = tempdir().unwrap();
        let v = Vault::open(dir.path()).unwrap();
        v.save_note("first line\nsecond line", None).unwrap();
        let summaries = v.list_notes_chrono(10).unwrap();
        assert_eq!(summaries[0].preview, "first line");
    }

    #[test]
    fn note_on_disk_is_human_readable_markdown() {
        // The user's contract: open the vault directory, cat any .md, get
        // valid YAML frontmatter + readable body. No proprietary blobs.
        let dir = tempdir().unwrap();
        let v = Vault::open(dir.path()).unwrap();
        v.save_note("a body that humans can read", Some("readable".to_string()))
            .unwrap();

        let entries = walk_md_files(&dir.path().join("notes"));
        let raw =
            std::fs::read_to_string(entries.first().expect("must produce a .md file")).unwrap();
        assert!(raw.starts_with("---\n"));
        assert!(raw.contains("title: readable\n"));
        assert!(raw.contains("a body that humans can read"));
    }

    // Catch the "_ = let" typo we accept in walk_md_files via underscore. This
    // test asserts the walker really skips dirs without crashing on permission
    // surprises later. (Currently no perm-denied path; placeholder for v0.2.)
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
