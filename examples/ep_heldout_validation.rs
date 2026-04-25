//! EP Held-Out Validation: EP=21 vs EP=24 on Pre-2021 Data
//!
//! PURPOSE: T3 integrity test — validate EP=24 (current default) against EP=21
//! on data the hyperopt NEVER used (pre-2021 held-out).
//!
//! METHOD: 3 pre-2021 test phases, each symbol × EP=21 and EP=24
//! - P1: Train ≤ 2019, Test 2020 (COVID crash + recovery)
//! - P2: Train ≤ 2020, Test 2021 (ETF mega-bull)  
//! - P3: Train ≤ 2018, Test 2019 (pre-COVID bear)
//!
//! Production params: CHAND(7,2.30), HOLD_MAX=12, ATR_ENTRY_MULT=0.00
//!
//! Usage: cargo run --example ep_heldout_validation --profile sweep

use anyhow::Result;
use chrono::Datelike;
use krypto::data::loader::DataLoader;
use polars::prelude::*;
use std::collections::HashMap;
use std::fs::File;
use std::io::Write;
use std::time::Instant;

const CANDLES: u32 = 3000;
const TAKER_FEE: f64 = 0.001;
const MIN_TRADES: usize = 3;
const CHAND_PERIOD: usize = 7;
const CHAND_MULT: f64 = 2.30;
const TURTLE_ATR_PERIOD: usize = 24;
const TURTLE_ATR_MULT: f64 = 2.0;
const ATR_ENTRY_MULT: f64 = 0.00;
const HOLD_MAX: usize = 12;
const POSITION_CAP: usize = 3;

const SYMBOLS: [&str; 10] = [
    "BTCUSDT", "ETHUSDT", "SOLUSDT", "XRPUSDT", "DOGEUSDT",
    "ADAUSDT", "LTCUSDT", "EOSUSDT", "BNBUSDT", "BCHUSDT",
];

#[derive(Clone)]
struct SymData {
    time:  Vec<i64>,
    close: Vec<f64>,
    high:  Vec<f64>,
    low:   Vec<f64>,
}

fn ms_to_year(ms: i64) -> i32 {
    chrono::DateTime::from_timestamp(ms / 1000, 0)
        .map(|dt| dt.year()).unwrap_or(1970)
}

fn atr_at(high: &[f64], low: &[f64], close: &[f64], period: usize, idx: usize) -> f64 {
    if idx < period { return 0.0; }
    let mut trs = vec![];
    for i in (idx + 1 - period)..=idx {
        let h = high.get(i).copied().unwrap_or(0.0);
        let l = low.get(i).copied().unwrap_or(0.0);
        let prev_c = close.get(i.saturating_sub(1)).copied().unwrap_or(h);
        trs.push((h - l).max((h - prev_c).abs()).max((l - prev_c).abs()));
    }
    trs.iter().sum::<f64>() / period as f64
}

fn turtle_signal(close: &[f64], high: &[f64], low: &[f64],
                 ep: usize, atr_period: usize, _atr_entry_mult: f64, idx: usize) -> bool {
    if idx < ep + atr_period { return false; }
    // Rank by ATR% over ep lookback
    let mut candidates = vec![];
    for i in atr_period..=idx {
        let hh = high[atr_period..=i].iter().fold(f64::NEG_INFINITY, |m, &v| m.max(v));
        let ll = low[atr_period..=i].iter().fold(f64::INFINITY, |m, &v| m.min(v));
        let channel = hh - ll;
        let pct = if channel > 0.0 { (high[i] - ll) / channel } else { 0.0 };
        candidates.push((pct, i));
    }
    candidates.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap());
    candidates.iter().take(ep).any(|&(_, i)| i == idx)
}

