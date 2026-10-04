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
    pub organizer: Option<String>,
    pub category: Option<String>,
    pub age_group: Option<String>,
    pub registered: bool,
    pub total: TotalState,
    /// How many attempts have been taken, across both lifts. Zero on a meet
    /// you have only entered, which is what separates the two kinds of row a
    /// list of competitions holds.
    pub attempts_taken: usize,
    /// How many qualifying marks this meet asks for, if it is one you chase.
    pub standards: usize,
    /// Your mark here is already met: the best total in its window that
    /// counts, and where. `None` while it is still to get, and on a meet with
    /// no standard.
    pub qualified: Option<QualifiedBy>,
    /// Set when the stored file no longer parses (e.g. edited externally).
    pub error: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct QualifiedBy {
    pub total: f64,
    pub meet: String,
    pub date: Option<String>,
    pub category: Option<String>,
}

/// Whether `c`'s own entry standard is already met by one of `all`. "Yours"
/// is the narrowest row for the group `c` says you are entering. With no
/// group said, only a table of one row can answer — any other row being met
/// could be someone else's number, and a tick on the list is a claim.
fn qualified(c: &Competition, all: &[Competition]) -> Option<QualifiedBy> {
    let q = c.qualification.as_ref()?;
    let others: Vec<Competition> = all
        .iter()
        .filter(|o| match (&o.id, &c.id) {
            (Some(a), Some(b)) => a != b,
            _ => o.name != c.name,
        })
        .cloned()
        .collect();
    let mine = q.applicable(c.age_group.as_deref(), c.category.as_deref());
    let said = c.age_group.is_some() || c.category.is_some();
    let standard = match mine.as_slice() {
        [only] => *only,
        _ if said => *mine.first()?,
        _ => return None,
    };
    let m = q.met_by(standard, &others)?;
    Some(QualifiedBy {
        total: m.total().kg()?,
        meet: m.name.clone(),
        date: m.date.clone(),
        category: m.category.clone(),
    })
}

