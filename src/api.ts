import { invoke } from "@tauri-apps/api/core";

export interface WorkoutSummary {
  slug: string;
  name: string;
  block_count: number;
  total_secs: number;
  /** How many parts run straight into the next one. */
  parts_without_rest: number;
  error: string | null;
}

export interface ParseError {
  line: number;
  message: string;
}

export type Preview =
  | {
      status: "ok";
      name: string;
      block_count: number;
      total_secs: number;
      /** How many parts run straight into the next one. */
      parts_without_rest: number;
    }
  | { status: "err"; errors: ParseError[] };

export interface Block {
  name: string;
  description_md: string;
  intervals: number;
  work_secs: number;
  rest_secs: number | null;
  rest_after_secs: number | null;
  color: string | null;
}

export interface Workout {
  /** Stable id carried in the workout's markdown; the backend mints one on
   *  save. Must be preserved across edits — dropping it makes the next save
   *  look like a brand-new workout. */
  id: string | null;
  name: string;
  blocks: Block[];
}

export interface ViewBlock {
  name: string;
  color: string | null;
  intervals: number;
  work_secs: number;
  rest_secs: number | null;
  rest_after_secs: number | null;
  /** Work + between-interval rest for this part, excluding the rest after it. */
  block_secs: number;
  /** This part runs straight into the next one, with nothing in between. */
  no_rest_after: boolean;
  description_html: string;
}

export interface WorkoutView {
  id: string | null;
  name: string;
  total_secs: number;
  blocks: ViewBlock[];
}

export type ParseFull =
  | { status: "ok"; workout: Workout }
  | { status: "err"; errors: ParseError[] };

/** What a restore did to one kind of document. */
export interface Counts {
  added: number;
  updated: number;
  /** Left alone: what is stored is newer, or it is a finished day. */
  skipped: number;
}

export interface ImportReport {
  workouts: Counts;
  plans: Counts;
  days: Counts;
  failed: number;
}

export type BundlePreview =
  | { status: "ok"; workouts: number; plans: number; days: number }
  | { status: "not_bundle" }
  | { status: "err"; errors: ParseError[] };

export type DayStatus = "planned" | "done";

export interface DayEntryInfo {
  name: string;
  status: DayStatus;
  completed_at: string | null;
  source_slug: string | null;
  source_plan: string | null;
  markdown: string;
}

export interface PlanSummary {
  slug: string;
  name: string;
  day_count: number;
  first_date: string;
  last_date: string;
  error: string | null;
}

/** What an uploaded plan file did to the plan it belongs to. */
export interface PlanImport {
  summary: PlanSummary;
  updated: number;
  added: number;
  synced: number;
}

export interface DaySummary {
  date: string;
  entries: { name: string; status: DayStatus }[];
}

/** Local date as YYYY-MM-DD (the user's timezone, not UTC). */
export function todayStr(): string {
  const d = new Date();
  return `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, "0")}-${String(d.getDate()).padStart(2, "0")}`;
}

export const PREPARE_SECS = 10;

/** Mirrors Workout::flatten totals: prepare + work + in-block rests + between-block rests. */
export function workoutTotalSecs(w: Workout): number {
  const last = w.blocks.length - 1;
  return (
    PREPARE_SECS +
    w.blocks.reduce(
      (sum, b, i) =>
        sum +
        b.intervals * b.work_secs +
        (b.intervals - 1) * (b.rest_secs ?? 0) +
        (i < last ? (b.rest_after_secs ?? 0) : 0),
      0,
    )
  );
}

/**
 * Mirrors Workout::parts_without_rest_after: the parts that run straight into
 * the next one. Used for live feedback in the builder, where the workout only
 * exists in the form; everything stored comes with the backend's own answer.
 */
export function partsWithoutRestAfter(w: Workout): number[] {
  return w.blocks.flatMap((b, i) =>
    i < w.blocks.length - 1 && !b.rest_after_secs ? [i] : [],
  );
}

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

