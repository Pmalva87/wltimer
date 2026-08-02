use serde::{Deserialize, Serialize};

/// Seconds of "get ready" countdown before the first work interval.
pub const PREPARE_SECS: u32 = 10;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Workout {
    /// Stable UUID identifying this workout document, carried in the markdown
    /// as `- id:` under the title. `None` only for a document that has not
    /// been through a store yet — everything written to disk has one, so
    /// re-imports and plan syncs can match a workout instead of copying it.
    #[serde(default)]
    pub id: Option<String>,
    pub name: String,
    pub blocks: Vec<Block>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Block {
    pub name: String,
    /// Free markdown prose from the block's section, shown during the block.
    pub description_md: String,
    pub intervals: u32,
    pub work_secs: u32,
    /// Rest between intervals within this block.
    pub rest_secs: Option<u32>,
    /// Rest after the whole block, before the next one.
    pub rest_after_secs: Option<u32>,
    /// Optional work-phase color override, e.g. "#e11d48".
    pub color: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PhaseKind {
    Prepare,
    Work,
    Rest,
    BlockRest,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Phase {
    pub kind: PhaseKind,
    pub secs: u32,
    pub block_idx: usize,
    /// 1-based interval number within the block (0 for Prepare/BlockRest).
    pub interval_idx: u32,
}

impl Workout {
    /// Flatten into the linear sequence of timed phases the engine runs through.
    pub fn flatten(&self) -> Vec<Phase> {
        let mut phases = Vec::new();
        phases.push(Phase {
            kind: PhaseKind::Prepare,
            secs: PREPARE_SECS,
            block_idx: 0,
            interval_idx: 0,
        });
        let last_block = self.blocks.len().saturating_sub(1);
        for (bi, block) in self.blocks.iter().enumerate() {
            for i in 1..=block.intervals {
                phases.push(Phase {
                    kind: PhaseKind::Work,
                    secs: block.work_secs,
                    block_idx: bi,
                    interval_idx: i,
                });
                if i < block.intervals {
                    if let Some(rest) = block.rest_secs.filter(|&r| r > 0) {
                        phases.push(Phase {
                            kind: PhaseKind::Rest,
                            secs: rest,
                            block_idx: bi,
                            interval_idx: i,
                        });
                    }
                }
            }
            if bi < last_block {
                if let Some(rest) = block.rest_after_secs.filter(|&r| r > 0) {
                    phases.push(Phase {
                        kind: PhaseKind::BlockRest,
                        secs: rest,
                        block_idx: bi,
                        interval_idx: 0,
                    });
                }
            }
        }
        phases
    }

    /// Total workout duration in seconds, including the prepare countdown.
    pub fn total_secs(&self) -> u32 {
        self.flatten().iter().map(|p| p.secs).sum()
    }

    /// Parts that run straight into the next one, with nothing in between —
    /// see `flatten`, which drops a zero or absent "rest after".
    ///
    /// Not an error: a workout is free to chain parts back to back and runs
    /// exactly as written. It is far more often an oversight than a choice,
    /// though, so every screen that shows a workout says so.
    pub fn parts_without_rest_after(&self) -> Vec<usize> {
        let last = self.blocks.len().saturating_sub(1);
        self.blocks
            .iter()
            .enumerate()
            .take(last)
            .filter(|(_, b)| b.rest_after_secs.unwrap_or(0) == 0)
            .map(|(i, _)| i)
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn block(intervals: u32, work: u32, rest: Option<u32>, after: Option<u32>) -> Block {
        Block {
            name: "b".into(),
            description_md: String::new(),
            intervals,
            work_secs: work,
            rest_secs: rest,
            rest_after_secs: after,
            color: None,
        }
    }

    #[test]
    fn flatten_single_block() {
        let w = Workout {
            id: None,
            name: "w".into(),
            blocks: vec![block(3, 60, Some(30), Some(120))],
        };
        let kinds: Vec<_> = w.flatten().iter().map(|p| (p.kind, p.secs)).collect();
        // No rest after the last interval, no block-rest after the last block.
        assert_eq!(
            kinds,
            vec![
                (PhaseKind::Prepare, PREPARE_SECS),
                (PhaseKind::Work, 60),
                (PhaseKind::Rest, 30),
                (PhaseKind::Work, 60),
                (PhaseKind::Rest, 30),
                (PhaseKind::Work, 60),
            ]
        );
    }

    #[test]
    fn flatten_two_blocks_with_block_rest() {
        let w = Workout {
            id: None,
            name: "w".into(),
            blocks: vec![block(2, 60, None, Some(90)), block(1, 45, None, Some(30))],
        };
        let kinds: Vec<_> = w.flatten().iter().map(|p| (p.kind, p.secs)).collect();
        assert_eq!(
            kinds,
            vec![
                (PhaseKind::Prepare, PREPARE_SECS),
                (PhaseKind::Work, 60),
                (PhaseKind::Work, 60),
                (PhaseKind::BlockRest, 90),
                (PhaseKind::Work, 45),
            ]
        );
        assert_eq!(w.total_secs(), PREPARE_SECS + 60 + 60 + 90 + 45);
    }

    #[test]
    fn parts_running_into_the_next_are_flagged() {
        let w = Workout {
            id: None,
            name: "w".into(),
            blocks: vec![
                block(1, 60, None, Some(90)),
                block(1, 60, None, None),
                block(1, 60, None, Some(0)),
                // The last part has nothing to run into, so it never counts,
                // whatever its "rest after" says.
                block(1, 60, None, None),
            ],
        };
        assert_eq!(w.parts_without_rest_after(), vec![1, 2]);
    }

    #[test]
    fn a_single_part_never_runs_into_anything() {
        let w = Workout {
            id: None,
            name: "w".into(),
            blocks: vec![block(3, 60, Some(30), None)],
        };
        assert!(w.parts_without_rest_after().is_empty());
    }

    #[test]
    fn zero_rest_is_skipped() {
        let w = Workout {
            id: None,
            name: "w".into(),
            blocks: vec![block(2, 60, Some(0), None)],
        };
        assert!(w.flatten().iter().all(|p| p.kind != PhaseKind::Rest));
    }
}
