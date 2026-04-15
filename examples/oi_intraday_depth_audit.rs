//! Intraday open-interest depth audit.
//!
//! Purpose:
//! - answer the blocked Track C question honestly: is Binance public OI history deep enough
//!   at intraday resolutions to reopen derivatives-state research?
//! - avoid pretending daily OI depth is adequate when it is not
//! - measure what the endpoint *actually* returns across candidate periods
//!
//! This is an infra/trust audit, not a strategy benchmark.

use anyhow::Result;
use chrono::{TimeZone, Utc};
use krypto::data::{OpenInterestLoader, OpenInterestPeriod};

const SYMBOLS: &[&str] = &[
    "BTCUSDT", "ETHUSDT", "SOLUSDT", "XRPUSDT", "ADAUSDT", "DOGEUSDT",
];
const PERIODS: &[OpenInterestPeriod] = &[
    OpenInterestPeriod::M5,
    OpenInterestPeriod::M15,
    OpenInterestPeriod::H1,
    OpenInterestPeriod::H4,
    OpenInterestPeriod::D1,
];

#[tokio::main]
async fn main() -> Result<()> {
    println!("=== BINANCE OI INTRADAY DEPTH AUDIT ===\n");
    println!("Goal: determine whether public OI history is deep enough to support intraday state work.\n");

    let loader = OpenInterestLoader::with_cache_dir("examples/open_interest_cache");

    println!(
        "{:<10} {:<6} {:>6} {:>12} {:>12} {:>12}",
        "Symbol", "Period", "Rows", "SpanDays", "Bars/Day", "HonestRead"
    );

    let mut best_candidate: Option<(&'static str, &'static str, usize, f64)> = None;

    for &symbol in SYMBOLS {
        for &period in PERIODS {
            let df = loader.fetch_with_period(symbol, period, None, None).await?;
            let rows = df.height();
            let times = df.column("time")?.cast(&polars::prelude::DataType::Int64)?;
            let ts: Vec<i64> = times.i64()?.into_iter().flatten().collect();

            let (span_days, bars_per_day) = if ts.len() >= 2 {
                let first = *ts.first().unwrap();
                let last = *ts.last().unwrap();
                let span_days = ((last - first) as f64 / 86_400_000.0).max(0.0);
                let bars_per_day = if span_days > 0.0 {
                    rows as f64 / span_days
                } else {
                    0.0
                };
                (span_days, bars_per_day)
            } else {
                (0.0, 0.0)
            };

            let honest = honest_read(rows, span_days);
            println!(
                "{:<10} {:<6} {:>6} {:>11.1} {:>12.1} {:>12}",
                symbol, period.label, rows, span_days, bars_per_day, honest
            );

            if best_candidate
                .map(|(_, _, br, bd)| rows > br || (rows == br && span_days > bd))
                .unwrap_or(true)
            {
                best_candidate = Some((symbol, period.label, rows, span_days));
            }
        }
    }

    println!("\n=== SAMPLE WINDOW CHECK ===");
    for &(symbol, period) in &[
        ("BTCUSDT", OpenInterestPeriod::M5),
        ("BTCUSDT", OpenInterestPeriod::H1),
        ("BTCUSDT", OpenInterestPeriod::D1),
    ] {
        let df = loader.fetch_with_period(symbol, period, None, None).await?;
        let times = df.column("time")?.cast(&polars::prelude::DataType::Int64)?;
        let ts: Vec<i64> = times.i64()?.into_iter().flatten().collect();
        if let (Some(first), Some(last)) = (ts.first(), ts.last()) {
            let first_dt = Utc.timestamp_millis_opt(*first).single().unwrap();
            let last_dt = Utc.timestamp_millis_opt(*last).single().unwrap();
            println!(
                "{} {}: {} rows | {} -> {}",
                symbol,
                period.label,
                df.height(),
                first_dt,
                last_dt
            );
        }
    }

    println!("\n=== INTERPRETATION ===");
    if let Some((symbol, period, rows, span_days)) = best_candidate {
        println!(
            "Best available public window in this audit: {} {} | {} rows across {:.1} days",
            symbol, period, rows, span_days
        );
    }
    println!("- If 5m/15m/1h data gives only weeks, intraday OI state work is still possible for short-horizon research but not for multi-year chronology claims.");
    println!("- If 4h reaches multiple months, that is enough to reopen a *modest* crowding-state study, but still not enough for the same harsh-universe trust bar as daily price factors.");
    println!("- If all periods are shallow, treat OI as a collector problem first, not a backtest problem.");

    Ok(())
}

fn honest_read(rows: usize, span_days: f64) -> &'static str {
    if rows >= 400 && span_days >= 30.0 {
        "usable"
    } else if rows >= 120 && span_days >= 7.0 {
        "limited"
    } else {
        "shallow"
    }
}
