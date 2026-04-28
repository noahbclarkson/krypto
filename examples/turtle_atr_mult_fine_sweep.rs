//! =========================================================
//! TURTLE_ATR_MULT Fine Sweep — Turtle-Only Exit
//! =========================================================
//!
//! TARGET: TURTLE_ATR_MULT — Turtle ATR trailing stop multiplier
//! PRIOR:  coarse-swept 2026-04-12: {1.0..5.0 step 0.5} → M=2.00 winner
//! SWEEP:  M ∈ [1.00..5.00] step 0.05 → 81 values × 9 universes × 6 windows
//! STRATEGY: Turtle-only exit (Chandelier shadowed)
//! PARAMS:  EP=21, ATR_P=24, ATR_EM=0.00, HM=12, CAP=3, VL=9
//! METHOD:   Walk-forward 252 train / 252 test, pass rate primary

use anyhow::Result;
use krypto::data::loader::DataLoader;
use polars::prelude::*;
use std::collections::HashMap;
use std::fs::File;
use std::io::Write;
use std::time::Instant;

const CANDLES: u32 = 3000;
const TRAIN_BARS: usize = 252;
const TEST_BARS: usize = 252;
const HOLD_MAX: usize = 12;
const TAKER_FEE: f64 = 0.001;
const POSITION_CAP: usize = 3;
const MIN_TRADES: usize = 3;

const EP: usize = 21;
const ATR_P: usize = 24;
const ATR_EM: f64 = 0.00;
const VOL_LOOKBACK: usize = 8;

const SWEEP_START: f64 = 1.00;
const SWEEP_END: f64 = 5.00;
const SWEEP_STEP: f64 = 0.05;
const N_VALUES: usize = 81;

const SYMBOLS: [&str; 6] = ["BTCUSDT","ETHUSDT","SOLUSDT","XRPUSDT","DOGEUSDT","ADAUSDT"];

const UNIVERSES: &[(&str, &[&str])] = &[
    ("Base5",        &["BTCUSDT","ETHUSDT","SOLUSDT","XRPUSDT","DOGEUSDT","ADAUSDT"]),
    ("NoDOGE",       &["BTCUSDT","ETHUSDT","SOLUSDT","XRPUSDT","ADAUSDT"]),
    ("Legacy4",      &["BTCUSDT","ETHUSDT","XRPUSDT","LTCUSDT","EOSUSDT"]),
    ("Legacy5BNB",   &["BTCUSDT","ETHUSDT","XRPUSDT","LTCUSDT","BNBUSDT","EOSUSDT"]),
    ("OldGuardNoBNB",&["BTCUSDT","ETHUSDT","XRPUSDT","LTCUSDT","EOSUSDT","BCHUSDT"]),
    ("LargeCaps5",   &["BTCUSDT","ETHUSDT","SOLUSDT","XRPUSDT","BNBUSDT","ADAUSDT"]),
    ("Legacy3",      &["BTCUSDT","XRPUSDT","LTCUSDT","EOSUSDT"]),
    ("LowVolume5",   &["XRPUSDT","LTCUSDT","EOSUSDT","BCHUSDT","ADAUSDT"]),
    ("OldGuard4",    &["BTCUSDT","XRPUSDT","LTCUSDT","EOSUSDT","BCHUSDT"]),
];

const CSV_OUT: &str = "snapshots/turtle_atr_mult_fine_sweep.csv";
const DETAIL_OUT: &str = "snapshots/turtle_atr_mult_fine_sweep_detail.csv";

struct SymData {
    close: Vec<f64>,
    high: Vec<f64>,
    low: Vec<f64>,
    vol: Vec<f64>,
}

fn tr(h: f64, l: f64, prev_c: f64) -> f64 {
    (h - l).max((h - prev_c).abs()).max((l - prev_c).abs())
}

fn turtle_entry(close: &[f64], ep: usize, idx: usize) -> bool {
    if idx < ep + 1 { return false; }
    close[idx.wrapping_sub(ep)..idx].iter().fold(0.0f64, |m, &x| m.max(x)) < close[idx]
}

