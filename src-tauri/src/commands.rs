use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};

use chrono::{Local, Utc};
use rusqlite::Connection;
use tauri::State;

use crate::db;
use crate::engine::{self, Fitted, ModelSpec};
use crate::error::AppError;
use crate::models::{
    BoardQuery, BoardRow, Bootstrap, CatalogItem, GameLog, PlayerOption, PredictQuery, Prediction,
    SeasonStatus, Stat, SyncReport, TrainQuery, TrainReport, TrainStatReport, TrendQuery,
    TrendReport, Window,
};
use crate::nba::NbaClient;
use crate::season::{self, SEASON_TYPES};
use crate::stats;

pub struct AppState {
    pub db: Mutex<Connection>,
    pub nba: NbaClient,
    pub syncing: AtomicBool,
    pub models_dir: PathBuf,
    pub spec_dirs: Vec<PathBuf>,
    fitted: Mutex<HashMap<String, Arc<Fitted>>>,
    /// Bumped when a train replaces the files, so an in-flight predict cannot put the old model back.
    model_generation: AtomicU64,
}

struct SyncGuard<'a>(&'a AtomicBool);

impl Drop for SyncGuard<'_> {
    fn drop(&mut self) {
        self.0.store(false, Ordering::SeqCst);
    }
}

fn enter_sync(flag: &AtomicBool) -> Result<SyncGuard<'_>, String> {
    if flag
        .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
        .is_err()
    {
        Err("A sync is already running.".to_string())
    } else {
        Ok(SyncGuard(flag))
    }
}

fn lock_db(db: &Mutex<Connection>) -> Result<MutexGuard<'_, Connection>, String> {
    db.lock()
        .map_err(|_| "the local database lock was poisoned".to_string())
}

fn show(error: AppError) -> String {
    error.to_string()
}

#[tauri::command]
pub fn bootstrap(state: State<'_, AppState>) -> Result<Bootstrap, String> {
    let today = Local::now().date_naive();
    let suggested = season::suggested_season(today);
    let stored = {
        let connection = lock_db(&state.db)?;
        db::statuses(&connection).map_err(show)?
    };
    let mut seasons = Vec::new();
    for start in season::season_starts(today) {
        let label = season::format_season(start);
        for season_type in SEASON_TYPES {
            let known = stored.iter().find(|status| {
                status.season == label && status.season_type == season_type
            });
            seasons.push(SeasonStatus {
                season: label.clone(),
                season_type: season_type.to_string(),
                games: known.map(|status| status.games).unwrap_or(0),
                players: known.map(|status| status.players).unwrap_or(0),
                first_game: known.and_then(|status| status.first_game.clone()),
                last_game: known.and_then(|status| status.last_game.clone()),
                synced_at: known.and_then(|status| status.synced_at.clone()),
            });
        }
    }
    Ok(Bootstrap {
        suggested_season: suggested,
        suggested_season_type: "Regular Season".to_string(),
        seasons,
        stats: Stat::all()
            .iter()
            .map(|stat| CatalogItem {
                id: stat.id().to_string(),
                label: stat.label().to_string(),
            })
            .collect(),
        windows: Window::all()
            .iter()
            .map(|window| CatalogItem {
                id: window.id().to_string(),
                label: window.label().to_string(),
            })
            .collect(),
    })
}

#[tauri::command]
pub async fn sync_season(
    state: State<'_, AppState>,
    season: String,
    season_type: String,
) -> Result<SyncReport, String> {
    let season = season::validate_season(&season).map_err(show)?;
    let season_type = season::validate_season_type(&season_type).map_err(show)?;
    // Field borrows stay split so the sync flag and the database lock do not
    // overlap, and the HTTP await never holds the SQLite mutex.
    let state = state.inner();
    let _guard = enter_sync(&state.syncing)?;
    let games = state
        .nba
        .player_game_logs(&season, &season_type)
        .await
        .map_err(show)?;
    let connection = lock_db(&state.db)?;
    db::merge_logs(&connection, &season, &season_type, &games).map_err(show)
}

#[tauri::command]
pub fn players(
    state: State<'_, AppState>,
    season: String,
    season_type: String,
) -> Result<Vec<PlayerOption>, String> {
    let season = season::validate_season(&season).map_err(show)?;
    let season_type = season::validate_season_type(&season_type).map_err(show)?;
    let connection = lock_db(&state.db)?;
    db::players(&connection, &season, &season_type).map_err(show)
}

