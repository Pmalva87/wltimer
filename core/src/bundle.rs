//! Backup bundles: every stored document in a single markdown file.
//!
//! One file to hand to a cloud drive in one action, and to read back to
//! rebuild a phone. Sections are separated by an HTML-comment marker at the
//! start of a line and carry their document byte-for-byte — no heading
//! demotion, unlike the `##` day sections of a plan, so each document is
//! validated by exactly the parser that reads it on its own and the round trip
//! is exact. The marker renders as nothing, so a backup is still a readable
//! markdown document.
//!
//! Restore is identity-driven and additive: documents are matched by the
//! `- id:` they carry, a copy older than the one already stored loses, a
//! calendar entry already marked done is never replaced, and nothing is
//! deleted for being absent from the bundle.

use crate::days::{self, valid_date, DayEntry, DayStatus, DayStore};
use crate::ids;
use crate::parser::{self, ParseError};
use crate::plan::{self, PlanStore};
use crate::store::{slugify, Store};
use serde::Serialize;
use std::collections::BTreeMap;

const MARKER: &str = "<!-- wltimer:";
const END: &str = "-->";

/// One document in a bundle, as it will be restored.
#[derive(Debug, Clone)]
pub enum Section {
    Workout(String),
    /// The slug is derived from the plan's title at parse time: a plan document
    /// carries ids for its days but none for itself, so its name is the only
    /// handle a restore has for matching the plan it replaces.
    Plan { slug: String, body: String },
    Day { date: String, entry: DayEntry },
}

#[derive(Debug, Default, Clone, Copy, Serialize)]
pub struct Counts {
    pub added: usize,
    pub updated: usize,
    /// Left alone: the stored copy is newer, or it is a finished day.
    pub skipped: usize,
}

#[derive(Debug, Default, Clone, Copy, Serialize)]
pub struct ImportReport {
    pub workouts: Counts,
    pub plans: Counts,
    pub days: Counts,
    /// Documents that parsed but could not be written (a full disk, say).
    pub failed: usize,
}

fn err(line: usize, message: impl Into<String>) -> ParseError {
    ParseError { line, message: message.into() }
}

// ---- markers ----

struct Marker {
    kind: String,
    meta: Vec<(String, String)>,
}

impl Marker {
    fn get(&self, key: &str) -> Option<&str> {
        self.meta.iter().find(|(k, _)| k == key).map(|(_, v)| v.as_str())
    }
}

fn marker(line: &str) -> Option<Marker> {
    let inner = line.trim().strip_prefix(MARKER)?.strip_suffix(END)?;
    let mut parts = inner.split_whitespace();
    let kind = parts.next()?.to_string();
    let meta = parts
        .filter_map(|p| p.split_once('='))
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect();
    Some(Marker { kind, meta })
}

/// Metadata values are written unquoted and split on whitespace, which is safe
/// only because every one of them is machine-generated: a slug, a canonical
/// timestamp, a date, or a status word. Anything else is dropped rather than
/// written, since all of it is informational and a broken marker line would
/// swallow the document under it.
fn safe_value(v: &str) -> bool {
    !v.is_empty()
        && v.chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | ':' | '.'))
}

fn marker_line(kind: &str, meta: &[(&str, Option<&str>)]) -> String {
    let mut out = format!("{MARKER}{kind}");
    for (k, v) in meta {
        if let Some(v) = v {
            if safe_value(v) {
                out.push_str(&format!(" {k}={v}"));
            }
        }
    }
    out.push(' ');
    out.push_str(END);
    out.push('\n');
    out
}

