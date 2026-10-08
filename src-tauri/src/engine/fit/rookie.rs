//! Rookie prior from draft position × minutes role.
//! Built from past rookie seasons; scored by `backtest-rookie`.

use std::collections::{BTreeMap, HashMap, HashSet};

use crate::error::AppResult;
use crate::models::{DraftPick, GameLog, RookieBucket, Stat};

use super::backtest::{lines_for, Score, BUCKETS, ALL_ROWS};
use super::{
    combine, fit_parts, observations, parts_of, role_index, seed_parts, ModelSpec, Obs,
    SeedPart, MIN_PRIOR,
};

/// One arm of the rookie-prior backtest.
#[derive(Debug, Clone, Copy)]
pub struct RookieArm {
    /// Pseudo-minutes on the draft×role rate. Zero = role prior only (today's baseline after 5 games).
    pub rookie_prior_minutes: f64,
    /// When true, price from game 1 using the rookie prior. When false, wait for MIN_PRIOR.
    pub from_game_one: bool,
}

#[derive(Debug, Clone, Default)]
struct Cell {
    stat: f64,
    minutes: f64,
    players: usize,
}

/// Rates and typical minutes by (bucket, role).
#[derive(Debug, Clone, Default)]
pub struct RookiePriorTable {
    /// role_count rates per bucket, flattened as bucket_index * role_count + role.
    rates: Vec<f64>,
    minute_means: Vec<f64>,
    role_count: usize,
    league_rate: f64,
}

impl RookiePriorTable {
    fn index(&self, bucket: RookieBucket, role: usize) -> usize {
        (bucket as usize) * self.role_count + role.min(self.role_count.saturating_sub(1))
    }

    pub fn rate(&self, bucket: RookieBucket, role: usize) -> f64 {
        self.rates
            .get(self.index(bucket, role))
            .copied()
            .unwrap_or(self.league_rate)
            .max(0.0)
    }

    pub fn minutes(&self, bucket: RookieBucket, role: usize) -> f64 {
        self.minute_means
            .get(self.index(bucket, role))
            .copied()
            .unwrap_or(18.0)
    }
}

const BUCKETS_ORDER: [RookieBucket; 5] = [
    RookieBucket::LotteryTop,
    RookieBucket::LotteryRest,
    RookieBucket::FirstRoundLate,
    RookieBucket::SecondRound,
    RookieBucket::Undrafted,
];

/// Past rookies = players in `season_games` with no minutes in `prior_games`, keyed by draft.
pub fn build_table(
    history: &[(Vec<GameLog>, i32)],
    drafts: &HashMap<i64, DraftPick>,
    role_cuts: &[f64],
    stat: Stat,
) -> RookiePriorTable {
    let role_count = role_cuts.len() + 1;
    let mut cells: Vec<Cell> = vec![Cell::default(); BUCKETS_ORDER.len() * role_count];
    let mut league_stat = 0.0;
    let mut league_minutes = 0.0;

    for (games, draft_year) in history {
        let part = parts_of(stat)[0];
        let rows = observations(games, part);
        let mut by_player: HashMap<i64, Vec<&Obs>> = HashMap::new();
        for row in &rows {
            by_player.entry(row.player_id).or_default().push(row);
        }
        // Players drafted in `draft_year` (or undrafted rookies: no draft that year, first season).
        for (player_id, player_rows) in &by_player {
            let pick = drafts.get(player_id);
            let bucket = match pick {
                Some(p) if p.draft_year == *draft_year => {
                    RookieBucket::from_overall(Some(p.overall_pick))
                }
                _ => continue,
            };
            let minutes: f64 = player_rows.iter().map(|r| r.minutes).sum();
            let games_n = player_rows.len();
            if games_n == 0 || minutes <= 0.0 {
                continue;
            }
            let avg = minutes / games_n as f64;
            let role = role_index(avg, role_cuts);
            let stat_sum: f64 = player_rows.iter().map(|r| r.stat).sum();
            let idx = (bucket as usize) * role_count + role;
            cells[idx].stat += stat_sum;
            cells[idx].minutes += minutes;
            cells[idx].players += 1;
            league_stat += stat_sum;
            league_minutes += minutes;
        }
    }

    let league_rate = if league_minutes > 0.0 {
        league_stat / league_minutes
    } else {
        0.4
    };
    let mut rates = Vec::with_capacity(cells.len());
    let mut minute_means = Vec::with_capacity(cells.len());
    for (i, cell) in cells.iter().enumerate() {
        let role = i % role_count;
        // Fall back to same-bucket all-roles, then league.
        let rate = if cell.minutes > 0.0 {
            cell.stat / cell.minutes
        } else {
            let bucket = i / role_count;
            let mut s = 0.0;
            let mut m = 0.0;
            for r in 0..role_count {
                let c = &cells[bucket * role_count + r];
                s += c.stat;
                m += c.minutes;
            }
            if m > 0.0 {
                s / m
            } else {
                league_rate
            }
        };
        rates.push(rate.max(0.0));
        let mean_minutes = if cell.players > 0 && cell.minutes > 0.0 {
            // Approximate: total minutes / (players * ~40 games) is rough; use role cut midpoints instead when empty.
            cell.minutes / cell.players.max(1) as f64 / 40.0
        } else {
            match role {
                0 => 10.0,
                1 => 22.0,
                _ => 32.0,
            }
        };
        minute_means.push(mean_minutes.clamp(4.0, 40.0));
        let _ = role;
    }

    RookiePriorTable {
        rates,
        minute_means,
        role_count,
        league_rate,
    }
}

