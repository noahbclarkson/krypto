//! CHAND_PERIOD Fine Sweep — Step 1 around P=7 region
//!
//! PRIOR: coarse sweep CP∈[5..60 step 2] × 9u×54w with EP=24/CHAND_M=2.25/HM=12 → P=7 winner
//! CURRENT params: EP=21, CHAND_M=2.30, ATR_P=24, ATR_M=2.0, HM=12, CAP=3, ATR_EM=0.00, VL=9
//! GAP: P=8 never tested. P=7 was optimal with stale params — may shift with CM=2.30 vs 2.25
//! 
//! Range: P ∈ [5..15] step 1 (11 values) — focused fine grid around the known winner
//! Harness: walk-forward 252/252, dual exit (Chandelier OR Turtle ATR fires first)
//! Metrics: pass rate, avg Sharpe, avg return, avg DD, trade count per P value

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
// Current production params
const TURTLE_ENTRY: usize = 21;
const CHAND_MULT: f64 = 2.30;
const TURTLE_ATR_PERIOD: usize = 24;
const TURTLE_ATR_MULT: f64 = 2.00;
const ATR_ENTRY_MULT: f64 = 0.00;
const VOL_LOOKBACK: usize = 8;
const BASELINE_P: usize = 7;

// Fine sweep: P ∈ [5..15] step 1 (11 values)
const CHAND_PERIODS: &[usize] = &[5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15];

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

const SUMMARY_CSV: &str = "snapshots/chand_period_fine_summary.csv";
const DETAIL_CSV: &str = "snapshots/chand_period_fine_detail.csv";
const EQUITY_CSV: &str = "snapshots/chand_period_fine_equity.csv";
const JSON_OUT: &str = "snapshots/chand_period_fine_summary.json";

struct SymData {
    close: Vec<f64>,
    high: Vec<f64>,
    low: Vec<f64>,
    vol: Vec<f64>,
}

fn atr_at(high: &[f64], low: &[f64], close: &[f64], period: usize, idx: usize) -> f64 {
    if idx < period { return 0.0; }
    let mut trs = Vec::with_capacity(period);
    for i in (idx + 1 - period)..=idx {
        let h = high.get(i).copied().unwrap_or(0.0);
        let l = low.get(i).copied().unwrap_or(0.0);
        let c0 = close.get(i.saturating_sub(1)).copied().unwrap_or(0.0);
        trs.push((h - l).max((h - c0).abs()).max((l - c0).abs()));
    }
    if trs.is_empty() { return 0.0; }
    trs.iter().sum::<f64>() / period as f64
}

fn rolling_avg(vals: &[f64], window: usize, idx: usize) -> f64 {
    if idx < window { return *vals.get(idx).unwrap_or(&0.0); }
    let start = idx + 1 - window;
    vals[start..=idx].iter().sum::<f64>() / window as f64
}

fn turtle_signal(close: &[f64], high: &[f64], low: &[f64], entry_period: usize, atr_period: usize, atr_mult: f64, idx: usize) -> bool {
    if idx < entry_period + 1 { return false; }
    let start = idx + 1 - entry_period;
    let mut max_close = f64::NEG_INFINITY;
    for i in start..idx {
        if let Some(&c) = close.get(i) { max_close = max_close.max(c); }
    }
    if let Some(&curr_close) = close.get(idx) {
        let breakout = curr_close > max_close;
        if breakout && atr_mult > 0.0 {
            let atr_val = atr_at(high, low, close, atr_period, idx);
            return curr_close >= max_close + atr_mult * atr_val;
        }
        breakout
    } else {
        false
    }
}

fn annualised_sharpe(daily_rets: &[f64]) -> f64 {
    if daily_rets.len() < 2 { return 0.0; }
    let mn: f64 = daily_rets.iter().sum::<f64>() / daily_rets.len() as f64;
    let sd = (daily_rets.iter().map(|x| (x - mn).powi(2)).sum::<f64>() / daily_rets.len() as f64).sqrt();
    if sd == 0.0 { return 0.0; }
    mn * 365.0_f64.sqrt() / sd
}

fn max_dd_from(equity: &[f64]) -> f64 {
    let mut peak = f64::NEG_INFINITY;
    let mut max_dd = 0.0_f64;
    for &e in equity {
        if e > peak { peak = e; }
        let dd = (peak - e) / peak;
        if dd > max_dd { max_dd = dd; }
    }
    max_dd * 100.0
}

