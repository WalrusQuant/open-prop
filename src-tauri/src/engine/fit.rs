use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::fs::File;
use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use chrono::{Datelike, NaiveDate};
use serde::{Deserialize, Serialize};

use crate::error::{AppError, AppResult};
use crate::models::{GameLog, Stat};
use crate::stats::split_matchup;

use super::bayes::{self, at_least, band, convolve, mean, minute_nodes, scale_mix, std_dev};
use super::spec::{validate, ModelSpec};

/// The spot the user names. There is no schedule, so nothing here is "tomorrow".
#[derive(Debug, Clone)]
pub struct Spot {
    pub player_id: i64,
    pub opponent: Option<String>,
    pub home: bool,
    pub rest_days: f64,
    pub minutes: Option<f64>,
}

const MIN_TRAIN_ROWS: usize = 20;
const MIN_PRIOR: usize = 5;
const REST_CAP: f64 = 14.0;
const FIT_PASSES: usize = 6;

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Population {
    stat: String,
    prior_minutes: f64,
    opponent_minutes: f64,
    shift_prior: f64,
    role_minutes: Vec<f64>,
    role_rates: Vec<f64>,
    role_minute_means: Vec<f64>,
    role_log_sd: Vec<f64>,
    opponents: BTreeMap<String, f64>,
    teams: BTreeSet<String>,
    home_log: f64,
    rest_log: f64,
    league_rate: f64,
}

#[derive(Debug, Clone)]
pub struct Fitted {
    pub stat: String,
    pub season: String,
    pub season_type: String,
    parts: Vec<Population>,
    pub holdout_mae: Option<f64>,
    pub baseline_mae: Option<f64>,
    pub holdout_coverage: Option<f64>,
    pub train_rows: usize,
    pub holdout_rows: usize,
    pub prior_minutes: f64,
    pub opponent_minutes: f64,
    pub shift_prior: f64,
    pub home_multiplier: Option<f64>,
    pub rest_per_day: Option<f64>,
}

pub struct PredictNumbers {
    pub mean: f64,
    pub low: f64,
    pub high: f64,
    pub clear_probability: f64,
    pub sigma: f64,
    pub minutes: f64,
    pub pmf: Vec<f64>,
    pub shift_probability: Option<f64>,
    pub holdout_mae: Option<f64>,
    pub baseline_mae: Option<f64>,
    pub holdout_coverage: Option<f64>,
    pub train_rows: usize,
    pub holdout_rows: usize,
}

#[derive(Debug, Clone)]
pub struct ModelScore {
    pub train_rows: usize,
    pub holdout_rows: usize,
    pub holdout_mae: Option<f64>,
    pub baseline_mae: Option<f64>,
    pub holdout_coverage: Option<f64>,
    pub fitted_at: Option<String>,
    pub prior_minutes: Option<f64>,
    pub opponent_minutes: Option<f64>,
    pub shift_prior: Option<f64>,
    pub home_multiplier: Option<f64>,
    pub rest_per_day: Option<f64>,
    pub settings_stored: bool,
}

#[derive(Clone)]
struct Obs {
    player_id: i64,
    date: String,
    opponent: String,
    home: bool,
    rest_days: f64,
    minutes: f64,
    stat: f64,
}

struct Forecast {
    pmf: Vec<f64>,
    mean: f64,
    low: f64,
    high: f64,
    minutes: f64,
    sigma: f64,
    shift: Option<f64>,
}

pub fn train_one(
    games: &[GameLog],
    spec: &ModelSpec,
    season: &str,
    season_type: &str,
) -> AppResult<Fitted> {
    validate(spec)?;
    let stat = Stat::parse(&spec.stat).ok_or_else(|| {
        AppError::message(format!("'{}' is not a stat this desk trains.", spec.stat))
    })?;
    let part_stats = parts_of(stat);
    let histories: Vec<Vec<Obs>> = part_stats
        .iter()
        .map(|part| observations(games, *part))
        .collect();
    let Some(sample) = histories.first() else {
        return Err(AppError::message(format!(
            "{} has no games to train on.",
            spec.stat
        )));
    };
    let cutoff = holdout_start(sample);
    let teams = known_teams(sample);
    let mut parts = Vec::with_capacity(histories.len());
    for (part, rows) in part_stats.iter().zip(histories.iter()) {
        let train: Vec<Obs> = match &cutoff {
            Some(date) => rows.iter().filter(|row| row.date.as_str() < date.as_str()).cloned().collect(),
            None => rows.clone(),
        };
        if train.is_empty() {
            return Err(AppError::message(format!(
                "{} has no training games before the holdout.",
                part.id()
            )));
        }
        parts.push(fit_population(&train, spec, part.id(), &teams)?);
    }
    let (train_rows, holdout_rows, holdout_mae, baseline_mae, holdout_coverage) =
        score(&parts, &histories, cutoff.as_deref());
    if train_rows < MIN_TRAIN_ROWS {
        return Err(AppError::message(format!(
            "{} has {train_rows} training rows after holding out the last 20% of dates. A model needs at least {MIN_TRAIN_ROWS}.",
            spec.stat
        )));
    }
    let (home_multiplier, rest_per_day) = if parts.len() == 1 {
        (
            Some(parts[0].home_log.exp()),
            Some(parts[0].rest_log.exp()),
        )
    } else {
        (None, None)
    };
    Ok(Fitted {
        stat: spec.stat.clone(),
        season: season.to_string(),
        season_type: season_type.to_string(),
        parts,
        holdout_mae,
        baseline_mae,
        holdout_coverage,
        train_rows,
        holdout_rows,
        prior_minutes: spec.prior_minutes,
        opponent_minutes: spec.opponent_minutes,
        shift_prior: spec.shift_prior,
        home_multiplier,
        rest_per_day,
    })
}