fn compute_atr(d: &SymData, abs_bar: usize) -> f64 {
    let start = abs_bar.saturating_sub(ATR_P);
    let mut sum = 0.0f64;
    for i in start..=abs_bar {
        let h_i = *d.high.get(i).unwrap_or(&0.0);
        let l_i = *d.low.get(i).unwrap_or(&0.0);
        let pc = *d.close.get(i.saturating_sub(1).max(0)).unwrap_or(&0.0);
        sum += tr(h_i, l_i, pc);
    }
    sum / ATR_P as f64
}

fn run_universe_window(
    data: &HashMap<String, SymData>,
    symbols: &[&str],
    mult: f64,
    test_start: usize,
    test_end: usize,
) -> Option<(f64, f64, f64, usize)> {
    let n_test = test_end.saturating_sub(test_start);
    let warmup = EP + ATR_P + 1;
    if n_test < warmup { return None; }

    // Volume rank in train period
    let mut vol_rank: Vec<(f64, &str)> = symbols.iter()
        .filter_map(|sym| {
            let d = data.get(*sym)?;
            if test_start > d.vol.len() { return None; }
            let train_vol: f64 = d.vol[0..test_start].iter().sum();
            Some((train_vol, *sym))
        })
        .collect();
    vol_rank.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap());
    let cap_syms: Vec<&str> = vol_rank.into_iter().take(POSITION_CAP).map(|(_, s)| s).collect();

    let mut equity = 1.0f64;
    let mut max_equity = 1.0f64;
    let mut max_dd = 0.0f64;
    let mut daily_rets: Vec<f64> = Vec::new();

    // Single position state
    let mut pos_open = false;
    let mut pos_sym: &str = "";
    let mut pos_entry_bar = 0isize;
    let mut pos_entry_close = 0.0f64;
    let mut pos_lowest_low = f64::INFINITY;
    let mut trades = 0usize;

    for b in 0..n_test {
        let abs_bar = test_start + b;

        // EXIT CHECK
        if pos_open {
            let bars_held = (b as isize - pos_entry_bar) as usize;
            let d_exit = data.get(pos_sym)?;
            let l_exit = *d_exit.low.get(abs_bar).unwrap_or(&0.0);
            let c_exit = *d_exit.close.get(abs_bar).unwrap_or(&0.0);

            // Update trailing stop
            if l_exit < pos_lowest_low {
                let atr = compute_atr(d_exit, abs_bar);
                if atr > 0.0 {
                    pos_lowest_low = l_exit - mult * atr;
                }
            }

            // Check exit
            let exit_triggered = if bars_held >= HOLD_MAX {
                Some(c_exit)
            } else if l_exit <= pos_lowest_low {
                Some(pos_lowest_low.max(l_exit))
            } else {
                None
            };

            if let Some(exit_px) = exit_triggered {
                let ret = (exit_px / pos_entry_close - 1.0) - TAKER_FEE;
                equity *= 1.0 + ret;
                daily_rets.push(ret);
                trades += 1;
                pos_open = false;
            }
        }

        // ENTRY CHECK
        if !pos_open {
            for &sym in cap_syms.iter() {
                let d = data.get(sym)?;
                if abs_bar >= d.close.len() { continue; }
                if turtle_entry(&d.close, EP, abs_bar) {
                    let entry_c = d.close[abs_bar];
                    pos_open = true;
                    pos_sym = sym;
                    pos_entry_bar = b as isize;
                    pos_entry_close = entry_c;

                    let l_entry = *d.low.get(abs_bar).unwrap_or(&0.0);
                    let atr = compute_atr(d, abs_bar);
                    pos_lowest_low = if atr > 0.0 { l_entry - mult * atr } else { l_entry };
                    break;
                }
            }
        }

        // Drawdown
        if equity > max_equity { max_equity = equity; }
        let dd = (max_equity - equity) / max_equity;
        if dd > max_dd { max_dd = dd; }
    }

    if trades < MIN_TRADES { return None; }

    let mean = daily_rets.iter().sum::<f64>() / daily_rets.len().max(1) as f64;
    let std = (daily_rets.iter().map(|r| (r - mean).powi(2)).sum::<f64>() / daily_rets.len().max(1) as f64).sqrt();
    let sharpe = if std > 1e-10 { mean / std * (252.0_f64.sqrt()) } else { 0.0 };

    Some((sharpe, equity - 1.0, max_dd, trades))
}

