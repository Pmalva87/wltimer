use serde::Serialize;
use std::collections::BTreeMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};
use tauri::{AppHandle, Emitter, Manager, State};
use wltimer_core::days::{self, DayEntry, DayStatus, DayStore, DaySummary};
use wltimer_core::engine::{Cue, Engine, Snapshot};
use wltimer_core::ids;
use wltimer_core::model::{Phase, Workout};
use wltimer_core::parser::{self, ParseError};
use wltimer_core::plan::{self, Plan, PlanStore, PlanSummary};
use wltimer_core::session::{RunOrigin, SavedSession, SessionStore};
use wltimer_core::store::{Store, WorkoutSummary};

pub struct AppState {
    pub engine: Mutex<Engine>,
    pub store: Store,
    pub days: DayStore,
    pub plans: PlanStore,
    pub sessions: SessionStore,
    pub origin: Mutex<RunOrigin>,
}

/// Everything the run screen needs up front; ticks then only carry indices.
#[derive(Serialize)]
pub struct RunPlan {
    pub workout_name: String,
    pub blocks: Vec<RunBlock>,
    pub phases: Vec<Phase>,
    pub total_secs: u32,
}

#[derive(Serialize)]
pub struct RunBlock {
    pub name: String,
    pub color: Option<String>,
    pub intervals: u32,
    pub description_html: String,
}

#[derive(Serialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum Preview {
    Ok {
        name: String,
        block_count: usize,
        total_secs: u32,
        /// How many parts run straight into the next one.
        parts_without_rest: usize,
    },
    Err {
        errors: Vec<ParseError>,
    },
}

/// Result of parsing a full document for the builder screen.
#[derive(Serialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum ParseFull {
    Ok { workout: Workout },
    Err { errors: Vec<ParseError> },
}

#[derive(Serialize)]
pub struct DayEntryInfo {
    pub name: String,
    pub status: DayStatus,
    pub completed_at: Option<String>,
    pub source_slug: Option<String>,
    pub source_plan: Option<String>,
    pub markdown: String,
}

fn local_date() -> String {
    chrono::Local::now().format("%Y-%m-%d").to_string()
}

/// The moment, in the canonical form every stored timestamp uses (UTC, second
/// precision — see `wltimer_core::time`). `core` deliberately cannot read the
/// clock, so this is the app's single answer to what time it is, and the only
/// thing that stamps documents as changed.
fn now() -> String {
    chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true)
}

/// Use the frontend-supplied date when it is well-formed, else fall back to
/// the device clock.
fn date_or_local(date: &str) -> String {
    if days::valid_date(date) {
        date.to_string()
    } else {
        local_date()
    }
}

fn check_date(date: &str) -> Result<(), String> {
    if days::valid_date(date) {
        Ok(())
    } else {
        Err(format!("invalid date '{date}' — expected YYYY-MM-DD"))
    }
}

fn date_errors(date: &str) -> Result<(), Vec<ParseError>> {
    check_date(date).map_err(|message| vec![ParseError { line: 1, message }])
}

// ---- library ----

#[tauri::command]
pub fn list_workouts(state: State<AppState>) -> Vec<WorkoutSummary> {
    state.store.list()
}

#[tauri::command]
pub fn get_workout_source(state: State<AppState>, slug: String) -> Result<String, String> {
    state.store.read_source(&slug)
}

#[tauri::command]
pub fn save_workout(
    state: State<AppState>,
    source: String,
    prev_slug: Option<String>,
) -> Result<WorkoutSummary, Vec<ParseError>> {
    state.store.save(&source, prev_slug.as_deref(), &now())
}

/// Copy a library workout into a second, independent workout.
///
/// Needs its own command because saving a document that already carries an id
/// *updates* that workout — which is exactly what makes re-import work, and
/// exactly why a deliberate copy has to mint a new identity first.
#[tauri::command]
pub fn duplicate_workout(
    state: State<AppState>,
    slug: String,
) -> Result<WorkoutSummary, Vec<ParseError>> {
    let source = state
        .store
        .read_source(&slug)
        .map_err(|message| vec![ParseError { line: 1, message }])?;
    let (source, _) = ids::with_new_id(&source);
    state.store.save(&source, None, &now())
}

