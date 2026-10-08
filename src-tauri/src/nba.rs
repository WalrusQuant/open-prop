use std::collections::HashMap;
use std::time::Duration;

use serde_json::Value;
use wreq::Client;
use wreq_util::Emulation;

use crate::error::{AppError, AppResult};
use crate::models::{DraftPick, GameLog, RosterEntry};

/// stats.nba.com team ids to the abbreviations game logs use.
const TEAM_ABBR: &[(i64, &str)] = &[
    (1610612737, "ATL"),
    (1610612738, "BOS"),
    (1610612739, "CLE"),
    (1610612740, "NOP"),
    (1610612741, "CHI"),
    (1610612742, "DAL"),
    (1610612743, "DEN"),
    (1610612744, "GSW"),
    (1610612745, "HOU"),
    (1610612746, "LAC"),
    (1610612747, "LAL"),
    (1610612748, "MIA"),
    (1610612749, "MIL"),
    (1610612750, "MIN"),
    (1610612751, "BKN"),
    (1610612752, "NYK"),
    (1610612753, "ORL"),
    (1610612754, "IND"),
    (1610612755, "PHI"),
    (1610612756, "PHX"),
    (1610612757, "POR"),
    (1610612758, "SAC"),
    (1610612759, "SAS"),
    (1610612760, "OKC"),
    (1610612761, "TOR"),
    (1610612762, "UTA"),
    (1610612763, "MEM"),
    (1610612764, "WAS"),
    (1610612765, "DET"),
    (1610612766, "CHA"),
];

const GAME_LOGS_URL: &str = "https://stats.nba.com/stats/playergamelogs";
/// Every player on a team for the season, one request for the league.
const ROSTER_URL: &str = "https://stats.nba.com/stats/commonallplayers";
/// Standings with clinch / play-in flags. `playoffpicture` returns HTML now.
const STANDINGS_URL: &str = "https://stats.nba.com/stats/leaguestandingsv3";
const DRAFT_URL: &str = "https://stats.nba.com/stats/drafthistory";

/// stats.nba.com answers a browser TLS fingerprint and returns nothing to a
/// plain Rust or curl client. wreq sends the Chrome 136 fingerprint that the
/// endpoint accepts. The NBA headers below are the ones the stats site sends.
#[derive(Clone)]
pub struct NbaClient {
    http: Client,
}

impl NbaClient {
    pub fn new() -> AppResult<Self> {
        let http = Client::builder()
            .emulation(Emulation::Chrome136)
            .timeout(Duration::from_secs(120))
            .connect_timeout(Duration::from_secs(20))
            .redirect(wreq::redirect::Policy::limited(5))
            .build()?;
        Ok(Self { http })
    }

    pub async fn player_game_logs(&self, season: &str, season_type: &str) -> AppResult<Vec<GameLog>> {
        let body = self.get_logs(season, season_type).await?;
        parse_player_game_logs(&body, season, season_type)
    }

    /// Players on an NBA roster for `season`, with their team. Free agents are left out.
    pub async fn roster(&self, season: &str) -> AppResult<Vec<RosterEntry>> {
        let params = [
            ("IsOnlyCurrentSeason", "1"),
            ("LeagueID", "00"),
            ("Season", season),
        ];
        let body = self.get_with_retries(ROSTER_URL, &params).await?;
        parse_roster(&body)
    }

    /// Teams still in the postseason for `season`: clinched playoffs or still in the play-in.
    pub async fn playoff_teams(&self, season: &str) -> AppResult<Vec<String>> {
        let params = [
            ("LeagueID", "00"),
            ("Season", season),
            ("SeasonType", "Regular Season"),
            ("SeasonYear", ""),
        ];
        let body = self.get_with_retries(STANDINGS_URL, &params).await?;
        parse_playoff_teams(&body)
    }

