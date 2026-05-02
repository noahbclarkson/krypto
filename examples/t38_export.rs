use chrono::NaiveDateTime;
use std::fs::File;
use std::io::Write;

fn main() {
    let mut file = File::create("snapshots/live_compatible_equity.csv").unwrap();
    writeln!(file, "Date,Equity,Drawdown").unwrap();
    // Implement dummy for now to check compilation
}
