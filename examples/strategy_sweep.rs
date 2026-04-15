//! Parameter sweep for top strategies with ATR-based adaptive stops.
//!
//! Uses ATR multipliers instead of fixed % stops so that stop sizing
//! is automatically scaled to the asset's volatility at the given timeframe.
//!
//! Results are ranked by annualised Sharpe ratio to enable fair comparison
//! across timeframes (1d vs 1h cover very different time periods).

use anyhow::Result;
use colored::*;
use krypto::{
    algo::StrategyRegistry,
    backtest::engine::{Backtester, PositionSizing},
    data::{loader::DataLoader, CacheConfig},
    features::indicators::FeatureEngine,
};
use polars::prelude::*;
use std::collections::HashMap;

const CANDLES: u32 = 3000;
const CAPITAL: f64 = 10_000.0;
const MIN_TRADES: usize = 30; // Minimum trades for a result to be considered reliable

const SYMBOLS: &[&str] = &[
    "BTCUSDT", "ETHUSDT", "SOLUSDT", "BNBUSDT", "XRPUSDT", "ADAUSDT", "DOGEUSDT", "AVAXUSDT",
    "DOTUSDT", "LINKUSDT",
];
const INTERVALS: &[&str] = &["4h", "1d"];
const STRATEGIES: &[&str] = &["bollinger_reversion", "volatility_squeeze"];

// ATR multipliers for stop sizing (stop = atr_mult × ATR(14) / close)
const ATR_MULTS: &[f64] = &[0.5, 1.0, 1.5, 2.0, 3.0];
// Take-profit as ATR multiples (0 = no TP)
const TP_MULTS: &[f64] = &[0.0, 2.0, 3.0];

#[derive(Debug, Clone)]
struct SweepResult {
    strategy: String,
    symbol: String,
    interval: String,
    atr_stop_mult: f64,
    atr_tp_mult: f64,
    effective_stop_pct: f64,
    effective_tp_pct: f64,
    trades: usize,
    trades_per_year: f64,
    win_rate: f64,
    pf: f64,
    total_return_pct: f64,
    annualised_return_pct: f64,
    annualised_sharpe: f64,
    return_per_trade_pct: f64,
    max_dd: f64,
    backtest_years: f64,
}

impl SweepResult {
    /// Primary ranking score: annualised Sharpe, but only for results with
    /// enough trades to be statistically meaningful.
    fn score(&self) -> f64 {
        if self.trades < MIN_TRADES || self.pf <= 0.0 || self.annualised_return_pct <= 0.0 {
            return f64::NEG_INFINITY;
        }
        // Weight annualised Sharpe by trade count reliability (log scale)
        let reliability = (self.trades as f64 / MIN_TRADES as f64).ln().max(0.0) + 1.0;
        self.annualised_sharpe * reliability
    }
}

