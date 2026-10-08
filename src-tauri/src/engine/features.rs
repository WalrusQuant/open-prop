use std::collections::{HashMap, HashSet};

use chrono::{Datelike, NaiveDate};

use crate::error::{AppError, AppResult};
use crate::models::{GameLog, Stat};
use crate::stats::split_matchup;

use super::fit::Spot;

/// A row exists only after this many earlier games for the same player.
pub const MIN_PRIOR: usize = 5;

/// Rest longer than two weeks is the same feature value. The flag for a back-to-back stays separate.
const REST_CAP: i64 = 14;

/// Column order. A spec names a subset. Unknown names are rejected.
pub const FEATURES: [&str; 13] = [
    "minutes_l10",
    "minutes_season",
    "rate_l5",
    "rate_l10",
    "rate_season",
    "stat_l10",
    "fga_per_min_l10",
    "home",
    "rest_days",
    "back_to_back",
    "opp_allow_season",
    "opp_allow_l15",
    "prior_games",
];

pub fn feature_index(name: &str) -> Option<usize> {
    FEATURES.iter().position(|item| *item == name)
}

#[derive(Debug, Clone)]
pub struct BuiltRow {
    pub player_id: i64,
    pub game_id: String,
    #[allow(dead_code)]
    pub game_date: String,
    pub target: f64,
    values: [f64; FEATURES.len()],
}

impl BuiltRow {
    pub fn feature(&self, name: &str) -> AppResult<f64> {
        let index = feature_index(name).ok_or_else(|| {
            AppError::message(format!("'{name}' is not a feature the model knows."))
        })?;
        Ok(self.values[index])
    }
}

#[derive(Default)]
struct PlayerHist {
    minutes: Vec<f64>,
    stat: Vec<f64>,
    fga: Vec<f64>,
    last_day: Option<i64>,
}

struct Closing {
    players: HashMap<i64, PlayerHist>,
    /// Per-game means of this stat posted against the team, oldest first.
    opponents: HashMap<String, Vec<f64>>,
    teams: HashSet<String>,
    league_mean: f64,
}

pub fn build_rows(games: &[GameLog], stat: Stat) -> Vec<BuiltRow> {
    walk(games, stat, true).0
}

pub fn features_for_spot(games: &[GameLog], stat: Stat, spot: &Spot) -> AppResult<BuiltRow> {
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
    let closing = walk(games, stat, false).1;
    let hist = closing.players.get(&spot.player_id).ok_or_else(|| {
        AppError::message(
            "No cached games for that player. Sync this season, then try the name again."
                .to_string(),
        )
    })?;
    if hist.stat.len() < MIN_PRIOR {
        return Err(AppError::message(format!(
            "This player has {} cached games. A prediction starts after 5.",
            hist.stat.len()
        )));
    }
    let (opp_season, opp_l15) = spot_allowance(&closing, spot.opponent.as_deref())?;
    let rest = spot.rest_days.min(REST_CAP as f64);
    let back_to_back = if rest == 0.0 { 1.0 } else { 0.0 };
    let values = fill(
        hist,
        if spot.home { 1.0 } else { 0.0 },
        rest,
        back_to_back,
        opp_season,
        opp_l15,
        spot.minutes,
    );
    Ok(BuiltRow {
        player_id: spot.player_id,
        game_id: String::new(),
        game_date: String::new(),
        target: 0.0,
        values,
    })
}

fn spot_allowance(closing: &Closing, opponent: Option<&str>) -> AppResult<(f64, f64)> {
    let Some(raw) = opponent.map(str::trim).filter(|value| !value.is_empty()) else {
        return Ok((closing.league_mean, closing.league_mean));
    };
    let team = team_key(raw);
    if !closing.teams.contains(&team) {
        return Err(AppError::message(format!(
            "No cached games for opponent {team}."
        )));
    }
    Ok(allowance(&closing.opponents, closing.league_mean, &team))
}

