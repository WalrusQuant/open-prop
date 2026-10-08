mod commands;
mod db;
mod engine;
mod error;
mod models;
mod nba;
mod season;
mod stats;

use std::path::Path;

use tauri::Manager;

pub use crate::models::{SyncReport, TrainReport};

pub fn train_cached(db_path: &Path, season: &str, season_type: &str) -> Result<TrainReport, String> {
    commands::train_cached(db_path, season, season_type)
}

/// Pulls one season type into the database at `db_path`, the same request and merge the app uses.
pub fn sync_cached(db_path: &Path, season: &str, season_type: &str) -> Result<SyncReport, String> {
    commands::sync_cached(db_path, season, season_type)
}

/// Scores the first `last_game` team games of `season` with each (carry, opponent carry) arm,
/// seeded from `seed_season`'s regular season. Returns markdown tables.
pub fn backtest_prior(
    db_path: &Path,
    season: &str,
    seed_season: &str,
    stats: &[String],
    arms: &[(f64, f64)],
    last_game: usize,
) -> Result<String, String> {
    commands::backtest_prior(db_path, season, seed_season, stats, arms, last_game)
}

pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .setup(|app| {
            let directory = app.path().app_data_dir()?;
            let connection = db::open(&directory.join("open-prop.db"))?;
            app.manage(commands::build_state(connection, &directory)?);
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::bootstrap,
            commands::sync_season,
            commands::players,
            commands::trend,
            commands::leaderboard,
            commands::train_models,
            commands::predict,
            commands::model_scores
        ])
        .run(tauri::generate_context!())
        .expect("Open Prop failed to start");
}
