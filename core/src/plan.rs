//! Training plans: one markdown file describing multiple dated workout days.
//!
//! Format: `# Plan Name`, then one `## YYYY-MM-DD: Day Name` section per day,
//! whose `###` headings are the exercises (same params/notes as a single
//! workout). Each day converts to a standalone workout document (`##` → `#`,
//! `###` → `##`) that gets scheduled on the calendar.

use crate::days::valid_date;
use crate::parser::{self, ParseError};
use crate::store::{retitle, slugify};
use crate::zio;
use serde::Serialize;
use std::fs;
use std::path::PathBuf;

#[derive(Debug, Clone, PartialEq)]
pub struct PlanDay {
    pub date: String,
    pub name: String,
    /// Standalone workout markdown for this day.
    pub workout_md: String,
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
        if days.iter().any(|d: &PlanDay| d.date == b.date) {
            errors.push(err(b.heading_line, format!("duplicate day '{}'", b.date)));
        }
        days.push(PlanDay { date: b.date, name: b.name, workout_md });
    }
    days.sort_by(|a, b| a.date.cmp(&b.date));

    if errors.is_empty() {
        Ok(Plan { name: plan_name, days })
    } else {
        errors.sort_by_key(|e| e.line);
        Err(errors)
    }
}

pub struct PlanStore {
    dir: PathBuf,
}

impl PlanStore {
    pub fn new(dir: PathBuf) -> std::io::Result<Self> {
        fs::create_dir_all(&dir)?;
        Ok(PlanStore { dir })
    }

    fn path(&self, slug: &str) -> PathBuf {
        self.dir.join(format!("{slug}.md.zst"))
    }

    pub fn list(&self) -> Vec<PlanSummary> {
        let mut out = Vec::new();
        let Ok(entries) = fs::read_dir(&self.dir) else {
            return out;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
            let Some(slug) = name.strip_suffix(".md.zst") else {
                continue;
            };
            let Ok(source) = zio::read_text(&path) else {
                continue;
            };
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
    pub fn save(&self, source: &str, prev_slug: Option<&str>) -> Result<(PlanSummary, Plan), Vec<ParseError>> {
        let plan = parse_plan(source)?;
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

    pub fn delete(&self, slug: &str) -> Result<(), String> {
        fs::remove_file(self.path(slug)).map_err(|e| format!("cannot delete plan '{slug}': {e}"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
    fn rejects_duplicate_dates() {
        let src = "# P\n\n## 2026-07-30: A\n### X\n- work: 30\n\n## 2026-07-30: B\n### Y\n- work: 30\n";
        let errs = parse_plan(src).unwrap_err();
        assert!(errs.iter().any(|e| e.message.contains("duplicate day")));
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
        let (sum, plan) = s.save(PLAN, None).unwrap();
        assert_eq!(sum.slug, "531-cycle-1");
        assert_eq!(sum.day_count, 2);
        assert_eq!(sum.first_date, "2026-07-30");
        assert_eq!(plan.days.len(), 2);
        let (dup, _) = s.save(PLAN, None).unwrap();
        assert_eq!(dup.slug, "531-cycle-1-2");
        // Re-saving under its own slug replaces in place.
        let (resaved, _) = s.save(PLAN, Some("531-cycle-1")).unwrap();
        assert_eq!(resaved.slug, "531-cycle-1");
        assert_eq!(s.list().len(), 2);
        s.delete("531-cycle-1-2").unwrap();
        assert_eq!(s.list().len(), 1);
    }
}
