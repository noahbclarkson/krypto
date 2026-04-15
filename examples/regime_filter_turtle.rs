//! Regime Filter for Turtle Breakout Strategy
//!
//! PLAN.md Session 12 showed Turtle breakout works on SOL/DOGE but is regime-dependent.
//! This test adds a regime filter (price > 200d MA) to avoid trading in bear markets.
//!
//! Hypothesis: Only trading breakouts when price > 200d MA should improve risk-adjusted returns.

use anyhow::Result;
use krypto::{data::loader::DataLoader, features::indicators::FeatureEngine};
use polars::prelude::*;

const SYMBOLS: &[&str] = &["SOLUSDT", "DOGEUSDT", "BTCUSDT", "ETHUSDT", "XRPUSDT"];
const CANDLES: u32 = 3000;
const TAKER_FEE: f64 = 0.001;
const HOLD_BARS: usize = 21;
const PERIOD: usize = 20; // 20-day breakout

#[tokio::main]
async fn main() -> Result<()> {
    println!("=== REGIME FILTER FOR TURTLE BREAKOUT ===\n");
    println!("Hypothesis: Only trade breakouts when price > 200d MA (bull regime).\n");
    println!(
        "Config: {}d breakout, {}d hold, {} fee\n",
        PERIOD, HOLD_BARS, TAKER_FEE
    );

    let loader = DataLoader::new(None, None);

    // Load data
    let mut data_cache: std::collections::HashMap<String, DataFrame> =
        std::collections::HashMap::new();

    for symbol in SYMBOLS {
        print!("Loading {}... ", symbol);
        match loader.fetch_data(symbol, "1d", CANDLES).await {
            Ok(raw) => {
                let df = FeatureEngine::add_technicals(&raw, None)?;
                println!("{} bars", df.height());
                data_cache.insert(symbol.to_string(), df);
            }
            Err(e) => {
                println!("SKIP ({})", e);
            }
        }
    }

    // 1. Compare unfiltered vs filtered on full history
    println!("\n{}", "=".repeat(80));
    println!("1. FULL HISTORY COMPARISON");
    println!("{}", "=".repeat(80));
    println!(
        "{:12} | {:^20} | {:^20} | {:^15}",
        "", "UNFILTERED", "REGIME FILTERED", "IMPROVEMENT"
    );
    println!(
        "{:12} | {:>7} {:>5} {:>5} | {:>7} {:>5} {:>5} | {:>7} {:>5}",
        "", "Ret%", "Trd", "Win%", "Ret%", "Trd", "Win%", "Ret%", "Win%"
    );
    println!("{}", "-".repeat(80));

    let mut total_unfilt_ret = 0.0;
    let mut total_filt_ret = 0.0;
    let mut total_unfilt_trades = 0;
    let mut total_filt_trades = 0;

    for symbol in SYMBOLS {
        if let Some(df) = data_cache.get(&symbol.to_string()) {
            let unfilt = run_turtle_backtest(df, HOLD_BARS, PERIOD, false)?;
            let filtered = run_turtle_backtest(df, HOLD_BARS, PERIOD, true)?;

            total_unfilt_ret += unfilt.total_return_pct;
            total_filt_ret += filtered.total_return_pct;
            total_unfilt_trades += unfilt.trades;
            total_filt_trades += filtered.trades;

            let ret_improve = filtered.total_return_pct - unfilt.total_return_pct;
            let win_improve = (filtered.win_rate - unfilt.win_rate) * 100.0;

            println!(
                "{:12} | {:>7.1} {:>5} {:>4.0}% | {:>7.1} {:>5} {:>4.0}% | {:>+7.1} {:>+5.1}%",
                symbol,
                unfilt.total_return_pct,
                unfilt.trades,
                unfilt.win_rate * 100.0,
                filtered.total_return_pct,
                filtered.trades,
                filtered.win_rate * 100.0,
                ret_improve,
                win_improve
            );
        }
    }

    println!("{}", "-".repeat(80));
    println!(
        "{:12} | {:>7.1} {:>5} {:>4} | {:>7.1} {:>5} {:>4} | {:>+7.1}",
        "TOTAL",
        total_unfilt_ret,
        total_unfilt_trades,
        "",
        total_filt_ret,
        total_filt_trades,
        "",
        total_filt_ret - total_unfilt_ret
    );

    // 2. Walk-forward validation with regime filter
    println!("\n\n{}", "=".repeat(80));
    println!("2. WALK-FORWARD VALIDATION (REGIME FILTERED)");
    println!("{}", "=".repeat(80));

    for symbol in SYMBOLS {
        if let Some(df) = data_cache.get(&symbol.to_string()) {
            let n = df.height();
            let quarter = n / 4;

            println!("\n{} ({} bars):", symbol, n);

            for i in 0..4 {
                let start = i * quarter;
                let end = if i == 3 { n } else { (i + 1) * quarter };
                let period_df = df.slice(start as i64, end - start);

                let result = run_turtle_backtest(&period_df, HOLD_BARS, PERIOD, true)?;

                println!(
                    "  Period {} ({} bars): Return={:>7.1}% | Trades={:>3} | WinRate={:.0}%",
                    i + 1,
                    end - start,
                    result.total_return_pct,
                    result.trades,
                    result.win_rate * 100.0
                );
            }
        }
    }

    // 3. Regime filter statistics
    println!("\n\n{}", "=".repeat(80));
    println!("3. REGIME FILTER STATISTICS");
    println!("{}", "=".repeat(80));

    for symbol in SYMBOLS {
        if let Some(df) = data_cache.get(&symbol.to_string()) {
            let stats = analyze_regime_filter(df, PERIOD)?;
            println!(
                "{:12} | Bull days: {:>5.0}% | Trades in bull: {:>3}/{:>3} ({:.0}%) | Bull trades win: {:.0}%",
                symbol,
                stats.pct_bull_days * 100.0,
                stats.trades_in_bull,
                stats.total_signals,
                stats.pct_trades_in_bull * 100.0,
                stats.bull_trade_win_rate * 100.0
            );
        }
    }

    // 4. Summary
    println!("\n\n{}", "=".repeat(80));
    println!("SUMMARY");
    println!("{}", "=".repeat(80));

    let improvement = total_filt_ret - total_unfilt_ret;
    let trade_reduction_pct = 1.0 - (total_filt_trades as f64 / total_unfilt_trades as f64);

    if improvement > 0.0 {
        println!("✓ Regime filter IMPROVES returns by {:.1}%", improvement);
        println!(
            "  Trade reduction: {:.0}% (fewer trades, better quality)",
            trade_reduction_pct * 100.0
        );
    } else {
        println!("✗ Regime filter HURTS returns by {:.1}%", improvement.abs());
        println!(
            "  Trade reduction: {:.0}% (filtering out profitable trades)",
            trade_reduction_pct * 100.0
        );
    }

    println!("\nNext: If filter helps, test combined signals (Turtle + MACD confirmation).");

    Ok(())
}