#[tauri::command]
pub fn delete_workout(state: State<AppState>, slug: String) -> Result<(), String> {
    state.store.delete(&slug)
}

// ---- parsing / serialization ----

#[tauri::command]
pub fn parse_preview(source: String) -> Preview {
    match parser::parse_workout(&source) {
        Ok(w) => Preview::Ok {
            name: w.name.clone(),
            block_count: w.blocks.len(),
            total_secs: w.total_secs(),
            parts_without_rest: w.parts_without_rest_after().len(),
        },
        Err(errors) => Preview::Err { errors },
    }
}

#[tauri::command]
pub fn parse_full(source: String) -> ParseFull {
    match parser::parse_workout(&source) {
        Ok(workout) => ParseFull::Ok { workout },
        Err(errors) => ParseFull::Err { errors },
    }
}

#[tauri::command]
pub fn serialize_workout(workout: Workout) -> String {
    parser::workout_to_markdown(&workout)
}

/// Everything the read-only view screen shows. Separate from `RunPlan` because
/// asking to *look* at a workout must not start one — `start_workout` sets the
/// run origin, which decides what gets written to the calendar when a run ends.
#[derive(Serialize)]
pub struct WorkoutView {
    pub id: Option<String>,
    pub name: String,
    pub total_secs: u32,
    pub blocks: Vec<ViewBlock>,
}

#[derive(Serialize)]
pub struct ViewBlock {
    pub name: String,
    pub color: Option<String>,
    pub intervals: u32,
    pub work_secs: u32,
    pub rest_secs: Option<u32>,
    pub rest_after_secs: Option<u32>,
    pub block_secs: u32,
    /// This part runs straight into the next one, with nothing in between.
    pub no_rest_after: bool,
    pub description_html: String,
}

#[tauri::command]
pub fn view_workout(source: String) -> Result<WorkoutView, Vec<ParseError>> {
    let w = parser::parse_workout(&source)?;
    let last = w.blocks.len().saturating_sub(1);
    let no_rest: std::collections::HashSet<usize> =
        w.parts_without_rest_after().into_iter().collect();
    Ok(WorkoutView {
        id: w.id.clone(),
        name: w.name.clone(),
        total_secs: w.total_secs(),
        blocks: w
            .blocks
            .iter()
            .enumerate()
            .map(|(i, b)| ViewBlock {
                name: b.name.clone(),
                color: b.color.clone(),
                intervals: b.intervals,
                work_secs: b.work_secs,
                rest_secs: b.rest_secs,
                rest_after_secs: if i < last { b.rest_after_secs } else { None },
                block_secs: b.intervals * b.work_secs
                    + b.rest_secs.unwrap_or(0) * b.intervals.saturating_sub(1),
                no_rest_after: no_rest.contains(&i),
                description_html: parser::render_markdown(&b.description_md),
            })
            .collect(),
    })
}

// ---- calendar ----

#[tauri::command]
pub fn get_month(state: State<AppState>, year: i32, month: u32) -> Vec<DaySummary> {
    state.days.month(year, month)
}

#[tauri::command]
pub fn get_day(state: State<AppState>, date: String) -> Result<Vec<DayEntryInfo>, String> {
    check_date(&date)?;
    Ok(state
        .days
        .load(&date)
        .iter()
        .map(|e| DayEntryInfo {
            name: days::entry_name(e),
            status: e.status,
            completed_at: e.completed_at.clone(),
            source_slug: e.source_slug.clone(),
            source_plan: e.source_plan.clone(),
            markdown: e.markdown.clone(),
        })
        .collect())
}

fn planned_entry(markdown: String, source_slug: Option<String>) -> DayEntry {
    DayEntry {
        markdown,
        status: DayStatus::Planned,
        completed_at: None,
        source_slug,
        source_plan: None,
    }
}

