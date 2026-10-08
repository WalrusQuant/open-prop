use std::path::PathBuf;

/// `backtest-rookie <db> <season> <history seasons csv> [stats] [prior minutes] [last game]`
fn main() {
    let mut args = std::env::args().skip(1);
    let usage = "usage: backtest-rookie <db> <season> <2021-22,2022-23,2023-24> [points] [100,200,300] [10]";
    let db = PathBuf::from(args.next().unwrap_or_else(|| panic!("{usage}")));
    let season = args.next().unwrap_or_else(|| panic!("{usage}"));
    let history = args
        .next()
        .unwrap_or_else(|| panic!("{usage}"))
        .split(',')
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>();
    let stats = args
        .next()
        .unwrap_or_else(|| "points".to_string())
        .split(',')
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>();
    let priors = args
        .next()
        .unwrap_or_else(|| "100,200,300,500".to_string())
        .split(',')
        .map(|s| s.trim().parse::<f64>().unwrap_or_else(|_| panic!("bad prior minutes")))
        .collect::<Vec<_>>();
    let last_game = args
        .next()
        .unwrap_or_else(|| "10".to_string())
        .parse()
        .unwrap_or_else(|_| panic!("last game must be a number"));
    let text = open_prop_lib::backtest_rookie(&db, &season, &history, &stats, &priors, last_game)
        .unwrap_or_else(|e| panic!("{e}"));
    print!("{text}");
}
