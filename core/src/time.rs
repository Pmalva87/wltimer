//! Timestamps, in one canonical form.
//!
//! Everything the app stores is RFC 3339, UTC, second precision:
//! `2026-08-09T13:45:31Z`. The point is that lexicographic order is then
//! chronological order, so "which of these two copies is newer" is a string
//! comparison. A device in a different offset — or the same device either side
//! of a DST change — cannot produce a timestamp that sorts wrong.
//!
//! Reading is tolerant: any RFC 3339 input is accepted and converted. That is
//! what migrates `completed_at` values written before this module existed, which
//! carry a local offset and nanosecond precision.
//!
//! There is deliberately no `now()` here. `core` does not read the wall clock —
//! the engine takes `Instant`s from its caller and the stores take timestamps
//! from theirs — so its tests stay deterministic. `chrono` is pulled in without
//! its `clock` feature to keep that honest rather than merely intended.

use chrono::{DateTime, SecondsFormat, Utc};
use std::time::SystemTime;

/// Convert any RFC 3339 timestamp to the canonical form, or `None` if it does
/// not parse.
pub fn canonical(ts: &str) -> Option<String> {
    DateTime::parse_from_rfc3339(ts)
        .ok()
        .map(|t| t.with_timezone(&Utc).to_rfc3339_opts(SecondsFormat::Secs, true))
}

pub fn valid(ts: &str) -> bool {
    DateTime::parse_from_rfc3339(ts).is_ok()
}

/// Canonical form of a filesystem timestamp. The backfill's only source of
/// "when did this last change" for documents written before `updated` existed.
pub fn from_system_time(t: SystemTime) -> Option<String> {
    let secs = t.duration_since(SystemTime::UNIX_EPOCH).ok()?.as_secs();
    DateTime::from_timestamp(secs as i64, 0)
        .map(|t| t.to_rfc3339_opts(SecondsFormat::Secs, true))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn canonicalises_to_utc_seconds() {
        assert_eq!(canonical("2026-08-09T13:45:31Z").unwrap(), "2026-08-09T13:45:31Z");
        // Local offset and sub-second precision are what the old completed_at
        // writer produced; both must collapse to the canonical form.
        assert_eq!(
            canonical("2026-08-09T14:45:31.123456789+01:00").unwrap(),
            "2026-08-09T13:45:31Z"
        );
    }

    #[test]
    fn rejects_what_is_not_a_timestamp() {
        for bad in ["", "nope", "2026-08-09", "2026-13-01T00:00:00Z"] {
            assert!(!valid(bad), "{bad} should be rejected");
        }
    }

    #[test]
    fn canonical_strings_sort_chronologically() {
        // The whole reason for the canonical form: no parsing needed to compare.
        let earlier = canonical("2026-08-09T14:45:31+01:00").unwrap();
        let later = canonical("2026-08-09T14:45:31Z").unwrap();
        assert!(earlier < later, "{earlier} should sort before {later}");
    }

    #[test]
    fn converts_filesystem_times() {
        let t = SystemTime::UNIX_EPOCH + Duration::from_secs(1_770_000_000);
        assert_eq!(from_system_time(t).unwrap(), "2026-02-02T02:40:00Z");
    }
}
