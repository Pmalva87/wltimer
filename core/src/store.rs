//! Library of workout templates: one zstd-compressed markdown file per workout.

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

    fn path(&self, slug: &str) -> PathBuf {
        self.dir.join(format!("{slug}.md.zst"))
    }

    fn write(&self, slug: &str, source: &str) -> std::io::Result<()> {
        zio::write_compressed(&self.path(slug), source.as_bytes())
    }

    pub fn list(&self) -> Vec<WorkoutSummary> {
        let mut by_slug: BTreeMap<String, WorkoutSummary> = BTreeMap::new();
        let Ok(entries) = fs::read_dir(&self.dir) else {
            return Vec::new();
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
            let slug = match name.strip_suffix(".md.zst").or_else(|| name.strip_suffix(".md")) {
                Some(s) if !s.is_empty() => s.to_string(),
                _ => continue,
            };
            let Ok(source) = zio::read_text(&path) else {
                continue;
            };
            let summary = match parser::parse_workout(&source) {
                Ok(w) => WorkoutSummary {
                    slug: slug.clone(),
                    name: w.name.clone(),
                    block_count: w.blocks.len(),
                    total_secs: w.total_secs(),
                    error: None,
                },
                Err(errs) => WorkoutSummary {
                    slug: slug.clone(),
                    name: slug.clone(),
                    block_count: 0,
                    total_secs: 0,
                    error: Some(format!("line {}: {}", errs[0].line, errs[0].message)),
                },
            };
            by_slug.insert(slug, summary);
        }
        let mut out: Vec<WorkoutSummary> = by_slug.into_values().collect();
        out.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
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
    /// When `prev_slug` is given and the workout was renamed, the old file is
    /// removed. A name that collides with a *different* stored workout is
    /// deduplicated with a counter: "Squats" → "Squats (2)", "Squats (3)"…
    /// (the document's title line is rewritten to match).
    pub fn save(
        &self,
        source: &str,
        prev_slug: Option<&str>,
    ) -> Result<WorkoutSummary, Vec<parser::ParseError>> {
        let workout = parser::parse_workout(source)?;
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
