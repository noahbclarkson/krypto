//! Intra-Bar Sequence Analysis — How often does high come before low?
//!
//! This is a simpler analysis that doesn't try to re-run the backtest.
//! Instead, it just looks at the 30m data to answer:
//! 1. How often does the daily high come BEFORE the daily low?
//! 2. When we're in a trade with a trailing stop, how often would the stop
//!    be hit BEFORE we get the trailing benefit from the high?
//!
//! Usage:
//!   cargo run --profile sweep --example intra_bar_sequence_analysis

use anyhow::Result;
use krypto::{
    algo::strategies::BollingerReversion, algo::SignalGenerator, data::loader::DataLoader,
    features::indicators::FeatureEngine,
};
use polars::prelude::*;

const SYMBOLS: &[&str] = &["BTCUSDT", "ETHUSDT", "SOLUSDT", "XRPUSDT", "DOGEUSDT"];
const CANDLES_1D: u32 = 2000;
const ATR_MULT: f64 = 0.3;

/// Compute ATR-based trailing stop from the last bar
fn compute_atr_stop(df: &DataFrame, atr_mult: f64) -> f64 {
    let atr = df
        .column("atr")
        .ok()
        .and_then(|s| {
            s.f64()
                .ok()
                .and_then(|ca| ca.get(ca.len().saturating_sub(1)))
        })
        .unwrap_or(0.0);
    let close = df
        .column("close")
        .ok()
        .and_then(|s| {
            s.f64()
                .ok()
                .and_then(|ca| ca.get(ca.len().saturating_sub(1)))
        })
        .unwrap_or(1.0);
    if close > 0.0 {
        (atr * atr_mult / close).clamp(0.005, 0.30)
    } else {
        0.05
    }
}

#[derive(Default)]
struct DaySequence {
    /// Did the daily high come before the daily low?
    high_first: bool,
    /// Index of the 30m bar where high occurred
    high_idx: usize,
    /// Index of the 30m bar where low occurred
    low_idx: usize,
    /// Number of 30m bars in this day
    n_bars: usize,
}

/// Analyze a single day's 30m bars to determine high/low sequence
fn analyze_day_sequence(highs_30m: &[f64], lows_30m: &[f64]) -> Option<DaySequence> {
    if highs_30m.is_empty() {
        return None;
    }

    let mut max_high = f64::MIN;
    let mut min_low = f64::MAX;
    let mut high_idx = 0;
    let mut low_idx = 0;

    for (i, (&h, &l)) in highs_30m.iter().zip(lows_30m.iter()).enumerate() {
        if h > max_high {
            max_high = h;
            high_idx = i;
        }
        if l < min_low {
            min_low = l;
            low_idx = i;
        }
    }

    Some(DaySequence {
        high_first: high_idx < low_idx,
        high_idx,
        low_idx,
        n_bars: highs_30m.len(),
    })
}

/// For a given trade, check if the stop would be hit differently with intra-bar resolution
#[derive(Default)]
struct StopAnalysis {
    /// Total days analyzed
    total_days: usize,
    /// Days where high came before low
    high_first_count: usize,
    /// Days where low came before high
    low_first_count: usize,
    /// Days where stop would be hit BEFORE trailing benefit (longs)
    long_stop_before_trail: usize,
    /// Days where stop would be hit BEFORE trailing benefit (shorts)
    short_stop_before_trail: usize,
    /// Average position of high in the day (0.0 = start, 1.0 = end)
    avg_high_position: f64,
    /// Average position of low in the day
    avg_low_position: f64,
}

