//! Live trading bot example.
//!
//! Demonstrates the live trading infrastructure in dry-run mode.
//!
//! # Usage
//!
//! ```bash
//! # Dry run (no real orders)
//! cargo run --example live_bot_demo
//!
//! # With real API credentials (still dry-run by default)
//! BINANCE_API_KEY=xxx BINANCE_API_SECRET=yyy cargo run --example live_bot_demo
//! ```

use anyhow::Result;
use krypto::live::{LiveBot, LiveConfig};

#[tokio::main]
async fn main() -> Result<()> {
    // Initialize logging
    tracing_subscriber::fmt::init();

    // Create configuration for dry-run paper trading
    let config = LiveConfig {
        symbols: vec![
            "BTCFDUSD".to_string(),
            "ETHFDUSD".to_string(),
            "SOLFDUSD".to_string(),
        ],
        interval: "1d".to_string(),
        initial_capital: 10_000.0,
        max_position_size: 1.0,
        fee_pct: 0.0, // 0% maker on FDUSD
        use_testnet: false,
        dry_run: true, // No real orders
        atr_stop_mult: 0.3,
        bb_period: 20,
        bb_std: 2.0,
        ..Default::default()
    };

    println!("=== Krypto Live Bot Demo ===");
    println!(
        "Mode: {}",
        if config.dry_run {
            "DRY RUN (no real orders)"
        } else {
            "LIVE"
        }
    );
    println!("Symbols: {:?}", config.symbols);
    println!("Interval: {}", config.interval);
    println!("Initial Capital: ${:.2}", config.initial_capital);
    println!("ATR Stop Multiplier: {}", config.atr_stop_mult);
    println!();

    // Create the bot
    let mut bot = LiveBot::new(config)?;

    println!("Starting bot...");
    println!("Press Ctrl+C to stop");
    println!();

    // Handle Ctrl+C gracefully
    tokio::spawn(async {
        tokio::signal::ctrl_c().await.ok();
        println!("\nShutting down...");
        std::process::exit(0);
    });

    // Start the bot (will run until interrupted)
    bot.start().await?;

    // Print final state
    let state = bot.state().await;
    println!("\n=== Final State ===");
    println!("Equity: ${:.2}", state.equity);
    println!(
        "Total PnL: ${:.2} ({:.2}%)",
        state.total_pnl, state.total_return_pct
    );
    println!("Trades: {}", state.trades);
    println!("Win Rate: {:.1}%", state.win_rate);

    Ok(())
}
