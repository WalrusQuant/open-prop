use chrono::{Datelike, NaiveDate};

use crate::error::{AppError, AppResult};

pub const SEASON_TYPES: [&str; 2] = ["Regular Season", "Playoffs"];

/// The regular season usually opens in the last third of October.
/// Early October stays on the season that just finished.
const OPENING_MONTH: u32 = 10;
const OPENING_DAY: u32 = 22;
const FIRST_SEASON_START: i32 = 2021;

pub fn format_season(start: i32) -> String {
    format!("{start}-{:02}", (start + 1) % 100)
}

pub fn suggested_season(today: NaiveDate) -> String {
    let start = if today.month() > OPENING_MONTH
        || (today.month() == OPENING_MONTH && today.day() >= OPENING_DAY)
    {
        today.year()
    } else {
        today.year() - 1
    };
    format_season(start)
}

/// From July the rosters belong to the season that opens in October.
fn upcoming_start(today: NaiveDate) -> i32 {
    if today.month() >= 7 {
        today.year()
    } else {
        today.year() - 1
    }
}

pub fn season_starts(today: NaiveDate) -> Vec<i32> {
    let suggested = season_start_year(&suggested_season(today)).unwrap_or(today.year() - 1);
    let last = suggested.max(upcoming_start(today));
    (FIRST_SEASON_START..=last).collect()
}

pub fn validate_season(value: &str) -> AppResult<String> {
    let start = season_start_year(value).ok_or_else(|| {
        AppError::message(format!(
            "'{value}' is not an NBA season. Use a start year like 2025-26."
        ))
    })?;
    if !(1990..=2100).contains(&start) {
        return Err(AppError::message(format!("'{value}' is outside the season range.")));
    }
    Ok(value.to_string())
}

pub fn validate_season_type(value: &str) -> AppResult<String> {
    if SEASON_TYPES.contains(&value) {
        Ok(value.to_string())
    } else {
        Err(AppError::message(
            "Season type has to be Regular Season or Playoffs.".to_string(),
        ))
    }
}

/// True for the season that today's rosters belong to: the one on now, or from July the one
/// about to open. The roster call answers with today's teams whatever season it is asked for.
pub fn is_roster_season(season: &str, today: NaiveDate) -> bool {
    season_start_year(season) == Some(upcoming_start(today))
}

/// Where a fit borrows its prior: playoffs from the same regular season, a regular season
/// from the one before.
pub fn seed_source(season: &str, season_type: &str) -> Option<(String, String)> {
    let regular = SEASON_TYPES[0].to_string();
    if season_type == SEASON_TYPES[1] {
        return Some((season.to_string(), regular));
    }
    let start = season_start_year(season)?;
    Some((format_season(start - 1), regular))
}

/// The calendar year of the draft that feeds this season's rookies (`2024` for `2024-25`).
pub fn draft_year(season: &str) -> Option<i32> {
    season_start_year(season)
}

fn season_start_year(value: &str) -> Option<i32> {
    let (start, end) = value.split_once('-')?;
    if start.len() != 4 || end.len() != 2 {
        return None;
    }
    let start: i32 = start.parse().ok()?;
    let end: i32 = end.parse().ok()?;
    if end != (start + 1) % 100 {
        return None;
    }
    Some(start)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn day(value: &str) -> NaiveDate {
        NaiveDate::parse_from_str(value, "%Y-%m-%d").unwrap()
    }

    #[test]
    fn october_before_opening_stays_on_the_finished_season() {
        assert_eq!(suggested_season(day("2026-10-07")), "2025-26");
    }

    #[test]
    fn opening_night_moves_to_the_new_season() {
        assert_eq!(suggested_season(day("2026-10-22")), "2026-27");
        assert_eq!(suggested_season(day("2026-03-15")), "2025-26");
        assert_eq!(suggested_season(day("2025-11-01")), "2025-26");
    }

    #[test]
    fn early_october_offers_the_upcoming_season_too() {
        let starts = season_starts(day("2026-10-07"));
        assert!(starts.contains(&2025));
        assert!(starts.contains(&2026));
        assert_eq!(*starts.first().unwrap(), 2021);
    }

    #[test]
    fn playoffs_seed_from_their_regular_season_and_a_season_from_the_last() {
        let regular = "Regular Season".to_string();
        assert_eq!(
            seed_source("2025-26", "Playoffs"),
            Some(("2025-26".to_string(), regular.clone()))
        );
        assert_eq!(
            seed_source("2025-26", "Regular Season"),
            Some(("2024-25".to_string(), regular.clone()))
        );
        assert_eq!(
            seed_source("2000-01", "Regular Season"),
            Some(("1999-00".to_string(), regular))
        );
        assert_eq!(seed_source("season", "Regular Season"), None);
    }

    #[test]
    fn rosters_are_for_the_season_on_now_or_about_to_open() {
        // Before opening night today's rosters are next season's, not the one that finished.
        let today = day("2026-10-08");
        assert!(is_roster_season("2026-27", today));
        assert!(!is_roster_season("2025-26", today));
        assert!(!is_roster_season("2024-25", today));
        assert!(is_roster_season("2025-26", day("2026-03-01")));
        assert!(is_roster_season("2025-26", day("2026-06-30")));
        assert!(!is_roster_season("2025-26", day("2026-07-01")));
        assert!(!is_roster_season("season", today));
    }

    #[test]
    fn season_labels_have_to_match_the_following_year() {
        assert!(validate_season("2025-26").is_ok());
        assert!(validate_season("1999-00").is_ok());
        assert!(validate_season("2025-27").is_err());
        assert!(validate_season("season").is_err());
        assert!(validate_season_type("Playoffs").is_ok());
        assert!(validate_season_type("Preseason").is_err());
    }
}
