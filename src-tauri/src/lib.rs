mod commands;
mod store;

use commands::AppState;
use std::sync::Mutex;
use tauri::Manager;
use wltimer_core::engine::Engine;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .setup(|app| {
            let dir = app.path().app_data_dir()?.join("workouts");
            let store = store::Store::new(dir)?;
            app.manage(AppState {
                engine: Mutex::new(Engine::new()),
                store,
            });
            commands::spawn_ticker(app.handle().clone());
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::list_workouts,
            commands::get_workout_source,
            commands::save_workout,
            commands::delete_workout,
            commands::parse_preview,
            commands::start_workout,
            commands::pause_timer,
            commands::resume_timer,
            commands::stop_timer,
            commands::skip_phase,
            commands::get_snapshot,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
