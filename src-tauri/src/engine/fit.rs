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

use super::bayes::{
    self, at_least, band, convolve, mean, minute_nodes, scale_mix, short_night, std_dev, widen,
};
use super::spec::{validate, ModelSpec};

pub mod backtest;
pub mod rookie;

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
/// A seeded fit borrows last season's role rates, minutes, home, and rest until this season has this many rows.
const SEED_POPULATION_ROWS: usize = 100;
/// Last season's minutes per game count as this many games in the minutes anchor and the role.
const CARRY_GAMES: f64 = 3.0;

/// A player's last season for one stat, already discounted to at most `carry_minutes`
/// pseudo-minutes. `stat` and `exposure` add to the gamma prior's shape and rate.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct Carry {
    stat: f64,
    exposure: f64,
    /// Minutes per game played last season.
    minutes: f64,
}

/// The cached games a fit seeds from: last season's Regular Season, or this season's for Playoffs.
pub struct Seed<'a> {
    pub games: &'a [GameLog],
    pub season: &'a str,
    pub season_type: &'a str,
}

impl Seed<'_> {
    fn label(&self) -> String {
        format!("{} {}", self.season, self.season_type)
    }
}

/// Last season's population for one part and every player's carry, built from the seed games.
#[derive(Clone)]
pub(crate) struct SeedPart {
    population: Population,
    carry: BTreeMap<i64, Carry>,
}

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
    /// Last season per player. Empty for an unseeded fit and for older files.
    #[serde(default)]
    carry: BTreeMap<i64, Carry>,
    #[serde(default)]
    carry_decay_tau: f64,
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
    /// "2025-26 Regular Season" when the fit carried that season over.
    pub seeded_from: Option<String>,
    pub carry_minutes: f64,
    pub opponent_carry_minutes: f64,
    pub carry_decay_tau: f64,
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
    /// The seed season while his carry-over still outweighs this season's minutes.
    pub prior_from: Option<String>,
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
    pub seeded_from: Option<String>,
    pub carry_minutes: Option<f64>,
    pub opponent_carry_minutes: Option<f64>,
    pub carry_decay_tau: Option<f64>,
}

#[derive(Clone)]
pub(crate) struct Obs {
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
    seed: Option<&Seed>,
) -> AppResult<Fitted> {
    validate(spec)?;
    let stat = Stat::parse(&spec.stat).ok_or_else(|| {
        AppError::message(format!("'{}' is not a stat this desk trains.", spec.stat))
    })?;
    let seeds = match seed {
        Some(seed) => seed_parts(seed.games, stat, spec)?,
        None => vec![None; parts_of(stat).len()],
    };
    let seeded_from = match seed {
        Some(seed) if seeds.iter().all(Option::is_some) => Some(seed.label()),
        _ => None,
    };
    let histories: Vec<Vec<Obs>> = parts_of(stat)
        .iter()
        .map(|part| observations(games, *part))
        .collect();
    let cutoff = histories.first().and_then(|rows| holdout_start(rows));
    let parts = fit_parts(&histories, stat, spec, &seeds, cutoff.as_deref())?;
    let (train_rows, holdout_rows, holdout_mae, baseline_mae, holdout_coverage) =
        score(&parts, &histories, cutoff.as_deref());
    // A seeded fit can price players before the holdout has enough rows to score.
    if train_rows < MIN_TRAIN_ROWS && seeded_from.is_none() {
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
        seeded_from,
        carry_minutes: spec.carry_minutes,
        opponent_carry_minutes: spec.opponent_carry_minutes,
        carry_decay_tau: spec.carry_decay_tau,
    })
}

/// Last season's population and carry for each part of `stat`. A part with no seed rows is None.
pub(crate) fn seed_parts(games: &[GameLog], stat: Stat, spec: &ModelSpec) -> AppResult<Vec<Option<SeedPart>>> {
    parts_of(stat)
        .iter()
        .map(|part| {
            let rows = observations(games, *part);
            if rows.is_empty() {
                return Ok(None);
            }
            let population = fit_population(&rows, spec, part.id(), &known_teams(&rows), &BTreeMap::new(), None)?;
            let carry = build_carry(&population, &rows, spec.carry_minutes);
            Ok(Some(SeedPart { population, carry }))
        })
        .collect()
}