#[tokio::main]
async fn main() -> Result<()> {
    let start = Instant::now();
    println!("=== TURTLE_ATR_MULT Fine Sweep ===");
    println!("M in [{:.2}..{:.2}] step {:.2} -> {} values", SWEEP_START, SWEEP_END, SWEEP_STEP, N_VALUES);
    println!("Strategy: Turtle-only exit");
    println!("Params: EP={}, ATR_P={}, HM={}, CAP={}", EP, ATR_P, HOLD_MAX, POSITION_CAP);
    println!();

    // Load data (same pattern as turtle_chandelier_walkforward.rs)
    let loader = DataLoader::new(None, None);
    let mut all_syms: std::collections::HashSet<String> = std::collections::HashSet::new();
    for (_, syms) in UNIVERSES {
        for &s in *syms { all_syms.insert(s.to_string()); }
    }

    let mut raw_cache: HashMap<String, DataFrame> = HashMap::new();
    for sym in all_syms.iter() {
        match loader.fetch_with_cache(sym.as_str(), "1d", CANDLES).await {
            Ok(df) => { raw_cache.insert(sym.clone(), df); }
            Err(e) => { eprintln!("  WARNING: {} load failed: {}", sym, e); }
        }
    }

    // Build SymData with uniform length
    let n = 2800_usize;
    let mut sym_data_map: HashMap<String, SymData> = HashMap::new();
    macro_rules! col_vec {
        ($df:expr, $name:expr) => {{
            let chunked = $df.column($name)?.f64()?;
            chunked.into_iter().filter_map(|x| x).take(n).collect::<Vec<_>>()
        }};
    }
    for sym in &all_syms {
        if let Some(df) = raw_cache.get(sym) {
            sym_data_map.insert(sym.clone(), SymData {
                close: col_vec!(df, "close"),
                high:  col_vec!(df, "high"),
                low:   col_vec!(df, "low"),
                vol:   col_vec!(df, "volume"),
            });
        }
    }

    println!("Loaded {} symbols", sym_data_map.len());
    println!();

    // Build sweep values
    let mut sweep: Vec<f64> = Vec::with_capacity(N_VALUES);
    let mut v = SWEEP_START;
    while v <= SWEEP_END + 1e-9 {
        sweep.push((v * 100.0).round() / 100.0);
        v += SWEEP_STEP;
    }

    let n_unis = UNIVERSES.len();
    let windows_per_uni = 6;

    // Results: [mult_idx][uni_idx] = (pass, sharpe_sum, ret_sum, dd_max, trades_sum)
    let mut results: Vec<Vec<(usize, f64, f64, f64, usize)>> =
        vec![vec![(0, 0.0, 0.0, 0.0, 0); n_unis]; sweep.len()];

    for (u_idx, (uni_name, uni_syms)) in UNIVERSES.iter().enumerate() {
        print!("{}: ", uni_name);
        std::io::Write::flush(&mut std::io::stdout());

        let min_bars = uni_syms.iter()
            .filter_map(|s| sym_data_map.get(*s))
            .map(|d| d.close.len())
            .min()
            .unwrap_or(0);

        let n_wf = (min_bars / TRAIN_BARS).saturating_sub(1).max(1);

        for (m_idx, &mult) in sweep.iter().enumerate() {
            let mut pass_count = 0usize;
            let mut sharpe_sum = 0.0f64;
            let mut ret_sum = 0.0f64;
            let mut dd_max = 0.0f64;
            let mut trades_sum = 0usize;

            for w in 0..n_wf {
                let train_end = TRAIN_BARS * (w + 1);
                let test_start = train_end;
                let test_end = (train_end + TEST_BARS).min(min_bars);

                if let Some((sh, rt, dd, tr)) = run_universe_window(&sym_data_map, uni_syms, mult, test_start, test_end) {
                    if sh > 0.0 { pass_count += 1; }
                    sharpe_sum += sh;
                    ret_sum += rt;
                    dd_max = dd_max.max(dd);
                    trades_sum += tr;
                }
            }
            results[m_idx][u_idx] = (pass_count, sharpe_sum, ret_sum, dd_max, trades_sum);
            print!(".");
            std::io::Write::flush(&mut std::io::stdout());
        }
        println!();
    }

    // Global aggregation
    let mut global_pass = vec![0usize; sweep.len()];
    let mut global_sharpe = vec![0.0f64; sweep.len()];
    let mut global_ret = vec![0.0f64; sweep.len()];
    let mut global_dd = vec![0.0f64; sweep.len()];
    let mut global_trades = vec![0usize; sweep.len()];

    for m_idx in 0..sweep.len() {
        for u_idx in 0..n_unis {
            let (p, sh, rt, dd, tr) = results[m_idx][u_idx];
            global_pass[m_idx] += p;
            global_sharpe[m_idx] += sh;
            global_ret[m_idx] += rt;
            global_dd[m_idx] = global_dd[m_idx].max(dd);
            global_trades[m_idx] += tr;
        }
    }

    // Print table
    println!("\n{:<6} {:>6} {:>8} {:>9} {:>8}", "M", "Pass", "AvgSharpe", "AvgRet%", "WorstDD%");
    println!("{}", "-".repeat(36));

    let mut best_idx = 0usize;
    let mut best_pass = 0usize;
    let mut best_sh = -999.0f64;

    for i in 0..sweep.len() {
        let pass = global_pass[i];
        let avg_sh = global_sharpe[i] / n_unis as f64;
        let avg_rt = global_ret[i] / n_unis as f64 * 100.0;
        let max_dd = global_dd[i] * 100.0;
        println!("{:.2}   {:>4}/{} {:>8.3} {:>9.1}% {:>8.1}%",
                 sweep[i], pass, n_unis * windows_per_uni, avg_sh, avg_rt, max_dd);
        if pass > best_pass || (pass == best_pass && avg_sh > best_sh) {
            best_idx = i;
            best_pass = pass;
            best_sh = avg_sh;
        }
    }

    let winner = sweep[best_idx];
    println!("\n=== WINNER: M={:.2} (pass={}/{}, Sharpe={:.3}) ===", winner, best_pass, n_unis * windows_per_uni, best_sh);

    // Write CSV
    let mut csv = File::create(CSV_OUT)?;
    csv.write_all(b"mult,pass,avg_sharpe,avg_return_pct,worst_dd_pct,total_trades\n")?;
    for i in 0..sweep.len() {
        csv.write_all(format!("{:.2},{},{:.4},{:.2},{:.2},{}\n",
            sweep[i], global_pass[i],
            global_sharpe[i] / n_unis as f64,
            global_ret[i] / n_unis as f64 * 100.0,
            global_dd[i] * 100.0,
            global_trades[i]
        ).as_bytes())?;
    }

    let mut csv2 = File::create(DETAIL_OUT)?;
    csv2.write_all(b"mult,universe,pases,avg_sharpe,return_pct,dd_pct,trades\n")?;
    for i in 0..sweep.len() {
        for (u_idx, (uni_name, _)) in UNIVERSES.iter().enumerate() {
            let (p, sh, rt, dd, tr) = results[i][u_idx];
            csv2.write_all(format!("{:.2},{},{},{:.4},{:.2},{:.2},{}\n",
                sweep[i], uni_name, p, sh, rt * 100.0, dd * 100.0, tr
            ).as_bytes())?;
        }
    }

    println!("\nWrote: {} ({} rows)", CSV_OUT, sweep.len());
    println!("Wrote: {} ({} rows)", DETAIL_OUT, sweep.len() * n_unis);
    println!("Runtime: {:.1}s", start.elapsed().as_secs_f64());

    Ok(())
}
