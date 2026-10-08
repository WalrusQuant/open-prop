use std::collections::{HashMap, HashSet};

use chrono::Utc;
use rusqlite::{params, Connection};

use crate::error::{AppError, AppResult};
use crate::models::{GameLog, PlayerOption, RosterEntry, SeasonStatus, SyncReport, TeamSource};
use crate::season;

const SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS game_logs (
    season TEXT NOT NULL,
    season_type TEXT NOT NULL,
    player_id INTEGER NOT NULL,
    player_name TEXT NOT NULL,
    team_abbr TEXT NOT NULL,
    game_id TEXT NOT NULL,
    game_date TEXT NOT NULL,
    matchup TEXT NOT NULL,
    wl TEXT NOT NULL,
    minutes REAL NOT NULL,
    pts INTEGER NOT NULL,
    reb INTEGER NOT NULL,
    ast INTEGER NOT NULL,
    stl INTEGER NOT NULL,
    blk INTEGER NOT NULL,
    tov INTEGER NOT NULL,
    fgm INTEGER NOT NULL,
    fga INTEGER NOT NULL,
    fg3m INTEGER NOT NULL,
    ftm INTEGER NOT NULL,
    plus_minus INTEGER NOT NULL,
    PRIMARY KEY (season, season_type, player_id, game_id)
);

CREATE INDEX IF NOT EXISTS idx_game_logs_player
    ON game_logs (season, season_type, player_id, game_date, game_id);

CREATE TABLE IF NOT EXISTS sync_meta (
    season TEXT NOT NULL,
    season_type TEXT NOT NULL,
    synced_at TEXT NOT NULL,
    games INTEGER NOT NULL,
    players INTEGER NOT NULL,
    PRIMARY KEY (season, season_type)
);

CREATE TABLE IF NOT EXISTS rosters (
    season TEXT NOT NULL,
    player_id INTEGER NOT NULL,
    player_name TEXT NOT NULL,
    team_abbr TEXT NOT NULL,
    fetched_at TEXT NOT NULL,
    PRIMARY KEY (season, player_id)
);
";

/// A roster answer with fewer players than this is treated as broken and the stored one is kept.
const MIN_ROSTER_PLAYERS: usize = 300;

pub fn open(path: &std::path::Path) -> AppResult<Connection> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|error| {
            AppError::message(format!("could not create the database folder: {error}"))
        })?;
    }
    let connection = Connection::open(path)?;
    connection.pragma_update(None, "journal_mode", "WAL")?;
    init(&connection)?;
    Ok(connection)
}

#[cfg(test)]
pub fn open_memory() -> AppResult<Connection> {
    let connection = Connection::open_in_memory()?;
    init(&connection)?;
    Ok(connection)
}

fn init(connection: &Connection) -> AppResult<()> {
    connection.pragma_update(None, "foreign_keys", "ON")?;
    connection.pragma_update(None, "busy_timeout", "5000")?;
    connection.execute_batch(SCHEMA)?;
    Ok(())
}

/// A sync under this share of the cached rows is treated as a bad response.
const SHORT_SYNC_SHARE: f64 = 0.5;

