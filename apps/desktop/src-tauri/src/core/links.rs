// SPDX-License-Identifier: AGPL-3.0-or-later
//! Wikilink parser.
//!
//! Plainnote uses Obsidian-style `[[…]]` syntax in note bodies:
//!
//!     [[Title]]                — link by exact title
//!     [[Title|alias]]          — link with display alias
//!     [[01HXYZ…]]              — link by ULID (rename-stable)
//!     \[[not a link]]          — escaped, parser ignores
//!
//! The parser returns a list of `Wikilink` records — the resolver in
//! `links::resolve_target` (M3-T3) decides whether each one points to an
//! actual note, an alias, or a dangling reference.

/// One parsed wikilink occurrence.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Wikilink {
    /// The raw matched text including delimiters, e.g. `[[Title|alias]]`.
    pub raw: String,
    /// The byte offset of the match start within the input.
    pub start: usize,
    /// The link target before the optional `|alias`. Trimmed.
    pub target_text: String,
    /// Display alias if `[[target|alias]]`. Trimmed; None if not provided.
    pub alias: Option<String>,
}

/// Extract every wikilink from `body`. Handles escaped `\[[`, requires a
/// closing `]]`, and rejects targets containing newlines or `[]` chars.
pub fn parse(body: &str) -> Vec<Wikilink> {
    let bytes = body.as_bytes();
    let mut out = Vec::new();
    let mut i = 0usize;
    while i + 1 < bytes.len() {
        // Look for `[[` not preceded by an unescaped `\`.
        if bytes[i] == b'[' && bytes[i + 1] == b'[' {
            // Escape: a single backslash directly before `[[` cancels the link.
            // (`\\[[` — backslash before backslash before `[[` — *is* a valid
            // link because the first backslash escapes the second.)
            let escaped = i > 0 && bytes[i - 1] == b'\\' && (i < 2 || bytes[i - 2] != b'\\');
            if escaped {
                i += 1;
                continue;
            }
            // Find the matching `]]`. Refuse newline or stray `[` inside.
            let inner_start = i + 2;
            let mut j = inner_start;
            let mut closing: Option<usize> = None;
            while j + 1 < bytes.len() {
                let b = bytes[j];
                if b == b'\n' || b == b'[' {
                    break;
                }
                if b == b']' && bytes[j + 1] == b']' {
                    closing = Some(j);
                    break;
                }
                j += 1;
            }
            if let Some(close) = closing {
                let inner = &body[inner_start..close];
                let (target, alias) = match inner.split_once('|') {
                    Some((t, a)) => (t.trim().to_string(), Some(a.trim().to_string())),
                    None => (inner.trim().to_string(), None),
                };
                if !target.is_empty() {
                    let raw = body[i..close + 2].to_string();
                    out.push(Wikilink {
                        raw,
                        start: i,
                        target_text: target,
                        alias,
                    });
                }
                i = close + 2;
                continue;
            }
        }
        i += 1;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_a_single_bare_wikilink() {
        let parsed = parse("see [[Calculus]] for more");
        assert_eq!(parsed.len(), 1);
        assert_eq!(parsed[0].target_text, "Calculus");
        assert!(parsed[0].alias.is_none());
        assert_eq!(&parsed[0].raw, "[[Calculus]]");
    }

    #[test]
    fn parses_alias_after_pipe() {
        let parsed = parse("see [[Calculus|the calc note]] please");
        assert_eq!(parsed.len(), 1);
        assert_eq!(parsed[0].target_text, "Calculus");
        assert_eq!(parsed[0].alias.as_deref(), Some("the calc note"));
        assert_eq!(&parsed[0].raw, "[[Calculus|the calc note]]");
    }

    #[test]
    fn parses_ulid_target() {
        let parsed = parse("here: [[01HXYZ0000000000000000000A]]");
        assert_eq!(parsed.len(), 1);
        assert_eq!(parsed[0].target_text, "01HXYZ0000000000000000000A");
    }

    #[test]
    fn extracts_multiple_wikilinks_in_order() {
        let body = "[[A]] then [[B|short]] then [[C]]";
        let parsed = parse(body);
        let targets: Vec<&str> = parsed.iter().map(|w| w.target_text.as_str()).collect();
        assert_eq!(targets, vec!["A", "B", "C"]);
        // Their start offsets must be strictly increasing.
        let starts: Vec<usize> = parsed.iter().map(|w| w.start).collect();
        for w in starts.windows(2) {
            assert!(w[0] < w[1]);
        }
    }

    #[test]
    fn ignores_escaped_double_bracket() {
        // Backslash directly before `[[` cancels the link.
        let parsed = parse("not a link: \\[[Calculus]]");
        assert_eq!(parsed.len(), 0, "got {parsed:?}");
    }

    #[test]
    fn double_backslash_does_not_escape() {
        // `\\[[Calculus]]` — backslash escapes backslash, so `[[` IS active.
        let parsed = parse("\\\\[[Calculus]]");
        assert_eq!(parsed.len(), 1);
        assert_eq!(parsed[0].target_text, "Calculus");
    }

    #[test]
    fn rejects_unclosed_link() {
        let parsed = parse("dangling [[Calculus and more text");
        assert_eq!(parsed.len(), 0);
    }

    #[test]
    fn rejects_link_with_newline_inside() {
        let parsed = parse("[[Cal\nculus]]");
        assert_eq!(parsed.len(), 0);
    }

    #[test]
    fn rejects_link_with_inner_open_bracket() {
        // Stops at the inner `[`. Leaves the rest unmatched.
        let parsed = parse("[[Cal[culus]]");
        assert_eq!(parsed.len(), 0);
    }

    #[test]
    fn rejects_empty_target() {
        let parsed = parse("[[]] and [[ ]] and [[||alias]]");
        assert_eq!(parsed.len(), 0);
    }

    #[test]
    fn trims_whitespace_in_target_and_alias() {
        let parsed = parse("[[  Calculus  |   short  ]]");
        assert_eq!(parsed[0].target_text, "Calculus");
        assert_eq!(parsed[0].alias.as_deref(), Some("short"));
    }

    #[test]
    fn raw_span_round_trips_via_start_offset() {
        // Slicing body[start..start+raw.len()] must equal `raw`. This is the
        // contract a future renderer relies on for in-place link replacement.
        let body = "prefix [[Target]] middle [[Other|x]] tail";
        for w in parse(body) {
            let span = &body[w.start..w.start + w.raw.len()];
            assert_eq!(span, w.raw);
        }
    }

    use proptest::prelude::*;
    proptest! {
        #[test]
        fn property_parsing_a_pure_text_body_finds_no_links(
            // Body of letters, digits, spaces — nothing that could form `[[`.
            body in "[A-Za-z0-9 ]{0,200}",
        ) {
            let parsed = parse(&body);
            prop_assert!(parsed.is_empty(), "got {parsed:?} for body {body:?}");
        }

        #[test]
        fn property_round_trip_through_format_is_stable(
            target in "[A-Za-z][A-Za-z0-9 _-]{0,40}",
        ) {
            let body = format!("prefix [[{target}]] tail");
            let parsed = parse(&body);
            prop_assert_eq!(parsed.len(), 1);
            prop_assert_eq!(parsed[0].target_text.as_str(), target.trim());
        }
    }
}
