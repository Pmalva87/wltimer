//! Library of workout templates: one zstd-compressed markdown file per workout.

use crate::ids;
use crate::parser;
use crate::zio;
use serde::Serialize;
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Serialize, Clone)]
pub struct WorkoutSummary {
    pub slug: String,
    pub name: String,
    pub block_count: usize,
    pub total_secs: u32,
    /// How many parts run straight into the next one — see
    /// `Workout::parts_without_rest_after`.
    pub parts_without_rest: usize,
    /// Set when the stored file no longer parses (e.g. edited externally).
    pub error: Option<String>,
}

pub fn slugify(name: &str) -> String {
    let mut out = String::new();
    let mut prev_dash = true;
    for c in name.chars() {
        if c.is_ascii_alphanumeric() {
            out.push(c.to_ascii_lowercase());
            prev_dash = false;
        } else if !prev_dash {
            out.push('-');
            prev_dash = true;
        }
    }
    let out = out.trim_end_matches('-').to_string();
    if out.is_empty() { "workout".into() } else { out }
}

/// Replace the document's `# Title` line with the given name.
pub(crate) fn retitle(source: &str, name: &str) -> String {
    let mut done = false;
    source
        .lines()
        .map(|line| {
            if !done && line.trim().starts_with("# ") {
                done = true;
                format!("# {name}")
            } else {
                line.to_string()
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
        + "\n"
}

/// Slug of a stored workout file, compressed or legacy plain. `None` for
/// anything else in the directory. Shared by every scan so they cannot
/// disagree about what counts as a stored file.
fn slug_of(path: &Path) -> Option<String> {
    let name = path.file_name().and_then(|n| n.to_str())?;
    let slug = name
        .strip_suffix(".md.zst")
        .or_else(|| name.strip_suffix(".md"))?;
    (!slug.is_empty()).then(|| slug.to_string())
}

pub struct Store {
    dir: PathBuf,
}

impl Store {
    pub fn new(dir: PathBuf) -> std::io::Result<Self> {
        fs::create_dir_all(&dir)?;
        let store = Store { dir };
        // Read before either migration runs. Both rewrite files, and their own
        // mtime would otherwise become every legacy document's "last changed" —
        // stamping the whole library with the moment of the upgrade.
        let mtimes = store.mtimes();
        store.migrate_plain_files();
        store.backfill_ids();
        store.backfill_updated(&mtimes);
        Ok(store)
    }

    /// Last-modified time per slug, canonical, as it stands on disk right now.
    fn mtimes(&self) -> BTreeMap<String, String> {
        let Ok(entries) = fs::read_dir(&self.dir) else {
            return BTreeMap::new();
        };
        let mut out = BTreeMap::new();
        for entry in entries.flatten() {
            let Some(slug) = slug_of(&entry.path()) else {
                continue;
            };
            let stamp = entry
                .metadata()
                .and_then(|m| m.modified())
                .ok()
                .and_then(crate::time::from_system_time);
            if let Some(stamp) = stamp {
                out.insert(slug, stamp);
            }
        }
        out
    }

    /// One-time migration: plain `.md` files become `.md.zst`.
    fn migrate_plain_files(&self) {
        let Ok(entries) = fs::read_dir(&self.dir) else {
            return;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
            if let Some(slug) = name.strip_suffix(".md") {
                if let Ok(source) = zio::read_text(&path) {
                    if self.write(slug, &source).is_ok() {
                        let _ = fs::remove_file(&path);
                    }
                }
            }
        }
    }

    /// One-time migration: workouts stored before ids existed get one, so
    /// every read path afterwards can assume identity is present. Runs after
    /// `migrate_plain_files` so it only ever rewrites the compressed form.
    fn backfill_ids(&self) {
        for (slug, source) in self.stored() {
            if ids::extract_id(&source).is_none() {
                let (with_id, _) = ids::ensure_id(&source);
                let _ = self.write(&slug, &with_id);
            }
        }
    }

    /// One-time migration: workouts stored before `updated` existed inherit
    /// their file's mtime, which is the only record on disk of when they last
    /// changed. A file whose mtime cannot be read is left unstamped rather
    /// than given a guessed one — absent reads as "oldest", and losing a
    /// comparison is a better failure than winning one on invented evidence.
    fn backfill_updated(&self, mtimes: &BTreeMap<String, String>) {
        for (slug, source) in self.stored() {
            if ids::extract_updated(&source).is_some() {
                continue;
            }
            if let Some(stamp) = mtimes.get(&slug) {
                let _ = self.write(&slug, &ids::set_updated(&source, stamp));
            }
        }
    }

    /// Every stored workout as `(slug, source)`, keyed so the compressed and
    /// legacy plain form of one slug collapse to a single entry. Shared by
    /// `list`, `find_by_id` and `backfill_ids` so they cannot disagree about
    /// what counts as stored.
    fn stored(&self) -> Vec<(String, String)> {
        let Ok(entries) = fs::read_dir(&self.dir) else {
            return Vec::new();
        };
        let mut by_slug: BTreeMap<String, String> = BTreeMap::new();
        for entry in entries.flatten() {
            let path = entry.path();
            let Some(slug) = slug_of(&path) else {
                continue;
            };
            if let Ok(source) = zio::read_text(&path) {
                by_slug.insert(slug, source);
            }
        }
        by_slug.into_iter().collect()
    }

    /// Slug of the stored workout carrying this id, if any.
    pub fn find_by_id(&self, id: &str) -> Option<String> {
        self.stored()
            .into_iter()
            .find(|(_, source)| ids::extract_id(source).as_deref() == Some(id))
            .map(|(slug, _)| slug)
    }

    fn path(&self, slug: &str) -> PathBuf {
        self.dir.join(format!("{slug}.md.zst"))
    }

    fn write(&self, slug: &str, source: &str) -> std::io::Result<()> {
        zio::write_compressed(&self.path(slug), source.as_bytes())
    }

    pub fn list(&self) -> Vec<WorkoutSummary> {
        let mut out: Vec<WorkoutSummary> = self
            .stored()
            .into_iter()
            .map(|(slug, source)| match parser::parse_workout(&source) {
                Ok(w) => WorkoutSummary {
                    slug,
                    name: w.name.clone(),
                    block_count: w.blocks.len(),
                    total_secs: w.total_secs(),
                    parts_without_rest: w.parts_without_rest_after().len(),
                    error: None,
                },
                Err(errs) => WorkoutSummary {
                    name: slug.clone(),
                    slug,
                    block_count: 0,
                    total_secs: 0,
                    parts_without_rest: 0,
                    error: Some(format!("line {}: {}", errs[0].line, errs[0].message)),
                },
            })
            .collect();
        out.sort_by_key(|s| s.name.to_lowercase());
        out
    }

    pub fn read_source(&self, slug: &str) -> Result<String, String> {
        zio::read_text(&self.path(slug))
            .or_else(|_| zio::read_text(&self.dir.join(format!("{slug}.md"))))
            .map_err(|e| format!("cannot read '{slug}': {e}"))
    }

    fn slug_taken(&self, slug: &str) -> bool {
        self.path(slug).exists() || self.dir.join(format!("{slug}.md")).exists()
    }

    /// Validate and write. Returns parse errors if the source is invalid.
    ///
    /// The document's id is the real handle: saving a workout the library
    /// already holds updates it in place, wherever it came from — so
    /// re-importing an exported `.md` edits the original instead of adding
    /// "Squats (2)". Only a genuinely new workout whose *name* collides gets
    /// the counter: "Squats" → "Squats (2)" (retitling the document to match).
    /// When the name changed, the old file is removed.
    ///
    /// `now` stamps the document as changed; `core` never reads the clock, so
    /// the caller says what time it is.
    pub fn save(
        &self,
        source: &str,
        prev_slug: Option<&str>,
        now: &str,
    ) -> Result<WorkoutSummary, Vec<parser::ParseError>> {
        let workout = parser::parse_workout(source)?;
        let (source, id) = ids::ensure_id(source);
        let owner = self.find_by_id(&id);

        // Two files must never claim one identity. If this document carries the
        // id of a *different* stored workout, it is a copy of it — pasting one
        // workout's markdown into another's editor — so it gets its own.
        let source = match (prev_slug, owner.as_deref()) {
            (Some(prev), Some(other)) if other != prev => ids::with_new_id(&source).0,
            _ => source,
        };
        let prev_slug = prev_slug.or(owner.as_deref());

        let source = source.as_str();
        let mut name = workout.name.clone();
        let mut slug = slugify(&name);
        if prev_slug != Some(slug.as_str()) {
            let mut n = 2;
            while self.slug_taken(&slug) {
                name = format!("{} ({n})", workout.name);
                slug = slugify(&name);
                n += 1;
            }
        }
        let source = if name == workout.name {
            source.to_string()
        } else {
            retitle(source, &name)
        };
        let source = ids::set_updated(&source, now);
        self.write(&slug, &source).map_err(|e| {
            vec![parser::ParseError { line: 1, message: format!("cannot save: {e}") }]
        })?;
        if let Some(prev) = prev_slug {
            if prev != slug {
                let _ = self.delete(prev);
            }
        }
        Ok(WorkoutSummary {
            slug,
            name,
            block_count: workout.blocks.len(),
            total_secs: workout.total_secs(),
            parts_without_rest: workout.parts_without_rest_after().len(),
            error: None,
        })
    }

    pub fn delete(&self, slug: &str) -> Result<(), String> {
        let zst = fs::remove_file(self.path(slug));
        let plain = fs::remove_file(self.dir.join(format!("{slug}.md")));
        if zst.is_err() && plain.is_err() {
            return Err(format!("cannot delete '{slug}'"));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const NOW: &str = "2026-08-09T13:45:31Z";

    fn temp_store(tag: &str) -> Store {
        let dir = std::env::temp_dir().join(format!("wltimer-store-{tag}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        Store::new(dir).unwrap()
    }

    #[test]
    fn starts_empty() {
        let s = temp_store("empty");
        assert!(s.list().is_empty());
    }

    #[test]
    fn save_rename_delete() {
        let s = temp_store("crud");
        let sum = s.save("# My Day\n\n## A\n- work: 30\n", None, NOW).unwrap();
        assert_eq!(sum.slug, "my-day");
        let renamed = s.save("# Other Day\n\n## A\n- work: 30\n", Some("my-day"), NOW).unwrap();
        assert_eq!(renamed.slug, "other-day");
        assert!(s.read_source("my-day").is_err());
        s.delete("other-day").unwrap();
        assert!(s.read_source("other-day").is_err());
    }

    #[test]
    fn duplicate_names_get_a_counter() {
        let s = temp_store("dup");
        let src = "# Squats\n\n## A\n- work: 30\n";
        assert_eq!(s.save(src, None, NOW).unwrap().slug, "squats");
        let second = s.save(src, None, NOW).unwrap();
        assert_eq!(second.slug, "squats-2");
        assert_eq!(second.name, "Squats (2)");
        assert!(s.read_source("squats-2").unwrap().starts_with("# Squats (2)\n"));
        assert_eq!(s.save(src, None, NOW).unwrap().slug, "squats-3");
        // Re-saving an existing workout under its own name is not a collision.
        let resaved = s.save(src, Some("squats"), NOW).unwrap();
        assert_eq!(resaved.slug, "squats");
        assert_eq!(resaved.name, "Squats");
    }

    #[test]
    fn saving_gives_the_document_an_id() {
        let s = temp_store("mint");
        let sum = s.save("# Squats\n\n## A\n- work: 30\n", None, NOW).unwrap();
        let stored = s.read_source(&sum.slug).unwrap();
        assert!(ids::extract_id(&stored).is_some(), "{stored}");
        assert!(stored.starts_with("# Squats\n- id: "), "{stored}");
    }

    #[test]
    fn re_saving_a_document_with_a_known_id_updates_in_place() {
        // The re-import case: export a workout, edit it elsewhere, upload it
        // again. It must edit the original, not pile up "Squats (2)".
        let s = temp_store("byid");
        let first = s.save("# Squats\n\n## A\n- work: 30\n", None, NOW).unwrap();
        let exported = s.read_source(&first.slug).unwrap();
        let edited = exported.replace("- work: 30", "- work: 45");

        let again = s.save(&edited, None, NOW).unwrap();
        assert_eq!(again.slug, "squats");
        assert_eq!(s.list().len(), 1, "must not have created a second workout");
        assert!(s.read_source("squats").unwrap().contains("- work: 45"));
    }

    #[test]
    fn a_renamed_document_with_a_known_id_is_renamed_not_copied() {
        let s = temp_store("byidrename");
        let first = s.save("# Squats\n\n## A\n- work: 30\n", None, NOW).unwrap();
        let exported = s.read_source(&first.slug).unwrap();

        let renamed = s.save(&exported.replace("# Squats", "# Front Squats"), None, NOW).unwrap();
        assert_eq!(renamed.slug, "front-squats");
        assert_eq!(renamed.name, "Front Squats");
        assert_eq!(s.list().len(), 1);
        assert!(s.read_source("squats").is_err(), "old slug should be gone");
    }

    #[test]
    fn an_explicit_copy_needs_a_fresh_id_to_become_a_second_workout() {
        // What the Duplicate button does. Re-saving the source as-is would
        // match the original by id and update it, so the copy must be minted a
        // new identity first — this pins that contract.
        let s = temp_store("copy");
        let first = s.save("# Squats\n\n## A\n- work: 30\n", None, NOW).unwrap();
        let source = s.read_source(&first.slug).unwrap();

        // As-is: an update, not a copy.
        s.save(&source, None, NOW).unwrap();
        assert_eq!(s.list().len(), 1);

        // With a fresh id: a genuine second workout.
        let copy = s.save(&ids::with_new_id(&source).0, None, NOW).unwrap();
        assert_eq!(copy.slug, "squats-2");
        assert_eq!(copy.name, "Squats (2)");
        assert_eq!(s.list().len(), 2);
    }

    #[test]
    fn an_id_belonging_to_another_workout_is_not_stolen() {
        // Pasting one workout's markdown into another's editor must not leave
        // two stored files claiming the same identity.
        let s = temp_store("steal");
        let a = s.save("# A\n\n## X\n- work: 30\n", None, NOW).unwrap();
        let b = s.save("# B\n\n## X\n- work: 30\n", None, NOW).unwrap();
        let a_src = s.read_source(&a.slug).unwrap();

        s.save(&a_src.replace("# A", "# B"), Some(&b.slug), NOW).unwrap();
        let a_id = ids::extract_id(&s.read_source(&a.slug).unwrap()).unwrap();
        let b_id = ids::extract_id(&s.read_source(&b.slug).unwrap()).unwrap();
        assert_ne!(a_id, b_id);
    }

    #[test]
    fn backfills_ids_for_workouts_stored_before_ids_existed() {
        let dir = std::env::temp_dir().join(format!("wltimer-store-bf-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        zio::write_compressed(
            &dir.join("old.md.zst"),
            b"# Old\n\n## A\n- work: 30\n",
        )
        .unwrap();

        let s = Store::new(dir).unwrap();
        let stored = s.read_source("old").unwrap();
        let id = ids::extract_id(&stored).expect("backfill should have minted an id");
        assert_eq!(s.find_by_id(&id).as_deref(), Some("old"));
    }

    #[test]
    fn saving_stamps_the_document_and_moves_the_stamp_on_each_save() {
        let s = temp_store("stamp");
        let sum = s.save("# Squats\n\n## A\n- work: 30\n", None, NOW).unwrap();
        let stored = s.read_source(&sum.slug).unwrap();
        assert_eq!(ids::extract_updated(&stored).as_deref(), Some(NOW));

        let later = "2026-08-10T09:00:00Z";
        s.save(&stored, Some(&sum.slug), later).unwrap();
        let stored = s.read_source(&sum.slug).unwrap();
        assert_eq!(ids::extract_updated(&stored).as_deref(), Some(later));
        assert_eq!(stored.matches("- updated:").count(), 1);
    }

    #[test]
    fn backfills_updated_from_the_file_rather_than_from_the_migration() {
        // The trap this pins: backfill_ids rewrites every legacy file, so a
        // naive backfill would stamp the whole library with the upgrade time
        // and throw away the only evidence of when things actually changed.
        let dir = std::env::temp_dir().join(format!("wltimer-store-bfu-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("old.md.zst");
        zio::write_compressed(&path, b"# Old\n\n## A\n- work: 30\n").unwrap();
        let mtime = crate::time::from_system_time(
            fs::metadata(&path).unwrap().modified().unwrap(),
        )
        .unwrap();

        let s = Store::new(dir).unwrap();
        let stored = s.read_source("old").unwrap();
        assert_eq!(
            ids::extract_updated(&stored).as_deref(),
            Some(mtime.as_str()),
            "should carry the original file's mtime"
        );
        // And the id backfill still happened, on the same document.
        assert!(ids::extract_id(&stored).is_some(), "{stored}");
    }

    #[test]
    fn backfill_leaves_an_already_stamped_document_alone() {
        let dir = std::env::temp_dir().join(format!("wltimer-store-bfu2-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let original = format!("# Old\n- updated: {NOW}\n\n## A\n- work: 30\n");
        zio::write_compressed(&dir.join("old.md.zst"), original.as_bytes()).unwrap();

        let s = Store::new(dir).unwrap();
        assert_eq!(
            ids::extract_updated(&s.read_source("old").unwrap()).as_deref(),
            Some(NOW)
        );
    }

    #[test]
    fn migrates_plain_md_files() {
        let dir = std::env::temp_dir().join(format!("wltimer-store-mig-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("legacy.md"), "# Legacy\n\n## A\n- work: 30\n").unwrap();
        let s = Store::new(dir.clone()).unwrap();
        assert!(!dir.join("legacy.md").exists());
        assert!(dir.join("legacy.md.zst").exists());
        assert!(s.read_source("legacy").unwrap().contains("# Legacy"));
    }
}