#[tauri::command]
pub fn add_day_entry(
    state: State<AppState>,
    date: String,
    source: String,
) -> Result<usize, Vec<ParseError>> {
    date_errors(&date)?;
    parser::parse_workout(&source)?;
    let (source, _) = ids::ensure_id(&source);
    state
        .days
        .add(&date, planned_entry(source, None), &now())
        .map_err(|message| vec![ParseError { line: 1, message }])
}

#[tauri::command]
pub fn add_day_from_library(
    state: State<AppState>,
    date: String,
    slug: String,
) -> Result<usize, String> {
    check_date(&date)?;
    let source = state.store.read_source(&slug)?;
    // A scheduled copy is a new occurrence, not the template: it gets its own
    // id so the two never collide. Provenance stays in `source_slug`.
    let (source, _) = ids::with_new_id(&source);
    state
        .days
        .add(&date, planned_entry(source, Some(slug)), &now())
}

#[tauri::command]
pub fn update_day_entry(
    state: State<AppState>,
    date: String,
    index: usize,
    source: String,
) -> Result<(), Vec<ParseError>> {
    date_errors(&date)?;
    parser::parse_workout(&source)?;
    state
        .days
        .update(&date, index, &now(), |e| {
            // Editing an entry must not re-mint its identity, or the next plan
            // sync would no longer recognise it. A document arriving without an
            // id inherits the one the entry already had.
            e.markdown = match (ids::extract_id(&source), days::entry_id(e)) {
                (None, Some(existing)) => ids::set_id(&source, &existing),
                _ => ids::ensure_id(&source).0,
            };
        })
        .map_err(|message| vec![ParseError { line: 1, message }])
}

#[tauri::command]
pub fn delete_day_entry(state: State<AppState>, date: String, index: usize) -> Result<(), String> {
    check_date(&date)?;
    state.days.delete(&date, index)
}

#[tauri::command]
pub fn move_day_entry(
    state: State<AppState>,
    from_date: String,
    index: usize,
    to_date: String,
) -> Result<(), String> {
    check_date(&from_date)?;
    check_date(&to_date)?;
    state.days.move_entry(&from_date, index, &to_date, &now())
}

/// Repeat a day entry on another date (used by Re-Run on a finished workout).
/// The copy mints its own id: the entry it came from is a finished occurrence
/// that must keep standing on its own day, and two entries sharing an id would
/// make a later plan sync unable to tell them apart. Provenance carries over.
#[tauri::command]
pub fn repeat_day_entry(
    state: State<AppState>,
    date: String,
    index: usize,
    to_date: String,
) -> Result<usize, String> {
    check_date(&date)?;
    check_date(&to_date)?;
    let entries = state.days.load(&date);
    let entry = entries
        .get(index)
        .ok_or_else(|| format!("no entry {index} on {date}"))?;
    let (source, _) = ids::with_new_id(&entry.markdown);
    state
        .days
        .add(
            &to_date,
            planned_entry(source, entry.source_slug.clone()),
            &now(),
        )
}

/// Save a day entry's workout into the library (explicit opt-in).
#[tauri::command]
pub fn promote_day_entry(
    state: State<AppState>,
    date: String,
    index: usize,
) -> Result<WorkoutSummary, Vec<ParseError>> {
    date_errors(&date)?;
    let entries = state.days.load(&date);
    let entry = entries
        .get(index)
        .ok_or_else(|| vec![ParseError { line: 1, message: format!("no entry {index} on {date}") }])?;
    // The library template is a new object, distinct from the dated entry it
    // was taken from, so it gets its own id rather than sharing the entry's.
    let (source, _) = ids::with_new_id(&entry.markdown);
    state.store.save(&source, None, &now())
}

// ---- training plans ----

/// Load the upcoming calendar, merge the plan into it (see
/// `plan::merge_into_calendar` for the matching rules), write back what
/// changed. Returns the number of days scheduled or updated.
fn sync_upcoming(state: &AppState, slug: &str, plan: &Plan, from: &str) -> usize {
    let mut by_date: BTreeMap<String, Vec<DayEntry>> = state
        .days
        .dates_from(from)
        .into_iter()
        .map(|date| {
            let entries = state.days.load(&date);
            (date, entries)
        })
        .collect();

    let (synced, dirty) = plan::merge_into_calendar(plan, slug, from, &now(), &mut by_date);

    for date in dirty {
        let entries = by_date.get(&date).map(Vec::as_slice).unwrap_or(&[]);
        let _ = state.days.save(&date, entries);
    }
    synced
}

