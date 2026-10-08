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
    if let Some(note) = &report.seed_note {
        println!("{note}");
    }
    if let Some(source) = report.stats.iter().find_map(|stat| stat.seeded_from.as_deref()) {
        println!("Carried {source}.");
    }
    println!(
        "{:<28} {:>8} {:>8} {:>8} {:>8} {:>8}",
        "stat", "holdout", "last 10", "cover", "train", "hold"
    );
    // A fit that stands on the carried season alone has no holdout yet.
    let number = |value: Option<f64>| value.map_or("-".to_string(), |value| format!("{value:.2}"));
    let percent = |value: Option<f64>| value.map_or("-".to_string(), |value| format!("{:.1}%", value * 100.0));
    for stat in report.stats {
        match stat.error {
            Some(error) => println!("{:<28} {error}", stat.label),
            None => println!(
                "{:<28} {:>8} {:>8} {:>8} {:>8} {:>8}",
                stat.label,
                number(stat.holdout_mae),
                number(stat.baseline_mae),
                percent(stat.holdout_coverage),
                stat.train_rows,
                stat.holdout_rows
            ),
        }
    }
}
