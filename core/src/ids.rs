//! Workout document identity and version.
//!
//! Every workout stored by the app carries two preamble bullets under its
//! title: a UUID as `- id:`, and the time it last changed as `- updated:`.
//! Both therefore travel with the file rather than with its name or its
//! filesystem metadata — which is what makes them survive an export to a cloud
//! drive, a mail attachment or a paste through the clipboard.
//!
//! Identity answers "is this the same workout": re-importing an exported
//! document updates the original instead of copying it, and a plan sync can
//! recognise the day it scheduled last time, including recognising that you
//! already completed it. Version answers the question identity cannot — "is
//! this copy newer than the one I already have" — which is what any sync or
//! re-import needs in order to pick a winner instead of clobbering blindly.
//!
//! Reading is tolerant (a hand-written `.md` needs neither bullet) but writing
//! is not: the stores run [`ensure_id`] and [`set_updated`] so nothing reaches
//! disk without both.

use crate::time;
use uuid::Uuid;

/// The bullet keys, in the same normalised form `parser` matches on.
const ID_KEY: &str = "id";
pub(crate) const UPDATED_KEY: &str = "updated";

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

/// Read a preamble bullet's value, without a full parse.
///
/// Only the preamble is scanned — the lines between the `# Title` and the
/// first `## Block`. An `- id:` inside a block is prose, not identity, which
/// matches how the parser treats every other unrecognised bullet there.
///
/// `accept` both validates and normalises: a value it rejects is treated as
/// absent rather than as an error, because neither bullet is load-bearing
/// enough to be worth stranding a file over.
fn extract(source: &str, key: &str, accept: impl Fn(&str) -> Option<String>) -> Option<String> {
    for line in source.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("## ") {
            break;
        }
        if let Some((k, val)) = bullet(line) {
            if k == key {
                if let Some(accepted) = accept(val) {
                    return Some(accepted);
                }
            }
        }
    }
    None
}

pub fn extract_id(source: &str) -> Option<String> {
    extract(source, ID_KEY, |v| valid_uuid(v).then(|| v.to_string()))
}

/// The document's last-changed time, in canonical form — so two documents can
/// be compared as strings whatever form their bullets were written in.
pub fn extract_updated(source: &str) -> Option<String> {
    extract(source, UPDATED_KEY, time::canonical)
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

/// Where a new preamble bullet belongs: after the title, and after any run of
/// bullets already sitting under it. Appending to that run rather than cutting
/// in at the top keeps `- id:` first, which is the order the format guide and
/// the README document.
fn insert_point(source: &str) -> usize {
    let mut offset = after_title(source);
    if offset == 0 {
        return 0;
    }
    for line in source[offset..].split_inclusive('\n') {
        if bullet(line).is_none() {
            break;
        }
        offset += line.len();
    }
    offset
}

/// Splice a bullet in under the title, leaving the rest of the document
/// byte-for-byte intact. A surgical rewrite rather than a re-serialize, so a
/// user's own formatting, spacing and prose survive — the same reason
/// `store::retitle` edits only the `# ` line.
fn insert(source: &str, key: &str, val: &str) -> String {
    let at = insert_point(source);
    let mut out = String::with_capacity(source.len() + 48);
    if at == 0 && !source.is_empty() {
        // No title line: keep the bullet first rather than losing it.
        out.push_str(&format!("- {key}: {val}\n"));
        out.push_str(source);
        return out;
    }
    out.push_str(&source[..at]);
    out.push_str(&format!("- {key}: {val}\n"));
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
    insert(&strip(source, ID_KEY), ID_KEY, id)
}

/// Stamp the document as changed at `now`, replacing any previous stamp.
///
/// Called on every write path, so `now` is the store's single say on what time
/// it is — `core` never asks the clock itself. A value that is not RFC 3339 is
/// dropped rather than written: a malformed stamp would read as "no version at
/// all" on the next load, which is worse than the stamp simply not moving.
pub fn set_updated(source: &str, now: &str) -> String {
    let stripped = strip(source, UPDATED_KEY);
    match time::canonical(now) {
        Some(ts) => insert(&stripped, UPDATED_KEY, &ts),
        None => stripped,
    }
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

/// Drop the preamble's bullet for this key, if any.
fn strip(source: &str, key: &str) -> String {
    let mut out = String::with_capacity(source.len());
    let mut in_preamble = true;
    for line in source.split_inclusive('\n') {
        if line.trim_start().starts_with("## ") {
            in_preamble = false;
        }
        if in_preamble {
            if let Some((k, _)) = bullet(line) {
                if k == key {
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

    const TS: &str = "2026-08-09T13:45:31Z";

    #[test]
    fn updated_lands_after_the_id_rather_than_above_it() {
        let (with_id, id) = ensure_id(PLAIN);
        let out = set_updated(&with_id, TS);
        assert_eq!(
            out,
            format!("# Squats\n- id: {id}\n- updated: {TS}\n\n## Back Squat\n- work: 1:30\n\nBrace hard.\n")
        );
    }

    #[test]
    fn set_updated_replaces_rather_than_duplicates() {
        let once = set_updated(PLAIN, TS);
        let twice = set_updated(&once, "2026-08-10T09:00:00Z");
        assert_eq!(twice.matches("- updated:").count(), 1);
        assert_eq!(extract_updated(&twice).as_deref(), Some("2026-08-10T09:00:00Z"));
    }

    #[test]
    fn extract_updated_canonicalises_whatever_form_it_finds() {
        // A hand-written file may carry any RFC 3339 form; comparisons happen
        // on strings, so reading has to normalise or the ordering is a lie.
        let src = "# W\n- updated: 2026-08-09T14:45:31.123+01:00\n\n## A\n- work: 30\n";
        assert_eq!(extract_updated(src).as_deref(), Some(TS));
    }

    #[test]
    fn a_malformed_updated_reads_as_absent_and_is_replaced() {
        let src = "# W\n- updated: yesterday\n\n## A\n- work: 30\n";
        assert_eq!(extract_updated(src), None);
        let fixed = set_updated(src, TS);
        assert_eq!(fixed.matches("- updated:").count(), 1);
        assert_eq!(extract_updated(&fixed).as_deref(), Some(TS));
    }

    #[test]
    fn an_unparseable_now_leaves_no_stamp_rather_than_a_bad_one() {
        let out = set_updated(&set_updated(PLAIN, TS), "not a time");
        assert_eq!(extract_updated(&out), None);
        assert!(!out.contains("- updated:"), "{out}");
    }

    #[test]
    fn extract_updated_ignores_a_stamp_inside_a_block() {
        let src = format!("# W\n\n## A\n- updated: {TS}\n- work: 30\n");
        assert_eq!(extract_updated(&src), None);
    }

    #[test]
    fn id_and_updated_do_not_disturb_each_other() {
        let stamped = set_updated(PLAIN, TS);
        let (both, id) = ensure_id(&stamped);
        assert_eq!(extract_id(&both).as_deref(), Some(id.as_str()));
        assert_eq!(extract_updated(&both).as_deref(), Some(TS));

        // Re-minting identity must not cost the document its version.
        let (fresh, new) = with_new_id(&both);
        assert_ne!(new, id);
        assert_eq!(extract_updated(&fresh).as_deref(), Some(TS));
        assert_eq!(fresh.matches("- id:").count(), 1);
    }
}
