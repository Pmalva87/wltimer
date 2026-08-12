//! Training plans: one markdown file describing multiple dated workout days.
//!
//! Format: `# Plan Name`, then one `## YYYY-MM-DD: Day Name` section per day,
//! whose `###` headings are the exercises (same params/notes as a single
//! workout). Each day converts to a standalone workout document (`##` → `#`,
//! `###` → `##`) that gets scheduled on the calendar.

use crate::days::{self, valid_date, DayEntry, DayStatus};
use crate::ids;
use crate::parser::{self, ParseError};
use crate::store::{retitle, slugify};
use crate::zio;
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::fs;
use std::path::PathBuf;

#[derive(Debug, Clone, PartialEq)]
pub struct PlanDay {
    pub date: String,
    pub name: String,
    /// Standalone workout markdown for this day.
    pub workout_md: String,
    /// The day's stable id, carried in the plan file as an `- id:` bullet
    /// under its `## YYYY-MM-DD` heading and inherited by the calendar entry
    /// this day schedules. That inheritance is what lets a re-sync recognise
    /// the entry it created last time — even from a plan re-uploaded under a
    /// different slug — instead of scheduling a duplicate.
    pub id: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Plan {
    pub name: String,
    pub days: Vec<PlanDay>,
}

#[derive(Serialize, Clone)]
pub struct PlanSummary {
    pub slug: String,
    pub name: String,
    pub day_count: usize,
    pub first_date: String,
    pub last_date: String,
    /// Set when the stored file no longer parses.
    pub error: Option<String>,
}

fn err(line: usize, message: impl Into<String>) -> ParseError {
    ParseError { line, message: message.into() }
}

/// Parse a `## ` day heading: `YYYY-MM-DD` optionally followed by `:`/`-` and
/// a name.
fn parse_day_heading(heading: &str) -> Option<(String, String)> {
    let heading = heading.trim();
    if heading.len() < 10 || !valid_date(&heading[..10]) {
        return None;
    }
    let date = heading[..10].to_string();
    let name = heading[10..].trim_start_matches([':', '-', ' ']).trim().to_string();
    Some((date, name))
}

pub fn parse_plan(source: &str) -> Result<Plan, Vec<ParseError>> {
    let mut errors: Vec<ParseError> = Vec::new();
    let mut plan_name: Option<String> = None;

    struct DayBuilder {
        date: String,
        name: String,
        heading_line: usize,
        /// Transformed sub-document lines and their original line numbers.
        lines: Vec<String>,
        line_map: Vec<usize>,
    }
    let mut builders: Vec<DayBuilder> = Vec::new();

    for (i, raw) in source.lines().enumerate() {
        let line_no = i + 1;
        let trimmed = raw.trim();
        if let Some(h) = trimmed.strip_prefix("## ") {
            match parse_day_heading(h) {
                Some((date, name)) => {
                    let name = if name.is_empty() { format!("Workout {date}") } else { name };
                    builders.push(DayBuilder {
                        date,
                        name,
                        heading_line: line_no,
                        lines: Vec::new(),
                        line_map: Vec::new(),
                    });
                }
                None => errors.push(err(
                    line_no,
                    format!("day heading must start with a date: '## YYYY-MM-DD: Name', got '## {h}'"),
                )),
            }
        } else if let Some(h) = trimmed.strip_prefix("# ") {
            if plan_name.is_none() && builders.is_empty() {
                plan_name = Some(h.trim().to_string());
            } else {
                errors.push(err(line_no, "unexpected extra '#' title — use '## YYYY-MM-DD: Name' for days"));
            }
        } else if let Some(day) = builders.last_mut() {
            // Demote exercise headings one level for the standalone document.
            let transformed = if let Some(rest) = trimmed.strip_prefix("### ") {
                format!("## {rest}")
            } else {
                raw.to_string()
            };
            day.lines.push(transformed);
            day.line_map.push(line_no);
        }
        // Lines between the plan title and the first day are ignored.
    }

    let plan_name = match plan_name {
        Some(n) => n,
        None => {
            errors.push(err(1, "missing plan title — start the document with '# My Plan'"));
            String::new()
        }
    };
    if builders.is_empty() && errors.is_empty() {
        errors.push(err(1, "no days — add at least one '## YYYY-MM-DD: Name' section"));
    }

    let mut days: Vec<PlanDay> = Vec::new();
    for b in builders {
        let workout_md = format!("# {}\n{}\n", b.name, b.lines.join("\n"));
        // Validate the day as a standalone workout, mapping error lines back
        // to their position in the plan file (line 1 is the synthetic title).
        if let Err(day_errors) = parser::parse_workout(&workout_md) {
            for e in day_errors {
                let orig = if e.line <= 1 {
                    b.heading_line
                } else {
                    *b.line_map.get(e.line - 2).unwrap_or(&b.heading_line)
                };
                errors.push(err(orig, format!("day {}: {}", b.date, e.message)));
            }
        }
        // A date may carry several days: the calendar holds a list per date,
        // and a plan that could not say "squats and then cardio on Tuesday"
        // could not describe what was actually done. Identity, not the date,
        // is what a sync matches on.
        //
        // Two days sharing an id, though, would make sync ambiguous about
        // which entry each one owns, so that stays an error.
        let id = ids::extract_id(&workout_md);
        if id.is_some() && days.iter().any(|d: &PlanDay| d.id == id) {
            errors.push(err(b.heading_line, format!("duplicate day id on '{}'", b.date)));
        }
        days.push(PlanDay { date: b.date, name: b.name, workout_md, id });
    }
    // Stable, so two days on one date keep the order they were written in —
    // which is the only thing that says which came first that day.
    days.sort_by(|a, b| a.date.cmp(&b.date));

    if errors.is_empty() {
        Ok(Plan { name: plan_name, days })
    } else {
        errors.sort_by_key(|e| e.line);
        Err(errors)
    }
}

/// Merge a plan's days into the calendar, from `from` onward.
///
/// Each plan day is matched to the entry it scheduled last time **by the day's
/// id**, which that entry carries in its markdown — not by the plan's slug.
/// That is what lets the same plan re-uploaded under a different slug update
/// its days instead of scheduling a second copy of everything.
///
/// A day already completed is never touched and never re-added: the done entry
/// stands as the record of what was performed, and no duplicate planned copy
/// appears beside it. Dates before `from` are not passed in and so are never
/// affected.
///
/// Operates on whole days in memory — holding positions across mutations would
/// be a shifting-index bug waiting to happen. Returns how many days were
/// scheduled or updated, and which dates changed.
pub fn merge_into_calendar(
    plan: &Plan,
    slug: &str,
    from: &str,
    now: &str,
    by_date: &mut BTreeMap<String, Vec<DayEntry>>,
) -> (usize, BTreeSet<String>) {
    let mut dirty: BTreeSet<String> = BTreeSet::new();
    let mut synced = 0;
    let plan_ids: HashSet<&str> = plan.days.iter().filter_map(|d| d.id.as_deref()).collect();

    fn locate(by_date: &BTreeMap<String, Vec<DayEntry>>, id: &str) -> Option<(String, usize)> {
        by_date.iter().find_map(|(date, entries)| {
            entries
                .iter()
                .position(|e| days::entry_id(e).as_deref() == Some(id))
                .map(|i| (date.clone(), i))
        })
    }

    for day in plan.days.iter().filter(|d| d.date.as_str() >= from) {
        match day.id.as_deref().and_then(|id| locate(by_date, id)) {
            Some((date, i)) => {
                if by_date[&date][i].status == DayStatus::Done {
                    // Already performed. Leave the record alone, and do not
                    // schedule a fresh copy of something that is finished.
                    continue;
                }
                let mut entry = by_date.get_mut(&date).unwrap().remove(i);
                dirty.insert(date);
                entry.markdown = ids::set_updated(&day.workout_md, now);
                entry.source_plan = Some(slug.to_string());
                // Re-dated in the plan file? The entry moves and keeps its id
                // (and so its identity) rather than being destroyed and rebuilt.
                by_date.entry(day.date.clone()).or_default().push(entry);
                dirty.insert(day.date.clone());
                synced += 1;
            }
            None => {
                // Nothing carries this day's id. Before adding, check whether
                // this plan already has a completed entry on the date that no
                // *other* day of the plan accounts for: entries scheduled
                // before day ids existed cannot be matched by id, and
                // duplicating a workout already done is the exact failure this
                // function exists to prevent.
                //
                // The "no other day accounts for it" part is what lets a date
                // hold two workouts. A done entry another plan day owns by id
                // is that day's record, and says nothing about this one — so
                // it must not block adding a second workout to a date already
                // trained.
                let already_done = by_date.get(&day.date).is_some_and(|entries| {
                    entries.iter().any(|e| {
                        e.status == DayStatus::Done
                            && e.source_plan.as_deref() == Some(slug)
                            && !days::entry_id(e).is_some_and(|id| plan_ids.contains(id.as_str()))
                    })
                });
                if already_done {
                    continue;
                }
                by_date.entry(day.date.clone()).or_default().push(DayEntry {
                    markdown: ids::set_updated(&day.workout_md, now),
                    status: DayStatus::Planned,
                    completed_at: None,
                    source_slug: None,
                    source_plan: Some(slug.to_string()),
                });
                dirty.insert(day.date.clone());
                synced += 1;
            }
        }
    }

    // Days dropped from the plan leave their still-planned entries behind.
    for (date, entries) in by_date.iter_mut() {
        let before = entries.len();
        entries.retain(|e| {
            e.status != DayStatus::Planned
                || e.source_plan.as_deref() != Some(slug)
                || days::entry_id(e).is_some_and(|id| plan_ids.contains(id.as_str()))
        });
        if entries.len() != before {
            dirty.insert(date.clone());
        }
    }

    (synced, dirty)
}

/// Split a plan document into its preamble — the title and its bullets — and
/// one string per `## ` day section, in file order. Slices the user's own bytes
/// rather than re-serialising, for the same reason [`ensure_day_ids`] is
/// surgical: a plan is stored as written and there is no `plan_to_markdown` to
/// round-trip through.
fn sections(source: &str) -> (String, Vec<String>) {
    let mut preamble = String::new();
    let mut sections: Vec<String> = Vec::new();
    for line in source.split_inclusive('\n') {
        let trimmed = line.trim();
        // A day heading, not an exercise heading (`### ` fails this prefix).
        let starts_day = trimmed
            .strip_prefix("## ")
            .is_some_and(|h| parse_day_heading(h).is_some());
        if starts_day {
            sections.push(String::new());
        }
        match sections.last_mut() {
            Some(section) => section.push_str(line),
            None => preamble.push_str(line),
        }
    }
    (preamble, sections)
}

/// The workout id a day section declares: the first valid `- id:` bullet under
/// its heading, before its first exercise.
fn section_id(section: &str) -> Option<String> {
    section
        .lines()
        .skip(1)
        .take_while(|l| !l.trim_start().starts_with('#'))
        .find_map(|l| match ids::bullet(l) {
            Some((k, v)) if k == "id" && ids::valid_uuid(v) => Some(v.to_string()),
            _ => None,
        })
}

/// Fold an uploaded plan file's days into the plan it belongs to, matching
/// **by workout id** — the same uuid the calendar entry carries.
///
/// A section whose id is already in the plan replaces that day wherever it
/// sits, date included, so a fix that also re-dates a day moves it instead of
/// adding a second copy. A section with no match is appended: with no id there
/// is nothing to match on, and guessing by date is exactly the fallback that
/// makes a re-dated fix duplicate itself.
///
/// Days the file does not mention are left untouched — the rule
/// `bundle::restore` follows, and what makes uploading a two-day fix a fix
/// rather than a new plan. Dropping a day is therefore something you ask for,
/// never a side effect of what a file happens to leave out.
///
/// Returns the merged document, how many days it replaced, and how many it
/// added.
pub fn merge_days_into(stored: &str, patch: &str) -> (String, usize, usize) {
    let (_, mut days) = sections(stored);
    let (preamble, incoming) = sections(patch);
    let (mut updated, mut added) = (0, 0);
    for section in incoming {
        let at = section_id(&section)
            .and_then(|id| days.iter().position(|d| section_id(d).as_deref() == Some(&id)));
        match at {
            Some(i) => {
                days[i] = section;
                updated += 1;
            }
            None => {
                days.push(section);
                added += 1;
            }
        }
    }
    (format!("{preamble}{}", days.concat()), updated, added)
}

/// Give every day section an id, leaving existing ones and every other byte
/// untouched. Surgical rather than a re-serialize because a plan is stored as
/// the user's own markdown and there is no `plan_to_markdown` to round-trip
/// through — the same reason `store::retitle` edits only the title line.
fn ensure_day_ids(source: &str) -> String {
    let lines: Vec<&str> = source.split_inclusive('\n').collect();
    let mut out = String::with_capacity(source.len() + 64);
    for (i, line) in lines.iter().enumerate() {
        out.push_str(line);
        let trimmed = line.trim();
        // A day heading, not an exercise heading (`### ` fails this prefix).
        if !trimmed.starts_with("## ") || parse_day_heading(&trimmed[3..]).is_none() {
            continue;
        }
        // Does the section already declare one before its first exercise?
        let declared = lines[i + 1..]
            .iter()
            .take_while(|l| !l.trim().starts_with("##"))
            .any(|l| matches!(ids::bullet(l), Some((k, v)) if k == "id" && ids::valid_uuid(v)));
        if !declared {
            out.push_str(&format!("- id: {}\n", ids::new_id()));
        }
    }
    out
}

pub struct PlanStore {
    dir: PathBuf,
}

impl PlanStore {
    pub fn new(dir: PathBuf) -> std::io::Result<Self> {
        fs::create_dir_all(&dir)?;
        let store = PlanStore { dir };
        store.backfill_day_ids_and_updated();
        Ok(store)
    }

