//! Prop prices from outside books. Each source sits behind [`OddsProvider`]; Kalshi is the free
//! built-in one. Quotes are stored as snapshots under `pulled_at`, so line history is whatever
//! the app happened to pull. Nothing here places an order or reads an account.

pub mod kalshi;
pub mod matching;
pub mod store;

use std::future::Future;

use chrono::{DateTime, Utc};
use serde::Serialize;

use crate::error::AppResult;

/// One yes/no rung: "Jayson Tatum: 25+ points" is stat `points`, threshold 25.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Quote {
    pub book: String,
    pub event_ticker: String,
    pub market_ticker: String,
    /// open-prop stat id from `catalog.json`.
    pub stat: String,
    pub player_name: String,
    /// The book's team code, when the ticker carries one.
    pub team: Option<String>,
    /// Yes pays when the stat is at least this many. `floor_strike` 24.5 is 25.
    pub threshold: f64,
    pub floor_strike: f64,
    /// Probabilities from 0 to 1. None when nobody is bidding or offering.
    pub yes_bid: Option<f64>,
    pub yes_ask: Option<f64>,
    pub volume: f64,
    pub open_interest: f64,
    /// Scheduled tip, when the book says.
    pub game_time: Option<String>,
    pub status: String,
}

impl Quote {
    pub fn mid(&self) -> Option<f64> {
        match (self.yes_bid, self.yes_ask) {
            (Some(bid), Some(ask)) => Some((bid + ask) / 2.0),
            _ => None,
        }
    }

    pub fn spread(&self) -> Option<f64> {
        match (self.yes_bid, self.yes_ask) {
            (Some(bid), Some(ask)) => Some(ask - bid),
            _ => None,
        }
    }

    /// A rung is thin when one side is empty, the spread is wide, or almost nothing traded.
    pub fn thin(&self) -> bool {
        match self.spread() {
            None => true,
            Some(spread) => spread > THIN_SPREAD || self.volume < THIN_VOLUME,
        }
    }
}

/// Wider than ten cents, the mid says little about the probability.
pub const THIN_SPREAD: f64 = 0.10;
/// Contracts traded. Below this the price is one or two orders.
pub const THIN_VOLUME: f64 = 100.0;
// Edge needs the model's tail at each rung, which the screens already hold, so the yes and no
// edge math lives in `src/lib/kalshi.ts`.

#[derive(Debug, Clone, Default)]
pub struct Pull {
    pub quotes: Vec<Quote>,
    pub events: usize,
    pub requests: usize,
}

/// A source of prop prices. Kalshi needs no key and costs nothing; a paid source would.
pub trait OddsProvider: Send + Sync {
    fn id(&self) -> &'static str;
    fn needs_key(&self) -> bool;
    /// Pregame quotes only: anything whose game has started is left out.
    fn pull(&self, now: DateTime<Utc>) -> impl Future<Output = AppResult<Pull>> + Send;
}

#[cfg(test)]
mod tests {
    use super::*;

    fn quote(bid: Option<f64>, ask: Option<f64>, volume: f64) -> Quote {
        Quote {
            book: "kalshi".into(),
            event_ticker: "E".into(),
            market_ticker: "M".into(),
            stat: "points".into(),
            player_name: "P".into(),
            team: None,
            threshold: 25.0,
            floor_strike: 24.5,
            yes_bid: bid,
            yes_ask: ask,
            volume,
            open_interest: 0.0,
            game_time: None,
            status: "active".into(),
        }
    }

    #[test]
    fn mid_spread_and_thin_flags() {
        let tight = quote(Some(0.61), Some(0.62), 5000.0);
        assert!((tight.mid().unwrap() - 0.615).abs() < 1e-12);
        assert!((tight.spread().unwrap() - 0.01).abs() < 1e-12);
        assert!(!tight.thin());
        assert!(quote(Some(0.40), Some(0.55), 5000.0).thin(), "wide spread");
        assert!(quote(Some(0.61), Some(0.62), 20.0).thin(), "little volume");
        assert!(quote(None, Some(0.62), 5000.0).thin(), "no bid");
        assert_eq!(quote(None, Some(0.62), 1.0).mid(), None);
    }

}