#[derive(Serialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum PlanPreview {
    Ok { name: String, day_count: usize },
    Err { errors: Vec<ParseError> },
}

/// Check whether a document is a training plan (used to route uploads:
/// plan files import as plans regardless of which upload button was used).
#[tauri::command]
pub fn parse_plan_preview(source: String) -> PlanPreview {
    match wltimer_core::plan::parse_plan(&source) {
        Ok(p) => PlanPreview::Ok {
            name: p.name,
            day_count: p.days.len(),
        },
        Err(errors) => PlanPreview::Err { errors },
    }
}

#[tauri::command]
pub fn list_plans(state: State<AppState>) -> Vec<PlanSummary> {
    state.plans.list()
}

#[tauri::command]
pub fn get_plan_source(state: State<AppState>, slug: String) -> Result<String, String> {
    state.plans.read_source(&slug)
}

/// Save (or replace) a plan and immediately sync its upcoming days onto the
/// calendar.
#[tauri::command]
pub fn save_plan(
    state: State<AppState>,
    source: String,
    prev_slug: Option<String>,
    today: String,
) -> Result<PlanSummary, Vec<ParseError>> {
    let (summary, plan) = state.plans.save(&source, prev_slug.as_deref(), &now())?;
    sync_upcoming(&state, &summary.slug, &plan, &date_or_local(&today));
    Ok(summary)
}

/// Re-apply a stored plan's upcoming days to the calendar; returns how many
/// days were scheduled.
#[tauri::command]
pub fn sync_plan(state: State<AppState>, slug: String, today: String) -> Result<usize, String> {
    let source = state.plans.read_source(&slug)?;
    let plan = wltimer_core::plan::parse_plan(&source)
        .map_err(|e| format!("plan no longer parses — line {}: {}", e[0].line, e[0].message))?;
    Ok(sync_upcoming(&state, &slug, &plan, &date_or_local(&today)))
}

#[tauri::command]
pub fn delete_plan(state: State<AppState>, slug: String) -> Result<(), String> {
    state.plans.delete(&slug)
}

// ---- starting runs ----

#[tauri::command]
pub fn start_workout(
    app: AppHandle,
    state: State<AppState>,
    slug: String,
    today: String,
) -> Result<RunPlan, String> {
    let source = state.store.read_source(&slug)?;
    let workout = parser::parse_workout(&source)
        .map_err(|e| format!("workout no longer parses — line {}: {}", e[0].line, e[0].message))?;
    *state.origin.lock().unwrap() = RunOrigin::Library {
        date: date_or_local(&today),
        slug,
    };
    start(app, state, workout)
}

#[tauri::command]
pub fn start_custom(
    app: AppHandle,
    state: State<AppState>,
    workout: Workout,
    today: String,
) -> Result<RunPlan, String> {
    if workout.blocks.is_empty() {
        return Err("add at least one part".into());
    }
    for (i, b) in workout.blocks.iter().enumerate() {
        if b.intervals < 1 {
            return Err(format!("part {}: intervals must be at least 1", i + 1));
        }
        if b.work_secs < 1 {
            return Err(format!("part {}: work time must be at least 1 second", i + 1));
        }
    }
    *state.origin.lock().unwrap() = RunOrigin::Adhoc {
        date: date_or_local(&today),
    };
    start(app, state, workout)
}

#[tauri::command]
pub fn start_day_entry(
    app: AppHandle,
    state: State<AppState>,
    date: String,
    index: usize,
) -> Result<RunPlan, String> {
    check_date(&date)?;
    let entries = state.days.load(&date);
    let entry = entries
        .get(index)
        .ok_or_else(|| format!("no entry {index} on {date}"))?;
    let workout = parser::parse_workout(&entry.markdown)
        .map_err(|e| format!("workout no longer parses — line {}: {}", e[0].line, e[0].message))?;
    *state.origin.lock().unwrap() = RunOrigin::Day { date, index };
    start(app, state, workout)
}

