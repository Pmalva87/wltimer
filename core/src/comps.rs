//! Competitions on disk: one zstd-compressed markdown document per meet.
//!
//! A younger sibling of [`crate::store`], and deliberately a plainer one:
//! there are no legacy plain files to migrate and nothing stored before ids
//! existed to backfill, because nothing was ever written here without both.
//! What it does share is the rule that matters — the document's own `- id:` is
//! the handle, so saving a meet the store already holds updates it in place
//! wherever the markdown came from, and only a genuinely new meet whose name
//! collides gets a counter.

use crate::comp::{self, Competition, TotalState};
use crate::ids;
use crate::parser::ParseError;
use crate::store::{retitle, slugify};
use crate::zio;
use serde::Serialize;
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

/// A meet as a list row.
#[derive(Debug, Clone, Serialize)]
pub struct CompSummary {
    pub slug: String,
    pub name: String,
    pub date: Option<String>,
    pub orgs: Vec<String>,
    pub category: Option<String>,
    pub age_group: Option<String>,
    pub total: TotalState,
    /// How many attempts have been taken, across both lifts. Zero on a meet
    /// you have only entered, which is what separates the two kinds of row a
    /// list of competitions holds.
    pub attempts_taken: usize,
    /// How many qualifying marks this meet asks for, if it is one you chase.
    pub standards: usize,
    /// Set when the stored file no longer parses (e.g. edited externally).
    pub error: Option<String>,
}

fn slug_of(path: &Path) -> Option<String> {
    let name = path.file_name().and_then(|n| n.to_str())?;
    let slug = name.strip_suffix(".md.zst")?;
    (!slug.is_empty()).then(|| slug.to_string())
}

pub struct CompStore {
    dir: PathBuf,
}

impl CompStore {
    pub fn new(dir: PathBuf) -> std::io::Result<Self> {
        fs::create_dir_all(&dir)?;
        Ok(CompStore { dir })
    }

    fn path(&self, slug: &str) -> PathBuf {
        self.dir.join(format!("{slug}.md.zst"))
    }

    fn write(&self, slug: &str, source: &str) -> std::io::Result<()> {
        zio::write_compressed(&self.path(slug), source.as_bytes())
    }

    /// Every stored meet as `(slug, source)`.
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

    /// Every meet that parses, for the cross-document questions — which marks
    /// are still open, and which meet already answered one.
    pub fn all(&self) -> Vec<Competition> {
        self.stored()
            .into_iter()
            .filter_map(|(_, source)| comp::parse_competition(&source).ok())
            .collect()
    }

    pub fn find_by_id(&self, id: &str) -> Option<String> {
        self.stored()
            .into_iter()
            .find(|(_, source)| ids::extract_id(source).as_deref() == Some(id))
            .map(|(slug, _)| slug)
    }

    /// Newest first. A meet with no date sorts last rather than first: it is
    /// one you have not pinned down, not one that happened at the dawn of
    /// time, and an undated row at the top of the list would read as next up.
    pub fn list(&self) -> Vec<CompSummary> {
        let mut out: Vec<CompSummary> = self
            .stored()
            .into_iter()
            .map(|(slug, source)| match comp::parse_competition(&source) {
                Ok(c) => CompSummary {
                    slug,
                    name: c.name.clone(),
                    date: c.date.clone(),
                    orgs: c.orgs.clone(),
                    category: c.category.clone(),
                    age_group: c.age_group.clone(),
                    total: c.total(),
                    attempts_taken: c.snatch.taken() + c.clean_jerk.taken(),
                    standards: c.qualification.as_ref().map_or(0, |q| q.standards.len()),
                    error: None,
                },
                Err(errs) => CompSummary {
                    name: slug.clone(),
                    slug,
                    date: None,
                    orgs: Vec::new(),
                    category: None,
                    age_group: None,
                    total: TotalState::Open,
                    attempts_taken: 0,
                    standards: 0,
                    error: Some(format!("line {}: {}", errs[0].line, errs[0].message)),
                },
            })
            .collect();
        out.sort_by(|a, b| match (&a.date, &b.date) {
            (Some(x), Some(y)) => y.cmp(x),
            (Some(_), None) => std::cmp::Ordering::Less,
            (None, Some(_)) => std::cmp::Ordering::Greater,
            (None, None) => a.name.to_lowercase().cmp(&b.name.to_lowercase()),
        });
        out
    }

    pub fn read_source(&self, slug: &str) -> Result<String, String> {
        zio::read_text(&self.path(slug)).map_err(|e| format!("cannot read '{slug}': {e}"))
    }

    fn slug_taken(&self, slug: &str) -> bool {
        self.path(slug).exists()
    }