/// One pass in date order.
///
/// Rows for a game are emitted before that game updates anyone's history or the
/// opponent's allowance. A game cannot see itself, and it cannot see the other
/// box scores from the same tip.
fn walk(games: &[GameLog], stat: Stat, collect: bool) -> (Vec<BuiltRow>, Closing) {
    let ordered = ordered(games);
    let mut rows = Vec::new();
    let mut players: HashMap<i64, PlayerHist> = HashMap::new();
    let mut opponents: HashMap<String, Vec<f64>> = HashMap::new();
    let mut teams = HashSet::new();
    let mut league_sum = 0.0;
    let mut league_count = 0u32;

    let mut index = 0;
    while index < ordered.len() {
        let start = index;
        let game_id = ordered[index].game_id.as_str();
        index += 1;
        while index < ordered.len() && ordered[index].game_id == game_id {
            index += 1;
        }
        let group = &ordered[start..index];
        let league = if league_count == 0 {
            0.0
        } else {
            league_sum / f64::from(league_count)
        };

        if collect {
            for game in group {
                let Some(hist) = players.get(&game.player_id) else {
                    continue;
                };
                if hist.stat.len() < MIN_PRIOR {
                    continue;
                }
                rows.push(emit(game, hist, &opponents, league, stat));
            }
        }

        // Allowance is the mean of player totals posted against the team in this game.
        // Both sides of the box score produce one number, then later games may use them.
        let mut against: HashMap<String, (f64, u32)> = HashMap::new();
        for game in group {
            teams.insert(team_key(&game.team_abbr));
            let (_, opponent) = split_matchup(&game.matchup);
            let opponent = team_key(&opponent);
            if opponent.is_empty() {
                continue;
            }
            teams.insert(opponent.clone());
            let slot = against.entry(opponent).or_insert((0.0, 0));
            slot.0 += stat.value(game);
            slot.1 += 1;
        }
        for (team, (sum, count)) in against {
            if count == 0 {
                continue;
            }
            let mean = sum / f64::from(count);
            opponents.entry(team).or_default().push(mean);
            league_sum += mean;
            league_count += 1;
        }

        for game in group {
            let hist = players.entry(game.player_id).or_default();
            hist.minutes.push(game.minutes);
            hist.stat.push(stat.value(game));
            hist.fga.push(f64::from(game.fga));
            if let Some(day) = day_index(&game.game_date) {
                hist.last_day = Some(day);
            }
        }
    }

    let league_mean = if league_count == 0 {
        0.0
    } else {
        league_sum / f64::from(league_count)
    };
    (
        rows,
        Closing {
            players,
            opponents,
            teams,
            league_mean,
        },
    )
}

fn emit(
    game: &GameLog,
    hist: &PlayerHist,
    opponents: &HashMap<String, Vec<f64>>,
    league: f64,
    stat: Stat,
) -> BuiltRow {
    let (_, opponent) = split_matchup(&game.matchup);
    let (opp_season, opp_l15) = allowance(opponents, league, &team_key(&opponent));
    let (location, _) = split_matchup(&game.matchup);
    let (rest_days, back_to_back) = rest_features(hist, day_index(&game.game_date));
    BuiltRow {
        player_id: game.player_id,
        game_id: game.game_id.clone(),
        game_date: game.game_date.clone(),
        target: stat.value(game),
        values: fill(
            hist,
            if location == "home" { 1.0 } else { 0.0 },
            rest_days,
            back_to_back,
            opp_season,
            opp_l15,
            None,
        ),
    }
}

fn fill(
    hist: &PlayerHist,
    home: f64,
    rest_days: f64,
    back_to_back: f64,
    opp_season: f64,
    opp_l15: f64,
    minutes_l10: Option<f64>,
) -> [f64; FEATURES.len()] {
    let count = hist.stat.len();
    [
        minutes_l10.unwrap_or_else(|| tail_mean(&hist.minutes, 10)),
        tail_mean(&hist.minutes, count),
        rate(&hist.stat, &hist.minutes, 5),
        rate(&hist.stat, &hist.minutes, 10),
        rate(&hist.stat, &hist.minutes, count),
        tail_mean(&hist.stat, 10),
        rate(&hist.fga, &hist.minutes, 10),
        home,
        rest_days,
        back_to_back,
        opp_season,
        opp_l15,
        count as f64,
    ]
}

