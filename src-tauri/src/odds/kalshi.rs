//! Kalshi's public market data. Reads only: `/events` and `/markets` on trade-api v2 need no
//! key, so this provider never takes one and never calls an order or portfolio endpoint.

use std::future::Future;
use std::time::Duration;

use chrono::{DateTime, Utc};
use serde_json::Value;
use wreq::Client;

use super::{OddsProvider, Pull, Quote};
use crate::error::{AppError, AppResult};

pub const BASE_URL: &str = "https://api.elections.kalshi.com/trade-api/v2";
/// Stop following a cursor after this many pages so a bad cursor cannot loop.
const MAX_PAGES: usize = 50;
const PAGE_LIMIT: &str = "200";
/// Space requests so a slate never bursts.
const REQUEST_GAP: Duration = Duration::from_millis(120);

/// Kalshi series and the open-prop stat each one prices. TOV, FGM, and FGA have no series.
pub const SERIES: &[(&str, &str)] = &[
    ("KXNBAPTS", "points"),
    ("KXNBAREB", "rebounds"),
    ("KXNBAAST", "assists"),
    ("KXNBA3PT", "three_point_field_goals_made"),
    ("KXNBASTL", "steals"),
    ("KXNBABLK", "blocks"),
    ("KXNBAFTM", "free_throws_made"),
    ("KXNBAPRA", "points_assists_rebounds"),
    ("KXNBAPA", "points_assists"),
    ("KXNBAPR", "points_rebounds"),
    ("KXNBARA", "assists_rebounds"),
];

#[cfg(test)]
pub fn stat_for_series(series: &str) -> Option<&'static str> {
    SERIES.iter().find(|(ticker, _)| *ticker == series).map(|(_, stat)| *stat)
}

/// Kalshi sends prices as dollar strings. 0 means no bid and 1 means no offer.
fn price(value: Option<&Value>) -> Option<f64> {
    let number = number(value)?;
    (number > 0.0 && number < 1.0).then_some(number)
}

fn number(value: Option<&Value>) -> Option<f64> {
    match value? {
        Value::String(text) => text.trim().parse().ok(),
        Value::Number(number) => number.as_f64(),
        _ => None,
    }
}

/// `KXNBAPTS-26OCT08BOSCLE` -> `BOSCLE`. The date is always `YYMMMDD`.
fn matchup_codes(event_ticker: &str) -> Option<&str> {
    let tail = event_ticker.split('-').nth(1)?;
    tail.get(7..).filter(|codes| !codes.is_empty())
}

/// Finds which side of `BOSCLE` the player segment `BOSJTATUM0` starts with.
pub fn team_from_tickers(event_ticker: &str, market_ticker: &str) -> Option<String> {
    let codes = matchup_codes(event_ticker)?;
    let player = market_ticker.split('-').nth(2)?;
    // Three-letter codes are the norm; two and four cover the odd franchise code.
    for split in [3, 2, 4] {
        if split >= codes.len() {
            continue;
        }
        let (first, second) = codes.split_at(split);
        if second.len() < 2 || second.len() > 4 {
            continue;
        }
        if player.starts_with(first) {
            return Some(first.to_string());
        }
        if player.starts_with(second) {
            return Some(second.to_string());
        }
    }
    None
}

/// `"Paul George: 25+ points"` -> `"Paul George"`.
fn player_from_title(title: &str) -> Option<String> {
    let name = title.split(':').next()?.trim();
    (!name.is_empty() && name.len() < title.len()).then(|| name.to_string())
}

/// One `/markets` row as a quote. Anything that is not a `greater` ladder rung is skipped.
pub fn parse_market(market: &Value, stat: &str) -> Option<Quote> {
    if market.get("strike_type")?.as_str()? != "greater" {
        return None;
    }
    let floor_strike = number(market.get("floor_strike"))?;
    let market_ticker = market.get("ticker")?.as_str()?.to_string();
    let event_ticker = market.get("event_ticker")?.as_str()?.to_string();
    let player_name = player_from_title(market.get("title")?.as_str()?)?;
    Some(Quote {
        book: "kalshi".to_string(),
        team: team_from_tickers(&event_ticker, &market_ticker),
        event_ticker,
        market_ticker,
        stat: stat.to_string(),
        player_name,
        threshold: floor_strike.floor() + 1.0,
        floor_strike,
        yes_bid: price(market.get("yes_bid_dollars")),
        yes_ask: price(market.get("yes_ask_dollars")),
        volume: number(market.get("volume_fp")).unwrap_or(0.0),
        open_interest: number(market.get("open_interest_fp")).unwrap_or(0.0),
        game_time: market
            .get("occurrence_datetime")
            .and_then(Value::as_str)
            .map(str::to_string),
        status: market.get("status").and_then(Value::as_str).unwrap_or("").to_string(),
    })
}

