use chrono::NaiveDate;

use crate::models::{
    BoardRow, BoardSplit, GameLog, LastSeason, PlayerOption, Stat, TeamSource, TrendGame, TrendReport,
    TrendSplit, TrendSummary, Window,
};

/// A game counts as an over when the stat is greater than or equal to the line.
pub fn is_over(stat: f64, line: f64) -> bool {
    stat >= line
}

/// A 0-minute game is a did-not-play. Hit rates leave it out, the same as the model.
pub fn played(game: &GameLog) -> bool {
    game.minutes > 0.0
}

/// Did-not-play games on or after the first game of `slice`. A whole season counts all of them.
fn dnp_since(games: &[GameLog], slice: &[GameLog], window: Window) -> usize {
    let first = match (window, slice.first()) {
        (Window::Season, _) => None,
        (_, Some(game)) => Some(game.game_date.as_str()),
        (_, None) => return 0,
    };
    games
        .iter()
        .filter(|game| !played(game))
        .filter(|game| first.is_none_or(|date| game.game_date.as_str() >= date))
        .count()
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

/// Days off before each played game, from the previous played game. The first has none.
fn rest_days(played_games: &[GameLog]) -> Vec<Option<i64>> {
    let mut previous: Option<NaiveDate> = None;
    played_games
        .iter()
        .map(|game| {
            let day = NaiveDate::parse_from_str(&game.game_date, "%Y-%m-%d").ok();
            let gap = match (previous, day) {
                (Some(before), Some(now)) => Some((now - before).num_days() - 1),
                _ => None,
            };
            if day.is_some() {
                previous = day;
            }
            gap
        })
        .collect()
}

/// One window against the line: the count, the rate, and its 95% Wilson band.
fn split(games: &[GameLog], played_games: &[GameLog], stat: Stat, window: Window, line: f64) -> TrendSplit {
    let slice = window_games(played_games, window);
    let overs = slice
        .iter()
        .filter(|game| is_over(stat.value(game), line))
        .count();
    let (low, high) = split_interval(wilson(overs as u32, slice.len() as u32));
    TrendSplit {
        window: window.id().to_string(),
        sample: slice.len(),
        overs,
        hit_rate: if slice.is_empty() {
            None
        } else {
            Some(overs as f64 / slice.len() as f64)
        },
        wilson_low: low,
        wilson_high: high,
        dnp: dnp_since(games, slice, window),
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
    let played_games: Vec<GameLog> = games.iter().filter(|game| played(game)).cloned().collect();
    let rests = rest_days(&played_games);
    let slice = window_games(&played_games, window);
    let slice_rests = &rests[played_games.len() - slice.len()..];
    let values: Vec<f64> = slice.iter().map(|game| stat.value(game)).collect();
    let averages = moving_average(&values, 3);
    let splits: Vec<TrendSplit> = Window::all()
        .iter()
        .map(|each| split(games, &played_games, stat, *each, line))
        .collect();
    let active = splits
        .iter()
        .find(|item| item.window == window.id())
        .cloned()
        .unwrap_or_else(|| split(games, &played_games, stat, window, line));
    let latest = games.last();
    let report_games = slice
        .iter()
        .zip(values.iter())
        .zip(averages.iter())
        .zip(slice_rests.iter())
        .map(|(((game, value), average), rest)| {
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
                rest_days: *rest,
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
            sample: active.sample,
            overs: active.overs,
            hit_rate: active.hit_rate,
            wilson_low: active.wilson_low,
            wilson_high: active.wilson_high,
            mean: mean(&values),
            median: median(&values),
            sd: sample_sd(&values),
            min: values.iter().copied().reduce(f64::min),
            max: values.iter().copied().reduce(f64::max),
            dnp: active.dnp,
        },
        splits,
        games: report_games,
        no_games: false,
        team_source: TeamSource::Season,
        last_season: None,
    }
}

/// The report for a listed player with no game this season: empty windows, plus his carried
/// season against the same line when `carried` has games (`("2025-26 Regular Season", games)`).
pub fn no_games_report(
    player: &PlayerOption,
    stat: Stat,
    window: Window,
    line: f64,
    season: &str,
    season_type: &str,
    carried: Option<(&str, &[GameLog])>,
) -> TrendReport {
    let mut report = trend_report(&[], stat, window, line, season, season_type);
    report.player_id = player.player_id;
    report.player_name = player.name.clone();
    report.team = player.team.clone();
    report.team_source = player.team_source;
    report.no_games = true;
    report.last_season = carried
        .filter(|(_, games)| games.iter().any(played))
        .and_then(|(label, games)| {
            let whole = trend_report(games, stat, Window::Season, line, season, season_type);
            let median = whole.summary.median;
            let split = whole.splits.into_iter().find(|item| item.window == Window::Season.id())?;
            Some(LastSeason {
                season: label.to_string(),
                split,
                median,
            })
        });
    report
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
        let all = &games[index..end];
        index = end;
        let group: Vec<&GameLog> = all.iter().filter(|game| played(game)).collect();
        if group.is_empty() || group.len() < min_games as usize {
            continue;
        }
        let values: Vec<f64> = group.iter().map(|game| stat.value(game)).collect();
        let latest = all.last().expect("group is non-empty");
        rows.push(BoardRow {
            player_id,
            name: latest.player_name.clone(),
            team: latest.team_abbr.clone(),
            games: group.len(),
            dnp: all.len() - group.len(),
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
    fn a_player_without_games_gets_empty_windows_and_last_season_labelled() {
        let player = PlayerOption {
            player_id: 7,
            name: "A Player".to_string(),
            team: "MIN".to_string(),
            games: 0,
            team_source: TeamSource::LastSeason,
        };
        let last = vec![game("2025-01-01", 30, 5, 5), game("2025-01-03", 10, 5, 5), game("2025-01-05", 26, 5, 5)];
        let report = no_games_report(
            &player,
            Stat::Points,
            Window::Last10,
            25.0,
            "2025-26",
            "Regular Season",
            Some(("2024-25 Regular Season", &last)),
        );
        assert!(report.no_games);
        assert_eq!((report.player_name.as_str(), report.team.as_str()), ("A Player", "MIN"));
        assert_eq!(report.team_source, TeamSource::LastSeason);
        assert!(report.games.is_empty());
        assert!(report.splits.iter().all(|split| split.sample == 0 && split.hit_rate.is_none()));
        let carried = report.last_season.unwrap();
        assert_eq!(carried.season, "2024-25 Regular Season");
        assert_eq!((carried.split.overs, carried.split.sample), (2, 3));
        assert_eq!(carried.median, Some(26.0));
        let rookie = no_games_report(&player, Stat::Points, Window::Last10, 25.0, "2025-26", "Regular Season", None);
        assert!(rookie.last_season.is_none());
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
    fn the_board_leaves_out_zero_minute_games() {
        let mut games: Vec<GameLog> = (1..=12)
            .map(|day| game(&format!("2026-01-{day:02}"), 30, 0, 0))
            .collect();
        games[11].minutes = 0.0;
        games[11].pts = 0;
        games[5].minutes = 0.0;
        games[5].pts = 0;
        let rows = leaderboard(&games, Stat::Points, 10, 20.0);
        assert_eq!(rows.len(), 1);
        assert_eq!((rows[0].games, rows[0].dnp), (10, 2));
        assert_eq!((rows[0].last5.overs, rows[0].last5.games), (5, 5));
        assert_eq!((rows[0].last10.overs, rows[0].last10.games), (10, 10));
        assert_eq!(rows[0].mean, 30.0);
        assert!(leaderboard(&games, Stat::Points, 11, 20.0).is_empty(), "min games counts played games");
    }

    #[test]
    fn the_trend_leaves_out_zero_minute_games_and_counts_them() {
        let mut games: Vec<GameLog> = (1..=12)
            .map(|day| game(&format!("2026-01-{day:02}"), 25, 0, 0))
            .collect();
        games[10].minutes = 0.0;
        games[10].pts = 0;
        games[1].minutes = 0.0;
        games[1].pts = 0;
        let last5 = trend_report(&games, Stat::Points, Window::Last5, 20.0, "2025-26", "Regular Season");
        assert_eq!(last5.games.len(), 5);
        assert_eq!(last5.games[0].game_date, "2026-01-07");
        assert_eq!((last5.summary.overs, last5.summary.sample, last5.summary.dnp), (5, 5, 1));
        assert_eq!(last5.summary.min, Some(25.0));
        let season = trend_report(&games, Stat::Points, Window::Season, 20.0, "2025-26", "Regular Season");
        assert_eq!((season.summary.overs, season.summary.sample, season.summary.dnp), (10, 10, 2));
        assert_eq!(season.summary.hit_rate, Some(1.0));
    }

    #[test]
    fn the_report_scores_every_window_against_one_line() {
        let games: Vec<GameLog> = (1..=24)
            .map(|day| game(&format!("2026-01-{day:02}"), if day > 17 { 30 } else { 10 }, 0, 0))
            .collect();
        let report = trend_report(&games, Stat::Points, Window::Last10, 20.0, "2025-26", "Regular Season");
        let counts: Vec<(&str, usize, usize)> = report
            .splits
            .iter()
            .map(|item| (item.window.as_str(), item.overs, item.sample))
            .collect();
        assert_eq!(
            counts,
            vec![("last_5", 5, 5), ("last_10", 7, 10), ("last_20", 7, 20), ("season", 7, 24)]
        );
        let last10 = &report.splits[1];
        let (low, high) = wilson(7, 10).unwrap();
        assert_eq!((last10.wilson_low, last10.wilson_high), (Some(low), Some(high)));
        assert_eq!(report.summary.overs, 7);
        assert_eq!(report.summary.wilson_low, Some(low));
    }

    #[test]
    fn rest_counts_from_the_last_game_played() {
        let mut games = vec![
            game("2026-01-01", 20, 0, 0),
            game("2026-01-02", 20, 0, 0),
            game("2026-01-04", 0, 0, 0),
            game("2026-01-06", 20, 0, 0),
        ];
        games[2].minutes = 0.0;
        let report = trend_report(&games, Stat::Points, Window::Season, 10.0, "2025-26", "Regular Season");
        let rests: Vec<Option<i64>> = report.games.iter().map(|item| item.rest_days).collect();
        assert_eq!(rests, vec![None, Some(0), Some(3)]);
    }

    fn numbers(value: &serde_json::Value) -> Vec<f64> {
        value.as_array().unwrap().iter().map(|item| item.as_f64().unwrap()).collect()
    }

    fn close(actual: Option<f64>, expected: &serde_json::Value, label: &str) {
        match (actual, expected.as_f64()) {
            (None, None) => {}
            (Some(actual), Some(expected)) => {
                assert!((actual - expected).abs() < 1e-9, "{label}: {actual} vs {expected}")
            }
            other => panic!("{label}: {other:?}"),
        }
    }

    /// The same file feeds vitest, so the TS copies the preview uses cannot drift from these.
    #[test]
    fn desk_math_matches_the_shared_fixture() {
        let fixture: serde_json::Value =
            serde_json::from_str(include_str!("../../tests/fixtures/desk-math.json")).unwrap();
        for case in fixture["wilson"].as_array().unwrap() {
            let overs = case["overs"].as_u64().unwrap() as u32;
            let games = case["games"].as_u64().unwrap() as u32;
            let band = wilson(overs, games);
            let label = format!("wilson {overs}/{games}");
            close(band.map(|(low, _)| low), &case["expected"][0], &label);
            close(band.map(|(_, high)| high), &case["expected"][1], &label);
        }
        for case in fixture["median"].as_array().unwrap() {
            close(median(&numbers(&case["values"])), &case["expected"], "median");
        }
        for case in fixture["mean"].as_array().unwrap() {
            close(mean(&numbers(&case["values"])), &case["expected"], "mean");
        }
        for case in fixture["sampleSd"].as_array().unwrap() {
            close(sample_sd(&numbers(&case["values"])), &case["expected"], "sample sd");
        }
        for case in fixture["movingAverage"].as_array().unwrap() {
            let width = case["width"].as_u64().unwrap() as usize;
            let averages = moving_average(&numbers(&case["values"]), width);
            let expected = case["expected"].as_array().unwrap();
            assert_eq!(averages.len(), expected.len());
            for (actual, expected) in averages.iter().zip(expected) {
                close(*actual, expected, "moving average");
            }
        }
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
