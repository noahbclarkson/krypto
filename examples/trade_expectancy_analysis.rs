//! Trade Expectancy Analysis — Turtle+Chandelier (NoDOGE)
//!
//! Outputs: snapshots/trade_expectancy.csv (all trade data)
//! Then run: python3 charts/plot_trade_expectancy.py
//!
//! Usage: cargo run --profile sweep --example trade_expectancy_analysis

use anyhow::Result;
use krypto::data::loader::DataLoader;
use polars::prelude::*;
use std::collections::HashMap;

const CANDLES: u32 = 5000;
const TAKER_FEE: f64 = 0.001;
const TURTLE_ENTRY: usize = 21;
const CHAND_PERIOD: usize = 28;
const CHAND_MULT: f64 = 2.00;
const TURTLE_ATR_PERIOD: usize = 24;
const TURTLE_ATR_MULT: f64 = 2.00;
const HOLD_MAX: usize = 45;

const SYMBOLS: [&str; 5] = ["BTCUSDT", "ETHUSDT", "SOLUSDT", "XRPUSDT", "DOGEUSDT"];

struct SymData {
    close: Vec<f64>,
    high: Vec<f64>,
    low: Vec<f64>,
    vol: Vec<f64>,
    time: Vec<i64>,
}

fn atr_at(high: &[f64], low: &[f64], close: &[f64], period: usize, idx: usize) -> f64 {
    if idx < period {
        return 0.0;
    }
    let mut trs = Vec::with_capacity(period);
    for i in (idx + 1 - period)..=idx {
        let h = high.get(i).copied().unwrap_or(0.0);
        let l = low.get(i).copied().unwrap_or(0.0);
        let c0 = *close.get(i.saturating_sub(1)).unwrap_or(&0.0);
        trs.push((h - l).max((h - c0).abs()).max((l - c0).abs()));
    }
    if trs.is_empty() {
        return 0.0;
    }
    trs.iter().sum::<f64>() / period as f64
}

