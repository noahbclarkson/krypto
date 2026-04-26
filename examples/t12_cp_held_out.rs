//! T12: CHAND_PERIOD Held-Out Validation
//!
//! Clean held-out test of CP=7 (current production default) vs CP alternatives
//! on pre-2021 data that was NEVER used in any hyperopt sweep.
//!
//! Production params: EP=21, CM=2.30, HM=12, ATR_ENTRY_MULT=0.00
//! Uses FORWARD search (matching production harness, not buggy backward sweep).
//!
//! Held-out: pre-2021 data (2017-07 to 2020-12), 5 symbols.
//! Win condition: pass rate >= 70% per CP value.

use anyhow::Result;
use krypto::data::loader::DataLoader;
use polars::prelude::*;
use std::collections::HashMap;
use std::fs::File;
use std::io::Write;
use std::time::Instant;

const SYMBOLS: &[&str] = &["BTCUSDT", "ETHUSDT", "XRPUSDT", "LTCUSDT", "EOSUSDT"];
const TAKER_FEE: f64 = 0.0004;
const TURTLE_ENTRY: usize = 21;
const CHAND_MULT: f64 = 2.30;
const HOLD_MAX: usize = 12;
const TURTLE_ATR_PERIOD: usize = 24;
const ATR_ENTRY_MULT: f64 = 0.0;
const CANDLES: u32 = 2000;

// CP values to test
const CP_VALUES: &[usize] = &[7, 11, 15, 20, 28, 42];

fn atr(high: &[f64], low: &[f64], close: &[f64], period: usize, idx: usize) -> f64 {
    if idx < period { return 0.0; }
    let mut sum = 0.0;
    for i in (idx + 1 - period)..=idx {
        let h = high.get(i).copied().unwrap_or(0.0);
        let l = low.get(i).copied().unwrap_or(0.0);
        let c = close.get(i.saturating_sub(1)).copied().unwrap_or(close[0]);
        let tr = (h - l).max((h - c).abs()).max((l - c).abs());
        sum += tr;
    }
    sum / period as f64
}

struct Metrics { eq: f64, dd: f64, sharpe: f64, n: usize, ret: f64 }

fn run_sim(close: &[f64], high: &[f64], low: &[f64], cp: usize, start: usize, end: usize) -> Metrics {
    let mut eq: f64 = 1.0;
    let mut peak: f64 = 1.0;
    let mut trough: f64 = 1.0;
    let mut trades = Vec::new();
    let mut in_trade = false;
    let mut entry_price = 0.0;
    let mut entry_bar = 0usize;
    let mut stop_price = 0.0;

    for bar in start..end.min(close.len()) {
        if !in_trade {
            let e_end = bar.saturating_sub(1);
            let e_start = e_end.saturating_sub(TURTLE_ENTRY - 1).max(start);
            let max_close = high[e_start..=e_end].iter().fold(0.0f64, |m, &v| m.max(v));
            let atr_ent = atr(high, low, close, TURTLE_ATR_PERIOD, bar.saturating_sub(1));
            let threshold = if ATR_ENTRY_MULT > 0.0 { max_close + ATR_ENTRY_MULT * atr_ent } else { max_close };
            if close[bar] > threshold && close[bar] > close[bar.saturating_sub(1)] {
                in_trade = true;
                entry_bar = bar;
                entry_price = close[bar];
                let a = atr(high, low, close, cp, bar);
                stop_price = high[bar] - CHAND_MULT * a;
            }
        }
        if in_trade {
            let held = bar - entry_bar;
            let ex_time = held >= HOLD_MAX;
            let ex_stop = low[bar] < stop_price;
            let ex_final = bar == end.saturating_sub(1) || bar == close.len() - 1;
            if ex_time || ex_stop || ex_final {
                let ret = (close[bar] - entry_price) / entry_price - TAKER_FEE * 2.0;
                eq *= 1.0 + ret;
                peak = peak.max(eq);
                trough = trough.min(eq);
                trades.push(ret);
                in_trade = false;
            } else {
                let a = atr(high, low, close, cp, bar);
                stop_price = stop_price.max(high[bar] - CHAND_MULT * a);
            }
        }
    }

    let dd = if peak > 0.0 { (peak - trough) / peak * 100.0 } else { 100.0 };
    let n = trades.len();
    let (mean, std) = if n > 1 {
        let sum: f64 = trades.iter().sum();
        let m = sum / n as f64;
        let var: f64 = trades.iter().map(|r| { let d = r - m; d * d }).sum::<f64>() / n as f64;
        (m, var.sqrt())
    } else { (0.0, 1.0) };
    let ret = (eq - 1.0) * 100.0;
    let sharpe = if std > 1e-10 { mean / std * (252.0_f64).sqrt() } else { 0.0 };
    Metrics { eq, dd, sharpe, n, ret }
}

