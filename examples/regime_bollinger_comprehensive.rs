//! Comprehensive RegimeBollingerReversion vs BollingerReversion comparison
//!
//! Tests across:
//! - Multiple timeframes: 1d, 4h, 1h
//! - Multiple tickers: XRP, DOGE, ADA, SOL, BTC, ETH
//! - Execution modes: Taker fee (0.05%) vs Passive (0% maker)
//!
//! Reports which strategy performs better in each configuration.

use anyhow::Result;
use colored::*;
use krypto::{
    algo::StrategyRegistry,
    backtest::{
        engine::{Backtester, PositionSizing},
        passive::{bars_per_signal, lower_interval_for_signal, PassiveConfig, TickSize},
    },
    data::{loader::DataLoader, CacheConfig},
    features::indicators::FeatureEngine,
};

const CAPITAL: f64 = 10_000.0;
const TAKER_FEE: f64 = 0.0005; // 0.05%

const TIMEFRAMES: &[&str] = &["1d", "4h", "1h"];
const SYMBOLS: &[&str] = &[
    "XRPUSDT", "DOGEUSDT", "ADAUSDT", "SOLUSDT", "BTCUSDT", "ETHUSDT",
];

// ATR stop percentages per timeframe (from research)
fn atr_stop_for_interval(interval: &str) -> f64 {
    match interval {
        "1d" => 0.032, // ~0.5× ATR for daily
        "4h" => 0.025, // tighter for 4h
        "1h" => 0.015, // very tight for 1h
        _ => 0.025,
    }
}

fn candles_for_interval(interval: &str) -> u32 {
    match interval {
        "1d" => 2000,
        "4h" => 3000,
        "1h" => 5000,
        _ => 2000,
    }
}

#[derive(Debug)]
struct TestResult {
    symbol: String,
    interval: String,
    strategy: String,
    execution: String,
    trades: usize,
    win_rate: f64,
    profit_factor: f64,
    ann_return: f64,
    ann_sharpe: f64,
    max_dd: f64,
    trades_per_year: f64,
    fill_rate: f64,
    price_improvement: f64,
}

fn print_header() {
    println!(
        "\n{:<10} {:<4} {:<12} {:<8} {:>5} {:>5} {:>5} {:>8} {:>7} {:>5} {:>5} {:>8} {:>8}",
        "Symbol",
        "TF",
        "Strategy",
        "Exec",
        "Trd",
        "WR%",
        "PF",
        "AnnR%",
        "Shp",
        "DD%",
        "FReq",
        "Trd/Yr",
        "PriceImpr"
    );
    println!("{}", "─".repeat(110));
}

fn print_result(r: &TestResult) {
    let ret_color = if r.ann_return > 0.0 {
        r.ann_return.to_string().green()
    } else {
        r.ann_return.to_string().red()
    };
    let shp_color = if r.ann_sharpe > 0.0 {
        format!("{:.1}", r.ann_sharpe).green()
    } else {
        format!("{:.1}", r.ann_sharpe).red()
    };

    println!(
        "{:<10} {:<4} {:<12} {:<8} {:>5} {:>5.0}% {:>5.1} {:>8} {:>7} {:>5.0}% {:>5.0}% {:>6.1}/yr {:>6.2}%",
        r.symbol,
        r.interval,
        r.strategy,
        r.execution,
        r.trades,
        r.win_rate,
        r.profit_factor,
        ret_color,
        shp_color,
        r.max_dd,
        r.fill_rate * 100.0,
        r.trades_per_year,
        r.price_improvement * 100.0,
    );
}

