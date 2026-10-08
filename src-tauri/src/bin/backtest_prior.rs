use std::path::PathBuf;

/// Scores the opening weeks of a season with last season carried over at several strengths.
/// `backtest-prior <db path> <season> <seed season> [carry list] [opponent carry list] [last team game]`
/// Lists are comma separated pseudo-minutes. Every carry runs with every opponent carry.
fn main() {
    let usage = "usage: backtest-prior <db path> <season> <seed season> [0,250,500] [1500] [10] [points,rebounds]";
    let mut args = std::env::args().skip(1);
    let db = PathBuf::from(args.next().unwrap_or_else(|| panic!("{usage}")));
    let season = args.next().unwrap_or_else(|| panic!("{usage}"));
    let seed_season = args.next().unwrap_or_else(|| panic!("{usage}"));
    let list = |value: Option<String>, default: &str| -> Vec<f64> {
        value
            .unwrap_or_else(|| default.to_string())
            .split(',')
            .map(|item| item.trim().parse().unwrap_or_else(|_| panic!("'{item}' is not a number. {usage}")))
            .collect()
    };
    let carries = list(args.next(), "0,100,250,500,1000,2000");
    let opponents = list(args.next(), "1500");
    let last_game: usize = args
        .next()
        .map(|value| value.parse().unwrap_or_else(|_| panic!("{usage}")))
        .unwrap_or(10);
    let stats: Vec<String> = args
        .next()
        .unwrap_or_else(|| "points,rebounds,assists,three_point_field_goals_made,points_assists_rebounds".to_string())
        .split(',')
        .map(str::to_string)
        .collect();
    let arms: Vec<(f64, f64)> = carries
        .iter()
        .flat_map(|carry| opponents.iter().map(move |opponent| (*carry, *opponent)))
        .collect();
    let text = open_prop_lib::backtest_prior(&db, &season, &seed_season, &stats, &arms, last_game)
        .unwrap_or_else(|error| panic!("{error}"));
    println!("{text}");
}