fn run_backtest(data: &SymData, train_end: usize, test_end: usize, ep: usize)
    -> (bool, f64, f64, f64, i64)
{
    let mut equity = 1.0;
    let mut eq_curve = vec![equity];
    let mut active = false;
    let mut entry_px = 0.0;
    let mut entry_bar = 0;
    let mut trades = 0i64;
    let mut wins = 0i64;
    let mut bars_held = 0;
    let mut peak = 1.0_f64;
    let mut max_dd = 0.0_f64;

    for bar in (train_end + 1)..=test_end.min(data.close.len() - 1) {
        // Exit check
        if active {
            bars_held += 1;
            let exit_turtle = {
                let atr = atr_at(&data.high, &data.low, &data.close, TURTLE_ATR_PERIOD, bar);
                data.close[bar] < entry_px * (1.0 - TURTLE_ATR_MULT * atr / entry_px)
            };
            let exit_chand = {
                let hh = data.high[entry_bar..=bar].iter().fold(f64::NEG_INFINITY, |m, &v| m.max(v));
                let atr = atr_at(&data.high, &data.low, &data.close, CHAND_PERIOD, bar);
                data.close[bar] < hh - CHAND_MULT * atr
            };
            if exit_turtle || exit_chand || bars_held >= HOLD_MAX {
                let slip_pct = 10.0_f64 / 10000.0;
                let exit = data.close[bar] * (1.0 - slip_pct) * (1.0 - TAKER_FEE);
                let ret = exit / entry_px - 1.0;
                equity *= 1.0 + ret;
                if ret > 0.0 { wins += 1; }
                trades += 1;
                active = false;
            }
        }

        // Entry check
        if !active && turtle_signal(&data.close, &data.high, &data.low, ep, TURTLE_ATR_PERIOD, ATR_ENTRY_MULT, bar) {
            let slip_pct = 10.0_f64 / 10000.0;
            entry_px = data.close[bar] * (1.0 + slip_pct) * (1.0 + TAKER_FEE);
            entry_bar = bar;
            bars_held = 0;
            active = true;
        }

        peak = peak.max(equity);
        let dd = (peak - equity) / peak;
        max_dd = max_dd.max(dd);
        eq_curve.push(equity);
    }

    if eq_curve.len() < 2 { return (false, -100.0, 0.0, 0.0, 0); }
    let rets: Vec<f64> = eq_curve.windows(2).map(|w| (w[1] - w[0]) / w[0]).collect();
    let ann_sharpe = {
        let mn: f64 = rets.iter().sum::<f64>() / rets.len() as f64;
        let sd = (rets.iter().map(|r| (r - mn).powi(2)).sum::<f64>() / rets.len() as f64).sqrt();
        if sd == 0.0 { 0.0 } else { mn / sd * (365.25_f64 / rets.len() as f64).sqrt() }
    };
    let pass = trades >= MIN_TRADES as i64 && ann_sharpe > 0.0 && max_dd < 0.50;
    (pass, (equity - 1.0) * 100.0, ann_sharpe, max_dd * 100.0, trades)
}

