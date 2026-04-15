//! Chandelier ATR Period Hyperopt — Full Sweep
//!
//! TARGET: CHAND_PERIOD (ATR lookback for Chandelier trailing stop)
//! Baseline: 45 (never validated)
//! Literature default: 22
//! Sweep range: 10 to 80 step 5 → 15 values
//!
//! Strategy: Turtle(EP=21) + Chandelier(P, M=2.05)
//! M=2.05 is the already-optimized multiplier from prior sweep.
//! Walk-forward: 252-bar train / 252-bar test
//! Universes: All 9 harsh universes
//! Fee: 0.1% taker each side

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
const HOLD_MAX: usize = 60;
const TAKER_FEE: f64 = 0.001;
const POSITION_CAP: usize = 2;
const MIN_TRADES: usize = 3;
const CHAND_MULT: f64 = 2.05; // already optimized
const TURTLE_ENTRY: usize = 21; // already optimized

const PERIOD_START: usize = 10;
const PERIOD_END: usize = 80;
const PERIOD_STEP: usize = 5;

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

const CSV_OUT: &str = "snapshots/chandelier_period_sweep.csv";
const EQUITY_OUT: &str = "snapshots/chandelier_period_equity_curves.csv";
const SUMMARY_OUT: &str = "snapshots/chandelier_period_sweep_summary.json";

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