    /// One-time migration for plans saved before day ids, a plan id, or
    /// `updated` existed. All in one pass over the directory: the mtime a plan
    /// inherits has to be read before this function's own rewrite replaces it.
    fn backfill_day_ids_and_updated(&self) {
        let Ok(entries) = fs::read_dir(&self.dir) else {
            return;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let Ok(source) = zio::read_text(&path) else {
                continue;
            };
            let mtime = entry
                .metadata()
                .and_then(|m| m.modified())
                .ok()
                .and_then(crate::time::from_system_time);
            let mut updated = ids::ensure_id(&ensure_day_ids(&source)).0;
            if ids::extract_updated(&updated).is_none() {
                if let Some(mtime) = mtime {
                    updated = ids::set_updated(&updated, &mtime);
                }
            }
            if updated != source {
                let _ = zio::write_compressed(&path, updated.as_bytes());
            }
        }
    }

    fn path(&self, slug: &str) -> PathBuf {
        self.dir.join(format!("{slug}.md.zst"))
    }

    /// Every stored plan as `(slug, source)`. Shared by `list` and the two
    /// lookups below so they cannot disagree about what counts as stored.
    fn stored(&self) -> Vec<(String, String)> {
        let Ok(entries) = fs::read_dir(&self.dir) else {
            return Vec::new();
        };
        let mut out = Vec::new();
        for entry in entries.flatten() {
            let path = entry.path();
            let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
            let Some(slug) = name.strip_suffix(".md.zst") else {
                continue;
            };
            if let Ok(source) = zio::read_text(&path) {
                out.push((slug.to_string(), source));
            }
        }
        out.sort();
        out
    }

