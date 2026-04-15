//! MTF Turtle 4h + Daily BTC Trend Filter Walk-Forward
//!
//! Tests: Turtle(EP=21) 4h breakout filtered by BTC daily SMA(21)>SMA(55).
//! Baseline: Turtle 4h WITHOUT daily filter (same params, same windows).
//!
//! Run: cargo run --example mtf_turtle_daily_wf --profile sweep

use anyhow::Result;
use krypto::data::loader::DataLoader;
use polars::prelude::*;
use std::collections::HashMap;
use std::fs::File;
use std::io::Write;
use std::time::Instant;

const CANDLES: u32 = 4000;
const TRAIN_BARS: usize = 252;
const TEST_BARS: usize = 252;
const MIN_TRADES: usize = 3;
const EP: usize = 21;
const POS_CAP: usize = 3;
const HOLD_MAX: usize = 45;
const CHAND_P: usize = 28;
const CHAND_M: f64 = 2.0;
const ATR_P: usize = 25;
const ATR_M: f64 = 2.0;
const DAILY_FAST: usize = 21;
const DAILY_SLOW: usize = 55;
const SYMBOLS: [&str; 5] = ["BTCUSDT", "ETHUSDT", "SOLUSDT", "XRPUSDT", "DOGEUSDT"];
const CSV_OUT: &str = "snapshots/mtf_turtle_daily_wf.csv";
const MD_OUT: &str = "snapshots/mtf_turtle_daily_wf.md";

fn tr(high: f64, low: f64, pc: f64) -> f64 {
    (high - low).max((high - pc).abs()).max((low - pc).abs())
}
fn calc_atr(h: &[f64], l: &[f64], c: &[f64], p: usize, idx: usize) -> f64 {
    if idx < p { return 0.0; }
    let mut s = 0.0;
    for j in (idx + 1 - p)..=idx {
        let pc = if j == 0 { c[0] } else { c[j - 1] };
        s += tr(h[j], l[j], pc);
    }
    s / p as f64
}
fn daily_bull(c: &[f64], idx: usize) -> bool {
    if idx >= c.len() || idx < DAILY_SLOW { return true; }
    let a21 = c[idx + 1 - DAILY_FAST..=idx].iter().sum::<f64>() / DAILY_FAST as f64;
    let a55 = c[idx + 1 - DAILY_SLOW..=idx].iter().sum::<f64>() / DAILY_SLOW as f64;
    a21 > a55
}
fn max_dd_from(eq: &[f64]) -> f64 {
    let mut peak = f64::MIN;
    let mut mdd = 0.0;
    for &e in eq {
        if e > peak { peak = e; }
        let dd = (peak - e) / peak;
        if dd > mdd { mdd = dd; }
    }
    mdd
}
fn annualised_sharpe(returns: &[f64]) -> f64 {
    if returns.len() < 2 { return 0.0; }
    let mn: f64 = returns.iter().sum::<f64>() / returns.len() as f64;
    let sd = (returns.iter().map(|x| (x - mn).powi(2)).sum::<f64>() / returns.len() as f64).sqrt();
    if sd == 0.0 { return 0.0; }
    mn * (252.0_f64).sqrt() / sd
}

struct SimResult {
    ret: f64, sharpe: f64, max_dd: f64,
    trades: usize, win_rate: f64, pass: bool,
}