#[tokio::main]
async fn main() -> Result<()> {
    println!("\n{}", "━".repeat(80));
    println!("  INTRA-BAR SEQUENCE ANALYSIS");
    println!("  How often does the daily high come before the daily low?");
    println!("{}", "━".repeat(80));
    println!("\n  If high usually comes first, optimistic trailing stops are valid.");
    println!("  If low often comes first, we're overstating returns.\n");

    let loader = DataLoader::new(None, None);
    let strat = BollingerReversion::new();

    println!(
        "{:<12} {:>8} {:>8} {:>8} {:>10} {:>10} {:>10}",
        "Symbol", "Days", "High 1st", "Low 1st", "High@pct", "Low@pct", "Trades"
    );
    println!("{}", "-".repeat(80));

    let mut total_high_first = 0;
    let mut total_low_first = 0;
    let mut total_days = 0;

    for symbol in SYMBOLS {
        // Fetch 1d data
        print!("{}: fetching 1d... ", symbol);
        let raw_1d = match loader.fetch_data(symbol, "1d", CANDLES_1D).await {
            Ok(d) => d,
            Err(e) => {
                println!("SKIP ({})", e);
                continue;
            }
        };
        let df_1d = FeatureEngine::add_technicals(&raw_1d, None)?;
        let n_1d = df_1d.height();
        print!("{} bars, 30m... ", n_1d);

        // Fetch 30m data
        let candles_30m = CANDLES_1D * 48;
        let df_30m = match loader.fetch_data(symbol, "30m", candles_30m).await {
            Ok(d) => d,
            Err(e) => {
                println!("30m fetch failed: {}", e);
                continue;
            }
        };
        println!("{} bars", df_30m.height());

        // Generate signals
        let signals_series = strat.predict(&df_1d)?;
        let signals: Vec<f64> = signals_series
            .f64()?
            .into_iter()
            .map(|v| v.unwrap_or(0.0))
            .collect();
        let n_trades = signals.iter().filter(|&&s| s != 0.0).count();

        // Analyze each day
        let times_1d = df_1d.column("time")?.datetime()?;
        let times_30m = df_30m.column("time")?.datetime()?;
        let highs_30m = df_30m.column("high")?.f64()?;
        let lows_30m = df_30m.column("low")?.f64()?;
        let n_30m = df_30m.height();

        // Build day -> 30m bars mapping
        let mut analysis = StopAnalysis::default();
        let mut day_highs: Vec<f64> = Vec::new();
        let mut day_lows: Vec<f64> = Vec::new();
        let mut current_day_end = if n_1d > 0 {
            times_1d.get(0).unwrap_or(0) + 24 * 60 * 60 * 1000
        } else {
            0
        };
        let mut day_idx = 0;
        let mut high_positions: Vec<f64> = Vec::new();
        let mut low_positions: Vec<f64> = Vec::new();

        for i in 0..n_30m {
            let t_30m = times_30m.get(i).unwrap_or(0);

            if t_30m >= current_day_end && day_idx < n_1d - 1 {
                // Process the previous day
                if let Some(seq) = analyze_day_sequence(&day_highs, &day_lows) {
                    analysis.total_days += 1;
                    if seq.high_first {
                        analysis.high_first_count += 1;
                    } else {
                        analysis.low_first_count += 1;
                    }
                    high_positions.push(seq.high_idx as f64 / seq.n_bars.max(1) as f64);
                    low_positions.push(seq.low_idx as f64 / seq.n_bars.max(1) as f64);
                }

                // Move to next day
                day_idx += 1;
                current_day_end = times_1d.get(day_idx).unwrap_or(0) + 24 * 60 * 60 * 1000;
                day_highs.clear();
                day_lows.clear();
            }

            day_highs.push(highs_30m.get(i).unwrap_or(0.0));
            day_lows.push(lows_30m.get(i).unwrap_or(0.0));
        }

        // Process the last day
        if let Some(seq) = analyze_day_sequence(&day_highs, &day_lows) {
            analysis.total_days += 1;
            if seq.high_first {
                analysis.high_first_count += 1;
            } else {
                analysis.low_first_count += 1;
            }
            high_positions.push(seq.high_idx as f64 / seq.n_bars.max(1) as f64);
            low_positions.push(seq.low_idx as f64 / seq.n_bars.max(1) as f64);
        }

        // Compute averages
        let avg_high_pos = if !high_positions.is_empty() {
            high_positions.iter().sum::<f64>() / high_positions.len() as f64 * 100.0
        } else {
            0.0
        };
        let avg_low_pos = if !low_positions.is_empty() {
            low_positions.iter().sum::<f64>() / low_positions.len() as f64 * 100.0
        } else {
            0.0
        };

        println!(
            "{:<12} {:>8} {:>8} {:>8} {:>9.1}% {:>9.1}% {:>10}",
            symbol,
            analysis.total_days,
            analysis.high_first_count,
            analysis.low_first_count,
            avg_high_pos,
            avg_low_pos,
            n_trades
        );

        total_high_first += analysis.high_first_count;
        total_low_first += analysis.low_first_count;
        total_days += analysis.total_days;
    }

    // Summary
    println!("\n{}", "━".repeat(80));
    if total_days > 0 {
        let high_first_pct = total_high_first as f64 / total_days as f64 * 100.0;
        let low_first_pct = total_low_first as f64 / total_days as f64 * 100.0;

        println!("  Total days analyzed:     {}", total_days);
        println!(
            "  High before low:         {} ({:.1}pct)",
            total_high_first, high_first_pct
        );
        println!(
            "  Low before high:         {} ({:.1}pct)",
            total_low_first, low_first_pct
        );

        if low_first_pct > 40.0 {
            println!(
                "\n  WARNING: Low often comes before high ({:.1}pct of days)",
                low_first_pct
            );
            println!("  Trailing stops may be hit before getting benefit from the high.");
        } else if low_first_pct > 30.0 {
            println!(
                "\n  CAUTION: Low sometimes comes before high ({:.1}pct of days)",
                low_first_pct
            );
            println!("  Some trailing stop benefits may be overstated.");
        } else {
            println!(
                "\n  OK: High usually comes before low ({:.1}pct of days)",
                high_first_pct
            );
            println!("  Optimistic trailing stops are mostly valid.");
        }
    }
    println!("{}", "━".repeat(80));

    Ok(())
}
