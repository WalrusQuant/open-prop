use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};

use chrono::{Local, Utc};
use rusqlite::Connection;
use tauri::State;

use crate::db;
use crate::engine::{self, Fitted, ModelSpec};
use crate::error::AppError;
use crate::models::{
    BoardQuery, BoardRow, Bootstrap, CatalogItem, GameLog, PlayerOption, PredictQuery, Prediction,
    RosterEntry, SeasonStatus, Stat, SyncReport, TrainQuery, TrainReport, TrainStatReport, TrendQuery,
    TrendReport, Window,
};
use crate::nba::NbaClient;
use crate::season::{self, SEASON_TYPES};
use crate::stats;

pub struct AppState {
    pub db: Mutex<Connection>,
    pub nba: NbaClient,
    pub syncing: AtomicBool,
    /// Held for a whole fit, so a second train from any page is turned away.
    pub training: AtomicBool,
    pub models_dir: PathBuf,
    pub spec_dirs: Vec<PathBuf>,
    fitted: ModelCache<Fitted>,
}

struct BusyGuard<'a>(&'a AtomicBool);

impl Drop for BusyGuard<'_> {
    fn drop(&mut self) {
        self.0.store(false, Ordering::SeqCst);
    }
}

fn enter<'a>(flag: &'a AtomicBool, busy: &str) -> Result<BusyGuard<'a>, String> {
    if flag
        .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
        .is_err()
    {
        Err(busy.to_string())
    } else {
        Ok(BusyGuard(flag))
    }
}

/// Loaded models by season, type, and stat. The generation and the map share one
/// lock, so a predict that loaded a file before a train finished cannot put the
/// old model back after the train clears the cache.
struct ModelCache<T> {
    inner: Mutex<CacheInner<T>>,
}

struct CacheInner<T> {
    generation: u64,
    entries: HashMap<String, Arc<T>>,
}

impl<T> ModelCache<T> {
    fn new() -> Self {
        Self {
            inner: Mutex::new(CacheInner {
                generation: 0,
                entries: HashMap::new(),
            }),
        }
    }

    fn lock(&self) -> Result<MutexGuard<'_, CacheInner<T>>, String> {
        self.inner
            .lock()
            .map_err(|_| "the model cache lock was poisoned".to_string())
    }

    /// The cached model, if any, and the generation it was read under.
    fn get(&self, key: &str) -> Result<(u64, Option<Arc<T>>), String> {
        let inner = self.lock()?;
        Ok((inner.generation, inner.entries.get(key).cloned()))
    }

    /// Keeps a model loaded under `generation` only if no train finished since.
    fn insert_if_current(&self, generation: u64, key: String, value: Arc<T>) -> Result<bool, String> {
        let mut inner = self.lock()?;
        if inner.generation != generation {
            return Ok(false);
        }
        inner.entries.entry(key).or_insert(value);
        Ok(true)
    }

    fn invalidate(&self) -> Result<(), String> {
        let mut inner = self.lock()?;
        inner.generation += 1;
        inner.entries.clear();
        Ok(())
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
    let _guard = enter(&state.syncing, "A sync is already running.")?;
    let games = state
        .nba
        .player_game_logs(&season, &season_type)
        .await
        .map_err(show)?;
    let roster = if wants_roster(&season) {
        state.nba.roster(&season).await.ok()
    } else {
        None
    };
    let connection = lock_db(&state.db)?;
    finish_sync(&connection, &season, &season_type, &games, roster.as_deref())
}

/// Rosters are for the season that is on or, from July, the one about to open. The call
/// answers with today's teams, so a finished season would get next season's rookies.
fn wants_roster(season: &str) -> bool {
    season::is_roster_season(season, Local::now().date_naive())
}

/// Merges the logs, then stores the roster when the call answered. A failed or thin roster
/// leaves the stored one, and the player list falls back to last season's teams.
fn finish_sync(
    connection: &Connection,
    season: &str,
    season_type: &str,
    games: &[GameLog],
    roster: Option<&[RosterEntry]>,
) -> Result<SyncReport, String> {
    let mut report = db::merge_logs(connection, season, season_type, games).map_err(show)?;
    if let Some(entries) = roster {
        report.roster_players = db::save_roster(connection, season, entries).map_err(show)?;
    }
    Ok(report)
}

