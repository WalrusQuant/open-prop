//! Scores the opening weeks of a season with and without last season carried over.
//! Every prediction uses only games before its date. The bin is `backtest-prior`.

use std::collections::{BTreeMap, HashMap, HashSet};

use crate::error::AppResult;
use crate::models::{GameLog, Stat};

use super::{at_least, build_carry, combine, fit_parts, observations, parts_of, seed_parts, ModelSpec, SeedPart};

/// One way to fit: the player carry and the opponent carry, in pseudo-minutes.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Arm {
    pub carry_minutes: f64,
    pub opponent_carry_minutes: f64,
}

/// Team game buckets the report splits on.
pub const BUCKETS: [(usize, usize); 3] = [(1, 3), (4, 6), (7, 10)];
/// Key of every row. The bucket after it holds the first game of the season of each player
/// who played in the seed season, the game a carry prices on its own.
pub const ALL_ROWS: usize = BUCKETS.len();
pub const FIRST_GAME: usize = BUCKETS.len() + 1;

#[derive(Debug, Clone, Default)]
pub struct Score {
    pub rows: usize,
    pub log_loss: f64,
    pub crps: f64,
    pub brier: f64,
    pub lines: usize,
    /// Rows a fit without carry could price today: five games before this one.
    pub covered_today: usize,
    covered_log_loss: f64,
    calibration: Vec<(f64, f64)>,
}

impl Score {
    fn add(&mut self, pmf: &[f64], actual: f64, lines: &[f64], covered: bool) {
        let index = actual.round().max(0.0) as usize;
        let mass = pmf.get(index).copied().unwrap_or(0.0);
        let loss = -mass.max(1e-12).ln();
        self.log_loss += loss;
        self.crps += crps(pmf, index);
        for line in lines {
            let probability = at_least(pmf, *line);
            let hit = if actual >= *line { 1.0 } else { 0.0 };
            self.brier += (probability - hit).powi(2);
            self.calibration.push((probability, hit));
            self.lines += 1;
        }
        self.rows += 1;
        if covered {
            self.covered_today += 1;
            self.covered_log_loss += loss;
        }
    }

    pub fn mean_log_loss(&self) -> f64 {
        self.log_loss / self.rows.max(1) as f64
    }

    /// Log loss over the rows the app could already price without carry.
    pub fn covered_log_loss(&self) -> f64 {
        self.covered_log_loss / self.covered_today.max(1) as f64
    }

    pub fn mean_crps(&self) -> f64 {
        self.crps / self.rows.max(1) as f64
    }

    pub fn mean_brier(&self) -> f64 {
        self.brier / self.lines.max(1) as f64
    }

    /// Expected calibration error of P(stat ≥ line) over ten probability bins.
    pub fn calibration_error(&self) -> f64 {
        if self.calibration.is_empty() {
            return 0.0;
        }
        let mut bins = [(0.0, 0.0, 0usize); 10];
        for (probability, hit) in &self.calibration {
            let bin = ((probability * 10.0) as usize).min(9);
            bins[bin].0 += probability;
            bins[bin].1 += hit;
            bins[bin].2 += 1;
        }
        let total = self.calibration.len() as f64;
        bins.iter()
            .filter(|bin| bin.2 > 0)
            .map(|(predicted, seen, _)| (predicted - seen).abs() / total)
            .sum()
    }
}

/// Discrete CRPS: the squared gap between the predictive CDF and the step at the actual count.
fn crps(pmf: &[f64], actual: usize) -> f64 {
    let mut cdf = 0.0;
    let mut total = 0.0;
    for k in 0..pmf.len().max(actual + 1) {
        cdf += pmf.get(k).copied().unwrap_or(0.0);
        let step = if actual <= k { 1.0 } else { 0.0 };
        total += (cdf.min(1.0) - step).powi(2);
    }
    total
}