struct WfResult {
    ret: f64,
    sharpe: f64,
    max_dd: f64,
    trades: usize,
    win_rate: f64,
    pass: bool,
}

fn run_sim(
    sym_data: &HashMap<String, SymData>,
    symbols: &[String],
    test_start: usize,
    test_end: usize,
    chand_period: usize,
) -> (WfResult, Vec<f64>) {
    let mut equity = 1.0_f64;
    let mut equity_curve = vec![1.0_f64];
    let mut peak = equity;
    let mut wins = 0usize;
    let mut total_trades = 0usize;
    let mut daily_rets = Vec::new();

    let mut bar = test_start;
    while bar + 2 < test_end {
        let mut scores: Vec<(&str, f64)> = Vec::new();
        for sym in symbols {
            if let Some(sd) = sym_data.get(sym) {
                if bar >= sd.close.len() { continue; }
                let rol_vol = rolling_avg(&sd.vol, VOL_LOOKBACK, bar);
                let price = sd.close.get(bar).copied().unwrap_or(0.0);
                let dv = rol_vol * price;
                scores.push((sym.as_str(), if dv.is_finite() && dv > 0.0 { dv } else { 0.0 }));
            }
        }
        scores.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());
        let top_syms: Vec<String> = scores.into_iter().take(POSITION_CAP).map(|(s, _)| s.to_string()).collect();

        if top_syms.is_empty() {
            equity_curve.push(equity);
            bar += 1;
            continue;
        }

        let mut entered = false;
        for sym in &top_syms {
            if let Some(sd) = sym_data.get(sym) {
                if bar >= TURTLE_ENTRY + 1 && bar < sd.close.len() {
                    if turtle_signal(&sd.close, &sd.high, &sd.low, TURTLE_ENTRY, TURTLE_ATR_PERIOD, ATR_ENTRY_MULT, bar) {
                        let entry_px = sd.close[bar];
                        let entry = entry_px * (1.0 - TAKER_FEE);
                        let entry_bar_next = bar + 1;
                        let n = sd.close.len();

                        let mut highest_high_chand = sd.high[entry_bar_next];
                        let mut lowest_low_turtle = sd.low[entry_bar_next];
                        let max_bar = (entry_bar_next + HOLD_MAX).min(n.saturating_sub(1));
                        let mut exit_bar = max_bar;
                        for b in entry_bar_next..=max_bar.min(n.saturating_sub(1)) {
                            highest_high_chand = highest_high_chand.max(sd.high[b]);
                            let atr_chand = atr_at(&sd.high, &sd.low, &sd.close, chand_period, b);
                            let trail_chand = highest_high_chand - CHAND_MULT * atr_chand;
                            lowest_low_turtle = lowest_low_turtle.min(sd.low[b]);
                            let atr_turtle = atr_at(&sd.high, &sd.low, &sd.close, TURTLE_ATR_PERIOD, b);
                            let trail_turtle = lowest_low_turtle - TURTLE_ATR_MULT * atr_turtle;
                            if sd.close[b] < trail_chand || sd.close[b] < trail_turtle {
                                exit_bar = b;
                                break;
                            }
                        }

                        if let Some(&exit_px) = sd.close.get(exit_bar) {
                            let exit = exit_px * (1.0 - TAKER_FEE);
                            let gross_ret = exit / entry - 1.0;
                            let bars_held = (exit_bar as i64 - entry_bar_next as i64).max(1) as usize;
                            wins += if gross_ret > 0.0 { 1 } else { 0 };
                            total_trades += 1;
                            equity *= 1.0 + gross_ret;
                            let avg_daily = gross_ret / bars_held as f64;
                            for _ in 0..bars_held {
                                daily_rets.push(avg_daily);
                            }
                            if equity > peak { peak = equity; }
                            equity_curve.push(equity);
                            bar = exit_bar + 1;
                            entered = true;
                            break;
                        }
                    }
                }
            }
        }

        if !entered {
            equity_curve.push(equity);
            bar += 1;
        }
    }

    let ret = (equity - 1.0) * 100.0;
    let sharpe = annualised_sharpe(&daily_rets);
    let max_dd = max_dd_from(&equity_curve);
    let pass = total_trades >= MIN_TRADES && ret > 0.0;
    let win_rate = if total_trades > 0 { wins as f64 / total_trades as f64 } else { 0.0 };

    (WfResult { ret, sharpe, max_dd, trades: total_trades, win_rate, pass }, equity_curve)
}

