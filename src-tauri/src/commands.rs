use serde::Serialize;
use std::collections::BTreeMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};
use tauri::{AppHandle, Emitter, Manager, State};
use wltimer_core::bundle::{self, ImportReport};
use wltimer_core::days::{self, DayEntry, DayStatus, DayStore, DaySummary};
use wltimer_core::engine::{Cue, Engine, Snapshot};
use wltimer_core::ids;
use wltimer_core::model::{Phase, Workout};
use wltimer_core::parser::{self, ParseError};
use wltimer_core::plan::{self, PatchCounts, Plan, PlanStore, PlanSummary};
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
    state
        .days
        .move_entry(&from_date, index, &to_date, &now())
        .map(|_| ())
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
/// Push a plan's upcoming days onto the calendar.
///
/// `plan_updated` is the stamp the stored plan file carries, and it decides
/// every conflict: an entry edited later than it is left alone, and entries
/// written by the sync take that stamp so they read as "this version of the
/// plan" rather than as edits made after it.
fn sync_upcoming(
    state: &AppState,
    slug: &str,
    plan: &Plan,
    plan_updated: &str,
    from: &str,
) -> plan::SyncReport {
    let mut by_date: BTreeMap<String, Vec<DayEntry>> = state
        .days
        .dates_from(from)
        .into_iter()
        .map(|date| {
            let entries = state.days.load(&date);
            (date, entries)
        })
        .collect();

    let (report, dirty) = plan::merge_into_calendar(plan, slug, from, plan_updated, &mut by_date);

    for date in dirty {
        let entries = by_date.get(&date).map(Vec::as_slice).unwrap_or(&[]);
        let _ = state.days.save(&date, entries);
    }
    report
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
    let now = now();
    let (summary, plan) = state.plans.save(&source, prev_slug.as_deref(), &now)?;
    // The plan was just written with this stamp, so it is the newest version
    // of every day it holds and wins any conflict on the calendar.
    sync_upcoming(&state, &summary.slug, &plan, &now, &date_or_local(&today));
    Ok(summary)
}

/// What an uploaded plan file did to the plan it belongs to.
#[derive(Serialize)]
pub struct PlanImport {
    pub summary: PlanSummary,
    /// Days replaced, added and removed. A section carrying no id has nothing
    /// to match on and so can only be an addition.
    #[serde(flatten)]
    pub counts: PatchCounts,
    /// What the follow-up sync did to the calendar.
    pub sync: plan::SyncReport,
}

/// Apply an uploaded plan file to the plan it belongs to, updating only the
/// days it carries, and sync what changed.
///
/// This is what every plan upload goes through: a file holding the two days
/// you fixed fixes those two, rather than becoming the whole plan and
/// unscheduling everything it left out. Replacing a plan outright is the
/// explicit `save_plan` path behind "Replace".
#[tauri::command]
pub fn import_plan(
    state: State<AppState>,
    source: String,
    today: String,
) -> Result<PlanImport, Vec<ParseError>> {
    let now = now();
    let (summary, plan, counts) = state.plans.patch(&source, &now)?;
    let sync = sync_upcoming(&state, &summary.slug, &plan, &now, &date_or_local(&today));
    Ok(PlanImport { summary, counts, sync })
}

/// A plan's day, together with what became of it on the calendar.
#[derive(Serialize)]
pub struct PlanDayView {
    pub id: Option<String>,
    pub name: String,
    /// The date the plan gives it.
    pub date: String,
    pub total_secs: u32,
    /// Where its calendar entry actually sits, when there is one — not always
    /// `date`: finishing a workout moves it to the day it was done.
    pub entry_date: Option<String>,
    pub entry_index: Option<usize>,
    pub status: Option<DayStatus>,
    /// The entry was changed after this version of the plan, so the next sync
    /// will leave it alone.
    pub edited: bool,
}

#[derive(Serialize)]
pub struct PlanView {
    pub slug: String,
    pub name: String,
    pub updated: Option<String>,
    pub days: Vec<PlanDayView>,
    /// Set when the stored file no longer parses; `days` is then empty.
    pub error: Option<String>,
}