/// Merges a sync into the cache. Rows are upserted by game, and cached rows the
/// response left out stay. An empty response, or one under half of what is
/// cached, writes nothing and comes back with a warning instead.
pub fn merge_logs(
    connection: &Connection,
    season: &str,
    season_type: &str,
    games: &[GameLog],
) -> AppResult<SyncReport> {
    let (cached_games, cached_players) = cached_counts(connection, season, season_type)?;
    let short = (games.len() as f64) < cached_games as f64 * SHORT_SYNC_SHARE;
    if games.is_empty() || short {
        let warning = if games.is_empty() && cached_games == 0 {
            format!("NBA returned 0 rows for {season} {season_type}. Nothing was cached.")
        } else if games.is_empty() {
            format!(
                "NBA returned 0 rows for {season} {season_type}. The {cached_games} cached games were kept."
            )
        } else {
            format!(
                "NBA returned {} rows for {season} {season_type}, under half of the {cached_games} cached. The cache was kept. Sync again later.",
                games.len()
            )
        };
        return Ok(SyncReport {
            season: season.to_string(),
            season_type: season_type.to_string(),
            games: cached_games,
            players: cached_players,
            fetched: games.len(),
            synced_at: synced_at(connection, season, season_type)?,
            warning: Some(warning),
            roster_players: None,
        });
    }
    let synced_at = Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string();
    let transaction = connection.unchecked_transaction()?;
    {
        let mut statement = transaction.prepare(
            "INSERT OR REPLACE INTO game_logs (
                season, season_type, player_id, player_name, team_abbr, game_id, game_date,
                matchup, wl, minutes, pts, reb, ast, stl, blk, tov, fgm, fga, fg3m, ftm, plus_minus
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19, ?20, ?21)",
        )?;
        for game in games {
            statement.execute(params![
                season,
                season_type,
                game.player_id,
                game.player_name,
                game.team_abbr,
                game.game_id,
                game.game_date,
                game.matchup,
                game.wl,
                game.minutes,
                game.pts,
                game.reb,
                game.ast,
                game.stl,
                game.blk,
                game.tov,
                game.fgm,
                game.fga,
                game.fg3m,
                game.ftm,
                game.plus_minus,
            ])?;
        }
    }
    let (total_games, total_players) = cached_counts(&transaction, season, season_type)?;
    transaction.execute(
        "INSERT INTO sync_meta (season, season_type, synced_at, games, players)
         VALUES (?1, ?2, ?3, ?4, ?5)
         ON CONFLICT (season, season_type) DO UPDATE SET
            synced_at = excluded.synced_at,
            games = excluded.games,
            players = excluded.players",
        params![season, season_type, synced_at, total_games as i64, total_players as i64],
    )?;
    transaction.commit()?;
    Ok(SyncReport {
        season: season.to_string(),
        season_type: season_type.to_string(),
        games: total_games,
        players: total_players,
        fetched: games.len(),
        synced_at: Some(synced_at),
        warning: None,
        roster_players: None,
    })
}

fn cached_counts(connection: &Connection, season: &str, season_type: &str) -> AppResult<(usize, usize)> {
    let (games, players): (i64, i64) = connection.query_row(
        "SELECT COUNT(*), COUNT(DISTINCT player_id) FROM game_logs WHERE season = ?1 AND season_type = ?2",
        params![season, season_type],
        |row| Ok((row.get(0)?, row.get(1)?)),
    )?;
    Ok((games.max(0) as usize, players.max(0) as usize))
}

fn synced_at(connection: &Connection, season: &str, season_type: &str) -> AppResult<Option<String>> {
    let mut statement =
        connection.prepare("SELECT synced_at FROM sync_meta WHERE season = ?1 AND season_type = ?2")?;
    let mut rows = statement.query(params![season, season_type])?;
    match rows.next()? {
        Some(row) => Ok(Some(row.get(0)?)),
        None => Ok(None),
    }
}

pub fn statuses(connection: &Connection) -> AppResult<Vec<SeasonStatus>> {
    let mut statement = connection.prepare(
        "SELECT season, season_type, games, players, synced_at,
                (SELECT MIN(game_date) FROM game_logs
                  WHERE game_logs.season = sync_meta.season
                    AND game_logs.season_type = sync_meta.season_type),
                (SELECT MAX(game_date) FROM game_logs
                  WHERE game_logs.season = sync_meta.season
                    AND game_logs.season_type = sync_meta.season_type)
         FROM sync_meta
         ORDER BY season, season_type",
    )?;
    let rows = statement.query_map([], |row| {
        Ok(SeasonStatus {
            season: row.get(0)?,
            season_type: row.get(1)?,
            games: row.get(2)?,
            players: row.get(3)?,
            synced_at: Some(row.get(4)?),
            first_game: row.get(5)?,
            last_game: row.get(6)?,
        })
    })?;
    rows.collect::<Result<Vec<_>, _>>().map_err(AppError::from)
}