    /// Slug of the stored plan carrying this id, if any.
    fn find_by_id(&self, id: &str) -> Option<String> {
        self.stored()
            .into_iter()
            .find(|(_, source)| ids::extract_id(source).as_deref() == Some(id))
            .map(|(slug, _)| slug)
    }

    /// Slug of the stored plan that shares a day with `days`.
    ///
    /// A plan file written before plans themselves had ids still carries a uuid
    /// per day, and those are preserved across revisions — so an overlap is
    /// proof this upload is a new version of that plan rather than a new one.
    /// Without this, the copy on your laptop keeps landing as "My Plan (2)"
    /// until you have exported the app's own copy at least once.
    fn find_by_day_ids(&self, days: &[PlanDay]) -> Option<String> {
        let ids: HashSet<&str> = days.iter().filter_map(|d| d.id.as_deref()).collect();
        if ids.is_empty() {
            return None;
        }
        self.stored().into_iter().find_map(|(slug, source)| {
            let stored = parse_plan(&source).ok()?;
            stored
                .days
                .iter()
                .any(|d| d.id.as_deref().is_some_and(|id| ids.contains(id)))
                .then_some(slug)
        })
    }

    pub fn list(&self) -> Vec<PlanSummary> {
        let mut out = Vec::new();
        for (slug, source) in self.stored() {
            let slug = slug.as_str();
            out.push(match parse_plan(&source) {
                Ok(p) => PlanSummary {
                    slug: slug.to_string(),
                    name: p.name,
                    day_count: p.days.len(),
                    first_date: p.days.first().map(|d| d.date.clone()).unwrap_or_default(),
                    last_date: p.days.last().map(|d| d.date.clone()).unwrap_or_default(),
                    error: None,
                },
                Err(errs) => PlanSummary {
                    slug: slug.to_string(),
                    name: slug.to_string(),
                    day_count: 0,
                    first_date: String::new(),
                    last_date: String::new(),
                    error: Some(format!("line {}: {}", errs[0].line, errs[0].message)),
                },
            });
        }
        out.sort_by_key(|s| s.name.to_lowercase());
        out
    }