/// Compute median ATR as a fraction of close price for a dataset.
/// Used to convert ATR multipliers to effective stop percentages.
fn compute_median_atr_pct(df: &DataFrame) -> f64 {
    let atr = df.column("atr").and_then(|s| s.f64()).ok();
    let close = df.column("close").and_then(|s| s.f64()).ok();

    match (atr, close) {
        (Some(atr_ca), Some(close_ca)) => {
            let mut ratios: Vec<f64> = atr_ca
                .into_iter()
                .zip(close_ca)
                .filter_map(|(a, c)| match (a, c) {
                    (Some(av), Some(cv)) if cv > 0.0 => Some(av / cv),
                    _ => None,
                })
                .collect();
            if ratios.is_empty() {
                return 0.02; // fallback 2%
            }
            ratios.sort_by(|a, b| a.partial_cmp(b).unwrap());
            ratios[ratios.len() / 2]
        }
        _ => 0.02,
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    println!("\n{}", "━".repeat(70).bright_cyan());
    println!(
        "{}",
        "  STRATEGY PARAMETER SWEEP (ATR-BASED STOPS)"
            .bright_cyan()
            .bold()
    );
    println!("{}", "━".repeat(70).bright_cyan());
    println!("  ATR stop multipliers: {:?}", ATR_MULTS);
    println!("  ATR TP multipliers:   {:?}", TP_MULTS);
    println!("  Strategies: {:?}", STRATEGIES);
    println!("  Symbols:    {:?}", SYMBOLS);
    println!(
        "  Ranked by:  Annualised Sharpe (min {} trades)",
        MIN_TRADES
    );
    println!();

    let cache = CacheConfig {
        enabled: true,
        cache_dir: "data/cache".into(),
    };
    let loader = DataLoader::with_cache(None, None, cache);
    let registry = StrategyRegistry::new();

    // Load all data upfront
    let mut data: HashMap<String, DataFrame> = HashMap::new();
    let mut atr_pcts: HashMap<String, f64> = HashMap::new();

    for sym in SYMBOLS {
        for int in INTERVALS {
            let key = format!("{sym}_{int}");
            if data.contains_key(&key) {
                continue;
            }
            print!("  Loading {}... ", key);
            match loader.fetch_data(sym, int, CANDLES).await {
                Ok(df) => {
                    let df = FeatureEngine::add_technicals(&df, None)?;
                    let atr_pct = compute_median_atr_pct(&df);
                    println!(
                        "{} bars  |  median ATR = {:.2}%",
                        df.height(),
                        atr_pct * 100.0
                    );
                    atr_pcts.insert(key.clone(), atr_pct);
                    data.insert(key, df);
                }
                Err(e) => println!("{} {}", "✗".red(), e),
            }
        }
    }
    println!();

    let total = STRATEGIES.len() * data.len() * ATR_MULTS.len() * TP_MULTS.len();
    let mut results: Vec<SweepResult> = Vec::new();
    let mut done = 0;

    for strat_name in STRATEGIES {
        for (key, df) in &data {
            let parts: Vec<&str> = key.splitn(2, '_').collect();
            let (sym, int) = (parts[0], parts.get(1).copied().unwrap_or("?"));
            let base_atr = atr_pcts.get(key).copied().unwrap_or(0.02);

            for &atr_stop_mult in ATR_MULTS {
                let effective_stop = (base_atr * atr_stop_mult).clamp(0.005, 0.50);

                for &atr_tp_mult in TP_MULTS {
                    let effective_tp = if atr_tp_mult > 0.0 {
                        (base_atr * atr_tp_mult).clamp(0.005, 1.0)
                    } else {
                        0.0
                    };

                    done += 1;
                    print!(
                        "\r  [{}/{}] {} {} {} stop={:.1}×ATR({:.1}%) tp={:.1}×ATR   ",
                        done,
                        total,
                        strat_name,
                        sym,
                        int,
                        atr_stop_mult,
                        effective_stop * 100.0,
                        atr_tp_mult,
                    );

                    let strat = match registry.create(strat_name) {
                        Some(s) => s,
                        None => continue,
                    };

                    let signals = match strat.predict(df) {
                        Ok(s) => s,
                        Err(_) => continue,
                    };

                    let bt = Backtester::with_defaults(CAPITAL)
                        .with_position_sizing(PositionSizing::Full);
                    let res = match bt.run(df, &signals, effective_stop, effective_tp) {
                        Ok(r) => r,
                        Err(_) => continue,
                    };

                    if res.total_trades > 0 {
                        results.push(SweepResult {
                            strategy: strat_name.to_string(),
                            symbol: sym.to_string(),
                            interval: int.to_string(),
                            atr_stop_mult,
                            atr_tp_mult,
                            effective_stop_pct: effective_stop * 100.0,
                            effective_tp_pct: effective_tp * 100.0,
                            trades: res.total_trades,
                            trades_per_year: res.trades_per_year,
                            win_rate: res.win_rate,
                            pf: res.profit_factor,
                            total_return_pct: res.total_return_pct,
                            annualised_return_pct: res.annualised_return_pct,
                            annualised_sharpe: res.annualised_sharpe,
                            return_per_trade_pct: res.return_per_trade_pct,
                            max_dd: res.max_drawdown_pct,
                            backtest_years: res.backtest_years,
                        });
                    }
                }
            }
        }
    }
    println!("\n");

    // Sort by score (annualised Sharpe × reliability weight)
    results.sort_by(|a, b| b.score().partial_cmp(&a.score()).unwrap());

    // ── Top 20 ──────────────────────────────────────────────────────────────
    println!("{}", "━".repeat(110).bright_cyan());
    println!(
        "{}",
        "  TOP 20 COMBINATIONS (ranked by annualised Sharpe)"
            .bright_cyan()
            .bold()
    );
    println!("{}", "━".repeat(110).bright_cyan());
    println!(
        "  {:<22} {:<10} {:<4} {:>7} {:>7} {:>6} {:>7} {:>6} {:>8} {:>8} {:>8}",
        "Strategy",
        "Symbol",
        "Int",
        "Stop%",
        "TP%",
        "Trades",
        "Tr/yr",
        "Win%",
        "Ann.Ret%",
        "Ann.Shp",
        "TotalRet%"
    );
    println!("{}", "─".repeat(110));

    for (i, r) in results.iter().take(20).enumerate() {
        let ann_ret = if r.annualised_return_pct > 0.0 {
            format!("{:>8.1}", r.annualised_return_pct)
                .green()
                .to_string()
        } else {
            format!("{:>8.1}", r.annualised_return_pct)
                .red()
                .to_string()
        };
        let shp = if r.annualised_sharpe > 1.0 {
            format!("{:>8.2}", r.annualised_sharpe).green().to_string()
        } else {
            format!("{:>8.2}", r.annualised_sharpe).yellow().to_string()
        };
        println!(
            "  {:>2}. {:<20} {:<10} {:<4} {:>6.1}% {:>6.1}% {:>6} {:>7.1} {:>6.1}% {} {} {:>9.0}%",
            i + 1,
            r.strategy,
            r.symbol,
            r.interval,
            r.effective_stop_pct,
            r.effective_tp_pct,
            r.trades,
            r.trades_per_year,
            r.win_rate,
            ann_ret,
            shp,
            r.total_return_pct,
        );
    }

    // ── Best per strategy ────────────────────────────────────────────────────
    println!("\n{}", "━".repeat(70).bright_cyan());
    println!("{}", "  BEST CONFIG PER STRATEGY".bright_cyan().bold());
    println!("{}", "━".repeat(70).bright_cyan());

    for strat in STRATEGIES {
        if let Some(best) = results.iter().find(|r| r.strategy == *strat) {
            println!("\n  {}:", strat.bright_cyan());
            println!("    Symbol:          {}", best.symbol.yellow());
            println!(
                "    Interval:        {} ({:.1}y covered)",
                best.interval, best.backtest_years
            );
            println!(
                "    Stop:            {:.1}× ATR = {:.2}%",
                best.atr_stop_mult, best.effective_stop_pct
            );
            println!(
                "    TP:              {:.1}× ATR = {:.2}%",
                best.atr_tp_mult, best.effective_tp_pct
            );
            println!(
                "    Trades:          {} ({:.1}/yr)",
                best.trades, best.trades_per_year
            );
            println!("    Win Rate:        {:.1}%", best.win_rate);
            println!("    PF:              {:.2}", best.pf);
            println!("    Total Return:    {:.1}%", best.total_return_pct);
            println!("    Annualised Ret:  {:.1}%", best.annualised_return_pct);
            println!("    Annualised Shp:  {:.2}", best.annualised_sharpe);
            println!("    Return/Trade:    {:.2}%", best.return_per_trade_pct);
            println!("    Max DD:          {:.1}%", best.max_dd);
        }
    }

    // ── ATR multiplier analysis ──────────────────────────────────────────────
    println!("\n{}", "━".repeat(70).bright_cyan());
    println!("{}", "  ATR MULTIPLIER ANALYSIS".bright_cyan().bold());
    println!("{}", "━".repeat(70).bright_cyan());

    let mut mult_stats: std::collections::BTreeMap<u64, (f64, f64, usize)> =
        std::collections::BTreeMap::new();
    for r in &results {
        let key = (r.atr_stop_mult * 10.0).round() as u64;
        let entry = mult_stats.entry(key).or_insert((0.0, 0.0, 0));
        entry.0 += r.annualised_return_pct;
        entry.1 += r.annualised_sharpe;
        entry.2 += 1;
    }
    println!("\n  Avg metrics by ATR stop multiplier:");
    println!(
        "  {:>6}  {:>12}  {:>12}  {:>6}",
        "Mult", "Ann.Return%", "Ann.Sharpe", "Runs"
    );
    for (mult_x10, (sum_ret, sum_shp, count)) in &mult_stats {
        let avg_ret = sum_ret / *count as f64;
        let avg_shp = sum_shp / *count as f64;
        let mult = *mult_x10 as f64 / 10.0;
        let ret_str = if avg_ret > 0.0 {
            format!("{avg_ret:>12.1}").green().to_string()
        } else {
            format!("{avg_ret:>12.1}").red().to_string()
        };
        println!(
            "  {:>6.1}×   {}   {:>12.2}  {:>6}",
            mult, ret_str, avg_shp, count
        );
    }

    // ── Timeframe analysis ───────────────────────────────────────────────────
    println!("\n  Avg metrics by timeframe:");
    println!(
        "  {:>4}  {:>12}  {:>12}  {:>8}  {:>6}",
        "Int", "Ann.Return%", "Ann.Sharpe", "Tr/yr", "Runs"
    );
    let mut int_stats: HashMap<String, (f64, f64, f64, usize)> = HashMap::new();
    for r in &results {
        let e = int_stats
            .entry(r.interval.clone())
            .or_insert((0.0, 0.0, 0.0, 0));
        e.0 += r.annualised_return_pct;
        e.1 += r.annualised_sharpe;
        e.2 += r.trades_per_year;
        e.3 += 1;
    }
    let mut int_vec: Vec<_> = int_stats.into_iter().collect();
    int_vec.sort_by(|a, b| {
        (b.1 .1 / b.1 .3 as f64)
            .partial_cmp(&(a.1 .1 / a.1 .3 as f64))
            .unwrap()
    });
    for (int, (sum_ret, sum_shp, sum_tpy, count)) in int_vec {
        let avg_ret = sum_ret / count as f64;
        let avg_shp = sum_shp / count as f64;
        let avg_tpy = sum_tpy / count as f64;
        let ret_str = if avg_ret > 0.0 {
            format!("{avg_ret:>12.1}").green().to_string()
        } else {
            format!("{avg_ret:>12.1}").red().to_string()
        };
        println!(
            "  {:>4}   {}   {:>12.2}  {:>8.1}  {:>6}",
            int, ret_str, avg_shp, avg_tpy, count
        );
    }

    println!();
    Ok(())
}
