//! RSI mean reversion with take profit optimization.
//!
//! Tests RSI_25_75 (which passed on ETH) with various take profit levels.

use anyhow::Result;
use krypto::{
    algo::strategies::RsiMeanReversion,
    algo::SignalGenerator,
    backtest::engine::Backtester,
    data::loader::DataLoader,
    features::indicators::FeatureEngine,
};

fn print_result(name: &str, result: &krypto::backtest::engine::BacktestResult) {
    let tp_str = if result.largest_win_pct > 0.0 {
        format!("{:.1}%", result.largest_win_pct)
    } else {
        "N/A".to_string()
    };
    println!(
        "{:<30} | Trades: {:>3} | WinRate: {:>5.1}% | Return: {:>7.2}% | PF: {:>5.2} | MaxWin: {:>6}",
        name, result.total_trades, result.win_rate, result.total_return_pct, result.profit_factor, tp_str
    );
}

#[tokio::main]
async fn main() -> Result<()> {
    let tests = vec![
        ("BTCUSDT", "1h", 500),
        ("ETHUSDT", "1h", 500),
        ("BTCUSDT", "4h", 500),
        ("ETHUSDT", "4h", 500),
    ];

    let take_profit_levels = vec![0.0, 0.02, 0.03, 0.05, 0.08, 0.10];
    
    let loader = DataLoader::new(None, None);

    println!("==========================================================================================");
    println!("RSI Mean Reversion with Take Profit Optimization");
    println!("==========================================================================================\n");

    for (symbol, interval, candles) in &tests {
        println!("--- {} {} ({} candles) ---", symbol, interval, candles);
        
        // Load data
        let raw_df = loader.fetch_data(symbol, interval, *candles).await?;
        let df = FeatureEngine::add_technicals(&raw_df, None)?;
        
        // RSI 25/75 config
        let mut strategy = RsiMeanReversion::new();
        strategy.rsi_lower = 25.0;
        strategy.rsi_upper = 75.0;
        let signal = strategy.predict(&df)?;
        
        println!("Testing take profit levels...");
        for tp in &take_profit_levels {
            let tp_pct = *tp * 100.0;
            let name = format!("RSI_25_75_TP_{:.0}%", tp_pct);
            
            let backtester = Backtester::with_defaults(10_000.0);
            let result = backtester.run(&df, &signal, 0.05, *tp)?;
            
            let passed = result.win_rate >= 52.0 && result.total_trades >= 10;
            let marker = if passed { "✓" } else { " " };
            
            print!("{} ", marker);
            print_result(&name, &result);
        }
        println!();
    }

    println!("==========================================================================================");
    println!("Gate: WinRate >= 52%, Trades >= 10");
    println!("==========================================================================================");

    Ok(())
}
