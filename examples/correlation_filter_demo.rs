//! Correlation filter demo for multi-asset portfolios.
//!
//! Shows how to use CorrelationFilter to prevent simultaneous entries
//! on highly correlated assets (e.g., BTC + ETH both long at the same time).
//!
//! Usage:
//!   cargo run --release --example correlation_filter_demo

use anyhow::Result;
use colored::*;
use krypto::{algo::CorrelationFilter, data::loader::DataLoader};
use std::collections::HashMap;

const SYMBOLS: &[&str] = &["BTCFDUSD", "ETHFDUSD", "SOLFDUSD", "XRPFDUSD", "DOGEFDUSD"];
const LOOKBACK: usize = 100; // Bars for correlation calculation
const THRESHOLD: f64 = 0.7; // 70% correlation threshold

#[tokio::main]
async fn main() -> Result<()> {
    println!("\n{}", "━".repeat(72).bright_cyan());
    println!("{}", "  CORRELATION FILTER DEMO".bright_cyan().bold());
    println!(
        "{}",
        format!(
            "  {} symbols, {}-bar lookback, {:.0}% threshold",
            SYMBOLS.len(),
            LOOKBACK,
            THRESHOLD * 100.0
        )
        .bright_cyan()
    );
    println!("{}", "━".repeat(72).bright_cyan());
    println!();

    let loader = DataLoader::new(None, None);

    // 1. Fetch close prices for all symbols
    println!("{}", "Fetching data...".yellow());
    let mut closes: HashMap<String, Vec<f64>> = HashMap::new();

    for symbol in SYMBOLS {
        match loader.fetch_data(symbol, "1d", LOOKBACK as u32).await {
            Ok(df) => {
                if let Some(ca) = df.column("close").ok().and_then(|s| s.f64().ok()) {
                    let prices: Vec<f64> = ca.into_iter().map(|v| v.unwrap_or(0.0)).collect();
                    closes.insert(symbol.to_string(), prices);
                }
            }
            Err(e) => println!("  {} failed: {}", symbol, e),
        }
    }

    println!("  Loaded {} symbols\n", closes.len());

    // 2. Compute correlation matrix
    println!("{}", "Computing correlation matrix...".yellow());
    let correlations = compute_correlation_matrix(&closes);

    print_correlation_matrix(&correlations, SYMBOLS);

    // 3. Demo: filter correlated signals
    println!();
    println!("{}", "━".repeat(72).bright_cyan());
    println!("{}", "  FILTER DEMO".bright_cyan().bold());
    println!("{}", "━".repeat(72).bright_cyan());
    println!();

    // Scenario 1: All symbols have long signals
    let mut all_long = HashMap::new();
    for symbol in SYMBOLS {
        all_long.insert(symbol.to_string(), 1.0);
    }

    println!("{}", "Scenario 1: All symbols have long signals".white());
    println!("  Before filter: {} signals", all_long.len());

    let filter = CorrelationFilter::new(THRESHOLD, 1);
    let filtered = filter.filter(&all_long, &correlations);

    let remaining: Vec<(&String, &f64)> =
        filtered.iter().filter(|(_, &s)| s.abs() > 0.01).collect();
    println!("  After filter:  {} signals", remaining.len());
    let kept_names: Vec<&str> = remaining.iter().map(|(s, _)| s.as_str()).collect();
    println!("  Kept: {}", kept_names.join(", "));

    // Scenario 2: Mixed signals (some long, some short)
    let mut mixed = HashMap::new();
    mixed.insert("BTCFDUSD".to_string(), 1.0); // Long
    mixed.insert("ETHFDUSD".to_string(), 1.0); // Long (correlated with BTC)
    mixed.insert("SOLFDUSD".to_string(), -1.0); // Short
    mixed.insert("XRPFDUSD".to_string(), 1.0); // Long
    mixed.insert("DOGEFDUSD".to_string(), 0.8); // Long (weaker)

    println!();
    println!("{}", "Scenario 2: Mixed signals".white());
    println!("  Before filter:");
    for (sym, sig) in &mixed {
        println!("    {}: {:.1}", sym, sig);
    }

    let filtered_mixed = filter.filter(&mixed, &correlations);

    println!("  After filter:");
    for sym in SYMBOLS {
        let sig = filtered_mixed.get(&sym.to_string()).unwrap_or(&0.0);
        if sig.abs() > 0.01 {
            println!("    {}: {:.1} ✓", sym, sig);
        } else {
            println!("    {}: filtered", sym);
        }
    }

    // 4. Key insight
    println!();
    println!("{}", "━".repeat(72).bright_cyan());
    println!("{}", "  KEY INSIGHT".bright_cyan().bold());
    println!("{}", "━".repeat(72).bright_cyan());
    println!();
    println!("  The correlation filter prevents over-exposure to correlated assets.");
    println!("  When BTC and ETH both signal long (85% correlation), only one is kept.");
    println!("  This reduces portfolio risk and improves diversification.");
    println!();
    println!("  In a live portfolio:");
    println!("  - Run correlation filter on all signals at each bar");
    println!("  - Only execute the highest-conviction signal per correlation cluster");
    println!("  - Re-compute correlation matrix weekly to adapt to market changes");
    println!();

    Ok(())
}