#[tokio::main]
async fn main() -> Result<()> {
    println!("Loading data for {} symbols...", SYMBOLS.len());
    let loader = DataLoader::new(None, None);
    let mut raw_cache: HashMap<String, DataFrame> = HashMap::new();
    for sym in SYMBOLS {
        match loader.fetch_with_cache(sym, "1d", CANDLES).await {
            Ok(df) => {
                raw_cache.insert(sym.to_string(), df);
            }
            Err(e) => {
                eprintln!("WARNING: {} failed: {}", sym, e);
            }
        }
    }

    let min_len = raw_cache
        .values()
        .map(|df: &DataFrame| df.height())
        .min()
        .unwrap_or(0)
        .min(2800);

    let mut sym_data: HashMap<String, SymData> = HashMap::new();
    for (sym, df) in &raw_cache {
        let n = min_len;
        macro_rules! col_f64 {
            ($name:expr) => {{
                let chunked = df.column($name)?.f64()?;
                chunked.into_iter().filter_map(|x| x).take(n).collect::<Vec<_>>()
            }};
        }
        let time_col = df.column("time")?;
        let time_vals: Vec<i64> = if let Ok(dt) = time_col.datetime() {
            dt.into_iter().filter_map(|x| x).take(n).collect()
        } else {
            let ms = time_col.cast(&DataType::Int64)?;
            ms.i64()?.into_iter().filter_map(|x| x).take(n).collect()
        };

        sym_data.insert(
            sym.clone(),
            SymData {
                close: col_f64!("close"),
                high: col_f64!("high"),
                low: col_f64!("low"),
                vol: col_f64!("volume"),
                time: time_vals,
            },
        );
    }

    let symbols: Vec<String> = SYMBOLS.iter().map(|s| s.to_string()).collect();
    let n = min_len;
    let mut trades_out: Vec<(String, i32, usize, usize, f64, f64, f64, usize, String)> =
        Vec::new();
    let mut bar = 0usize;

    while bar + 2 < n {
        let mut scores: Vec<(&str, f64)> = Vec::new();
        for sym in &symbols {
            if let Some(sd) = sym_data.get(sym) {
                if bar >= sd.close.len() {
                    continue;
                }
                let dv = sd.vol.get(bar).copied().unwrap_or(0.0)
                    * sd.close.get(bar).copied().unwrap_or(0.0);
                scores.push((
                    sym.as_str(),
                    if dv.is_finite() && dv > 0.0 { dv } else { 0.0 },
                ));
            }
        }
        scores.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());
        let top_syms: Vec<&str> = scores.into_iter().take(3).map(|(s, _)| s).collect();

        if top_syms.is_empty() {
            bar += 1;
            continue;
        }

        let mut entered = false;
        for sym in top_syms {
            if let Some(sd) = sym_data.get(sym) {
                if bar < TURTLE_ENTRY + 1 || bar >= sd.close.len() {
                    continue;
                }

                let start = bar + 1 - TURTLE_ENTRY;
                let max_close =
                    sd.close[start..bar].iter().fold(f64::NEG_INFINITY, |m, &c| m.max(c));
                if sd.close[bar] <= max_close {
                    continue;
                }

                let entry_px = sd.close[bar];
                let entry_fee = entry_px * (1.0 - TAKER_FEE);
                let entry_bar = bar;
                let n_bars = sd.close.len();

                let mut highest_chand = sd.high[bar + 1];
                let mut highest_turtle = sd.high[bar + 1];
                let max_bar = (bar + 1 + HOLD_MAX).min(n_bars.saturating_sub(1));
                let mut exit_bar = max_bar;
                let mut exit_reason = "hold_max";

                for b in (bar + 1)..=max_bar {
                    highest_chand = highest_chand.max(sd.high[b]);
                    let atr_chand =
                        atr_at(&sd.high, &sd.low, &sd.close, CHAND_PERIOD, b);
                    let trail_chand = highest_chand - CHAND_MULT * atr_chand;

                    highest_turtle = highest_turtle.max(sd.high[b]);
                    let atr_turtle =
                        atr_at(&sd.high, &sd.low, &sd.close, TURTLE_ATR_PERIOD, b);
                    let trail_turtle = highest_turtle - TURTLE_ATR_MULT * atr_turtle;

                    if sd.close[b] < trail_chand {
                        exit_bar = b;
                        exit_reason = "chandelier";
                        break;
                    }
                    if sd.close[b] < trail_turtle {
                        exit_bar = b;
                        exit_reason = "turtle_atr";
                        break;
                    }
                }

                if exit_bar < bar + 1 {
                    exit_bar = bar + 1;
                }

                let exit_px = *sd.close.get(exit_bar).unwrap_or(&entry_px);
                let exit_fee = exit_px * (1.0 - TAKER_FEE);
                let gross_ret = exit_fee / entry_fee - 1.0;
                let bars_held = (exit_bar as i64 - entry_bar as i64).max(1) as usize;

                let ts = *sd.time.get(entry_bar).unwrap_or(&0);
                let secs = (ts / 1000) as i64;
                let days_since_epoch = secs / 86400;
                let year = (1970.0 + days_since_epoch as f64 / 365.25) as i32;

                trades_out.push((
                    sym.to_string(),
                    year,
                    entry_bar,
                    exit_bar,
                    entry_px,
                    exit_px,
                    gross_ret,
                    bars_held,
                    exit_reason.to_string(),
                ));

                bar = exit_bar + 1;
                entered = true;
                break;
            }
        }

        if !entered {
            bar += 1;
        }
    }

    // Write CSV
    let mut f = std::fs::File::create("snapshots/trade_expectancy.csv")?;
    use std::io::Write;
    writeln!(f, "symbol,year,entry_bar,exit_bar,entry_px,exit_px,ret_pct,bars_held,exit_reason")?;
    for (sym, yr, eb, exb, ep, xp, ret, bh, er) in &trades_out {
        writeln!(f, "{},{},{},{},{},{},{},{},{}", sym, yr, eb, exb, ep, xp, ret * 100.0, bh, er)?;
    }

    let n = trades_out.len();
    let rets: Vec<f64> = trades_out.iter().map(|t| t.6).collect();
    let wins_usize = rets.iter().filter(|&&r| r > 0.0).count();
    let losses = n - wins_usize;
    let win_rate = wins_usize as f64 / n as f64 * 100.0;
    let avg_win = if wins_usize > 0 {
        rets.iter().filter(|&&r| r > 0.0).sum::<f64>() / wins_usize as f64 * 100.0
    } else {
        0.0
    };
    let avg_loss = if losses > 0 {
        rets.iter().filter(|&&r| r < 0.0).sum::<f64>() / losses as f64 * 100.0
    } else {
        0.0
    };
    let avg_trade = rets.iter().sum::<f64>() / n as f64 * 100.0;
    let expectancy = (win_rate / 100.0) * (avg_win / 100.0)
        - ((100.0 - win_rate) / 100.0) * (avg_loss.abs() / 100.0);
    let pf = if avg_loss.abs() > 0.0001 {
        (wins_usize as f64 * avg_win) / (losses as f64 * avg_loss.abs())
    } else {
        0.0
    };

    let mut sorted = rets.clone();
    sorted.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let median = sorted[n / 2] * 100.0;
    let p5 = sorted[(n as f64 * 0.05) as usize] * 100.0;
    let p10 = sorted[(n as f64 * 0.10) as usize] * 100.0;
    let p90 = sorted[(n as f64 * 0.90) as usize] * 100.0;
    let p95 = sorted[(n as f64 * 0.95) as usize] * 100.0;
    let best = *sorted.last().unwrap_or(&0.0) * 100.0;
    let worst = *sorted.first().unwrap_or(&0.0) * 100.0;

    println!("\n{}", "=".repeat(70));
    println!("  TRADE EXPECTANCY — Turtle+Chandelier NoDOGE");
    println!("{}", "=".repeat(70));
    println!("  Total trades:    {}", n);
    println!("  Win rate:       {:.1}%", win_rate);
    println!("  Avg win:        {:.2}%", avg_win);
    println!("  Avg loss:       {:.2}%", avg_loss);
    println!("  Avg trade:      {:.3}%", avg_trade);
    println!("  Expectancy:     {:.4} (per trade)", expectancy * 100.0);
    println!("  Profit factor:  {:.2}x", pf);
    println!("  Median return:  {:.2}%", median);
    println!("  P5:             {:.2}%", p5);
    println!("  P10:            {:.2}%", p10);
    println!("  P90:            {:.2}%", p90);
    println!("  P95:            {:.2}%", p95);
    println!("  Best trade:     {:.1}%", best);
    println!("  Worst trade:    {:.1}%", worst);
    println!("{}", "=".repeat(70));

    let chand: Vec<_> = trades_out.iter().filter(|t| t.8 == "chandelier").collect();
    let turtle: Vec<_> = trades_out.iter().filter(|t| t.8 == "turtle_atr").collect();
    let hold: Vec<_> = trades_out.iter().filter(|t| t.8 == "hold_max").collect();

    println!("\n  Exit reasons:");
    for (label, group) in &[("Chandelier", &chand), ("Turtle ATR", &turtle), ("Hold Max", &hold)] {
        if group.is_empty() {
            continue;
        }
        let wr = group.iter().filter(|t| t.6 > 0.0).count() as f64 / group.len() as f64 * 100.0;
        let avg = group.iter().map(|t| t.6).sum::<f64>() / group.len() as f64 * 100.0;
        println!("    {:<12} n={:4}  avg={:+7.2}%  wr={:.0}%", label, group.len(), avg, wr);
    }

    println!("\n  By symbol:");
    for sym in &["BTCUSDT", "ETHUSDT", "SOLUSDT", "XRPUSDT", "DOGEUSDT"] {
        let s: Vec<_> = trades_out.iter().filter(|t| t.0 == *sym).collect();
        if s.is_empty() {
            continue;
        }
        let wr = s.iter().filter(|t| t.6 > 0.0).count() as f64 / s.len() as f64 * 100.0;
        let avg = s.iter().map(|t| t.6).sum::<f64>() / s.len() as f64 * 100.0;
        println!("    {:<10} n={:4}  avg={:+7.2}%  wr={:.0}%", sym, s.len(), avg, wr);
    }

    let times: Vec<usize> = trades_out.iter().map(|t| t.7).collect();
    let avg_t = times.iter().sum::<usize>() as f64 / times.len() as f64;
    let mut ts_sorted = times.clone();
    ts_sorted.sort();
    let med_t = ts_sorted[ts_sorted.len() / 2];
    let max_t = *times.iter().max().unwrap_or(&1);
    println!("\n  Holding times: avg={:.1} bars, median={}, max={}", avg_t, med_t, max_t);

    println!("\n  Distribution:");
    let below_neg10 = rets.iter().filter(|&&r| r < -0.10).count();
    let above_10 = rets.iter().filter(|&&r| r > 0.10).count();
    let between = n - below_neg10 - above_10;
    println!(
        "    >+10%:   {:4} ({:.0}%)",
        above_10,
        above_10 as f64 / n as f64 * 100.0
    );
    println!(
        "    -10% to +10%: {:4} ({:.0}%)",
        between,
        between as f64 / n as f64 * 100.0
    );
    println!(
        "    <-10%:   {:4} ({:.0}%)",
        below_neg10,
        below_neg10 as f64 / n as f64 * 100.0
    );

    println!("\n  Per-year:");
    let mut years: Vec<i32> = trades_out
        .iter()
        .map(|t| t.1)
        .collect::<std::collections::HashSet<_>>()
        .into_iter()
        .collect();
    years.sort();
    for yr in years {
        let yr_trades: Vec<_> = trades_out.iter().filter(|t| t.1 == yr).collect();
        if yr_trades.is_empty() {
            continue;
        }
        let yr_wr = yr_trades.iter().filter(|t| t.6 > 0.0).count() as f64 / yr_trades.len() as f64 * 100.0;
        let yr_avg = yr_trades.iter().map(|t| t.6).sum::<f64>() / yr_trades.len() as f64 * 100.0;
        println!("    {:4}: n={:3}  avg={:+7.2}%  wr={:.0}%", yr, yr_trades.len(), yr_avg, yr_wr);
    }

    println!("{}", "=".repeat(70));
    println!("\nCSV: snapshots/trade_expectancy.csv");
    println!("Charts: python3 charts/plot_trade_expectancy.py");

    Ok(())
}