use serde::Serialize;
use std::collections::BTreeMap;
use std::sync::Mutex;
use std::time::Instant;
use tauri::{AppHandle, Emitter, Manager, State};
use wltimer_core::days::{self, DayEntry, DayStatus, DayStore, DaySummary};
use wltimer_core::engine::{Cue, Engine, Snapshot};
use wltimer_core::ids;
use wltimer_core::model::{Phase, Workout};
use wltimer_core::parser::{self, ParseError};
use wltimer_core::plan::{self, Plan, PlanStore, PlanSummary};
use wltimer_core::store::{Store, WorkoutSummary};

/// Where the currently running workout came from — decides how a finished run
/// is recorded on the calendar.
pub enum RunOrigin {
    None,
    /// A scheduled calendar entry: flip it to done.
    Day { date: String, index: usize },
    /// A library template: append a done entry to the start date.
    Library { date: String, slug: String },
    /// An unsaved builder run: append a done entry to the start date.
    Adhoc { date: String },
}

pub struct AppState {
    pub engine: Mutex<Engine>,
    pub store: Store,
    pub days: DayStore,
    pub plans: PlanStore,
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
    state.store.save(&source, prev_slug.as_deref())
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
        .add(&date, planned_entry(source, None))
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
    state.days.add(&date, planned_entry(source, Some(slug)))
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
        .update(&date, index, |e| {
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
    state.days.move_entry(&from_date, index, &to_date)
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
    state.store.save(&source, None)
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

    let (synced, dirty) = plan::merge_into_calendar(plan, slug, from, &mut by_date);

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
    let (summary, plan) = state.plans.save(&source, prev_slug.as_deref())?;
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

fn start(app: AppHandle, state: State<AppState>, workout: Workout) -> Result<RunPlan, String> {
    let plan = RunPlan {
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
    };
    let now = Instant::now();
    let mut engine = state.engine.lock().unwrap();
    let cues = engine.start(workout, now);
    emit(&app, &engine.snapshot(now), &cues);
    Ok(plan)
}

// ---- run control ----

#[tauri::command]
pub fn pause_timer(app: AppHandle, state: State<AppState>) {
    let now = Instant::now();
    let mut engine = state.engine.lock().unwrap();
    engine.pause(now);
    emit(&app, &engine.snapshot(now), &[]);
}

#[tauri::command]
pub fn resume_timer(app: AppHandle, state: State<AppState>) {
    let now = Instant::now();
    let mut engine = state.engine.lock().unwrap();
    engine.resume(now);
    emit(&app, &engine.snapshot(now), &[]);
}

#[tauri::command]
pub fn stop_timer(app: AppHandle, state: State<AppState>) {
    let mut engine = state.engine.lock().unwrap();
    engine.stop();
    *state.origin.lock().unwrap() = RunOrigin::None;
    emit(&app, &engine.snapshot(Instant::now()), &[]);
}

#[tauri::command]
pub fn skip_phase(app: AppHandle, state: State<AppState>) {
    let now = Instant::now();
    let mut engine = state.engine.lock().unwrap();
    let cues = engine.skip(now);
    maybe_record(&state, &engine, &cues);
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

/// When a run just finished, record it on the calendar according to its origin.
fn maybe_record(state: &AppState, engine: &Engine, cues: &[Cue]) {
    if !cues.iter().any(|c| matches!(c, Cue::Finished)) {
        return;
    }
    let Some(workout) = engine.workout() else {
        return;
    };
    let markdown = parser::workout_to_markdown(workout);
    let origin = std::mem::replace(&mut *state.origin.lock().unwrap(), RunOrigin::None);
    let now = chrono::Local::now().to_rfc3339();
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
                .update(&date, index, |e| {
                    e.status = DayStatus::Done;
                    e.completed_at = Some(now.clone());
                })
                .is_ok();
            if !updated {
                // The scheduled entry vanished mid-run; still record the work.
                let _ = state.days.add(&date, done(markdown, None));
            }
        }
        RunOrigin::Library { date, slug } => {
            let _ = state.days.add(&date, done(markdown, Some(slug)));
        }
        RunOrigin::Adhoc { date } => {
            let _ = state.days.add(&date, done(markdown, None));
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
                        maybe_record(&state, &engine, &cues);
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