fn allowance(opponents: &HashMap<String, Vec<f64>>, league: f64, team: &str) -> (f64, f64) {
    match opponents.get(team) {
        Some(values) if !values.is_empty() => (tail_mean(values, values.len()), tail_mean(values, 15)),
        _ => (league, league),
    }
}

fn rest_features(hist: &PlayerHist, day: Option<i64>) -> (f64, f64) {
    match (hist.last_day, day) {
        (Some(previous), Some(day)) => {
            let gap = (day - previous - 1).clamp(0, REST_CAP);
            let back_to_back = if gap == 0 { 1.0 } else { 0.0 };
            (gap as f64, back_to_back)
        }
        _ => (REST_CAP as f64, 0.0),
    }
}

fn ordered(games: &[GameLog]) -> Vec<&GameLog> {
    let mut refs: Vec<&GameLog> = games.iter().collect();
    refs.sort_by(|left, right| {
        left.game_date
            .cmp(&right.game_date)
            .then(left.game_id.cmp(&right.game_id))
            .then(left.player_id.cmp(&right.player_id))
    });
    refs
}

fn team_key(value: &str) -> String {
    value.trim().to_uppercase()
}

fn day_index(iso: &str) -> Option<i64> {
    NaiveDate::parse_from_str(iso, "%Y-%m-%d")
        .ok()
        .map(|date| i64::from(date.num_days_from_ce()))
}

fn tail_mean(values: &[f64], window: usize) -> f64 {
    if values.is_empty() || window == 0 {
        return 0.0;
    }
    let start = values.len().saturating_sub(window);
    let slice = &values[start..];
    slice.iter().sum::<f64>() / slice.len() as f64
}

