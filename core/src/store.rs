//! Library of workout templates: one zstd-compressed markdown file per workout.

use crate::ids;
use crate::parser;
use crate::zio;
use serde::Serialize;
use std::collections::BTreeMap;
use std::fs;
use std::path::PathBuf;

#[derive(Serialize, Clone)]
pub struct WorkoutSummary {
    pub slug: String,
    pub name: String,
    pub block_count: usize,
    pub total_secs: u32,
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

pub struct Store {
    dir: PathBuf,
}

impl Store {
    pub fn new(dir: PathBuf) -> std::io::Result<Self> {
        fs::create_dir_all(&dir)?;
        let store = Store { dir };
        store.migrate_plain_files();
        store.backfill_ids();
        Ok(store)
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
            let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
            let slug = match name.strip_suffix(".md.zst").or_else(|| name.strip_suffix(".md")) {
                Some(s) if !s.is_empty() => s.to_string(),
                _ => continue,
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
                    error: None,
                },
                Err(errs) => WorkoutSummary {
                    name: slug.clone(),
                    slug,
                    block_count: 0,
                    total_secs: 0,
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
    pub fn save(
        &self,
        source: &str,
        prev_slug: Option<&str>,
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
        let sum = s.save("# My Day\n\n## A\n- work: 30\n", None).unwrap();
        assert_eq!(sum.slug, "my-day");
        let renamed = s.save("# Other Day\n\n## A\n- work: 30\n", Some("my-day")).unwrap();
        assert_eq!(renamed.slug, "other-day");
        assert!(s.read_source("my-day").is_err());
        s.delete("other-day").unwrap();
        assert!(s.read_source("other-day").is_err());
    }

    #[test]
    fn duplicate_names_get_a_counter() {
        let s = temp_store("dup");
        let src = "# Squats\n\n## A\n- work: 30\n";
        assert_eq!(s.save(src, None).unwrap().slug, "squats");
        let second = s.save(src, None).unwrap();
        assert_eq!(second.slug, "squats-2");
        assert_eq!(second.name, "Squats (2)");
        assert!(s.read_source("squats-2").unwrap().starts_with("# Squats (2)\n"));
        assert_eq!(s.save(src, None).unwrap().slug, "squats-3");
        // Re-saving an existing workout under its own name is not a collision.
        let resaved = s.save(src, Some("squats")).unwrap();
        assert_eq!(resaved.slug, "squats");
        assert_eq!(resaved.name, "Squats");
    }

    #[test]
    fn saving_gives_the_document_an_id() {
        let s = temp_store("mint");
        let sum = s.save("# Squats\n\n## A\n- work: 30\n", None).unwrap();
        let stored = s.read_source(&sum.slug).unwrap();
        assert!(ids::extract_id(&stored).is_some(), "{stored}");
        assert!(stored.starts_with("# Squats\n- id: "), "{stored}");
    }

    #[test]
    fn re_saving_a_document_with_a_known_id_updates_in_place() {
        // The re-import case: export a workout, edit it elsewhere, upload it
        // again. It must edit the original, not pile up "Squats (2)".
        let s = temp_store("byid");
        let first = s.save("# Squats\n\n## A\n- work: 30\n", None).unwrap();
        let exported = s.read_source(&first.slug).unwrap();
        let edited = exported.replace("- work: 30", "- work: 45");

        let again = s.save(&edited, None).unwrap();
        assert_eq!(again.slug, "squats");
        assert_eq!(s.list().len(), 1, "must not have created a second workout");
        assert!(s.read_source("squats").unwrap().contains("- work: 45"));
    }

    #[test]
    fn a_renamed_document_with_a_known_id_is_renamed_not_copied() {
        let s = temp_store("byidrename");
        let first = s.save("# Squats\n\n## A\n- work: 30\n", None).unwrap();
        let exported = s.read_source(&first.slug).unwrap();

        let renamed = s.save(&exported.replace("# Squats", "# Front Squats"), None).unwrap();
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
        let first = s.save("# Squats\n\n## A\n- work: 30\n", None).unwrap();
        let source = s.read_source(&first.slug).unwrap();

        // As-is: an update, not a copy.
        s.save(&source, None).unwrap();
        assert_eq!(s.list().len(), 1);

        // With a fresh id: a genuine second workout.
        let copy = s.save(&ids::with_new_id(&source).0, None).unwrap();
        assert_eq!(copy.slug, "squats-2");
        assert_eq!(copy.name, "Squats (2)");
        assert_eq!(s.list().len(), 2);
    }

    #[test]
    fn an_id_belonging_to_another_workout_is_not_stolen() {
        // Pasting one workout's markdown into another's editor must not leave
        // two stored files claiming the same identity.
        let s = temp_store("steal");
        let a = s.save("# A\n\n## X\n- work: 30\n", None).unwrap();
        let b = s.save("# B\n\n## X\n- work: 30\n", None).unwrap();
        let a_src = s.read_source(&a.slug).unwrap();

        s.save(&a_src.replace("# A", "# B"), Some(&b.slug)).unwrap();
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
