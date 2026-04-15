//! Quick test of live bot with 1m candles to verify signal generation.
//!
//! # Usage
//! ```bash
//! timeout 120 cargo run --example live_bot_1m_test --profile sweep
//! ```

use anyhow::Result;
use krypto::live::{LiveBot, LiveConfig};

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt::init();

    let config = LiveConfig {
        symbols: vec!["BTCFDUSD".to_string()],
        interval: "1m".to_string(), // 1m for quick testing
        initial_capital: 10_000.0,
        max_position_size: 1.0,
        fee_pct: 0.0,
        use_testnet: false,
        dry_run: true,
        atr_stop_mult: 0.3,
        bb_period: 20,
        bb_std: 2.0,
        ..Default::default()
    };

    println!("=== Live Bot 1m Test ===");
    println!("Interval: 1m (testing signal generation)");
    println!("Will run for ~2 minutes to capture at least 2 closed candles\n");

    let mut bot = LiveBot::new(config)?;

    // Auto-stop after 130 seconds
    tokio::spawn(async {
        tokio::time::sleep(tokio::time::Duration::from_secs(130)).await;
        println!("\n[Auto-stop] 130s elapsed, shutting down...");
        std::process::exit(0);
    });

    tokio::spawn(async {
        tokio::signal::ctrl_c().await.ok();
        println!("\nShutting down...");
        std::process::exit(0);
    });

    bot.start().await?;

    let state = bot.state().await;
    println!("\n=== Final State ===");
    println!("Equity: ${:.2}", state.equity);
    println!("Trades: {}", state.trades);
    println!("Win Rate: {:.1}%", state.win_rate);

    Ok(())
}
