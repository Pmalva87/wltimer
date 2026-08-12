//! Calendar storage: one zstd-compressed JSON file per date, holding the list
//! of workouts planned or done that day. Every entry embeds its own complete
//! markdown copy, so the same workout can live on many days independently.

use crate::ids;
use crate::parser;
use crate::zio;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DayStatus {
    Planned,
    Done,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DayEntry {
    pub markdown: String,
    pub status: DayStatus,
    pub completed_at: Option<String>,
    /// Library template this entry was copied from, if any (informational).
    pub source_slug: Option<String>,
    /// Training plan this entry was scheduled by, if any. Plan syncs replace
    /// still-planned future entries carrying their slug.
    #[serde(default)]
    pub source_plan: Option<String>,
}

/// Lightweight per-entry info for calendar rendering.
#[derive(Debug, Clone, Serialize)]
pub struct DayEntrySummary {
    pub name: String,
    pub status: DayStatus,
}

#[derive(Debug, Clone, Serialize)]
pub struct DaySummary {
    pub date: String,
    pub entries: Vec<DayEntrySummary>,
}

pub fn valid_date(date: &str) -> bool {
    let b = date.as_bytes();
    b.len() == 10
        && b[4] == b'-'
        && b[7] == b'-'
        && [0, 1, 2, 3, 5, 6, 8, 9].iter().all(|&i| b[i].is_ascii_digit())
}

pub fn entry_name(entry: &DayEntry) -> String {
    parser::parse_workout(&entry.markdown)
        .map(|w| w.name)
        .unwrap_or_else(|_| "Workout".into())
}

/// The entry's document id, read from its embedded markdown.
///
/// Identity deliberately lives in the markdown rather than in a `DayEntry`
/// field: `load` silently falls back to an empty day on any deserialize
/// failure, so a JSON schema change that goes wrong looks like a wiped
/// calendar. This also leaves no second copy that could drift.
pub fn entry_id(entry: &DayEntry) -> Option<String> {
    ids::extract_id(&entry.markdown)
}

/// When this entry last changed, canonical. Lives in the markdown for the same
/// reason [`entry_id`] does — a second copy in a `DayEntry` field could drift
/// from the document, and every schema change to that struct risks a day that
/// deserializes to nothing.
pub fn entry_updated(entry: &DayEntry) -> Option<String> {
    ids::extract_updated(&entry.markdown)
}

/// Stamp an entry as changed at `now`. Applied per entry rather than in
/// [`DayStore::save`], which rewrites the whole day: touching one entry must
/// not advance the version of everything else scheduled beside it.
fn stamp_entry(entry: &mut DayEntry, now: &str) {
    entry.markdown = ids::set_updated(&entry.markdown, now);
}

pub struct DayStore {
    dir: PathBuf,
}

impl DayStore {
    pub fn new(dir: PathBuf) -> std::io::Result<Self> {
        fs::create_dir_all(&dir)?;
        let store = DayStore { dir };
        // Captured before the id backfill rewrites the day files.
        let mtimes = store.mtimes();
        store.backfill_ids();
        store.migrate_entries(&mtimes);
        Ok(store)
    }

    /// Last-modified time per date file, canonical, as it stands on disk now.
    fn mtimes(&self) -> std::collections::BTreeMap<String, String> {
        let mut out = std::collections::BTreeMap::new();
        let Ok(entries) = fs::read_dir(&self.dir) else {
            return out;
        };
        for entry in entries.flatten() {
            let name = entry.file_name();
            let Some(date) = name.to_str().and_then(|n| n.strip_suffix(".json.zst")) else {
                continue;
            };
            let stamp = entry
                .metadata()
                .and_then(|m| m.modified())
                .ok()
                .and_then(crate::time::from_system_time);
            if let Some(stamp) = stamp {
                out.insert(date.to_string(), stamp);
            }
        }
        out
    }

    /// One-time migration, two jobs on one pass over the calendar.
    ///
    /// `completed_at` is rewritten canonical. Entries finished before
    /// `time` existed carry a local offset and nanosecond precision, which
    /// sorts wrong against a UTC stamp — the instant is unchanged, only its
    /// spelling.
    ///
    /// Entries with no `updated` inherit one: their `completed_at` when they
    /// have one, since a finished entry knows exactly when it last meant
    /// something different, and otherwise the day file's mtime. Deliberately
    /// *not* the entry's own date — a workout planned for next month would
    /// take a stamp in the future and beat every later edit until that date
    /// went by.
    fn migrate_entries(&self, mtimes: &std::collections::BTreeMap<String, String>) {
        for date in self.dates_from("") {
            let mut entries = self.load(&date);
            let mut changed = false;
            for entry in &mut entries {
                let canonical = entry.completed_at.as_deref().and_then(crate::time::canonical);
                if canonical.is_some() && canonical != entry.completed_at {
                    entry.completed_at = canonical;
                    changed = true;
                }
                if entry_updated(entry).is_some() {
                    continue;
                }
                let inherited = entry
                    .completed_at
                    .clone()
                    .or_else(|| mtimes.get(&date).cloned());
                if let Some(stamp) = inherited {
                    stamp_entry(entry, &stamp);
                    changed = true;
                }
            }
            if changed {
                let _ = self.save(&date, &entries);
            }
        }
    }

    /// One-time migration: entries scheduled before ids existed get one, so a
    /// plan sync can recognise them instead of scheduling a duplicate copy.
    fn backfill_ids(&self) {
        for date in self.dates_from("") {
            let mut entries = self.load(&date);
            let mut changed = false;
            for entry in &mut entries {
                if entry_id(entry).is_none() {
                    entry.markdown = ids::ensure_id(&entry.markdown).0;
                    changed = true;
                }
            }
            if changed {
                let _ = self.save(&date, &entries);
            }
        }
    }

    fn path(&self, date: &str) -> PathBuf {
        self.dir.join(format!("{date}.json.zst"))
    }

    pub fn load(&self, date: &str) -> Vec<DayEntry> {
        zio::read_text(&self.path(date))
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default()
    }

    pub fn save(&self, date: &str, entries: &[DayEntry]) -> Result<(), String> {
        if entries.is_empty() {
            let _ = fs::remove_file(self.path(date));
            return Ok(());
        }
        let json = serde_json::to_string(entries).map_err(|e| e.to_string())?;
        zio::write_compressed(&self.path(date), json.as_bytes())
            .map_err(|e| format!("cannot save day '{date}': {e}"))
    }

    /// Append an entry; returns its index within the day.
    pub fn add(&self, date: &str, mut entry: DayEntry, now: &str) -> Result<usize, String> {
        stamp_entry(&mut entry, now);
        let mut entries = self.load(date);
        entries.push(entry);
        self.save(date, &entries)?;
        Ok(entries.len() - 1)
    }

    pub fn update<F>(&self, date: &str, index: usize, now: &str, f: F) -> Result<(), String>
    where
        F: FnOnce(&mut DayEntry),
    {
        let mut entries = self.load(date);
        let entry = entries
            .get_mut(index)
            .ok_or_else(|| format!("no entry {index} on {date}"))?;
        f(entry);
        stamp_entry(entry, now);
        self.save(date, &entries)
    }

    pub fn delete(&self, date: &str, index: usize) -> Result<(), String> {
        let mut entries = self.load(date);
        if index >= entries.len() {
            return Err(format!("no entry {index} on {date}"));
        }
        entries.remove(index);
        self.save(date, &entries)
    }

    /// Move an entry (with its status/metadata) from one date to another;
    /// returns its index within the destination day.
    pub fn move_entry(
        &self,
        from_date: &str,
        index: usize,
        to_date: &str,
        now: &str,
    ) -> Result<usize, String> {
        // Moving an entry to the day it is already on changes nothing: doing
        // it the long way would shuffle it to the end of the day and stamp it
        // as edited, which a plan sync would then have to explain.
        if from_date == to_date {
            return if index < self.load(from_date).len() {
                Ok(index)
            } else {
                Err(format!("no entry {index} on {from_date}"))
            };
        }
        let mut from = self.load(from_date);
        if index >= from.len() {
            return Err(format!("no entry {index} on {from_date}"));
        }
        let mut entry = from.remove(index);
        // The document did not change, but when it is scheduled did — and that
        // is part of what a sync has to carry.
        stamp_entry(&mut entry, now);
        let mut to = self.load(to_date);
        to.push(entry);
        // Write destination first so a failure can't lose the entry.
        self.save(to_date, &to)?;
        self.save(from_date, &from)?;
        Ok(to.len() - 1)
    }

    /// All stored dates on or after `from`, ascending.
    pub fn dates_from(&self, from: &str) -> Vec<String> {
        let mut out = Vec::new();
        let Ok(entries) = fs::read_dir(&self.dir) else {
            return out;
        };
        for entry in entries.flatten() {
            let name = entry.file_name();
            let name = name.to_str().unwrap_or("");
            if let Some(date) = name.strip_suffix(".json.zst") {
                if valid_date(date) && date >= from {
                    out.push(date.to_string());
                }
            }
        }
        out.sort();
        out
    }

    /// Summaries for every stored date in the given month.
    pub fn month(&self, year: i32, month: u32) -> Vec<DaySummary> {
        let prefix = format!("{year:04}-{month:02}-");
        let mut out = Vec::new();
        let Ok(entries) = fs::read_dir(&self.dir) else {
            return out;
        };
        for entry in entries.flatten() {
            let name = entry.file_name();
            let name = name.to_str().unwrap_or("");
            let Some(date) = name.strip_suffix(".json.zst") else {
                continue;
            };
            if !date.starts_with(&prefix) || !valid_date(date) {
                continue;
            }
            let summaries = self
                .load(date)
                .iter()
                .map(|e| DayEntrySummary {
                    name: entry_name(e),
                    status: e.status,
                })
                .collect();
            out.push(DaySummary {
                date: date.to_string(),
                entries: summaries,
            });
        }
        out.sort_by(|a, b| a.date.cmp(&b.date));
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(md: &str) -> DayEntry {
        DayEntry {
            markdown: md.into(),
            status: DayStatus::Planned,
            completed_at: None,
            source_slug: None,
            source_plan: None,
        }
    }

    fn temp_store(tag: &str) -> DayStore {
        let dir = std::env::temp_dir().join(format!("wltimer-days-{tag}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        DayStore::new(dir).unwrap()
    }

    const MD: &str = "# Squats\n\n## A\n- work: 30\n";
    const NOW: &str = "2026-08-09T13:45:31Z";
    const LATER: &str = "2026-08-10T09:00:00Z";

    #[test]
    fn entry_id_reads_the_embedded_document_id() {
        let (md, id) = ids::ensure_id(MD);
        assert_eq!(entry_id(&entry(&md)).as_deref(), Some(id.as_str()));
        assert_eq!(entry_id(&entry(MD)), None);
    }

    #[test]
    fn backfills_ids_for_entries_scheduled_before_ids_existed() {
        let dir = std::env::temp_dir().join(format!("wltimer-days-bf-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        let s = DayStore::new(dir.clone()).unwrap();
        s.add("2026-07-28", entry(MD), NOW).unwrap();
        s.add("2026-07-28", entry(MD), NOW).unwrap();
        assert!(s.load("2026-07-28").iter().all(|e| entry_id(e).is_none()));

        // Re-opening the store migrates what is already on disk.
        let s = DayStore::new(dir).unwrap();
        let entries = s.load("2026-07-28");
        let a = entry_id(&entries[0]).expect("backfilled");
        let b = entry_id(&entries[1]).expect("backfilled");
        assert_ne!(a, b, "each entry is its own occurrence");
    }

    #[test]
    fn add_load_update_delete() {
        let s = temp_store("crud");
        assert_eq!(s.add("2026-07-28", entry(MD), NOW).unwrap(), 0);
        assert_eq!(s.add("2026-07-28", entry(MD), NOW).unwrap(), 1);
        assert_eq!(s.load("2026-07-28").len(), 2);
        s.update("2026-07-28", 1, NOW, |e| {
            e.status = DayStatus::Done;
            e.completed_at = Some("2026-07-28T18:00:00Z".into());
        })
        .unwrap();
        assert_eq!(s.load("2026-07-28")[1].status, DayStatus::Done);
        s.delete("2026-07-28", 0).unwrap();
        assert_eq!(s.load("2026-07-28").len(), 1);
        // Deleting the last entry removes the file entirely.
        s.delete("2026-07-28", 0).unwrap();
        assert!(s.load("2026-07-28").is_empty());
        assert!(!s.path("2026-07-28").exists());
    }

    #[test]
    fn move_between_days() {
        let s = temp_store("move");
        s.add("2026-07-30", entry(MD), NOW).unwrap();
        s.add("2026-07-28", entry(MD), NOW).unwrap();
        let index = s.move_entry("2026-07-28", 0, "2026-07-30", LATER).unwrap();
        assert!(s.load("2026-07-28").is_empty());
        let moved = s.load("2026-07-30");
        assert_eq!(moved.len(), 2);
        // The index is where the entry landed, so the caller can go on
        // working with it — that is how a finished run marks itself done.
        assert_eq!(index, 1);
        assert!(moved[index].markdown.contains("- work: 30"));
        // Rescheduling is a change, so the entry's version moves with it.
        assert_eq!(entry_updated(&moved[index]).as_deref(), Some(LATER));
    }

    #[test]
    fn moving_to_the_same_day_leaves_the_entry_where_it_is() {
        let s = temp_store("move-noop");
        s.add("2026-07-28", entry(MD), NOW).unwrap();
        s.add("2026-07-28", entry(MD), NOW).unwrap();
        assert_eq!(s.move_entry("2026-07-28", 0, "2026-07-28", LATER), Ok(0));
        let entries = s.load("2026-07-28");
        assert_eq!(entries.len(), 2);
        // Neither reordered nor stamped as edited.
        assert_eq!(entry_updated(&entries[0]).as_deref(), Some(NOW));
        assert!(s.move_entry("2026-07-28", 2, "2026-07-28", LATER).is_err());
    }

    #[test]
    fn touching_one_entry_leaves_its_neighbours_versions_alone() {
        // The day file is rewritten whole, so this is the easy thing to get
        // wrong: editing one entry must not look like every entry changed.
        let s = temp_store("neighbours");
        s.add("2026-07-28", entry(MD), NOW).unwrap();
        s.add("2026-07-28", entry(MD), NOW).unwrap();
        s.update("2026-07-28", 1, LATER, |e| e.status = DayStatus::Done).unwrap();

        let entries = s.load("2026-07-28");
        assert_eq!(entry_updated(&entries[0]).as_deref(), Some(NOW));
        assert_eq!(entry_updated(&entries[1]).as_deref(), Some(LATER));
    }

    #[test]
    fn backfills_updated_from_completed_at_then_from_the_file() {
        let dir = std::env::temp_dir().join(format!("wltimer-days-bfu-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        // Written straight to disk so the entries arrive unstamped, as they
        // would have been before `updated` existed.
        let done = DayEntry {
            markdown: MD.into(),
            status: DayStatus::Done,
            // Local offset and sub-second precision: the old writer's format.
            completed_at: Some("2026-07-28T19:00:00.500+01:00".into()),
            source_slug: None,
            source_plan: None,
        };
        let planned = entry(MD);
        let path = dir.join("2026-07-28.json.zst");
        let json = serde_json::to_string(&[done, planned]).unwrap();
        crate::zio::write_compressed(&path, json.as_bytes()).unwrap();
        let mtime =
            crate::time::from_system_time(fs::metadata(&path).unwrap().modified().unwrap()).unwrap();

        let s = DayStore::new(dir).unwrap();
        let entries = s.load("2026-07-28");
        assert_eq!(
            entry_updated(&entries[0]).as_deref(),
            Some("2026-07-28T18:00:00Z"),
            "a finished entry knows when it last meant something different"
        );
        assert_eq!(
            entry_updated(&entries[1]).as_deref(),
            Some(mtime.as_str()),
            "a planned entry falls back to the day file"
        );
        assert_eq!(
            entries[0].completed_at.as_deref(),
            Some("2026-07-28T18:00:00Z"),
            "the stored completed_at is rewritten canonical too"
        );
    }

    #[test]
    fn migration_normalises_completed_at_without_moving_the_instant() {
        let dir = std::env::temp_dir().join(format!("wltimer-days-norm-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        // Already stamped, so only the completed_at rewrite is under test.
        let mut done = entry(&ids::set_updated(MD, NOW));
        done.status = DayStatus::Done;
        done.completed_at = Some("2026-07-28T19:00:00.500+01:00".into());
        let json = serde_json::to_string(&[done]).unwrap();
        crate::zio::write_compressed(&dir.join("2026-07-28.json.zst"), json.as_bytes()).unwrap();

        let s = DayStore::new(dir).unwrap();
        let entries = s.load("2026-07-28");
        assert_eq!(entries[0].completed_at.as_deref(), Some("2026-07-28T18:00:00Z"));
        assert_eq!(entry_updated(&entries[0]).as_deref(), Some(NOW), "left alone");
    }

    #[test]
    fn backfill_never_stamps_a_planned_entry_into_the_future() {
        // The trap: a workout planned for next month must not inherit its own
        // date, or the first real edit would lose to a stamp that has not
        // happened yet.
        let dir = std::env::temp_dir().join(format!("wltimer-days-fut-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let json = serde_json::to_string(&[entry(MD)]).unwrap();
        let path = dir.join("2099-01-01.json.zst");
        crate::zio::write_compressed(&path, json.as_bytes()).unwrap();
        let mtime =
            crate::time::from_system_time(fs::metadata(&path).unwrap().modified().unwrap()).unwrap();

        let s = DayStore::new(dir).unwrap();
        let stamped = entry_updated(&s.load("2099-01-01")[0]).expect("backfilled");
        assert_eq!(stamped, mtime);
        assert!(stamped.as_str() < "2099-01-01T00:00:00Z", "{stamped}");
    }

    #[test]
    fn month_summaries() {
        let s = temp_store("month");
        s.add("2026-07-28", entry(MD), NOW).unwrap();
        s.add("2026-07-02", entry(MD), NOW).unwrap();
        s.add("2026-08-01", entry(MD), NOW).unwrap();
        let july = s.month(2026, 7);
        assert_eq!(july.len(), 2);
        assert_eq!(july[0].date, "2026-07-02");
        assert_eq!(july[1].entries[0].name, "Squats");
    }

    #[test]
    fn validates_dates() {
        assert!(valid_date("2026-07-28"));
        assert!(!valid_date("2026-7-28"));
        assert!(!valid_date("not-a-date!"));
        assert!(!valid_date("2026-07-28x"));
    }
}
