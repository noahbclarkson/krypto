//! FDUSD Passive Execution Validation
//!
//! Takes the top strategy candidates identified by the USDT ATR sweep
//! and validates them on real FDUSD pairs using passive limit-order execution
//! (0% maker fee).
//!
//! ## What this tests
//!
//! For each candidate:
//! 1. Run on USDT pair with standard taker fee (0.05%) — baseline
//! 2. Run on FDUSD pair with passive execution (0% maker fee) — real scenario
//! 3. Compare: fill rate, price improvement, annualised return, Sharpe
//!
//! FDUSD data is shorter (~1-2y vs 7-8y for USDT) so direct return comparison
//! is not meaningful — use annualised metrics and Sharpe for comparison.
//!
//! ## Top candidates (from USDT ATR sweep, ranked by annualised Sharpe)
//!
//! | Symbol  | Strategy            | Interval | ATR mult | Ann. Return | Ann. Sharpe |
//! |---------|---------------------|----------|----------|-------------|-------------|
//! | XRP     | bollinger_reversion | 1d       | 0.5×     | +90%        | 6060        |
//! | DOGE    | bollinger_reversion | 1d       | 0.5×     | +125%       | 5404        |
//! | ADA     | bollinger_reversion | 1d       | 0.5×     | +62%        | 1035        |
//! | LINK    | bollinger_reversion | 1d       | 0.5×     | +66%        | 762         |
//! | BNB     | bollinger_reversion | 1d       | 0.5×     | +48%        | 284         |
//! | SOL     | bollinger_reversion | 1d       | 0.5×     | (sweep)     | (sweep)     |
//! | BTC     | bollinger_reversion | 1d       | 0.5×     | (sweep)     | (sweep)     |
//! | ETH     | bollinger_reversion | 1d       | 0.5×     | (sweep)     | (sweep)     |

use anyhow::Result;
use colored::*;
use krypto::{
    algo::StrategyRegistry,
    backtest::{
        engine::{Backtester, BacktestResult, PositionSizing},
        passive::{bars_per_signal, lower_interval_for_signal, PassiveConfig, TickSize},
    },
    data::{loader::DataLoader, CacheConfig},
    features::indicators::FeatureEngine,
};

const CAPITAL: f64 = 10_000.0;
const CANDLES: u32 = 3000;
// For FDUSD pairs (shorter history), fetch fewer candles
const FDUSD_CANDLES: u32 = 1500;
// Taker fee for USDT baseline
const TAKER_FEE: f64 = 0.0005; // 0.05%
const STRATEGY: &str = "bollinger_reversion";
const INTERVAL: &str = "1d";
// ATR multiplier that swept best (0.5×)
const ATR_STOP_MULT: f64 = 0.5;

/// Candidate: symbol root + expected ATR % stop (pre-computed from USDT sweep)
struct Candidate {
    root: &'static str,
    /// Effective stop % from USDT sweep (ATR_STOP_MULT × median ATR%)
    stop_pct: f64,
}

const CANDIDATES: &[Candidate] = &[
    Candidate { root: "XRP",  stop_pct: 0.0317 },
    Candidate { root: "DOGE", stop_pct: 0.0362 },
    Candidate { root: "ADA",  stop_pct: 0.0362 },
    Candidate { root: "LINK", stop_pct: 0.0387 },
    Candidate { root: "BNB",  stop_pct: 0.0281 },
    Candidate { root: "SOL",  stop_pct: 0.0350 },
    Candidate { root: "BTC",  stop_pct: 0.0180 },
    Candidate { root: "ETH",  stop_pct: 0.0200 },
];

#[derive(Debug)]
struct ValidationRow {
    symbol_root: String,
    /// USDT taker result
    usdt_result: Option<BacktestResult>,
    /// FDUSD passive result
    fdusd_result: Option<BacktestResult>,
    fdusd_fill_rate: f64,
    fdusd_avg_price_improvement_ticks: f64,
    fdusd_avg_bars_to_fill: f64,
}

fn print_result_row(label: &str, r: &BacktestResult) {
    println!(
        "    {}  trades={:>4}  WR={:>5.1}%  PF={:>5.2}  Ann.Ret={:>8.1}%  Ann.Shp={:>8.2}  DD={:>5.1}%  Tr/yr={:>5.1}",
        label,
        r.total_trades,
        r.win_rate,
        r.profit_factor,
        r.annualised_return_pct,
        r.annualised_sharpe,
        r.max_drawdown_pct,
        r.trades_per_year,
    );
}