    /// Validate and write, returning parse errors if the document is invalid.
    ///
    /// Follows [`crate::store::Store::save`] line for line, for the same
    /// reasons: identity decides what is being updated, a document carrying
    /// another meet's id is a copy and gets its own, and a name collision on a
    /// genuinely new meet is resolved by counting rather than by overwriting.
    pub fn save(
        &self,
        source: &str,
        prev_slug: Option<&str>,
        now: &str,
    ) -> Result<CompSummary, Vec<ParseError>> {
        let c = comp::parse_competition(source)?;
        let (source, id) = ids::ensure_id(source);
        let owner = self.find_by_id(&id);
        let source = match (prev_slug, owner.as_deref()) {
            (Some(prev), Some(other)) if other != prev => ids::with_new_id(&source).0,
            _ => source,
        };
        let prev_slug = prev_slug.or(owner.as_deref());

        let mut name = c.name.clone();
        let mut slug = slugify(&name);
        if prev_slug != Some(slug.as_str()) {
            let mut n = 2;
            while self.slug_taken(&slug) {
                name = format!("{} ({n})", c.name);
                slug = slugify(&name);
                n += 1;
            }
        }
        let source = if name == c.name {
            source
        } else {
            retitle(&source, &name)
        };
        let source = ids::set_updated(&source, now);
        self.write(&slug, &source).map_err(|e| {
            vec![ParseError {
                line: 1,
                message: format!("cannot save: {e}"),
            }]
        })?;
        if let Some(prev) = prev_slug {
            if prev != slug {
                let _ = self.delete(prev);
            }
        }
        let mut summary = self
            .list()
            .into_iter()
            .find(|s| s.slug == slug)
            .expect("just written");
        summary.name = name;
        Ok(summary)
    }

    pub fn delete(&self, slug: &str) -> Result<(), String> {
        fs::remove_file(self.path(slug)).map_err(|_| format!("cannot delete '{slug}'"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const NOW: &str = "2026-09-12T10:00:00Z";

    fn temp_store(tag: &str) -> CompStore {
        let dir = std::env::temp_dir().join(format!("wltimer-comps-{tag}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        CompStore::new(dir).unwrap()
    }

    const MEET: &str = "# Lisbon Open\n- kind: competition\n- date: 2026-05-10\n- org: FPH\n";

    #[test]
    fn saves_and_reads_back() {
        let s = temp_store("saves_and_reads_back");
        let saved = s.save(MEET, None, NOW).unwrap();
        assert_eq!(saved.slug, "lisbon-open");
        assert_eq!(saved.date.as_deref(), Some("2026-05-10"));
        let source = s.read_source("lisbon-open").unwrap();
        assert!(source.contains("- id: "));
        assert!(source.contains(&format!("- updated: {NOW}")));
    }

    #[test]
    fn saving_the_same_document_again_updates_it_rather_than_copying_it() {
        let s = temp_store("saving_the_same_document_again_updates_it_rather_than_copying_it");
        s.save(MEET, None, NOW).unwrap();
        let source = s.read_source("lisbon-open").unwrap();
        // Straight back in, as a re-import of an exported file would arrive:
        // no slug to go on, only the id inside the document.
        let again = s.save(&source, None, NOW).unwrap();
        assert_eq!(again.slug, "lisbon-open");
        assert_eq!(s.list().len(), 1);
    }

    #[test]
    fn a_new_meet_that_shares_a_name_is_counted_not_overwritten() {
        let s = temp_store("a_new_meet_that_shares_a_name_is_counted_not_overwritten");
        s.save(MEET, None, NOW).unwrap();
        let second = s.save(MEET, None, NOW).unwrap();
        assert_eq!(second.name, "Lisbon Open (2)");
        assert_eq!(s.list().len(), 2);
    }

    #[test]
    fn renaming_moves_the_file_rather_than_leaving_both() {
        let s = temp_store("renaming_moves_the_file_rather_than_leaving_both");
        s.save(MEET, None, NOW).unwrap();
        let source = s.read_source("lisbon-open").unwrap().replace("# Lisbon Open", "# Lisbon Cup");
        let renamed = s.save(&source, Some("lisbon-open"), NOW).unwrap();
        assert_eq!(renamed.slug, "lisbon-cup");
        assert_eq!(s.list().len(), 1);
        assert!(s.read_source("lisbon-open").is_err());
    }

    #[test]
    fn a_document_that_does_not_parse_is_not_written() {
        let s = temp_store("a_document_that_does_not_parse_is_not_written");
        let errs = s.save("- kind: competition\n", None, NOW).unwrap_err();
        assert!(errs[0].message.contains("missing competition title"));
        assert!(s.list().is_empty());
    }

    #[test]
    fn meets_are_listed_newest_first_with_undated_ones_last() {
        let s = temp_store("meets_are_listed_newest_first_with_undated_ones_last");
        s.save("# Undated\n- kind: competition\n", None, NOW).unwrap();
        s.save("# Old\n- kind: competition\n- date: 2025-01-01\n", None, NOW).unwrap();
        s.save("# Next\n- kind: competition\n- date: 2027-01-01\n", None, NOW).unwrap();
        let names: Vec<String> = s.list().into_iter().map(|c| c.name).collect();
        assert_eq!(names, vec!["Next", "Old", "Undated"]);
    }

    #[test]
    fn a_list_row_says_what_kind_of_meet_it_is() {
        let s = temp_store("a_list_row_says_what_kind_of_meet_it_is");
        s.save(
            "# Europeans 2027\n- kind: competition\n- date: 2027-04-10\n\n## Qualification\n- needs: 250\n",
            None,
            NOW,
        )
        .unwrap();
        s.save(
            "# Club Open\n- kind: competition\n- date: 2026-02-01\n\n## Snatch\n- 1: 95 good\n",
            None,
            NOW,
        )
        .unwrap();
        let list = s.list();
        assert_eq!(list[0].standards, 1);
        assert_eq!(list[0].attempts_taken, 0);
        assert_eq!(list[1].standards, 0);
        assert_eq!(list[1].attempts_taken, 1);
    }
}