pub fn parse_markets(page: &Value, stat: &str) -> Vec<Quote> {
    page.get("markets")
        .and_then(Value::as_array)
        .map(|markets| markets.iter().filter_map(|market| parse_market(market, stat)).collect())
        .unwrap_or_default()
}

pub fn event_tickers(page: &Value) -> Vec<String> {
    page.get("events")
        .and_then(Value::as_array)
        .map(|events| {
            events
                .iter()
                .filter_map(|event| event.get("event_ticker")?.as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default()
}

fn next_cursor(page: &Value) -> Option<String> {
    page.get("cursor")
        .and_then(Value::as_str)
        .filter(|cursor| !cursor.is_empty())
        .map(str::to_string)
}

/// Follows `cursor` until it comes back empty. `fetch` gets the cursor for the next page.
pub async fn paginate<F, Fut>(mut fetch: F) -> AppResult<Vec<Value>>
where
    F: FnMut(Option<String>) -> Fut,
    Fut: Future<Output = AppResult<Value>>,
{
    let mut pages = Vec::new();
    let mut cursor = None;
    loop {
        let page = fetch(cursor.clone()).await?;
        let next = next_cursor(&page);
        pages.push(page);
        match next {
            Some(value) if Some(&value) != cursor.as_ref() && pages.len() < MAX_PAGES => cursor = Some(value),
            _ => break,
        }
    }
    Ok(pages)
}

/// Open for trading and not yet tipped. A rung with no tip time is kept while it is active.
pub fn is_pregame(quote: &Quote, now: DateTime<Utc>) -> bool {
    if quote.status != "active" {
        return false;
    }
    match quote.game_time.as_deref().and_then(|time| DateTime::parse_from_rfc3339(time).ok()) {
        Some(tip) => tip.with_timezone(&Utc) > now,
        None => true,
    }
}

pub struct KalshiProvider {
    http: Client,
    base: String,
}

impl KalshiProvider {
    pub fn new() -> AppResult<Self> {
        let http = Client::builder()
            .timeout(Duration::from_secs(30))
            .connect_timeout(Duration::from_secs(15))
            .build()?;
        Ok(Self { http, base: BASE_URL.to_string() })
    }

    async fn get(&self, path: &str, query: &[(&str, &str)]) -> AppResult<Value> {
        let response = self
            .http
            .get(format!("{}{path}", self.base))
            .query(query)
            .header("accept", "application/json")
            .send()
            .await?;
        let status = response.status();
        let body = response.text().await?;
        if !status.is_success() {
            let short: String = body.chars().take(200).collect();
            return Err(AppError::message(format!("Kalshi answered {status} for {path}: {short}")));
        }
        Ok(serde_json::from_str(&body)?)
    }

    async fn pages(&self, path: &str, filter: (&str, &str), status: Option<&str>, requests: &mut usize) -> AppResult<Vec<Value>> {
        let mut count = 0;
        let pages = paginate(|cursor| {
            count += 1;
            async move {
                tokio::time::sleep(REQUEST_GAP).await;
                let mut query = vec![filter, ("limit", PAGE_LIMIT)];
                if let Some(status) = status {
                    query.push(("status", status));
                }
                if let Some(cursor) = cursor.as_deref() {
                    query.push(("cursor", cursor));
                }
                self.get(path, &query).await
            }
        })
        .await?;
        *requests += count;
        Ok(pages)
    }
}

impl OddsProvider for KalshiProvider {
    fn id(&self) -> &'static str {
        "kalshi"
    }

    fn needs_key(&self) -> bool {
        false
    }

    async fn pull(&self, now: DateTime<Utc>) -> AppResult<Pull> {
        let mut pull = Pull::default();
        for (series, stat) in SERIES {
            let events = self
                .pages("/events", ("series_ticker", series), Some("open"), &mut pull.requests)
                .await?;
            for event in events.iter().flat_map(event_tickers) {
                pull.events += 1;
                let markets = self
                    .pages("/markets", ("event_ticker", event.as_str()), None, &mut pull.requests)
                    .await?;
                pull.quotes.extend(
                    markets
                        .iter()
                        .flat_map(|page| parse_markets(page, stat))
                        .filter(|quote| is_pregame(quote, now)),
                );
            }
        }
        Ok(pull)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture(name: &str) -> Value {
        let path = format!("{}/fixtures/kalshi/{name}", env!("CARGO_MANIFEST_DIR"));
        serde_json::from_str(&std::fs::read_to_string(path).expect("fixture")).expect("json")
    }

    #[test]
    fn settled_points_ladder_parses() {
        let quotes = parse_markets(&fixture("markets_pts_bos_cle_settled.json"), "points");
        assert_eq!(quotes.len(), 8);
        let george = quotes
            .iter()
            .find(|quote| quote.market_ticker == "KXNBAPTS-26OCT08BOSCLE-BOSPGEORGE13-25")
            .expect("George 25+");
        assert_eq!(george.player_name, "Paul George");
        assert_eq!(george.team.as_deref(), Some("BOS"));
        assert_eq!(george.floor_strike, 24.5);
        assert_eq!(george.threshold, 25.0);
        assert_eq!(george.stat, "points");
        assert_eq!(george.status, "finalized");
        assert_eq!(george.game_time.as_deref(), Some("2026-10-09T02:00:00Z"));
        // Settled: 0 bid and 1 ask read as an empty book, so the rung is thin.
        assert_eq!((george.yes_bid, george.yes_ask), (None, None));
        assert!(george.thin());
        let ladder: Vec<f64> = {
            let mut rungs: Vec<f64> = quotes
                .iter()
                .filter(|quote| quote.player_name == "Jayson Tatum")
                .map(|quote| quote.threshold)
                .collect();
            rungs.sort_by(f64::total_cmp);
            rungs
        };
        assert_eq!(ladder, vec![10.0, 15.0, 20.0, 25.0]);
        let tatum_20 = quotes.iter().find(|q| q.market_ticker.ends_with("JTATUM0-20")).unwrap();
        assert!((tatum_20.volume - 2428.49).abs() < 1e-9);
        assert!((tatum_20.open_interest - 1235.96).abs() < 1e-9);
    }

    #[test]
    fn prices_read_dollar_strings() {
        let market = serde_json::json!({
            "ticker": "KXNBAREB-26OCT21NYKBOS-NYKKTOWNS32-10",
            "event_ticker": "KXNBAREB-26OCT21NYKBOS",
            "title": "Karl-Anthony Towns: 10+ rebounds",
            "floor_strike": 9.5,
            "strike_type": "greater",
            "yes_bid_dollars": "0.5400",
            "yes_ask_dollars": "0.5600",
            "volume_fp": "812.00",
            "open_interest_fp": "300.50",
            "status": "active",
            "occurrence_datetime": "2026-10-21T23:30:00Z"
        });
        let quote = parse_market(&market, "rebounds").expect("quote");
        assert_eq!(quote.player_name, "Karl-Anthony Towns");
        assert_eq!(quote.team.as_deref(), Some("NYK"));
        assert_eq!(quote.threshold, 10.0);
        assert_eq!((quote.yes_bid, quote.yes_ask), (Some(0.54), Some(0.56)));
        assert!((quote.mid().unwrap() - 0.55).abs() < 1e-12);
        assert!(!quote.thin());
    }

    #[test]
    fn non_ladder_markets_are_skipped() {
        let market = serde_json::json!({
            "ticker": "X-26OCT21NYKBOS-A-1", "event_ticker": "X-26OCT21NYKBOS",
            "title": "Someone: thing", "floor_strike": 1.0, "strike_type": "between"
        });
        assert!(parse_market(&market, "points").is_none());
    }

    #[test]
    fn team_codes_come_from_the_tickers() {
        assert_eq!(team_from_tickers("KXNBAPTS-26OCT08BOSCLE", "KXNBAPTS-26OCT08BOSCLE-CLEDMITCHELL45-25").as_deref(), Some("CLE"));
        assert_eq!(team_from_tickers("KXNBAPTS-26OCT08BOSCLE", "KXNBAPTS-26OCT08BOSCLE-XYZ-25"), None);
        assert_eq!(team_from_tickers("bad", "bad"), None);
    }

    #[test]
    fn pregame_filter_drops_started_and_settled_games() {
        let mut quote = parse_markets(&fixture("markets_pts_bos_cle_settled.json"), "points").remove(0);
        let before_tip = DateTime::parse_from_rfc3339("2026-10-08T20:00:00Z").unwrap().with_timezone(&Utc);
        let after_tip = DateTime::parse_from_rfc3339("2026-10-09T03:00:00Z").unwrap().with_timezone(&Utc);
        assert!(!is_pregame(&quote, before_tip), "finalized is never pregame");
        quote.status = "active".into();
        assert!(is_pregame(&quote, before_tip));
        assert!(!is_pregame(&quote, after_tip));
        quote.game_time = None;
        assert!(is_pregame(&quote, after_tip), "no tip time: trust the active status");
    }

    #[tokio::test]
    async fn pagination_follows_the_cursor_until_empty() {
        let pages = [fixture("events_pts_page1.json"), fixture("events_pts_page2.json")];
        // Page two's cursor is not empty in the recording, so end the run with an empty page.
        let empty = fixture("events_pra_empty.json");
        let mut seen = Vec::new();
        let result = paginate(|cursor| {
            seen.push(cursor.clone());
            let page = match seen.len() {
                1 => pages[0].clone(),
                2 => pages[1].clone(),
                _ => empty.clone(),
            };
            async move { Ok(page) }
        })
        .await
        .expect("pages");
        assert_eq!(result.len(), 3);
        assert_eq!(seen[0], None);
        assert_eq!(seen[1].as_deref(), pages[0]["cursor"].as_str());
        let tickers: Vec<String> = result.iter().flat_map(event_tickers).collect();
        assert_eq!(tickers.len(), 4);
        assert!(tickers.contains(&"KXNBAPTS-26OCT08BOSCLE".to_string()));
    }

    #[tokio::test]
    async fn an_unlisted_series_is_one_empty_page() {
        let empty = fixture("events_pra_empty.json");
        let pages = paginate(|_| {
            let page = empty.clone();
            async move { Ok(page) }
        })
        .await
        .unwrap();
        assert_eq!(pages.len(), 1);
        assert!(event_tickers(&pages[0]).is_empty());
    }

    #[test]
    fn every_series_maps_to_a_catalog_stat() {
        let catalog: Value = serde_json::from_str(include_str!("../../../src/lib/catalog.json")).unwrap();
        let ids: Vec<&str> = catalog["stats"].as_array().unwrap().iter().map(|s| s["id"].as_str().unwrap()).collect();
        for (_, stat) in SERIES {
            assert!(ids.contains(stat), "{stat}");
        }
        for missing in ["turnovers", "field_goals_made", "field_goals_attempted"] {
            assert!(SERIES.iter().all(|(_, stat)| *stat != missing));
        }
        assert_eq!(stat_for_series("KXNBA3PT"), Some("three_point_field_goals_made"));
    }

    /// One real read of today's open NBA props. Run with `cargo test -- --ignored live_kalshi`.
    #[tokio::test]
    #[ignore]
    async fn live_kalshi_pull_reads_open_props() {
        let provider = KalshiProvider::new().expect("client");
        assert!(!provider.needs_key());
        let pull = provider.pull(Utc::now()).await.expect("pull");
        println!("events {} quotes {} requests {}", pull.events, pull.quotes.len(), pull.requests);
        assert!(pull.requests >= SERIES.len());
    }

    /// Today's open game totals are the same `greater` ladder, so they prove the parser on live
    /// data even before props list. Run with `cargo test -- --ignored live_kalshi`.
    #[tokio::test]
    #[ignore]
    async fn live_kalshi_game_totals_parse() {
        let provider = KalshiProvider::new().expect("client");
        let mut requests = 0;
        let pages = provider
            .pages("/markets", ("series_ticker", "KXNBATOTAL"), Some("open"), &mut requests)
            .await
            .expect("markets");
        let quotes: Vec<Quote> = pages.iter().flat_map(|page| parse_markets(page, "game_total")).collect();
        let pregame = quotes.iter().filter(|quote| is_pregame(quote, Utc::now())).count();
        println!("pages {} total rungs {} pregame {}", pages.len(), quotes.len(), pregame);
        if let Some(quote) = quotes.first() {
            println!("example {} threshold {} bid {:?} ask {:?} volume {}", quote.market_ticker, quote.threshold, quote.yes_bid, quote.yes_ask, quote.volume);
        }
    }
}