#[tauri::command]
pub fn players(
    state: State<'_, AppState>,
    season: String,
    season_type: String,
    carry: Option<bool>,
) -> Result<Vec<PlayerOption>, String> {
    let season = season::validate_season(&season).map_err(show)?;
    let season_type = season::validate_season_type(&season_type).map_err(show)?;
    let connection = lock_db(&state.db)?;
    // With carry on, last season's players and this season's rosters are listed before their first game.
    if carry.unwrap_or(false) {
        db::players_with_carry(&connection, &season, &season_type).map_err(show)
    } else {
        db::players(&connection, &season, &season_type).map_err(show)
    }
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
        return no_games_trend(&connection, &season, &season_type, query.player_id, stat, window, query.line);
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

/// A listed player with no game this season gets empty windows and his carried season.
fn no_games_trend(
    connection: &Connection,
    season: &str,
    season_type: &str,
    player_id: i64,
    stat: Stat,
    window: Window,
    line: f64,
) -> Result<TrendReport, String> {
    let player = db::players_with_carry(connection, season, season_type)
        .map_err(show)?
        .into_iter()
        .find(|player| player.player_id == player_id)
        .ok_or_else(|| {
            "No cached games for that player. Sync this season, then try the name again.".to_string()
        })?;
    let carried = match season::seed_source(season, season_type) {
        Some((seed_season, seed_type)) => {
            let games = db::player_games(connection, &seed_season, &seed_type, player_id).map_err(show)?;
            Some((format!("{seed_season} {seed_type}"), games))
        }
        None => None,
    };
    Ok(stats::no_games_report(
        &player,
        stat,
        window,
        line,
        season,
        season_type,
        carried.as_ref().map(|(label, games)| (label.as_str(), games.as_slice())),
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
    let _guard = enter(&state.training, "A fit is already running. Wait for it to finish.")?;
    let games = {
        let connection = lock_db(&state.db)?;
        db::season_games(&connection, &season, &season_type).map_err(show)?
    };
    let (seed, seed_note) = if query.seed {
        let connection = lock_db(&state.db)?;
        seed_games(&connection, &season, &season_type)?
    } else {
        (None, None)
    };
    // Before opening night a fit can stand on the carried season alone.
    if games.is_empty() && seed.is_none() {
        return Err("No cached games for that season. Sync it, then train.".to_string());
    }
    let spec_dirs = state.spec_dirs.clone();
    let models_dir = state.models_dir.clone();
    let season_for_train = season.clone();
    let type_for_train = season_type.clone();
    let stats = tokio::task::spawn_blocking(move || {
        train_season(&games, &season_for_train, &type_for_train, seed.as_ref(), &spec_dirs, &models_dir)
    })
    .await
    .map_err(|error| format!("training stopped: {error}"))?;
    state.fitted.invalidate()?;
    Ok(TrainReport {
        season,
        season_type,
        stats,
        seed_note,
    })
}

/// Cached games of the season that seeds this one, or a note saying why there are none.
type SeedGames = (String, String, Vec<GameLog>);

fn seed_games(
    connection: &rusqlite::Connection,
    season: &str,
    season_type: &str,
) -> Result<(Option<SeedGames>, Option<String>), String> {
    let Some((seed_season, seed_type)) = season::seed_source(season, season_type) else {
        return Ok((None, None));
    };
    let games = db::season_games(connection, &seed_season, &seed_type).map_err(show)?;
    if games.is_empty() {
        let note = format!(
            "{seed_season} {seed_type} is not cached, so this fit starts from the role priors. Sync it to carry it over."
        );
        return Ok((None, Some(note)));
    }
    Ok((Some((seed_season, seed_type, games)), None))
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
    // The spot only needs this player's history, so skip the rest of the league.
    let games = {
        let connection = lock_db(&state.db)?;
        db::player_games(&connection, &season, &season_type, query.player_id).map_err(show)?
    };
    // No games this season is fine: a carried player is priced from the prior, and the
    // model says why anyone else has to wait.
    let key = format!("{season}|{season_type}|{}", stat.id());
    let (generation, cached) = state.fitted.get(&key)?;
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
            None => Arc::new(
                engine::load_model(&models_dir, &season, &season_type, stat.id()).map_err(show)?,
            ),
        };
        let numbers = engine::predict_spot(&fitted, &games, &spot, &window, line).map_err(show)?;
        Ok::<_, String>((fitted, numbers))
    })
    .await
    .map_err(|error| format!("prediction stopped: {error}"))??;
    state.fitted.insert_if_current(generation, key, fitted)?;
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
        prior_from: numbers.prior_from,
    })
}