fn run_sim(b4h_h: &[f64], b4h_l: &[f64], b4h_c: &[f64],
           daily: &[f64], dpi: &[usize],
           ts: usize, te: usize, fee: f64, filter: bool) -> SimResult {
    let mut eq = vec![1.0];
    let mut pos = None::<(f64, f64, usize)>; // (entry, high_h water, bars)
    let mut conc = 0usize;
    let mut wins = 0usize;
    let mut ntrade = 0usize;
    let mut daily_idx = 0usize;

    for i in ts..te {
        let cl = b4h_c[i];
        let hi = b4h_h[i];
        let di = (i / 6).min(daily.len().saturating_sub(1));
        let bull = daily_bull(daily, di);

        if let Some((entry, mut hh, mut bh)) = pos {
            let atr = calc_atr(b4h_h, b4h_l, b4h_c, CHAND_P, i);
            let chand_exit = hh - cl > CHAND_M * atr;
            let atr2 = calc_atr(b4h_h, b4h_l, b4h_c, ATR_P, i);
            let atr_exit = hh - cl > ATR_M * atr2;
            let hold_exit = bh >= HOLD_MAX;

            if chand_exit || atr_exit || hold_exit {
                let r = (cl - entry) / entry - fee;
                if r > 0.0 { wins += 1; }
                ntrade += 1;
                let last = eq.last().copied().unwrap_or(1.0);
                eq.push(last * (1.0 + r));
                conc = conc.saturating_sub(1);
                pos = None;
            } else {
                hh = hh.max(hi);
                let last = eq.last().copied().unwrap_or(1.0);
                let bar_ret = if i > ts { (cl - b4h_c[i - 1]) / b4h_c[i - 1] } else { 0.0 };
                eq.push(last * (1.0 + bar_ret));
                pos = Some((entry, hh, bh + 1));
            }
        } else {
            if !filter || bull {
                if conc < POS_CAP && i >= EP {
                    let start = i.saturating_sub(EP);
                    let max_h = b4h_h[start..i].iter().fold(0.0f64, |a, &b| a.max(b));
                    if cl > max_h {
                        conc += 1;
                        pos = Some((cl, hi, 0));
                    }
                }
            }
        };
        if pos.is_none() {
            let last = eq.last().copied().unwrap_or(1.0);
            let bar_ret = if i > ts { (cl - b4h_c[i - 1]) / b4h_c[i - 1] } else { 0.0 };
            eq.push(last * (1.0 + bar_ret));
        }
    }

    let returns: Vec<f64> = eq.windows(2).map(|w| (w[1] - w[0]) / w[0]).collect();
    let mdd = max_dd_from(&eq);
    let sh = annualised_sharpe(&returns);
    SimResult {
        ret: (eq.last().copied().unwrap_or(1.0) - 1.0) * 100.0,
        sharpe: sh,
        max_dd: mdd * 100.0,
        trades: ntrade,
        win_rate: if ntrade > 0 { wins as f64 / ntrade as f64 * 100.0 } else { 0.0 },
        pass: eq.last().copied().unwrap_or(1.0) > 1.0 && ntrade >= MIN_TRADES,
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    let t0 = Instant::now();
    let loader = DataLoader::new(None, None);

    eprintln!("==== MTF Turtle 4h + Daily BTC Trend Filter ====");
    eprintln!("Turtle(EP=21) 4h + Chandelier(28,2) + ATR(25,2)");
    eprintln!("Filter: BTC daily SMA(21) > SMA(55) = BULL");
    eprintln!();

    // Load BTC daily for trend filter
    let btc_df = loader.fetch_with_cache("BTCUSDT", "1d", 600).await?;
    let btc_close: Vec<f64> = {
        let col = btc_df.column("close")?.f64()?;
        col.into_iter().filter_map(|x| x).collect()
    };
    eprintln!("BTC 1d: {} bars", btc_close.len());

    // Load 4h bars for all symbols
    let mut b4h_h = HashMap::<&str, Vec<f64>>::new();
    let mut b4h_l = HashMap::<&str, Vec<f64>>::new();
    let mut b4h_c = HashMap::<&str, Vec<f64>>::new();

    for sym in SYMBOLS {
        let df = loader.fetch_with_cache(sym, "4h", CANDLES).await?;
        let h: Vec<f64> = {
            let col = df.column("high")?.f64()?;
            col.into_iter().filter_map(|x| x).collect()
        };
        let l: Vec<f64> = {
            let col = df.column("low")?.f64()?;
            col.into_iter().filter_map(|x| x).collect()
        };
        let c: Vec<f64> = {
            let col = df.column("close")?.f64()?;
            col.into_iter().filter_map(|x| x).collect()
        };
        eprintln!("{sym} 4h: {} bars", c.len());
        b4h_h.insert(sym, h);
        b4h_l.insert(sym, l);
        b4h_c.insert(sym, c);
    }

    let n4h = b4h_c["BTCUSDT"].len();
    let dpi: Vec<usize> = (0..n4h).map(|i| i / 6).collect();
    let nwin = (n4h.saturating_sub(TRAIN_BARS)) / TEST_BARS;
    eprintln!("\nWalk-forward windows: {} per symbol", nwin);

    let mut csv = vec!["symbol,config,window,return_pct,sharpe,max_dd_pct,trades,win_rate_pct,pass".to_string()];
    let mut g_mtf_pass = 0usize; let mut g_bas_pass = 0usize;
    let mut g_mtf_sh = 0.0f64;   let mut g_bas_sh = 0.0f64;
    let mut g_mtf_t = 0usize;    let mut g_bas_t = 0usize;

    for sym in SYMBOLS {
        let h = &b4h_h[sym];
        let l = &b4h_l[sym];
        let c = &b4h_c[sym];
        let n = c.len();
        eprintln!("\n==== {sym} ====");

        for wi in 0..nwin {
            let ts = TRAIN_BARS + wi * TEST_BARS;
            let te = (ts + TEST_BARS).min(n);
            let fee = 0.0004;

            let mtf = run_sim(h, l, c, &btc_close, &dpi, ts, te, fee, true);
            let bas = run_sim(h, l, c, &btc_close, &dpi, ts, te, fee, false);

            eprintln!("  W{:02} | MTF {:+8.1}% DD={:5.1}% {:3}t {:4.1}% {} | Turtle {:+8.1}% DD={:5.1}% {:3}t {:4.1}% {}",
                wi, mtf.ret, mtf.max_dd, mtf.trades, mtf.win_rate,
                if mtf.pass { "PASS" } else { "FAIL" },
                bas.ret, bas.max_dd, bas.trades, bas.win_rate,
                if bas.pass { "PASS" } else { "FAIL" });

            if mtf.pass { g_mtf_pass += 1; }
            if bas.pass { g_bas_pass += 1; }
            g_mtf_sh += mtf.sharpe; g_bas_sh += bas.sharpe;
            g_mtf_t += mtf.trades; g_bas_t += bas.trades;

            csv.push(format!("{},MTF_filter,W{:02},{:.2},{:.4},{:.2},{},{:.2},{}",
                sym, wi, mtf.ret, mtf.sharpe, mtf.max_dd, mtf.trades, mtf.win_rate,
                if mtf.pass { "true" } else { "false" }));
            csv.push(format!("{},Turtle_no_filter,W{:02},{:.2},{:.4},{:.2},{},{:.2},{}",
                sym, wi, bas.ret, bas.sharpe, bas.max_dd, bas.trades, bas.win_rate,
                if bas.pass { "true" } else { "false" }));
        }
    }

    let nr = SYMBOLS.len() * nwin;
    eprintln!("\n===== SUMMARY =====");
    eprintln!("MTF w/ Daily Filter:  {:3.0}% pass ({}/{}) | Sharpe {:.3} | {} trades",
        g_mtf_pass as f64 / nr as f64, g_mtf_pass, nr, g_mtf_sh / nr as f64, g_mtf_t);
    eprintln!("Turtle No Filter:    {:3.0}% pass ({}/{}) | Sharpe {:.3} | {} trades",
        g_bas_pass as f64 / nr as f64, g_bas_pass, nr, g_bas_sh / nr as f64, g_bas_t);
    eprintln!("Runtime: {:?}", t0.elapsed());

    let mut f = File::create(CSV_OUT)?;
    for l in &csv { writeln!(f, "{}", l)?; }

    let verdict = if g_mtf_pass > g_bas_pass || (g_mtf_pass == g_bas_pass && g_mtf_sh > g_bas_sh) {
        "DAILY FILTER IMPROVES"
    } else if g_mtf_pass == g_bas_pass {
        "FILTER NEUTRAL"
    } else {
        "FILTER HARMS — GRAVEYARD"
    };

    let mut md = File::create(MD_OUT)?;
    writeln!(md, "# MTF Turtle + Daily Trend Filter Walk-Forward\n")?;
    writeln!(md, "| Config | Pass Rate | Avg Sharpe | Trades |")?;
    writeln!(md, "|--------|----------|------------|-------|")?;
    writeln!(md, "| MTF w/ Filter | {}/{} ({:.0}%) | {:.3} | {} |",
        g_mtf_pass, nr, g_mtf_pass as f64/nr as f64*100.0, g_mtf_sh/nr as f64, g_mtf_t)?;
    writeln!(md, "| Turtle No Filter | {}/{} ({:.0}%) | {:.3} | {} |",
        g_bas_pass, nr, g_bas_pass as f64/nr as f64*100.0, g_bas_sh/nr as f64, g_bas_t)?;
    writeln!(md, "\n## Verdict\n\n{verdict}")?;

    Ok(())
}
