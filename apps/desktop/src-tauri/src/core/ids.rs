// SPDX-License-Identifier: AGPL-3.0-or-later
//! Note identifiers — ULIDs in Crockford base32 form.
//!
//! ULID = 26 chars, lexicographically sortable, time-ordered, conflict-free
//! across devices for offline-first sync. We wrap it in a `NoteId` newtype
//! so the rest of the codebase can never accidentally pass a raw string
//! where a validated id is expected.

use serde::{Deserialize, Serialize};
use std::fmt;
use std::str::FromStr;

/// 26-character Crockford-base32 ULID.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct NoteId(ulid::Ulid);

#[derive(Debug, thiserror::Error)]
pub enum IdError {
    #[error("invalid ULID: {0}")]
    Invalid(String),
}

impl NoteId {
    /// Generate a fresh time-ordered id.
    pub fn new() -> Self {
        Self(ulid::Ulid::new())
    }

    /// Parse from the canonical 26-char Crockford-base32 form.
    pub fn parse(s: &str) -> Result<Self, IdError> {
        ulid::Ulid::from_str(s)
            .map(NoteId)
            .map_err(|e| IdError::Invalid(e.to_string()))
    }
}

impl Default for NoteId {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Display for NoteId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

#[cfg(test)]
mod tests {
    // First failing test (TDD RED): a freshly-generated id must round-trip
    // through its string form unchanged.

    use super::*;

    #[test]
    fn note_id_round_trips_through_string() {
        let id = NoteId::new();
        let s = id.to_string();
        let parsed = NoteId::parse(&s).expect("must parse its own output");
        assert_eq!(id, parsed);
    }

    #[test]
    fn parse_rejects_wrong_length() {
        // ULID is exactly 26 chars; anything else is invalid.
        assert!(NoteId::parse("").is_err());
        assert!(NoteId::parse("0").is_err());
        assert!(NoteId::parse(&"0".repeat(25)).is_err());
        assert!(NoteId::parse(&"0".repeat(27)).is_err());
        assert!(NoteId::parse(&"0".repeat(100)).is_err());
    }

    #[test]
    fn parse_rejects_non_crockford_alphabet() {
        // Crockford base32 excludes I, L, O, U to avoid confusion with 1, 1, 0, V.
        // The ulid crate enforces this; we encode the contract in a test so a
        // future change to the underlying crate surfaces immediately.
        let id = NoteId::new();
        let mut s = id.to_string();
        s.replace_range(0..1, "I");
        assert!(
            NoteId::parse(&s).is_err(),
            "I must not be a valid Crockford char"
        );
    }

    #[test]
    fn parse_rejects_embedded_nul() {
        // Defense-in-depth: even if 26 chars, NUL bytes must never reach a path join.
        let mut s = NoteId::new().to_string();
        s.replace_range(0..1, "\0");
        assert!(NoteId::parse(&s).is_err());
    }

    #[test]
    fn parse_rejects_path_traversal() {
        // ".." or "/" must never be mistaken for a valid id, even by length.
        // A 26-char string built from these characters cannot exist in
        // Crockford alphabet, so parse must reject every variant.
        for hostile in [".................", "../../etc/passwd", "/", "..", "../"] {
            assert!(
                NoteId::parse(hostile).is_err(),
                "must reject path-traversal-shaped input: {hostile:?}"
            );
        }
    }

    #[test]
    fn ids_are_lexicographically_sortable_by_creation_time() {
        // ULID timestamp is the leading component; later id sorts after earlier.
        let early = NoteId::new();
        std::thread::sleep(std::time::Duration::from_millis(2));
        let late = NoteId::new();
        assert!(
            early.to_string() < late.to_string(),
            "later id must sort after earlier id"
        );
    }
}