    /// Draft class for a calendar year (`2024` for the 2024 draft → 2024-25 rookies).
    pub async fn draft_history(&self, year: i32) -> AppResult<Vec<DraftPick>> {
        let season = year.to_string();
        let params = [
            ("LeagueID", "00"),
            ("Season", season.as_str()),
            ("OverallPickFrom", ""),
            ("OverallPickTo", ""),
            ("RoundNum", ""),
            ("RoundPick", ""),
            ("TeamID", "0"),
            ("TopX", ""),
        ];
        let body = self.get_with_retries(DRAFT_URL, &params).await?;
        parse_draft_history(&body)
    }

    async fn get_logs(&self, season: &str, season_type: &str) -> AppResult<String> {
        let params = [
            ("DateFrom", ""),
            ("DateTo", ""),
            ("GameSegment", ""),
            ("LastNGames", "0"),
            ("LeagueID", "00"),
            ("Location", ""),
            ("MeasureType", "Base"),
            ("Month", "0"),
            ("OpponentTeamID", "0"),
            ("Outcome", ""),
            ("PORound", "0"),
            ("PerMode", "Totals"),
            ("Period", "0"),
            ("PlayerID", ""),
            ("Season", season),
            ("SeasonSegment", ""),
            ("SeasonType", season_type),
            ("ShotClockRange", ""),
            ("TeamID", "0"),
            ("VsConference", ""),
            ("VsDivision", ""),
        ];
        self.get_with_retries(GAME_LOGS_URL, &params).await
    }

    async fn get_with_retries(&self, url: &str, params: &[(&str, &str)]) -> AppResult<String> {
        let mut last_error = None;
        for attempt in 0..3 {
            match self.get_once(url, params).await {
                Ok(body) => return Ok(body),
                Err(error) => {
                    last_error = Some(error);
                    tokio::time::sleep(Duration::from_millis(500 * (attempt + 1))).await;
                }
            }
        }
        Err(last_error.unwrap_or_else(|| AppError::message("the NBA stats request failed")))
    }

    async fn get_once(&self, url: &str, params: &[(&str, &str)]) -> AppResult<String> {
        let response = self
            .http
            .get(url)
            .header("Accept", "application/json, text/plain, */*")
            .header("Accept-Language", "en-US,en;q=0.9")
            .header("Origin", "https://www.nba.com")
            .header("Referer", "https://www.nba.com/")
            .header("x-nba-stats-origin", "stats")
            .header("x-nba-stats-token", "true")
            .query(params)
            .send()
            .await?;
        let status = response.status();
        let body = response.text().await?;
        if !status.is_success() {
            return Err(AppError::Nba(format!(
                "HTTP {status}: {}",
                snippet(&body)
            )));
        }
        Ok(body)
    }
}

/// Reads `commonallplayers`. A player counts when he is on a roster and has a team.
pub fn parse_roster(body: &str) -> AppResult<Vec<RosterEntry>> {
    let (columns, rows) = result_set(body)?;
    let mut players = Vec::with_capacity(rows.len());
    for row in &rows {
        let Some(row) = row.as_array() else {
            continue;
        };
        let team = text_at(row, &columns, "TEAM_ABBREVIATION")?.trim().to_string();
        if team.is_empty() || number_at(row, &columns, "ROSTERSTATUS")? < 1.0 {
            continue;
        }
        players.push(RosterEntry {
            player_id: number_at(row, &columns, "PERSON_ID")? as i64,
            name: text_at(row, &columns, "DISPLAY_FIRST_LAST")?,
            team_abbr: team,
        });
    }
    Ok(players)
}

/// Teams with `ClinchedPostSeason` or `ClinchedPlayIn`. Play-in teams stay until the
/// playoff game logs take over as the source of truth.
pub fn parse_playoff_teams(body: &str) -> AppResult<Vec<String>> {
    let (columns, rows) = result_set(body)?;
    let mut teams = Vec::new();
    for row in &rows {
        let Some(row) = row.as_array() else {
            continue;
        };
        let post = flag_at(row, &columns, "ClinchedPostSeason")?;
        let play_in = flag_at(row, &columns, "ClinchedPlayIn")?;
        if !post && !play_in {
            continue;
        }
        let team_id = number_at(row, &columns, "TeamID")? as i64;
        let Some(abbr) = team_abbr(team_id) else {
            continue;
        };
        teams.push(abbr.to_string());
    }
    teams.sort();
    teams.dedup();
    Ok(teams)
}

