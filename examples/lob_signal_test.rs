use anyhow::Result;
use std::fs::File;
use std::io::{BufRead, BufReader};

fn main() -> Result<()> {
    println!("Analyzing LOB Microstructure Daemon Log Data");

    // Parse the log file to extract NOBI values manually since we don't have regex in Cargo
    let log_path = "data/lob_daemon.log";
    let file = match File::open(log_path) {
        Ok(f) => f,
        Err(e) => {
            println!("Error opening log file: {}", e);
            return Ok(());
        }
    };

    let reader = BufReader::new(file);

    let mut nobi_sums = std::collections::HashMap::new();
    let mut nobi_counts = std::collections::HashMap::new();

    for line in reader.lines() {
        let line = line?;
        if line.contains("NOBI =") {
            let parts: Vec<&str> = line.split(':').collect();
            if parts.len() < 2 {
                continue;
            }

            let symbol = parts[0].trim().to_string();

            let nobi_part = parts[1].split("NOBI =").nth(1).unwrap_or("").trim();
            let nobi_val_str = nobi_part.split_whitespace().next().unwrap_or("0.0");

            if let Ok(nobi) = nobi_val_str.parse::<f64>() {
                *nobi_sums.entry(symbol.clone()).or_insert(0.0) += nobi;
                *nobi_counts.entry(symbol).or_insert(0) += 1;
            }
        }
    }

    println!("\nAggregate NOBI Metrics from live daemon run:");
    println!("{:-<40}", "");

    for (symbol, sum) in &nobi_sums {
        let count = nobi_counts.get(symbol).unwrap();
        let avg = sum / *count as f64;
        println!(
            "{:10} | Samples: {:5} | Avg NOBI: {:+.4}",
            symbol, count, avg
        );
    }

    println!("{:-<40}", "");
    println!("Insight: We need price data joined to this tick data to evaluate predictive power.");
    println!("The daemon is caching raw ticks. This is Track C (Broaden edge discovery).");

    Ok(())
}