    pub fn read_source(&self, slug: &str) -> Result<String, String> {
        zio::read_text(&self.path(slug)).map_err(|e| format!("cannot read plan '{slug}': {e}"))
    }

    /// Validate and write; name collisions with other plans get a counter,
    /// like workout saves.
    pub fn save(
        &self,
        source: &str,
        prev_slug: Option<&str>,
        now: &str,
    ) -> Result<(PlanSummary, Plan), Vec<ParseError>> {
        // Validate what the user wrote (so error lines match their file), then
        // re-parse with ids in place so the returned Plan carries the same ids
        // that were written to disk — sync matches on them.
        parse_plan(source)?;
        let source = ensure_day_ids(source);
        let had_id = ids::extract_id(&source).is_some();
        let (source, id) = ids::ensure_id(&source);
        let plan = parse_plan(&source)?;

        // Which stored plan is this a new version of? Its own id, when it
        // carries one. A file carrying none can still be a revision written
        // from a copy taken before plans had ids — its day ids give it away,
        // since those are preserved across revisions — and adopting the stored
        // plan's id keeps identity stable from then on. Without this the copy
        // on your laptop lands as "My Plan (2)" every single time.
        let (source, owner) = match self.find_by_id(&id) {
            Some(slug) => (source, Some(slug)),
            None if had_id => (source, None),
            None => match self.find_by_day_ids(&plan.days) {
                Some(slug) => {
                    let stored_id = self.read_source(&slug).ok().and_then(|s| ids::extract_id(&s));
                    let source = match stored_id {
                        Some(stored_id) => ids::set_id(&source, &stored_id),
                        None => source,
                    };
                    (source, Some(slug))
                }
                None => (source, None),
            },
        };

        // Two files must never claim one identity: a document carrying another
        // stored plan's id is a copy of it, so it gets its own — the same rule
        // `store::save` applies to workouts.
        let source = match (prev_slug, owner.as_deref()) {
            (Some(prev), Some(other)) if other != prev => ids::with_new_id(&source).0,
            _ => source,
        };
        let prev_slug = prev_slug.or(owner.as_deref());

        let source = source.as_str();
        let mut name = plan.name.clone();
        let mut slug = slugify(&name);
        if prev_slug != Some(slug.as_str()) {
            let mut n = 2;
            while self.path(&slug).exists() {
                name = format!("{} ({n})", plan.name);
                slug = slugify(&name);
                n += 1;
            }
        }
        let source = if name == plan.name {
            source.to_string()
        } else {
            retitle(source, &name)
        };
        let source = ids::set_updated(&source, now);
        zio::write_compressed(&self.path(&slug), source.as_bytes())
            .map_err(|e| vec![err(1, format!("cannot save plan: {e}"))])?;
        if let Some(prev) = prev_slug {
            if prev != slug {
                let _ = fs::remove_file(self.path(prev));
            }
        }
        let summary = PlanSummary {
            slug,
            name,
            day_count: plan.days.len(),
            first_date: plan.days.first().map(|d| d.date.clone()).unwrap_or_default(),
            last_date: plan.days.last().map(|d| d.date.clone()).unwrap_or_default(),
            error: None,
        };
        Ok((summary, plan))
    }

