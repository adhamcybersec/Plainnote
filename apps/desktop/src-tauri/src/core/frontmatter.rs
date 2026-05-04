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