#[tokio::main]
async fn main() -> Result<()> {
    let start = Instant::now();
    let loader = DataLoader::new(None, None);
    let mut data: HashMap<String, SymData> = HashMap::new();

    let mut all_syms = std::collections::HashSet::new();
    for (_, syms) in UNIVERSES {
        for s in *syms { all_syms.insert(s.to_string()); }
    }

    for sym in all_syms {
        match loader.fetch_with_cache(&sym, "1d", CANDLES).await {
            Ok(df) => {
                let close: Vec<f64> = df.column("close")?.f64()?.into_no_null_iter().collect();
                let high: Vec<f64> = df.column("high")?.f64()?.into_no_null_iter().collect();
                let low: Vec<f64> = df.column("low")?.f64()?.into_no_null_iter().collect();
                let vol: Vec<f64> = df.column("volume")?.f64()?.into_no_null_iter().collect();
                data.insert(sym, SymData { close, high, low, vol });
            }
            Err(e) => { eprintln!("WARNING: {} load failed: {}", sym, e); }
        }
    }

    let min_len = UNIVERSES.iter().filter_map(|(_, s)| {
        data.get(s[0]).map(|d| d.close.len())
    }).min().unwrap_or(0);

    let n_windows = (min_len.saturating_sub(TRAIN_BARS)) / TEST_BARS;
    println!("CHAND_PERIOD Fine Sweep: P ∈ [5..15] step 1, {} values", CHAND_PERIODS.len());
    println!("Bars: {}, Windows: {}, Universes: {}", min_len, n_windows, UNIVERSES.len());
    println!("Total runs: {} × {} × {} = {}",
        CHAND_PERIODS.len(), UNIVERSES.len(), n_windows,
        CHAND_PERIODS.len() * UNIVERSES.len() * n_windows);

    let mut summary_csv = File::create(SUMMARY_CSV)?;
    writeln!(summary_csv, "chand_period,universe,pass_windows,total_windows,pass_rate,avg_sharpe,avg_ret,avg_dd,total_trades")?;

    let mut detail_csv = File::create(DETAIL_CSV)?;
    writeln!(detail_csv, "chand_period,universe,window,ret,sharpe,trades,max_dd,win_rate,pass")?;

    let mut equity_csv = File::create(EQUITY_CSV)?;
    writeln!(equity_csv, "day,chand_period,universe,equity")?;

    // Per-universe, per-P equity accumulation for charting
    let mut equity_by_p: HashMap<usize, HashMap<String, Vec<f64>>> = HashMap::new();

    let mut global: HashMap<usize, (usize, usize, f64, f64, f64, usize)> = HashMap::new();

    for cp in CHAND_PERIODS {
        equity_by_p.insert(*cp, HashMap::new());
    }

    for (uname, symbols) in UNIVERSES {
        for cp in CHAND_PERIODS {
            let mut total_pass = 0usize;
            let mut total_ret = 0.0f64;
            let mut total_sharpe = 0.0f64;
            let mut total_dd = 0.0f64;
            let mut total_trades = 0usize;
            let mut n = 0usize;
            let mut agg_equity: Vec<f64> = vec![1.0_f64];

            for w in 0..n_windows {
                let train_end = TRAIN_BARS + w * TEST_BARS;
                let test_end = (train_end + TEST_BARS).min(min_len);
                if test_end <= train_end + 30 { continue; }

                let syms: Vec<String> = symbols.iter().map(|s| s.to_string()).collect();
                let (result, eq_curve) = run_sim(&data, &syms, train_end, test_end, *cp);

                total_pass += result.pass as usize;
                total_ret += result.ret;
                total_sharpe += result.sharpe;
                total_dd += result.max_dd;
                total_trades += result.trades;
                n += 1;

                writeln!(detail_csv, "{},{},{},{:.2},{:.3},{},{:.2},{:.3},{}",
                    cp, uname, w, result.ret, result.sharpe, result.trades, result.max_dd, result.win_rate, result.pass)?;

                // Compound equity across windows
                let start_eq = *agg_equity.last().unwrap();
                for &e in &eq_curve {
                    agg_equity.push(start_eq * e);
                }
            }

            if n > 0 {
                let avg_sharpe = total_sharpe / n as f64;
                let avg_ret = total_ret / n as f64;
                let avg_dd = total_dd / n as f64;
                let pass_rate = total_pass as f64 / n as f64 * 100.0;
                writeln!(summary_csv, "{},{},{},{},{:.1},{:.3},{:.1},{:.1},{}",
                    cp, uname, total_pass, n, pass_rate, avg_sharpe, avg_ret, avg_dd, total_trades)?;

                global.entry(*cp).or_insert_with(|| (0, 0, 0.0, 0.0, 0.0, 0));
                if let Some(g) = global.get_mut(cp) {
                    g.0 += total_pass;
                    g.1 += n;
                    g.2 += avg_ret;
                    g.3 += avg_sharpe;
                    g.4 += avg_dd;
                    g.5 += total_trades;
                }

                // Write equity curve
                if let Some(uni_map) = equity_by_p.get_mut(cp) {
                    for (day_idx, &eq) in agg_equity.iter().enumerate() {
                        writeln!(equity_csv, "{},{},{},{:.6}", day_idx, cp, uname, eq)?;
                    }
                    uni_map.insert(uname.to_string(), agg_equity);
                }
            }
        }
    }

    // Print global summary sorted by P
    let mut sorted: Vec<_> = global.iter().collect();
    sorted.sort_by_key(|(p, _)| *p);

    println!("\n=== Global Results (P ∈ [5..15] step 1) ===");
    println!("{:>4} {:>8} {:>10} {:>10} {:>8} {:>8}",
        "P", "PassRate", "AvgSharpe", "AvgRet%", "AvgDD%", "Trades");
    println!("{}", "-".repeat(54));

    let baseline_sharpe = {
        let mut bs = 0.0_f64;
        for (p, (_, _, _, sh, _, _)) in &sorted {
            if **p == BASELINE_P { bs = *sh; }
        }
        bs
    };

    let mut winner_p = BASELINE_P;
    let mut winner_sharpe = baseline_sharpe;
    let mut winner_pass_rate = 0.0f64;

    for (p, (pass, total, _, sh, _, trades)) in &sorted {
        let pr = *pass as f64 / *total as f64 * 100.0;
        let delta = if baseline_sharpe > 0.0 { (*sh - baseline_sharpe) / baseline_sharpe * 100.0 } else { 0.0 };
        let marker = if **p == BASELINE_P { " [BASE]" } else { "" };
        println!("{:4}{} {:7.1}% {:10.3} {:8}",
            p, marker, pr, sh, trades);
        
        // Winner selection: pass rate first, then Sharpe
        let pr_winner = *pass as f64 / *total as f64 * 100.0;
        if pr_winner > winner_pass_rate || (pr_winner == winner_pass_rate && *sh > winner_sharpe) {
            winner_p = **p;
            winner_sharpe = *sh;
            winner_pass_rate = pr_winner;
        }
    }

    println!("\n=== Winner: P={} (Sharpe {:.3}, pass {:.1}%) ===", winner_p, winner_sharpe, winner_pass_rate);
    if winner_p != BASELINE_P as usize {
        let delta = (winner_sharpe - baseline_sharpe) / baseline_sharpe * 100.0;
        println!("Change from baseline P={}: {:+.2}% Sharpe", BASELINE_P, delta);
    }

    // Write JSON summary
    let mut json_entries = Vec::new();
    for (p, (pass, total, ret, sh, dd, trades)) in &sorted {
        let pr = *pass as f64 / *total as f64 * 100.0;
        json_entries.push(serde_json::json!({
            "chand_period": p,
            "pass_rate": pr,
            "avg_sharpe": sh,
            "avg_ret": ret,
            "avg_dd": dd,
            "total_trades": trades,
            "winner": **p == winner_p
        }));
    }

    let json_obj = serde_json::json!({
        "parameter": "CHAND_PERIOD",
        "sweep_range": "[5..15] step 1",
        "baseline": BASELINE_P,
        "winner": winner_p,
        "baseline_sharpe": baseline_sharpe,
        "winner_sharpe": winner_sharpe,
        "results": json_entries
    });
    let mut jf = File::create(JSON_OUT)?;
    jf.write_all(serde_json::to_string_pretty(&json_obj)?.as_bytes())?;

    println!("\nFiles written:");
    println!("  {}", SUMMARY_CSV);
    println!("  {}", DETAIL_CSV);
    println!("  {}", EQUITY_CSV);
    println!("  {}", JSON_OUT);
    println!("\nRuntime: {:.1}s", start.elapsed().as_secs_f64());

    Ok(())
}
