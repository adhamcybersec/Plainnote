// SPDX-License-Identifier: AGPL-3.0-or-later
//! Strict YAML frontmatter for `.md` notes.
//!
//! The schema is closed: any unknown key is a parse error. Anchors, tags
//! (`!!type`), and multi-document streams are rejected. This is defence in
//! depth — if a Syncthing-synced file shows up with unexpected keys we want
//! to fail loudly, not silently drop them.

use serde::{Deserialize, Serialize};

use crate::core::ids::NoteId;

/// The closed schema. Adding a field requires a migration plan.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Frontmatter {
    pub id: NoteId,
    pub created: String, // ISO-8601 with trailing 'Z'; validated separately
    pub updated: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tags: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub links: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub attachments: Vec<Attachment>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, tag = "kind", rename_all = "lowercase")]
pub enum Attachment {
    Image {
        file: String,
    },
    Audio {
        file: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        transcript: Option<String>,
    },
}

#[derive(Debug, thiserror::Error)]
pub enum FrontmatterError {
    #[error("frontmatter must start with a `---` fence on its own line")]
    MissingOpeningFence,
    #[error("frontmatter has no closing `---` fence")]
    MissingClosingFence,
    #[error("YAML parse error: {0}")]
    Yaml(#[from] serde_yaml::Error),
    #[error("YAML feature is not allowed in this schema: {0}")]
    DisallowedYaml(&'static str),
}

/// Write a `Frontmatter` + body to canonical `.md` form.
///
/// Output guarantees that exist for sync-safety:
///   * UTF-8 bytes are byte-stable for identical inputs.
///   * Keys appear in a fixed canonical order (id, created, updated, title,
///     tags, links, attachments).
///   * Empty optional fields are omitted entirely.
///   * Tag and link arrays are emitted in block style with two-space indents.
///   * `created` and `updated` are validated as ISO-8601 with trailing `Z`.
///   * Strings are written without YAML-implicit quoting unless they contain
///     characters that would otherwise change the parsed type.
pub fn write(fm: &Frontmatter, body: &str) -> Result<String, FrontmatterError> {
    validate_iso8601_z(&fm.created)?;
    validate_iso8601_z(&fm.updated)?;

    let mut out = String::with_capacity(256 + body.len());
    out.push_str("---\n");

    write_kv(&mut out, "id", &fm.id.to_string());
    write_kv(&mut out, "created", &fm.created);
    write_kv(&mut out, "updated", &fm.updated);

    if let Some(title) = &fm.title {
        write_kv(&mut out, "title", title);
    }
    if !fm.tags.is_empty() {
        out.push_str("tags:\n");
        for tag in &fm.tags {
            out.push_str("  - ");
            out.push_str(&yaml_scalar(tag));
            out.push('\n');
        }
    }
    if !fm.links.is_empty() {
        out.push_str("links:\n");
        for link in &fm.links {
            out.push_str("  - ");
            out.push_str(&yaml_scalar(link));
            out.push('\n');
        }
    }
    if !fm.attachments.is_empty() {
        out.push_str("attachments:\n");
        for att in &fm.attachments {
            match att {
                Attachment::Image { file } => {
                    out.push_str("  - kind: image\n    file: ");
                    out.push_str(&yaml_scalar(file));
                    out.push('\n');
                }
                Attachment::Audio { file, transcript } => {
                    out.push_str("  - kind: audio\n    file: ");
                    out.push_str(&yaml_scalar(file));
                    out.push('\n');
                    if let Some(t) = transcript {
                        out.push_str("    transcript: ");
                        out.push_str(&yaml_scalar(t));
                        out.push('\n');
                    }
                }
            }
        }
    }

    out.push_str("---\n\n");
    out.push_str(body);
    Ok(out)
}

fn write_kv(out: &mut String, key: &str, value: &str) {
    out.push_str(key);
    out.push_str(": ");
    out.push_str(&yaml_scalar(value));
    out.push('\n');
}

/// Quote a scalar only when it would otherwise be parsed as a non-string type.
/// Strings that look like booleans, null, numbers, or contain colons / leading
/// dashes / hashes get double-quoted; everything else is emitted bare.
fn yaml_scalar(s: &str) -> String {
    let needs_quote = s.is_empty()
        || s.trim().is_empty()
        || s.trim().len() != s.len()
        || s.starts_with([
            '-', '#', '?', ':', '[', ']', '{', '}', '&', '*', '!', '|', '>', '\'', '"', '%', '@',
            '`',
        ])
        || s.contains(['\n', '\t', ':', '#'])
        || matches!(
            s.to_ascii_lowercase().as_str(),
            "true" | "false" | "null" | "~" | "yes" | "no" | "on" | "off"
        )
        || s.parse::<f64>().is_ok();
    if needs_quote {
        let escaped = s.replace('\\', "\\\\").replace('"', "\\\"");
        format!("\"{escaped}\"")
    } else {
        s.to_string()
    }
}

fn validate_iso8601_z(s: &str) -> Result<(), FrontmatterError> {
    // Cheap structural check: YYYY-MM-DDTHH:MM:SSZ (20 chars). Full datetime
    // validation lives in the timestamp helper layer (M1a-T5+).
    if s.len() != 20 || !s.ends_with('Z') {
        return Err(FrontmatterError::DisallowedYaml(
            "timestamp must be ISO-8601 with trailing Z",
        ));
    }
    let bytes = s.as_bytes();
    if bytes[4] != b'-'
        || bytes[7] != b'-'
        || bytes[10] != b'T'
        || bytes[13] != b':'
        || bytes[16] != b':'
    {
        return Err(FrontmatterError::DisallowedYaml(
            "timestamp must be ISO-8601 with trailing Z",
        ));
    }
    Ok(())
}

/// Parse a `.md` file containing frontmatter into the typed schema and the body.
pub fn parse(source: &str) -> Result<(Frontmatter, &str), FrontmatterError> {
    // Reject non-trivial YAML features that we never emit and don't want to
    // accept from external editors. Cheap pre-check before handing to serde.
    let (yaml, body) = split_frontmatter(source)?;
    if yaml.contains('&') || yaml.contains('*') {
        return Err(FrontmatterError::DisallowedYaml("YAML anchors (&/*)"));
    }
    if yaml.contains("!!") {
        return Err(FrontmatterError::DisallowedYaml("YAML explicit tags (!!)"));
    }
    let fm: Frontmatter = serde_yaml::from_str(yaml)?;
    Ok((fm, body))
}

/// Split a markdown source on the opening and closing `---` fences.
fn split_frontmatter(source: &str) -> Result<(&str, &str), FrontmatterError> {
    let rest = source
        .strip_prefix("---\n")
        .or_else(|| source.strip_prefix("---\r\n"))
        .ok_or(FrontmatterError::MissingOpeningFence)?;

    // Closing fence must be on its own line.
    let close = rest
        .find("\n---\n")
        .or_else(|| rest.find("\n---\r\n"))
        .or_else(|| {
            // EOF after fence with no trailing newline
            if rest.ends_with("\n---") {
                Some(rest.len() - 4)
            } else {
                None
            }
        })
        .ok_or(FrontmatterError::MissingClosingFence)?;

    let yaml = &rest[..close];
    let body = rest[close..]
        .trim_start_matches('\n')
        .trim_start_matches("---")
        .trim_start_matches('\n')
        .trim_start_matches("\r\n");
    Ok((yaml, body))
}

#[cfg(test)]
mod tests {
    // First failing test (TDD RED): parse our canonical schema example.

    use super::*;

    const MINIMAL: &str = "---\n\
id: 01HXYZ0000000000000000000A\n\
created: 2026-05-04T10:23:11Z\n\
updated: 2026-05-04T10:25:02Z\n\
---\n\nBody text after the closing fence.";

    #[test]
    fn parses_minimal_valid_frontmatter() {
        let (fm, body) = parse(MINIMAL).expect("must parse");
        assert_eq!(fm.id.to_string(), "01HXYZ0000000000000000000A");
        assert_eq!(fm.title, None);
        assert!(fm.tags.is_empty());
        assert!(fm.links.is_empty());
        assert!(fm.attachments.is_empty());
        assert_eq!(body.trim(), "Body text after the closing fence.");
    }

    #[test]
    fn parses_full_schema() {
        let input = "---\n\
id: 01HXYZ0000000000000000000A\n\
created: 2026-05-04T10:23:11Z\n\
updated: 2026-05-04T10:25:02Z\n\
title: Optional title\n\
tags:\n  \
- learning/mathematics/calculus\n  \
- work/projectTTK\n\
links:\n  \
- 01HABC0000000000000000000A\n  \
- 01HDEF0000000000000000000A\n\
attachments:\n  \
- kind: image\n    \
file: img-001.png\n  \
- kind: audio\n    \
file: audio-001.opus\n    \
transcript: audio-001.transcript.txt\n\
---\nBody";
        let (fm, _) = parse(input).expect("must parse full schema");
        assert_eq!(fm.title.as_deref(), Some("Optional title"));
        assert_eq!(fm.tags.len(), 2);
        assert_eq!(fm.tags[0], "learning/mathematics/calculus");
        assert_eq!(fm.links.len(), 2);
        assert_eq!(fm.attachments.len(), 2);
        assert!(matches!(fm.attachments[0], Attachment::Image { .. }));
        assert!(matches!(
            fm.attachments[1],
            Attachment::Audio {
                transcript: Some(_),
                ..
            }
        ));
    }

    #[test]
    fn rejects_unknown_top_level_keys() {
        // Defence-in-depth: a Syncthing-synced file with extra keys must fail loudly.
        let input = "---\n\
id: 01HXYZ0000000000000000000A\n\
created: 2026-05-04T10:23:11Z\n\
updated: 2026-05-04T10:25:02Z\n\
secret_telemetry_field: please_send_data\n\
---\nBody";
        let err = parse(input).expect_err("unknown field must be a parse error");
        assert!(matches!(err, FrontmatterError::Yaml(_)), "got {err:?}");
    }

    #[test]
    fn rejects_yaml_anchors() {
        // Anchors are a denial-of-service vector (billion laughs etc.). Not used.
        let input = "---\n\
id: &anchor 01HXYZ0000000000000000000A\n\
created: 2026-05-04T10:23:11Z\n\
updated: 2026-05-04T10:25:02Z\n\
---\nBody";
        let err = parse(input).expect_err("anchors must be rejected");
        assert!(matches!(err, FrontmatterError::DisallowedYaml(_)));
    }

    #[test]
    fn rejects_yaml_explicit_tags() {
        let input = "---\n\
id: !!str 01HXYZ0000000000000000000A\n\
created: 2026-05-04T10:23:11Z\n\
updated: 2026-05-04T10:25:02Z\n\
---\nBody";
        let err = parse(input).expect_err("explicit tags must be rejected");
        assert!(matches!(err, FrontmatterError::DisallowedYaml(_)));
    }

    #[test]
    fn rejects_missing_opening_fence() {
        let input = "id: 01HXYZ0000000000000000000A\n";
        assert!(matches!(
            parse(input).unwrap_err(),
            FrontmatterError::MissingOpeningFence
        ));
    }

    #[test]
    fn rejects_missing_closing_fence() {
        let input = "---\nid: 01HXYZ0000000000000000000A\nno fence below";
        assert!(matches!(
            parse(input).unwrap_err(),
            FrontmatterError::MissingClosingFence
        ));
    }

    #[test]
    fn rejects_invalid_id_in_frontmatter() {
        let input = "---\n\
id: not-a-ulid\n\
created: 2026-05-04T10:23:11Z\n\
updated: 2026-05-04T10:25:02Z\n\
---\nBody";
        assert!(parse(input).is_err());
    }

    // ─── writer tests (T3) ─────────────────────────────────────────────────

    fn sample_full() -> Frontmatter {
        Frontmatter {
            id: NoteId::parse("01HXYZ0000000000000000000A").unwrap(),
            created: "2026-05-04T10:23:11Z".to_string(),
            updated: "2026-05-04T10:25:02Z".to_string(),
            title: Some("Optional title".to_string()),
            tags: vec![
                "learning/mathematics/calculus".to_string(),
                "work/projectTTK".to_string(),
            ],
            links: vec![
                "01HABC0000000000000000000A".to_string(),
                "01HDEF0000000000000000000A".to_string(),
            ],
            attachments: vec![
                Attachment::Image {
                    file: "img-001.png".to_string(),
                },
                Attachment::Audio {
                    file: "audio-001.opus".to_string(),
                    transcript: Some("audio-001.transcript.txt".to_string()),
                },
            ],
        }
    }

    #[test]
    fn write_emits_canonical_form_with_fences_and_blank_line() {
        // The output must round-trip via parse() and start with `---\n`.
        let fm = sample_full();
        let out = write(&fm, "Body text").unwrap();
        assert!(out.starts_with("---\n"));
        assert!(
            out.contains("\n---\n\n"),
            "must have blank line after fence"
        );
        assert!(out.ends_with("Body text"));
    }

    #[test]
    fn write_emits_keys_in_canonical_order() {
        // id → created → updated → title → tags → links → attachments.
        // Order is deterministic to avoid sync churn from key reordering.
        let fm = sample_full();
        let out = write(&fm, "").unwrap();
        let positions: Vec<usize> = [
            "id:",
            "created:",
            "updated:",
            "title:",
            "tags:",
            "links:",
            "attachments:",
        ]
        .iter()
        .map(|k| out.find(k).unwrap_or_else(|| panic!("missing key: {k}")))
        .collect();
        for w in positions.windows(2) {
            assert!(w[0] < w[1], "keys out of canonical order: {positions:?}");
        }
    }

    #[test]
    fn write_skips_empty_optional_fields() {
        // No title, no tags, no links, no attachments → none of those keys appear.
        let fm = Frontmatter {
            id: NoteId::parse("01HXYZ0000000000000000000A").unwrap(),
            created: "2026-05-04T10:23:11Z".to_string(),
            updated: "2026-05-04T10:25:02Z".to_string(),
            title: None,
            tags: vec![],
            links: vec![],
            attachments: vec![],
        };
        let out = write(&fm, "Body").unwrap();
        assert!(!out.contains("title:"));
        assert!(!out.contains("tags:"));
        assert!(!out.contains("links:"));
        assert!(!out.contains("attachments:"));
    }

    #[test]
    fn write_emits_tags_in_block_style() {
        // Block-style YAML arrays diff better than flow-style ([a, b, c]).
        let fm = sample_full();
        let out = write(&fm, "").unwrap();
        assert!(
            out.contains("tags:\n  - learning/mathematics/calculus\n  - work/projectTTK\n"),
            "tags must be in block form, got:\n{out}"
        );
    }

    #[test]
    fn write_then_parse_is_identity() {
        // Round-trip: parse(write(parse(s))) == parse(s).
        // Strongest contract for sync-safety.
        let fm = sample_full();
        let body = "Body content with [[wikilink]] and #tag";
        let out = write(&fm, body).unwrap();
        let (round, round_body) = parse(&out).unwrap();
        assert_eq!(round, fm);
        assert_eq!(round_body, body);
    }

    #[test]
    fn write_rejects_invalid_iso8601_timestamp() {
        // Defence: the schema demands ISO-8601 with 'Z'; reject other shapes
        // before they hit disk and start a sync churn loop.
        let mut fm = sample_full();
        fm.created = "yesterday".to_string();
        assert!(write(&fm, "").is_err());
    }

    #[test]
    fn write_is_byte_stable() {
        // Two writes of the same Frontmatter produce byte-identical output.
        let fm = sample_full();
        let a = write(&fm, "Body").unwrap();
        let b = write(&fm, "Body").unwrap();
        assert_eq!(a, b);
    }

    proptest::proptest! {
        #[test]
        fn property_round_trip_preserves_frontmatter(
            // Generate frontmatters from constrained inputs so the property
            // exercises the writer's edge cases without producing YAML the
            // schema doesn't allow.
            title_present in proptest::prelude::any::<bool>(),
            title in "[A-Za-z0-9 ]{1,40}",
            tag_count in 0usize..5,
            link_count in 0usize..5,
        ) {
            let fm = Frontmatter {
                id: NoteId::new(),
                created: "2026-05-04T10:23:11Z".to_string(),
                updated: "2026-05-04T10:25:02Z".to_string(),
                title: title_present.then_some(title),
                tags: (0..tag_count).map(|i| format!("topic/sub{i}")).collect(),
                links: (0..link_count).map(|_| NoteId::new().to_string()).collect(),
                attachments: vec![],
            };
            let out = write(&fm, "").unwrap();
            let (round, _) = parse(&out).expect("written form must parse");
            proptest::prop_assert_eq!(round, fm);
        }
    }

    #[test]
    fn rejects_unknown_attachment_kind() {
        // The attachment enum is also closed: only image/audio are allowed.
        let input = "---\n\
id: 01HXYZ0000000000000000000A\n\
created: 2026-05-04T10:23:11Z\n\
updated: 2026-05-04T10:25:02Z\n\
attachments:\n  \
- kind: telemetry\n    \
file: spy.json\n\
---\nBody";
        assert!(parse(input).is_err());
    }
}