#[tokio::main]
async fn main() -> Result<()> {
    let start = Instant::now();
    println!("\n=== EP Held-Out Validation: EP=21 vs EP=24 (Pre-2021 Data) ===\n");

    let loader = DataLoader::new(None, None);
    let all_syms: std::collections::HashSet<String> = SYMBOLS.iter().map(|s| s.to_string()).collect();

    let mut raw_cache: HashMap<String, DataFrame> = HashMap::new();
    for sym in all_syms.iter() {
        match loader.fetch_with_cache(sym, "1d", CANDLES).await {
            Ok(df) => { raw_cache.insert(sym.clone(), df); }
            Err(e) => { eprintln!("  WARNING: {} load failed: {}", sym, e); }
        }
    }

    // Build SymData for each symbol (truncate to common min length)
    let n = raw_cache.values().map(|df| df.height()).min().unwrap_or(0).min(2800);
    let mut sym_data_map: HashMap<String, SymData> = HashMap::new();
    for sym in all_syms.iter() {
        if let Some(df) = raw_cache.get(sym) {
            macro_rules! col_vec {
                ($name:expr) => {{
                    let chunked = df.column($name)?.f64()?;
                    chunked.into_iter().filter_map(|x| x).take(n).collect::<Vec<_>>()
                }};
            }
            let time_col = df.column("time")?.datetime()?;
            let time: Vec<i64> = time_col.into_iter().filter_map(|x| x).take(n).collect();
            sym_data_map.insert(sym.clone(), SymData {
                close: col_vec!("close"),
                high:  col_vec!("high"),
                low:   col_vec!("low"),
                time,
            });
        }
    }
    println!("Loaded {} symbols, {} bars\n", sym_data_map.len(), n);

    // ── Pre-2021 held-out phases ─────────────────────────────────────────
    let phases = [
        ("P1-2020", "Train≤2019 / Test 2020", 2020),
        ("P2-2021", "Train≤2020 / Test 2021", 2021),
        ("P3-2019", "Train≤2018 / Test 2019", 2019),
    ];

    let mut all_results: Vec<(String, String, bool, f64, f64, f64, i64)> = Vec::new();
    let mut by_phase: HashMap<String, Vec<(bool, f64)>> = HashMap::new();

    for (phase, desc, test_year) in phases {
        println!("--- {phase}: {desc} ---");
        for sym in SYMBOLS {
            let Some(data) = sym_data_map.get(sym) else { continue; };
            let train_end = data.time.iter().position(|&ms| ms_to_year(ms) >= test_year).unwrap_or(0);
            let test_end = data.close.len().saturating_sub(1);
            if train_end < 300 || test_end <= train_end { continue; }

            let (p21, r21, sh21, dd21, t21) = run_backtest(data, train_end, test_end, 21);
            let (p24, r24, sh24, dd24, t24) = run_backtest(data, train_end, test_end, 24);

            let winner = if p24 && (!p21 || sh24 > sh21) { "24" } else { "21" };
            println!("  {sym:10} EP21={p21}({r21:+.1}%,SH={sh21:.2},T={t21}) EP24={p24}({r24:+.1}%,SH={sh24:.2},T={t24}) → EP={winner}");

            all_results.push((sym.to_string(), format!("{}_21", phase), p21, r21, sh21, dd21, t21));
            all_results.push((sym.to_string(), format!("{}_24", phase), p24, r24, sh24, dd24, t24));
            by_phase.entry(format!("{}_21", phase)).or_default().push((p21, sh21));
            by_phase.entry(format!("{}_24", phase)).or_default().push((p24, sh24));
        }
        println!();
    }

    // ── Aggregate ────────────────────────────────────────────────────────
    println!("========== AGGREGATE RESULTS ==========");
    let mut ep21_pass = 0; let mut ep21_total = 0; let mut ep21_sh = 0.0_f64;
    let mut ep24_pass = 0; let mut ep24_total = 0; let mut ep24_sh = 0.0_f64;

    for (phase, desc, test_year) in phases {
        let r21 = by_phase.get(&format!("{}_21", phase)).cloned().unwrap_or_default();
        let r24 = by_phase.get(&format!("{}_24", phase)).cloned().unwrap_or_default();
        let (p21, sh21) = if !r21.is_empty() {
            let ps = r21.iter().filter(|(p, _)| *p).count();
            let sh = r21.iter().map(|(_, s)| *s).sum::<f64>() / r21.len() as f64;
            ep21_pass += ps; ep21_total += r21.len(); ep21_sh += sh * r21.len() as f64;
            (ps, sh)
        } else { (0, 0.0) };
        let (p24, sh24) = if !r24.is_empty() {
            let ps = r24.iter().filter(|(p, _)| *p).count();
            let sh = r24.iter().map(|(_, s)| *s).sum::<f64>() / r24.len() as f64;
            ep24_pass += ps; ep24_total += r24.len(); ep24_sh += sh * r24.len() as f64;
            (ps, sh)
        } else { (0, 0.0) };
        println!("  {phase} [{desc}]: EP21={p21}/{} pass, Sharpe={sh21:.2}  |  EP24={p24}/{} pass, Sharpe={sh24:.2}",
            r21.len(), r24.len());
    }

    let ep21_avg_sh = ep21_sh / ep21_total.max(1) as f64;
    let ep24_avg_sh = ep24_sh / ep24_total.max(1) as f64;
    let ep21_pct = 100.0 * ep21_pass as f64 / ep21_total.max(1) as f64;
    let ep24_pct = 100.0 * ep24_pass as f64 / ep24_total.max(1) as f64;

    println!("\n  OVERALL: EP=21: {}/{} ({:.1}%) Sharpe={:.2}", ep21_pass, ep21_total, ep21_pct, ep21_avg_sh);
    println!("  OVERALL: EP=24: {}/{} ({:.1}%) Sharpe={:.2}", ep24_pass, ep24_total, ep24_pct, ep24_avg_sh);

    // ── Verdict ──────────────────────────────────────────────────────────
    println!("\n========== VERDICT ==========");
    let delta = ep24_pass as i32 - ep21_pass as i32;
    if ep24_pass >= ep21_pass && ep24_avg_sh >= ep21_avg_sh {
        println!("  ✅ EP=24 HOLDS on held-out: delta={:+} passes, Sharpe +{:.2}", delta, ep24_avg_sh - ep21_avg_sh);
        println!("     EP=24 is NOT in-sample inflation. Keep as production default.");
    } else if ep21_pass > ep24_pass || (ep21_pass == ep24_pass && ep21_avg_sh > ep24_avg_sh) {
        println!("  ⚠️  EP=21 WINS on held-out: delta={:+}, Sharpe {:.2} vs {:.2}", delta, ep21_avg_sh, ep24_avg_sh);
        println!("     EP=24 was in-sample inflation. REVERT to EP=21.");
    } else {
        println!("  ⚖️  STATISTICALLY EQUIVALENT. EP21={} vs EP24={} passes, Sharpe {:.2} vs {:.2}",
            ep21_pass, ep24_pass, ep21_avg_sh, ep24_avg_sh);
    }

    // ── CSV ───────────────────────────────────────────────────────────────
    let csv_path = "snapshots/ep_heldout_validation.csv";
    let mut csv = File::create(csv_path)?;
    writeln!(csv, "symbol,phase,ep,pass,return_pct,sharpe,max_dd_pct,trades")?;
    for (sym, phase, pass, ret, sh, dd, trades) in &all_results {
        let ep_str = if phase.ends_with("_21") { "21" } else { "24" };
        writeln!(csv, "{sym},{phase},{ep_str},{pass},{:.2},{:.4},{:.2},{trades}", ret, sh, dd)?;
    }
    println!("\nCSV: {csv_path}");
    println!("Total time: {:.1}s", start.elapsed().as_secs_f32());
    Ok(())
}