/// A body line that would read back as a section marker is escaped on the way
/// out and restored on the way in — otherwise a workout whose notes quote a
/// bundle would split the file at that line and lose everything under it.
fn escape_body(body: &str) -> String {
    if !body.lines().any(|l| l.trim_start().starts_with(MARKER)) {
        return body.to_string();
    }
    body.lines()
        .map(|l| {
            if l.trim_start().starts_with(MARKER) {
                l.replacen("wltimer:", "wltimer\\:", 1)
            } else {
                l.to_string()
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
        + "\n"
}

fn unescape_body(body: &str) -> String {
    if !body.contains("wltimer\\:") {
        return body.to_string();
    }
    body.lines()
        .map(|l| {
            if l.trim_start().starts_with("<!-- wltimer\\:") {
                l.replacen("wltimer\\:", "wltimer:", 1)
            } else {
                l.to_string()
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
        + "\n"
}

// ---- writing ----

/// Everything in the three stores, as one document.
pub fn export(store: &Store, plans: &PlanStore, days: &DayStore, exported: &str) -> String {
    let workouts: Vec<String> = store
        .list()
        .into_iter()
        .filter_map(|s| store.read_source(&s.slug).ok())
        .collect();
    let plan_docs: Vec<String> = plans
        .list()
        .into_iter()
        .filter_map(|s| plans.read_source(&s.slug).ok())
        .collect();
    let calendar: Vec<(String, Vec<DayEntry>)> = days
        .dates_from("")
        .into_iter()
        .map(|date| {
            let entries = days.load(&date);
            (date, entries)
        })
        .filter(|(_, entries)| !entries.is_empty())
        .collect();
    build(&workouts, &plan_docs, &calendar, exported)
}

pub fn build(
    workouts: &[String],
    plans: &[String],
    days: &[(String, Vec<DayEntry>)],
    exported: &str,
) -> String {
    let mut out = marker_line("backup", &[("exported", Some(exported))]);
    let mut push = |head: String, body: &str| {
        out.push('\n');
        out.push_str(&head);
        out.push_str(&escape_body(body));
        if !body.ends_with('\n') {
            out.push('\n');
        }
    };

    for w in workouts {
        push(marker_line("workout", &[]), w);
    }
    for p in plans {
        push(marker_line("plan", &[]), p);
    }
    for (date, entries) in days {
        for e in entries {
            let status = match e.status {
                DayStatus::Planned => "planned",
                DayStatus::Done => "done",
            };
            let head = marker_line(
                "day",
                &[
                    ("date", Some(date.as_str())),
                    ("status", Some(status)),
                    ("completed", e.completed_at.as_deref()),
                    ("from", e.source_slug.as_deref()),
                    ("plan", e.source_plan.as_deref()),
                ],
            );
            push(head, &e.markdown);
        }
    }
    out
}

// ---- reading ----

/// Whether this document is a backup bundle at all, so an upload can be routed
/// to the right importer before anything is written.
pub fn is_bundle(source: &str) -> bool {
    source.lines().any(|l| marker(l).is_some())
}

struct Raw {
    m: Marker,
    line: usize,
    /// Line in the bundle that the body's own line 1 corresponds to.
    body_line: usize,
    body: String,
}

fn split(source: &str) -> Vec<Raw> {
    let mut out: Vec<Raw> = Vec::new();
    for (i, line) in source.lines().enumerate() {
        match marker(line) {
            Some(m) => out.push(Raw { m, line: i + 1, body_line: i + 2, body: String::new() }),
            None => {
                if let Some(last) = out.last_mut() {
                    last.body.push_str(line);
                    last.body.push('\n');
                }
            }
        }
    }
    // Sections are written a blank line apart, so every body but the last ends
    // with the separator before the next marker. Trailing blank lines carry no
    // markdown meaning, so collapsing them to one newline is what makes the
    // round trip exact for the documents the stores actually write.
    for raw in &mut out {
        let trimmed = raw.body.trim_end_matches('\n');
        raw.body = if trimmed.is_empty() { String::new() } else { format!("{trimmed}\n") };
    }
    out
}

/// Validate every section, mapping each document's own error lines back to
/// where they sit in the bundle so a message points at the file the user has.
pub fn parse(source: &str) -> Result<Vec<Section>, Vec<ParseError>> {
    let raws = split(source);
    if raws.is_empty() {
        return Err(vec![err(1, "not a backup file — no '<!-- wltimer:… -->' sections")]);
    }

    let mut errors: Vec<ParseError> = Vec::new();
    let mut sections: Vec<Section> = Vec::new();

    for raw in &raws {
        let at = |e: &ParseError| raw.body_line + e.line.saturating_sub(1);
        let body = unescape_body(&raw.body);
        match raw.m.kind.as_str() {
            // The header carries no document; its `exported` stamp is a note to
            // the reader, not something a restore acts on.
            "backup" => {}
            "workout" => match parser::parse_workout(&body) {
                Ok(_) => sections.push(Section::Workout(body)),
                Err(es) => errors.extend(es.iter().map(|e| err(at(e), e.message.clone()))),
            },
            "plan" => match plan::parse_plan(&body) {
                Ok(p) => sections.push(Section::Plan { slug: slugify(&p.name), body }),
                Err(es) => errors.extend(es.iter().map(|e| err(at(e), e.message.clone()))),
            },
            "day" => {
                let date = raw.m.get("date").unwrap_or_default().to_string();
                if !valid_date(&date) {
                    errors.push(err(raw.line, "day section needs a valid 'date=YYYY-MM-DD'"));
                    continue;
                }
                let status = match raw.m.get("status") {
                    Some("done") => DayStatus::Done,
                    _ => DayStatus::Planned,
                };
                match parser::parse_workout(&body) {
                    Ok(_) => sections.push(Section::Day {
                        date,
                        entry: DayEntry {
                            markdown: body,
                            status,
                            completed_at: raw.m.get("completed").map(str::to_string),
                            source_slug: raw.m.get("from").map(str::to_string),
                            source_plan: raw.m.get("plan").map(str::to_string),
                        },
                    }),
                    Err(es) => errors.extend(es.iter().map(|e| err(at(e), e.message.clone()))),
                }
            }
            other => errors.push(err(raw.line, format!("unknown section '{other}'"))),
        }
    }

    if errors.is_empty() {
        Ok(sections)
    } else {
        errors.sort_by_key(|e| e.line);
        Err(errors)
    }
}

// ---- restoring ----

/// Is the incoming copy beaten by what is already stored? An absent stamp reads
/// as oldest, and equal stamps let the incoming copy through: a document edited
/// by hand outside the app keeps the stamp it was exported with, and dropping
/// that edit would be worse than rewriting an identical document.
fn superseded(incoming: Option<&str>, stored: Option<&str>) -> bool {
    match (incoming, stored) {
        (_, None) => false,
        (None, Some(_)) => true,
        (Some(i), Some(s)) => s > i,
    }
}

/// A restored document keeps the stamp it was exported with rather than taking
/// `now`: it last changed when the backup says it did, and stamping the whole
/// library with the moment of the restore would destroy the only evidence any
/// later comparison has. `now` is the fallback for a document that has no stamp
/// at all — every write path still leaves one behind.
fn stamp_of(source: &str, now: &str) -> String {
    ids::extract_updated(source).unwrap_or_else(|| now.to_string())
}

pub fn restore(
    store: &Store,
    plans: &PlanStore,
    days: &DayStore,
    sections: &[Section],
    now: &str,
) -> ImportReport {
    let mut report = ImportReport::default();

    for section in sections {
        match section {
            Section::Workout(body) => {
                let owner = ids::extract_id(body).and_then(|id| store.find_by_id(&id));
                let stored = owner.as_deref().and_then(|s| store.read_source(s).ok());
                if superseded(
                    ids::extract_updated(body).as_deref(),
                    stored.as_deref().and_then(ids::extract_updated).as_deref(),
                ) {
                    report.workouts.skipped += 1;
                    continue;
                }
                match store.save(body, owner.as_deref(), &stamp_of(body, now)) {
                    Ok(_) if owner.is_some() => report.workouts.updated += 1,
                    Ok(_) => report.workouts.added += 1,
                    Err(_) => report.failed += 1,
                }
            }
            Section::Plan { slug, body } => {
                let stored = plans.read_source(slug).ok();
                if superseded(
                    ids::extract_updated(body).as_deref(),
                    stored.as_deref().and_then(ids::extract_updated).as_deref(),
                ) {
                    report.plans.skipped += 1;
                    continue;
                }
                // Deliberately no calendar sync: the day sections restore the
                // calendar exactly as it was, and syncing on top of them would
                // re-plan days over their restored state.
                let prev = stored.as_ref().map(|_| slug.as_str());
                match plans.save(body, prev, &stamp_of(body, now)) {
                    Ok(_) if prev.is_some() => report.plans.updated += 1,
                    Ok(_) => report.plans.added += 1,
                    Err(_) => report.failed += 1,
                }
            }
            Section::Day { .. } => {}
        }
    }

    restore_days(days, sections, now, &mut report);
    report
}

/// Days are restored a date at a time: every entry on one date lives in one
/// file, so merging them in a single pass keeps a busy day from being rewritten
/// once per entry.
fn restore_days(days: &DayStore, sections: &[Section], now: &str, report: &mut ImportReport) {
    let mut by_date: BTreeMap<&str, Vec<&DayEntry>> = BTreeMap::new();
    for section in sections {
        if let Section::Day { date, entry } = section {
            by_date.entry(date.as_str()).or_default().push(entry);
        }
    }

    for (date, incoming) in by_date {
        let mut entries = days.load(date);
        let mut changed = false;
        for entry in incoming {
            let id = days::entry_id(entry);
            let at = id
                .as_deref()
                .and_then(|id| entries.iter().position(|e| days::entry_id(e).as_deref() == Some(id)));
            match at {
                Some(i) => {
                    // A finished entry is the record of what was performed, and
                    // the same rule a plan sync follows: it is never replaced.
                    if entries[i].status == DayStatus::Done
                        || superseded(
                            days::entry_updated(entry).as_deref(),
                            days::entry_updated(&entries[i]).as_deref(),
                        )
                    {
                        report.days.skipped += 1;
                        continue;
                    }
                    entries[i] = entry.clone();
                    report.days.updated += 1;
                }
                None => {
                    let mut entry = entry.clone();
                    // Nothing on the calendar may lack an id, or the next
                    // restore would add a second copy of it.
                    entry.markdown = ids::ensure_id(&entry.markdown).0;
                    if ids::extract_updated(&entry.markdown).is_none() {
                        entry.markdown = ids::set_updated(&entry.markdown, now);
                    }
                    entries.push(entry);
                    report.days.added += 1;
                }
            }
            changed = true;
        }
        if changed && days.save(date, &entries).is_err() {
            report.failed += 1;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::PathBuf;

    const NOW: &str = "2026-08-09T13:45:31Z";
    const LATER: &str = "2026-08-10T09:00:00Z";

    fn temp_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("wltimer-bundle-{tag}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        dir
    }

    struct Stores {
        store: Store,
        plans: PlanStore,
        days: DayStore,
    }

    fn stores(tag: &str) -> Stores {
        let dir = temp_dir(tag);
        Stores {
            store: Store::new(dir.join("workouts")).unwrap(),
            plans: PlanStore::new(dir.join("plans")).unwrap(),
            days: DayStore::new(dir.join("days")).unwrap(),
        }
    }

    fn workout(name: &str) -> String {
        format!("# {name}\n\n## A\n- work: 30\n")
    }

    fn plan_doc() -> String {
        "# Block\n\n## 2026-08-10: Day 1\n### Snatch\n- work: 60\n".to_string()
    }

    fn entry(md: &str, status: DayStatus) -> DayEntry {
        DayEntry {
            markdown: ids::set_updated(&ids::ensure_id(md).0, NOW),
            status,
            completed_at: None,
            source_slug: None,
            source_plan: None,
        }
    }

    #[test]
    fn round_trips_every_kind_of_document() {
        let w = ids::set_updated(&ids::ensure_id(&workout("Squats")).0, NOW);
        let p = ids::set_updated(&plan_doc(), NOW);
        let day = entry(&workout("Monday"), DayStatus::Done);
        let days_in = vec![("2026-08-09".to_string(), vec![day.clone()])];

        let text = build(std::slice::from_ref(&w), std::slice::from_ref(&p), &days_in, NOW);
        let sections = parse(&text).expect("should parse");

        assert_eq!(sections.len(), 3);
        match &sections[0] {
            Section::Workout(body) => assert_eq!(body, &w),
            other => panic!("expected a workout, got {other:?}"),
        }
        match &sections[1] {
            Section::Plan { slug, body } => {
                assert_eq!(slug, "block");
                assert_eq!(body, &p);
            }
            other => panic!("expected a plan, got {other:?}"),
        }
        match &sections[2] {
            Section::Day { date, entry } => {
                assert_eq!(date, "2026-08-09");
                assert_eq!(entry.markdown, day.markdown);
                assert_eq!(entry.status, DayStatus::Done);
            }
            other => panic!("expected a day, got {other:?}"),
        }
    }

    #[test]
    fn carries_a_days_metadata_across() {
        let mut e = entry(&workout("Monday"), DayStatus::Done);
        e.completed_at = Some(LATER.to_string());
        e.source_slug = Some("monday-squats".into());
        e.source_plan = Some("block".into());

        let text = build(&[], &[], &[("2026-08-09".into(), vec![e])], NOW);
        match &parse(&text).unwrap()[0] {
            Section::Day { entry, .. } => {
                assert_eq!(entry.completed_at.as_deref(), Some(LATER));
                assert_eq!(entry.source_slug.as_deref(), Some("monday-squats"));
                assert_eq!(entry.source_plan.as_deref(), Some("block"));
            }
            other => panic!("expected a day, got {other:?}"),
        }
    }

    #[test]
    fn a_note_that_looks_like_a_marker_survives() {
        // Otherwise the file would split at that line and lose the rest of the
        // document under it.
        let w = "# Squats\n\n## A\n- work: 30\n\n<!-- wltimer:day date=2026-01-01 -->\n".to_string();
        let text = build(std::slice::from_ref(&w), &[], &[], NOW);
        let sections = parse(&text).unwrap();
        assert_eq!(sections.len(), 1, "the note must not have opened a section");
        match &sections[0] {
            Section::Workout(body) => assert_eq!(body, &w),
            other => panic!("expected a workout, got {other:?}"),
        }
    }

    #[test]
    fn plain_documents_are_not_bundles() {
        assert!(!is_bundle(&workout("Squats")));
        assert!(!is_bundle(&plan_doc()));
        assert!(is_bundle(&build(&[], &[], &[], NOW)));
        assert!(parse(&workout("Squats")).is_err());
    }

    #[test]
    fn errors_point_at_the_line_in_the_bundle() {
        let text = build(&[workout("Fine"), "# Broken\n\n## A\n- work: nope\n".into()], &[], &[], NOW);
        let errors = parse(&text).expect_err("second workout should fail");
        let line = errors[0].line;
        let at = text.lines().nth(line - 1).unwrap_or_default();
        assert!(at.contains("work: nope"), "line {line} was '{at}'");
    }

    #[test]
    fn restores_into_empty_stores() {
        let s = stores("empty");
        let text = build(
            &[workout("Squats")],
            &[plan_doc()],
            &[("2026-08-09".into(), vec![entry(&workout("Monday"), DayStatus::Done)])],
            NOW,
        );
        let report = restore(&s.store, &s.plans, &s.days, &parse(&text).unwrap(), NOW);

        assert_eq!(report.workouts.added, 1);
        assert_eq!(report.plans.added, 1);
        assert_eq!(report.days.added, 1);
        assert_eq!(report.failed, 0);
        assert_eq!(s.store.list().len(), 1);
        assert_eq!(s.plans.list().len(), 1);
        assert_eq!(s.days.load("2026-08-09").len(), 1);
    }

    #[test]
    fn restoring_twice_changes_nothing_the_second_time() {
        // The property that makes a bundle a backup rather than an archive.
        // Taken from a real export, because it is the ids an export carries
        // that make the second pass recognise what the first one wrote.
        let from = stores("idempotent-from");
        from.store.save(&workout("Squats"), None, NOW).unwrap();
        from.plans.save(&plan_doc(), None, NOW).unwrap();
        from.days
            .add("2026-08-09", entry(&workout("Monday"), DayStatus::Planned), NOW)
            .unwrap();
        let sections = parse(&export(&from.store, &from.plans, &from.days, NOW)).unwrap();

        let to = stores("idempotent-to");
        restore(&to.store, &to.plans, &to.days, &sections, NOW);
        let second = restore(&to.store, &to.plans, &to.days, &sections, NOW);

        assert_eq!(to.store.list().len(), 1, "workout duplicated");
        assert_eq!(to.plans.list().len(), 1, "plan duplicated");
        assert_eq!(to.days.load("2026-08-09").len(), 1, "day duplicated");
        assert_eq!(second.workouts.added, 0);
        assert_eq!(second.plans.added, 0);
        assert_eq!(second.days.added, 0);
    }

    #[test]
    fn an_older_copy_never_beats_what_is_stored() {
        let s = stores("older");
        let saved = s.store.save(&workout("Squats"), None, LATER).unwrap();
        let current = s.store.read_source(&saved.slug).unwrap();
        let stale = ids::set_updated(&current.replace("- work: 30", "- work: 15"), NOW);

        let report = restore(&s.store, &s.plans, &s.days, &[Section::Workout(stale)], NOW);
        assert_eq!(report.workouts.skipped, 1);
        assert!(s.store.read_source(&saved.slug).unwrap().contains("- work: 30"));
    }

    #[test]
    fn a_newer_copy_updates_in_place_and_keeps_its_own_stamp() {
        let s = stores("newer");
        let saved = s.store.save(&workout("Squats"), None, NOW).unwrap();
        let current = s.store.read_source(&saved.slug).unwrap();
        let fresh = ids::set_updated(&current.replace("- work: 30", "- work: 45"), LATER);

        let report = restore(&s.store, &s.plans, &s.days, &[Section::Workout(fresh)], NOW);
        assert_eq!(report.workouts.updated, 1);
        assert_eq!(s.store.list().len(), 1);
        let stored = s.store.read_source(&saved.slug).unwrap();
        assert!(stored.contains("- work: 45"));
        assert_eq!(
            ids::extract_updated(&stored).as_deref(),
            Some(LATER),
            "the restore must not restamp the document with its own time"
        );
    }

    #[test]
    fn a_finished_day_is_never_replaced() {
        let s = stores("done");
        let done = entry(&workout("Monday"), DayStatus::Done);
        s.days.save("2026-08-09", std::slice::from_ref(&done)).unwrap();

        // The same entry as it was before it was performed, and newer.
        let mut planned = done.clone();
        planned.status = DayStatus::Planned;
        planned.markdown = ids::set_updated(&planned.markdown, LATER);

        let report = restore(
            &s.store,
            &s.plans,
            &s.days,
            &[Section::Day { date: "2026-08-09".into(), entry: planned }],
            NOW,
        );
        assert_eq!(report.days.skipped, 1);
        assert_eq!(s.days.load("2026-08-09")[0].status, DayStatus::Done);
    }

    #[test]
    fn a_finished_backup_restores_over_a_planned_entry() {
        // The other direction: rebuilding a phone must bring the history back.
        let s = stores("history");
        let planned = entry(&workout("Monday"), DayStatus::Planned);
        s.days.save("2026-08-09", std::slice::from_ref(&planned)).unwrap();

        let mut done = planned.clone();
        done.status = DayStatus::Done;
        done.completed_at = Some(LATER.into());
        done.markdown = ids::set_updated(&done.markdown, LATER);

        restore(
            &s.store,
            &s.plans,
            &s.days,
            &[Section::Day { date: "2026-08-09".into(), entry: done }],
            NOW,
        );
        let stored = s.days.load("2026-08-09");
        assert_eq!(stored.len(), 1);
        assert_eq!(stored[0].status, DayStatus::Done);
        assert_eq!(stored[0].completed_at.as_deref(), Some(LATER));
    }

    #[test]
    fn a_restored_plan_does_not_reschedule_the_calendar() {
        // The day sections are the record of what was scheduled; a sync on top
        // of them would re-plan days over their restored state.
        let s = stores("nosync");
        let sections = parse(&build(&[], &[plan_doc()], &[], NOW)).unwrap();
        restore(&s.store, &s.plans, &s.days, &sections, NOW);

        assert_eq!(s.plans.list().len(), 1);
        assert!(s.days.load("2026-08-10").is_empty(), "restore must not sync");
    }

    #[test]
    fn a_day_entry_without_an_id_is_given_one() {
        let s = stores("dayid");
        let entry = DayEntry {
            markdown: workout("Handwritten"),
            status: DayStatus::Planned,
            completed_at: None,
            source_slug: None,
            source_plan: None,
        };
        restore(
            &s.store,
            &s.plans,
            &s.days,
            &[Section::Day { date: "2026-08-09".into(), entry }],
            NOW,
        );
        let stored = s.days.load("2026-08-09");
        assert!(days::entry_id(&stored[0]).is_some());
        assert_eq!(days::entry_updated(&stored[0]).as_deref(), Some(NOW));
    }

    #[test]
    fn exports_what_the_stores_hold() {
        let s = stores("export");
        s.store.save(&workout("Squats"), None, NOW).unwrap();
        s.plans.save(&plan_doc(), None, NOW).unwrap();
        s.days
            .add("2026-08-09", entry(&workout("Monday"), DayStatus::Done), NOW)
            .unwrap();

        let text = export(&s.store, &s.plans, &s.days, NOW);
        let sections = parse(&text).expect("an export must parse");
        assert_eq!(sections.len(), 3, "{text}");
    }
}
