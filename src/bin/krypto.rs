//! Krypto CLI - Config-driven backtesting and experiment runner.
//!
//! Usage:
//!   krypto run <config.json>           Run an experiment from config
//!   krypto list                        List all experiment runs
//!   krypto example                     Generate example config
//!   krypto validate <config.json>      Validate a config file
//!
//! Note: Minimal CLI to avoid clap dependency on older Rust versions.
//!       For full CLI features, upgrade to Rust 1.82+.

use anyhow::Result;
use std::path::PathBuf;

use krypto::config::ExperimentConfig;
use krypto::experiment::{list_runs, ExperimentRunner};

fn print_usage() {
    println!(
        "krypto v{} - Config-driven crypto backtesting",
        env!("CARGO_PKG_VERSION")
    );
    println!();
    println!("Usage:");
    println!("  krypto run <config.json>       Run an experiment");
    println!("  krypto list [dir]              List experiment runs");
    println!("  krypto example [output.json]   Generate example config");
    println!("  krypto validate <config.json>  Validate a config file");
    println!();
    println!("Examples:");
    println!("  krypto run experiments/my_strategy/config.json");
    println!("  krypto list ./experiments");
    println!("  krypto example my_config.json");
}

#[tokio::main]
async fn main() -> Result<()> {
    // Initialize logging
    tracing_subscriber::fmt::init();

    let args: Vec<String> = std::env::args().collect();

    if args.len() < 2 {
        print_usage();
        std::process::exit(1);
    }

    let command = &args[1];

    match command.as_str() {
        "run" => {
            if args.len() < 3 {
                eprintln!("Error: Missing config file path");
                println!();
                print_usage();
                std::process::exit(1);
            }

            let config_path = PathBuf::from(&args[2]);
            println!("🚀 Running experiment from: {:?}", config_path);

            let config = ExperimentConfig::from_json(&config_path)?;
            let mut runner = ExperimentRunner::new(config)?;

            match runner.run() {
                Ok(summary) => {
                    println!("\n✅ Experiment completed successfully!\n");
                    print_results(&summary);
                }
                Err(e) => {
                    eprintln!("❌ Experiment failed: {}", e);
                    std::process::exit(1);
                }
            }
        }

        "list" => {
            let dir = args
                .get(3)
                .map(PathBuf::from)
                .unwrap_or_else(|| PathBuf::from("./experiments"));

            println!("📋 Listing experiment runs in {:?}...\n", dir);

            match list_runs(&dir) {
                Ok(runs) => {
                    if runs.is_empty() {
                        println!("No experiment runs found.");
                    } else {
                        for (i, run) in runs.iter().enumerate() {
                            println!("{}. {} [{}]", i + 1, run.run_id, run.status_text());
                            if let Some(ref results) = run.results {
                                println!(
                                    "   Sharpe: {:.2}, Return: {:.1}%, DD: {:.1}%",
                                    results.best.sharpe_ratio,
                                    results.best.total_return_pct,
                                    results.best.max_drawdown_pct
                                );
                            }
                            println!();
                        }
                    }
                }
                Err(e) => {
                    eprintln!("Failed to list runs: {}", e);
                    std::process::exit(1);
                }
            }
        }

        "example" => {
            let output = args
                .get(3)
                .map(PathBuf::from)
                .unwrap_or_else(|| PathBuf::from("experiment.example.json"));

            let example = ExperimentConfig::example();
            example.to_json(&output)?;
            println!("✅ Example config written to: {:?}", output);
        }

        "validate" => {
            if args.len() < 3 {
                eprintln!("Error: Missing config file path");
                std::process::exit(1);
            }

            let config_path = PathBuf::from(&args[2]);
            let config = ExperimentConfig::from_json(&config_path)?;
            config.validate()?;

            println!("✅ Configuration is valid!");
            println!("   Name: {}", config.name);
            println!("   Symbols: {:?}", config.data.symbols);
            println!("   Validation: {}", config.validation.method);
        }

        "help" | "--help" | "-h" => {
            print_usage();
        }

        other => {
            eprintln!("Unknown command: {}", other);
            println!();
            print_usage();
            std::process::exit(1);
        }
    }

    Ok(())
}

/// Print results summary.
fn print_results(summary: &krypto::experiment::ResultsSummary) {
    println!("📊 Results Summary");
    println!("{}", "=".repeat(50));
    println!();

    println!("Best Result:");
    println!("  Trades:     {}", summary.best.total_trades);
    println!("  Win Rate:   {:.1}%", summary.best.win_rate);
    println!("  Sharpe:     {:.2}", summary.best.sharpe_ratio);
    println!("  Return:     {:.1}%", summary.best.total_return_pct);
    println!("  Max DD:     {:.1}%", summary.best.max_drawdown_pct);
    println!("  Kelly:      {:.1}%", summary.best.kelly_fraction * 100.0);
    println!();

    if let Some(ref robustness) = summary.robustness {
        println!("Robustness (test/train): {:.1}%", robustness * 100.0);
    }

    println!();
    println!("Combinations tested: {}", summary.combinations_tested);
    println!("Combinations passed: {}", summary.combinations_passed);
}
