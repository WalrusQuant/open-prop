use crate::models::{
    BoardRow, BoardSplit, GameLog, Stat, TrendGame, TrendReport, TrendSummary, Window,
};

/// A game counts as an over when the stat is greater than or equal to the line.
pub fn is_over(stat: f64, line: f64) -> bool {
    stat >= line
}

/// Trailing three-game average, including the current game.
/// The first two games in the window have no average.
pub fn moving_average(values: &[f64], width: usize) -> Vec<Option<f64>> {
    values
        .iter()
        .enumerate()
        .map(|(index, _)| {
            if width == 0 || index + 1 < width {
                return None;
            }
            let start = index + 1 - width;
            let sum: f64 = values[start..=index].iter().sum();
            Some(sum / width as f64)
        })
        .collect()
}

pub fn mean(values: &[f64]) -> Option<f64> {
    if values.is_empty() {
        None
    } else {
        Some(values.iter().sum::<f64>() / values.len() as f64)
    }
}

pub fn median(values: &[f64]) -> Option<f64> {
    if values.is_empty() {
        return None;
    }
    let mut sorted = values.to_vec();
    sorted.sort_by(|left, right| left.partial_cmp(right).unwrap_or(std::cmp::Ordering::Equal));
    let mid = sorted.len() / 2;
    if sorted.len() % 2 == 0 {
        Some((sorted[mid - 1] + sorted[mid]) / 2.0)
    } else {
        Some(sorted[mid])
    }
}

/// Sample standard deviation. One game has no deviation to report.
pub fn sample_sd(values: &[f64]) -> Option<f64> {
    if values.len() < 2 {
        return None;
    }
    let center = mean(values)?;
    let square_sum = values
        .iter()
        .map(|value| {
            let delta = value - center;
            delta * delta
        })
        .sum::<f64>();
    Some((square_sum / (values.len() - 1) as f64).sqrt())
}

/// Wilson score interval for a hit rate. `z` is 1.96 for 95%.
pub fn wilson(successes: u32, trials: u32) -> Option<(f64, f64)> {
    if trials == 0 {
        return None;
    }
    let n = f64::from(trials);
    let phat = f64::from(successes) / n;
    let z = 1.96_f64;
    let z2 = z * z;
    let denom = 1.0 + z2 / n;
    let center = (phat + z2 / (2.0 * n)) / denom;
    let margin = z * ((phat * (1.0 - phat) / n) + (z2 / (4.0 * n * n))).sqrt() / denom;
    Some(((center - margin).clamp(0.0, 1.0), (center + margin).clamp(0.0, 1.0)))
}

pub fn split_matchup(matchup: &str) -> (String, String) {
    if let Some((_, opponent)) = matchup.split_once(" @ ") {
        return ("away".to_string(), opponent.trim().to_string());
    }
    if let Some((_, opponent)) = matchup.split_once(" vs. ") {
        return ("home".to_string(), opponent.trim().to_string());
    }
    if let Some((_, opponent)) = matchup.split_once(" vs ") {
        return ("home".to_string(), opponent.trim().to_string());
    }
    (String::new(), matchup.trim().to_string())
}

pub fn window_games(games: &[GameLog], window: Window) -> &[GameLog] {
    let count = match window {
        Window::Last5 => 5,
        Window::Last10 => 10,
        Window::Last20 => 20,
        Window::Season => return games,
    };
    if games.len() <= count {
        games
    } else {
        &games[games.len() - count..]
    }
}

pub fn trend_report(
    games: &[GameLog],
    stat: Stat,
    window: Window,
    line: f64,
    season: &str,
    season_type: &str,
) -> TrendReport {
    let slice = window_games(games, window);
    let values: Vec<f64> = slice.iter().map(|game| stat.value(game)).collect();
    let averages = moving_average(&values, 3);
    let overs = values.iter().filter(|value| is_over(**value, line)).count();
    let (low, high) = split_interval(wilson(overs as u32, values.len() as u32));
    let latest = games.last();
    let report_games = slice
        .iter()
        .zip(values.iter())
        .zip(averages.iter())
        .map(|((game, value), average)| {
            let (location, opponent) = split_matchup(&game.matchup);
            TrendGame {
                game_id: game.game_id.clone(),
                game_date: game.game_date.clone(),
                matchup: game.matchup.clone(),
                opponent,
                location,
                result: game.wl.clone(),
                minutes: game.minutes,
                stat: *value,
                over: is_over(*value, line),
                moving_avg: *average,
                points: game.pts,
                rebounds: game.reb,
                assists: game.ast,
                steals: game.stl,
                blocks: game.blk,
                turnovers: game.tov,
                plus_minus: game.plus_minus,
            }
        })
        .collect();

    TrendReport {
        player_id: latest.map(|game| game.player_id).unwrap_or(0),
        player_name: latest
            .map(|game| game.player_name.clone())
            .unwrap_or_default(),
        team: latest.map(|game| game.team_abbr.clone()).unwrap_or_default(),
        season: season.to_string(),
        season_type: season_type.to_string(),
        stat: stat.id().to_string(),
        stat_label: stat.label().to_string(),
        window: window.id().to_string(),
        window_label: window.label().to_string(),
        line,
        summary: TrendSummary {
            sample: values.len(),
            overs,
            hit_rate: if values.is_empty() {
                None
            } else {
                Some(overs as f64 / values.len() as f64)
            },
            wilson_low: low,
            wilson_high: high,
            mean: mean(&values),
            median: median(&values),
            sd: sample_sd(&values),
            min: values.iter().copied().reduce(f64::min),
            max: values.iter().copied().reduce(f64::max),
        },
        games: report_games,
    }
}