struct TrendResult {
    total_return_pct: f64,
    trades: usize,
    win_rate: f64,
}

struct RegimeStats {
    pct_bull_days: f64,
    total_signals: usize,
    trades_in_bull: usize,
    pct_trades_in_bull: f64,
    bull_trade_win_rate: f64,
}

fn run_turtle_backtest(
    df: &DataFrame,
    hold_bars: usize,
    period: usize,
    use_regime_filter: bool,
) -> Result<TrendResult> {
    let close = df.column("close")?.f64()?;
    let high = df.column("high")?.f64()?;
    let low = df.column("low")?.f64()?;
    let n = close.len();

    // Calculate SMA 200 for regime filter
    let sma_200 = calculate_sma(&close, 200)?;

    // Generate Turtle signals (20d high/low breakout)
    let signals = generate_turtle_signals(df, period)?;

    let mut trade_returns: Vec<f64> = Vec::new();
    let mut i = 200; // Start after SMA 200 warmup

    while i < n.saturating_sub(hold_bars + 1) {
        let signal = signals[i];

        if signal != 0 && i + 1 + hold_bars < n {
            // Regime filter: only trade longs when price > SMA 200
            if use_regime_filter && signal > 0 {
                let current_price = close.get(i).unwrap_or(0.0);
                let current_sma = match sma_200.get(i) {
                    Some(Some(v)) => *v,
                    _ => 0.0,
                };

                if current_price <= current_sma {
                    // Skip long signals in bear regime
                    i += 1;
                    continue;
                }
            }

            let entry_price = match close.get(i + 1) {
                Some(p) if p > 0.0 => p,
                _ => {
                    i += 1;
                    continue;
                }
            };
            let exit_idx = i + 1 + hold_bars;
            let exit_price = match close.get(exit_idx) {
                Some(p) if p > 0.0 => p,
                _ => {
                    i += 1;
                    continue;
                }
            };

            // Calculate return
            let return_pct = if signal > 0 {
                (exit_price / entry_price - 1.0) * 100.0
            } else {
                (entry_price / exit_price - 1.0) * 100.0
            };

            // Deduct fees (2x taker: entry + exit)
            let net_return = return_pct - 2.0 * TAKER_FEE * 100.0;

            trade_returns.push(net_return);
            i += hold_bars + 1; // Skip to after exit
        } else {
            i += 1;
        }
    }

    let trades = trade_returns.len();
    let total_return_pct = trade_returns.iter().sum();
    let win_rate = if trades > 0 {
        trade_returns.iter().filter(|&&r| r > 0.0).count() as f64 / trades as f64
    } else {
        0.0
    };

    Ok(TrendResult {
        total_return_pct,
        trades,
        win_rate,
    })
}

