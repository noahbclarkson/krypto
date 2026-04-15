//! Market Microstructure Analyzer
//!
//! Validates Turtle+Chandelier execution assumptions using local parquet cache.
//! Tests maker-fill rate: limit at bar close → next bar open >= entry?
//! W05 FTX crash microstructure analysis included.
//!
//! Usage:
//!   cargo run --profile sweep --example microstructure_analyzer 2>&1

use anyhow::Result;
use krypto::data::loader::DataLoader;
use polars::prelude::*;
use std::time::Instant;

// ── Types ─────────────────────────────────────────────────────────────────────

#[derive(Clone)]
struct Bar {
    open: f64,
    high: f64,
    low: f64,
    close: f64,
    time: i64, // Unix seconds
}

struct Microanalysis {
    n_signals: usize,
    maker_fill_pct: f64,
    avg_gap_bps: f64,
    avg_next_ret_bps: f64,
    avg_stop_bps: f64,
    win_rate: f64,
}

impl std::fmt::Display for Microanalysis {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        writeln!(f, "  {:35} {:>8}", "Signals", self.n_signals)?;
        writeln!(f, "  {:35} {:>7.1}%", "Entry fills as MAKER", self.maker_fill_pct)?;
        writeln!(f, "  {:35} {:>+8.2} bps", "Avg next-bar open gap", self.avg_gap_bps)?;
        writeln!(f, "  {:35} {:>+8.2} bps", "Avg next-bar return", self.avg_next_ret_bps)?;
        writeln!(f, "  {:35} {:>8.2} bps", "Avg Chandelier stop (2xATR)", self.avg_stop_bps)?;
        writeln!(f, "  {:35} {:>7.1}%", "Win rate (estimated)", self.win_rate)?;
        Ok(())
    }
}

impl Microanalysis {
    fn run(bars: &[Bar], ep: usize, atr_p: usize, atr_mult: f64) -> Self {
        let n = bars.len();
        let mut maker_fills = 0usize;
        let mut gap_sum = 0.0_f64;
        let mut ret_sum = 0.0_f64;
        let mut stop_sum = 0.0_f64;
        let mut wins = 0usize;
        let mut signals = 0usize;

        for i in (ep + 1)..n.saturating_sub(1) {
            let ws = i.saturating_sub(ep);
            let max_close = bars[ws..i].iter().map(|b| b.close).fold(f64::NEG_INFINITY, f64::max);
            if bars[i].close < max_close { continue; }
            signals += 1;

            let next = &bars[i + 1];
            let gap = next.open - bars[i].close;
            let gap_bps = gap / bars[i].close * 10_000.0;
            let ret_bps = (next.close - next.open) / next.open * 10_000.0;

            if next.open >= bars[i].close { maker_fills += 1; }
            gap_sum += gap_bps;
            ret_sum += ret_bps;

            let atr = Self::compute_atr(bars, i, atr_p);
            stop_sum += atr_mult * atr / bars[i].close * 10_000.0;

            let exit_price = Self::chandelier_exit(bars, i + 1, atr_p, atr_mult);
            if exit_price > bars[i].close { wins += 1; }
        }

        let n_sig = signals.max(1) as f64;
        Self {
            n_signals: signals,
            maker_fill_pct: maker_fills as f64 / n_sig * 100.0,
            avg_gap_bps: gap_sum / n_sig,
            avg_next_ret_bps: ret_sum / n_sig,
            avg_stop_bps: stop_sum / n_sig,
            win_rate: wins as f64 / n_sig * 100.0,
        }
    }

    fn compute_atr(bars: &[Bar], end: usize, period: usize) -> f64 {
        if end < period { return 0.0; }
        let mut trs = Vec::with_capacity(period);
        for i in (end + 1 - period)..=end {
            let h = bars[i].high;
            let l = bars[i].low;
            let c0 = if i > 0 { bars[i - 1].close } else { bars[0].close };
            trs.push((h - l).max((h - c0).abs()).max((l - c0).abs()));
        }
        trs.iter().sum::<f64>() / period as f64
    }

    fn chandelier_exit(bars: &[Bar], start: usize, atr_p: usize, atr_mult: f64) -> f64 {
        let n = bars.len();
        let mut hh = bars[start].high;
        for i in start..n {
            hh = hh.max(bars[i].high);
            let atr = Self::compute_atr(bars, i, atr_p);
            let stop = hh - atr_mult * atr;
            if bars[i].close < stop { return bars[i].close; }
        }
        bars[n.saturating_sub(1)].close
    }
}

