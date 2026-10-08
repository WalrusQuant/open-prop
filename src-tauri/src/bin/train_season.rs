use std::path::PathBuf;

fn main() {
    let db = PathBuf::from(
        std::env::args()
            .nth(1)
            .unwrap_or_else(|| {
                let home = std::env::var("HOME").unwrap_or_else(|_| ".".to_string());
                format!("{home}/Library/Application Support/com.openprop.desk/open-prop.db")
            }),
    );
    let season = std::env::args().nth(2).unwrap_or_else(|| "2025-26".to_string());
    let season_type = std::env::args()
        .nth(3)
        .unwrap_or_else(|| "Regular Season".to_string());
    let report = open_prop_lib::train_cached(&db, &season, &season_type)
        .unwrap_or_else(|error| panic!("{error}"));
    println!(
        "{:<28} {:>8} {:>8} {:>8} {:>8} {:>8}",
        "stat", "holdout", "last 10", "cover", "train", "hold"
    );
    for stat in report.stats {
        match stat.error {
            Some(error) => println!("{:<28} {error}", stat.label),
            None => println!(
                "{:<28} {:>8.2} {:>8.2} {:>7.1}% {:>8} {:>8}",
                stat.label,
                stat.holdout_mae.unwrap_or(f64::NAN),
                stat.baseline_mae.unwrap_or(f64::NAN),
                stat.holdout_coverage.unwrap_or(f64::NAN) * 100.0,
                stat.train_rows,
                stat.holdout_rows
            ),
        }
    }
}
