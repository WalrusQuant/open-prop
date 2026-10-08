use chrono::Utc;
use rusqlite::{params, Connection};

use crate::error::{AppError, AppResult};
use crate::models::{GameLog, PlayerOption, SeasonStatus, SyncReport};

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
";

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

pub fn replace_logs(
    connection: &Connection,
    season: &str,
    season_type: &str,
    games: &[GameLog],
) -> AppResult<SyncReport> {
    let synced_at = Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string();
    let players = {
        let mut ids: Vec<i64> = games.iter().map(|game| game.player_id).collect();
        ids.sort_unstable();
        ids.dedup();
        ids.len()
    };
    let transaction = connection.unchecked_transaction()?;
    transaction.execute(
        "DELETE FROM game_logs WHERE season = ?1 AND season_type = ?2",
        params![season, season_type],
    )?;
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
    transaction.execute(
        "INSERT INTO sync_meta (season, season_type, synced_at, games, players)
         VALUES (?1, ?2, ?3, ?4, ?5)
         ON CONFLICT (season, season_type) DO UPDATE SET
            synced_at = excluded.synced_at,
            games = excluded.games,
            players = excluded.players",
        params![season, season_type, synced_at, games.len() as i64, players as i64],
    )?;
    transaction.commit()?;
    Ok(SyncReport {
        season: season.to_string(),
        season_type: season_type.to_string(),
        games: games.len(),
        players,
        synced_at,
    })
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

    #[test]
    fn latest_team_wins_and_a_resync_replaces_the_season() {
        let connection = open_memory().unwrap();
        let first = vec![
            sample(7, "A Player", "DEN", "2026-01-01", 20),
            sample(7, "A Player", "MIN", "2026-02-01", 30),
            sample(8, "B Player", "BOS", "2026-01-04", 18),
        ];
        let report = replace_logs(&connection, "2025-26", "Regular Season", &first).unwrap();
        assert_eq!(report.games, 3);
        assert_eq!(report.players, 2);
        let listed = players(&connection, "2025-26", "Regular Season").unwrap();
        assert_eq!(listed[0].team, "MIN");
        assert_eq!(listed[0].games, 2);

        replace_logs(
            &connection,
            "2025-26",
            "Regular Season",
            &[sample(7, "A Player", "DEN", "2026-03-01", 12)],
        )
        .unwrap();
        let games = player_games(&connection, "2025-26", "Regular Season", 7).unwrap();
        assert_eq!(games.len(), 1);
        assert_eq!(games[0].pts, 12);
        let status = statuses(&connection).unwrap();
        assert_eq!(status[0].games, 1);
        assert_eq!(status[0].players, 1);
        assert_eq!(status[0].first_game.as_deref(), Some("2026-03-01"));
        assert_eq!(status[0].last_game.as_deref(), Some("2026-03-01"));
    }
}