/// Apply a draft×role prior onto a population for one player as synthetic carry.
pub fn synthetic_carry(
    table: &RookiePriorTable,
    bucket: RookieBucket,
    role: usize,
    rookie_prior_minutes: f64,
) -> super::Carry {
    let rate = table.rate(bucket, role);
    let minutes = table.minutes(bucket, role);
    super::Carry {
        stat: rookie_prior_minutes * rate,
        exposure: rookie_prior_minutes,
        minutes,
    }
}

/// Scores rookies' first `last_game` player-games.
/// `history` is prior seasons with their draft years for building the table.
/// `target` is the season under test; `seed` is last season (for role population).
/// `drafts` maps player_id → pick; rookies are those drafted in `target_draft_year`.
pub fn run(
    target: &[GameLog],
    seed: &[GameLog],
    history: &[(Vec<GameLog>, i32)],
    drafts: &HashMap<i64, DraftPick>,
    target_draft_year: i32,
    stat: Stat,
    spec: &ModelSpec,
    arms: &[RookieArm],
    last_player_game: usize,
) -> AppResult<BTreeMap<(usize, usize), Score>> {
    let table = build_table(history, drafts, &spec.role_minutes, stat);
    let rookies: HashSet<i64> = drafts
        .values()
        .filter(|p| p.draft_year == target_draft_year)
        .map(|p| p.player_id)
        .collect();
    let pick_of: HashMap<i64, &DraftPick> = drafts
        .values()
        .filter(|p| p.draft_year == target_draft_year)
        .map(|p| (p.player_id, p))
        .collect();

    let histories: Vec<_> = parts_of(stat).iter().map(|part| observations(target, *part)).collect();
    let primary = &histories[0];
    let base = seed_parts(seed, stat, spec)?;
    let seed_rows: Vec<_> = parts_of(stat).iter().map(|part| observations(seed, *part)).collect();
    // Carry off for the population seed; we inject rookie carry per arm.
    let seed_parts_zero: Vec<Option<SeedPart>> = base
        .iter()
        .zip(seed_rows.iter())
        .map(|(part, rows)| {
            part.as_ref().map(|part| SeedPart {
                population: part.population.clone(),
                carry: super::build_carry(&part.population, rows, 0.0),
            })
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
        .filter(|row| rookies.contains(&row.player_id))
        .map(|row| row.date.as_str())
        .collect::<HashSet<_>>()
        .into_iter()
        .collect();
    dates.sort();

    let lines = lines_for(stat);
    let mut scores: BTreeMap<(usize, usize), Score> = BTreeMap::new();

    for date in dates {
        let today: Vec<usize> = (0..primary.len())
            .filter(|&index| {
                let row = &primary[index];
                row.date == date
                    && rookies.contains(&row.player_id)
                    && nth_of[index] < last_player_game
            })
            .collect();
        if today.is_empty() {
            continue;
        }
        for (arm_index, arm) in arms.iter().enumerate() {
            let mut settings = spec.clone();
            settings.carry_minutes = 0.0;
            let mut parts = fit_parts(&histories, stat, &settings, &seed_parts_zero, Some(date))?;
            // Inject synthetic carries for rookies onto each part.
            if arm.rookie_prior_minutes > 0.0 {
                for part in &mut parts {
                    for &player_id in &rookies {
                        let pick = match pick_of.get(&player_id) {
                            Some(p) => p,
                            None => continue,
                        };
                        let bucket = RookieBucket::from_overall(Some(pick.overall_pick));
                        // Role from minutes so far before this date, else middle role.
                        let past_minutes: Vec<f64> = primary
                            .iter()
                            .filter(|r| r.player_id == player_id && r.date.as_str() < date)
                            .map(|r| r.minutes)
                            .collect();
                        let role = if past_minutes.is_empty() {
                            // Use draft-bucket typical starter/bench: lottery → higher role.
                            match bucket {
                                RookieBucket::LotteryTop | RookieBucket::LotteryRest => {
                                    part.role_minutes.len()
                                }
                                RookieBucket::FirstRoundLate => 1.min(part.role_minutes.len()),
                                _ => 0,
                            }
                        } else {
                            let avg = past_minutes.iter().sum::<f64>() / past_minutes.len() as f64;
                            role_index(avg, &part.role_minutes)
                        };
                        let carry = synthetic_carry(&table, bucket, role, arm.rookie_prior_minutes);
                        part.carry.insert(player_id, carry);
                    }
                }
            }
            for &index in &today {
                let row = &primary[index];
                let nth = nth_of[index];
                if !arm.from_game_one && nth < MIN_PRIOR {
                    continue;
                }
                let indexes = &by_player[&row.player_id];
                let past: Vec<Vec<_>> = histories
                    .iter()
                    .map(|rows| indexes.iter().take(nth).map(|&past| rows[past].clone()).collect())
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
                let actual: f64 = histories.iter().map(|rows| rows[index].stat).sum();
                let covered = nth >= MIN_PRIOR;
                // Bucket by player-game number (1-indexed).
                let player_game = nth + 1;
                if let Some(bucket) =
                    BUCKETS.iter().position(|(low, high)| (*low..=*high).contains(&player_game))
                {
                    scores
                        .entry((arm_index, bucket))
                        .or_default()
                        .add(&forecast.pmf, actual, &lines, covered);
                }
                scores
                    .entry((arm_index, ALL_ROWS))
                    .or_default()
                    .add(&forecast.pmf, actual, &lines, covered);
            }
        }
    }
    Ok(scores)
}

pub fn report(stat: Stat, arms: &[RookieArm], scores: &BTreeMap<(usize, usize), Score>) -> String {
    let mut text = format!(
        "### {} rookies

| prior | from_g1 | LL g1-3 | LL g4-6 | LL g7-10 | LL all | rows | rows@5+ |
|---|---|---:|---:|---:|---:|---:|---:|
",
        stat.label()
    );
    for (arm_index, arm) in arms.iter().enumerate() {
        let cell = |bucket: usize| scores.get(&(arm_index, bucket)).cloned().unwrap_or_default();
        let all = cell(ALL_ROWS);
        let label = if arm.rookie_prior_minutes <= 0.0 {
            "role".to_string()
        } else {
            format!("{:.0}", arm.rookie_prior_minutes)
        };
        text.push_str(&format!(
            "| {} | {} | {:.4} | {:.4} | {:.4} | {:.4} | {} | {} |
",
            label,
            arm.from_game_one,
            cell(0).mean_log_loss(),
            cell(1).mean_log_loss(),
            cell(2).mean_log_loss(),
            all.mean_log_loss(),
            all.rows,
            all.covered_today,
        ));
    }
    text
}


#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::RookieBucket;

    #[test]
    fn table_rates_follow_draft_bucket() {
        // Minimal synthetic: one lottery player at starter minutes scoring a lot.
        let mut games = Vec::new();
        for day in 0..40 {
            games.push(GameLog {
                player_id: 1,
                player_name: "Star".into(),
                team_abbr: "BOS".into(),
                game_id: format!("g{day}"),
                game_date: format!("2023-11-{:02}", (day % 28) + 1),
                matchup: "BOS vs. NYK".into(),
                wl: "W".into(),
                minutes: 32.0,
                pts: 20,
                reb: 5,
                ast: 5,
                stl: 1,
                blk: 0,
                tov: 2,
                fgm: 8,
                fga: 15,
                fg3m: 2,
                ftm: 2,
                plus_minus: 0,
            });
        }
        let mut drafts = HashMap::new();
        drafts.insert(
            1,
            DraftPick {
                player_id: 1,
                name: "Star".into(),
                draft_year: 2023,
                round: 1,
                overall_pick: 2,
            },
        );
        let table = build_table(&[(games, 2023)], &drafts, &[15.0, 28.0], Stat::Points);
        let rate = table.rate(RookieBucket::LotteryTop, 2);
        assert!(rate > 0.4, "lottery starter rate {rate}");
    }
}