fn window_hits(values: &[f64], line: f64, count: Option<usize>) -> BoardSplit {
    let slice = match count {
        Some(count) if values.len() > count => &values[values.len() - count..],
        _ => values,
    };
    BoardSplit {
        overs: slice.iter().filter(|value| is_over(**value, line)).count() as u32,
        games: slice.len() as u32,
    }
}

pub fn leaderboard(games: &[GameLog], stat: Stat, min_games: u32, line: f64) -> Vec<BoardRow> {
    let mut rows: Vec<BoardRow> = Vec::new();
    let mut index = 0;
    while index < games.len() {
        let player_id = games[index].player_id;
        let end = games[index..]
            .iter()
            .position(|game| game.player_id != player_id)
            .map(|offset| index + offset)
            .unwrap_or(games.len());
        let group = &games[index..end];
        index = end;
        if group.len() < min_games as usize {
            continue;
        }
        let values: Vec<f64> = group.iter().map(|game| stat.value(game)).collect();
        let latest = group.last().expect("group is non-empty");
        rows.push(BoardRow {
            player_id,
            name: latest.player_name.clone(),
            team: latest.team_abbr.clone(),
            games: group.len(),
            mean: mean(&values).unwrap_or(0.0),
            last5: window_hits(&values, line, Some(5)),
            last10: window_hits(&values, line, Some(10)),
            last20: window_hits(&values, line, Some(20)),
            season: window_hits(&values, line, None),
        });
    }
    rows.sort_by(|left, right| {
        hit_rate(right.last10.overs, right.last10.games)
            .partial_cmp(&hit_rate(left.last10.overs, left.last10.games))
            .unwrap_or(std::cmp::Ordering::Equal)
            .then(right.last10.overs.cmp(&left.last10.overs))
            .then(left.name.cmp(&right.name))
    });
    rows
}

fn hit_rate(overs: u32, games: u32) -> f64 {
    if games == 0 {
        0.0
    } else {
        f64::from(overs) / f64::from(games)
    }
}

fn split_interval(value: Option<(f64, f64)>) -> (Option<f64>, Option<f64>) {
    match value {
        Some((low, high)) => (Some(low), Some(high)),
        None => (None, None),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn game(date: &str, pts: i32, reb: i32, ast: i32) -> GameLog {
        GameLog {
            player_id: 1,
            player_name: "Sample".to_string(),
            team_abbr: "DEN".to_string(),
            game_id: date.to_string(),
            game_date: date.to_string(),
            matchup: "DEN vs. LAL".to_string(),
            wl: "W".to_string(),
            minutes: 34.0,
            pts,
            reb,
            ast,
            stl: 1,
            blk: 1,
            tov: 2,
            fgm: 8,
            fga: 16,
            fg3m: 2,
            ftm: 4,
            plus_minus: 5,
        }
    }

    #[test]
    fn board_counts_recent_windows_against_the_line() {
        let games: Vec<GameLog> = (1..=12)
            .map(|day| game(&format!("2026-01-{day:02}"), if day > 7 { 30 } else { 10 }, 0, 0))
            .collect();
        let rows = leaderboard(&games, Stat::Points, 1, 20.0);
        assert_eq!(rows.len(), 1);
        assert_eq!((rows[0].last5.overs, rows[0].last5.games), (5, 5));
        assert_eq!((rows[0].last10.overs, rows[0].last10.games), (5, 10));
        assert_eq!((rows[0].season.overs, rows[0].season.games), (5, 12));
    }

    #[test]
    fn tie_counts_as_over() {
        assert!(is_over(26.0, 26.0));
        assert!(!is_over(25.0, 25.5));
    }

    #[test]
    fn moving_average_waits_for_three_games() {
        let averages = moving_average(&[10.0, 20.0, 30.0, 40.0], 3);
        assert_eq!(averages, vec![None, None, Some(20.0), Some(30.0)]);
    }

    #[test]
    fn median_and_sample_deviation() {
        assert_eq!(median(&[1.0, 2.0, 3.0, 4.0]), Some(2.5));
        assert_eq!(median(&[1.0, 2.0, 3.0]), Some(2.0));
        assert_eq!(sample_sd(&[2.0, 4.0]), Some(2.0_f64.sqrt()));
        assert_eq!(sample_sd(&[5.0]), None);
    }

    #[test]
    fn wilson_interval_for_seven_of_ten() {
        let (low, high) = wilson(7, 10).unwrap();
        assert!((low - 0.3968).abs() < 0.001, "low {low}");
        assert!((high - 0.8922).abs() < 0.001, "high {high}");
        assert!(wilson(0, 0).is_none());
    }

    #[test]
    fn matchup_sides() {
        assert_eq!(split_matchup("DEN @ SAS"), ("away".into(), "SAS".into()));
        assert_eq!(split_matchup("DAL vs. CHI"), ("home".into(), "CHI".into()));
    }

    #[test]
    fn last_ten_uses_the_recent_edge_and_combo_stats() {
        let games: Vec<_> = (1..=12)
            .map(|day| game(&format!("2026-01-{day:02}"), day, 10, 5))
            .collect();
        let report = trend_report(
            &games,
            Stat::PointsAssists,
            Window::Last10,
            14.0,
            "2025-26",
            "Regular Season",
        );
        assert_eq!(report.games.len(), 10);
        assert_eq!(report.games[0].game_date, "2026-01-03");
        assert_eq!(report.games[0].stat, 8.0);
        assert_eq!(report.summary.overs, 4);
        assert!(report.games[0].moving_avg.is_none());
        assert_eq!(report.games[2].moving_avg, Some(9.0));
    }
}