/// Prop lines the calibration and Brier columns use.
pub fn lines_for(stat: Stat) -> Vec<f64> {
    match stat {
        Stat::Points => vec![10.5, 15.5, 20.5, 25.5],
        Stat::Rebounds => vec![4.5, 6.5, 8.5],
        Stat::Assists => vec![2.5, 4.5, 6.5],
        Stat::ThreePointersMade => vec![1.5, 2.5],
        Stat::PointsAssistsRebounds => vec![20.5, 30.5],
        _ => Vec::new(),
    }
}

/// Each player-game's team game number, counted from the team's first game of the season.
fn team_game_numbers(games: &[GameLog]) -> HashMap<(i64, String), usize> {
    let mut team_dates: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for game in games {
        team_dates.entry(game.team_abbr.clone()).or_default().push(game.game_date.clone());
    }
    let mut numbers: HashMap<(String, String), usize> = HashMap::new();
    for (team, mut dates) in team_dates {
        dates.sort();
        dates.dedup();
        for (nth, date) in dates.into_iter().enumerate() {
            numbers.insert((team.clone(), date), nth + 1);
        }
    }
    games
        .iter()
        .filter_map(|game| {
            numbers
                .get(&(game.team_abbr.clone(), game.game_date.clone()))
                .map(|number| ((game.player_id, game.game_date.clone()), *number))
        })
        .collect()
}