/** A run left unfinished earlier today, waiting to be resumed or started over. */
export interface SessionInfo {
  /** The `#/run/<target>` route this session belongs to. */
  target: string;
  workout_name: string;
  phase_idx: number;
  total_phases: number;
  remaining_secs: number;
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
  duplicateWorkout: (slug: string) =>
    invoke<WorkoutSummary>("duplicate_workout", { slug }),
  deleteWorkout: (slug: string) => invoke<void>("delete_workout", { slug }),
  viewWorkout: (source: string) => invoke<WorkoutView>("view_workout", { source }),
  parsePreview: (source: string) => invoke<Preview>("parse_preview", { source }),
  startWorkout: (slug: string) =>
    invoke<RunPlan>("start_workout", { slug, today: todayStr() }),
  startCustom: (workout: Workout) =>
    invoke<RunPlan>("start_custom", { workout, today: todayStr() }),
  startDayEntry: (date: string, index: number) =>
    invoke<RunPlan>("start_day_entry", { date, index }),
  parseFull: (source: string) => invoke<ParseFull>("parse_full", { source }),
  serializeWorkout: (workout: Workout) => invoke<string>("serialize_workout", { workout }),
  getMonth: (year: number, month: number) => invoke<DaySummary[]>("get_month", { year, month }),
  getDay: (date: string) => invoke<DayEntryInfo[]>("get_day", { date }),
  addDayEntry: (date: string, source: string) =>
    invoke<number>("add_day_entry", { date, source }),
  addDayFromLibrary: (date: string, slug: string) =>
    invoke<number>("add_day_from_library", { date, slug }),
  updateDayEntry: (date: string, index: number, source: string) =>
    invoke<void>("update_day_entry", { date, index, source }),
  deleteDayEntry: (date: string, index: number) =>
    invoke<void>("delete_day_entry", { date, index }),
  moveDayEntry: (fromDate: string, index: number, toDate: string) =>
    invoke<void>("move_day_entry", { fromDate, index, toDate }),
  /** Copy an entry onto `toDate` as a fresh planned occurrence with its own
   *  id. Returns the new entry's index on that day. */
  repeatDayEntry: (date: string, index: number, toDate: string) =>
    invoke<number>("repeat_day_entry", { date, index, toDate }),
  promoteDayEntry: (date: string, index: number) =>
    invoke<WorkoutSummary>("promote_day_entry", { date, index }),
  parsePlanPreview: (source: string) =>
    invoke<{ status: "ok"; name: string; day_count: number } | { status: "err"; errors: ParseError[] }>(
      "parse_plan_preview",
      { source },
    ),
  listPlans: () => invoke<PlanSummary[]>("list_plans"),
  getPlanSource: (slug: string) => invoke<string>("get_plan_source", { slug }),
  savePlan: (source: string, prevSlug: string | null) =>
    invoke<PlanSummary>("save_plan", { source, prevSlug, today: todayStr() }),
  /** Upload a plan file: updates the days it carries, leaves the rest alone. */
  importPlan: (source: string) =>
    invoke<PlanImport>("import_plan", { source, today: todayStr() }),
  syncPlan: (slug: string) => invoke<number>("sync_plan", { slug, today: todayStr() }),
  deletePlan: (slug: string) => invoke<void>("delete_plan", { slug }),
  /** The whole library, plans and calendar as one markdown document. */
  exportBundle: () => invoke<string>("export_bundle"),
  parseBundlePreview: (source: string) =>
    invoke<BundlePreview>("parse_bundle_preview", { source }),
  importBundle: (source: string) => invoke<ImportReport>("import_bundle", { source }),
  pause: () => invoke<void>("pause_timer"),
  resume: () => invoke<void>("resume_timer"),
  /** Leave the run screen; an unfinished run is kept as a resumable session. */
  suspend: () => invoke<void>("suspend_timer"),
  skip: () => invoke<void>("skip_phase"),
  getSession: () => invoke<SessionInfo | null>("get_session", { today: todayStr() }),
  resumeSession: () => invoke<RunPlan>("resume_session", { today: todayStr() }),
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