fn rate(stat: &[f64], minutes: &[f64], window: usize) -> f64 {
    if stat.is_empty() || window == 0 {
        return 0.0;
    }
    let start = stat.len().saturating_sub(window);
    let minutes_sum = minutes[start..].iter().sum::<f64>();
    if minutes_sum <= 0.0 {
        0.0
    } else {
        stat[start..].iter().sum::<f64>() / minutes_sum
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn on_date(offset: i64) -> String {
        NaiveDate::from_ymd_opt(2025, 10, 22)
            .unwrap()
            .checked_add_signed(chrono::Duration::days(offset))
            .unwrap()
            .format("%Y-%m-%d")
            .to_string()
    }

    fn log(
        player: i64,
        team: &str,
        opponent: &str,
        home: bool,
        day: i64,
        pts: i32,
        game_id: &str,
    ) -> GameLog {
        GameLog {
            player_id: player,
            player_name: format!("P{player}"),
            team_abbr: team.to_string(),
            game_id: game_id.to_string(),
            game_date: on_date(day),
            matchup: if home {
                format!("{team} vs. {opponent}")
            } else {
                format!("{team} @ {opponent}")
            },
            wl: "W".to_string(),
            minutes: 30.0,
            pts,
            reb: 0,
            ast: 0,
            stl: 0,
            blk: 0,
            tov: 0,
            fgm: 0,
            fga: 10,
            fg3m: 0,
            ftm: 0,
            plus_minus: 0,
        }
    }

    #[test]
    fn a_game_never_sees_itself() {
        let mut games = vec![
            // This game is listed first on purpose. Date order, not input order, decides history.
            log(1, "DAL", "CHI", false, 6, 99, "late"),
            log(7, "NYK", "CHI", false, 3, 40, "against-chi"),
            log(8, "NYK", "CHI", false, 3, 20, "against-chi"),
        ];
        for day in 0..5 {
            games.push(log(1, "DAL", "BOS", true, day, 10, &format!("early-{day}")));
        }
        games.push(log(1, "DAL", "CHI", false, 7, 12, "after"));

        let rows = build_rows(&games, Stat::Points);
        let ids: Vec<&str> = rows.iter().map(|row| row.game_id.as_str()).collect();
        assert!(
            !ids.contains(&"early-4"),
            "the fifth game has four priors and is not a row"
        );
        assert!(ids.contains(&"late"), "the sixth game is the first row");

        let late = rows.iter().find(|row| row.game_id == "late").unwrap();
        assert_eq!(late.player_id, 1);
        assert!((late.target - 99.0).abs() < 1e-9);
        assert_eq!(late.feature("prior_games").unwrap(), 5.0);
        // Last-10 mean of the five 10-point games. 99 is the target, not an input.
        assert!((late.feature("stat_l10").unwrap() - 10.0).abs() < 1e-9);
        // CHI had already allowed a 30-point player average. This 99 is not in that number.
        assert!((late.feature("opp_allow_season").unwrap() - 30.0).abs() < 1e-9);
        assert!((late.feature("opp_allow_l15").unwrap() - 30.0).abs() < 1e-9);
        assert_eq!(late.feature("home").unwrap(), 0.0);

        let after = rows.iter().find(|row| row.game_id == "after").unwrap();
        // The previous game against CHI now counts: (30 + 99) / 2.
        assert!((after.feature("opp_allow_season").unwrap() - 64.5).abs() < 1e-9);
        assert!((after.feature("stat_l10").unwrap() - (50.0 + 99.0) / 6.0).abs() < 1e-9);
    }

    #[test]
    fn minutes_override_does_not_touch_the_rate() {
        let mut games = Vec::new();
        for day in 0..6 {
            games.push(log(1, "DAL", "BOS", true, day, 10, &format!("g{day}")));
        }
        let spot = Spot {
            player_id: 1,
            opponent: None,
            home: false,
            rest_days: 2.0,
            minutes: Some(40.0),
        };
        let row = features_for_spot(&games, Stat::Points, &spot).unwrap();
        assert!((row.feature("minutes_l10").unwrap() - 40.0).abs() < 1e-9);
        assert!((row.feature("stat_l10").unwrap() - 10.0).abs() < 1e-9);
        assert!((row.feature("rate_l10").unwrap() - 10.0 / 30.0).abs() < 1e-9);
        assert_eq!(row.feature("home").unwrap(), 0.0);
        assert_eq!(row.feature("rest_days").unwrap(), 2.0);
        assert_eq!(row.feature("back_to_back").unwrap(), 0.0);
    }

    #[test]
    fn a_blank_opponent_uses_the_league_allowance() {
        let mut games = vec![
            log(7, "NYK", "CHI", false, 0, 40, "against-chi"),
            log(8, "NYK", "CHI", false, 0, 20, "against-chi"),
        ];
        for day in 1..7 {
            games.push(log(1, "DAL", "BOS", true, day, 10, &format!("g{day}")));
        }
        let row = features_for_spot(
            &games,
            Stat::Points,
            &Spot {
                player_id: 1,
                opponent: Some("  ".to_string()),
                home: true,
                rest_days: 1.0,
                minutes: None,
            },
        )
        .unwrap();
        // DAL's six games each put a 10-point mean on BOS, and NYK put a 30-point mean on CHI.
        let league = (10.0 * 6.0 + 30.0) / 7.0;
        assert!((row.feature("opp_allow_season").unwrap() - league).abs() < 1e-9);
        assert!((row.feature("opp_allow_l15").unwrap() - league).abs() < 1e-9);
        assert!((row.feature("minutes_l10").unwrap() - 30.0).abs() < 1e-9);
    }

    #[test]
    fn five_games_is_not_enough_to_predict() {
        let games: Vec<GameLog> = (0..4)
            .map(|day| log(1, "DAL", "BOS", true, day, 10, &format!("g{day}")))
            .collect();
        let error = features_for_spot(
            &games,
            Stat::Points,
            &Spot {
                player_id: 1,
                opponent: None,
                home: true,
                rest_days: 1.0,
                minutes: None,
            },
        )
        .unwrap_err();
        assert!(error.to_string().contains("after 5"));
    }
}
