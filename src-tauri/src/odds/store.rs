//! Odds snapshots in SQLite. Every pull writes its rows under one `pulled_at`, so line movement
//! builds up from whatever the app happened to pull. Kalshi rows use `bookmaker_key = 'kalshi'`.

use rusqlite::{params, Connection, OptionalExtension};
use serde::Serialize;

use super::Quote;
use crate::error::{AppError, AppResult};

pub const SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS odds_snapshots (
    id INTEGER PRIMARY KEY,
    pulled_at TEXT NOT NULL,
    event_id TEXT NOT NULL,
    bookmaker_key TEXT NOT NULL,
    region_key TEXT,
    book_kind TEXT,
    market_key TEXT NOT NULL,
    market_ticker TEXT,
    player_name TEXT NOT NULL,
    player_team TEXT,
    player_id INTEGER,
    name TEXT NOT NULL,
    price REAL,
    odds_format TEXT NOT NULL,
    point REAL,
    bookmaker_last_update TEXT,
    bid REAL,
    ask REAL,
    volume REAL,
    open_interest REAL,
    game_time TEXT,
    UNIQUE (pulled_at, event_id, bookmaker_key, market_key, player_name, name, point)
);
CREATE INDEX IF NOT EXISTS odds_snapshots_latest ON odds_snapshots (bookmaker_key, pulled_at);
";

pub fn init(connection: &Connection) -> AppResult<()> {
    connection.execute_batch(SCHEMA)?;
    Ok(())
}

/// Writes one pull in a single short transaction. `player_ids` lines up with `quotes`.
pub fn save_pull(connection: &Connection, pulled_at: &str, quotes: &[Quote], player_ids: &[Option<i64>]) -> AppResult<usize> {
    if quotes.len() != player_ids.len() {
        return Err(AppError::message("every quote needs a match result"));
    }
    let transaction = connection.unchecked_transaction()?;
    {
        let mut statement = transaction.prepare(
            "INSERT OR REPLACE INTO odds_snapshots (
                pulled_at, event_id, bookmaker_key, region_key, book_kind, market_key, market_ticker,
                player_name, player_team, player_id, name, price, odds_format, point,
                bid, ask, volume, open_interest, game_time
            ) VALUES (?1, ?2, ?3, NULL, 'exchange', ?4, ?5, ?6, ?7, ?8, 'yes', ?9, 'prob', ?10, ?11, ?12, ?13, ?14, ?15)",
        )?;
        for (quote, player_id) in quotes.iter().zip(player_ids) {
            statement.execute(params![
                pulled_at,
                quote.event_ticker,
                quote.book,
                quote.stat,
                quote.market_ticker,
                quote.player_name,
                quote.team,
                player_id,
                quote.yes_ask,
                quote.threshold,
                quote.yes_bid,
                quote.yes_ask,
                quote.volume,
                quote.open_interest,
                quote.game_time,
            ])?;
        }
    }
    transaction.commit()?;
    Ok(quotes.len())
}

/// One stored rung as the screens read it.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StoredQuote {
    pub market_ticker: String,
    pub event_ticker: String,
    pub stat: String,
    pub player_name: String,
    pub team: Option<String>,
    pub player_id: Option<i64>,
    pub threshold: f64,
    pub yes_bid: Option<f64>,
    pub yes_ask: Option<f64>,
    pub mid: Option<f64>,
    pub spread: Option<f64>,
    pub volume: f64,
    pub open_interest: f64,
    pub thin: bool,
    pub game_time: Option<String>,
}

pub fn latest_pulled_at(connection: &Connection, book: &str) -> AppResult<Option<String>> {
    Ok(connection
        .query_row(
            "SELECT MAX(pulled_at) FROM odds_snapshots WHERE bookmaker_key = ?1",
            params![book],
            |row| row.get::<_, Option<String>>(0),
        )
        .optional()?
        .flatten())
}

