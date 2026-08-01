//! Workout document identity.
//!
//! Every workout stored by the app carries a UUID in its markdown, as an
//! `- id:` bullet directly under the title. Identity therefore travels with
//! the file: re-importing an exported workout updates the original instead of
//! copying it, and a plan sync can recognise the day it scheduled last time —
//! including recognising that you already completed it.
//!
//! Reading is tolerant (a hand-written `.md` needs no id) but writing is not:
//! the stores run [`ensure_id`] so nothing reaches disk without one.

use uuid::Uuid;

/// The bullet key, in the same normalised form `parser` matches on.
const ID_KEY: &str = "id";

pub fn new_id() -> String {
    Uuid::new_v4().to_string()
}

/// Shape check for the canonical 8-4-4-4-12 hyphenated form, in the style of
/// `parser::valid_color`. Deliberately not a full UUID parse: any stable
/// opaque string of the right shape works, and rejecting a variant/version
/// nibble the app itself never mints would only strand user data.
pub fn valid_uuid(s: &str) -> bool {
    let groups: Vec<&str> = s.split('-').collect();
    groups.len() == 5
        && [8, 4, 4, 4, 12] == groups.iter().map(|g| g.len()).collect::<Vec<_>>()[..]
        && groups
            .iter()
            .all(|g| g.bytes().all(|b| b.is_ascii_hexdigit()))
}

/// Split a `- key: value` / `* key: value` bullet, normalising the key.
/// Shared with the parser so the two can't drift on what counts as a bullet
/// or how `Rest After` / `rest-after` / `REST_AFTER` collapse to one key.
pub(crate) fn bullet(line: &str) -> Option<(String, &str)> {
    let trimmed = line.trim();
    let rest = trimmed
        .strip_prefix("- ")
        .or_else(|| trimmed.strip_prefix("* "))?;
    let (key, val) = rest.split_once(':')?;
    Some((key.trim().to_lowercase().replace(['-', '_'], " "), val.trim()))
}

/// Read the id out of a workout document without a full parse.
///
/// Only the preamble is scanned — the lines between the `# Title` and the
/// first `## Block`. An `- id:` inside a block is prose, not identity, which
/// matches how the parser treats every other unrecognised bullet there.
pub fn extract_id(source: &str) -> Option<String> {
    for line in source.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("## ") {
            break;
        }
        if let Some((key, val)) = bullet(line) {
            if key == ID_KEY && valid_uuid(val) {
                return Some(val.to_string());
            }
        }
    }
    None
}

/// Byte offset just past the document's first `# Title` line, i.e. where the
/// id bullet belongs. Falls back to the start for a document with no title
/// (the parser rejects those anyway, but the stores must not panic).
fn after_title(source: &str) -> usize {
    let mut offset = 0;
    for line in source.split_inclusive('\n') {
        if line.trim_start().starts_with("# ") {
            return offset + line.len();
        }
        offset += line.len();
    }
    0
}

/// Splice an id bullet in under the title, leaving the rest of the document
/// byte-for-byte intact. A surgical rewrite rather than a re-serialize, so a
/// user's own formatting, spacing and prose survive — the same reason
/// `store::retitle` edits only the `# ` line.
fn insert_id(source: &str, id: &str) -> String {
    let at = after_title(source);
    let mut out = String::with_capacity(source.len() + 48);
    out.push_str(&source[..at]);
    if at == 0 && !source.is_empty() {
        // No title line: keep the id first rather than losing it.
        out.push_str(&format!("- {ID_KEY}: {id}\n"));
        out.push_str(source);
        return out;
    }
    out.push_str(&format!("- {ID_KEY}: {id}\n"));
    out.push_str(&source[at..]);
    out
}

/// Return the document with a guaranteed id, plus that id. A valid existing id
/// is left alone, so this is safe to call on every write path.
///
/// A *malformed* id bullet is replaced rather than kept: leaving it in place
/// alongside a freshly minted one would make the document fail to parse, which
/// would strand the file for good.
pub fn ensure_id(source: &str) -> (String, String) {
    match extract_id(source) {
        Some(id) => (source.to_string(), id),
        None => with_new_id(source),
    }
}

/// Return the document carrying exactly this id, replacing any it already had.
pub fn set_id(source: &str, id: &str) -> String {
    insert_id(&strip_id(source), id)
}

/// Return the document carrying a *fresh* id, replacing any it already had.
///
/// For copies that are new occurrences rather than the same workout: putting
/// a library template on a date, or promoting a dated entry back into the
/// library. Both would otherwise leave two objects sharing one identity.
pub fn with_new_id(source: &str) -> (String, String) {
    let id = new_id();
    (set_id(source, &id), id)
}

/// Drop the preamble's id bullet, if any.
fn strip_id(source: &str) -> String {
    let mut out = String::with_capacity(source.len());
    let mut in_preamble = true;
    for line in source.split_inclusive('\n') {
        if line.trim_start().starts_with("## ") {
            in_preamble = false;
        }
        if in_preamble {
            if let Some((key, _)) = bullet(line) {
                if key == ID_KEY {
                    continue;
                }
            }
        }
        out.push_str(line);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const PLAIN: &str = "# Squats\n\n## Back Squat\n- work: 1:30\n\nBrace hard.\n";

    #[test]
    fn mints_valid_uuids() {
        let id = new_id();
        assert!(valid_uuid(&id), "{id}");
        assert_ne!(new_id(), new_id());
    }

    #[test]
    fn rejects_non_uuids() {
        for bad in ["", "nope", "1234", &"x".repeat(36), "9f2c8e1a4b7d4c2e9a116f0d3e5b8c74"] {
            assert!(!valid_uuid(bad), "{bad} should be rejected");
        }
    }

    #[test]
    fn ensure_id_inserts_under_the_title_and_preserves_the_rest() {
        let (out, id) = ensure_id(PLAIN);
        assert_eq!(out, format!("# Squats\n- id: {id}\n\n## Back Squat\n- work: 1:30\n\nBrace hard.\n"));
        assert_eq!(extract_id(&out).as_deref(), Some(id.as_str()));
    }

    #[test]
    fn ensure_id_is_idempotent() {
        let (once, id) = ensure_id(PLAIN);
        let (twice, same) = ensure_id(&once);
        assert_eq!(once, twice);
        assert_eq!(id, same);
    }

    #[test]
    fn extract_ignores_an_id_inside_a_block() {
        let src = "# Squats\n\n## Back Squat\n- id: 9f2c8e1a-4b7d-4c2e-9a11-6f0d3e5b8c74\n- work: 30\n";
        assert_eq!(extract_id(src), None);
    }

    #[test]
    fn extract_ignores_a_malformed_id() {
        assert_eq!(extract_id("# Squats\n- id: not-a-uuid\n\n## A\n- work: 30\n"), None);
    }

    #[test]
    fn with_new_id_replaces_rather_than_duplicates() {
        let (first, id1) = ensure_id(PLAIN);
        let (second, id2) = with_new_id(&first);
        assert_ne!(id1, id2);
        assert_eq!(extract_id(&second).as_deref(), Some(id2.as_str()));
        assert_eq!(second.matches("- id:").count(), 1);
    }

    #[test]
    fn tolerates_a_document_with_no_title() {
        let (out, id) = ensure_id("no title here\n");
        assert_eq!(extract_id(&out).as_deref(), Some(id.as_str()));
    }
}