/// Fits each part on rows before `cutoff`. A seeded part with fewer than
/// `SEED_POPULATION_ROWS` rows borrows last season's population.
fn fit_parts(
    histories: &[Vec<Obs>],
    stat: Stat,
    spec: &ModelSpec,
    seeds: &[Option<SeedPart>],
    cutoff: Option<&str>,
) -> AppResult<Vec<Population>> {
    let mut teams = histories.first().map(|rows| known_teams(rows)).unwrap_or_default();
    for seed in seeds.iter().flatten() {
        teams.extend(seed.population.teams.iter().cloned());
    }
    let none = BTreeMap::new();
    let mut parts = Vec::with_capacity(histories.len());
    for ((part, rows), seed) in parts_of(stat).iter().zip(histories.iter()).zip(seeds.iter()) {
        let train: Vec<Obs> = match cutoff {
            Some(date) => rows.iter().filter(|row| row.date.as_str() < date).cloned().collect(),
            None => rows.clone(),
        };
        let population = match seed {
            Some(seed) if train.len() < SEED_POPULATION_ROWS => borrowed_population(seed, spec, &teams),
            Some(seed) => fit_population(
                &train,
                spec,
                part.id(),
                &teams,
                &seed.carry,
                Some(&seed.population.opponents),
            )?,
            None if train.is_empty() => {
                return Err(AppError::message(format!(
                    "{} has no training games before the holdout.",
                    part.id()
                )));
            }
            None => fit_population(&train, spec, part.id(), &teams, &none, None)?,
        };
        parts.push(population);
    }
    Ok(parts)
}

/// Last season's population with this spec's priors, every opponent multiplier shrunk
/// toward 1 by `opponent_carry_minutes`, and the player carry attached.
fn borrowed_population(seed: &SeedPart, spec: &ModelSpec, teams: &BTreeSet<String>) -> Population {
    let mut population = seed.population.clone();
    let k = spec.opponent_minutes;
    let k_carry = spec.opponent_carry_minutes;
    population.opponents = population
        .opponents
        .iter()
        .map(|(team, factor)| (team.clone(), (k + k_carry * factor) / (k + k_carry)))
        .collect();
    population.prior_minutes = spec.prior_minutes;
    population.opponent_minutes = spec.opponent_minutes;
    population.shift_prior = spec.shift_prior;
    population.teams = teams.clone();
    population.carry = seed.carry.clone();
    population.carry_decay_tau = spec.carry_decay_tau;
    population
}

/// Each player's seed totals, raised to the power `min(1, carry_minutes / M)` so at most
/// `carry_minutes` adjusted minutes come over. Zero turns the player seed off.
fn build_carry(population: &Population, rows: &[Obs], carry_minutes: f64) -> BTreeMap<i64, Carry> {
    let mut totals: HashMap<i64, (f64, f64, f64, f64)> = HashMap::new();
    for row in rows {
        let entry = totals.entry(row.player_id).or_insert((0.0, 0.0, 0.0, 0.0));
        entry.0 += row.stat;
        entry.1 += adjusted_exposure(population, row);
        entry.2 += row.minutes;
        entry.3 += 1.0;
    }
    if !(carry_minutes > 0.0) {
        return BTreeMap::new();
    }
    totals
        .into_iter()
        .filter(|(_, (_, exposure, _, games))| *exposure > 0.0 && *games > 0.0)
        .map(|(player, (stat, exposure, minutes, games))| {
            let power = (carry_minutes / exposure).min(1.0);
            (
                player,
                Carry {
                    stat: stat * power,
                    exposure: exposure * power,
                    minutes: minutes / games,
                },
            )
        })
        .collect()
}

pub fn predict_spot(
    fitted: &Fitted,
    games: &[GameLog],
    spot: &Spot,
    window: &str,
    line: f64,
) -> AppResult<PredictNumbers> {
    // A player with last season carried over can be priced before his first game.
    let carried = fitted
        .parts
        .first()
        .is_some_and(|part| part.carry.contains_key(&spot.player_id));
    if !carried {
        let stat = Stat::parse(&fitted.stat).ok_or_else(|| {
            AppError::message(format!("'{}' is not a stat this desk tracks.", fitted.stat))
        })?;
        let played = parts_of(stat)
            .first()
            .map(|part| {
                observations(games, *part)
                    .iter()
                    .filter(|row| row.player_id == spot.player_id)
                    .count()
            })
            .unwrap_or(0);
        if played < MIN_PRIOR {
            let games_word = if played == 1 { "game" } else { "games" };
            return Err(AppError::message(match fitted.seeded_from.as_deref() {
                Some(source) => format!(
                    "He has no {source} minutes to carry, so a prediction waits for {MIN_PRIOR} games this season. He has {played} {games_word}."
                ),
                None => format!(
                    "This player has {played} cached {games_word}. A prediction starts after {MIN_PRIOR}."
                ),
            }));
        }
    }
    predict_with(fitted, games, spot, window, line, 0)
}