pub fn predict_spot(
    fitted: &Fitted,
    games: &[GameLog],
    spot: &Spot,
    window: &str,
    line: f64,
) -> AppResult<PredictNumbers> {
    if !line.is_finite() || line < 0.0 {
        return Err(AppError::message(
            "The line has to be a number that is zero or greater.".to_string(),
        ));
    }
    if let Some(minutes) = spot.minutes {
        if !minutes.is_finite() || !(0.0..=60.0).contains(&minutes) {
            return Err(AppError::message(
                "Minutes have to be a number from 0 to 60.".to_string(),
            ));
        }
    }
    if !spot.rest_days.is_finite() || spot.rest_days < 0.0 {
        return Err(AppError::message(
            "Rest has to be a number that is zero or greater.".to_string(),
        ));
    }
    let stat = Stat::parse(&fitted.stat).ok_or_else(|| {
        AppError::message(format!("'{}' is not a stat this desk tracks.", fitted.stat))
    })?;
    let mut histories = Vec::new();
    for part in parts_of(stat) {
        let mine: Vec<Obs> = observations(games, part)
            .into_iter()
            .filter(|row| row.player_id == spot.player_id)
            .collect();
        if mine.len() < MIN_PRIOR {
            return Err(AppError::message(format!(
                "This player has {} cached games. A prediction starts after {MIN_PRIOR}.",
                mine.len()
            )));
        }
        histories.push(mine);
    }
    let opponent = spot
        .opponent
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("")
        .to_string();
    if !opponent.is_empty() {
        let known = histories
            .first()
            .and_then(|_| fitted.parts.first())
            .map(|part| part.teams.contains(&team_key(&opponent)))
            .unwrap_or(false);
        if !known {
            return Err(AppError::message(format!(
                "No cached games for opponent {}.",
                team_key(&opponent)
            )));
        }
    }
    let rest = spot.rest_days.min(REST_CAP);
    let forecast = combine(
        &fitted.parts,
        &histories,
        window_len(window),
        spot.minutes,
        &opponent,
        spot.home,
        rest,
        true,
    );
    Ok(PredictNumbers {
        clear_probability: at_least(&forecast.pmf, line),
        mean: forecast.mean,
        low: forecast.low,
        high: forecast.high,
        sigma: forecast.sigma,
        minutes: forecast.minutes,
        pmf: forecast.pmf,
        shift_probability: forecast.shift,
        holdout_mae: fitted.holdout_mae,
        baseline_mae: fitted.baseline_mae,
        holdout_coverage: fitted.holdout_coverage,
        train_rows: fitted.train_rows,
        holdout_rows: fitted.holdout_rows,
    })
}

/// Saves to `<models>/<season>/<season type>/<stat>.json`, so fits for different
/// seasons and season types sit side by side.
pub fn save_model(fitted: &Fitted, fitted_at: &str, directory: &Path) -> AppResult<()> {
    let path = model_path(directory, &fitted.season, &fitted.season_type, &fitted.stat);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let saved = SavedModel {
        kind: "boxscore".to_string(),
        stat: fitted.stat.clone(),
        season: fitted.season.clone(),
        season_type: fitted.season_type.clone(),
        parts: fitted.parts.clone(),
        holdout_mae: fitted.holdout_mae,
        baseline_mae: fitted.baseline_mae,
        holdout_coverage: fitted.holdout_coverage,
        train_rows: fitted.train_rows,
        holdout_rows: fitted.holdout_rows,
        prior_minutes: fitted.prior_minutes,
        opponent_minutes: fitted.opponent_minutes,
        shift_prior: fitted.shift_prior,
        home_multiplier: fitted.home_multiplier,
        rest_per_day: fitted.rest_per_day,
        fitted_at: Some(fitted_at.to_string()),
    };
    write_atomic(&path, |writer| {
        serde_json::to_writer(writer, &saved)?;
        Ok(())
    })
}