// W05: Nov 2021 – May 2022 (Unix seconds)
// 2021-11-01 00:00:00 = 1635724800
// 2022-05-25 00:00:00 = 1653436800
// 2021-01-01 00:00:00 = 1609459200
const W05_START: i64 = 1635724800;
const W05_END: i64 = 1653436800;
const PRE_W05_START: i64 = 1609459200;

// ── Load bars from parquet ────────────────────────────────────────────────────

fn load_bars(df: &DataFrame, n: usize) -> Vec<Bar> {
    let n = df.height().min(n);
    macro_rules! col_vec {
        ($name:expr) => {{
            let chunked = df.column($name).unwrap().f64().unwrap();
            chunked.into_iter().filter_map(|x| x).take(n).collect::<Vec<_>>()
        }};
    }

    // Time column — datetime[ms] → extract as i64 (milliseconds)
    let time_col = df.column("time").unwrap();
    let time_ms: Vec<i64> = if let Ok(tc) = time_col.datetime() {
        tc.into_iter().filter_map(|v| v).take(n).collect()
    } else if let Ok(tc) = time_col.i64() {
        tc.into_iter().filter_map(|v| v).take(n).collect()
    } else {
        vec![]
    };

    let open_v   = col_vec!("open");
    let high_v   = col_vec!("high");
    let low_v    = col_vec!("low");
    let close_v  = col_vec!("close");

    let mut bars = Vec::with_capacity(n);
    for i in 0..n {
        let t_ms = time_ms.get(i).copied().unwrap_or(0);
        bars.push(Bar {
            open: open_v.get(i).copied().unwrap_or(0.0),
            high: high_v.get(i).copied().unwrap_or(0.0),
            low: low_v.get(i).copied().unwrap_or(0.0),
            close: close_v.get(i).copied().unwrap_or(0.0),
            // Convert ms → seconds for W05 filtering
            time: t_ms / 1000,
        });
    }
    bars
}

// ── Format timestamp ──────────────────────────────────────────────────────────

fn fmt_ts(unix_secs: i64) -> String {
    use chrono::TimeZone;
    chrono::Utc.timestamp_opt(unix_secs, 0)
        .single()
        .map(|dt| dt.format("%Y-%m-%d").to_string())
        .unwrap_or_else(|| "?".to_string())
}

// ── Main ─────────────────────────────────────────────────────────────────────