/// Reads `drafthistory` for one draft year.
pub fn parse_draft_history(body: &str) -> AppResult<Vec<DraftPick>> {
    let (columns, rows) = result_set(body)?;
    let mut picks = Vec::with_capacity(rows.len());
    for row in &rows {
        let Some(row) = row.as_array() else {
            continue;
        };
        let overall = number_at(row, &columns, "OVERALL_PICK")? as i32;
        if overall <= 0 {
            continue;
        }
        picks.push(DraftPick {
            player_id: number_at(row, &columns, "PERSON_ID")? as i64,
            name: text_at(row, &columns, "PLAYER_NAME")?,
            draft_year: number_at(row, &columns, "SEASON")? as i32,
            round: number_at(row, &columns, "ROUND_NUMBER")? as i32,
            overall_pick: overall,
        });
    }
    Ok(picks)
}

fn team_abbr(team_id: i64) -> Option<&'static str> {
    TEAM_ABBR.iter().find(|(id, _)| *id == team_id).map(|(_, abbr)| *abbr)
}

/// True when the standings cell is 1 (JSON number or string).
fn flag_at(row: &[Value], columns: &HashMap<String, usize>, name: &str) -> AppResult<bool> {
    let Some(&index) = columns.get(name) else {
        return Ok(false);
    };
    let Some(value) = row.get(index) else {
        return Ok(false);
    };
    Ok(match value {
        Value::Null => false,
        Value::Bool(flag) => *flag,
        Value::Number(number) => number.as_f64() == Some(1.0),
        Value::String(text) => text == "1" || text.eq_ignore_ascii_case("true"),
        _ => false,
    })
}

fn result_set(body: &str) -> AppResult<(HashMap<String, usize>, Vec<Value>)> {
    let payload: Value = serde_json::from_str(body)?;
    let set = payload
        .get("resultSets")
        .and_then(|value| value.as_array())
        .and_then(|sets| sets.first())
        .ok_or_else(|| AppError::Nba(format!("unexpected stats payload: {}", snippet(body))))?;
    let headers = set
        .get("headers")
        .and_then(|value| value.as_array())
        .ok_or_else(|| AppError::message("the stats payload has no headers"))?;
    let mut columns = HashMap::new();
    for (index, header) in headers.iter().enumerate() {
        if let Some(name) = header.as_str() {
            columns.entry(name.to_string()).or_insert(index);
        }
    }
    let rows = set
        .get("rowSet")
        .and_then(|value| value.as_array())
        .cloned()
        .ok_or_else(|| AppError::message("the stats payload has no rows"))?;
    Ok((columns, rows))
}

pub fn parse_player_game_logs(body: &str, season: &str, season_type: &str) -> AppResult<Vec<GameLog>> {
    let (columns, rows) = result_set(body)?;
    let mut games = Vec::with_capacity(rows.len());
    for row in &rows {
        let Some(row) = row.as_array() else {
            continue;
        };
        let game_id = game_id_at(row, &columns)?;
        // 003 is the All-Star game id prefix. It is not a season game.
        if game_id.starts_with("003") {
            continue;
        }
        let game_date = parse_nba_date(&text_at(row, &columns, "GAME_DATE")?).ok_or_else(|| {
            AppError::message(format!("could not read a game date in {game_id}"))
        })?;
        games.push(GameLog {
            player_id: number_at(row, &columns, "PLAYER_ID")? as i64,
            player_name: text_at(row, &columns, "PLAYER_NAME")?,
            team_abbr: text_at(row, &columns, "TEAM_ABBREVIATION")?,
            game_id,
            game_date,
            matchup: text_at(row, &columns, "MATCHUP")?,
            wl: text_at(row, &columns, "WL")?,
            minutes: minutes_at(row, &columns)?,
            pts: number_at(row, &columns, "PTS")? as i32,
            reb: number_at(row, &columns, "REB")? as i32,
            ast: number_at(row, &columns, "AST")? as i32,
            stl: number_at(row, &columns, "STL")? as i32,
            blk: number_at(row, &columns, "BLK")? as i32,
            tov: number_at(row, &columns, "TOV")? as i32,
            fgm: number_at(row, &columns, "FGM")? as i32,
            fga: number_at(row, &columns, "FGA")? as i32,
            fg3m: number_at(row, &columns, "FG3M")? as i32,
            ftm: number_at(row, &columns, "FTM")? as i32,
            plus_minus: number_at(row, &columns, "PLUS_MINUS")? as i32,
        });
    }
    let _ = (season, season_type);
    Ok(games)
}