fn train_season(
    games: &[GameLog],
    season: &str,
    season_type: &str,
    seed: Option<&SeedGames>,
    spec_dirs: &[PathBuf],
    models_dir: &Path,
) -> Vec<TrainStatReport> {
    let seed = seed.map(|(seed_season, seed_type, seed_games)| engine::Seed {
        games: seed_games,
        season: seed_season,
        season_type: seed_type,
    });
    Stat::all()
        .iter()
        .map(|stat| match train_stat(games, *stat, season, season_type, seed.as_ref(), spec_dirs, models_dir) {
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
    let mut report = match engine::load_score(directory, season, season_type, stat.id()) {
        Ok(score) => {
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
            report.seeded_from = score.seeded_from;
            report.carry_minutes = score.carry_minutes;
            report.opponent_carry_minutes = score.opponent_carry_minutes;
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
        seeded_from: None,
        carry_minutes: None,
        opponent_carry_minutes: None,
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
    if report.carry_minutes.is_none() {
        report.carry_minutes = Some(spec.carry_minutes);
    }
    if report.opponent_carry_minutes.is_none() {
        report.opponent_carry_minutes = Some(spec.opponent_carry_minutes);
    }
}

fn train_stat(
    games: &[GameLog],
    stat: Stat,
    season: &str,
    season_type: &str,
    seed: Option<&engine::Seed>,
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
    let fitted = engine::train_one(games, &spec, season, season_type, seed).map_err(show)?;
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
    report.seeded_from = fitted.seeded_from.clone();
    report.carry_minutes = Some(fitted.carry_minutes);
    report.opponent_carry_minutes = Some(fitted.opponent_carry_minutes);
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
    let (seed, seed_note) = seed_games(&connection, &season, &season_type)?;
    if games.is_empty() && seed.is_none() {
        return Err("No cached games for that season. Sync it, then train.".to_string());
    }
    let models_dir = directory.join("models");
    engine::migrate_legacy_models(&models_dir).map_err(show)?;
    let stats = train_season(
        &games,
        &season,
        &season_type,
        seed.as_ref(),
        &engine::spec_dirs(directory),
        &models_dir,
    );
    Ok(TrainReport {
        season,
        season_type,
        stats,
        seed_note,
    })
}

pub fn backtest_prior(
    db_path: &Path,
    season: &str,
    seed_season: &str,
    stats: &[String],
    arms: &[(f64, f64)],
    last_game: usize,
) -> Result<String, String> {
    let directory = db_path.parent().unwrap_or_else(|| Path::new("."));
    let connection = db::open(db_path).map_err(show)?;
    let regular = season::SEASON_TYPES[0];
    let season = season::validate_season(season).map_err(show)?;
    let seed_season = season::validate_season(seed_season).map_err(show)?;
    let games = db::season_games(&connection, &season, regular).map_err(show)?;
    let seed = db::season_games(&connection, &seed_season, regular).map_err(show)?;
    if games.is_empty() || seed.is_empty() {
        return Err(format!(
            "Sync {season} and {seed_season} {regular} into this database first."
        ));
    }
    let arms: Vec<engine::backtest::Arm> = arms
        .iter()
        .map(|(carry, opponent)| engine::backtest::Arm {
            carry_minutes: *carry,
            opponent_carry_minutes: *opponent,
        })
        .collect();
    let mut text = format!(
        "## {season} seeded from {seed_season}, team games 1-{last_game}\n\n"
    );
    for id in stats {
        let stat = Stat::parse(id).ok_or_else(|| format!("'{id}' is not a stat this desk tracks."))?;
        let spec = engine::load_spec(stat.id(), &engine::spec_dirs(directory)).map_err(show)?;
        eprintln!("backtesting {}...", stat.id());
        let scores = engine::backtest::run(&games, &seed, stat, &spec, &arms, last_game).map_err(show)?;
        text.push_str(&engine::backtest::report(stat, &arms, &scores));
        text.push('\n');
    }
    Ok(text)
}

pub fn sync_cached(db_path: &Path, season: &str, season_type: &str) -> Result<SyncReport, String> {
    let season = season::validate_season(season).map_err(show)?;
    let season_type = season::validate_season_type(season_type).map_err(show)?;
    let client = NbaClient::new().map_err(show)?;
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|error| format!("could not start the sync runtime: {error}"))?;
    let games = runtime
        .block_on(client.player_game_logs(&season, &season_type))
        .map_err(show)?;
    let roster = if wants_roster(&season) {
        runtime.block_on(client.roster(&season)).ok()
    } else {
        None
    };
    let connection = db::open(db_path).map_err(show)?;
    finish_sync(&connection, &season, &season_type, &games, roster.as_deref())
}

pub fn build_state(connection: Connection, data_dir: &Path) -> Result<AppState, AppError> {
    let models_dir = data_dir.join("models");
    // Older installs kept one file per stat. A failed move leaves that file in place.
    if let Err(error) = engine::migrate_legacy_models(&models_dir) {
        eprintln!("could not move the old model files: {error}");
    }
    Ok(AppState {
        db: Mutex::new(connection),
        nba: NbaClient::new()?,
        syncing: AtomicBool::new(false),
        training: AtomicBool::new(false),
        models_dir,
        spec_dirs: engine::spec_dirs(data_dir),
        fitted: ModelCache::new(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn log(id: i64, team: &str, date: &str, pts: i32) -> GameLog {
        GameLog {
            player_id: id,
            player_name: format!("P{id}"),
            team_abbr: team.to_string(),
            game_id: format!("{id}-{date}"),
            game_date: date.to_string(),
            matchup: format!("{team} vs. BOS"),
            wl: "W".to_string(),
            minutes: 30.0,
            pts,
            reb: 5,
            ast: 5,
            stl: 1,
            blk: 0,
            tov: 2,
            fgm: 8,
            fga: 15,
            fg3m: 2,
            ftm: 3,
            plus_minus: 1,
        }
    }

    #[test]
    fn a_failed_roster_call_falls_back_to_last_seasons_team() {
        let connection = db::open_memory().unwrap();
        db::merge_logs(&connection, "2024-25", "Regular Season", &[log(7, "DEN", "2025-03-01", 22)]).unwrap();
        // Opening night: no games yet, and the roster call failed.
        let report = finish_sync(&connection, "2025-26", "Regular Season", &[], None).unwrap();
        assert!(report.warning.is_some());
        assert_eq!(report.roster_players, None);
        let player = db::players_with_carry(&connection, "2025-26", "Regular Season").unwrap();
        assert_eq!(player[0].team, "DEN");
        assert_eq!(player[0].team_source, crate::models::TeamSource::LastSeason);
        let trend = no_games_trend(&connection, "2025-26", "Regular Season", 7, Stat::Points, Window::Last10, 20.5)
            .unwrap();
        assert!(trend.no_games);
        assert_eq!(trend.last_season.as_ref().map(|last| last.split.overs), Some(1));
        // A roster that answers moves him.
        let mut roster: Vec<RosterEntry> = (100..500)
            .map(|id| RosterEntry { player_id: id, name: format!("P{id}"), team_abbr: "UTA".to_string() })
            .collect();
        roster.push(RosterEntry { player_id: 7, name: "P7".to_string(), team_abbr: "OKC".to_string() });
        let report = finish_sync(&connection, "2025-26", "Regular Season", &[], Some(&roster)).unwrap();
        assert_eq!(report.roster_players, Some(401));
        let player = db::players_with_carry(&connection, "2025-26", "Regular Season").unwrap();
        let seven = player.iter().find(|player| player.player_id == 7).unwrap();
        assert_eq!(seven.team, "OKC");
        assert!(no_games_trend(&connection, "2025-26", "Regular Season", 42, Stat::Points, Window::Last10, 20.5).is_err());
    }

    #[test]
    fn a_second_fit_is_turned_away_until_the_first_ends() {
        let flag = AtomicBool::new(false);
        let first = enter(&flag, "A fit is already running.").unwrap();
        let second = enter(&flag, "A fit is already running.");
        assert_eq!(second.err().as_deref(), Some("A fit is already running."));
        drop(first);
        assert!(enter(&flag, "busy").is_ok(), "the guard releases on drop");
    }

    #[test]
    fn a_fit_running_on_another_thread_blocks_a_second_one() {
        let flag = Arc::new(AtomicBool::new(false));
        let (started, wait_start) = std::sync::mpsc::channel();
        let (finish, wait_finish) = std::sync::mpsc::channel::<()>();
        let worker = {
            let flag = Arc::clone(&flag);
            std::thread::spawn(move || {
                let _guard = enter(&flag, "busy").unwrap();
                started.send(()).unwrap();
                wait_finish.recv().unwrap();
            })
        };
        wait_start.recv().unwrap();
        assert!(enter(&flag, "busy").is_err());
        finish.send(()).unwrap();
        worker.join().unwrap();
        assert!(enter(&flag, "busy").is_ok());
    }

    #[test]
    fn a_predict_that_straddles_a_train_does_not_pin_the_old_model() {
        let cache: ModelCache<String> = ModelCache::new();
        let (generation, cached) = cache.get("2025-26|Regular Season|points").unwrap();
        assert!(cached.is_none());
        // The train finishes while the predict is still reading the old file.
        cache.invalidate().unwrap();
        let kept = cache
            .insert_if_current(generation, "2025-26|Regular Season|points".to_string(), Arc::new("old".to_string()))
            .unwrap();
        assert!(!kept);
        assert!(cache.get("2025-26|Regular Season|points").unwrap().1.is_none());

        let (generation, _) = cache.get("2025-26|Regular Season|points").unwrap();
        assert!(cache
            .insert_if_current(generation, "2025-26|Regular Season|points".to_string(), Arc::new("new".to_string()))
            .unwrap());
        let (_, cached) = cache.get("2025-26|Regular Season|points").unwrap();
        assert_eq!(cached.as_deref().map(String::as_str), Some("new"));
    }
}
