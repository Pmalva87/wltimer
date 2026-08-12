mod commands;

use commands::AppState;
use std::sync::Mutex;
use tauri::Manager;
use wltimer_core::days::DayStore;
use wltimer_core::engine::Engine;
use wltimer_core::plan::PlanStore;
use wltimer_core::session::{RunOrigin, SessionStore};
use wltimer_core::store::Store;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .setup(|app| {
            let data = app.path().app_data_dir()?;
            app.manage(AppState {
                engine: Mutex::new(Engine::new()),
                store: Store::new(data.join("workouts"))?,
                days: DayStore::new(data.join("days"))?,
                plans: PlanStore::new(data.join("plans"))?,
                sessions: SessionStore::new(data)?,
                origin: Mutex::new(RunOrigin::None),
            });
            commands::spawn_ticker(app.handle().clone());
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::list_workouts,
            commands::get_workout_source,
            commands::save_workout,
            commands::duplicate_workout,
            commands::delete_workout,
            commands::parse_preview,
            commands::parse_full,
            commands::serialize_workout,
            commands::view_workout,
            commands::get_month,
            commands::get_day,
            commands::add_day_entry,
            commands::add_day_from_library,
            commands::update_day_entry,
            commands::delete_day_entry,
            commands::move_day_entry,
            commands::repeat_day_entry,
            commands::promote_day_entry,
            commands::parse_plan_preview,
            commands::list_plans,
            commands::get_plan_source,
            commands::save_plan,
            commands::import_plan,
            commands::view_plan,
            commands::rename_plan,
            commands::delete_plan_day,
            commands::list_day_entries,
            commands::create_plan_from_days,
            commands::sync_plan,
            commands::delete_plan,
            commands::export_bundle,
            commands::parse_bundle_preview,
            commands::import_bundle,
            commands::start_workout,
            commands::start_custom,
            commands::start_day_entry,
            commands::pause_timer,
            commands::resume_timer,
            commands::suspend_timer,
            commands::skip_phase,
            commands::get_session,
            commands::resume_session,
            commands::get_snapshot,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