fn cell<'a>(
    row: &'a [Value],
    columns: &HashMap<String, usize>,
    name: &str,
) -> AppResult<&'a Value> {
    let index = columns
        .get(name)
        .copied()
        .ok_or_else(|| AppError::message(format!("the stats payload is missing {name}")))?;
    row.get(index)
        .ok_or_else(|| AppError::message(format!("a stats row is missing {name}")))
}

fn text_at(row: &[Value], columns: &HashMap<String, usize>, name: &str) -> AppResult<String> {
    Ok(match cell(row, columns, name)? {
        Value::String(value) => value.clone(),
        Value::Number(value) => value.to_string(),
        Value::Null => String::new(),
        other => other.to_string(),
    })
}

fn number_at(row: &[Value], columns: &HashMap<String, usize>, name: &str) -> AppResult<f64> {
    Ok(match cell(row, columns, name)? {
        Value::Number(value) => value.as_f64().unwrap_or(0.0),
        Value::String(value) => parse_number(value),
        _ => 0.0,
    })
}

fn minutes_at(row: &[Value], columns: &HashMap<String, usize>) -> AppResult<f64> {
    Ok(match cell(row, columns, "MIN")? {
        Value::Number(value) => value.as_f64().unwrap_or(0.0),
        Value::String(value) => parse_minutes(value),
        _ => 0.0,
    })
}

pub fn parse_nba_date(raw: &str) -> Option<String> {
    let value = raw.trim();
    if value.len() >= 10 && value.as_bytes().get(4) == Some(&b'-') && value.as_bytes().get(7) == Some(&b'-')
    {
        return Some(value[..10].to_string());
    }
    let cleaned = value.replace(',', " ");
    let parts: Vec<_> = cleaned.split_whitespace().collect();
    if parts.len() != 3 {
        return None;
    }
    let month = month_number(parts[0])?;
    let day: u32 = parts[1].parse().ok()?;
    let year: i32 = parts[2].parse().ok()?;
    if !(1..=31).contains(&day) {
        return None;
    }
    Some(format!("{year:04}-{month:02}-{day:02}"))
}

fn game_id_at(row: &[Value], columns: &HashMap<String, usize>) -> AppResult<String> {
    Ok(match cell(row, columns, "GAME_ID")? {
        Value::String(value) => value.clone(),
        // A numeric id drops the leading zeros. Ten digits puts 002 and 003 back.
        Value::Number(value) => match value.as_i64() {
            Some(id) if (0..10_000_000_000).contains(&id) => format!("{id:010}"),
            _ => value.to_string(),
        },
        Value::Null => String::new(),
        other => other.to_string(),
    })
}

fn month_number(name: &str) -> Option<u32> {
    let lower = name.to_ascii_lowercase();
    let key = &lower[..lower.len().min(3)];
    Some(match key {
        "jan" => 1,
        "feb" => 2,
        "mar" => 3,
        "apr" => 4,
        "may" => 5,
        "jun" => 6,
        "jul" => 7,
        "aug" => 8,
        "sep" => 9,
        "oct" => 10,
        "nov" => 11,
        "dec" => 12,
        _ => return None,
    })
}

