//! Organizations: the federations and clubs a competition names, kept as a
//! flat list the app helps you build rather than one it ships.
//!
//! `comp.rs` deliberately leaves `orgs`, `organizer` and a qualification's
//! `counts` as free text — no list the app owns would keep up with what a
//! lifter actually competes under. This store does not change that: it is
//! not the source of truth for those fields, only a picker's suggestions, so
//! nothing here is a foreign key. Deleting an organization never touches a
//! competition that already named it, and a competition can still name one
//! that was never added here — typed by hand, or added on a phone that has
//! since been wiped.
//!
//! A single JSON blob, in the style of [`crate::session::SessionStore`]:
//! there is nothing here worth a document of its own, so no id and no
//! `updated` stamp, and no place in a backup bundle.

use crate::zio;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct OrgList {
    names: Vec<String>,
}

pub struct OrgStore {
    path: PathBuf,
}

impl OrgStore {
    pub fn new(dir: PathBuf) -> std::io::Result<Self> {
        fs::create_dir_all(&dir)?;
        Ok(OrgStore {
            path: dir.join("organizations.json.zst"),
        })
    }

    fn read(&self) -> OrgList {
        zio::read_text(&self.path)
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default()
    }

    fn write(&self, list: &OrgList) -> Result<(), String> {
        let json = serde_json::to_string(list).map_err(|e| e.to_string())?;
        zio::write_compressed(&self.path, json.as_bytes())
            .map_err(|e| format!("cannot save organizations: {e}"))
    }

    /// Alphabetical, case-insensitively — the order a picker should offer
    /// them in, not the order they were added.
    pub fn list(&self) -> Vec<String> {
        let mut names = self.read().names;
        names.sort_by_key(|n| n.to_lowercase());
        names
    }

    /// Add a name and return the list as it stands after. Adding one already
    /// there — even spelled with different case — is a no-op rather than a
    /// second copy: "BWL" and "bwl" are the same body twice, not two of them.
    pub fn add(&self, name: &str) -> Result<Vec<String>, String> {
        let name = name.trim();
        if name.is_empty() {
            return Err("organization name cannot be empty".into());
        }
        let mut list = self.read();
        if !list.names.iter().any(|n| n.eq_ignore_ascii_case(name)) {
            list.names.push(name.to_string());
            self.write(&list)?;
        }
        Ok(self.list())
    }

    pub fn delete(&self, name: &str) -> Result<Vec<String>, String> {
        let mut list = self.read();
        list.names.retain(|n| !n.eq_ignore_ascii_case(name));
        self.write(&list)?;
        Ok(self.list())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_store(tag: &str) -> OrgStore {
        let dir = std::env::temp_dir().join(format!("wltimer-orgs-{tag}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        OrgStore::new(dir).unwrap()
    }

    #[test]
    fn starts_empty() {
        let s = temp_store("starts_empty");
        assert!(s.list().is_empty());
    }

    #[test]
    fn adds_and_lists_alphabetically() {
        let s = temp_store("adds_and_lists_alphabetically");
        s.add("IWF").unwrap();
        s.add("BWL").unwrap();
        assert_eq!(s.list(), vec!["BWL", "IWF"]);
    }

    #[test]
    fn adding_the_same_name_twice_is_not_a_duplicate() {
        let s = temp_store("adding_the_same_name_twice_is_not_a_duplicate");
        s.add("BWL").unwrap();
        s.add("bwl").unwrap();
        assert_eq!(s.list(), vec!["BWL"]);
    }

    #[test]
    fn blank_names_are_rejected() {
        let s = temp_store("blank_names_are_rejected");
        assert!(s.add("   ").is_err());
        assert!(s.list().is_empty());
    }

    #[test]
    fn deletes_case_insensitively() {
        let s = temp_store("deletes_case_insensitively");
        s.add("BWL").unwrap();
        let after = s.delete("bwl").unwrap();
        assert!(after.is_empty());
    }

    #[test]
    fn survives_a_fresh_store_over_the_same_directory() {
        let dir = std::env::temp_dir().join(format!(
            "wltimer-orgs-persists-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&dir);
        OrgStore::new(dir.clone()).unwrap().add("FPH").unwrap();
        assert_eq!(OrgStore::new(dir).unwrap().list(), vec!["FPH"]);
    }
}
