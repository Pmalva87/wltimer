import { invoke } from "@tauri-apps/api/core";

export interface WorkoutSummary {
  slug: string;
  name: string;
  block_count: number;
  total_secs: number;
  error: string | null;
}

export interface ParseError {
  line: number;
  message: string;
}

export type Preview =
  | { status: "ok"; name: string; block_count: number; total_secs: number }
  | { status: "err"; errors: ParseError[] };

export type PhaseKind = "prepare" | "work" | "rest" | "block_rest";

export interface Phase {
  kind: PhaseKind;
  secs: number;
  block_idx: number;
  interval_idx: number;
}

export interface RunBlock {
  name: string;
  color: string | null;
  intervals: number;
  description_html: string;
}

export interface RunPlan {
  workout_name: string;
  blocks: RunBlock[];
  phases: Phase[];
  total_secs: number;
}

export type EngineState = "idle" | "running" | "paused" | "finished";

export interface Snapshot {
  state: EngineState;
  phase_idx: number;
  total_phases: number;
  phase_kind: PhaseKind | null;
  phase_secs: number;
  remaining_ms: number;
  total_remaining_ms: number;
  block_idx: number;
  interval_idx: number;
  next_kind: PhaseKind | null;
  next_block_idx: number | null;
}

export type Cue =
  | { kind: "pre_alert"; secs_left: number }
  | { kind: "phase_start"; phase: PhaseKind }
  | { kind: "finished" };

export const api = {
  listWorkouts: () => invoke<WorkoutSummary[]>("list_workouts"),
  getSource: (slug: string) => invoke<string>("get_workout_source", { slug }),
  saveWorkout: (source: string, prevSlug: string | null) =>
    invoke<WorkoutSummary>("save_workout", { source, prevSlug }),
  deleteWorkout: (slug: string) => invoke<void>("delete_workout", { slug }),
  parsePreview: (source: string) => invoke<Preview>("parse_preview", { source }),
  startWorkout: (slug: string) => invoke<RunPlan>("start_workout", { slug }),
  startQuick: (
    parts: { intervals: number; workSecs: number; restSecs: number }[],
    restBetweenSecs: number,
  ) => invoke<RunPlan>("start_quick", { parts, restBetweenSecs }),
  pause: () => invoke<void>("pause_timer"),
  resume: () => invoke<void>("resume_timer"),
  stop: () => invoke<void>("stop_timer"),
  skip: () => invoke<void>("skip_phase"),
  getSnapshot: () => invoke<Snapshot>("get_snapshot"),
};

export function fmtDuration(totalSecs: number): string {
  const h = Math.floor(totalSecs / 3600);
  const m = Math.floor((totalSecs % 3600) / 60);
  const s = totalSecs % 60;
  const mm = h > 0 ? String(m).padStart(2, "0") : String(m);
  const ss = String(s).padStart(2, "0");
  return h > 0 ? `${h}:${mm}:${ss}` : `${mm}:${ss}`;
}