    /// Apply an uploaded plan file to the plan it belongs to, day by day.
    ///
    /// This is what the upload button uses: a file holding only the days you
    /// fixed updates exactly those and leaves the rest of the plan standing.
    /// A file belonging to no stored plan is simply saved as a new one, so
    /// uploading a brand-new plan still works the way it always did.
    ///
    /// Returns the days replaced and the days added alongside the summary.
    pub fn patch(
        &self,
        source: &str,
        now: &str,
    ) -> Result<(PlanSummary, Plan, usize, usize), Vec<ParseError>> {
        let uploaded = parse_plan(source)?;
        let owner = ids::extract_id(source)
            .and_then(|id| self.find_by_id(&id))
            .or_else(|| self.find_by_day_ids(&uploaded.days));
        let stored = owner.as_deref().and_then(|slug| self.read_source(slug).ok());
        let (Some(slug), Some(stored)) = (owner, stored) else {
            let (summary, plan) = self.save(source, None, now)?;
            let added = plan.days.len();
            return Ok((summary, plan, 0, added));
        };

        let (merged, updated, added) = merge_days_into(&stored, source);
        // The merged document keeps the stored plan's identity even when the
        // uploaded file has none of its own.
        let merged = match ids::extract_id(&stored) {
            Some(id) => ids::set_id(&merged, &id),
            None => merged,
        };
        // A merge can still fail to parse — two days sharing an id, say. Error
        // lines then point into the merged document rather than the uploaded
        // file, but such a collision can only come from the upload's own ids.
        let (summary, plan) = self.save(&merged, Some(&slug), now)?;
        Ok((summary, plan, updated, added))
    }