/// `predict_spot` with the game minimum as an argument. The backtest prices game one with 0.
pub(crate) fn predict_with(
    fitted: &Fitted,
    games: &[GameLog],
    spot: &Spot,
    window: &str,
    line: f64,
    min_games: usize,
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
        if mine.len() < min_games {
            return Err(AppError::message(format!(
                "This player has {} cached games. A prediction starts after {min_games}.",
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
        spot.player_id,
        window_len(window),
        spot.minutes,
        &opponent,
        spot.home,
        rest,
        true,
    );
    // The note stays while last season's pseudo-minutes outweigh this season's adjusted minutes.
    let prior_from = match (fitted.seeded_from.as_ref(), fitted.parts.first(), histories.first()) {
        (Some(source), Some(part), Some(history)) => part
            .carry
            .get(&spot.player_id)
            .filter(|carry| carry.exposure > adjusted_totals(part, history).1)
            .map(|_| source.clone()),
        _ => None,
    };
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
        prior_from,
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
        seeded_from: fitted.seeded_from.clone(),
        carry_minutes: fitted.carry_minutes,
        opponent_carry_minutes: fitted.opponent_carry_minutes,
        carry_decay_tau: fitted.carry_decay_tau,
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
        seeded_from: saved.seeded_from,
        carry_minutes: saved.carry_minutes,
        opponent_carry_minutes: saved.opponent_carry_minutes,
        carry_decay_tau: saved.carry_decay_tau,
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
        seeded_from: saved.seeded_from,
        carry_minutes: Some(saved.carry_minutes),
        opponent_carry_minutes: Some(saved.opponent_carry_minutes),
        carry_decay_tau: Some(saved.carry_decay_tau),
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
    carry: &BTreeMap<i64, Carry>,
    opponent_prior: Option<&BTreeMap<String, f64>>,
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
    let roles = player_roles(train, &spec.role_minutes, carry);
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
            carry,
        );
        opponents = opponent_factors(
            train,
            &lambdas,
            home_log,
            rest_log,
            spec.opponent_minutes,
            league_rate,
            opponent_prior.map(|prior| (prior, spec.opponent_carry_minutes)),
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
        carry: carry.clone(),
        carry_decay_tau: spec.carry_decay_tau,
    })
}

fn player_roles(rows: &[Obs], cuts: &[f64], carry: &BTreeMap<i64, Carry>) -> HashMap<i64, usize> {
    let mut totals: HashMap<i64, (f64, usize)> = HashMap::new();
    for row in rows {
        let entry = totals.entry(row.player_id).or_insert((0.0, 0));
        entry.0 += row.minutes;
        entry.1 += 1;
    }
    totals
        .into_iter()
        .map(|(player, (minutes, games))| {
            let average = role_minutes(minutes, games, carry.get(&player));
            (player, role_index(average, cuts))
        })
        .collect()
}

/// Average minutes for the role cut. Last season's minutes per game count as `CARRY_GAMES` games.
fn role_minutes(minutes: f64, games: usize, carry: Option<&Carry>) -> f64 {
    match carry {
        Some(carry) => (minutes + CARRY_GAMES * carry.minutes) / (games as f64 + CARRY_GAMES),
        None if games == 0 => 0.0,
        None => minutes / games as f64,
    }
}

/// Weight on last season's carry after `current_minutes` this season. Tau 0 keeps full carry.
fn carry_weight(current_minutes: f64, tau: f64) -> f64 {
    if !(tau > 0.0) || !(current_minutes > 0.0) {
        return 1.0;
    }
    (-current_minutes / tau).exp()
}

/// The gamma prior on a player's rate: the role prior, plus last season's discounted totals.
fn rate_prior(
    part: &Population,
    prior_rate: f64,
    carry: Option<&Carry>,
    current_minutes: f64,
) -> (f64, f64) {
    let weight = carry_weight(current_minutes, part.carry_decay_tau);
    let (stat, exposure) = carry.map_or((0.0, 0.0), |carry| (carry.stat * weight, carry.exposure * weight));
    (part.prior_minutes * prior_rate.max(0.0) + stat, part.prior_minutes + exposure)
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
    carry: &BTreeMap<i64, Carry>,
) -> HashMap<i64, f64> {
    let mut shape: HashMap<i64, f64> = HashMap::new();
    let mut rate: HashMap<i64, f64> = HashMap::new();
    for row in rows {
        let role = roles.get(&row.player_id).copied().unwrap_or(0);
        let prior_rate = role_rates.get(role).copied().unwrap_or(league_rate).max(0.0);
        let (carry_stat, carry_exposure) = carry
            .get(&row.player_id)
            .map_or((0.0, 0.0), |carry| (carry.stat, carry.exposure));
        shape.entry(row.player_id).or_insert(prior_minutes * prior_rate + carry_stat);
        rate.entry(row.player_id).or_insert(prior_minutes + carry_exposure);
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
    prior: Option<(&BTreeMap<String, f64>, f64)>,
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
    // Last season's multiplier is a second pseudo-observation, worth `opponent_carry_minutes`.
    let (carried, carry_strength) = match prior {
        Some((factors, minutes)) => (Some(factors), (minutes * league_rate).max(0.0)),
        None => (None, 0.0),
    };
    for team in carried.into_iter().flat_map(|factors| factors.keys()) {
        observed.entry(team.clone()).or_insert(0.0);
    }
    observed
        .into_iter()
        .map(|(team, seen)| {
            let expect = expected.get(&team).copied().unwrap_or(0.0);
            let before = carried.and_then(|factors| factors.get(&team)).copied().unwrap_or(1.0);
            let factor = (seen + strength + carry_strength * before) / (expect + strength + carry_strength);
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
                row.player_id,
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

/// Points only. The October 2026 walk-forward against Kalshi points props found real results
/// landing about 1.34 times farther from the mean (squared) than the predictive allowed, and
/// 4.7% of games at 3 or fewer points against 2.3% predicted. These stretch the points
/// predictive about 15% around its mean and mix in a 5% short night at about a third of
/// normal scoring. Other stats keep 1.0 and 0.0 until they have their own test.
pub const POINTS_SPREAD_FACTOR: f64 = 1.15;
pub const POINTS_SHORT_NIGHT_WEIGHT: f64 = 0.05;
pub const POINTS_SHORT_NIGHT_SCALE: f64 = 0.35;

/// Applies the points spread and short-night settings to a single-part points predictive.
fn adjust_tail(parts: &[Population], pmf: Vec<f64>) -> Vec<f64> {
    match parts {
        [only] if only.stat == "points" => short_night(
            &widen(&pmf, POINTS_SPREAD_FACTOR),
            POINTS_SHORT_NIGHT_WEIGHT,
            POINTS_SHORT_NIGHT_SCALE,
        ),
        _ => pmf,
    }
}

fn combine(
    parts: &[Population],
    histories: &[Vec<Obs>],
    player_id: i64,
    window: Option<usize>,
    minutes: Option<f64>,
    opponent: &str,
    home: bool,
    rest_days: f64,
    report_shift: bool,
) -> Forecast {
    let minute_plan = minutes_plan(
        parts.first(),
        histories.first().map(Vec::as_slice).unwrap_or(&[]),
        minutes,
        player_id,
    );
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
                part.carry.get(&player_id),
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
    let pmf = adjust_tail(parts, scale_mix(&weighted, &weights));
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

fn minutes_plan(
    part: Option<&Population>,
    history: &[Obs],
    minutes: Option<f64>,
    player_id: i64,
) -> Vec<(f64, f64)> {
    if let Some(minutes) = minutes {
        return vec![(minutes.clamp(0.0, 48.0), 1.0)];
    }
    let Some(part) = part else {
        return vec![(24.0, 1.0)];
    };
    let carry = part.carry.get(&player_id);
    let role = role_index(history_role_minutes(history, carry), &part.role_minutes);
    // A seeded player's anchor starts at his own minutes last season and fades to his role's
    // as this season's games come in, the same weight the role blend gives them.
    let role_mean = part.role_minute_means.get(role).copied().unwrap_or(24.0);
    let anchor = match carry {
        Some(carry) => {
            let games = history.len() as f64;
            (CARRY_GAMES * carry.minutes + games * role_mean) / (CARRY_GAMES + games)
        }
        None => role_mean,
    };
    let recent = last_minutes(history, 10);
    let mean = if recent.is_empty() {
        anchor
    } else {
        let sum: f64 = recent.iter().sum();
        (sum + CARRY_GAMES * anchor) / (recent.len() as f64 + CARRY_GAMES)
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

fn history_role_minutes(history: &[Obs], carry: Option<&Carry>) -> f64 {
    role_minutes(history.iter().map(|row| row.minutes).sum(), history.len(), carry)
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
    carry: Option<&Carry>,
) -> (Vec<f64>, Option<f64>) {
    let role = role_index(history_role_minutes(history, carry), &part.role_minutes);
    let prior_rate = part.role_rates.get(role).copied().unwrap_or(part.league_rate).max(0.0);
    let current_minutes: f64 = history.iter().map(|row| row.minutes).sum();
    let (prior_shape, prior_scale) = rate_prior(part, prior_rate, carry, current_minutes);
    let season = posterior(part, history, prior_shape, prior_scale);
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
    let recent_posterior = posterior(part, recent, prior_shape, prior_scale);
    let recent_pmf = bayes::negative_binomial(recent_posterior.shape, recent_posterior.rate, exposure);
    let (early_y, early_m) = adjusted_totals(part, early);
    let (window_y, window_m) = adjusted_totals(part, recent);
    let shift = bayes::shift_probability(
        prior_shape.max(part.prior_minutes * 1e-6),
        prior_scale,
        early_y,
        early_m,
        window_y,
        window_m,
        part.shift_prior,
    );
    // A zero prior rate makes the gamma shape zero. Keep a tiny shape so the marginal stays defined.
    let shift = if prior_shape <= 1e-8 {
        0.0
    } else {
        shift
    };
    (bayes::mix(&season_pmf, &recent_pmf, shift), Some(shift))
}

fn posterior(part: &Population, history: &[Obs], prior_shape: f64, prior_scale: f64) -> RatePosterior {
    let mut shape = prior_shape;
    let mut rate = prior_scale;
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
    #[serde(default)]
    seeded_from: Option<String>,
    #[serde(default)]
    carry_minutes: f64,
    #[serde(default)]
    opponent_carry_minutes: f64,
    #[serde(default)]
    carry_decay_tau: f64,
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
            carry_minutes: 400.0,
            opponent_carry_minutes: 1500.0,
            carry_decay_tau: 0.0,
        }
    }

    fn player_game(player: i64, day: i64, opponent: &str, minutes: f64, points: i32) -> GameLog {
        let mut row = game(day, day % 2 == 0, points);
        row.player_id = player;
        row.player_name = format!("P{player}");
        row.game_id = format!("g{player}-{day}");
        row.minutes = minutes;
        row.matchup = if day % 2 == 0 {
            format!("DAL vs. {opponent}")
        } else {
            format!("DAL @ {opponent}")
        };
        row
    }

    /// A season before 2025-26: player 1 scores 30 a night, players 2-7 score 10.
    fn last_season() -> Vec<GameLog> {
        let mut games = Vec::new();
        for day in 0..60 {
            let opponent = if day % 2 == 0 { "BOS" } else { "NYK" };
            games.push(player_game(1, day * 2 - 400, opponent, 32.0, 30));
            for player in 2..8 {
                games.push(player_game(player, day * 2 - 400, opponent, 32.0, 10));
            }
        }
        games
    }

    fn seed(games: &[GameLog]) -> Seed<'_> {
        Seed {
            games,
            season: "2024-25",
            season_type: "Regular Season",
        }
    }

    #[test]
    fn carry_decays_with_current_minutes() {
        assert!((carry_weight(0.0, 1000.0) - 1.0).abs() < 1e-12);
        assert!((carry_weight(500.0, 0.0) - 1.0).abs() < 1e-12);
        let half = carry_weight(1000.0 * (2.0_f64).ln(), 1000.0);
        assert!((half - 0.5).abs() < 1e-9, "{half}");
        let prior = rate_prior(
            &Population {
                stat: "pts".into(),
                prior_minutes: 100.0,
                opponent_minutes: 500.0,
                shift_prior: 0.1,
                role_minutes: vec![15.0, 28.0],
                role_rates: vec![0.5],
                role_minute_means: vec![30.0],
                role_log_sd: vec![0.2],
                opponents: BTreeMap::new(),
                teams: BTreeSet::new(),
                home_log: 0.0,
                rest_log: 0.0,
                league_rate: 0.5,
                carry: BTreeMap::new(),
                carry_decay_tau: 1000.0,
            },
            0.5,
            Some(&Carry { stat: 200.0, exposure: 400.0, minutes: 32.0 }),
            1000.0 * (2.0_f64).ln(),
        );
        assert!((prior.0 - (100.0 * 0.5 + 100.0)).abs() < 1e-6, "{:?}", prior);
        assert!((prior.1 - (100.0 + 200.0)).abs() < 1e-6, "{:?}", prior);
    }

    #[test]
    fn carry_is_capped_at_carry_minutes_and_zero_turns_it_off() {
        let mut before = last_season();
        for day in 0..3 {
            before.push(player_game(9, day * 2 - 400, "BOS", 20.0, 6));
        }
        let parts = seed_parts(&before, Stat::Points, &spec(0.08)).unwrap();
        let part = parts[0].as_ref().unwrap();
        let star = &part.carry[&1];
        // 60 games of 32 minutes come over as 400 pseudo-minutes at his own rate.
        assert!((star.exposure - 400.0).abs() < 1e-6, "{}", star.exposure);
        assert!((star.stat / star.exposure - 30.0 / 32.0).abs() < 0.05, "{star:?}");
        assert_eq!(star.minutes, 32.0);
        // Under the cap the whole season comes over.
        let bench = &part.carry[&9];
        assert!(bench.exposure < 70.0 && bench.exposure > 50.0, "{bench:?}");
        assert_eq!(bench.stat, 18.0);
        let mut off = spec(0.08);
        off.carry_minutes = 0.0;
        let parts = seed_parts(&before, Stat::Points, &off).unwrap();
        assert!(parts[0].as_ref().unwrap().carry.is_empty());
    }

    #[test]
    fn a_seeded_player_starts_from_last_season_and_a_rookie_from_his_role() {
        let before = last_season();
        // Two games in: player 1 was traded and scores 12, rookie 9 scores 12, the rest 10.
        let mut games = Vec::new();
        for day in 0..2 {
            games.push(player_game(1, day * 2, "LAL", 32.0, 12));
            games.push(player_game(9, day * 2, "LAL", 32.0, 12));
            for player in 2..8 {
                games.push(player_game(player, day * 2, "LAL", 32.0, 10));
            }
        }
        let error = train_one(&games, &spec(0.08), "2025-26", "Regular Season", None).unwrap_err();
        assert!(error.to_string().contains("training rows"), "{error}");
        let fitted =
            train_one(&games, &spec(0.08), "2025-26", "Regular Season", Some(&seed(&before))).unwrap();
        assert_eq!(fitted.seeded_from.as_deref(), Some("2024-25 Regular Season"));
        let spot = |player| Spot {
            player_id: player,
            opponent: Some("LAL".to_string()),
            home: true,
            rest_days: 2.0,
            minutes: Some(32.0),
        };
        let star = predict_spot(&fitted, &games, &spot(1), "last_10", 20.5).unwrap();
        assert!(star.mean > 20.0, "the carry should hold him near 30, mean {:.2}", star.mean);
        assert_eq!(star.prior_from.as_deref(), Some("2024-25 Regular Season"));
        // The rookie keeps the five-game gate and, under it, the role prior.
        let error = predict_spot(&fitted, &games, &spot(9), "last_10", 20.5).err().unwrap();
        assert!(error.to_string().contains("waits for 5 games"), "{error}");
        let rookie = predict_with(&fitted, &games, &spot(9), "last_10", 20.5, 0).unwrap();
        assert!(rookie.prior_from.is_none());
        assert!(rookie.mean > 10.0 && rookie.mean < 18.0, "rookie mean {:.2}", rookie.mean);
        // The carry fades: once his own minutes outweigh it, the note goes away.
        let mut longer = games.clone();
        for day in 2..20 {
            longer.push(player_game(1, day * 2, "LAL", 32.0, 12));
        }
        let later = predict_spot(&fitted, &longer, &spot(1), "season", 20.5).unwrap();
        assert!(later.prior_from.is_none());
        assert!(later.mean < star.mean);
    }

    #[test]
    fn a_seeded_fit_with_no_games_this_season_borrows_last_season() {
        let before = last_season();
        let error = train_one(&[], &spec(0.08), "2025-26", "Regular Season", None).unwrap_err();
        assert!(error.to_string().contains("no training games"), "{error}");
        let fitted = train_one(&[], &spec(0.08), "2025-26", "Regular Season", Some(&seed(&before))).unwrap();
        assert_eq!(fitted.seeded_from.as_deref(), Some("2024-25 Regular Season"));
        assert_eq!((fitted.train_rows, fitted.holdout_rows), (0, 0));
        assert!(fitted.holdout_mae.is_none());
        let source = seed_parts(&before, Stat::Points, &spec(0.08)).unwrap();
        let source = source[0].as_ref().unwrap();
        assert_eq!(fitted.parts[0].role_rates, source.population.role_rates);
        assert_eq!(fitted.parts[0].role_minute_means, source.population.role_minute_means);
        assert!(fitted.parts[0].teams.contains("BOS"));
        // Every part of a combo is borrowed the same way.
        let mut combo = spec(0.08);
        combo.stat = "points_assists_rebounds".to_string();
        let fitted = train_one(&[], &combo, "2025-26", "Regular Season", Some(&seed(&before))).unwrap();
        assert_eq!(fitted.parts.len(), 3);
    }

    #[test]
    fn a_carried_player_is_priced_before_his_first_game_and_a_rookie_is_told_why_not() {
        let before = last_season();
        let fitted = train_one(&[], &spec(0.08), "2025-26", "Regular Season", Some(&seed(&before))).unwrap();
        let spot = |player| Spot {
            player_id: player,
            opponent: Some("BOS".to_string()),
            home: true,
            rest_days: 2.0,
            minutes: None,
        };
        let star = predict_spot(&fitted, &[], &spot(1), "last_10", 25.5).unwrap();
        assert!(star.mean > 24.0 && star.mean < 34.0, "carry alone, mean {:.2}", star.mean);
        assert!((star.minutes - 32.0).abs() < 2.0, "minutes start at last season's 32, {:.1}", star.minutes);
        assert_eq!(star.prior_from.as_deref(), Some("2024-25 Regular Season"));
        assert!(star.clear_probability > 0.5);
        assert!((star.pmf.iter().sum::<f64>() - 1.0).abs() < 0.02);
        let bench = predict_spot(&fitted, &[], &spot(2), "last_10", 25.5).unwrap();
        assert!(bench.mean < 14.0, "a 10-point player stays near 10, mean {:.2}", bench.mean);
        let error = predict_spot(&fitted, &[], &spot(9), "last_10", 25.5).err().unwrap();
        let message = error.to_string();
        assert!(message.contains("no 2024-25 Regular Season minutes to carry"), "{message}");
        assert!(message.contains("He has 0 games"), "{message}");
    }

    #[test]
    fn a_playoffs_fit_seeds_from_the_same_regular_season() {
        let mut regular = last_season();
        for row in &mut regular {
            row.game_date = on_date(-2);
        }
        let playoffs: Vec<GameLog> = (0..2).map(|day| player_game(1, day * 2, "BOS", 32.0, 26)).collect();
        let source = Seed {
            games: &regular,
            season: "2025-26",
            season_type: "Regular Season",
        };
        let fitted = train_one(&playoffs, &spec(0.08), "2025-26", "Playoffs", Some(&source)).unwrap();
        assert_eq!(fitted.seeded_from.as_deref(), Some("2025-26 Regular Season"));
        assert_eq!(fitted.season_type, "Playoffs");
        assert!(fitted.parts[0].carry.contains_key(&1));
        // Too few playoff rows to fit a population, so the regular season's is borrowed.
        assert!(fitted.parts[0].teams.contains("NYK"));
    }

    #[test]
    fn a_seeded_players_minutes_start_from_last_season_and_fade_to_his_role() {
        let part = Population {
            stat: "pts".to_string(),
            prior_minutes: 80.0,
            opponent_minutes: 500.0,
            shift_prior: 0.1,
            role_minutes: vec![15.0, 28.0],
            role_rates: vec![0.3, 0.4, 0.5],
            role_minute_means: vec![10.0, 22.0, 32.0],
            role_log_sd: vec![0.3, 0.3, 0.3],
            opponents: BTreeMap::new(),
            teams: BTreeSet::new(),
            home_log: 0.0,
            rest_log: 0.0,
            league_rate: 0.4,
            carry: [(1, Carry { stat: 300.0, exposure: 600.0, minutes: 34.0 })].into_iter().collect(),
            carry_decay_tau: 0.0,
        };
        let center = |history: &[Obs], player: i64| {
            let plan = minutes_plan(Some(&part), history, None, player);
            plan.iter().map(|(minutes, weight)| minutes * weight).sum::<f64>()
                / plan.iter().map(|(_, weight)| weight).sum::<f64>()
        };
        // No games yet: the starter opens near his 34 minutes, the rookie at the bench role.
        assert!(center(&[], 1) > 28.0, "{}", center(&[], 1));
        assert!(center(&[], 2) < 14.0, "{}", center(&[], 2));
        // Thirty games of 20 minutes: the anchor has faded to the middle role, and he plays 20.
        let history: Vec<Obs> = (0..30)
            .map(|day| Obs {
                player_id: 1,
                date: on_date(day),
                opponent: "BOS".to_string(),
                home: true,
                rest_days: 1.0,
                minutes: 20.0,
                stat: 8.0,
            })
            .collect();
        let later = center(&history, 1);
        assert!((18.0..23.0).contains(&later), "{later}");
    }

    #[test]
    fn opponent_carry_shrinks_toward_one() {
        let league_rate = 0.5;
        let prior: BTreeMap<String, f64> = [("BOS".to_string(), 1.3)].into_iter().collect();
        let lambdas = HashMap::new();
        let factor = |carry_minutes: f64| {
            opponent_factors(&[], &lambdas, 0.0, 0.0, 500.0, league_rate, Some((&prior, carry_minutes)))["BOS"]
        };
        assert!((factor(0.0) - 1.0).abs() < 1e-12);
        assert!((factor(1500.0) - (500.0 + 1500.0 * 1.3) / 2000.0).abs() < 1e-12);
        assert!(factor(20000.0) > 1.28);
        // A team with no multiplier last season starts at 1.
        let none = opponent_factors(&[], &lambdas, 0.0, 0.0, 500.0, league_rate, Some((&prior, 1500.0)));
        assert!(!none.contains_key("NYK"));
        let part = Population {
            stat: "pts".to_string(),
            prior_minutes: 80.0,
            opponent_minutes: 500.0,
            shift_prior: 0.1,
            role_minutes: vec![15.0, 28.0],
            role_rates: vec![0.3, 0.4, 0.5],
            role_minute_means: vec![10.0, 22.0, 32.0],
            role_log_sd: vec![0.3, 0.3, 0.3],
            opponents: [("BOS".to_string(), 1.3)].into_iter().collect(),
            teams: BTreeSet::new(),
            home_log: 0.0,
            rest_log: 0.0,
            league_rate,
            carry: BTreeMap::new(),
            carry_decay_tau: 0.0,
        };
        let seed = SeedPart { population: part, carry: BTreeMap::new() };
        let mut settings = spec(0.08);
        settings.opponent_minutes = 500.0;
        let borrowed = borrowed_population(&seed, &settings, &BTreeSet::new());
        assert!((borrowed.opponents["BOS"] - factor(1500.0)).abs() < 1e-12);
        assert_eq!(borrowed.prior_minutes, 80.0);
    }

    #[test]
    fn home_games_score_higher_than_away_games() {
        let games: Vec<GameLog> = (0..40).map(|day| game(day, day % 2 == 0, if day % 2 == 0 { 25 } else { 15 })).collect();
        let fitted = train_one(&games, &spec(0.4), "2025-26", "Regular Season", None).unwrap();
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
        let fitted = train_one(&games, &spec(0.08), "2025-26", "Regular Season", None).unwrap();
        let mae = fitted.holdout_mae.expect("holdout");
        assert!(
            mae > 15.0,
            "leaking the 40-point games would shrink the error, mae {mae:.2}"
        );
    }

    #[test]
    fn a_saved_model_predicts_the_same_mean() {
        let games: Vec<GameLog> = (0..36).map(|day| game(day * 2, true, 18)).collect();
        let fitted = train_one(&games, &spec(0.08), "2025-26", "Regular Season", None).unwrap();
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
        assert_eq!(score.seeded_from, None);
        let before = last_season();
        let seeded =
            train_one(&games, &spec(0.08), "2025-26", "Regular Season", Some(&seed(&before))).unwrap();
        let original = predict_spot(&seeded, &games, &spot, "last_10", 15.0).unwrap();
        save_model(&seeded, "2026-10-07T18:00:00Z", &directory).unwrap();
        let loaded = load_model(&directory, "2025-26", "Regular Season", "points").unwrap();
        let again = predict_spot(&loaded, &games, &spot, "last_10", 15.0).unwrap();
        assert!((again.mean - original.mean).abs() < 1e-6, "{} {}", again.mean, original.mean);
        assert_eq!(loaded.parts[0].carry, seeded.parts[0].carry);
        let score = load_score(&directory, "2025-26", "Regular Season", "points").unwrap();
        assert_eq!(score.seeded_from.as_deref(), Some("2024-25 Regular Season"));
        assert_eq!(score.carry_minutes, Some(400.0));
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
        let fitted = train_one(&games, &spec(0.08), "2025-26", "Regular Season", None).unwrap();
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
        let fitted = train_one(&games, &spec(0.08), "2025-26", "Regular Season", None).unwrap();
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
        let regular = train_one(&games, &spec(0.08), "2025-26", "Regular Season", None).unwrap();
        let playoffs = train_one(&games, &spec(0.08), "2025-26", "Playoffs", None).unwrap();
        let older = train_one(&games, &spec(0.08), "2024-25", "Regular Season", None).unwrap();
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
        let playoffs = train_one(&games, &spec(0.08), "2024-25", "Playoffs", None).unwrap();
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