/// A plan and the state of every day it scheduled.
///
/// Each day is resolved against the calendar by id, searched from the plan's
/// first date onward rather than only on the date the plan gives: an entry
/// moves when it is finished on another day, and a plan that reported those as
/// missing would be lying about the thing it exists to show.
#[tauri::command]
pub fn view_plan(state: State<AppState>, slug: String) -> Result<PlanView, String> {
    let source = state.plans.read_source(&slug)?;
    let updated = ids::extract_updated(&source);
    let plan = match wltimer_core::plan::parse_plan(&source) {
        Ok(plan) => plan,
        Err(errors) => {
            return Ok(PlanView {
                slug,
                name: source.lines().next().unwrap_or("").trim_start_matches("# ").into(),
                updated,
                days: Vec::new(),
                error: Some(format!("line {}: {}", errors[0].line, errors[0].message)),
            })
        }
    };

    let first = plan.days.first().map(|d| d.date.clone()).unwrap_or_default();
    let mut located: BTreeMap<String, (String, usize, DayStatus, Option<String>)> = BTreeMap::new();
    for date in state.days.dates_from(&first) {
        for (index, entry) in state.days.load(&date).iter().enumerate() {
            if let Some(id) = days::entry_id(entry) {
                located.insert(
                    id,
                    (date.clone(), index, entry.status, days::entry_updated(entry)),
                );
            }
        }
    }

    let days = plan
        .days
        .iter()
        .map(|day| {
            let found = day.id.as_deref().and_then(|id| located.get(id));
            PlanDayView {
                id: day.id.clone(),
                name: day.name.clone(),
                date: day.date.clone(),
                total_secs: parser::parse_workout(&day.workout_md)
                    .map(|w| w.total_secs())
                    .unwrap_or(0),
                entry_date: found.map(|f| f.0.clone()),
                entry_index: found.map(|f| f.1),
                status: found.map(|f| f.2),
                edited: found.is_some_and(|f| match (&f.3, &updated) {
                    (Some(entry), Some(plan)) => entry > plan,
                    _ => false,
                }),
            }
        })
        .collect();

    Ok(PlanView { slug, name: plan.name, updated, days, error: None })
}

/// Rename a plan. Its slug does not move, so the calendar entries that name it
/// stay attached, and no sync is needed — no day changed.
#[tauri::command]
pub fn rename_plan(
    state: State<AppState>,
    slug: String,
    name: String,
) -> Result<PlanSummary, String> {
    state.plans.rename(&slug, &name)
}

/// Remove one day from a plan, and unschedule it if it is still only planned.
///
/// The on-phone counterpart to uploading a `- deleted: true` section: same
/// effect, without a round trip through a file.
#[tauri::command]
pub fn delete_plan_day(
    state: State<AppState>,
    slug: String,
    day_id: String,
    today: String,
) -> Result<plan::SyncReport, String> {
    let source = state.plans.read_source(&slug)?;
    let merged = plan::remove_day(&source, &day_id).ok_or(
        "cannot remove that day — either it is not in this plan, or it is the \
         only one left and what you want is to delete the plan",
    )?;
    let now = now();
    let (summary, plan) = state
        .plans
        .save(&merged, Some(&slug), &now)
        .map_err(|e| format!("line {}: {}", e[0].line, e[0].message))?;
    Ok(sync_upcoming(&state, &summary.slug, &plan, &now, &date_or_local(&today)))
}

/// One calendar entry offered for picking when building a plan from history.
#[derive(Serialize)]
pub struct DayPick {
    pub date: String,
    pub index: usize,
    pub name: String,
    pub status: DayStatus,
    pub completed_at: Option<String>,
}

/// Every calendar entry between two dates, for the "from calendar" picker.
#[tauri::command]
pub fn list_day_entries(
    state: State<AppState>,
    from: String,
    to: String,
) -> Result<Vec<DayPick>, String> {
    check_date(&from)?;
    check_date(&to)?;
    let mut out = Vec::new();
    for date in state.days.dates_from(&from) {
        if date > to {
            break;
        }
        for (index, entry) in state.days.load(&date).iter().enumerate() {
            out.push(DayPick {
                date: date.clone(),
                index,
                name: days::entry_name(entry),
                status: entry.status,
                completed_at: entry.completed_at.clone(),
            });
        }
    }
    Ok(out)
}

/// Which calendar entry to take, as the picker names it.
#[derive(serde::Deserialize)]
pub struct DayRef {
    pub date: String,
    pub index: usize,
}