/// The rows from the most recent pull of `book`, optionally for one stat or player.
pub fn latest(connection: &Connection, book: &str, stat: Option<&str>, player_id: Option<i64>) -> AppResult<(Option<String>, Vec<StoredQuote>)> {
    let Some(pulled_at) = latest_pulled_at(connection, book)? else {
        return Ok((None, Vec::new()));
    };
    let mut statement = connection.prepare(
        "SELECT market_ticker, event_id, market_key, player_name, player_team, player_id, point,
                bid, ask, volume, open_interest, game_time
         FROM odds_snapshots
         WHERE bookmaker_key = ?1 AND pulled_at = ?2
           AND (?3 IS NULL OR market_key = ?3)
           AND (?4 IS NULL OR player_id = ?4)
         ORDER BY market_key, player_name, point",
    )?;
    let rows = statement.query_map(params![book, pulled_at, stat, player_id], |row| {
        let yes_bid: Option<f64> = row.get(7)?;
        let yes_ask: Option<f64> = row.get(8)?;
        let volume: f64 = row.get::<_, Option<f64>>(9)?.unwrap_or(0.0);
        let quote = Quote {
            book: book.to_string(),
            event_ticker: row.get(1)?,
            market_ticker: row.get::<_, Option<String>>(0)?.unwrap_or_default(),
            stat: row.get(2)?,
            player_name: row.get(3)?,
            team: row.get(4)?,
            threshold: row.get::<_, Option<f64>>(6)?.unwrap_or(0.0),
            floor_strike: 0.0,
            yes_bid,
            yes_ask,
            volume,
            open_interest: row.get::<_, Option<f64>>(10)?.unwrap_or(0.0),
            game_time: row.get(11)?,
            status: String::new(),
        };
        Ok(StoredQuote {
            mid: quote.mid(),
            spread: quote.spread(),
            thin: quote.thin(),
            market_ticker: quote.market_ticker,
            event_ticker: quote.event_ticker,
            stat: quote.stat,
            player_name: quote.player_name,
            team: quote.team,
            player_id: row.get(5)?,
            threshold: quote.threshold,
            yes_bid,
            yes_ask,
            volume,
            open_interest: quote.open_interest,
            game_time: quote.game_time,
        })
    })?;
    Ok((Some(pulled_at), rows.collect::<Result<Vec<_>, _>>()?))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn quote(player: &str, threshold: f64, ask: Option<f64>) -> Quote {
        Quote {
            book: "kalshi".into(),
            event_ticker: "KXNBAPTS-26OCT21NYKBOS".into(),
            market_ticker: format!("KXNBAPTS-26OCT21NYKBOS-{player}-{threshold}"),
            stat: "points".into(),
            player_name: player.into(),
            team: Some("BOS".into()),
            threshold,
            floor_strike: threshold - 0.5,
            yes_bid: ask.map(|a| a - 0.02),
            yes_ask: ask,
            volume: 500.0,
            open_interest: 100.0,
            game_time: Some("2026-10-21T23:30:00Z".into()),
            status: "active".into(),
        }
    }

    #[test]
    fn latest_reads_only_the_newest_pull() {
        let connection = crate::db::open_memory().unwrap();
        assert_eq!(latest(&connection, "kalshi", None, None).unwrap(), (None, vec![]));
        save_pull(&connection, "2026-10-21T18:00:00Z", &[quote("Tatum", 25.0, Some(0.50))], &[Some(1)]).unwrap();
        save_pull(
            &connection,
            "2026-10-21T19:00:00Z",
            &[quote("Tatum", 25.0, Some(0.55)), quote("Tatum", 30.0, None), quote("Brown", 20.0, Some(0.6))],
            &[Some(1), Some(1), None],
        )
        .unwrap();
        let (pulled_at, rows) = latest(&connection, "kalshi", Some("points"), Some(1)).unwrap();
        assert_eq!(pulled_at.as_deref(), Some("2026-10-21T19:00:00Z"));
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].threshold, 25.0);
        assert_eq!(rows[0].yes_ask, Some(0.55));
        assert!((rows[0].mid.unwrap() - 0.54).abs() < 1e-12);
        assert!(rows[1].thin, "no offer");
        let (_, all) = latest(&connection, "kalshi", None, None).unwrap();
        assert_eq!(all.len(), 3);
        assert!(all.iter().any(|row| row.player_id.is_none()), "unmatched rows are kept");
        let history: i64 = connection.query_row("SELECT COUNT(*) FROM odds_snapshots", [], |r| r.get(0)).unwrap();
        assert_eq!(history, 4, "older pulls stay for line history");
    }

    #[test]
    fn mismatched_lengths_are_refused() {
        let connection = crate::db::open_memory().unwrap();
        assert!(save_pull(&connection, "t", &[quote("A", 10.0, None)], &[]).is_err());
    }
}