fn generate_turtle_signals(df: &DataFrame, period: usize) -> Result<Vec<i32>> {
    let close = df.column("close")?.f64()?;
    let high = df.column("high")?.f64()?;
    let low = df.column("low")?.f64()?;
    let n = close.len();

    let mut signals = vec![0i32; n];

    for i in period..n {
        // Calculate period high/low
        let mut period_high = f64::NEG_INFINITY;
        let mut period_low = f64::INFINITY;

        for j in (i - period)..i {
            if let Some(h) = high.get(j) {
                period_high = period_high.max(h);
            }
            if let Some(l) = low.get(j) {
                period_low = period_low.min(l);
            }
        }

        let current_close = close.get(i).unwrap_or(0.0);

        // Turtle breakout signals
        if current_close > period_high {
            signals[i] = 1; // Long on breakout above period high
        } else if current_close < period_low {
            signals[i] = -1; // Short on breakout below period low
        }
    }

    Ok(signals)
}

fn calculate_sma(series: &ChunkedArray<Float64Type>, period: usize) -> Result<Vec<Option<f64>>> {
    let n = series.len();
    let mut sma = vec![None; n];

    for i in period..n {
        let sum: f64 = (0..period).filter_map(|j| series.get(i - j)).sum();
        sma[i] = Some(sum / period as f64);
    }

    Ok(sma)
}

fn analyze_regime_filter(df: &DataFrame, period: usize) -> Result<RegimeStats> {
    let close = df.column("close")?.f64()?;
    let n = close.len();

    // Calculate SMA 200
    let sma_200 = calculate_sma(&close, 200)?;

    // Generate signals
    let signals = generate_turtle_signals(df, period)?;

    // Count bull days
    let mut bull_days = 0;
    let mut total_days = 0;

    for i in 200..n {
        let price = close.get(i).unwrap_or(0.0);
        let sma = match sma_200.get(i) {
            Some(Some(v)) => *v,
            _ => 0.0,
        };

        if price > sma {
            bull_days += 1;
        }
        total_days += 1;
    }

    // Count signals and trades in bull regime
    let mut total_signals = 0;
    let mut bull_signals = 0;
    let mut bull_wins = 0;
    let mut bull_trades = 0;

    let mut i = 200;
    while i < n.saturating_sub(HOLD_BARS + 1) {
        let signal = signals[i];

        if signal > 0 && i + 1 + HOLD_BARS < n {
            total_signals += 1;

            let price = close.get(i).unwrap_or(0.0);
            let sma = match sma_200.get(i) {
                Some(Some(v)) => *v,
                _ => 0.0,
            };

            if price > sma {
                bull_signals += 1;

                // Check if trade wins
                let entry = close.get(i + 1).unwrap_or(0.0);
                let exit = close.get(i + 1 + HOLD_BARS).unwrap_or(0.0);
                let ret = (exit / entry - 1.0) * 100.0 - 2.0 * TAKER_FEE * 100.0;

                bull_trades += 1;
                if ret > 0.0 {
                    bull_wins += 1;
                }

                i += HOLD_BARS + 1;
                continue;
            }
        }
        i += 1;
    }

    Ok(RegimeStats {
        pct_bull_days: if total_days > 0 {
            bull_days as f64 / total_days as f64
        } else {
            0.0
        },
        total_signals,
        trades_in_bull: bull_signals,
        pct_trades_in_bull: if total_signals > 0 {
            bull_signals as f64 / total_signals as f64
        } else {
            0.0
        },
        bull_trade_win_rate: if bull_trades > 0 {
            bull_wins as f64 / bull_trades as f64
        } else {
            0.0
        },
    })
}