pub fn players(connection: &Connection, season: &str, season_type: &str) -> AppResult<Vec<PlayerOption>> {
    let mut statement = connection.prepare(
        "SELECT player_id, player_name, team_abbr, games
         FROM (
            SELECT player_id,
                   player_name,
                   team_abbr,
                   COUNT(*) OVER (PARTITION BY player_id) AS games,
                   ROW_NUMBER() OVER (
                       PARTITION BY player_id
                       ORDER BY game_date DESC, game_id DESC
                   ) AS rn
            FROM game_logs
            WHERE season = ?1 AND season_type = ?2
         )
         WHERE rn = 1
         ORDER BY player_name COLLATE NOCASE",
    )?;
    let rows = statement.query_map(params![season, season_type], |row| {
        Ok(PlayerOption {
            player_id: row.get(0)?,
            name: row.get(1)?,
            team: row.get(2)?,
            games: row.get(3)?,
            team_source: TeamSource::Season,
        })
    })?;
    rows.collect::<Result<Vec<_>, _>>().map_err(AppError::from)
}

/// The player list with carry on: this season's players, then players from the season it
/// carries who have no game yet, then anyone else on this season's rosters. With a stored
/// roster, carried players who are on no roster are dropped and teams come from the roster.
pub fn players_with_carry(
    connection: &Connection,
    season: &str,
    season_type: &str,
) -> AppResult<Vec<PlayerOption>> {
    let current = players(connection, season, season_type)?;
    let carried = match season::seed_source(season, season_type) {
        Some((seed_season, seed_type)) => players(connection, &seed_season, &seed_type)?,
        None => Vec::new(),
    };
    let roster = roster(connection, season)?;
    Ok(merge_players(current, carried, &roster))
}

/// Unions the three lists. A player keeps the first entry he has: this season, then carried, then roster.
pub fn merge_players(
    current: Vec<PlayerOption>,
    carried: Vec<PlayerOption>,
    roster: &[RosterEntry],
) -> Vec<PlayerOption> {
    let on_roster: HashMap<i64, &RosterEntry> = roster.iter().map(|entry| (entry.player_id, entry)).collect();
    let mut seen: HashSet<i64> = current.iter().map(|player| player.player_id).collect();
    let mut merged = current;
    for player in carried {
        if !seen.insert(player.player_id) {
            continue;
        }
        let entry = if roster.is_empty() {
            Some(PlayerOption {
                games: 0,
                team_source: TeamSource::LastSeason,
                ..player
            })
        } else {
            on_roster.get(&player.player_id).map(|listed| PlayerOption {
                team: listed.team_abbr.clone(),
                games: 0,
                team_source: TeamSource::Roster,
                ..player
            })
        };
        merged.extend(entry);
    }
    for listed in roster {
        if seen.insert(listed.player_id) {
            merged.push(PlayerOption {
                player_id: listed.player_id,
                name: listed.name.clone(),
                team: listed.team_abbr.clone(),
                games: 0,
                team_source: TeamSource::Roster,
            });
        }
    }
    merged.sort_by(|left, right| {
        left.name
            .to_lowercase()
            .cmp(&right.name.to_lowercase())
            .then(left.player_id.cmp(&right.player_id))
    });
    merged
}

/// Replaces the stored roster for `season`. An answer under `MIN_ROSTER_PLAYERS` keeps the old one
/// and returns None.
pub fn save_roster(connection: &Connection, season: &str, entries: &[RosterEntry]) -> AppResult<Option<usize>> {
    if entries.len() < MIN_ROSTER_PLAYERS {
        return Ok(None);
    }
    let fetched_at = Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string();
    let transaction = connection.unchecked_transaction()?;
    transaction.execute("DELETE FROM rosters WHERE season = ?1", params![season])?;
    {
        let mut statement = transaction.prepare(
            "INSERT OR REPLACE INTO rosters (season, player_id, player_name, team_abbr, fetched_at)
             VALUES (?1, ?2, ?3, ?4, ?5)",
        )?;
        for entry in entries {
            statement.execute(params![season, entry.player_id, entry.name, entry.team_abbr, fetched_at])?;
        }
    }
    transaction.commit()?;
    Ok(Some(entries.len()))
}

