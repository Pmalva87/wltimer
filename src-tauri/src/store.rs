use serde::Serialize;
use std::fs;
use std::path::PathBuf;
use wltimer_core::parser::{self, ParseError};

pub const SAMPLE: &str = "\
# Example Workout

## Back Squat
- intervals: 5
- work: 2:00
- rest: 1:30
- rest after: 3:00

Brace hard, hit depth, drive up fast.

## Bench Press
- intervals: 3
- work: 1:00
- rest: 0:45
";

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

pub struct Store {
    dir: PathBuf,
}

impl Store {
    pub fn new(dir: PathBuf) -> std::io::Result<Self> {
        fs::create_dir_all(&dir)?;
        let store = Store { dir };
        if store.list().is_empty() {
            let _ = fs::write(store.path(&slugify("Example Workout")), SAMPLE);
        }
        Ok(store)
    }

    fn path(&self, slug: &str) -> PathBuf {
        self.dir.join(format!("{slug}.md"))
    }

    pub fn list(&self) -> Vec<WorkoutSummary> {
        let mut out = Vec::new();
        let Ok(entries) = fs::read_dir(&self.dir) else {
            return out;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some("md") {
                continue;
            }
            let slug = match path.file_stem().and_then(|s| s.to_str()) {
                Some(s) => s.to_string(),
                None => continue,
            };
            let Ok(source) = fs::read_to_string(&path) else {
                continue;
            };
            out.push(match parser::parse_workout(&source) {
                Ok(w) => WorkoutSummary {
                    slug,
                    name: w.name.clone(),
                    block_count: w.blocks.len(),
                    total_secs: w.total_secs(),
                    error: None,
                },
                Err(errs) => WorkoutSummary {
                    slug: slug.clone(),
                    name: slug,
                    block_count: 0,
                    total_secs: 0,
                    error: Some(format!("line {}: {}", errs[0].line, errs[0].message)),
                },
            });
        }
        out.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
        out
    }

    pub fn read_source(&self, slug: &str) -> Result<String, String> {
        fs::read_to_string(self.path(slug)).map_err(|e| format!("cannot read '{slug}': {e}"))
    }

    /// Validate and write. Returns parse errors if the source is invalid.
    /// When `prev_slug` is given and the workout was renamed, the old file is removed.
    pub fn save(&self, source: &str, prev_slug: Option<&str>) -> Result<WorkoutSummary, Vec<ParseError>> {
        let workout = parser::parse_workout(source)?;
        let slug = slugify(&workout.name);
        fs::write(self.path(&slug), source).map_err(|e| {
            vec![ParseError { line: 1, message: format!("cannot save: {e}") }]
        })?;
        if let Some(prev) = prev_slug {
            if prev != slug {
                let _ = fs::remove_file(self.path(prev));
            }
        }
        Ok(WorkoutSummary {
            slug,
            name: workout.name.clone(),
            block_count: workout.blocks.len(),
            total_secs: workout.total_secs(),
            error: None,
        })
    }

    pub fn delete(&self, slug: &str) -> Result<(), String> {
        fs::remove_file(self.path(slug)).map_err(|e| format!("cannot delete '{slug}': {e}"))
    }
}