#[tokio::main]
async fn main() -> Result<()> {
    println!("\n{}", "━".repeat(90).bright_cyan());
    println!(
        "{}",
        "  REGIME vs STANDARD BOLLINGER - COMPREHENSIVE COMPARISON"
            .bright_cyan()
            .bold()
    );
    println!("{}", "━".repeat(90).bright_cyan());
    println!(
        "  Testing: {} timeframes × {} symbols × 2 strategies × 2 execution modes",
        TIMEFRAMES.len(),
        SYMBOLS.len()
    );
    println!(
        "  Total configurations: {}\n",
        TIMEFRAMES.len() * SYMBOLS.len() * 4
    );

    let cache = CacheConfig {
        enabled: true,
        cache_dir: "data/cache".into(),
    };
    let loader = DataLoader::with_cache(None, None, cache);
    let registry = StrategyRegistry::new();

    let mut results: Vec<TestResult> = Vec::new();
    let mut errors: Vec<String> = Vec::new();

    for &interval in TIMEFRAMES {
        println!(
            "\n{}",
            format!("══ {} INTERVAL ══", interval)
                .bright_yellow()
                .bold()
        );
        print_header();

        for &symbol in SYMBOLS {
            let candles = candles_for_interval(interval);
            let atr_stop = atr_stop_for_interval(interval);

            // Fetch data
            let df = match loader.fetch_data(symbol, interval, candles).await {
                Ok(df) => df,
                Err(e) => {
                    errors.push(format!("{} {} load failed: {}", symbol, interval, e));
                    continue;
                }
            };

            let df = match FeatureEngine::add_technicals(&df, None) {
                Ok(df) => df,
                Err(e) => {
                    errors.push(format!("{} {} features failed: {}", symbol, interval, e));
                    continue;
                }
            };

            // Test both strategies
            for strat_name in &["bollinger_reversion", "regime_bollinger_reversion"] {
                let strategy = registry.create(strat_name).unwrap();
                let signals = match strategy.predict(&df) {
                    Ok(s) => s,
                    Err(e) => {
                        errors.push(format!(
                            "{} {} {} predict failed: {}",
                            symbol, interval, strat_name, e
                        ));
                        continue;
                    }
                };

                // Taker execution
                let bt = Backtester::new(CAPITAL, TAKER_FEE, 5.0)
                    .with_position_sizing(PositionSizing::Full);

                if let Ok(r) = bt.run(&df, &signals, atr_stop, 0.0) {
                    let result = TestResult {
                        symbol: symbol.replace("USDT", ""),
                        interval: interval.to_string(),
                        strategy: if *strat_name == "bollinger_reversion" {
                            "Standard"
                        } else {
                            "Regime"
                        }
                        .to_string(),
                        execution: "Taker".to_string(),
                        trades: r.total_trades,
                        win_rate: r.win_rate,
                        profit_factor: r.profit_factor,
                        ann_return: r.annualised_return_pct,
                        ann_sharpe: r.annualised_sharpe,
                        max_dd: r.max_drawdown_pct,
                        trades_per_year: r.trades_per_year,
                        fill_rate: 1.0,
                        price_improvement: 0.0,
                    };
                    print_result(&result);
                    results.push(result);
                }

                // Passive execution (only for FDUSD pairs that exist)
                let fdusd_sym = symbol.replace("USDT", "FDUSD");
                let lower_interval = lower_interval_for_signal(interval);
                let lower_candles = (candles as usize * bars_per_signal(interval) * 2) as u32;

                // Try to fetch lower-TF data for passive
                let df_low = loader
                    .fetch_data(&fdusd_sym, lower_interval, lower_candles)
                    .await;

                if let Ok(df_low) = df_low {
                    let tick = TickSize::fetch(&fdusd_sym)
                        .await
                        .unwrap_or(TickSize::from_value(0.0001));

                    let passive_cfg = PassiveConfig {
                        ticks_below_open: 3,
                        tick_size: tick,
                        max_wait_bars: bars_per_signal(interval),
                        maker_fee: 0.0,
                        update_threshold_ticks: None,
                        anchor_to_signal: true,
                    };

                    let bt = Backtester::new(CAPITAL, 0.0, 0.0)
                        .with_position_sizing(PositionSizing::Full);

                    if let Ok((r, stats)) = bt
                        .run_with_passive(&df, &df_low, &signals, atr_stop, 0.0, passive_cfg)
                        .await
                    {
                        let result = TestResult {
                            symbol: symbol.replace("USDT", ""),
                            interval: interval.to_string(),
                            strategy: if *strat_name == "bollinger_reversion" {
                                "Standard"
                            } else {
                                "Regime"
                            }
                            .to_string(),
                            execution: "Passive".to_string(),
                            trades: r.total_trades,
                            win_rate: r.win_rate,
                            profit_factor: r.profit_factor,
                            ann_return: r.annualised_return_pct,
                            ann_sharpe: r.annualised_sharpe,
                            max_dd: r.max_drawdown_pct,
                            trades_per_year: r.trades_per_year,
                            fill_rate: stats.fill_rate,
                            price_improvement: stats.avg_price_improvement_ticks,
                        };
                        print_result(&result);
                        results.push(result);
                    }
                }
            }
        }
    }

    // ── ANALYSIS ─────────────────────────────────────────────────────────────
    println!("\n\n{}", "━".repeat(90).bright_cyan());
    println!("{}", "  ANALYSIS: REGIME vs STANDARD".bright_cyan().bold());
    println!("{}", "━".repeat(90).bright_cyan());

    // Group by timeframe and compare
    for &interval in TIMEFRAMES {
        println!("\n{} {}", "═".repeat(40), interval);

        let taker_std: Vec<_> = results
            .iter()
            .filter(|r| {
                r.interval == interval && r.strategy == "Standard" && r.execution == "Taker"
            })
            .collect();
        let taker_reg: Vec<_> = results
            .iter()
            .filter(|r| r.interval == interval && r.strategy == "Regime" && r.execution == "Taker")
            .collect();

        if !taker_std.is_empty() && !taker_reg.is_empty() {
            let avg_shp_std: f64 =
                taker_std.iter().map(|r| r.ann_sharpe).sum::<f64>() / taker_std.len() as f64;
            let avg_shp_reg: f64 =
                taker_reg.iter().map(|r| r.ann_sharpe).sum::<f64>() / taker_reg.len() as f64;
            let avg_dd_std: f64 =
                taker_std.iter().map(|r| r.max_dd).sum::<f64>() / taker_std.len() as f64;
            let avg_dd_reg: f64 =
                taker_reg.iter().map(|r| r.max_dd).sum::<f64>() / taker_reg.len() as f64;
            let avg_ret_std: f64 =
                taker_std.iter().map(|r| r.ann_return).sum::<f64>() / taker_std.len() as f64;
            let avg_ret_reg: f64 =
                taker_reg.iter().map(|r| r.ann_return).sum::<f64>() / taker_reg.len() as f64;

            println!(
                "  Standard: AvgRet={:.1}%  AvgSharpe={:.2}  AvgDD={:.1}%",
                avg_ret_std, avg_shp_std, avg_dd_std
            );
            println!(
                "  Regime:   AvgRet={:.1}%  AvgSharpe={:.2}  AvgDD={:.1}%",
                avg_ret_reg, avg_shp_reg, avg_dd_reg
            );

            let shp_delta = avg_shp_reg - avg_shp_std;
            let dd_delta = avg_dd_std - avg_dd_reg;

            if shp_delta > 0.0 {
                println!(
                    "  {} Regime filter improves Sharpe by {:.2}",
                    "✓".green(),
                    shp_delta
                );
            } else {
                println!(
                    "  {} Regime filter reduces Sharpe by {:.2}",
                    "✗".red(),
                    shp_delta.abs()
                );
            }

            if dd_delta > 0.0 {
                println!(
                    "  {} Regime filter reduces DD by {:.1}%",
                    "✓".green(),
                    dd_delta
                );
            } else {
                println!(
                    "  {} Regime filter increases DD by {:.1}%",
                    "✗".red(),
                    dd_delta.abs()
                );
            }
        }
    }

    // ── BEST CONFIGURATIONS ─────────────────────────────────────────────────────
    println!("\n\n{}", "━".repeat(90).bright_cyan());
    println!(
        "{}",
        "  TOP 10 BY SHARPE (All Configurations)"
            .bright_cyan()
            .bold()
    );
    println!("{}", "━".repeat(90).bright_cyan());

    let mut sorted: Vec<_> = results.iter().filter(|r| r.trades >= 10).collect();
    sorted.sort_by(|a, b| b.ann_sharpe.partial_cmp(&a.ann_sharpe).unwrap());

    print_header();
    for r in sorted.iter().take(10) {
        print_result(r);
    }

    // ── REGIME WINS ─────────────────────────────────────────────────────────────
    println!("\n\n{}", "━".repeat(90).bright_cyan());
    println!(
        "{}",
        "  WHERE REGIME BEATS STANDARD (Taker, Same TF)"
            .bright_cyan()
            .bold()
    );
    println!("{}", "━".repeat(90).bright_cyan());

    print_header();
    let mut regime_wins = 0;
    let mut std_wins = 0;

    for &interval in TIMEFRAMES {
        for &symbol in SYMBOLS {
            let std_r = results.iter().find(|r| {
                r.symbol == symbol.replace("USDT", "")
                    && r.interval == interval
                    && r.strategy == "Standard"
                    && r.execution == "Taker"
            });
            let reg_r = results.iter().find(|r| {
                r.symbol == symbol.replace("USDT", "")
                    && r.interval == interval
                    && r.strategy == "Regime"
                    && r.execution == "Taker"
            });

            if let (Some(s), Some(r)) = (std_r, reg_r) {
                if r.ann_sharpe > s.ann_sharpe {
                    print_result(r);
                    regime_wins += 1;
                } else {
                    std_wins += 1;
                }
            }
        }
    }

    println!(
        "\n  Regime wins: {} | Standard wins: {}",
        regime_wins, std_wins
    );

    // ── ERRORS ───────────────────────────────────────────────────────────────
    if !errors.is_empty() {
        println!("\n{}", "━".repeat(90).dimmed());
        println!("{}", "  ERRORS".dimmed());
        println!("{}", "━".repeat(90).dimmed());
        for e in &errors {
            println!("  {}", e.dimmed());
        }
    }

    println!();
    Ok(())
}