pub fn roster(connection: &Connection, season: &str) -> AppResult<Vec<RosterEntry>> {
    let mut statement = connection.prepare(
        "SELECT player_id, player_name, team_abbr FROM rosters WHERE season = ?1 ORDER BY player_id",
    )?;
    let rows = statement.query_map(params![season], |row| {
        Ok(RosterEntry {
            player_id: row.get(0)?,
            name: row.get(1)?,
            team_abbr: row.get(2)?,
        })
    })?;
    rows.collect::<Result<Vec<_>, _>>().map_err(AppError::from)
}

pub fn player_games(
    connection: &Connection,
    season: &str,
    season_type: &str,
    player_id: i64,
) -> AppResult<Vec<GameLog>> {
    let mut statement = connection.prepare(
        "SELECT player_id, player_name, team_abbr, game_id, game_date, matchup, wl, minutes,
                pts, reb, ast, stl, blk, tov, fgm, fga, fg3m, ftm, plus_minus
         FROM game_logs
         WHERE season = ?1 AND season_type = ?2 AND player_id = ?3
         ORDER BY game_date ASC, game_id ASC",
    )?;
    let rows = statement.query_map(params![season, season_type, player_id], map_game)?;
    rows.collect::<Result<Vec<_>, _>>().map_err(AppError::from)
}

pub fn season_games(connection: &Connection, season: &str, season_type: &str) -> AppResult<Vec<GameLog>> {
    let mut statement = connection.prepare(
        "SELECT player_id, player_name, team_abbr, game_id, game_date, matchup, wl, minutes,
                pts, reb, ast, stl, blk, tov, fgm, fga, fg3m, ftm, plus_minus
         FROM game_logs
         WHERE season = ?1 AND season_type = ?2
         ORDER BY player_id ASC, game_date ASC, game_id ASC",
    )?;
    let rows = statement.query_map(params![season, season_type], map_game)?;
    rows.collect::<Result<Vec<_>, _>>().map_err(AppError::from)
}

