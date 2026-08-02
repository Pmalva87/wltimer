//! The one run that is in progress but not on screen, kept on disk so leaving
//! the timer (or the app being killed mid-workout) does not throw the session
//! away. A session only lives for its day: coming back tomorrow to a workout
//! half-done is not something to walk into blind, so a stale session is
//! dropped and the workout starts over.

use crate::model::Workout;
use crate::zio;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

/// Where a run came from — decides how a finished run is recorded on the
/// calendar, and which start screen offers to pick a suspended one back up.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum RunOrigin {
    None,
    /// A scheduled calendar entry: flip it to done.
    Day { date: String, index: usize },
    /// A library template: append a done entry to the start date.
    Library { date: String, slug: String },
    /// An unsaved builder run: append a done entry to the start date.
    Adhoc { date: String },
}

impl RunOrigin {
    /// The run-screen route this origin belongs to, i.e. the `<target>` in
    /// `#/run/<target>`. `None` when no run is under way.
    pub fn target(&self) -> Option<String> {
        match self {
            RunOrigin::None => None,
            RunOrigin::Day { date, index } => Some(format!("@{date}:{index}")),
            RunOrigin::Library { slug, .. } => Some(slug.clone()),
            RunOrigin::Adhoc { .. } => Some("draft".into()),
        }
    }
}

/// A run frozen mid-flight: the workout itself travels with it, so resuming
/// does not depend on the library entry or calendar entry still being there.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SavedSession {
    /// Local date the session was written on — not the origin's date, which
    /// may be a future day for a calendar entry started ahead of time.
    pub date: String,
    pub origin: RunOrigin,
    pub workout: Workout,
    pub phase_idx: usize,
    /// How far into `phase_idx` the run had got.
    pub elapsed_ms: u64,
}

impl SavedSession {
    /// Seconds of workout left from where it was suspended.
    pub fn remaining_secs(&self) -> u32 {
        let left: u32 = self
            .workout
            .flatten()
            .iter()
            .skip(self.phase_idx)
            .map(|p| p.secs)
            .sum();
        left.saturating_sub((self.elapsed_ms / 1000) as u32)
    }
}

/// Holds at most one session — the timer runs one workout at a time, and
/// starting another replaces it.
pub struct SessionStore {
    path: PathBuf,
}

impl SessionStore {
    pub fn new(dir: PathBuf) -> std::io::Result<Self> {
        fs::create_dir_all(&dir)?;
        Ok(SessionStore {
            path: dir.join("session.json.zst"),
        })
    }

    /// The suspended session, if there is one and it belongs to `today`. One
    /// left over from an earlier day is deleted rather than offered.
    pub fn load(&self, today: &str) -> Option<SavedSession> {
        let saved: SavedSession = zio::read_text(&self.path)
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())?;
        if saved.date == today {
            Some(saved)
        } else {
            self.clear();
            None
        }
    }

    pub fn save(&self, session: &SavedSession) -> Result<(), String> {
        let json = serde_json::to_string(session).map_err(|e| e.to_string())?;
        zio::write_compressed(&self.path, json.as_bytes())
            .map_err(|e| format!("cannot save session: {e}"))
    }

    pub fn clear(&self) {
        let _ = fs::remove_file(&self.path);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Block, PREPARE_SECS};

    fn workout() -> Workout {
        Workout {
            id: None,
            name: "w".into(),
            blocks: vec![Block {
                name: "a".into(),
                description_md: String::new(),
                intervals: 2,
                work_secs: 60,
                rest_secs: Some(30),
                rest_after_secs: None,
                color: None,
            }],
        }
    }

    fn session(date: &str) -> SavedSession {
        SavedSession {
            date: date.into(),
            origin: RunOrigin::Library {
                date: date.into(),
                slug: "squats".into(),
            },
            workout: workout(),
            phase_idx: 1,
            elapsed_ms: 20_000,
        }
    }

    fn temp_store(tag: &str) -> SessionStore {
        let dir = std::env::temp_dir().join(format!("wltimer-session-{tag}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        SessionStore::new(dir).unwrap()
    }

    #[test]
    fn round_trips_a_session_from_today() {
        let s = temp_store("today");
        assert!(s.load("2026-08-02").is_none());
        s.save(&session("2026-08-02")).unwrap();
        let loaded = s.load("2026-08-02").expect("saved today");
        assert_eq!(loaded.phase_idx, 1);
        assert_eq!(loaded.workout.name, "w");
        assert_eq!(
            loaded.origin,
            RunOrigin::Library {
                date: "2026-08-02".into(),
                slug: "squats".into()
            }
        );
    }

    #[test]
    fn yesterdays_session_is_dropped() {
        let s = temp_store("stale");
        s.save(&session("2026-08-01")).unwrap();
        assert!(s.load("2026-08-02").is_none());
        // Gone for good, so it cannot resurface if the clock moves back.
        assert!(s.load("2026-08-01").is_none());
    }

    #[test]
    fn clear_removes_the_session() {
        let s = temp_store("clear");
        s.save(&session("2026-08-02")).unwrap();
        s.clear();
        assert!(s.load("2026-08-02").is_none());
        // Clearing when there is nothing saved is fine.
        s.clear();
    }

    #[test]
    fn remaining_counts_the_current_phase_and_the_rest() {
        // Prepare(10) Work(60) Rest(30) Work(60); suspended 20s into the first
        // work interval leaves 40 + 30 + 60.
        let s = session("2026-08-02");
        assert_eq!(s.remaining_secs(), 130);
        assert_eq!(s.workout.total_secs(), PREPARE_SECS + 60 + 30 + 60);
    }

    #[test]
    fn origin_maps_to_the_run_route() {
        assert_eq!(RunOrigin::None.target(), None);
        assert_eq!(
            RunOrigin::Day {
                date: "2026-08-02".into(),
                index: 3
            }
            .target()
            .as_deref(),
            Some("@2026-08-02:3")
        );
        assert_eq!(
            RunOrigin::Adhoc {
                date: "2026-08-02".into()
            }
            .target()
            .as_deref(),
            Some("draft")
        );
    }
}