fn run_plan(workout: &Workout) -> RunPlan {
    RunPlan {
        workout_name: workout.name.clone(),
        blocks: workout
            .blocks
            .iter()
            .map(|b| RunBlock {
                name: b.name.clone(),
                color: b.color.clone(),
                intervals: b.intervals,
                description_html: parser::render_markdown(&b.description_md),
            })
            .collect(),
        phases: workout.flatten(),
        total_secs: workout.total_secs(),
    }
}

fn start(app: AppHandle, state: State<AppState>, workout: Workout) -> Result<RunPlan, String> {
    let plan = run_plan(&workout);
    let now = Instant::now();
    let mut engine = state.engine.lock().unwrap();
    let cues = engine.start(workout, now);
    after_cues(&state, &engine, &cues, now);
    emit(&app, &engine.snapshot(now), &cues);
    Ok(plan)
}

// ---- suspended sessions ----

/// What the start screen shows about a run waiting to be picked back up.
#[derive(Serialize)]
pub struct SessionInfo {
    /// The `#/run/<target>` route this session belongs to.
    pub target: String,
    pub workout_name: String,
    pub phase_idx: usize,
    pub total_phases: usize,
    pub remaining_secs: u32,
}

/// The run suspended earlier today, if any. Sessions do not outlive their day.
#[tauri::command]
pub fn get_session(state: State<AppState>, today: String) -> Option<SessionInfo> {
    let saved = state.sessions.load(&date_or_local(&today))?;
    Some(SessionInfo {
        target: saved.origin.target()?,
        workout_name: saved.workout.name.clone(),
        phase_idx: saved.phase_idx,
        total_phases: saved.workout.flatten().len(),
        remaining_secs: saved.remaining_secs(),
    })
}

/// Put the suspended run back on the clock, right where it stopped.
#[tauri::command]
pub fn resume_session(
    app: AppHandle,
    state: State<AppState>,
    today: String,
) -> Result<RunPlan, String> {
    let saved = state
        .sessions
        .load(&date_or_local(&today))
        .ok_or("nothing to resume")?;
    let plan = run_plan(&saved.workout);
    *state.origin.lock().unwrap() = saved.origin;
    let now = Instant::now();
    let mut engine = state.engine.lock().unwrap();
    let cues = engine.restore(
        saved.workout,
        saved.phase_idx,
        Duration::from_millis(saved.elapsed_ms),
        now,
    );
    emit(&app, &engine.snapshot(now), &cues);
    Ok(plan)
}

/// Write the run in progress to disk so it can be picked up again later today.
/// A run without an origin has nowhere to be recorded and is not saved.
fn save_session(state: &AppState, engine: &Engine, now: Instant) {
    let origin = state.origin.lock().unwrap().clone();
    if origin == RunOrigin::None {
        return;
    }
    let (Some(workout), Some((phase_idx, elapsed))) = (engine.workout(), engine.position(now))
    else {
        return;
    };
    let _ = state.sessions.save(&SavedSession {
        date: local_date(),
        origin,
        workout: workout.clone(),
        phase_idx,
        elapsed_ms: elapsed.as_millis() as u64,
    });
}

// ---- run control ----

#[tauri::command]
pub fn pause_timer(app: AppHandle, state: State<AppState>) {
    let now = Instant::now();
    let mut engine = state.engine.lock().unwrap();
    engine.pause(now);
    save_session(&state, &engine, now);
    emit(&app, &engine.snapshot(now), &[]);
}

#[tauri::command]
pub fn resume_timer(app: AppHandle, state: State<AppState>) {
    let now = Instant::now();
    let mut engine = state.engine.lock().unwrap();
    engine.resume(now);
    emit(&app, &engine.snapshot(now), &[]);
}

/// Leave the run screen without giving the workout up: an unfinished run is
/// frozen on disk first, so the same workout can be resumed later today.
#[tauri::command]
pub fn suspend_timer(app: AppHandle, state: State<AppState>) {
    let now = Instant::now();
    let mut engine = state.engine.lock().unwrap();
    engine.pause(now);
    save_session(&state, &engine, now);
    engine.stop();
    *state.origin.lock().unwrap() = RunOrigin::None;
    emit(&app, &engine.snapshot(now), &[]);
}