fn turtle_signal(close: &[f64], entry_period: usize, idx: usize) -> bool {
    if idx < entry_period + 1 { return false; }
    let start = idx + 1 - entry_period;
    let mut max_close = f64::NEG_INFINITY;
    for i in start..idx {
        if let Some(&c) = close.get(i) { max_close = max_close.max(c); }
    }
    if let Some(&curr_close) = close.get(idx) {
        curr_close > max_close
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

#[derive(Clone)]
struct SimResult {
    ret: f64,
    sharpe: f64,
    max_dd: f64,
    trades: usize,
    win_rate: f64,
    pass: bool,
    equity_curve: Vec<f64>,
}

fn run_sim(
    sym_data: &HashMap<String, SymData>,
    symbols: &[String],
    test_start: usize,
    test_end: usize,
    chand_period: usize,
) -> SimResult {
    let mut equity = 1.0_f64;
    let mut equity_curve = vec![1.0_f64];
    let mut wins = 0usize;
    let mut total_trades = 0usize;
    let mut daily_rets = Vec::new();

    let mut bar = test_start;
    while bar + 2 < test_end {
        let mut scores: Vec<(&str, f64)> = Vec::new();
        for sym in symbols {
            if let Some(sd) = sym_data.get(sym) {
                if bar >= sd.close.len() { continue; }
                let dv = sd.vol.get(bar).copied().unwrap_or(0.0)
                    * sd.close.get(bar).copied().unwrap_or(0.0);
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
                    if turtle_signal(&sd.close, TURTLE_ENTRY, bar) {
                        let entry_px = sd.close[bar];
                        let entry = entry_px * (1.0 - TAKER_FEE);
                        let entry_bar_next = bar + 1;
                        let n = sd.close.len();

                        let mut highest_high = sd.high[entry_bar_next];
                        let mut exit_bar = (entry_bar_next + HOLD_MAX).min(n.saturating_sub(1));
                        for b in entry_bar_next..exit_bar.min(n.saturating_sub(1)) {
                            highest_high = highest_high.max(sd.high[b]);
                            let atr_val = atr_at(&sd.high, &sd.low, &sd.close, chand_period, b);
                            let trail = highest_high - CHAND_MULT * atr_val;
                            if sd.close[b] < trail {
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
    let win_rate = if total_trades > 0 { wins as f64 / total_trades as f64 * 100.0 } else { 0.0 };
    let pass = total_trades >= MIN_TRADES && ret > 0.0;

    SimResult { ret, sharpe, max_dd, trades: total_trades, win_rate, pass, equity_curve }
}

fn main() -> Result<()> {
    let t0 = Instant::now();

    let periods: Vec<usize> = (PERIOD_START..=PERIOD_END)
        .step_by(PERIOD_STEP)
        .collect();
    let n_periods = periods.len();

    eprintln!("==== Chandelier ATR Period Sweep ====");
    eprintln!("Sweep: {} to {} step {} -> {} values", PERIOD_START, PERIOD_END, PERIOD_STEP, n_periods);
    eprintln!("CHAND_MULT={}, TURTLE_ENTRY={}", CHAND_MULT, TURTLE_ENTRY);
    eprintln!("Universes: 9 | Train/Test: {}/{}\n", TRAIN_BARS, TEST_BARS);

    // Load data
    let rt = tokio::runtime::Runtime::new()?;
    let loader = DataLoader::new(None, None);
    let mut all_syms: std::collections::HashSet<String> = std::collections::HashSet::new();
    for (_, syms) in UNIVERSES {
        for &s in *syms { all_syms.insert(s.to_string()); }
    }

    let mut raw_cache: HashMap<String, DataFrame> = HashMap::new();
    let mut min_len = usize::MAX;
    for sym in all_syms.iter() {
        match rt.block_on(loader.fetch_with_cache(sym.as_str(), "1d", CANDLES)) {
            Ok(df) => {
                min_len = min_len.min(df.height());
                raw_cache.insert(sym.clone(), df);
            }
            Err(e) => { eprintln!("  WARNING: {} load failed: {}", sym, e); }
        }
    }

    let n = min_len.min(2800);
    let mut sym_data_map: HashMap<String, SymData> = HashMap::new();
    for sym in &all_syms {
        if let Some(df) = raw_cache.get(sym) {
            let n_min = df.height().min(n);
            macro_rules! col_vec {
                ($name:expr) => {{
                    let chunked = df.column($name)?.f64()?;
                    chunked.into_iter().filter_map(|x| x).take(n_min).collect::<Vec<_>>()
                }};
            }
            sym_data_map.insert(sym.clone(), SymData {
                close: col_vec!("close"),
                high:  col_vec!("high"),
                low:   col_vec!("low"),
                vol:   col_vec!("volume"),
            });
        }
    }
    eprintln!("Loaded {} symbols, {} bars\n", sym_data_map.len(), n);

    // Open output files
    let mut csv_file = File::create(CSV_OUT)?;
    csv_file.write_all(b"period,universe,window,return_pct,sharpe,max_dd_pct,trades,win_rate_pct,pass\n")?;

    let mut equity_file = File::create(EQUITY_OUT)?;
    equity_file.write_all(b"period,universe,step,equity\n")?;

    // Run sweep
    let mut global_results: HashMap<usize, Vec<SimResult>> = HashMap::new();

    for &chand_period in &periods {
        eprintln!("--- CHAND_PERIOD = {} ---", chand_period);
        let mut period_all: Vec<SimResult> = Vec::new();

        for &(label, symbols) in UNIVERSES {
            let symbols: Vec<String> = symbols.iter().map(|s| s.to_string()).collect();
            let all_loaded = symbols.iter().all(|s| sym_data_map.contains_key(s));
            if !all_loaded {
                eprintln!("  {:>20} SKIPPED (missing data)", label);
                continue;
            }

            let total_windows = n.saturating_sub(TRAIN_BARS + TEST_BARS) / TEST_BARS;
            if total_windows == 0 { continue; }

            let mut uni_results: Vec<SimResult> = Vec::new();

            for wi in 0..total_windows {
                let train_end = TRAIN_BARS + wi * TEST_BARS;
                let test_start = train_end;
                let test_end = (test_start + TEST_BARS).min(n);

                if test_end.saturating_sub(test_start) < 5 { continue; }

                let r = run_sim(&sym_data_map, &symbols, test_start, test_end, chand_period);

                let csv_line = format!(
                    "{},{},{},{:.4},{:.4},{:.4},{},{:.4},{}\n",
                    chand_period, label, wi, r.ret, r.sharpe, r.max_dd, r.trades, r.win_rate, r.pass
                );
                csv_file.write_all(csv_line.as_bytes())?;

                // Equity curve every 5th window
                if wi % 5 == 0 {
                    for (step, &eq) in r.equity_curve.iter().enumerate() {
                        let eq_line = format!("{},{},{},{:.8}\n", chand_period, label, wi * TEST_BARS + step, eq);
                        equity_file.write_all(eq_line.as_bytes())?;
                    }
                }

                uni_results.push(r);
            }

            if !uni_results.is_empty() {
                let avg_sharpe: f64 = uni_results.iter().map(|r| r.sharpe).sum::<f64>() / uni_results.len() as f64;
                let avg_ret: f64 = uni_results.iter().map(|r| r.ret).sum::<f64>() / uni_results.len() as f64;
                let pass_count = uni_results.iter().filter(|r| r.pass).count();
                let thin_count = uni_results.iter().filter(|r| r.trades < MIN_TRADES).count();
                let thin_str: String = if thin_count > 0 { format!(" ({}THIN)", thin_count) } else { String::new() };
                let star_str: &str = if pass_count == uni_results.len() { " *" } else { "" };
                eprintln!(
                    "  {:>20} | sh={:6.2} ret={:+8.1}% {}/{} pass{}{}",
                    label, avg_sharpe, avg_ret, pass_count, uni_results.len(), thin_str, star_str
                );
            }

            period_all.extend(uni_results);
        }

        if !period_all.is_empty() {
            let avg_sharpe: f64 = period_all.iter().map(|r| r.sharpe).sum::<f64>() / period_all.len() as f64;
            let avg_ret: f64 = period_all.iter().map(|r| r.ret).sum::<f64>() / period_all.len() as f64;
            let avg_dd: f64 = period_all.iter().map(|r| r.max_dd).sum::<f64>() / period_all.len() as f64;
            let total_pass = period_all.iter().filter(|r| r.pass).count();
            let total_trades: usize = period_all.iter().map(|r| r.trades).sum();
            let pass_rate = total_pass as f64 / period_all.len() as f64 * 100.0;
            eprintln!(
                "  *** GLOBAL: sh={:.4} ret={:.2}% DD={:.2}% {}/{} pass ({:.1}%) {} trades\n",
                avg_sharpe, avg_ret, avg_dd, total_pass, period_all.len(), pass_rate, total_trades
            );
        }

        global_results.insert(chand_period, period_all);
    }

    // Ranking
    eprintln!("\n==== SWEEP RANKING (by avg OOS Sharpe) ====");
    let baseline_period: usize = 45;
    let mut rankings: Vec<(usize, f64, f64, f64, f64, usize, usize)> = Vec::new();
    for (&period, ref results) in &global_results {
        let avg_sharpe = results.iter().map(|r| r.sharpe).sum::<f64>() / results.len().max(1) as f64;
        let avg_ret    = results.iter().map(|r| r.ret).sum::<f64>() / results.len().max(1) as f64;
        let avg_dd     = results.iter().map(|r| r.max_dd).sum::<f64>() / results.len().max(1) as f64;
        let total_pass = results.iter().filter(|r| r.pass).count();
        let total_trades: usize = results.iter().map(|r| r.trades).sum();
        let pass_rate = total_pass as f64 / results.len().max(1) as f64 * 100.0;
        rankings.push((period, avg_sharpe, avg_ret, avg_dd, pass_rate, total_pass, total_trades));
    }
    rankings.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());

    let mut rank = 1usize;
    for &(period, avg_sharpe, avg_ret, avg_dd, pass_rate, total_pass, total_trades) in &rankings {
        let n_results = global_results.get(&period).map(|r| r.len()).unwrap_or(0);
        let marker: &str = if period == baseline_period { " <-BASELINE" } else { "" };
        let star: &str = if rank == 1 { " *WIN" } else { "" };
        eprintln!(
            "  #{:>2} | P={} | Sharpe={:.4} | Ret={:.2}% | DD={:.2}% | {}/{} pass ({:.1}%) | {} trades{}{}",
            rank, period, avg_sharpe, avg_ret, avg_dd, total_pass, n_results, pass_rate, total_trades, star, marker
        );
        rank += 1;
    }

    // JSON summary
    let mut json_str = String::new();
    json_str.push_str("{\n");
    json_str.push_str("  \"parameter\": \"CHAND_PERIOD\",\n");
    json_str.push_str(&format!("  \"sweep\": {{ \"start\": {}, \"end\": {}, \"step\": {} }},\n",
        PERIOD_START, PERIOD_END, PERIOD_STEP));
    json_str.push_str(&format!("  \"n_values\": {},\n", n_periods));
    json_str.push_str(&format!("  \"baseline\": {},\n", baseline_period));
    json_str.push_str("  \"universes\": 9,\n");
    json_str.push_str("  \"rankings\": [\n");

    let last_idx = rankings.len().saturating_sub(1);
    for (i, &(period, avg_sharpe, avg_ret, avg_dd, pass_rate, total_pass, total_trades)) in rankings.iter().enumerate() {
        let n_results = global_results.get(&period).map(|r| r.len()).unwrap_or(0);
        let is_base: &str = if period == baseline_period { "true" } else { "false" };
        let comma: &str = if i < last_idx { "," } else { "" };

        json_str.push_str("    {\n");
        json_str.push_str(&format!("      \"rank\": {},\n", i + 1));
        json_str.push_str(&format!("      \"period\": {},\n", period));
        json_str.push_str(&format!("      \"avg_sharpe\": {:.6},\n", avg_sharpe));
        json_str.push_str(&format!("      \"avg_return\": {:.4},\n", avg_ret));
        json_str.push_str(&format!("      \"avg_dd\": {:.4},\n", avg_dd));
        json_str.push_str(&format!("      \"pass_windows\": {},\n", total_pass));
        json_str.push_str(&format!("      \"total_windows\": {},\n", n_results));
        json_str.push_str(&format!("      \"pass_rate\": {:.2},\n", pass_rate));
        json_str.push_str(&format!("      \"total_trades\": {},\n", total_trades));
        json_str.push_str(&format!("      \"is_baseline\": {}", is_base));
        json_str.push_str("\n    }");
        json_str.push_str(comma);
        json_str.push('\n');
    }

    json_str.push_str("  ]\n");
    json_str.push_str("}\n");

    let mut json_file = File::create(SUMMARY_OUT)?;
    json_file.write_all(json_str.as_bytes())?;

    eprintln!("\nCSV: {}", CSV_OUT);
    eprintln!("Equity curves: {}", EQUITY_OUT);
    eprintln!("Summary: {}", SUMMARY_OUT);
    eprintln!("Runtime: {:?}", t0.elapsed());

    Ok(())
}
