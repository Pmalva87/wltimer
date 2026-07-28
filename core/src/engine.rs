use crate::model::{Phase, PhaseKind, Workout};
use serde::Serialize;
use std::time::{Duration, Instant};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum State {
    Idle,
    Running,
    Paused,
    Finished,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Cue {
    /// Fires once at ~3, ~2 and ~1 whole seconds remaining in any phase.
    PreAlert { secs_left: u32 },
    PhaseStart { phase: PhaseKind },
    Finished,
}

/// Point-in-time view of the engine, serialized to the UI on every tick.
#[derive(Debug, Clone, Serialize)]
pub struct Snapshot {
    pub state: State,
    pub phase_idx: usize,
    pub total_phases: usize,
    pub phase_kind: Option<PhaseKind>,
    pub phase_secs: u32,
    pub remaining_ms: u64,
    /// Time left for the whole workout: current phase remainder + all later phases.
    pub total_remaining_ms: u64,
    pub block_idx: usize,
    pub interval_idx: u32,
    pub next_kind: Option<PhaseKind>,
    pub next_block_idx: Option<usize>,
}

pub struct Engine {
    workout: Option<Workout>,
    phases: Vec<Phase>,
    idx: usize,
    state: State,
    /// Start instant of the current phase; shifted forward on resume.
    phase_start: Option<Instant>,
    /// Elapsed time in the current phase at the moment of pausing.
    paused_elapsed: Duration,
    /// Whole seconds remaining last time we looked (pre-alert edge detection).
    prev_remaining_ceil: u32,
}

impl Default for Engine {
    fn default() -> Self {
        Self::new()
    }
}

impl Engine {
    pub fn new() -> Self {
        Engine {
            workout: None,
            phases: Vec::new(),
            idx: 0,
            state: State::Idle,
            phase_start: None,
            paused_elapsed: Duration::ZERO,
            prev_remaining_ceil: 0,
        }
    }

    pub fn state(&self) -> State {
        self.state
    }

    pub fn workout(&self) -> Option<&Workout> {
        self.workout.as_ref()
    }

    pub fn phases(&self) -> &[Phase] {
        &self.phases
    }

    pub fn start(&mut self, workout: Workout, now: Instant) -> Vec<Cue> {
        let phases = workout.flatten();
        debug_assert!(!phases.is_empty());
        self.prev_remaining_ceil = phases[0].secs;
        self.workout = Some(workout);
        self.phases = phases;
        self.idx = 0;
        self.state = State::Running;
        self.phase_start = Some(now);
        self.paused_elapsed = Duration::ZERO;
        vec![Cue::PhaseStart {
            phase: self.phases[0].kind,
        }]
    }

    pub fn pause(&mut self, now: Instant) {
        if self.state == State::Running {
            self.paused_elapsed = now - self.phase_start.expect("running engine has phase_start");
            self.state = State::Paused;
        }
    }

    pub fn resume(&mut self, now: Instant) {
        if self.state == State::Paused {
            self.phase_start = Some(now - self.paused_elapsed);
            self.state = State::Running;
        }
    }

    pub fn stop(&mut self) {
        *self = Engine::new();
    }

    /// Jump to the next phase immediately.
    pub fn skip(&mut self, now: Instant) -> Vec<Cue> {
        if self.state != State::Running && self.state != State::Paused {
            return Vec::new();
        }
        self.enter_phase(self.idx + 1, now)
    }

    fn enter_phase(&mut self, idx: usize, now: Instant) -> Vec<Cue> {
        if idx >= self.phases.len() {
            self.state = State::Finished;
            return vec![Cue::Finished];
        }
        self.idx = idx;
        self.phase_start = Some(now);
        self.paused_elapsed = Duration::ZERO;
        self.prev_remaining_ceil = self.phases[idx].secs;
        vec![Cue::PhaseStart {
            phase: self.phases[idx].kind,
        }]
    }

    /// Advance the clock to `now`, moving through any phase boundaries passed,
    /// and return the cues that fired.
    pub fn advance(&mut self, now: Instant) -> Vec<Cue> {
        let mut cues = Vec::new();
        if self.state != State::Running {
            return cues;
        }
        loop {
            let phase_len = Duration::from_secs(self.phases[self.idx].secs as u64);
            let start = self.phase_start.expect("running engine has phase_start");
            let elapsed = now.saturating_duration_since(start);
            if elapsed >= phase_len {
                if self.idx + 1 >= self.phases.len() {
                    self.state = State::Finished;
                    cues.push(Cue::Finished);
                    return cues;
                }
                // Anchor the next phase at the exact boundary so no time drifts.
                self.idx += 1;
                self.phase_start = Some(start + phase_len);
                self.prev_remaining_ceil = self.phases[self.idx].secs;
                cues.push(Cue::PhaseStart {
                    phase: self.phases[self.idx].kind,
                });
            } else {
                let remaining = phase_len - elapsed;
                let ceil = remaining.as_millis().div_ceil(1000) as u32;
                if ceil < self.prev_remaining_ceil && (1..=3).contains(&ceil) {
                    cues.push(Cue::PreAlert { secs_left: ceil });
                }
                self.prev_remaining_ceil = ceil;
                return cues;
            }
        }
    }

    pub fn snapshot(&self, now: Instant) -> Snapshot {
        let phase = self.phases.get(self.idx);
        let remaining_ms = match (self.state, phase) {
            (State::Running, Some(p)) => {
                let elapsed = now.saturating_duration_since(self.phase_start.expect("running"));
                Duration::from_secs(p.secs as u64)
                    .saturating_sub(elapsed)
                    .as_millis() as u64
            }
            (State::Paused, Some(p)) => Duration::from_secs(p.secs as u64)
                .saturating_sub(self.paused_elapsed)
                .as_millis() as u64,
            _ => 0,
        };
        let showing = matches!(self.state, State::Running | State::Paused);
        let next = if showing {
            self.phases.get(self.idx + 1)
        } else {
            None
        };
        let total_remaining_ms = if showing {
            let future_ms: u64 = self.phases[self.idx + 1..]
                .iter()
                .map(|p| p.secs as u64 * 1000)
                .sum();
            remaining_ms + future_ms
        } else {
            0
        };
        Snapshot {
            state: self.state,
            phase_idx: self.idx,
            total_phases: self.phases.len(),
            phase_kind: if showing { phase.map(|p| p.kind) } else { None },
            phase_secs: phase.map(|p| p.secs).unwrap_or(0),
            remaining_ms,
            total_remaining_ms,
            block_idx: phase.map(|p| p.block_idx).unwrap_or(0),
            interval_idx: phase.map(|p| p.interval_idx).unwrap_or(0),
            next_kind: next.map(|p| p.kind),
            next_block_idx: next.map(|p| p.block_idx),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Block, PREPARE_SECS};

    fn workout() -> Workout {
        Workout {
            name: "w".into(),
            blocks: vec![
                Block {
                    name: "a".into(),
                    description_md: String::new(),
                    intervals: 2,
                    work_secs: 10,
                    rest_secs: Some(5),
                    rest_after_secs: Some(7),
                    color: None,
                },
                Block {
                    name: "b".into(),
                    description_md: String::new(),
                    intervals: 1,
                    work_secs: 8,
                    rest_secs: None,
                    rest_after_secs: None,
                    color: None,
                },
            ],
        }
    }

    fn secs(s: u64) -> Duration {
        Duration::from_secs(s)
    }

    #[test]
    fn runs_through_all_phases() {
        // Prepare(10) Work(10) Rest(5) Work(10) BlockRest(7) Work(8) = 50s total
        let t0 = Instant::now();
        let mut e = Engine::new();
        let cues = e.start(workout(), t0);
        assert_eq!(cues, vec![Cue::PhaseStart { phase: PhaseKind::Prepare }]);

        let cues = e.advance(t0 + secs(PREPARE_SECS as u64));
        assert_eq!(cues, vec![Cue::PhaseStart { phase: PhaseKind::Work }]);
        assert_eq!(e.snapshot(t0 + secs(10)).interval_idx, 1);

        // Jump over several boundaries at once: land 1s into the 2nd work interval.
        let cues = e.advance(t0 + secs(26));
        assert_eq!(
            cues,
            vec![
                Cue::PhaseStart { phase: PhaseKind::Rest },
                Cue::PhaseStart { phase: PhaseKind::Work },
            ]
        );
        let snap = e.snapshot(t0 + secs(26));
        assert_eq!(snap.phase_kind, Some(PhaseKind::Work));
        assert_eq!(snap.interval_idx, 2);
        assert_eq!(snap.remaining_ms, 9000);
        // 50s workout, 26s in: 24s left overall.
        assert_eq!(snap.total_remaining_ms, 24_000);

        let cues = e.advance(t0 + secs(36));
        assert_eq!(cues, vec![Cue::PhaseStart { phase: PhaseKind::BlockRest }]);

        let cues = e.advance(t0 + secs(43));
        assert_eq!(cues, vec![Cue::PhaseStart { phase: PhaseKind::Work }]);
        assert_eq!(e.snapshot(t0 + secs(43)).block_idx, 1);

        let cues = e.advance(t0 + secs(51));
        assert_eq!(cues, vec![Cue::Finished]);
        assert_eq!(e.state(), State::Finished);
    }

    #[test]
    fn prealert_fires_at_3_2_1() {
        let t0 = Instant::now();
        let mut e = Engine::new();
        e.start(workout(), t0);
        let mut fired = Vec::new();
        // Tick every 200ms through the 10s prepare phase.
        for ms in (0..10_000).step_by(200) {
            for cue in e.advance(t0 + Duration::from_millis(ms)) {
                if let Cue::PreAlert { secs_left } = cue {
                    fired.push(secs_left);
                }
            }
        }
        assert_eq!(fired, vec![3, 2, 1]);
    }

    #[test]
    fn prealert_not_duplicated_within_same_second() {
        let t0 = Instant::now();
        let mut e = Engine::new();
        e.start(workout(), t0);
        let a = e.advance(t0 + Duration::from_millis(7100)); // 2.9s left -> ceil 3
        let b = e.advance(t0 + Duration::from_millis(7300)); // 2.7s left -> ceil 3 again
        assert_eq!(a, vec![Cue::PreAlert { secs_left: 3 }]);
        assert_eq!(b, vec![]);
    }

    #[test]
    fn pause_freezes_remaining_time() {
        let t0 = Instant::now();
        let mut e = Engine::new();
        e.start(workout(), t0);
        e.advance(t0 + secs(4));
        e.pause(t0 + secs(4));
        assert_eq!(e.state(), State::Paused);
        // Time passes while paused; remaining stays put and advance is a no-op.
        assert_eq!(e.snapshot(t0 + secs(60)).remaining_ms, 6000);
        assert_eq!(e.advance(t0 + secs(60)), vec![]);
        e.resume(t0 + secs(60));
        // 6s left on the prepare phase after resuming.
        assert_eq!(e.snapshot(t0 + secs(63)).remaining_ms, 3000);
        let cues = e.advance(t0 + secs(66));
        assert_eq!(cues, vec![Cue::PhaseStart { phase: PhaseKind::Work }]);
    }

    #[test]
    fn skip_jumps_to_next_phase() {
        let t0 = Instant::now();
        let mut e = Engine::new();
        e.start(workout(), t0);
        let cues = e.skip(t0 + secs(2));
        assert_eq!(cues, vec![Cue::PhaseStart { phase: PhaseKind::Work }]);
        let snap = e.snapshot(t0 + secs(2));
        assert_eq!(snap.phase_kind, Some(PhaseKind::Work));
        assert_eq!(snap.remaining_ms, 10_000);
    }

    #[test]
    fn skip_on_last_phase_finishes() {
        let t0 = Instant::now();
        let mut e = Engine::new();
        e.start(workout(), t0);
        let mut cues;
        loop {
            cues = e.skip(t0);
            if cues.contains(&Cue::Finished) {
                break;
            }
        }
        assert_eq!(e.state(), State::Finished);
    }

    #[test]
    fn stop_resets_to_idle() {
        let t0 = Instant::now();
        let mut e = Engine::new();
        e.start(workout(), t0);
        e.stop();
        assert_eq!(e.state(), State::Idle);
        assert!(e.workout().is_none());
    }
}