/// The Competitions list's order: what is still to come, soonest first and
/// starting from today, then the meets already behind you, most recent first.
/// Both halves read outward from now, which is where you are looking from.
///
/// A meet with no date sits between the two: it is one you have not pinned
/// down, so not history, but not next up either. `today` comes from the shell,
/// since `core` has no clock.
pub fn order_from(list: &mut [CompSummary], today: &str) {
    // 0 = upcoming, 1 = undated, 2 = past.
    let group = |s: &CompSummary| match s.date.as_deref() {
        Some(d) if d >= today => 0,
        None => 1,
        Some(_) => 2,
    };
    list.sort_by(|a, b| {
        group(a).cmp(&group(b)).then_with(|| match (group(a), &a.date, &b.date) {
            (2, Some(x), Some(y)) => y.cmp(x),
            (_, Some(x), Some(y)) => x.cmp(y),
            _ => a.name.to_lowercase().cmp(&b.name.to_lowercase()),
        })
    });
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

    /// Chronological, earliest date first, undated last — a stable order for
    /// callers that have no today to read from. The Competitions screen's own
    /// order, split around today, is [`order_from`].
    pub fn list(&self) -> Vec<CompSummary> {
        let parsed: Vec<(String, Result<Competition, Vec<ParseError>>)> = self
            .stored()
            .into_iter()
            .map(|(slug, source)| (slug, comp::parse_competition(&source)))
            .collect();
        let all: Vec<Competition> = parsed.iter().filter_map(|(_, r)| r.as_ref().ok().cloned()).collect();
        let mut out: Vec<CompSummary> = parsed
            .into_iter()
            .map(|(slug, parsed)| match parsed {
                Ok(c) => CompSummary {
                    qualified: qualified(&c, &all),
                    slug,
                    name: c.name.clone(),
                    date: c.date.clone(),
                    orgs: c.orgs.clone(),
                    organizer: c.organizer.clone(),
                    category: c.category.clone(),
                    age_group: c.age_group.clone(),
                    registered: c.registered(),
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
                    organizer: None,
                    category: None,
                    age_group: None,
                    registered: true,
                    total: TotalState::Open,
                    attempts_taken: 0,
                    standards: 0,
                    qualified: None,
                    error: Some(format!("line {}: {}", errs[0].line, errs[0].message)),
                },
            })
            .collect();
        out.sort_by(|a, b| match (&a.date, &b.date) {
            (Some(x), Some(y)) => x.cmp(y),
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

    /// Whether the stored meet has begun — see [`Competition::started`].
    pub fn started(&self, slug: &str) -> bool {
        self.read_source(slug)
            .ok()
            .and_then(|s| comp::parse_competition(&s).ok())
            .is_some_and(|c| c.started())
    }

    /// Save an uploaded meet file — [`Self::save`], refusing to replace a meet
    /// that has begun. A file written off the phone knows the plan, not the
    /// day, so landing on top of ticked warmups and taken attempts would erase
    /// the one record of them. The meet's own screen saves through `save`.
    pub fn import(&self, source: &str, now: &str) -> Result<CompSummary, Vec<ParseError>> {
        comp::parse_competition(source)?;
        if let Some(slug) = ids::extract_id(source).and_then(|id| self.find_by_id(&id)) {
            if self.started(&slug) {
                return Err(vec![ParseError {
                    line: 1,
                    message: "that meet is already under way on this phone — its warmups and \
                              attempts are kept, so the upload was not applied"
                        .into(),
                }]);
            }
        }
        self.save(source, None, now)
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
    fn upcoming_meets_lead_soonest_first_and_past_ones_follow_newest_first() {
        let s = temp_store("order");
        for (name, date) in [
            ("Old", Some("2025-03-01")),
            ("Next", Some("2026-10-04")),
            ("Later", Some("2026-12-01")),
            ("Recent", Some("2026-09-20")),
            ("Someday", None),
        ] {
            let date = date.map_or(String::new(), |d| format!("- date: {d}\n"));
            s.save(&format!("# {name}\n- kind: competition\n{date}"), None, NOW).unwrap();
        }
        let mut list = s.list();
        order_from(&mut list, "2026-10-04");
        let names: Vec<String> = list.into_iter().map(|c| c.name).collect();
        assert_eq!(names, ["Next", "Later", "Someday", "Recent", "Old"]);
    }

    #[test]
    fn an_upload_never_lands_on_a_meet_that_has_begun() {
        let store = temp_store("import-started");
        let id = "5a1b2c3d-4e5f-4a6b-8c7d-9e0f1a2b3c4d";
        let meet = format!("# Nationals\n- id: {id}\n- kind: competition\n\n## Snatch\n- 1: 95 planned\n- [ ] 20 x 5\n");
        let slug = store.import(&meet, NOW).unwrap().slug;
        // Not begun: an upload is a fix, and replaces it.
        store.import(&meet.replace("95 planned", "97 planned"), NOW).unwrap();
        store.save(&meet.replace("- [ ] 20", "- [x] 20"), Some(&slug), NOW).unwrap();
        assert!(store.import(&meet.replace("95 planned", "99 planned"), NOW).is_err());
        assert!(store.read_source(&slug).unwrap().contains("- [x] 20"));
        assert_eq!(store.list().len(), 1);
    }

    #[test]
    fn a_meet_whose_mark_is_already_met_lists_the_best_total_that_met_it() {
        let store = temp_store("qualified");
        let euros = "# Europeans\n- kind: competition\n- date: 2026-11-20\n- age group: M40\n\n\
                     ## Qualification\n- from: 2026-01-01\n- to: 2026-10-31\n\n\
                     ### M40\n- needs: 200\n\n### M45\n- needs: 180\n";
        let result = |name: &str, date: &str, sn: u32, cj: u32| {
            format!("# {name}\n- kind: competition\n- date: {date}\n\n## Snatch\n- 1: {sn} good\n\n## Clean & Jerk\n- 1: {cj} good\n")
        };
        store.save(euros, None, NOW).unwrap();
        store.save(&result("Spring Open", "2026-03-01", 82, 108), None, NOW).unwrap();
        let find = |store: &CompStore| store.list().into_iter().find(|s| s.name == "Europeans").unwrap();
        // 190 is over the M45 row, but that is not this entry's row.
        assert_eq!(find(&store).qualified, None);

        store.save(&result("Summer Open", "2026-06-01", 92, 118), None, NOW).unwrap();
        store.save(&result("Autumn Open", "2026-09-01", 91, 115), None, NOW).unwrap();
        let q = find(&store).qualified.unwrap();
        assert_eq!((q.total, q.meet.as_str()), (210.0, "Summer Open"));
    }

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
    fn meets_are_listed_earliest_date_first_with_undated_ones_last() {
        let s = temp_store("meets_are_listed_earliest_date_first_with_undated_ones_last");
        s.save("# Undated\n- kind: competition\n", None, NOW).unwrap();
        s.save("# Old\n- kind: competition\n- date: 2025-01-01\n", None, NOW).unwrap();
        s.save("# Next\n- kind: competition\n- date: 2027-01-01\n", None, NOW).unwrap();
        let names: Vec<String> = s.list().into_iter().map(|c| c.name).collect();
        assert_eq!(names, vec!["Old", "Next", "Undated"]);
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
        // Club Open (2026-02-01) sorts before Europeans 2027 (2027-04-10).
        assert_eq!(list[0].standards, 0);
        assert_eq!(list[0].attempts_taken, 1);
        assert_eq!(list[1].standards, 1);
        assert_eq!(list[1].attempts_taken, 0);
    }
}