#[tauri::command]
pub fn trend(state: State<'_, AppState>, query: TrendQuery) -> Result<TrendReport, String> {
    let season = season::validate_season(&query.season).map_err(show)?;
    let season_type = season::validate_season_type(&query.season_type).map_err(show)?;
    let stat = Stat::parse(&query.stat)
        .ok_or_else(|| format!("'{}' is not a stat this desk tracks.", query.stat))?;
    let window = Window::parse(&query.window)
        .ok_or_else(|| format!("'{}' is not a window this desk tracks.", query.window))?;
    if !query.line.is_finite() || query.line < 0.0 {
        return Err("The line has to be a number that is zero or greater.".to_string());
    }
    let connection = lock_db(&state.db)?;
    let games = db::player_games(&connection, &season, &season_type, query.player_id).map_err(show)?;
    if games.is_empty() {
        return Err(
            "No cached games for that player. Sync this season, then try the name again.".to_string(),
        );
    }
    Ok(stats::trend_report(
        &games,
        stat,
        window,
        query.line,
        &season,
        &season_type,
    ))
}

#[tauri::command]
pub fn leaderboard(state: State<'_, AppState>, query: BoardQuery) -> Result<Vec<BoardRow>, String> {
    let season = season::validate_season(&query.season).map_err(show)?;
    let season_type = season::validate_season_type(&query.season_type).map_err(show)?;
    let stat = Stat::parse(&query.stat)
        .ok_or_else(|| format!("'{}' is not a stat this desk tracks.", query.stat))?;
    if !query.line.is_finite() || query.line < 0.0 {
        return Err("The line has to be a number that is zero or greater.".to_string());
    }
    let connection = lock_db(&state.db)?;
    let games = db::season_games(&connection, &season, &season_type).map_err(show)?;
    Ok(stats::leaderboard(&games, stat, query.min_games, query.line))
}

#[tauri::command]
pub fn model_scores(
    state: State<'_, AppState>,
    query: TrainQuery,
) -> Result<Vec<TrainStatReport>, String> {
    let season = season::validate_season(&query.season).map_err(show)?;
    let season_type = season::validate_season_type(&query.season_type).map_err(show)?;
    let state = state.inner();
    Ok(Stat::all()
        .iter()
        .map(|stat| score_stat(&state.models_dir, &state.spec_dirs, *stat, &season, &season_type))
        .collect())
}

#[tauri::command]
pub async fn train_models(
    state: State<'_, AppState>,
    query: TrainQuery,
) -> Result<TrainReport, String> {
    let season = season::validate_season(&query.season).map_err(show)?;
    let season_type = season::validate_season_type(&query.season_type).map_err(show)?;
    let state = state.inner();
    let games = {
        let connection = lock_db(&state.db)?;
        db::season_games(&connection, &season, &season_type).map_err(show)?
    };
    if games.is_empty() {
        return Err("No cached games for that season. Sync it, then train.".to_string());
    }
    let spec_dirs = state.spec_dirs.clone();
    let models_dir = state.models_dir.clone();
    let season_for_train = season.clone();
    let type_for_train = season_type.clone();
    let stats = tokio::task::spawn_blocking(move || {
        train_season(&games, &season_for_train, &type_for_train, &spec_dirs, &models_dir)
    })
    .await
    .map_err(|error| format!("training stopped: {error}"))?;
    {
        let mut cache = state
            .fitted
            .lock()
            .map_err(|_| "the model cache lock was poisoned".to_string())?;
        cache.clear();
    }
    state.model_generation.fetch_add(1, Ordering::SeqCst);
    Ok(TrainReport {
        season,
        season_type,
        stats,
    })
}

