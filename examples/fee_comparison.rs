//! Fee comparison: taker (0.1%) vs FDUSD maker (0.0%)
//!
//! Runs the same strategies/symbols/intervals twice — once with taker fees,
//! once with 0% maker fees — to quantify the real edge from FDUSD passive execution.

use anyhow::Result;
use colored::*;
use krypto::{
    algo::SignalGenerator,
    algo::strategies::*,
    backtest::engine::Backtester,
    data::loader::DataLoader,
    features::indicators::FeatureEngine,
};

const SYMBOLS: &[&str] = &["BTCUSDT", "ETHUSDT", "SOLUSDT", "BNBUSDT"];
const INTERVALS: &[&str] = &["1h", "4h", "1d"];
const CANDLES: u16 = 3000;
const CAPITAL: f64 = 10_000.0;
const TRAILING_STOP: f64 = 0.05;
const TAKE_PROFIT: f64 = 0.15;
const MIN_TRADES: usize = 15;

// Fees
const TAKER_FEE: f64 = 0.001;   // 0.1% per round trip
const MAKER_FEE: f64 = 0.0;     // 0.0% FDUSD maker

#[tokio::main]
async fn main() -> Result<()> {
    println!("\n{}", "━".repeat(72).bright_cyan());
    println!("{}", "  FEE COMPARISON: Taker 0.1% vs FDUSD Maker 0.0%".bright_cyan().bold());
    println!("{}", "━".repeat(72).bright_cyan());

    let loader = DataLoader::new(None, None);

    // Build strategies — same list as full_strategy_backtest
    let strategy_fns: Vec<(&str, fn() -> Box<dyn SignalGenerator>)> = vec![
        ("dynamic_trend",        || Box::new(DynamicTrend::new())),
        ("bollinger",            || Box::new(BollingerReversion::new())),
        ("rsi",                  || Box::new(RsiMeanReversion::new())),
        ("atr_breakout",         || Box::new(AtrBreakout::new())),
        ("macd_trend",           || Box::new(MacdTrend::new())),
        ("price_momentum",       || Box::new(PriceMomentum::new())),
        ("volatility_squeeze",   || Box::new(VolatilitySqueeze::new())),
        ("obv_trend",            || Box::new(ObvTrend::new())),
        ("adaptive_ma",          || Box::new(AdaptiveMaCrossover::new())),
    ];

    struct Row {
        strategy: &'static str,
        symbol: &'static str,
        interval: &'static str,
        trades: usize,
        taker_ret: f64,
        maker_ret: f64,
    }

    let mut rows: Vec<Row> = Vec::new();

    for symbol in SYMBOLS {
        for interval in INTERVALS {
            print!("Fetching {} {}... ", symbol, interval);
            let df = match loader.fetch_data(symbol, interval, CANDLES).await {
                Ok(df) => match FeatureEngine::add_technicals(&df, None) {
                    Ok(df) => { println!("✓"); df }
                    Err(e) => { println!("technicals err: {e}"); continue; }
                },
                Err(e) => { println!("fetch err: {e}"); continue; }
            };

            for (name, make_strat) in &strategy_fns {
                let mut strat = make_strat();
                let signals = match strat.predict(&df) {
                    Ok(s) => s,
                    Err(_) => continue,
                };

                let taker = Backtester::new(CAPITAL, TAKER_FEE, 5.0);
                let maker = Backtester::new(CAPITAL, MAKER_FEE, 0.0);

                let r_taker = match taker.run(&df, &signals, TRAILING_STOP, TAKE_PROFIT) {
                    Ok(r) => r,
                    Err(_) => continue,
                };
                let r_maker = match maker.run(&df, &signals, TRAILING_STOP, TAKE_PROFIT) {
                    Ok(r) => r,
                    Err(_) => continue,
                };

                if r_taker.total_trades < MIN_TRADES { continue; }

                rows.push(Row {
                    strategy: name,
                    symbol,
                    interval,
                    trades: r_taker.total_trades,
                    taker_ret: r_taker.total_return_pct,
                    maker_ret: r_maker.total_return_pct,
                });
            }
        }
    }

    // Sort by maker_ret descending
    rows.sort_by(|a, b| b.maker_ret.partial_cmp(&a.maker_ret).unwrap_or(std::cmp::Ordering::Equal));

    println!("\n{}", "━".repeat(80).bright_cyan());
    println!("{:<22} {:<10} {:<6} {:>7} {:>10} {:>10} {:>8}",
        "Strategy", "Symbol", "Int", "Trades", "Taker%", "Maker%", "Edge");
    println!("{}", "─".repeat(80));

    for r in &rows {
        let edge = r.maker_ret - r.taker_ret;
        let maker_str = if r.maker_ret > 0.0 {
            format!("{:>9.1}%", r.maker_ret).green().to_string()
        } else {
            format!("{:>9.1}%", r.maker_ret).red().to_string()
        };
        println!("{:<22} {:<10} {:<6} {:>7} {:>9.1}% {} {:>+7.1}%",
            r.strategy, r.symbol, r.interval, r.trades,
            r.taker_ret, maker_str, edge);
    }

    // Aggregate stats
    let n = rows.len() as f64;
    let avg_taker = rows.iter().map(|r| r.taker_ret).sum::<f64>() / n;
    let avg_maker = rows.iter().map(|r| r.maker_ret).sum::<f64>() / n;
    let profitable_taker = rows.iter().filter(|r| r.taker_ret > 0.0).count();
    let profitable_maker = rows.iter().filter(|r| r.maker_ret > 0.0).count();
    let avg_trades = rows.iter().map(|r| r.trades as f64).sum::<f64>() / n;
    let avg_edge = rows.iter().map(|r| r.maker_ret - r.taker_ret).sum::<f64>() / n;

    println!("{}", "━".repeat(80).bright_cyan());
    println!("\n{}", "SUMMARY".bright_yellow().bold());
    println!("  Strategies tested:      {}", rows.len());
    println!("  Avg trades per run:     {:.0}", avg_trades);
    println!("  Avg return (taker):     {:.1}%", avg_taker);
    println!("  Avg return (maker):     {:.1}%", avg_maker);
    println!("  Avg edge (maker-taker): {:.1}%", avg_edge);
    println!("  Profitable (taker):     {} / {}", profitable_taker, rows.len());
    println!("  Profitable (maker):     {} / {}", profitable_maker, rows.len());

    // Fee savings in dollar terms
    println!("\n{}", "FEE SAVINGS (per $10k position)".bright_green());
    println!("  Taker round trip cost:  ${:.2}  (0.1%)", CAPITAL * TAKER_FEE * 2.0);
    println!("  Maker round trip cost:  ${:.2}  (0.0%)", CAPITAL * MAKER_FEE * 2.0);
    println!("  Savings per trade:      ${:.2}", CAPITAL * (TAKER_FEE - MAKER_FEE) * 2.0);
    println!("  Savings on {:.0} trades:   ${:.2}", avg_trades,
        avg_trades * CAPITAL * (TAKER_FEE - MAKER_FEE) * 2.0);

    println!("\n  → The fee saving alone is worth {:.1}% over {:.0} trades",
        avg_trades * (TAKER_FEE - MAKER_FEE) * 2.0 * 100.0, avg_trades);

    Ok(())
}
