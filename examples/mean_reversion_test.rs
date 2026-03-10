//! Mean-reversion strategy test with win rate validation.
//!
//! Tests existing mean-reversion strategies (BollingerReversion, RsiMeanReversion)
//! to see if they perform better than trend-following.
//!
//! Usage:
//!   cargo run --example mean_reversion_test -- [symbol] [interval] [candles]


use krypto::{
    algo::strategies::{BollingerReversion, RsiMeanReversion},
    algo::SignalGenerator,
    backtest::engine::Backtester,
    data::loader::DataLoader,
    features::indicators::FeatureEngine,
};

fn print_result(name: &str, result: &krypto::backtest::engine::BacktestResult) {
    let passed = result.win_rate >= 52.0 && result.total_trades >= 10;
    let marker = if passed { "✓" } else { " " };
    println!(
        "{} {:<25} | Trades: {:>3} | WinRate: {:>5.1}% | Return: {:>7.2}% | PF: {:>5.2}",
        marker, name, result.total_trades, result.win_rate, result.total_return_pct, result.profit_factor,
    );
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().collect();
    let symbol = args.get(1).map(|s| s.as_str()).unwrap_or("BTCUSDT");
    let interval = args.get(2).map(|s| s.as_str()).unwrap_or("1h");
    let candles: u16 = args.get(3).and_then(|s| s.parse().ok()).unwrap_or(1000);

    println!("=== Mean-Reversion Strategy Test ===");
    println!("Symbol: {}, Interval: {}, Candles: {}\n", symbol, interval, candles);

    // Load data
    println!("Fetching data from Binance...");
    let loader = DataLoader::new(None, None);
    let raw_df = loader.fetch_data(symbol, interval, candles as u32).await?;
    println!("Loaded {} bars", raw_df.height());

    // Add technical indicators
    println!("Computing technical indicators...");
    let df = FeatureEngine::add_technicals(&raw_df, None)?;
    println!("Features computed\n");

    // Define strategy configurations to test
    let bb_configs: Vec<(usize, f64, f64)> = vec![
        (20, 2.0, 30.0),  // Default
        (20, 2.0, 40.0),  // Higher RSI filter
        (20, 2.5, 35.0),  // Wider bands
        (15, 2.0, 30.0),  // Shorter period
        (30, 2.0, 25.0),  // Longer period
    ];

    let rsi_configs: Vec<(f64, f64)> = vec![
        (30.0, 70.0),  // Default
        (25.0, 75.0),  // Wider bands
        (35.0, 65.0),  // Narrower bands
        (20.0, 80.0),  // Very wide
    ];

    let initial_capital = 10_000.0;
    let trailing_stop = 0.05;
    let take_profit = 0.0;

    println!("Testing BollingerReversion strategies...");
    for (period, std_dev, rsi_filter) in &bb_configs {
        let mut strategy = BollingerReversion::new();
        strategy.bb_period = *period;
        strategy.bb_std = *std_dev;
        strategy.rsi_filter = *rsi_filter;

        let name = format!("BB_{}_{}_{}", period, std_dev, rsi_filter);
        
        // Generate signals
        let signal = strategy.predict(&df)?;
        
        // Run backtest
        let backtester = Backtester::with_defaults(initial_capital);
        let result = backtester.run(&df, &signal, trailing_stop, take_profit)?;
        
        print_result(&name, &result);
    }

    println!("\nTesting RsiMeanReversion strategies...");
    for (lower, upper) in &rsi_configs{
        let mut strategy = RsiMeanReversion::new();
        strategy.rsi_lower = *lower;
        strategy.rsi_upper = *upper;

        let name = format!("RSI_{}_{}", lower, upper);
        
        // Generate signals
        let signal = strategy.predict(&df)?;
        
        // Run backtest
        let backtester = Backtester::with_defaults(initial_capital);
        let result = backtester.run(&df, &signal, trailing_stop, take_profit)?;
        
        print_result(&name, &result);
    }

    println!("\n=== Summary ===");
    println!("Gate: WinRate >= 52%, Trades >= 10");
    println!("Mean-reversion strategies test complete.");

    Ok(())
}