#[tauri::command]
pub async fn predict(state: State<'_, AppState>, query: PredictQuery) -> Result<Prediction, String> {
    let season = season::validate_season(&query.season).map_err(show)?;
    let season_type = season::validate_season_type(&query.season_type).map_err(show)?;
    let stat = Stat::parse(&query.stat)
        .ok_or_else(|| format!("'{}' is not a stat this desk tracks.", query.stat))?;
    if !query.line.is_finite() || query.line < 0.0 {
        return Err("The line has to be a number that is zero or greater.".to_string());
    }
    let state = state.inner();
    let games = {
        let connection = lock_db(&state.db)?;
        db::season_games(&connection, &season, &season_type).map_err(show)?
    };
    if games.is_empty() {
        return Err("No cached games for that season. Sync it, then train.".to_string());
    }
    let key = format!("{season}|{season_type}|{}", stat.id());
    let generation = state.model_generation.load(Ordering::SeqCst);
    let cached = {
        let cache = state
            .fitted
            .lock()
            .map_err(|_| "the model cache lock was poisoned".to_string())?;
        cache.get(&key).cloned()
    };
    let models_dir = state.models_dir.clone();
    let line = query.line;
    let window = query.window;
    let spot = engine::Spot {
        player_id: query.player_id,
        opponent: Some(query.opponent),
        home: query.home,
        rest_days: query.rest_days,
        minutes: query.minutes,
    };
    let (fitted, numbers) = tokio::task::spawn_blocking(move || {
        let fitted = match cached {
            Some(fitted) => fitted,
            None => Arc::new(engine::load_model(&models_dir, stat.id()).map_err(show)?),
        };
        if fitted.season != season || fitted.season_type != season_type {
            return Err(format!(
                "The saved {} model is for {} {}. Train this season to replace it.",
                stat.label(),
                fitted.season,
                fitted.season_type
            ));
        }
        let numbers = engine::predict_spot(&fitted, &games, &spot, &window, line).map_err(show)?;
        Ok((fitted, numbers))
    })
    .await
    .map_err(|error| format!("prediction stopped: {error}"))??;
    {
        let mut cache = state
            .fitted
            .lock()
            .map_err(|_| "the model cache lock was poisoned".to_string())?;
        if state.model_generation.load(Ordering::SeqCst) == generation {
            cache.entry(key).or_insert(fitted);
        }
    }
    Ok(Prediction {
        stat: stat.id().to_string(),
        stat_label: stat.label().to_string(),
        mean: numbers.mean,
        low: numbers.low,
        high: numbers.high,
        clear_probability: numbers.clear_probability,
        sigma: numbers.sigma,
        pmf: numbers.pmf,
        shift_probability: numbers.shift_probability,
        minutes: numbers.minutes,
        holdout_mae: numbers.holdout_mae,
        baseline_mae: numbers.baseline_mae,
        holdout_coverage: numbers.holdout_coverage,
        train_rows: numbers.train_rows,
        holdout_rows: numbers.holdout_rows,
    })
}

fn train_season(
    games: &[GameLog],
    season: &str,
    season_type: &str,
    spec_dirs: &[PathBuf],
    models_dir: &Path,
) -> Vec<TrainStatReport> {
    Stat::all()
        .iter()
        .map(|stat| match train_stat(games, *stat, season, season_type, spec_dirs, models_dir) {
            Ok(report) => report,
            Err(error) => {
                let mut report = bare_report(*stat);
                report.error = Some(error);
                if let Ok(spec) = engine::load_spec(stat.id(), spec_dirs) {
                    fill_spec_gaps(&mut report, &spec);
                }
                report
            }
        })
        .collect()
}

fn score_stat(
    directory: &Path,
    spec_dirs: &[PathBuf],
    stat: Stat,
    season: &str,
    season_type: &str,
) -> TrainStatReport {
    let spec = engine::load_spec(stat.id(), spec_dirs).ok();
    let mut report = match engine::load_score(directory, stat.id()) {
        Ok(score) if score.season == season && score.season_type == season_type => {
            let mut report = bare_report(stat);
            report.train_rows = score.train_rows;
            report.holdout_rows = score.holdout_rows;
            report.holdout_mae = score.holdout_mae;
            report.baseline_mae = score.baseline_mae;
            report.holdout_coverage = score.holdout_coverage;
            report.fitted_at = score.fitted_at;
            report.prior_minutes = score.prior_minutes;
            report.opponent_minutes = score.opponent_minutes;
            report.shift_prior = score.shift_prior;
            report.home_multiplier = score.home_multiplier;
            report.rest_per_day = score.rest_per_day;
            report.settings_stored = score.settings_stored;
            report
        }
        Ok(score) => {
            let mut report = bare_report(stat);
            report.error = Some(format!(
                "The saved model is for {} {}.",
                score.season, score.season_type
            ));
            report.fitted_at = score.fitted_at;
            report
        }
        Err(error) => {
            let mut report = bare_report(stat);
            report.error = Some(error.to_string());
            report
        }
    };
    if let Some(spec) = spec.as_ref() {
        fill_spec_gaps(&mut report, spec);
    }
    report
}