/// Scores every player-game through team game `last_game` for each arm. The key is
/// (arm index, bucket index); `ALL_ROWS` holds every row and `FIRST_GAME` the first game of
/// each player with seed-season minutes, priced from the prior alone.
pub fn run(
    games: &[GameLog],
    seed: &[GameLog],
    stat: Stat,
    spec: &ModelSpec,
    arms: &[Arm],
    last_game: usize,
) -> AppResult<BTreeMap<(usize, usize), Score>> {
    let numbers = team_game_numbers(games);
    let histories: Vec<_> = parts_of(stat).iter().map(|part| observations(games, *part)).collect();
    let primary = &histories[0];
    // Last season's populations fit once. Only the carry depends on the arm.
    let base = seed_parts(seed, stat, spec)?;
    let seed_rows: Vec<_> = parts_of(stat).iter().map(|part| observations(seed, *part)).collect();
    let carried: HashSet<i64> = seed_rows[0].iter().map(|row| row.player_id).collect();
    let seeds: Vec<Vec<Option<SeedPart>>> = arms
        .iter()
        .map(|arm| {
            base.iter()
                .zip(seed_rows.iter())
                .map(|(part, rows)| {
                    part.as_ref().map(|part| SeedPart {
                        population: part.population.clone(),
                        carry: build_carry(&part.population, rows, arm.carry_minutes),
                    })
                })
                .collect()
        })
        .collect();
    let mut nth_of: Vec<usize> = Vec::with_capacity(primary.len());
    let mut seen: HashMap<i64, usize> = HashMap::new();
    let mut by_player: HashMap<i64, Vec<usize>> = HashMap::new();
    for (index, row) in primary.iter().enumerate() {
        let count = seen.entry(row.player_id).or_insert(0);
        nth_of.push(*count);
        *count += 1;
        by_player.entry(row.player_id).or_default().push(index);
    }
    let mut dates: Vec<&str> = primary
        .iter()
        .filter(|row| numbers.get(&(row.player_id, row.date.clone())).is_some_and(|n| *n <= last_game))
        .map(|row| row.date.as_str())
        .collect::<HashSet<_>>()
        .into_iter()
        .collect();
    dates.sort();
    let lines = lines_for(stat);
    let mut scores: BTreeMap<(usize, usize), Score> = BTreeMap::new();
    for date in dates {
        let today: Vec<usize> = (0..primary.len())
            .filter(|index| primary[*index].date == date)
            .filter(|index| {
                numbers
                    .get(&(primary[*index].player_id, date.to_string()))
                    .is_some_and(|n| *n <= last_game)
            })
            .collect();
        for (arm_index, arm) in arms.iter().enumerate() {
            let mut settings = spec.clone();
            settings.carry_minutes = arm.carry_minutes;
            settings.opponent_carry_minutes = arm.opponent_carry_minutes;
            let parts = fit_parts(&histories, stat, &settings, &seeds[arm_index], Some(date))?;
            for index in &today {
                let row = &primary[*index];
                let nth = nth_of[*index];
                let indexes = &by_player[&row.player_id];
                let past: Vec<Vec<_>> = histories
                    .iter()
                    .map(|rows| indexes.iter().take(nth).map(|past| rows[*past].clone()).collect())
                    .collect();
                let forecast = combine(
                    &parts,
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
                let number = numbers[&(row.player_id, row.date.clone())];
                let covered = nth >= super::MIN_PRIOR;
                if let Some(bucket) = BUCKETS.iter().position(|(low, high)| (*low..=*high).contains(&number)) {
                    scores.entry((arm_index, bucket)).or_default().add(&forecast.pmf, actual, &lines, covered);
                }
                scores
                    .entry((arm_index, ALL_ROWS))
                    .or_default()
                    .add(&forecast.pmf, actual, &lines, covered);
                if nth == 0 && carried.contains(&row.player_id) {
                    scores
                        .entry((arm_index, FIRST_GAME))
                        .or_default()
                        .add(&forecast.pmf, actual, &lines, covered);
                }
            }
        }
    }
    Ok(scores)
}

/// A markdown table per stat: log loss by bucket, then CRPS, Brier, and calibration over all rows,
/// then the first game of each player with seed-season minutes (`p-g1`) on its own.
pub fn report(stat: Stat, arms: &[Arm], scores: &BTreeMap<(usize, usize), Score>) -> String {
    let mut text = format!(
        "### {}\n\n| carry | opp carry | LL g1-3 | LL g4-6 | LL g7-10 | LL all | LL today rows | CRPS | Brier | ECE | rows | today | LL p-g1 | CRPS p-g1 | Brier p-g1 | ECE p-g1 | p-g1 rows |\n|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|\n",
        stat.label()
    );
    for (arm_index, arm) in arms.iter().enumerate() {
        let cell = |bucket: usize| scores.get(&(arm_index, bucket)).cloned().unwrap_or_default();
        let all = cell(ALL_ROWS);
        let first = cell(FIRST_GAME);
        text.push_str(&format!(
            "| {:.0} | {:.0} | {:.4} | {:.4} | {:.4} | {:.4} | {:.4} | {:.4} | {:.4} | {:.4} | {} | {:.0}% | {:.4} | {:.4} | {:.4} | {:.4} | {} |\n",
            arm.carry_minutes,
            arm.opponent_carry_minutes,
            cell(0).mean_log_loss(),
            cell(1).mean_log_loss(),
            cell(2).mean_log_loss(),
            all.mean_log_loss(),
            all.covered_log_loss(),
            all.mean_crps(),
            all.mean_brier(),
            all.calibration_error(),
            all.rows,
            100.0 * all.covered_today as f64 / all.rows.max(1) as f64,
            first.mean_log_loss(),
            first.mean_crps(),
            first.mean_brier(),
            first.calibration_error(),
            first.rows,
        ));
    }
    text
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crps_is_zero_for_a_sure_hit_and_grows_with_the_miss() {
        assert!(crps(&[0.0, 0.0, 1.0], 2).abs() < 1e-12);
        assert!((crps(&[0.0, 0.0, 1.0], 0) - 2.0).abs() < 1e-12);
        assert!((crps(&[1.0], 3) - 3.0).abs() < 1e-12);
    }

    #[test]
    fn calibration_error_is_the_gap_between_forecast_and_hit_rate() {
        let mut score = Score::default();
        for hit in [1.0, 0.0, 1.0, 0.0] {
            score.calibration.push((0.55, hit));
        }
        assert!((score.calibration_error() - 0.05).abs() < 1e-12);
    }
}