#[tokio::main]
async fn main() -> Result<()> {
    let loader = DataLoader::new(None, None);

    println!();
    println!("{}", "═".repeat(76));
    println!("  {:^72}", "MARKET MICROSTRUCTURE ANALYZER");
    println!("  {:^72}", "Turtle+Chandelier execution quality validation");
    println!("{}", "═".repeat(76));
    println!();
    println!("  Testing maker-fill: limit at bar close, next bar open >= entry?");
    println!("  {} = maker fill, {} = taker miss", "✅", "❌");
    println!();

    // ── Live microstructure ─────────────────────────────────────────────────
    println!("  ─── LIVE MICROSTRUCTURE ───");

    let symbols = ["BTCUSDT", "ETHUSDT", "SOLUSDT"];
    let mut all_maker = Vec::new();

    for sym in symbols {
        print!("  Loading {}...", sym);
        let t0 = Instant::now();
        match loader.fetch_with_cache(sym, "1d", 3000).await {
            Ok(df) => {
                let bars = load_bars(&df, 2800);
                println!(" ✅ {} bars ({}ms)", bars.len(), t0.elapsed().as_millis());

                let ma = Microanalysis::run(&bars, 21, 28, 2.0);
                print!("{}", ma);

                let v = if ma.maker_fill_pct >= 60.0 {
                    "✅ MAKER ADVANTAGE"
                } else if ma.maker_fill_pct >= 45.0 {
                    "⚠️ NEUTRAL"
                } else {
                    "❌ TAKER DISADVANTAGE"
                };
                println!("  {:35} {}", "Verdict", v);
                all_maker.push(ma.maker_fill_pct);
                println!();
            }
            Err(e) => println!(" ❌ {}", e),
        }
    }

    // ── Aggregate ──────────────────────────────────────────────────────────
    if !all_maker.is_empty() {
        let avg_maker = all_maker.iter().sum::<f64>() / all_maker.len() as f64;
        println!();
        println!("  ─── AGGREGATE MAKER-FILL RATE ───");
        println!();
        println!("  {:35} {:>7.1}%", "Average maker-fill rate", avg_maker);
        println!();

        let maker_fee = 0.0000_f64;
        let taker_fee = 0.0004_f64;
        let maker_rate = avg_maker / 100.0;
        let expected_fee = maker_rate * maker_fee + (1.0 - maker_rate) * taker_fee;
        let backtest_fee = 0.0010_f64;

        println!("  {:35} {:>6.1}% (maker) + {:>5.1}% (taker)", "Execution mix",
            maker_rate * 100.0, (1.0 - maker_rate) * 100.0);
        println!("  {:35} {:>6.4}% (live) vs {:>6.4}% (backtest)", "Expected fee/trade",
            expected_fee * 100.0, backtest_fee * 100.0);
        println!("  {:35} {:>+.3}%", "Fee saving vs backtest",
            (backtest_fee - expected_fee) * 100.0);

        println!();
        if avg_maker >= 55.0 {
            println!("  ✅ MAKER-FILL CONFIRMED (~{:.0}% of entries)", avg_maker);
            let bp = (backtest_fee - expected_fee) * 10000.0;
            println!("  Live execution saves ~{:.1}bp/trade vs backtest assumption.", bp);
        } else {
            println!("  ⚠️ Maker-fill below 55% — limit order discipline critical");
        }
    }

    // ── W05 FTX crash microstructure ────────────────────────────────────────
    println!();
    println!("  ─── W05 FTX CRASH MICROSTRUCTURE (BTCUSDT) ───");
    println!();
    println!("  W05 ≈ Nov 2021 – May 2022 (BTC: $69k → $31k).");
    println!();

    print!("  Loading BTCUSDT...");
    let t0 = Instant::now();
    match loader.fetch_with_cache("BTCUSDT", "1d", 3000).await {
        Ok(df) => {
            let all_bars = load_bars(&df, 2800);
            println!(" ✅ ({}ms)", t0.elapsed().as_millis());

            if !all_bars.is_empty() {
                let earliest = all_bars.iter().map(|b| b.time).min().unwrap_or(0);
                let latest = all_bars.iter().map(|b| b.time).max().unwrap_or(0);
                println!("  Data range: {} → {}", fmt_ts(earliest), fmt_ts(latest));
            }

            let w05_bars: Vec<Bar> = all_bars.iter()
                .filter(|b| b.time >= W05_START && b.time < W05_END)
                .cloned()
                .collect();

            println!("  W05 window: {} bars", w05_bars.len());

            if w05_bars.len() > 50 {
                let w05_ma = Microanalysis::run(&w05_bars, 21, 28, 2.0);
                println!();
                print!("{}", w05_ma);

                let verdict = if w05_ma.maker_fill_pct < 40.0 {
                    "❌ MAKER FILL COLLAPSED"
                } else if w05_ma.maker_fill_pct < 50.0 {
                    "⚠️ Maker fill degraded"
                } else {
                    "✅ Maker-fill held"
                };
                println!("  {:35} {}", "W05 verdict", verdict);
                println!();

                let pre_w05: Vec<Bar> = all_bars.iter()
                    .filter(|b| b.time >= PRE_W05_START && b.time < W05_START)
                    .cloned()
                    .collect();

                if pre_w05.len() > 50 {
                    let pre_ma = Microanalysis::run(&pre_w05, 21, 28, 2.0);
                    println!("  {:35} {:>7.1}%", "Pre-W05 maker-fill (2021)", pre_ma.maker_fill_pct);
                    println!("  {:35} {:>7.1}%", "W05 maker-fill (FTX crash)", w05_ma.maker_fill_pct);
                    let delta = w05_ma.maker_fill_pct - pre_ma.maker_fill_pct;
                    println!("  {:35} {:>+7.1}pp", "Change", delta);
                    if delta < -10.0 {
                        println!();
                        println!("  ⚠️ Maker-fill dropped >10pp in crash — live trading W05 would be taker-heavy");
                    } else {
                        println!();
                        println!("  ✅ Maker-fill held during crash");
                    }
                }
            } else {
                println!("  ⚠️ Insufficient W05 bars — data may not reach 2021-2022");
            }
        }
        Err(e) => println!(" ❌ {}", e),
    }

    println!();
    println!("{}", "═".repeat(76));

    Ok(())
}