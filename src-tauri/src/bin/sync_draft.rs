use std::path::PathBuf;

/// `sync-draft <db path> <year>` — e.g. 2024 for the 2024 draft / 2024-25 rookies.
fn main() {
    let mut args = std::env::args().skip(1);
    let db = PathBuf::from(args.next().unwrap_or_else(|| {
        panic!("usage: sync-draft <db path> <year>")
    }));
    let year: i32 = args
        .next()
        .unwrap_or_else(|| panic!("usage: sync-draft <db path> <year>"))
        .parse()
        .unwrap_or_else(|_| panic!("year must be an integer like 2024"));
    let n = open_prop_lib::sync_draft_year(&db, year).unwrap_or_else(|e| panic!("{e}"));
    println!("draft {year}: {n} picks stored");
}