    pub fn delete(&self, slug: &str) -> Result<(), String> {
        fs::remove_file(self.path(slug)).map_err(|e| format!("cannot delete plan '{slug}': {e}"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const NOW: &str = "2026-08-09T13:45:31Z";
    const LATER: &str = "2026-08-10T09:00:00Z";

    const PLAN: &str = "\
# 531 Cycle 1

## 2026-07-30: Heavy Squats
### Back Squat
- intervals: 5
- work: 2:00
- rest: 3:00

Brace hard.

## 2026-08-01: Bench Day
### Bench Press
- intervals: 3
- work: 1:00
";

    #[test]
    fn parses_plan_and_converts_days() {
        let p = parse_plan(PLAN).unwrap();
        assert_eq!(p.name, "531 Cycle 1");
        assert_eq!(p.days.len(), 2);
        assert_eq!(p.days[0].date, "2026-07-30");
        assert_eq!(p.days[0].name, "Heavy Squats");
        // Each day's markdown is a valid standalone workout.
        let w = parser::parse_workout(&p.days[0].workout_md).unwrap();
        assert_eq!(w.name, "Heavy Squats");
        assert_eq!(w.blocks[0].name, "Back Squat");
        assert_eq!(w.blocks[0].work_secs, 120);
        assert!(w.blocks[0].description_md.contains("Brace hard."));
    }

    #[test]
    fn days_are_sorted_by_date() {
        let src = "# P\n\n## 2026-08-01: B\n### X\n- work: 30\n\n## 2026-07-30: A\n### Y\n- work: 30\n";
        let p = parse_plan(src).unwrap();
        assert_eq!(p.days[0].date, "2026-07-30");
    }

    #[test]
    fn rejects_headings_without_dates() {
        let errs = parse_plan("# P\n\n## Day 1\n### X\n- work: 30\n").unwrap_err();
        assert_eq!(errs[0].line, 3);
        assert!(errs[0].message.contains("must start with a date"));
    }

    #[test]
    fn a_date_may_hold_several_workouts_in_the_order_written() {
        let src = "# P\n\n## 2026-07-30: A\n### X\n- work: 30\n\n## 2026-07-30: B\n### Y\n- work: 30\n";
        let p = parse_plan(src).unwrap();
        assert_eq!(p.days.len(), 2);
        assert_eq!(p.days[0].name, "A");
        assert_eq!(p.days[1].name, "B");
    }

    #[test]
    fn day_errors_map_to_plan_lines() {
        // Line 5 has the bad work value.
        let src = "# P\n\n## 2026-07-30: A\n### X\n- work: nope\n";
        let errs = parse_plan(src).unwrap_err();
        assert_eq!(errs[0].line, 5);
        assert!(errs[0].message.contains("day 2026-07-30"));
        assert!(errs[0].message.contains("invalid time"));
    }

    #[test]
    fn store_saves_lists_dedupes() {
        let dir = std::env::temp_dir().join(format!("wltimer-plans-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        let s = PlanStore::new(dir).unwrap();
        let (sum, plan) = s.save(PLAN, None, NOW).unwrap();
        assert_eq!(sum.slug, "531-cycle-1");
        assert_eq!(sum.day_count, 2);
        assert_eq!(sum.first_date, "2026-07-30");
        assert_eq!(plan.days.len(), 2);
        let (dup, _) = s.save(PLAN, None, NOW).unwrap();
        assert_eq!(dup.slug, "531-cycle-1-2");
        // Re-saving under its own slug replaces in place.
        let (resaved, _) = s.save(PLAN, Some("531-cycle-1"), NOW).unwrap();
        assert_eq!(resaved.slug, "531-cycle-1");
        assert_eq!(s.list().len(), 2);
        s.delete("531-cycle-1-2").unwrap();
        assert_eq!(s.list().len(), 1);
    }

    // ---- day ids and calendar merge ----

    /// The same file as it would have looked before plans had ids: the
    /// preamble `- id:` gone, every day id left in place.
    fn drop_plan_id(source: &str) -> String {
        let mut out = String::new();
        let mut preamble = true;
        for line in source.split_inclusive('\n') {
            if line.trim_start().starts_with("## ") {
                preamble = false;
            }
            if preamble && matches!(ids::bullet(line), Some((k, _)) if k == "id") {
                continue;
            }
            out.push_str(line);
        }
        out
    }

    fn plan_store(tag: &str) -> PlanStore {
        let dir = std::env::temp_dir().join(format!("wltimer-plans-{tag}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        PlanStore::new(dir).unwrap()
    }

    /// Save the plan so its days get ids, and return the stored Plan.
    fn saved_plan(tag: &str) -> (PlanStore, Plan) {
        let s = plan_store(tag);
        let (_, plan) = s.save(PLAN, None, NOW).unwrap();
        (s, plan)
    }

    fn calendar(plan: &Plan, slug: &str) -> BTreeMap<String, Vec<DayEntry>> {
        let mut by_date = BTreeMap::new();
        merge_into_calendar(plan, slug, "2026-07-01", NOW, &mut by_date);
        by_date
    }

    fn all_entries(by_date: &BTreeMap<String, Vec<DayEntry>>) -> Vec<(&str, &DayEntry)> {
        by_date
            .iter()
            .flat_map(|(d, es)| es.iter().map(move |e| (d.as_str(), e)))
            .collect()
    }

    #[test]
    fn saving_a_plan_gives_every_day_an_id() {
        let (_, plan) = saved_plan("ids");
        assert!(plan.days.iter().all(|d| d.id.is_some()), "{plan:?}");
        let a = plan.days[0].id.clone().unwrap();
        let b = plan.days[1].id.clone().unwrap();
        assert_ne!(a, b);
    }

    #[test]
    fn saving_a_plan_twice_keeps_the_same_day_ids() {
        let s = plan_store("stable");
        let (_, first) = s.save(PLAN, None, NOW).unwrap();
        let stored = s.read_source("531-cycle-1").unwrap();
        let (_, again) = s.save(&stored, Some("531-cycle-1"), NOW).unwrap();
        assert_eq!(
            first.days.iter().map(|d| d.id.clone()).collect::<Vec<_>>(),
            again.days.iter().map(|d| d.id.clone()).collect::<Vec<_>>()
        );
    }

    #[test]
    fn a_patch_updates_only_the_days_it_carries() {
        let s = plan_store("patch");
        let (sum, plan) = s.save(PLAN, None, NOW).unwrap();
        let second = plan.days[1].id.clone().unwrap();

        // A file holding one corrected day, the way you would hand it back
        // after fixing it: the plan title, that day, nothing else.
        let fix = format!(
            "# 531 Cycle 1\n\n## 2026-08-01: Bench Day\n- id: {second}\n### Bench Press\n- intervals: 5\n- work: 1:30\n"
        );
        let (again, merged, updated, added) = s.patch(&fix, LATER).unwrap();

        assert_eq!(again.slug, sum.slug, "the same plan, not a second one");
        assert_eq!((updated, added), (1, 0));
        assert_eq!(merged.days.len(), 2, "the untouched day is still there");
        assert_eq!(merged.days[0].name, "Heavy Squats");
        assert!(merged.days[0].workout_md.contains("Brace hard."));
        assert!(merged.days[1].workout_md.contains("- work: 1:30"));
        // Identity survives, which is what lets the sync update the calendar
        // entry this day already scheduled.
        assert_eq!(merged.days[1].id.as_deref(), Some(second.as_str()));
    }

    #[test]
    fn a_patch_that_re_dates_a_day_moves_it_rather_than_copying_it() {
        let s = plan_store("patch-redate");
        let (_, plan) = s.save(PLAN, None, NOW).unwrap();
        let first = plan.days[0].id.clone().unwrap();

        let fix =
            format!("# 531 Cycle 1\n\n## 2026-08-05: Heavy Squats\n- id: {first}\n### Back Squat\n- work: 2:00\n");
        let (_, merged, updated, added) = s.patch(&fix, LATER).unwrap();

        assert_eq!((updated, added), (1, 0));
        assert_eq!(merged.days.len(), 2);
        assert!(!merged.days.iter().any(|d| d.date == "2026-07-30"));
        assert_eq!(merged.days.iter().filter(|d| d.id.as_deref() == Some(&first)).count(), 1);
    }

    #[test]
    fn a_patch_day_with_no_id_is_added_as_a_new_one() {
        let s = plan_store("patch-newday");
        let (sum, plan) = s.save(PLAN, None, NOW).unwrap();
        let known: Vec<_> = plan.days.iter().map(|d| d.id.clone()).collect();
        let plan_id = ids::extract_id(&s.read_source(&sum.slug).unwrap()).unwrap();

        // The plan id says which plan this belongs to; the day has no id to
        // match on, so it can only be new — and it must not displace the day
        // that happens to share its date.
        let fix = format!(
            "# 531 Cycle 1\n- id: {plan_id}\n\n## 2026-08-01: Evening Cardio\n### Row\n- work: 5:00\n"
        );
        let (_, merged, updated, added) = s.patch(&fix, LATER).unwrap();

        assert_eq!((updated, added), (0, 1));
        assert_eq!(merged.days.len(), 3);
        let cardio = merged.days.iter().find(|d| d.name == "Evening Cardio").unwrap();
        assert_eq!(cardio.date, "2026-08-01");
        assert!(cardio.id.is_some(), "saving mints one");
        assert!(!known.contains(&cardio.id), "and it is not another day's");
    }

    #[test]
    fn an_upload_belonging_to_no_stored_plan_is_still_a_new_plan() {
        let s = plan_store("patch-new");
        let (summary, plan, updated, added) = s.patch(PLAN, NOW).unwrap();
        assert_eq!(summary.slug, "531-cycle-1");
        assert_eq!((updated, added), (0, 2));
        assert_eq!(plan.days.len(), 2);
    }

    #[test]
    fn merge_schedules_both_workouts_of_a_doubled_date() {
        let two = "# P\n\n## 2026-07-30: Squats\n### X\n- work: 30\n\n## 2026-07-30: Cardio\n### Y\n- work: 30\n";
        let s = plan_store("doubled");
        let (_, plan) = s.save(two, None, NOW).unwrap();
        let cal = calendar(&plan, "p");
        assert_eq!(cal["2026-07-30"].len(), 2);
        assert_eq!(days::entry_name(&cal["2026-07-30"][0]), "Squats");
        assert_eq!(days::entry_name(&cal["2026-07-30"][1]), "Cardio");
    }

    #[test]
    fn a_second_workout_can_be_added_to_a_date_already_trained() {
        // The first workout of the day is done; adding another one to the plan
        // must schedule it rather than read as "that day is finished".
        let one = "# P\n\n## 2026-07-30: Squats\n### X\n- work: 30\n";
        let s = plan_store("second");
        let (sum, plan) = s.save(one, None, NOW).unwrap();
        let mut cal = calendar(&plan, &sum.slug);
        let entry = &mut cal.get_mut("2026-07-30").unwrap()[0];
        entry.status = DayStatus::Done;
        entry.completed_at = Some("2026-07-30T18:00:00Z".into());

        let stored = s.read_source(&sum.slug).unwrap();
        let two = format!("{stored}\n## 2026-07-30: Cardio\n### Y\n- work: 30\n");
        let (_, plan) = s.save(&two, Some(&sum.slug), NOW).unwrap();
        merge_into_calendar(&plan, &sum.slug, "2026-07-01", NOW, &mut cal);

        let day = &cal["2026-07-30"];
        assert_eq!(day.len(), 2, "the done squats plus the new cardio");
        assert_eq!(day[0].status, DayStatus::Done);
        assert_eq!(days::entry_name(&day[1]), "Cardio");
    }

    #[test]
    fn re_uploading_an_edited_plan_updates_it_in_place() {
        // The whole point: a fix to a plan you already have must not land
        // beside it as "531 Cycle 1 (2)".
        let s = plan_store("reupload");
        let first = s.save(PLAN, None, NOW).unwrap().0;
        let exported = s.read_source(&first.slug).unwrap();
        let edited = exported.replace("Brace hard.", "Brace harder.");

        // Uploaded as a plain upload — no `prev_slug`, the way the file picker
        // calls it. The id in the file is what routes it home.
        let again = s.save(&edited, None, NOW).unwrap().0;
        assert_eq!(again.slug, first.slug);
        assert_eq!(s.list().len(), 1);
        assert!(s.read_source(&first.slug).unwrap().contains("Brace harder."));
    }

    #[test]
    fn a_plan_revised_from_a_copy_without_a_plan_id_is_matched_by_its_days() {
        // The realistic case: the .md on your laptop predates plan ids, so it
        // carries day ids only. Those still identify the plan it came from.
        let s = plan_store("bydays");
        let first = s.save(PLAN, None, NOW).unwrap().0;
        let stored = s.read_source(&first.slug).unwrap();
        let plan_id = ids::extract_id(&stored).expect("plan gets an id on save");
        let without_plan_id = drop_plan_id(&stored);
        assert!(ids::extract_id(&without_plan_id).is_none());

        let again = s.save(&without_plan_id, None, NOW).unwrap().0;
        assert_eq!(again.slug, first.slug);
        assert_eq!(s.list().len(), 1);
        // It adopts the stored plan's id rather than minting one, so the next
        // upload from any copy still lands here.
        assert_eq!(
            ids::extract_id(&s.read_source(&first.slug).unwrap()).as_deref(),
            Some(plan_id.as_str())
        );
    }

    #[test]
    fn an_unrelated_plan_still_gets_its_own_file() {
        let s = plan_store("unrelated");
        s.save(PLAN, None, NOW).unwrap();
        let other = "# 531 Cycle 1\n\n## 2026-09-01: Deadlifts\n### Deadlift\n- work: 60\n";
        let saved = s.save(other, None, NOW).unwrap().0;
        // Same title, no shared days: a different plan, counter and all.
        assert_eq!(saved.name, "531 Cycle 1 (2)");
        assert_eq!(s.list().len(), 2);
    }

    #[test]
    fn duplicate_day_ids_are_an_error() {
        let id = ids::new_id();
        let src = format!(
            "# P\n\n## 2026-07-30: A\n- id: {id}\n### X\n- work: 30\n\n## 2026-08-01: B\n- id: {id}\n### Y\n- work: 30\n"
        );
        let errs = parse_plan(&src).unwrap_err();
        assert!(errs.iter().any(|e| e.message.contains("duplicate day id")), "{errs:?}");
    }

    #[test]
    fn merge_schedules_each_day_once_however_often_it_runs() {
        let (_, plan) = saved_plan("once");
        let mut cal = calendar(&plan, "531-cycle-1");
        assert_eq!(all_entries(&cal).len(), 2);

        // Re-syncing the same plan must update, never accumulate.
        let (synced, _) = merge_into_calendar(&plan, "531-cycle-1", "2026-07-01", NOW, &mut cal);
        assert_eq!(synced, 2);
        assert_eq!(all_entries(&cal).len(), 2);
    }

    #[test]
    fn merge_matches_the_same_plan_re_uploaded_under_a_new_slug() {
        // Uploading the plan file again makes a second plan with a different
        // slug; the day ids in the file are the same, so its days must update
        // the existing entries rather than double-book every date.
        let (_, plan) = saved_plan("reupload");
        let mut cal = calendar(&plan, "531-cycle-1");
        merge_into_calendar(&plan, "531-cycle-1-2", "2026-07-01", NOW, &mut cal);
        assert_eq!(all_entries(&cal).len(), 2);
    }

    #[test]
    fn merge_leaves_a_completed_day_alone_and_adds_no_copy() {
        let (_, plan) = saved_plan("done");
        let mut cal = calendar(&plan, "531-cycle-1");
        let done_id = {
            let entry = &mut cal.get_mut("2026-07-30").unwrap()[0];
            entry.status = DayStatus::Done;
            entry.completed_at = Some("2026-07-30T18:00:00+01:00".into());
            days::entry_id(entry).unwrap()
        };

        merge_into_calendar(&plan, "531-cycle-1", "2026-07-01", NOW, &mut cal);

        let day = &cal["2026-07-30"];
        assert_eq!(day.len(), 1, "a done workout must not gain a planned twin");
        assert_eq!(day[0].status, DayStatus::Done);
        assert_eq!(days::entry_id(&day[0]).unwrap(), done_id);
        assert!(day[0].completed_at.is_some());
    }

    #[test]
    fn merge_moves_an_entry_when_the_plan_re_dates_it() {
        let (store, plan) = saved_plan("moved");
        let mut cal = calendar(&plan, "531-cycle-1");
        let id = days::entry_id(&cal["2026-07-30"][0]).unwrap();

        let moved = store.read_source("531-cycle-1").unwrap().replace("2026-07-30", "2026-07-31");
        let (_, moved) = store.save(&moved, Some("531-cycle-1"), NOW).unwrap();
        merge_into_calendar(&moved, "531-cycle-1", "2026-07-01", NOW, &mut cal);

        assert!(cal.get("2026-07-30").is_none_or(|e| e.is_empty()));
        assert_eq!(days::entry_id(&cal["2026-07-31"][0]).unwrap(), id, "identity survives the move");
    }

    #[test]
    fn merge_updates_the_markdown_of_an_edited_day() {
        let (store, plan) = saved_plan("edited");
        let mut cal = calendar(&plan, "531-cycle-1");

        let edited = store.read_source("531-cycle-1").unwrap().replace("- work: 2:00", "- work: 2:30");
        let (_, edited) = store.save(&edited, Some("531-cycle-1"), NOW).unwrap();
        merge_into_calendar(&edited, "531-cycle-1", "2026-07-01", NOW, &mut cal);

        assert_eq!(all_entries(&cal).len(), 2);
        assert!(cal["2026-07-30"][0].markdown.contains("- work: 2:30"));
    }

    #[test]
    fn merge_drops_a_day_removed_from_the_plan() {
        let (store, plan) = saved_plan("dropped");
        let mut cal = calendar(&plan, "531-cycle-1");

        let source = store.read_source("531-cycle-1").unwrap();
        let trimmed = &source[..source.find("## 2026-08-01").unwrap()];
        let (_, shorter) = store.save(trimmed, Some("531-cycle-1"), NOW).unwrap();
        merge_into_calendar(&shorter, "531-cycle-1", "2026-07-01", NOW, &mut cal);

        assert_eq!(all_entries(&cal).len(), 1);
        assert!(cal.get("2026-08-01").is_none_or(|e| e.is_empty()));
    }

    #[test]
    fn merge_never_touches_another_plans_entries_or_your_own() {
        let (_, plan) = saved_plan("others");
        let mut cal = calendar(&plan, "531-cycle-1");
        cal.entry("2026-07-30".into()).or_default().push(DayEntry {
            markdown: ids::ensure_id("# Mine\n\n## A\n- work: 30\n").0,
            status: DayStatus::Planned,
            completed_at: None,
            source_slug: None,
            source_plan: None,
        });

        merge_into_calendar(&plan, "531-cycle-1", "2026-07-01", NOW, &mut cal);

        assert_eq!(cal["2026-07-30"].len(), 2);
        assert!(cal["2026-07-30"].iter().any(|e| e.markdown.contains("# Mine")));
    }

    #[test]
    fn merge_ignores_dates_before_the_cutoff() {
        let (_, plan) = saved_plan("cutoff");
        let mut cal = BTreeMap::new();
        let (synced, _) = merge_into_calendar(&plan, "531-cycle-1", "2026-08-01", NOW, &mut cal);
        assert_eq!(synced, 1);
        assert!(!cal.contains_key("2026-07-30"));
    }
}
