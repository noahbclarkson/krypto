//! RegimeBollingerReversion Strategy Test
//!
//! Compares standard BollingerReversion against RegimeBollingerReversion
//! on the top USDT candidates to measure the regime filter's impact.
//!
//! Hypothesis: regime filter should reduce drawdown while maintaining
//! edge in range-bound markets.

use anyhow::Result;
use colored::*;
use krypto::{
    algo::StrategyRegistry,
    backtest::engine::{BacktestResult, Backtester, PositionSizing},
    data::{loader::DataLoader, CacheConfig},
    features::indicators::FeatureEngine,
};

const CAPITAL: f64 = 10_000.0;
const CANDLES: u32 = 3000;
const TAKER_FEE: f64 = 0.0005;
const INTERVAL: &str = "1d";
const ATR_STOP: f64 = 0.032; // ~0.5× ATR for XRP/DOGE

const SYMBOLS: &[&str] = &["XRPUSDT", "DOGEUSDT", "ADAUSDT", "SOLUSDT", "BTCUSDT"];

fn print_result(name: &str, r: &BacktestResult) {
    let ann_r = format!("{:.1}%", r.annualised_return_pct);
    let ann_r_colored = if r.annualised_return_pct > 0.0 {
        ann_r.green()
    } else {
        ann_r.red()
    };
    println!(
        "    {:<28} trades={:>4}  WR={:.1}%  PF={:.2}  AnnRet={}  AnnShp={:.1}  DD={:.1}%",
        name,
        r.total_trades,
        r.win_rate,
        r.profit_factor,
        ann_r_colored,
        r.annualised_sharpe,
        r.max_drawdown_pct,
    );
}

#[tokio::main]
async fn main() -> Result<()> {
    println!("\n{}", "━".repeat(90).bright_cyan());
    println!(
        "{}",
        "  REGIME BOLLINGER REVERSION vs STANDARD BOLLINGER"
            .bright_cyan()
            .bold()
    );
    println!("{}", "━".repeat(90).bright_cyan());
    println!(
        "  Interval: {}  |  ATR Stop: {:.1}%  |  Fee: {:.3}%",
        INTERVAL,
        ATR_STOP * 100.0,
        TAKER_FEE * 100.0
    );
    println!();

    let cache = CacheConfig {
        enabled: true,
        cache_dir: "data/cache".into(),
    };
    let loader = DataLoader::with_cache(None, None, cache);
    let registry = StrategyRegistry::new();

    let mut improvement_count = 0;
    let mut total_dd_reduction = 0.0;
    let mut valid_pairs = 0;

    for &symbol in SYMBOLS {
        println!("{}", format!("  {}", symbol).bright_white().bold());

        let df = match loader.fetch_data(symbol, INTERVAL, CANDLES).await {
            Ok(df) => df,
            Err(e) => {
                println!("    Failed to load: {}", e);
                continue;
            }
        };

        let df = FeatureEngine::add_technicals(&df, None)?;
        let bt =
            Backtester::new(CAPITAL, TAKER_FEE, 5.0).with_position_sizing(PositionSizing::Full);

        // Standard bollinger
        let standard = registry.create("bollinger_reversion").unwrap();
        let signals_std = standard.predict(&df)?;
        let result_std = bt.run(&df, &signals_std, ATR_STOP, 0.0);

        // Regime bollinger
        let regime = registry.create("regime_bollinger_reversion").unwrap();
        let signals_reg = regime.predict(&df)?;
        let result_reg = bt.run(&df, &signals_reg, ATR_STOP, 0.0);

        match (&result_std, &result_reg) {
            (Ok(r_std), Ok(r_reg)) => {
                print_result("BollingerReversion (standard)", r_std);
                print_result("RegimeBollingerReversion     ", r_reg);

                let dd_delta = r_std.max_drawdown_pct - r_reg.max_drawdown_pct;
                let shp_delta = r_reg.annualised_sharpe - r_std.annualised_sharpe;
                let symbol_str = if shp_delta > 0.0 {
                    format!(
                        "    ↑ Regime filter: DD {:.1}% better, Sharpe {:+.1}",
                        dd_delta, shp_delta
                    )
                    .green()
                } else {
                    format!(
                        "    ↓ Regime filter: DD {:.1}% change, Sharpe {:+.1}",
                        dd_delta, shp_delta
                    )
                    .yellow()
                };
                println!("{}", symbol_str);

                if shp_delta > 0.0 {
                    improvement_count += 1;
                }
                total_dd_reduction += dd_delta;
                valid_pairs += 1;
            }
            (Err(e), _) => println!("    Standard error: {}", e),
            (_, Err(e)) => println!("    Regime error: {}", e),
        }
        println!();
    }

    if valid_pairs > 0 {
        println!("{}", "━".repeat(90).bright_cyan());
        println!("{}", "  SUMMARY".bright_cyan().bold());
        println!(
            "  Regime filter improved Sharpe: {}/{}",
            improvement_count, valid_pairs
        );
        println!(
            "  Avg DD reduction: {:.1}%",
            total_dd_reduction / valid_pairs as f64
        );

        if improvement_count > valid_pairs / 2 {
            println!(
                "{}",
                "  ✓ Regime filter shows consistent improvement".green()
            );
        } else {
            println!("{}", "  ✗ Regime filter needs tuning".yellow());
        }
    }

    println!();
    Ok(())
}