#[tauri::command]
pub fn skip_phase(app: AppHandle, state: State<AppState>) {
    let now = Instant::now();
    let mut engine = state.engine.lock().unwrap();
    let cues = engine.skip(now);
    after_cues(&state, &engine, &cues, now);
    emit(&app, &engine.snapshot(now), &cues);
}

#[tauri::command]
pub fn get_snapshot(state: State<AppState>) -> Snapshot {
    state.engine.lock().unwrap().snapshot(Instant::now())
}

// ---- events + completion recording ----

fn emit(app: &AppHandle, snapshot: &Snapshot, cues: &[Cue]) {
    let _ = app.emit("timer:tick", snapshot);
    for cue in cues {
        let _ = app.emit("timer:cue", cue);
    }
}

/// Bookkeeping for the cues a tick produced: a finished run goes on the
/// calendar and gives up its session, while crossing into a new phase refreshes
/// the saved one, so a run cut short by the OS resumes at the phase it reached.
fn after_cues(state: &AppState, engine: &Engine, cues: &[Cue], now: Instant) {
    if cues.iter().any(|c| matches!(c, Cue::Finished)) {
        record_finished(state, engine);
        state.sessions.clear();
    } else if cues.iter().any(|c| matches!(c, Cue::PhaseStart { .. })) {
        save_session(state, engine, now);
    }
}

/// Record a finished run on the calendar according to its origin.
fn record_finished(state: &AppState, engine: &Engine) {
    let Some(workout) = engine.workout() else {
        return;
    };
    let markdown = parser::workout_to_markdown(workout);
    let origin = std::mem::replace(&mut *state.origin.lock().unwrap(), RunOrigin::None);
    // One reading of the clock for the whole recording, so the entry's
    // `completed_at` and the version stamp on its document agree.
    let now = now();
    let done = |markdown: String, source_slug: Option<String>| DayEntry {
        // A finished run is its own occurrence on the calendar, distinct from
        // the library template it was started from, so it gets its own id.
        // (A run that came *from* a calendar entry takes the branch above and
        // keeps that entry's id — which is how a sync knows it is done.)
        markdown: ids::with_new_id(&markdown).0,
        status: DayStatus::Done,
        completed_at: Some(now.clone()),
        source_slug,
        source_plan: None,
    };
    match origin {
        RunOrigin::Day { date, index } => {
            let updated = state
                .days
                .update(&date, index, &now, |e| {
                    e.status = DayStatus::Done;
                    e.completed_at = Some(now.clone());
                })
                .is_ok();
            if !updated {
                // The scheduled entry vanished mid-run; still record the work.
                let _ = state.days.add(&date, done(markdown, None), &now);
            }
        }
        RunOrigin::Library { date, slug } => {
            let _ = state.days.add(&date, done(markdown, Some(slug)), &now);
        }
        RunOrigin::Adhoc { date } => {
            let _ = state.days.add(&date, done(markdown, None), &now);
        }
        RunOrigin::None => {}
    }
}

/// Background loop driving the engine ~5×/s while a workout is running.
pub fn spawn_ticker(app: AppHandle) {
    tauri::async_runtime::spawn(async move {
        let mut interval = tokio::time::interval(std::time::Duration::from_millis(200));
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        loop {
            interval.tick().await;
            let state = app.state::<AppState>();
            let now = Instant::now();
            let (snapshot, cues) = {
                let mut engine = state.engine.lock().unwrap();
                match engine.state() {
                    wltimer_core::engine::State::Running => {
                        let cues = engine.advance(now);
                        after_cues(&state, &engine, &cues, now);
                        (Some(engine.snapshot(now)), cues)
                    }
                    wltimer_core::engine::State::Paused => (Some(engine.snapshot(now)), Vec::new()),
                    _ => (None, Vec::new()),
                }
            };
            if let Some(snapshot) = snapshot {
                emit(&app, &snapshot, &cues);
            }
        }
    });
}
