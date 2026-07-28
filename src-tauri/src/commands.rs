use crate::store::{Store, WorkoutSummary};
use serde::Serialize;
use std::sync::Mutex;
use std::time::Instant;
use tauri::{AppHandle, Emitter, Manager, State};
use wltimer_core::engine::{Engine, Snapshot};
use wltimer_core::model::{Phase, Workout};
use wltimer_core::parser::{self, ParseError};

pub struct AppState {
    pub engine: Mutex<Engine>,
    pub store: Store,
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

/// Result of parsing a full document for the builder screen.
#[derive(Serialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum ParseFull {
    Ok { workout: Workout },
    Err { errors: Vec<ParseError> },
}

/// Parse markdown into the full workout structure (populates the builder UI).
#[tauri::command]
pub fn parse_full(source: String) -> ParseFull {
    match parser::parse_workout(&source) {
        Ok(workout) => ParseFull::Ok { workout },
        Err(errors) => ParseFull::Err { errors },
    }
}

/// Serialize a builder-built workout to its canonical markdown form.
#[tauri::command]
pub fn serialize_workout(workout: Workout) -> String {
    parser::workout_to_markdown(&workout)
}

/// Start a workout built directly in the UI, without saving it first.
#[tauri::command]
pub fn start_custom(
    app: AppHandle,
    state: State<AppState>,
    workout: Workout,
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
    start(app, state, workout)
}

#[tauri::command]
pub fn start_workout(app: AppHandle, state: State<AppState>, slug: String) -> Result<RunPlan, String> {
    let source = state.store.read_source(&slug)?;
    let workout = parser::parse_workout(&source)
        .map_err(|e| format!("workout no longer parses — line {}: {}", e[0].line, e[0].message))?;
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
    emit(&app, &engine.snapshot(Instant::now()), &[]);
}

#[tauri::command]
pub fn skip_phase(app: AppHandle, state: State<AppState>) {
    let now = Instant::now();
    let mut engine = state.engine.lock().unwrap();
    let cues = engine.skip(now);
    emit(&app, &engine.snapshot(now), &cues);
}

#[tauri::command]
pub fn get_snapshot(state: State<AppState>) -> Snapshot {
    state.engine.lock().unwrap().snapshot(Instant::now())
}

fn emit(app: &AppHandle, snapshot: &Snapshot, cues: &[wltimer_core::engine::Cue]) {
    let _ = app.emit("timer:tick", snapshot);
    for cue in cues {
        let _ = app.emit("timer:cue", cue);
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
