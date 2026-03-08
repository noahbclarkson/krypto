//! RSI mean reversion - test different configurations.
//!
//! Compares RSI_25_75 vs RSI_30_70 across timeframes.

use anyhow::Result;
use krypto::{
    algo::strategies::RsiMeanReversion,
    algo::SignalGenerator,
    backtest::engine::{Backtester, PositionSizing},
    data::loader::DataLoader,
    features::indicators::FeatureEngine,
};

fn print_result(name: &str, result: &krypto::backtest::engine::BacktestResult) {
    println!(
        "{:<35} | Trades: {:>3} | WinRate: {:>5.1}% | Return: {:>7.2}% | PF: {:>5.2} | DD: {:>5.1}%",
        name, result.total_trades, result.win_rate, result.total_return_pct, result.profit_factor, result.max_drawdown_pct
    );
}

#[tokio::main]
async fn main() -> Result<()> {
    let tests = vec![
        ("BTCUSDT", "1h", 500),
        ("ETHUSDT", "1h", 500),
        ("SOLUSDT", "1h", 500),
    ];

    let loader = DataLoader::new(None, None);

    println!("==========================================================================================");
    println!("RSI Mean Reversion - Best Configuration Test (5% TP, 5% SL)");
    println!("==========================================================================================\n");

    for (symbol, interval, candles) in &tests {
        println!("--- {} {} ({} candles) ---", symbol, interval, candles);
        
        // Load data
        let raw_df = loader.fetch_data(symbol, interval, *candles).await?;
        let df = FeatureEngine::add_technicals(&raw_df, None)?;
        
        // Test RSI 25/75
        let mut strategy_25_75 = RsiMeanReversion::new();
        strategy_25_75.rsi_lower = 25.0;
        strategy_25_75.rsi_upper = 75.0;
        let signal_25_75 = strategy_25_75.predict(&df)?;
        
        // Test RSI 30/70
        let mut strategy_30_70 = RsiMeanReversion::new();
        strategy_30_70.rsi_lower = 30.0;
        strategy_30_70.rsi_upper = 70.0;
        let signal_30_70 = strategy_30_70.predict(&df)?;
        
        // Test with 5% TP, 5% SL
        for (name, signal) in [("RSI_25_75", &signal_25_75), ("RSI_30_70", &signal_30_70)] {
            let bt = Backtester::with_defaults(10_000.0);
            let result = bt.run(&df, signal, 0.05, 0.05)?;
            let passed = result.win_rate >= 52.0 && result.total_trades >= 10;
            print!("{} ", if passed { "✓" } else { " " });
            print_result(name, &result);
        }
        
        // Test with 3% TP, 5% SL
        println!("\nWith 3% TP:");
        for (name, signal) in [("RSI_25_75", &signal_25_75), ("RSI_30_70", &signal_30_70)] {
            let bt = Backtester::with_defaults(10_000.0);
            let result = bt.run(&df, signal, 0.05, 0.03)?;
            let passed = result.win_rate >= 52.0 && result.total_trades >= 10;
            print!("{} ", if passed { "✓" } else { " " });
            print_result(name, &result);
        }
        
        println!();
    }

    Ok(())
}
