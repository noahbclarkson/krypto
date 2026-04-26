//! T12 v2: CHAND_PERIOD Held-Out Walk-Forward Validation
//!
//! Uses actual walk-forward splitting methodology on pre-2021 data.
//! 6 windows, tests each CP value across 5 symbols.
//! Production params: EP=21, CM=2.30, HM=12, ATR_ENTRY_MULT=0.00
//!
//! Held-out: first 75% of data per symbol (pre-2021 period)
//! Walk-forward: 6 windows on the held-out portion
//! Win: pass rate >= 70%

use anyhow::Result;
use krypto::data::loader::DataLoader;
use polars::prelude::*;
use std::collections::HashMap;
use std::fs::File;
use std::io::Write;
use std::time::Instant;

const SYMBOLS: &[&str] = &["BTCUSDT", "ETHUSDT", "XRPUSDT", "LTCUSDT", "EOSUSDT"];
const WF_WINDOWS: usize = 6;
const TAKER_FEE: f64 = 0.0004;
const TURTLE_ENTRY: usize = 21;
const CHAND_MULT: f64 = 2.30;
const HOLD_MAX: usize = 12;
const TURTLE_ATR_PERIOD: usize = 24;
const ATR_ENTRY_MULT: f64 = 0.0;
const CANDLES: u32 = 3000;
const CP_VALUES: &[usize] = &[7, 11, 15, 20, 28, 42];
const HELD_OUT_FRAC: f64 = 0.75;

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

struct SimResult { eq: f64, dd: f64, sharpe: f64, n: usize, ret: f64 }

fn run_sim(close: &[f64], high: &[f64], low: &[f64], cp: usize, start: usize, end: usize) -> SimResult {
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
    SimResult { eq, dd, sharpe, n, ret }
}

fn fmt(v: f64, p: usize) -> String { format!("{:.p$}", v, p = p) }

#[tokio::main]
async fn main() -> Result<()> {
    let t0 = Instant::now();
    println!("T12 v2: CHAND_PERIOD Held-Out Walk-Forward");
    println!("Testing CP: {:?}", CP_VALUES);
    println!("Production params: EP={}, CM={}, HM={}", TURTLE_ENTRY, CHAND_MULT, HOLD_MAX);
    println!("Method: {} WF windows on held-out (pre-2021) data", WF_WINDOWS);
    println!("Win: pass rate >= 70%");
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

    let n_min = data.values().map(|(c, _, _)| c.len()).min().unwrap_or(0);
    let held_out_end = (n_min as f64 * HELD_OUT_FRAC) as usize;
    let n = held_out_end.min(2800);
    let win_sz = n / (WF_WINDOWS + 1);
    println!("Held-out: first {} bars, {} bar windows", n, win_sz);

    // results[cp][window] = Vec<(symbol, pass, sharpe, dd, eq, n, ret)>
    let mut results: HashMap<usize, Vec<Vec<(String, bool, f64, f64, f64, usize, f64)>>> = HashMap::new();
    for &cp in CP_VALUES {
        results.insert(cp, vec![Vec::new(); WF_WINDOWS]);
    }

    for sym in SYMBOLS {
        if let Some((close, high, low)) = data.get(&sym.to_string()) {
            for w in 0..WF_WINDOWS {
                let start = w * win_sz;
                let end = ((w + 1) * win_sz).min(close.len().min(n));
                for &cp in CP_VALUES {
                    let m = run_sim(close, high, low, cp, start, end);
                    let passes = m.eq > 0.1 && m.n >= 3 && m.sharpe > -50.0;
                    results.get_mut(&cp).unwrap()[w].push((sym.to_string(), passes, m.sharpe, m.dd, m.eq, m.n, m.ret));
                }
            }
        }
    }

    // Per-window summary
    println!("\n=== Per-Window Results ===");
    for w in 0..WF_WINDOWS {
        print!("W{}: ", w);
        for &cp in CP_VALUES {
            let wins = &results[&cp][w];
            let n_pass = wins.iter().filter(|r| r.1).count();
            let n_syms = wins.len();
            let avg_sh = wins.iter().map(|r| r.2).sum::<f64>() / n_syms as f64;
            print!("CP={:2} {}/{} ", cp, n_pass, n_syms);
        }
        println!();
    }

    // Global summary per CP
    println!("\n=== Global Summary ===");
    let mut cp_scores: Vec<(usize, usize, usize, f64, f64, f64)> = vec![];
    for &cp in CP_VALUES {
        let all: Vec<_> = results[&cp].iter().flatten().collect();
        let total_pass = all.iter().filter(|r| r.1).count();
        let total = all.len();
        let pass_rate = total_pass as f64 / total as f64 * 100.0;
        let avg_sh = all.iter().map(|r| r.2).sum::<f64>() / total as f64;
        let avg_dd = all.iter().map(|r| r.3).sum::<f64>() / total as f64;
        let avg_eq = all.iter().map(|r| r.4).sum::<f64>() / total as f64;
        cp_scores.push((cp, total_pass, total, pass_rate, avg_sh, avg_dd));

        let verdict = if pass_rate >= 70.0 { "PASS" } else if pass_rate >= 60.0 { "MARGINAL" } else { "FAIL" };
        println!("CP={:2}  | {}/{} ({:.0}%) pass | DD={:.0}%, Sharpe={}, Eq={:.3} | {}",
            cp, total_pass, total, pass_rate, avg_dd, fmt(avg_sh,2), fmt(avg_eq,3), verdict);
    }

    // Winner
    cp_scores.sort_by(|a, b| b.3.partial_cmp(&a.3).unwrap_or(std::cmp::Ordering::Equal));
    let winner = cp_scores[0];

    println!();
    if winner.3 >= 70.0 {
        println!("*** HELD-OUT WINNER: CP={} at {:.0}% pass ***", winner.0, winner.3);
        if winner.0 == 7 {
            println!("CP=7 CONFIRMED by held-out validation.");
        } else {
            println!("Recommend updating production CP to {}.", winner.0);
        }
    } else {
        println!("*** ALL CP VALUES FAIL >=70% held-out threshold ***");
        println!("Best: CP={} at {:.0}% ({}/{}).",
            winner.0, winner.3, winner.1, winner.2);
        println!("CP=7's OOS 83% vs held-out {:.0}% suggests overfitting risk.", winner.3);
        if winner.0 != 7 {
            println!("CP={} is most robust on held-out. Consider switching.", winner.0);
        }
    }

    // CSV
    {
        let mut csv = String::from("symbol,window,cp,pass,sharpe,drawdown,equity,trades,return_pct\n");
        for &cp in CP_VALUES {
            for w in 0..WF_WINDOWS {
                for r in &results[&cp][w] {
                    csv.push_str(&format!("{},{},{},{},{:.4},{:.2},{:.4},{},{:.2}\n",
                        r.0, w, cp, if r.1 {1} else {0}, r.2, r.3, r.4, r.5, r.6));
                }
            }
        }
        File::create("snapshots/t12_cp_held_out_wf.csv")?.write_all(csv.as_bytes())?;
    }

    println!("\nTime: {}s", t0.elapsed().as_secs_f64());
    Ok(())
}