fn bare_report(stat: Stat) -> TrainStatReport {
    TrainStatReport {
        stat: stat.id().to_string(),
        label: stat.label().to_string(),
        error: None,
        train_rows: 0,
        holdout_rows: 0,
        holdout_mae: None,
        baseline_mae: None,
        holdout_coverage: None,
        fitted_at: None,
        prior_minutes: None,
        opponent_minutes: None,
        shift_prior: None,
        home_multiplier: None,
        rest_per_day: None,
        settings_stored: false,
    }
}

/// A missing fit can still show the priors an agent would train with.
fn fill_spec_gaps(report: &mut TrainStatReport, spec: &ModelSpec) {
    if report.prior_minutes.is_none() {
        report.prior_minutes = Some(spec.prior_minutes);
    }
    if report.opponent_minutes.is_none() {
        report.opponent_minutes = Some(spec.opponent_minutes);
    }
    if report.shift_prior.is_none() {
        report.shift_prior = Some(spec.shift_prior);
    }
}

fn train_stat(
    games: &[GameLog],
    stat: Stat,
    season: &str,
    season_type: &str,
    spec_dirs: &[PathBuf],
    models_dir: &Path,
) -> Result<TrainStatReport, String> {
    eprintln!("training {}...", stat.id());
    let _ = std::io::Write::flush(&mut std::io::stderr());
    let spec = engine::load_spec(stat.id(), spec_dirs).map_err(show)?;
    if spec.stat != stat.id() {
        return Err(format!(
            "The {} spec file says {}.",
            stat.id(),
            spec.stat
        ));
    }
    let fitted = engine::train_one(games, &spec, season, season_type).map_err(show)?;
    let fitted_at = Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string();
    engine::save_model(&fitted, &fitted_at, models_dir).map_err(show)?;
    let mut report = bare_report(stat);
    report.train_rows = fitted.train_rows;
    report.holdout_rows = fitted.holdout_rows;
    report.holdout_mae = fitted.holdout_mae;
    report.baseline_mae = fitted.baseline_mae;
    report.holdout_coverage = fitted.holdout_coverage;
    report.fitted_at = Some(fitted_at);
    report.prior_minutes = Some(fitted.prior_minutes);
    report.opponent_minutes = Some(fitted.opponent_minutes);
    report.shift_prior = Some(fitted.shift_prior);
    report.home_multiplier = fitted.home_multiplier;
    report.rest_per_day = fitted.rest_per_day;
    report.settings_stored = true;
    fill_spec_gaps(&mut report, &spec);
    Ok(report)
}

pub fn train_cached(db_path: &Path, season: &str, season_type: &str) -> Result<TrainReport, String> {
    let directory = db_path.parent().ok_or_else(|| {
        "The database path has no folder to store the models.".to_string()
    })?;
    let connection = db::open(db_path).map_err(show)?;
    let season = season::validate_season(season).map_err(show)?;
    let season_type = season::validate_season_type(season_type).map_err(show)?;
    let games = db::season_games(&connection, &season, &season_type).map_err(show)?;
    if games.is_empty() {
        return Err("No cached games for that season. Sync it, then train.".to_string());
    }
    let models_dir = directory.join("models");
    let stats = train_season(&games, &season, &season_type, &engine::spec_dirs(directory), &models_dir);
    Ok(TrainReport {
        season,
        season_type,
        stats,
    })
}

pub fn build_state(connection: Connection, data_dir: &Path) -> Result<AppState, AppError> {
    Ok(AppState {
        db: Mutex::new(connection),
        nba: NbaClient::new()?,
        syncing: AtomicBool::new(false),
        models_dir: data_dir.join("models"),
        spec_dirs: engine::spec_dirs(data_dir),
        fitted: Mutex::new(HashMap::new()),
        model_generation: AtomicU64::new(0),
    })
}