fn map_game(row: &rusqlite::Row<'_>) -> rusqlite::Result<GameLog> {
    Ok(GameLog {
        player_id: row.get(0)?,
        player_name: row.get(1)?,
        team_abbr: row.get(2)?,
        game_id: row.get(3)?,
        game_date: row.get(4)?,
        matchup: row.get(5)?,
        wl: row.get(6)?,
        minutes: row.get(7)?,
        pts: row.get(8)?,
        reb: row.get(9)?,
        ast: row.get(10)?,
        stl: row.get(11)?,
        blk: row.get(12)?,
        tov: row.get(13)?,
        fgm: row.get(14)?,
        fga: row.get(15)?,
        fg3m: row.get(16)?,
        ftm: row.get(17)?,
        plus_minus: row.get(18)?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample(id: i64, name: &str, team: &str, date: &str, pts: i32) -> GameLog {
        GameLog {
            player_id: id,
            player_name: name.to_string(),
            team_abbr: team.to_string(),
            game_id: format!("{id}-{date}"),
            game_date: date.to_string(),
            matchup: format!("{team} vs. LAL"),
            wl: "W".to_string(),
            minutes: 30.0,
            pts,
            reb: 8,
            ast: 6,
            stl: 1,
            blk: 1,
            tov: 2,
            fgm: 8,
            fga: 15,
            fg3m: 1,
            ftm: 4,
            plus_minus: 4,
        }
    }

    fn roster_of(extra: &[(i64, &str, &str)]) -> Vec<RosterEntry> {
        let mut entries: Vec<RosterEntry> = (1000..1000 + MIN_ROSTER_PLAYERS as i64)
            .map(|id| RosterEntry {
                player_id: id,
                name: format!("Depth {id}"),
                team_abbr: "UTA".to_string(),
            })
            .collect();
        for (id, name, team) in extra {
            entries.push(RosterEntry {
                player_id: *id,
                name: name.to_string(),
                team_abbr: team.to_string(),
            });
        }
        entries
    }

    fn opening_night() -> Connection {
        let connection = open_memory().unwrap();
        let last = vec![
            sample(7, "A Player", "DEN", "2025-01-01", 20),
            sample(7, "A Player", "MIN", "2025-02-01", 30),
            sample(8, "B Player", "BOS", "2025-01-04", 18),
            sample(9, "C Player", "LAL", "2025-01-04", 12),
        ];
        merge_logs(&connection, "2024-25", "Regular Season", &last).unwrap();
        merge_logs(&connection, "2025-26", "Regular Season", &[sample(7, "A Player", "MIN", "2025-10-22", 25)])
            .unwrap();
        connection
    }

    #[test]
    fn carried_players_join_the_list_with_last_seasons_team() {
        let connection = opening_night();
        let plain = players(&connection, "2025-26", "Regular Season").unwrap();
        assert_eq!(plain.len(), 1);
        let listed = players_with_carry(&connection, "2025-26", "Regular Season").unwrap();
        let names: Vec<_> = listed.iter().map(|player| (player.player_id, player.team.as_str(), player.games)).collect();
        assert_eq!(names, vec![(7, "MIN", 1), (8, "BOS", 0), (9, "LAL", 0)]);
        assert_eq!(listed[0].team_source, TeamSource::Season);
        assert_eq!(listed[1].team_source, TeamSource::LastSeason);
        // Playoffs carry their regular season, so every regular-season player is listed.
        let playoffs = players_with_carry(&connection, "2025-26", "Playoffs").unwrap();
        assert_eq!(playoffs.len(), 1);
        assert_eq!(playoffs[0].team_source, TeamSource::LastSeason);
    }

    #[test]
    fn a_stored_roster_moves_teams_drops_the_gone_and_adds_rookies() {
        let connection = opening_night();
        let entries = roster_of(&[(8, "B Player", "PHX"), (20, "D Rookie", "SAS")]);
        assert_eq!(save_roster(&connection, "2025-26", &entries).unwrap(), Some(entries.len()));
        let listed = players_with_carry(&connection, "2025-26", "Regular Season").unwrap();
        let find = |id: i64| listed.iter().find(|player| player.player_id == id);
        assert_eq!(find(7).unwrap().team, "MIN", "a player with games keeps his game team");
        assert_eq!(find(8).map(|player| (player.team.as_str(), player.team_source)), Some(("PHX", TeamSource::Roster)));
        assert!(find(9).is_none(), "on no roster, so he is gone");
        let rookie = find(20).unwrap();
        assert_eq!((rookie.games, rookie.team_source), (0, TeamSource::Roster));
        assert_eq!(listed.len(), 3 + MIN_ROSTER_PLAYERS);
        // A thin answer keeps the stored roster.
        assert_eq!(save_roster(&connection, "2025-26", &entries[..10]).unwrap(), None);
        assert_eq!(roster(&connection, "2025-26").unwrap().len(), entries.len());
    }

    #[test]
    fn latest_team_wins_and_a_resync_updates_the_season() {
        let connection = open_memory().unwrap();
        let first = vec![
            sample(7, "A Player", "DEN", "2026-01-01", 20),
            sample(7, "A Player", "MIN", "2026-02-01", 30),
            sample(8, "B Player", "BOS", "2026-01-04", 18),
        ];
        let report = merge_logs(&connection, "2025-26", "Regular Season", &first).unwrap();
        assert_eq!(report.games, 3);
        assert_eq!(report.players, 2);
        assert!(report.warning.is_none());
        let listed = players(&connection, "2025-26", "Regular Season").unwrap();
        assert_eq!(listed[0].team, "MIN");
        assert_eq!(listed[0].games, 2);

        let mut corrected = first.clone();
        corrected[0].pts = 22;
        corrected.push(sample(7, "A Player", "MIN", "2026-03-01", 12));
        let report = merge_logs(&connection, "2025-26", "Regular Season", &corrected).unwrap();
        assert_eq!((report.games, report.fetched), (4, 4));
        let games = player_games(&connection, "2025-26", "Regular Season", 7).unwrap();
        assert_eq!(games.len(), 3);
        assert_eq!(games[0].pts, 22);
        assert_eq!(games[2].pts, 12);
        let status = statuses(&connection).unwrap();
        assert_eq!(status[0].games, 4);
        assert_eq!(status[0].players, 2);
        assert_eq!(status[0].first_game.as_deref(), Some("2026-01-01"));
        assert_eq!(status[0].last_game.as_deref(), Some("2026-03-01"));
    }

    fn season_of(count: usize) -> Vec<GameLog> {
        (0..count)
            .map(|day| {
                let date = format!("2026-01-{:02}", day % 28 + 1);
                let mut game = sample(day as i64 / 28 + 1, "P", "DEN", &date, 10 + day as i32);
                game.game_id = format!("g{day}");
                game
            })
            .collect()
    }

    #[test]
    fn an_empty_sync_keeps_the_cache() {
        let connection = open_memory().unwrap();
        merge_logs(&connection, "2025-26", "Regular Season", &season_of(40)).unwrap();
        let before = statuses(&connection).unwrap();
        let report = merge_logs(&connection, "2025-26", "Regular Season", &[]).unwrap();
        let warning = report.warning.expect("an empty sync warns");
        assert!(warning.contains("0 rows") && warning.contains("kept"), "{warning}");
        assert_eq!((report.games, report.fetched), (40, 0));
        assert_eq!(report.synced_at, before[0].synced_at);
        assert_eq!(season_games(&connection, "2025-26", "Regular Season").unwrap().len(), 40);
        assert_eq!(statuses(&connection).unwrap()[0].games, 40);
    }

    #[test]
    fn an_empty_sync_of_an_empty_season_writes_nothing() {
        let connection = open_memory().unwrap();
        let report = merge_logs(&connection, "2025-26", "Playoffs", &[]).unwrap();
        assert!(report.warning.unwrap().contains("Nothing was cached"));
        assert_eq!(report.synced_at, None);
        assert!(statuses(&connection).unwrap().is_empty());
    }

    #[test]
    fn a_short_sync_keeps_the_cache() {
        let connection = open_memory().unwrap();
        let full = season_of(40);
        merge_logs(&connection, "2025-26", "Regular Season", &full).unwrap();
        let mut short = full[..15].to_vec();
        short[0].pts = 99;
        let report = merge_logs(&connection, "2025-26", "Regular Season", &short).unwrap();
        assert!(report.warning.unwrap().contains("under half"));
        assert_eq!((report.games, report.fetched), (40, 15));
        let games = season_games(&connection, "2025-26", "Regular Season").unwrap();
        assert_eq!(games.len(), 40);
        assert!(games.iter().all(|game| game.pts != 99), "a short sync writes nothing");
    }

    #[test]
    fn a_partial_sync_merges_into_the_cache() {
        let connection = open_memory().unwrap();
        let full = season_of(40);
        merge_logs(&connection, "2025-26", "Regular Season", &full).unwrap();
        let mut partial = full[..30].to_vec();
        partial[0].pts = 77;
        let mut extra = sample(9, "C Player", "BOS", "2026-02-01", 31);
        extra.game_id = "g-new".to_string();
        partial.push(extra);
        let report = merge_logs(&connection, "2025-26", "Regular Season", &partial).unwrap();
        assert!(report.warning.is_none());
        assert_eq!((report.games, report.fetched), (41, 31));
        let games = season_games(&connection, "2025-26", "Regular Season").unwrap();
        assert_eq!(games.len(), 41);
        assert!(games.iter().any(|game| game.pts == 77), "the response row replaced the cached one");
        assert!(games.iter().any(|game| game.game_id == "g39"), "rows the response left out stay");
        assert_eq!(statuses(&connection).unwrap()[0].games, 41);
    }

    #[test]
    fn seasons_and_types_do_not_touch_each_other() {
        let connection = open_memory().unwrap();
        merge_logs(&connection, "2025-26", "Regular Season", &season_of(40)).unwrap();
        merge_logs(&connection, "2025-26", "Playoffs", &season_of(3)).unwrap();
        assert_eq!(season_games(&connection, "2025-26", "Regular Season").unwrap().len(), 40);
        assert_eq!(season_games(&connection, "2025-26", "Playoffs").unwrap().len(), 3);
    }
}