/// Build a plan out of calendar entries you pick — the reverse of scheduling,
/// and what makes a plan something you can fix rather than only import.
///
/// The generated days keep the entries' ids, so the plan owns them: fixing a
/// day later updates the entry it was built from. It deliberately does **not**
/// sync afterwards, for the reason `bundle::restore` does not either — those
/// entries already are the record, and re-scheduling on top of them could only
/// disturb what it was just built from.
#[tauri::command]
pub fn create_plan_from_days(
    state: State<AppState>,
    name: String,
    picks: Vec<DayRef>,
) -> Result<PlanSummary, Vec<ParseError>> {
    let fail = |message: String| vec![ParseError { line: 1, message }];
    if picks.is_empty() {
        return Err(fail("pick at least one workout".into()));
    }
    let name = name.trim();
    if name.is_empty() {
        return Err(fail("give the plan a name".into()));
    }
    let mut days = Vec::with_capacity(picks.len());
    for pick in &picks {
        check_date(&pick.date).map_err(fail)?;
        let entries = state.days.load(&pick.date);
        let entry = entries
            .get(pick.index)
            .ok_or_else(|| fail(format!("no entry {} on {}", pick.index, pick.date)))?;
        days.push((pick.date.clone(), entry.markdown.clone()));
    }
    let (summary, _) = state
        .plans
        .save(&plan::plan_to_markdown(name, &days), None, &now())?;
    Ok(summary)
}

/// Re-apply a stored plan's upcoming days to the calendar.
///
/// Unlike a save, this carries no new version of anything: the comparison uses
/// the stamp the stored plan already has, so pressing Sync cannot overwrite a
/// day you edited on the calendar afterwards.
#[tauri::command]
pub fn sync_plan(
    state: State<AppState>,
    slug: String,
    today: String,
) -> Result<plan::SyncReport, String> {
    let source = state.plans.read_source(&slug)?;
    let plan = wltimer_core::plan::parse_plan(&source)
        .map_err(|e| format!("plan no longer parses — line {}: {}", e[0].line, e[0].message))?;
    let updated = ids::extract_updated(&source).unwrap_or_default();
    Ok(sync_upcoming(&state, &slug, &plan, &updated, &date_or_local(&today)))
}

#[tauri::command]
pub fn delete_plan(state: State<AppState>, slug: String) -> Result<(), String> {
    state.plans.delete(&slug)
}

// ---- backup ----

#[derive(Serialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum BundlePreview {
    Ok { workouts: usize, plans: usize, days: usize },
    /// A perfectly good file that simply is not a backup — the upload falls
    /// through to the plan and workout importers.
    NotBundle,
    Err { errors: Vec<ParseError> },
}

/// Everything in every store as one markdown document, for saving off the
/// phone. The only export that captures the calendar.
#[tauri::command]
pub fn export_bundle(state: State<AppState>) -> String {
    bundle::export(&state.store, &state.plans, &state.days, &now())
}

/// Check whether an upload is a backup bundle, and that all of it parses,
/// before any of it is written.
#[tauri::command]
pub fn parse_bundle_preview(source: String) -> BundlePreview {
    if !bundle::is_bundle(&source) {
        return BundlePreview::NotBundle;
    }
    match bundle::parse(&source) {
        Ok(sections) => {
            let count = |f: fn(&bundle::Section) -> bool| sections.iter().filter(|s| f(s)).count();
            BundlePreview::Ok {
                workouts: count(|s| matches!(s, bundle::Section::Workout(_))),
                plans: count(|s| matches!(s, bundle::Section::Plan { .. })),
                days: count(|s| matches!(s, bundle::Section::Day { .. })),
            }
        }
        Err(errors) => BundlePreview::Err { errors },
    }
}

#[tauri::command]
pub fn import_bundle(
    state: State<AppState>,
    source: String,
) -> Result<ImportReport, Vec<ParseError>> {
    let sections = bundle::parse(&source)?;
    Ok(bundle::restore(
        &state.store,
        &state.plans,
        &state.days,
        &sections,
        &now(),
    ))
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
            // The calendar records what happened, so a workout scheduled for
            // another day moves to the day it was actually done rather than
            // ticking off a date nobody trained on. It keeps its id through
            // the move, which is how a plan sync still recognises it as the
            // day it scheduled — and so leaves the finished work alone.
            let today = local_date();
            let recorded = state
                .days
                .move_entry(&date, index, &today, &now)
                .and_then(|index| {
                    state.days.update(&today, index, &now, |e| {
                        e.status = DayStatus::Done;
                        e.completed_at = Some(now.clone());
                    })
                });
            if recorded.is_err() {
                // The scheduled entry vanished mid-run; still record the work.
                let _ = state.days.add(&today, done(markdown, None), &now);
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
