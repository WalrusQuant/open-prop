use std::path::PathBuf;

/// Fills a database from stats.nba.com without opening the window. One request per season type.
/// `sync-season <db path> <season> [season type]`
fn main() {
    let mut args = std::env::args().skip(1);
    let db = PathBuf::from(args.next().unwrap_or_else(|| {
        panic!("usage: sync-season <db path> <season> [\"Regular Season\" | Playoffs]")
    }));
    let season = args.next().unwrap_or_else(|| "2025-26".to_string());
    let season_type = args.next().unwrap_or_else(|| "Regular Season".to_string());
    let report = open_prop_lib::sync_cached(&db, &season, &season_type)
        .unwrap_or_else(|error| panic!("{error}"));
    println!(
        "{} {}: {} rows fetched, {} games and {} players cached",
        report.season, report.season_type, report.fetched, report.games, report.players
    );
    if let Some(warning) = report.warning {
        println!("{warning}");
    }
}