/// Writes next to `path` and renames over it, so a reader or a crash never sees half a model.
fn write_atomic(
    path: &Path,
    write: impl FnOnce(&mut BufWriter<File>) -> AppResult<()>,
) -> AppResult<()> {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| AppError::message("A model path has no file name.".to_string()))?;
    let temp: PathBuf = path.with_file_name(format!(
        ".{name}.{}.{}.tmp",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    let written = (|| -> AppResult<()> {
        let mut writer = BufWriter::new(File::create(&temp)?);
        write(&mut writer)?;
        writer.flush()?;
        writer.get_ref().sync_all()?;
        Ok(())
    })();
    if let Err(error) = written {
        let _ = std::fs::remove_file(&temp);
        return Err(error);
    }
    if let Err(error) = std::fs::rename(&temp, path) {
        let _ = std::fs::remove_file(&temp);
        return Err(error.into());
    }
    Ok(())
}

pub fn load_model(directory: &Path, season: &str, season_type: &str, stat: &str) -> AppResult<Fitted> {
    let saved = read_saved(directory, season, season_type, stat)?;
    Ok(Fitted {
        stat: saved.stat,
        season: saved.season,
        season_type: saved.season_type,
        parts: saved.parts,
        holdout_mae: saved.holdout_mae,
        baseline_mae: saved.baseline_mae,
        holdout_coverage: saved.holdout_coverage,
        train_rows: saved.train_rows,
        holdout_rows: saved.holdout_rows,
        prior_minutes: saved.prior_minutes,
        opponent_minutes: saved.opponent_minutes,
        shift_prior: saved.shift_prior,
        home_multiplier: saved.home_multiplier,
        rest_per_day: saved.rest_per_day,
    })
}

pub fn load_score(
    directory: &Path,
    season: &str,
    season_type: &str,
    stat: &str,
) -> AppResult<ModelScore> {
    let saved = read_saved(directory, season, season_type, stat)?;
    Ok(ModelScore {
        train_rows: saved.train_rows,
        holdout_rows: saved.holdout_rows,
        holdout_mae: saved.holdout_mae,
        baseline_mae: saved.baseline_mae,
        holdout_coverage: saved.holdout_coverage,
        fitted_at: saved.fitted_at,
        prior_minutes: Some(saved.prior_minutes),
        opponent_minutes: Some(saved.opponent_minutes),
        shift_prior: Some(saved.shift_prior),
        home_multiplier: saved.home_multiplier,
        rest_per_day: saved.rest_per_day,
        settings_stored: true,
    })
}

fn parts_of(stat: Stat) -> Vec<Stat> {
    match stat {
        Stat::PointsAssists => vec![Stat::Points, Stat::Assists],
        Stat::PointsRebounds => vec![Stat::Points, Stat::Rebounds],
        Stat::AssistsRebounds => vec![Stat::Assists, Stat::Rebounds],
        Stat::PointsAssistsRebounds => vec![Stat::Points, Stat::Rebounds, Stat::Assists],
        other => vec![other],
    }
}

fn window_len(window: &str) -> Option<usize> {
    match window {
        "last_5" => Some(5),
        "last_10" => Some(10),
        "last_20" => Some(20),
        "season" => None,
        _ => Some(10),
    }
}

fn team_key(value: &str) -> String {
    value.trim().to_uppercase()
}

fn observations(games: &[GameLog], stat: Stat) -> Vec<Obs> {
    let mut ordered: Vec<&GameLog> = games.iter().collect();
    ordered.sort_by(|left, right| {
        left.game_date
            .cmp(&right.game_date)
            .then(left.game_id.cmp(&right.game_id))
            .then(left.player_id.cmp(&right.player_id))
    });
    let mut last_day: HashMap<i64, i64> = HashMap::new();
    let mut rows = Vec::new();
    for game in ordered {
        if game.minutes <= 0.0 {
            continue;
        }
        let Ok(date) = NaiveDate::parse_from_str(&game.game_date, "%Y-%m-%d") else {
            continue;
        };
        let day = i64::from(date.num_days_from_ce());
        let rest = match last_day.insert(game.player_id, day) {
            Some(previous) => (day - previous - 1).clamp(0, REST_CAP as i64) as f64,
            None => 2.0,
        };
        let (site, opponent) = split_matchup(&game.matchup);
        let value = stat.value(game);
        if !value.is_finite() || value < 0.0 {
            continue;
        }
        rows.push(Obs {
            player_id: game.player_id,
            date: game.game_date.clone(),
            opponent: team_key(&opponent),
            home: site == "home",
            rest_days: rest,
            minutes: game.minutes,
            stat: value,
        });
    }
    rows
}

fn known_teams(rows: &[Obs]) -> BTreeSet<String> {
    rows.iter().map(|row| row.opponent.clone()).collect()
}

fn holdout_start(rows: &[Obs]) -> Option<String> {
    let mut dates: Vec<&str> = rows.iter().map(|row| row.date.as_str()).collect();
    dates.sort_unstable();
    dates.dedup();
    if dates.len() < 2 {
        return None;
    }
    let mut cut = (dates.len() as f64 * 0.8).floor() as usize;
    cut = cut.clamp(1, dates.len() - 1);
    Some(dates[cut].to_string())
}

fn fit_population(
    train: &[Obs],
    spec: &ModelSpec,
    stat_id: &str,
    teams: &BTreeSet<String>,
) -> AppResult<Population> {
    if train.is_empty() {
        return Err(AppError::message(format!(
            "{stat_id} has no training games."
        )));
    }
    let league_minutes: f64 = train.iter().map(|row| row.minutes).sum();
    let league_stat: f64 = train.iter().map(|row| row.stat).sum();
    let league_rate = if league_minutes > 0.0 {
        league_stat / league_minutes
    } else {
        0.0
    };
    let roles = player_roles(train, &spec.role_minutes);
    let role_count = spec.role_minutes.len() + 1;
    let mut role_stat = vec![0.0; role_count];
    let mut role_minutes = vec![0.0; role_count];
    let mut role_player_minutes = vec![Vec::<f64>::new(); role_count];
    let mut role_game_minutes = vec![Vec::<f64>::new(); role_count];
    for row in train {
        let role = roles.get(&row.player_id).copied().unwrap_or(0);
        role_stat[role] += row.stat;
        role_minutes[role] += row.minutes;
        role_game_minutes[role].push(row.minutes);
    }
    let mut seen_players: HashMap<i64, (f64, f64)> = HashMap::new();
    for row in train {
        let entry = seen_players.entry(row.player_id).or_insert((0.0, 0.0));
        entry.0 += row.minutes;
        entry.1 += 1.0;
    }
    for (player, (minutes, games)) in &seen_players {
        let role = roles.get(player).copied().unwrap_or(0);
        if *games > 0.0 {
            role_player_minutes[role].push(minutes / games);
        }
    }
    let mut role_rates = Vec::with_capacity(role_count);
    let mut role_minute_means = Vec::with_capacity(role_count);
    let mut role_log_sd = Vec::with_capacity(role_count);
    for index in 0..role_count {
        let rate = if role_minutes[index] > 0.0 {
            role_stat[index] / role_minutes[index]
        } else {
            league_rate
        };
        role_rates.push(rate.max(0.0));
        let minute_mean = if role_player_minutes[index].is_empty() {
            spec.role_minutes.first().copied().unwrap_or(20.0)
        } else {
            role_player_minutes[index].iter().sum::<f64>() / role_player_minutes[index].len() as f64
        };
        role_minute_means.push(minute_mean.clamp(1.0, 42.0));
        role_log_sd.push(log_spread(&role_game_minutes[index]));
    }

    let mut home_log = 0.0;
    let mut rest_log = 0.0;
    let mut opponents: HashMap<String, f64> = HashMap::new();
    for _ in 0..FIT_PASSES {
        let lambdas = player_rates(
            train,
            spec.prior_minutes,
            &role_rates,
            &roles,
            &opponents,
            home_log,
            rest_log,
            league_rate,
        );
        opponents = opponent_factors(
            train,
            &lambdas,
            home_log,
            rest_log,
            spec.opponent_minutes,
            league_rate,
        );
        fit_context(
            train,
            &lambdas,
            &opponents,
            &mut home_log,
            &mut rest_log,
            spec.home_sd,
            spec.rest_sd,
        );
    }
    Ok(Population {
        stat: stat_id.to_string(),
        prior_minutes: spec.prior_minutes,
        opponent_minutes: spec.opponent_minutes,
        shift_prior: spec.shift_prior,
        role_minutes: spec.role_minutes.clone(),
        role_rates,
        role_minute_means,
        role_log_sd,
        opponents: opponents.into_iter().collect(),
        teams: teams.clone(),
        home_log,
        rest_log,
        league_rate,
    })
}

fn player_roles(rows: &[Obs], cuts: &[f64]) -> HashMap<i64, usize> {
    let mut totals: HashMap<i64, (f64, f64)> = HashMap::new();
    for row in rows {
        let entry = totals.entry(row.player_id).or_insert((0.0, 0.0));
        entry.0 += row.minutes;
        entry.1 += 1.0;
    }
    totals
        .into_iter()
        .map(|(player, (minutes, games))| {
            let average = if games > 0.0 { minutes / games } else { 0.0 };
            (player, role_index(average, cuts))
        })
        .collect()
}

fn role_index(minutes: f64, cuts: &[f64]) -> usize {
    cuts.iter().filter(|cut| minutes >= **cut).count()
}

fn log_spread(minutes: &[f64]) -> f64 {
    let logs: Vec<f64> = minutes.iter().filter(|value| **value > 0.0).map(|value| value.ln()).collect();
    if logs.len() < 2 {
        return 0.2;
    }
    let mean = logs.iter().sum::<f64>() / logs.len() as f64;
    let variance = logs.iter().map(|value| (value - mean).powi(2)).sum::<f64>() / (logs.len() - 1) as f64;
    variance.sqrt().clamp(0.12, 0.45)
}

fn context_multiplier(home: bool, rest_days: f64, home_log: f64, rest_log: f64) -> f64 {
    let home_bit = if home { 1.0 } else { 0.0 };
    (home_log * home_bit + rest_log * rest_days).exp()
}

fn player_rates(
    rows: &[Obs],
    prior_minutes: f64,
    role_rates: &[f64],
    roles: &HashMap<i64, usize>,
    opponents: &HashMap<String, f64>,
    home_log: f64,
    rest_log: f64,
    league_rate: f64,
) -> HashMap<i64, f64> {
    let mut shape: HashMap<i64, f64> = HashMap::new();
    let mut rate: HashMap<i64, f64> = HashMap::new();
    for row in rows {
        let role = roles.get(&row.player_id).copied().unwrap_or(0);
        let prior_rate = role_rates.get(role).copied().unwrap_or(league_rate).max(0.0);
        shape.entry(row.player_id).or_insert(prior_minutes * prior_rate);
        rate.entry(row.player_id).or_insert(prior_minutes);
        let opp = opponents.get(&row.opponent).copied().unwrap_or(1.0);
        let exposure = row.minutes * opp * context_multiplier(row.home, row.rest_days, home_log, rest_log);
        *shape.get_mut(&row.player_id).unwrap() += row.stat;
        *rate.get_mut(&row.player_id).unwrap() += exposure;
    }
    shape
        .into_iter()
        .map(|(player, shape)| {
            let rate = rate.get(&player).copied().unwrap_or(prior_minutes).max(1e-6);
            (player, (shape / rate).max(0.0))
        })
        .collect()
}

fn opponent_factors(
    rows: &[Obs],
    lambdas: &HashMap<i64, f64>,
    home_log: f64,
    rest_log: f64,
    opponent_minutes: f64,
    league_rate: f64,
) -> HashMap<String, f64> {
    let mut observed: HashMap<String, f64> = HashMap::new();
    let mut expected: HashMap<String, f64> = HashMap::new();
    for row in rows {
        let lambda = lambdas.get(&row.player_id).copied().unwrap_or(league_rate);
        let base = lambda * row.minutes * context_multiplier(row.home, row.rest_days, home_log, rest_log);
        *observed.entry(row.opponent.clone()).or_insert(0.0) += row.stat;
        *expected.entry(row.opponent.clone()).or_insert(0.0) += base;
    }
    let strength = (opponent_minutes * league_rate).max(1e-6);
    observed
        .into_iter()
        .map(|(team, seen)| {
            let expect = expected.get(&team).copied().unwrap_or(0.0);
            let factor = (seen + strength) / (expect + strength);
            (team, factor.clamp(0.67, 1.5))
        })
        .collect()
}

fn fit_context(
    rows: &[Obs],
    lambdas: &HashMap<i64, f64>,
    opponents: &HashMap<String, f64>,
    home_log: &mut f64,
    rest_log: &mut f64,
    home_sd: f64,
    rest_sd: f64,
) {
    let prior_home = 1.0 / (home_sd * home_sd);
    let prior_rest = 1.0 / (rest_sd * rest_sd);
    for _ in 0..12 {
        let mut grad_home = -*home_log * prior_home;
        let mut grad_rest = -*rest_log * prior_rest;
        let mut hess_home = -prior_home;
        let mut hess_rest = -prior_rest;
        let mut hess_cross = 0.0;
        for row in rows {
            let lambda = lambdas.get(&row.player_id).copied().unwrap_or(0.0);
            let opp = opponents.get(&row.opponent).copied().unwrap_or(1.0);
            let base = (lambda * row.minutes * opp).max(1e-9);
            let home_bit = if row.home { 1.0 } else { 0.0 };
            let mu = base * context_multiplier(row.home, row.rest_days, *home_log, *rest_log);
            let resid = row.stat - mu;
            grad_home += resid * home_bit;
            grad_rest += resid * row.rest_days;
            hess_home -= mu * home_bit * home_bit;
            hess_rest -= mu * row.rest_days * row.rest_days;
            hess_cross -= mu * home_bit * row.rest_days;
        }
        let det = hess_home * hess_rest - hess_cross * hess_cross;
        if det.abs() < 1e-8 {
            break;
        }
        let step_home = (hess_rest * grad_home - hess_cross * grad_rest) / det;
        let step_rest = (hess_home * grad_rest - hess_cross * grad_home) / det;
        *home_log = (*home_log - step_home).clamp(-0.4, 0.4);
        *rest_log = (*rest_log - step_rest).clamp(-0.05, 0.05);
        if step_home.abs() + step_rest.abs() < 1e-6 {
            break;
        }
    }
}

fn score(
    parts: &[Population],
    histories: &[Vec<Obs>],
    cutoff: Option<&str>,
) -> (usize, usize, Option<f64>, Option<f64>, Option<f64>) {
    let Some(primary) = histories.first() else {
        return (0, 0, None, None, None);
    };
    let mut by_player: HashMap<i64, Vec<usize>> = HashMap::new();
    for (index, row) in primary.iter().enumerate() {
        by_player.entry(row.player_id).or_default().push(index);
    }
    let mut train_rows = 0usize;
    let mut holdout_abs = 0.0;
    let mut baseline_abs = 0.0;
    let mut holdout_rows = 0usize;
    let mut covered = 0usize;
    for indexes in by_player.values() {
        for (nth, index) in indexes.iter().enumerate() {
            if nth < MIN_PRIOR {
                continue;
            }
            let row = &primary[*index];
            let holdout = cutoff.is_some_and(|date| row.date.as_str() >= date);
            if !holdout {
                train_rows += 1;
                continue;
            }
            let past: Vec<Vec<Obs>> = histories
                .iter()
                .map(|rows| {
                    indexes
                        .iter()
                        .take(nth)
                        .map(|past_index| rows[*past_index].clone())
                        .collect()
                })
                .collect();
            let forecast = combine(
                parts,
                &past,
                Some(10),
                None,
                &row.opponent,
                row.home,
                row.rest_days,
                false,
            );
            let actual: f64 = histories.iter().map(|rows| rows[*index].stat).sum();
            holdout_abs += (actual - forecast.mean).abs();
            let baseline: f64 = past.iter().map(|history| last_totals(history, 10)).sum();
            baseline_abs += (actual - baseline).abs();
            if actual >= forecast.low && actual <= forecast.high {
                covered += 1;
            }
            holdout_rows += 1;
        }
    }
    if holdout_rows == 0 {
        return (train_rows, 0, None, None, None);
    }
    let count = holdout_rows as f64;
    (
        train_rows,
        holdout_rows,
        Some(holdout_abs / count),
        Some(baseline_abs / count),
        Some(covered as f64 / count),
    )
}

fn last_totals(history: &[Obs], count: usize) -> f64 {
    if history.is_empty() {
        return 0.0;
    }
    let start = history.len().saturating_sub(count);
    let slice = &history[start..];
    slice.iter().map(|row| row.stat).sum::<f64>() / slice.len() as f64
}

fn combine(
    parts: &[Population],
    histories: &[Vec<Obs>],
    window: Option<usize>,
    minutes: Option<f64>,
    opponent: &str,
    home: bool,
    rest_days: f64,
    report_shift: bool,
) -> Forecast {
    let minute_plan = minutes_plan(parts.first(), histories.first().map(Vec::as_slice).unwrap_or(&[]), minutes);
    let mut weighted = Vec::new();
    let mut weights = Vec::new();
    let mut minute_total = 0.0;
    let mut weight_total = 0.0;
    let mut shift = None;
    for (minutes_value, weight) in &minute_plan {
        let mut pmf: Option<Vec<f64>> = None;
        for (part, history) in parts.iter().zip(histories.iter()) {
            let (next, part_shift) = pmf_at(
                part,
                history,
                window,
                *minutes_value,
                opponent,
                home,
                rest_days,
            );
            if shift.is_none() && report_shift && parts.len() == 1 {
                shift = part_shift;
            }
            pmf = Some(match pmf {
                None => next,
                Some(so_far) => convolve(&so_far, &next),
            });
        }
        if let Some(pmf) = pmf {
            weighted.push(pmf);
            weights.push(*weight);
            minute_total += minutes_value * weight;
            weight_total += weight;
        }
    }
    let pmf = scale_mix(&weighted, &weights);
    let (low, high) = band(&pmf);
    Forecast {
        sigma: std_dev(&pmf),
        mean: mean(&pmf),
        low,
        high,
        minutes: if weight_total > 0.0 {
            minute_total / weight_total
        } else {
            minutes.unwrap_or(0.0)
        },
        pmf,
        shift,
    }
}

fn minutes_plan(part: Option<&Population>, history: &[Obs], minutes: Option<f64>) -> Vec<(f64, f64)> {
    if let Some(minutes) = minutes {
        return vec![(minutes.clamp(0.0, 48.0), 1.0)];
    }
    let Some(part) = part else {
        return vec![(24.0, 1.0)];
    };
    let role = role_index(average_minutes(history), &part.role_minutes);
    let role_mean = part
        .role_minute_means
        .get(role)
        .copied()
        .unwrap_or(24.0);
    let recent = last_minutes(history, 10);
    let mean = if recent.is_empty() {
        role_mean
    } else {
        let sum: f64 = recent.iter().sum();
        (sum + 3.0 * role_mean) / (recent.len() as f64 + 3.0)
    };
    let log_sd = part.role_log_sd.get(role).copied().unwrap_or(0.2);
    minute_nodes(mean, log_sd)
}

fn last_minutes(history: &[Obs], count: usize) -> Vec<f64> {
    let start = history.len().saturating_sub(count);
    history[start..]
        .iter()
        .map(|row| row.minutes)
        .filter(|minutes| *minutes > 0.0)
        .collect()
}

fn average_minutes(history: &[Obs]) -> f64 {
    if history.is_empty() {
        return 0.0;
    }
    history.iter().map(|row| row.minutes).sum::<f64>() / history.len() as f64
}

struct RatePosterior {
    shape: f64,
    rate: f64,
}

fn pmf_at(
    part: &Population,
    history: &[Obs],
    window: Option<usize>,
    minutes: f64,
    opponent: &str,
    home: bool,
    rest_days: f64,
) -> (Vec<f64>, Option<f64>) {
    let role = role_index(average_minutes(history), &part.role_minutes);
    let prior_rate = part.role_rates.get(role).copied().unwrap_or(part.league_rate).max(0.0);
    let season = posterior(part, history, prior_rate);
    let exposure = predictive_exposure(part, minutes, opponent, home, rest_days);
    let season_pmf = bayes::negative_binomial(season.shape, season.rate, exposure);
    let Some(window) = window else {
        return (season_pmf, None);
    };
    if history.len() < window + MIN_PRIOR {
        return (season_pmf, Some(0.0));
    }
    let split = history.len() - window;
    let early = &history[..split];
    let recent = &history[split..];
    let recent_posterior = posterior(part, recent, prior_rate);
    let recent_pmf = bayes::negative_binomial(recent_posterior.shape, recent_posterior.rate, exposure);
    let (early_y, early_m) = adjusted_totals(part, early);
    let (window_y, window_m) = adjusted_totals(part, recent);
    let shift = bayes::shift_probability(
        part.prior_minutes * prior_rate.max(1e-6),
        part.prior_minutes,
        early_y,
        early_m,
        window_y,
        window_m,
        part.shift_prior,
    );
    // A zero prior rate makes the gamma shape zero. Keep a tiny shape so the marginal stays defined.
    let shift = if (part.prior_minutes * prior_rate) <= 1e-8 {
        0.0
    } else {
        shift
    };
    (bayes::mix(&season_pmf, &recent_pmf, shift), Some(shift))
}

fn posterior(part: &Population, history: &[Obs], prior_rate: f64) -> RatePosterior {
    let mut shape = part.prior_minutes * prior_rate.max(0.0);
    let mut rate = part.prior_minutes;
    if shape <= 1e-8 {
        shape = 1e-3;
    }
    for row in history {
        shape += row.stat;
        rate += adjusted_exposure(part, row);
    }
    RatePosterior { shape, rate }
}

fn adjusted_exposure(part: &Population, row: &Obs) -> f64 {
    let opp = part.opponents.get(&row.opponent).copied().unwrap_or(1.0);
    row.minutes * opp * context_multiplier(row.home, row.rest_days, part.home_log, part.rest_log)
}

fn adjusted_totals(part: &Population, rows: &[Obs]) -> (f64, f64) {
    let mut stat = 0.0;
    let mut exposure = 0.0;
    for row in rows {
        stat += row.stat;
        exposure += adjusted_exposure(part, row);
    }
    (stat, exposure)
}

fn predictive_exposure(
    part: &Population,
    minutes: f64,
    opponent: &str,
    home: bool,
    rest_days: f64,
) -> f64 {
    let opp = if opponent.is_empty() {
        1.0
    } else {
        part.opponents.get(&team_key(opponent)).copied().unwrap_or(1.0)
    };
    minutes * opp * context_multiplier(home, rest_days, part.home_log, part.rest_log)
}

#[derive(Serialize, Deserialize)]
struct SavedModel {
    kind: String,
    stat: String,
    season: String,
    season_type: String,
    parts: Vec<Population>,
    holdout_mae: Option<f64>,
    baseline_mae: Option<f64>,
    holdout_coverage: Option<f64>,
    train_rows: usize,
    holdout_rows: usize,
    prior_minutes: f64,
    opponent_minutes: f64,
    shift_prior: f64,
    home_multiplier: Option<f64>,
    rest_per_day: Option<f64>,
    #[serde(default)]
    fitted_at: Option<String>,
}

/// `Regular Season` becomes `regular-season`. Anything outside letters and digits becomes a dash.
fn path_part(value: &str) -> String {
    value
        .trim()
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() {
                character.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect()
}

pub fn model_path(directory: &Path, season: &str, season_type: &str, stat: &str) -> PathBuf {
    directory
        .join(path_part(season))
        .join(path_part(season_type))
        .join(format!("{}.json", path_part(stat).replace('-', "_")))
}

/// Files from before per-season fits sit at `<models>/<stat>.json`. Each one moves
/// to the season and type it was fit on, unless a newer fit already holds that
/// slot. Files that do not parse stay where they are. Returns how many moved.
pub fn migrate_legacy_models(directory: &Path) -> AppResult<usize> {
    if !directory.is_dir() {
        return Ok(0);
    }
    let mut moved = 0;
    for entry in std::fs::read_dir(directory)? {
        let path = entry?.path();
        let is_json = path.extension().and_then(|value| value.to_str()) == Some("json");
        let hidden = path
            .file_name()
            .and_then(|value| value.to_str())
            .is_some_and(|name| name.starts_with('.'));
        if !path.is_file() || !is_json || hidden {
            continue;
        }
        let Ok(text) = std::fs::read_to_string(&path) else {
            continue;
        };
        let Ok(value) = serde_json::from_str::<serde_json::Value>(&text) else {
            continue;
        };
        let field = |name: &str| value.get(name).and_then(|item| item.as_str()).map(str::to_string);
        let (Some(stat), Some(season), Some(season_type)) =
            (field("stat"), field("season"), field("season_type"))
        else {
            continue;
        };
        if Stat::parse(&stat).is_none()
            || crate::season::validate_season(&season).is_err()
            || crate::season::validate_season_type(&season_type).is_err()
        {
            continue;
        }
        let target = model_path(directory, &season, &season_type, &stat);
        if target.exists() {
            continue;
        }
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::rename(&path, &target)?;
        moved += 1;
    }
    Ok(moved)
}

fn read_saved(directory: &Path, season: &str, season_type: &str, stat: &str) -> AppResult<SavedModel> {
    let path = model_path(directory, season, season_type, stat);
    if !path.is_file() {
        return Err(AppError::message(format!(
            "No trained model for {stat} in {season} {season_type}."
        )));
    }
    let text = std::fs::read_to_string(&path)?;
    let value: serde_json::Value = serde_json::from_str(&text)?;
    if value.get("booster").is_some() && value.get("parts").is_none() {
        return Err(AppError::message(
            "This file is the old tree model. Refit the season on the home page.".to_string(),
        ));
    }
    let saved: SavedModel = serde_json::from_value(value)?;
    if saved.stat != stat {
        return Err(AppError::message(format!(
            "The file for {stat} says {}.",
            saved.stat
        )));
    }
    if saved.season != season || saved.season_type != season_type {
        return Err(AppError::message(format!(
            "The {stat} file for {season} {season_type} says {} {}. Refit this season.",
            saved.season, saved.season_type
        )));
    }
    if saved.parts.is_empty() {
        return Err(AppError::message(format!(
            "The saved {stat} model has no rates."
        )));
    }
    Ok(saved)
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::NaiveDate;

    fn on_date(offset: i64) -> String {
        NaiveDate::from_ymd_opt(2025, 10, 22)
            .unwrap()
            .checked_add_signed(chrono::Duration::days(offset))
            .unwrap()
            .format("%Y-%m-%d")
            .to_string()
    }

    fn game(day: i64, home: bool, points: i32) -> GameLog {
        let team = "DAL";
        let opponent = "BOS";
        GameLog {
            player_id: 1,
            player_name: "P1".to_string(),
            team_abbr: team.to_string(),
            game_id: format!("g{day}"),
            game_date: on_date(day),
            matchup: if home {
                format!("{team} vs. {opponent}")
            } else {
                format!("{team} @ {opponent}")
            },
            wl: "W".to_string(),
            minutes: 32.0,
            pts: points,
            reb: 4,
            ast: 3,
            stl: 1,
            blk: 0,
            tov: 1,
            fgm: 8,
            fga: 15,
            fg3m: 2,
            ftm: 3,
            plus_minus: 0,
        }
    }

    fn spec(home_sd: f64) -> ModelSpec {
        ModelSpec {
            stat: "points".to_string(),
            prior_minutes: 80.0,
            opponent_minutes: 400.0,
            shift_prior: 0.1,
            role_minutes: vec![15.0, 28.0],
            home_sd,
            rest_sd: 0.02,
        }
    }

    #[test]
    fn home_games_score_higher_than_away_games() {
        let games: Vec<GameLog> = (0..40).map(|day| game(day, day % 2 == 0, if day % 2 == 0 { 25 } else { 15 })).collect();
        let fitted = train_one(&games, &spec(0.4), "2025-26", "Regular Season").unwrap();
        let spot = |home| Spot {
            player_id: 1,
            opponent: Some("BOS".to_string()),
            home,
            rest_days: 1.0,
            minutes: Some(32.0),
        };
        let home = predict_spot(&fitted, &games, &spot(true), "season", 20.0).unwrap();
        let away = predict_spot(&fitted, &games, &spot(false), "season", 20.0).unwrap();
        assert!(
            home.mean > away.mean + 4.0,
            "home {:.2} away {:.2}",
            home.mean,
            away.mean
        );
        assert!(home.low >= 0.0 && home.pmf.iter().sum::<f64>() > 0.98);
    }

    #[test]
    fn a_holdout_game_does_not_train_its_own_prediction() {
        let mut games = Vec::new();
        for day in 0..8 {
            games.push(game(day * 2, true, 10));
        }
        games.push(game(16, true, 40));
        games.push(game(18, true, 40));
        for player in 2..8 {
            for day in 0..8 {
                let mut row = game(day * 2, true, 10);
                row.player_id = player;
                row.player_name = format!("P{player}");
                games.push(row);
            }
        }
        let fitted = train_one(&games, &spec(0.08), "2025-26", "Regular Season").unwrap();
        let mae = fitted.holdout_mae.expect("holdout");
        assert!(
            mae > 15.0,
            "leaking the 40-point games would shrink the error, mae {mae:.2}"
        );
    }

    #[test]
    fn a_saved_model_predicts_the_same_mean() {
        let games: Vec<GameLog> = (0..36).map(|day| game(day * 2, true, 18)).collect();
        let fitted = train_one(&games, &spec(0.08), "2025-26", "Regular Season").unwrap();
        let spot = Spot {
            player_id: 1,
            opponent: Some("BOS".to_string()),
            home: true,
            rest_days: 2.0,
            minutes: Some(32.0),
        };
        let original = predict_spot(&fitted, &games, &spot, "last_10", 15.0).unwrap();
        let directory = std::env::temp_dir().join(format!("open-prop-bayes-{}", std::process::id()));
        save_model(&fitted, "2026-10-07T18:00:00Z", &directory).unwrap();
        let loaded = load_model(&directory, "2025-26", "Regular Season", "points").unwrap();
        let again = predict_spot(&loaded, &games, &spot, "last_10", 15.0).unwrap();
        assert!((again.mean - original.mean).abs() < 1e-6, "{} {}", again.mean, original.mean);
        let score = load_score(&directory, "2025-26", "Regular Season", "points").unwrap();
        assert_eq!(score.fitted_at.as_deref(), Some("2026-10-07T18:00:00Z"));
        assert!(score.settings_stored);
        let _ = std::fs::remove_dir_all(&directory);
    }

    #[test]
    fn one_players_games_predict_the_same_as_the_whole_league() {
        let mut games = Vec::new();
        for player in 1..6 {
            for day in 0..24 {
                let mut row = game(day * 2 + player % 2, day % 2 == 0, 10 + (player as i32) * 3 + (day as i32 % 5));
                row.player_id = player;
                row.player_name = format!("P{player}");
                games.push(row);
            }
        }
        let fitted = train_one(&games, &spec(0.08), "2025-26", "Regular Season").unwrap();
        let spot = Spot {
            player_id: 3,
            opponent: Some("BOS".to_string()),
            home: false,
            rest_days: 1.0,
            minutes: None,
        };
        let mine: Vec<GameLog> = games.iter().filter(|row| row.player_id == 3).cloned().collect();
        let league = predict_spot(&fitted, &games, &spot, "last_10", 18.5).unwrap();
        let alone = predict_spot(&fitted, &mine, &spot, "last_10", 18.5).unwrap();
        assert!((league.mean - alone.mean).abs() < 1e-12, "{} {}", league.mean, alone.mean);
        assert!((league.clear_probability - alone.clear_probability).abs() < 1e-12);
        assert_eq!(league.shift_probability, alone.shift_probability);
    }

    #[test]
    fn a_failed_save_leaves_the_old_model_loadable() {
        let games: Vec<GameLog> = (0..36).map(|day| game(day * 2, true, 18)).collect();
        let fitted = train_one(&games, &spec(0.08), "2025-26", "Regular Season").unwrap();
        let directory = std::env::temp_dir().join(format!("open-prop-atomic-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&directory);
        save_model(&fitted, "2026-10-07T18:00:00Z", &directory).unwrap();
        let path = model_path(&directory, "2025-26", "Regular Season", "points");
        let error = write_atomic(&path, |writer| {
            writer.write_all(br#"{"kind":"boxscore","stat":"poi"#)?;
            Err(AppError::message("disk full".to_string()))
        })
        .unwrap_err();
        assert_eq!(error.to_string(), "disk full");
        let loaded = load_model(&directory, "2025-26", "Regular Season", "points").unwrap();
        assert_eq!(loaded.season, "2025-26");
        let leftovers: Vec<_> = std::fs::read_dir(path.parent().unwrap())
            .unwrap()
            .filter_map(Result::ok)
            .filter(|entry| entry.file_name().to_string_lossy().ends_with(".tmp"))
            .collect();
        assert!(leftovers.is_empty(), "a failed write cleans up its temp file");
        save_model(&fitted, "2026-10-08T18:00:00Z", &directory).unwrap();
        let score = load_score(&directory, "2025-26", "Regular Season", "points").unwrap();
        assert_eq!(score.fitted_at.as_deref(), Some("2026-10-08T18:00:00Z"));
        let _ = std::fs::remove_dir_all(&directory);
    }

    #[test]
    fn an_old_tree_file_asks_for_a_refit() {
        let directory = std::env::temp_dir().join(format!("open-prop-old-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&directory);
        std::fs::create_dir_all(&directory).unwrap();
        std::fs::write(
            directory.join("points.json"),
            r#"{"stat":"points","season":"2025-26","season_type":"Regular Season","booster":{}}"#,
        )
        .unwrap();
        assert_eq!(migrate_legacy_models(&directory).unwrap(), 1);
        let error = load_model(&directory, "2025-26", "Regular Season", "points").unwrap_err();
        assert!(error.to_string().contains("old tree model"), "{error}");
        let _ = std::fs::remove_dir_all(&directory);
    }

    #[test]
    fn a_playoffs_fit_does_not_overwrite_the_regular_season() {
        let games: Vec<GameLog> = (0..36).map(|day| game(day * 2, true, 18)).collect();
        let regular = train_one(&games, &spec(0.08), "2025-26", "Regular Season").unwrap();
        let playoffs = train_one(&games, &spec(0.08), "2025-26", "Playoffs").unwrap();
        let older = train_one(&games, &spec(0.08), "2024-25", "Regular Season").unwrap();
        let directory = std::env::temp_dir().join(format!("open-prop-seasons-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&directory);
        save_model(&regular, "2026-10-07T18:00:00Z", &directory).unwrap();
        save_model(&playoffs, "2026-10-07T19:00:00Z", &directory).unwrap();
        save_model(&older, "2026-10-07T20:00:00Z", &directory).unwrap();
        assert!(directory.join("2025-26/regular-season/points.json").is_file());
        assert!(directory.join("2025-26/playoffs/points.json").is_file());
        let score = load_score(&directory, "2025-26", "Regular Season", "points").unwrap();
        assert_eq!(score.fitted_at.as_deref(), Some("2026-10-07T18:00:00Z"));
        let score = load_score(&directory, "2025-26", "Playoffs", "points").unwrap();
        assert_eq!(score.fitted_at.as_deref(), Some("2026-10-07T19:00:00Z"));
        let loaded = load_model(&directory, "2024-25", "Regular Season", "points").unwrap();
        assert_eq!((loaded.season.as_str(), loaded.season_type.as_str()), ("2024-25", "Regular Season"));
        let missing = load_model(&directory, "2023-24", "Playoffs", "points").unwrap_err();
        assert_eq!(missing.to_string(), "No trained model for points in 2023-24 Playoffs.");
        let _ = std::fs::remove_dir_all(&directory);
    }

    #[test]
    fn a_single_file_model_moves_to_its_own_season() {
        let games: Vec<GameLog> = (0..36).map(|day| game(day * 2, true, 18)).collect();
        let playoffs = train_one(&games, &spec(0.08), "2024-25", "Playoffs").unwrap();
        let directory = std::env::temp_dir().join(format!("open-prop-legacy-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&directory);
        save_model(&playoffs, "2026-10-07T18:00:00Z", &directory).unwrap();
        let keyed = model_path(&directory, "2024-25", "Playoffs", "points");
        std::fs::rename(&keyed, directory.join("points.json")).unwrap();
        std::fs::write(directory.join("notes.json"), "not a model").unwrap();

        assert_eq!(migrate_legacy_models(&directory).unwrap(), 1);
        assert!(!directory.join("points.json").exists());
        assert!(directory.join("notes.json").exists(), "files that are not models stay");
        let loaded = load_model(&directory, "2024-25", "Playoffs", "points").unwrap();
        assert_eq!(loaded.season_type, "Playoffs");
        let other = load_model(&directory, "2025-26", "Regular Season", "points").unwrap_err();
        assert!(other.to_string().starts_with("No trained model"), "{other}");
        assert_eq!(migrate_legacy_models(&directory).unwrap(), 0, "a second pass moves nothing");
        let _ = std::fs::remove_dir_all(&directory);
    }
}