#[tokio::main]
async fn main() -> Result<()> {
    println!("\n{}", "━".repeat(80).bright_cyan());
    println!("{}", "  FDUSD PASSIVE EXECUTION VALIDATION".bright_cyan().bold());
    println!("{}", "━".repeat(80).bright_cyan());
    println!("  Strategy:  {}", STRATEGY.yellow());
    println!("  Interval:  {}", INTERVAL.yellow());
    println!("  ATR Stop:  {:.1}× ATR", ATR_STOP_MULT);
    println!("  Lower TF:  {} (adaptive, {} bars/signal bar)",
        lower_interval_for_signal(INTERVAL),
        bars_per_signal(INTERVAL));
    println!("  USDT fee:  {:.3}% taker", TAKER_FEE * 100.0);
    println!("  FDUSD fee: 0.000% maker (passive limit orders)");
    println!();

    let cache = CacheConfig { enabled: true, cache_dir: "data/cache".into() };
    let loader = DataLoader::with_cache(None, None, cache);
    let registry = StrategyRegistry::new();
    let lower_interval = lower_interval_for_signal(INTERVAL);
    let max_wait = bars_per_signal(INTERVAL);

    let mut rows: Vec<ValidationRow> = Vec::new();

    for cand in CANDIDATES {
        let usdt_sym = format!("{}USDT", cand.root);
        let fdusd_sym = format!("{}FDUSD", cand.root);
        let stop = cand.stop_pct;

        println!("{}", format!("  {} / {}", usdt_sym, fdusd_sym).bright_white().bold());

        // ── USDT baseline ─────────────────────────────────────────────────
        let usdt_result = match loader.fetch_data(&usdt_sym, INTERVAL, CANDLES).await {
            Ok(df) => {
                let df = FeatureEngine::add_technicals(&df, None)?;
                let strat = registry.create(STRATEGY).unwrap();
                let signals = strat.predict(&df)?;
                let bt = Backtester::new(CAPITAL, TAKER_FEE, 5.0)
                    .with_position_sizing(PositionSizing::Full);
                match bt.run(&df, &signals, stop, 0.0) {
                    Ok(r) => {
                        print_result_row("USDT taker  ", &r);
                        Some(r)
                    }
                    Err(e) => { println!("    USDT error: {}", e); None }
                }
            }
            Err(e) => { println!("    USDT load failed: {}", e); None }
        };

        // ── FDUSD passive ──────────────────────────────────────────────────
        let tick = match TickSize::fetch(&fdusd_sym).await {
            Ok(t) => t,
            Err(_) => TickSize::from_value(0.0001),
        };

        let passive_cfg = PassiveConfig {
            ticks_below_open: 3,
            tick_size: tick,
            max_wait_bars: max_wait,
            maker_fee: 0.0,
            update_threshold_ticks: None,
            anchor_to_signal: true,
        };

        let fdusd_result = match loader.fetch_data(&fdusd_sym, INTERVAL, FDUSD_CANDLES).await {
            Ok(df_high) => {
                let df_high = FeatureEngine::add_technicals(&df_high, None)?;

                // Fetch lower-timeframe data
                let lower_candles = (FDUSD_CANDLES as usize * max_wait * 2) as u32;
                let df_low = match loader.fetch_data(&fdusd_sym, lower_interval, lower_candles).await {
                    Ok(df) => df,
                    Err(e) => {
                        println!("    FDUSD lower-TF load failed: {}", e);
                        rows.push(ValidationRow {
                            symbol_root: cand.root.to_string(),
                            usdt_result,
                            fdusd_result: None,
                            fdusd_fill_rate: 0.0,
                            fdusd_avg_price_improvement_ticks: 0.0,
                            fdusd_avg_bars_to_fill: 0.0,
                        });
                        println!();
                        continue;
                    }
                };

                let strat = registry.create(STRATEGY).unwrap();
                let signals = strat.predict(&df_high)?;

                let bt = Backtester::new(CAPITAL, 0.0, 0.0)
                    .with_position_sizing(PositionSizing::Full);

                match bt.run_with_passive(&df_high, &df_low, &signals, stop, 0.0, passive_cfg.clone()).await {
                    Ok((r, stats)) => {
                        print_result_row("FDUSD passv ", &r);
                        println!(
                            "    {} fill_rate={:.1}%  price_impr={:.1}ticks  bars_to_fill={:.1}",
                            "└──".dimmed(),
                            stats.fill_rate * 100.0,
                            stats.avg_price_improvement_ticks,
                            stats.avg_bars_to_fill,
                        );
                        let fill_rate = stats.fill_rate;
                        let price_impr = stats.avg_price_improvement_ticks;
                        let bars_fill = stats.avg_bars_to_fill;
                        rows.push(ValidationRow {
                            symbol_root: cand.root.to_string(),
                            usdt_result,
                            fdusd_result: Some(r),
                            fdusd_fill_rate: fill_rate,
                            fdusd_avg_price_improvement_ticks: price_impr,
                            fdusd_avg_bars_to_fill: bars_fill,
                        });
                        println!();
                        continue;
                    }
                    Err(e) => { println!("    FDUSD passive error: {}", e); None }
                }
            }
            Err(e) => { println!("    FDUSD load failed (pair may not exist): {}", e); None }
        };

        rows.push(ValidationRow {
            symbol_root: cand.root.to_string(),
            usdt_result,
            fdusd_result,
            fdusd_fill_rate: 0.0,
            fdusd_avg_price_improvement_ticks: 0.0,
            fdusd_avg_bars_to_fill: 0.0,
        });
        println!();
    }

    // ── Summary table ─────────────────────────────────────────────────────
    println!("\n{}", "━".repeat(110).bright_cyan());
    println!("{}", "  SUMMARY: USDT TAKER vs FDUSD PASSIVE".bright_cyan().bold());
    println!("{}", "━".repeat(110).bright_cyan());
    println!(
        "  {:<6}  {:>8}  {:>8}  {:>8}  {:>8}  {:>8}  {:>8}  {:>9}  {:>8}",
        "Root",
        "USDAnnR%", "USDShp", "FDSAnnR%", "FDSShp",
        "FillRate", "PrImprTk", "BarsToFll",
        "ShpDelta",
    );
    println!("{}", "─".repeat(110));

    let mut total_shp_delta = 0.0;
    let mut shp_delta_count = 0;

    for row in &rows {
        let usdt_ann = row.usdt_result.as_ref().map(|r| r.annualised_return_pct).unwrap_or(f64::NAN);
        let usdt_shp = row.usdt_result.as_ref().map(|r| r.annualised_sharpe).unwrap_or(f64::NAN);
        let fds_ann = row.fdusd_result.as_ref().map(|r| r.annualised_return_pct).unwrap_or(f64::NAN);
        let fds_shp = row.fdusd_result.as_ref().map(|r| r.annualised_sharpe).unwrap_or(f64::NAN);

        let shp_delta = if usdt_shp.is_finite() && fds_shp.is_finite() {
            let delta = fds_shp - usdt_shp;
            total_shp_delta += delta;
            shp_delta_count += 1;
            format!("{:>+8.2}", delta)
        } else {
            "     N/A".to_string()
        };

        let ann_r_str = if fds_ann.is_finite() && fds_ann > 0.0 {
            format!("{:>8.1}", fds_ann).green().to_string()
        } else if fds_ann.is_finite() {
            format!("{:>8.1}", fds_ann).red().to_string()
        } else {
            "     N/A".normal().to_string()
        };

        println!(
            "  {:<6}  {:>8.1}  {:>8.2}  {}  {:>8.2}  {:>8.1}%  {:>8.1}  {:>9.1}  {}",
            row.symbol_root,
            usdt_ann,
            usdt_shp,
            ann_r_str,
            fds_shp,
            row.fdusd_fill_rate * 100.0,
            row.fdusd_avg_price_improvement_ticks,
            row.fdusd_avg_bars_to_fill,
            shp_delta,
        );
    }

    if shp_delta_count > 0 {
        let avg_delta = total_shp_delta / shp_delta_count as f64;
        println!("\n  Avg Sharpe delta (FDUSD passive − USDT taker): {:>+.2}", avg_delta);
        if avg_delta > 0.0 {
            println!("  {} FDUSD passive outperforms USDT taker on Sharpe", "✓".green());
        } else {
            println!("  {} USDT taker outperforms FDUSD passive on Sharpe", "✗".red());
        }
    }

    // ── Viable FDUSD candidates ────────────────────────────────────────────
    println!("\n{}", "━".repeat(70).bright_cyan());
    println!("{}", "  VIABLE FDUSD CANDIDATES (fill rate ≥ 50%, ann. return > 0%)".bright_cyan().bold());
    println!("{}", "━".repeat(70).bright_cyan());

    let viable: Vec<_> = rows.iter()
        .filter(|r| {
            r.fdusd_fill_rate >= 0.5
                && r.fdusd_result.as_ref().map(|res| res.annualised_return_pct > 0.0).unwrap_or(false)
                && r.fdusd_result.as_ref().map(|res| res.total_trades >= 15).unwrap_or(false)
        })
        .collect();

    if viable.is_empty() {
        println!("  No candidates met viability criteria.");
        println!("  Consider: relaxing fill_rate threshold or using USDT results as primary.");
    } else {
        for v in &viable {
            let r = v.fdusd_result.as_ref().unwrap();
            println!(
                "\n  {}FDUSD — bollinger_reversion 1d (0.5× ATR stop):",
                v.symbol_root.yellow()
            );
            println!("    Ann. Return: {:.1}%", r.annualised_return_pct);
            println!("    Ann. Sharpe: {:.2}", r.annualised_sharpe);
            println!("    Trades/yr:   {:.1}", r.trades_per_year);
            println!("    Win Rate:    {:.1}%", r.win_rate);
            println!("    Max DD:      {:.1}%", r.max_drawdown_pct);
            println!("    Fill Rate:   {:.1}%", v.fdusd_fill_rate * 100.0);
        }
    }

    println!();
    Ok(())
}