fn compute_correlation_matrix(
    closes: &HashMap<String, Vec<f64>>,
) -> HashMap<(String, String), f64> {
    let mut correlations: HashMap<(String, String), f64> = HashMap::new();
    let symbols: Vec<&String> = closes.keys().collect();

    for i in 0..symbols.len() {
        for j in i..symbols.len() {
            let sym_i = symbols[i];
            let sym_j = symbols[j];

            if i == j {
                correlations.insert((sym_i.clone(), sym_j.clone()), 1.0);
                continue;
            }

            let prices_i = &closes[sym_i];
            let prices_j = &closes[sym_j];

            // Compute returns
            let rets_i: Vec<f64> = prices_i.windows(2).map(|w| (w[1] - w[0]) / w[0]).collect();
            let rets_j: Vec<f64> = prices_j.windows(2).map(|w| (w[1] - w[0]) / w[0]).collect();

            let min_len = rets_i.len().min(rets_j.len());
            if min_len < 10 {
                correlations.insert((sym_i.clone(), sym_j.clone()), 0.0);
                continue;
            }

            let corr = pearson(&rets_i[..min_len], &rets_j[..min_len]);
            correlations.insert((sym_i.clone(), sym_j.clone()), corr);
            correlations.insert((sym_j.clone(), sym_i.clone()), corr);
        }
    }

    correlations
}

/// Compute Pearson correlation coefficient.
fn pearson(x: &[f64], y: &[f64]) -> f64 {
    if x.len() != y.len() || x.is_empty() {
        return 0.0;
    }

    let n = x.len() as f64;
    let sum_x: f64 = x.iter().sum();
    let sum_y: f64 = y.iter().sum();
    let sum_xy: f64 = x.iter().zip(y.iter()).map(|(a, b)| a * b).sum();
    let sum_x2: f64 = x.iter().map(|a| a * a).sum();
    let sum_y2: f64 = y.iter().map(|b| b * b).sum();

    let numerator = sum_xy - (sum_x * sum_y / n);
    let denominator = ((sum_x2 - sum_x * sum_x / n) * (sum_y2 - sum_y * sum_y / n)).sqrt();

    if denominator == 0.0 {
        return 0.0;
    }

    numerator / denominator
}

fn print_correlation_matrix(correlations: &HashMap<(String, String), f64>, symbols: &[&str]) {
    println!();
    print!("          ");
    for sym in symbols {
        print!("{:>10}", sym.replace("FDUSD", ""));
    }
    println!();

    for sym_i in symbols {
        print!("{:>10}", sym_i.replace("FDUSD", ""));
        for sym_j in symbols {
            let corr = correlations
                .get(&(sym_i.to_string(), sym_j.to_string()))
                .unwrap_or(&0.0);
            if sym_i == sym_j {
                print!("    1.00   ");
            } else if *corr >= 0.7 {
                print!("{}", format!("{:10.2}", corr).red());
            } else if *corr >= 0.5 {
                print!("{}", format!("{:10.2}", corr).yellow());
            } else {
                print!("{}", format!("{:10.2}", corr).green());
            }
        }
        println!();
    }
}
