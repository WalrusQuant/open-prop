use std::path::PathBuf;

/// Scores a season with last season carried over at several strengths.
/// `backtest-prior <db> <season> <seed> [carries] [opp carries] [last team game] [stats] [decay taus]`
fn main() {
    let usage = "usage: backtest-prior <db> <season> <seed> [0,500,1000] [1500] [10] [points] [0,1000]";
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
        .unwrap_or_else(|| {
            "points,rebounds,assists,three_point_field_goals_made,points_assists_rebounds".to_string()
        })
        .split(',')
        .map(str::to_string)
        .collect();
    let taus = list(args.next(), "0");
    let mut arms = Vec::new();
    for carry in &carries {
        for opponent in &opponents {
            for tau in &taus {
                arms.push((*carry, *opponent, *tau));
            }
        }
    }
    let text = open_prop_lib::backtest_prior(&db, &season, &seed_season, &stats, &arms, last_game)
        .unwrap_or_else(|error| panic!("{error}"));
    println!("{text}");
}