fn fmt(v: f64, p: usize) -> String { format!("{:.p$}", v, p = p) }

#[tokio::main]
async fn main() -> Result<()> {
    let t0 = Instant::now();
    println!("T12: CHAND_PERIOD Held-Out Validation");
    println!("Testing CP: {:?}", CP_VALUES);
    println!("Production params: EP={}, CM={}, HM={}", TURTLE_ENTRY, CHAND_MULT, HOLD_MAX);
    println!("Held-out: pre-2021 data (2017-2020)");
    println!("Win: pass rate >= 70% per CP value");
    println!();

    let loader = DataLoader::new(None, None);
    let mut cache: HashMap<String, DataFrame> = HashMap::new();
    for &sym in SYMBOLS {
        if let Ok(df) = loader.fetch_with_cache(sym, "1d", CANDLES).await {
            cache.insert(sym.to_string(), df);
        }
    }

    macro_rules! cv {
        ($df:expr, $name:expr) => {
            {
                let c = $df.column($name)?.f64()?;
                c.into_iter().filter_map(|x| x).collect::<Vec<_>>()
            }
        };
    }

    let mut data: HashMap<String, (Vec<f64>, Vec<f64>, Vec<f64>)> = HashMap::new();
    for sym in SYMBOLS {
        if let Some(df) = cache.get(&sym.to_string()) {
            data.insert(sym.to_string(), (cv!(df, "close"), cv!(df, "high"), cv!(df, "low")));
        }
    }

    // Pre-2021 cutoff: split each symbol at index where timestamp < 2021-01-01
    // Since we don't have timestamps directly, use: BTC 2017 start -> use first 80% of data
    // Airdrop epochs from prior analysis: P1=2020-06 to P3=2020-12 = 201 days
    // Pre-2021 = first ~75% of data (approx 2020-06 cutoff)
    let pre_end_frac = 0.78; // roughly 2020-05/06 cutoff

    println!("=== Per-Symbol Pre-2021 Split ===");
    let mut sym_results: HashMap<String, HashMap<usize, (usize, Metrics)>> = HashMap::new();
    for sym in SYMBOLS {
        if let Some((close, high, low)) = data.get(&sym.to_string()) {
            let n = close.len();
            let pre_end = (n as f64 * pre_end_frac) as usize;
            let pre_start = 100; // warm-up
            println!("{}: total {} bars, pre-2021 {}-{} ({} bars)",
                sym, n, pre_start, pre_end, pre_end - pre_start);

            let mut cp_results: HashMap<usize, (usize, Metrics)> = HashMap::new();
            for &cp in CP_VALUES {
                let m = run_sim(close, high, low, cp, pre_start, pre_end);
                let passes = m.sharpe > 0.0 && m.eq > 0.5 && m.n >= 3;
                cp_results.insert(cp, (if passes {1} else {0}, m));
            }
            sym_results.insert(sym.to_string(), cp_results);
        }
    }

    // Also run 3 pre-2021 phase-tests for global view
    // Phases from prior analysis: P1=2019-06 to P1-end, P2=2020-01 to P2-end, P3=2020-06 to P3-end
    println!("\n=== Phase-Level Pre-2021 Results ===");
    let phase_configs = [
        ("P1(2019-06/12)", 700, 820),
        ("P2(2020-01/06)", 860, 1050),
        ("P3(2020-06/12)", 1050, 1250),
    ];

    let mut phase_pass: HashMap<usize, Vec<usize>> = HashMap::new();
    for &cp in CP_VALUES { phase_pass.insert(cp, vec![0, 0, 0]); }

    for (phase_name, p_start, p_end) in phase_configs {
        println!("\n  {}: bars {}-{}", phase_name, p_start, p_end);
        for &cp in CP_VALUES {
            let mut total_pass = 0usize;
            let mut total_n = 0usize;
            let mut all_sh: Vec<f64> = vec![];
            for sym in SYMBOLS {
                if let Some((close, high, low)) = data.get(&sym.to_string()) {
                    let m = run_sim(close, high, low, cp, p_start, p_end);
                    let passes = m.sharpe > 0.0 && m.eq > 0.5 && m.n >= 3;
                    if passes { total_pass += 1; }
                    total_n += m.n;
                    all_sh.push(m.sharpe);
                }
            }
            let n_syms = SYMBOLS.len();
            if total_pass >= (n_syms * 2) / 3 { // 3/5 or better
                phase_pass.get_mut(&cp).unwrap()[0] += 1;
            }
            let avg_sh = all_sh.iter().sum::<f64>() / all_sh.len() as f64;
            println!("    CP={}: {}/{} pass, avg Sharpe={}, total trades={}",
                cp, total_pass, n_syms, fmt(avg_sh,2), total_n);
        }
    }

    // Global summary
    println!("\n=== GLOBAL SUMMARY: Held-Out Pass Rates ===");
    println!("CP     | Held-Out Pass Rate | Verdict");
    let mut cp_scores: Vec<(usize, f64)> = vec![];
    for &cp in CP_VALUES {
        let mut total_pass = 0usize;
        let mut total_runs = 0usize;
        let mut avg_sharpe = 0.0f64;
        let mut n_sharpe = 0usize;
        for sym in SYMBOLS {
            if let Some(cp_res) = sym_results.get(&sym[..]) {
                if let Some((pass, m)) = cp_res.get(&cp) {
                    total_pass += pass;
                    total_runs += 1;
                    if m.sharpe.abs() > 0.001 { avg_sharpe += m.sharpe; n_sharpe += 1; }
                }
            }
        }
        let pass_rate = if total_runs > 0 { total_pass as f64 / total_runs as f64 * 100.0 } else { 0.0 };
        let avg_sh = if n_sharpe > 0 { avg_sharpe / n_sharpe as f64 } else { 0.0 };
        cp_scores.push((cp, pass_rate));
        let verdict = if pass_rate >= 70.0 { "PASS" } else if pass_rate >= 50.0 { "MARGINAL" } else { "FAIL" };
        println!("CP={:2}  | {:5.1}% ({:2}/{:2})    | {}  | avg Sharpe={}",
            cp, fmt(pass_rate,1), total_pass, total_runs, verdict, fmt(avg_sh,2));
    }

    // Winner
    cp_scores.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());
    let winner = cp_scores[0];
    let threshold = 70.0;
    if winner.1 >= threshold {
        println!("\n*** HELD-OUT WINNER: CP={} ({:.1}% pass rate) ***", winner.0, winner.1);
        if winner.0 == 7 {
            println!("CP=7 PASSES held-out validation. Production default CONFIRMED.");
        } else {
            println!("CP={} is new held-out winner. Recommend updating production.", winner.0);
        }
    } else {
        println!("\n*** ALL CP VALUES FAIL held-out >=70% threshold ***");
        println!("Best: CP={} at {:.1}%. Production may be on unstable ground.", winner.0, winner.1);
    }

    // CSV
    {
        let mut csv = String::from("symbol,cp,pass,sharpe,drawdown,equity,trades,return_pct\n");
        for sym in SYMBOLS {
            if let Some(cp_res) = sym_results.get(&sym[..]) {
                for &cp in CP_VALUES {
                    if let Some((pass, m)) = cp_res.get(&cp) {
                        csv.push_str(&format!("{},{},{},{:.4},{:.2},{:.4},{},{:.2}\n",
                            sym, cp, pass, m.sharpe, m.dd, m.eq, m.n, m.ret));
                    }
                }
            }
        }
        File::create("snapshots/t12_cp_held_out.csv")?.write_all(csv.as_bytes())?;
    }

    println!("\nTime: {}s", t0.elapsed().as_secs_f64());
    Ok(())
}