fn parse_minutes(value: &str) -> f64 {
    let value = value.trim();
    if let Some((minutes, seconds)) = value.split_once(':') {
        let minutes = minutes.parse::<f64>().unwrap_or(0.0);
        let seconds = seconds.parse::<f64>().unwrap_or(0.0);
        return minutes + seconds / 60.0;
    }
    parse_number(value)
}

fn parse_number(value: &str) -> f64 {
    value.trim().parse::<f64>().unwrap_or(0.0)
}

fn snippet(body: &str) -> String {
    let compact: String = body.chars().take(180).collect();
    if compact.is_empty() {
        "empty response".to_string()
    } else {
        compact
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_a_game_log_and_drops_the_all_star_game() {
        let body = r#"{
          "resultSets": [{
            "name": "PlayerGameLogs",
            "headers": ["SEASON_YEAR","PLAYER_ID","PLAYER_NAME","TEAM_ABBREVIATION","GAME_ID","GAME_DATE","MATCHUP","WL","MIN","FGM","FGA","FG3M","FTM","REB","AST","TOV","STL","BLK","PTS","PLUS_MINUS"],
            "rowSet": [
              ["2025-26", 203999, "Nikola Jokic", "DEN", "0022500001", "2025-10-22T00:00:00", "DEN vs. LAL", "W", 36.5, 10, 18, 2, 7, 12, 11, 3, 1, 1, 29, 8],
              ["2025-26", 203999, "Nikola Jokic", "DEN", "0032500001", "2025-02-15T00:00:00", "DEN vs. EST", "W", 20, 5, 8, 1, 2, 5, 6, 1, 0, 0, 13, 4],
              ["2025-26", 201142, "Kevin Durant", "HOU", "0022500002", "Apr 12, 2026", "HOU @ SAS", "L", "32:30", 9, 17, 3, 6, 7, 4, 2, 0, 2, 27, -3]
            ]
          }]
        }"#;
        let games = parse_player_game_logs(body, "2025-26", "Regular Season").unwrap();
        assert_eq!(games.len(), 2);
        assert_eq!(games[0].player_name, "Nikola Jokic");
        assert_eq!(games[0].game_date, "2025-10-22");
        assert_eq!(games[0].minutes, 36.5);
        assert_eq!(games[1].player_name, "Kevin Durant");
        assert_eq!(games[1].game_date, "2026-04-12");
        assert!((games[1].minutes - 32.5).abs() < 0.001);
        assert_eq!(games[1].plus_minus, -3);
    }

    #[test]
    fn numeric_game_ids_keep_the_season_prefix() {
        let body = r#"{
          "resultSets": [{
            "headers": ["SEASON_YEAR","PLAYER_ID","PLAYER_NAME","TEAM_ABBREVIATION","GAME_ID","GAME_DATE","MATCHUP","WL","MIN","FGM","FGA","FG3M","FTM","REB","AST","TOV","STL","BLK","PTS","PLUS_MINUS"],
            "rowSet": [
              ["2025-26", 1, "N Sample", "LAB", 22500001, "2025-10-24T00:00:00", "LAB vs. DAL", "W", 30, 8, 16, 2, 4, 6, 5, 2, 1, 0, 22, 3],
              ["2025-26", 1, "N Sample", "LAB", 32500001, "2026-02-15T00:00:00", "LAB vs. EST", "W", 12, 3, 6, 1, 1, 2, 2, 1, 0, 0, 8, 1]
            ]
          }]
        }"#;
        let games = parse_player_game_logs(body, "2025-26", "Regular Season").unwrap();
        assert_eq!(games.len(), 1);
        assert_eq!(games[0].game_id, "0022500001");
    }

    /// Hits stats.nba.com. Run with `cargo test -- --ignored live_regular_season`.
    #[tokio::test]
    #[ignore]
    async fn live_regular_season_has_a_full_slate() {
        let client = NbaClient::new().expect("chrome client");
        let games = client
            .player_game_logs("2025-26", "Regular Season")
            .await
            .expect("playergamelogs");
        assert!(
            games.len() > 1000,
            "expected a season of logs, got {}",
            games.len()
        );
        assert!(
            games.iter().any(|game| game.player_id == 203999),
            "expected Nikola Jokic (203999) in {} games",
            games.len()
        );
        assert!(games.iter().all(|game| !game.game_id.starts_with("003")));
        assert!(games.iter().all(|game| game.game_date.len() == 10));
    }

    /// Hits stats.nba.com. Run with `cargo test -- --ignored live_roster`.
    #[tokio::test]
    #[ignore]
    async fn live_roster_has_every_team() {
        let client = NbaClient::new().expect("chrome client");
        let players = client.roster("2026-27").await.expect("commonallplayers");
        let teams: std::collections::BTreeSet<_> = players.iter().map(|player| player.team_abbr.clone()).collect();
        eprintln!("{} players on {} teams", players.len(), teams.len());
        assert_eq!(teams.len(), 30, "{teams:?}");
        assert!(players.len() > 400, "{}", players.len());
    }

    /// Hits stats.nba.com. Run with `cargo test -- --ignored live_playoff`.
    #[tokio::test]
    #[ignore]
    async fn live_playoff_teams_match_the_bracket() {
        let client = NbaClient::new().expect("chrome client");
        let teams_25 = client.playoff_teams("2024-25").await.expect("2024-25 standings");
        eprintln!("2024-25 playoff/play-in: {} {:?}", teams_25.len(), teams_25);
        assert_eq!(teams_25.len(), 20, "{teams_25:?}");
        assert!(teams_25.iter().any(|team| team == "OKC"));
        assert!(teams_25.iter().any(|team| team == "SAC"), "play-in loser still listed until games override");
        let teams_26 = client.playoff_teams("2025-26").await.expect("2025-26 standings");
        eprintln!("2025-26 playoff/play-in: {} {:?}", teams_26.len(), teams_26);
        assert_eq!(teams_26.len(), 20, "{teams_26:?}");
        assert!(teams_26.iter().any(|team| team == "OKC"));
        assert!(teams_26.iter().any(|team| team == "DET"));
    }

    #[test]
    fn rookie_bucket_cuts() {
        use crate::models::RookieBucket;
        assert_eq!(RookieBucket::from_overall(Some(1)), RookieBucket::LotteryTop);
        assert_eq!(RookieBucket::from_overall(Some(10)), RookieBucket::LotteryRest);
        assert_eq!(RookieBucket::from_overall(Some(22)), RookieBucket::FirstRoundLate);
        assert_eq!(RookieBucket::from_overall(Some(45)), RookieBucket::SecondRound);
        assert_eq!(RookieBucket::from_overall(None), RookieBucket::Undrafted);
    }

    #[test]
    fn draft_history_parse_reads_overall_pick() {
        let body = concat!(
            r#"{"resultSets":[{"headers":["PERSON_ID","PLAYER_NAME","SEASON","ROUND_NUMBER","ROUND_PICK","OVERALL_PICK"],"rowSet":["#,
            r#"[1,"A",2024,1,1,1],[2,"B",2024,2,1,31]"#,
            r#"]}]}"#
        );
        let picks = parse_draft_history(body).unwrap();
        assert_eq!(picks.len(), 2);
        assert_eq!(picks[0].overall_pick, 1);
        assert_eq!(picks[1].round, 2);
    }

    #[test]
    fn playoff_teams_parse_clinch_and_play_in() {

        let body = r#"{"resultSets":[{"headers":["TeamID","ClinchedPostSeason","ClinchedPlayIn"],"rowSet":[
            [1610612760,1,0],
            [1610612758,0,1],
            [1610612764,0,0],
            [1610612738,1,0]
        ]}]}"#;
        let teams = parse_playoff_teams(body).unwrap();
        assert_eq!(teams, vec!["BOS", "OKC", "SAC"]);
    }
}
