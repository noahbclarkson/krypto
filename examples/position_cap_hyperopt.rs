//! Position Cap Hyperopt — Turtle+Chandelier
//!
//! HYPOTHESIS: POSITION_CAP=2 was assumed from intuition, never validated.
//! Sweep: POSITION_CAP ∈ {1, 2, 3, 4, 5}
//! Strategy: Turtle(EP=21) + Chandelier(28, 2.00)
//! Validation: Walk-forward 252/252, 9 universes
//! Metric: Avg OOS Sharpe across all universes
//!
//! CRITICAL: Export equity time-series for baseline, winner, runner-ups.

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
const MIN_TRADES: usize = 3;
const CHAND_PERIOD: usize = 28;
const CHAND_MULT: f64 = 2.00;
const TURTLE_ENTRY: usize = 21;

const POSITION_CAPS: [usize; 5] = [1, 2, 3, 4, 5];

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

type WfTuple = (f64, f64, f64, usize, f64, bool);
type CapAgg = (Vec<f64>, Vec<f64>, Vec<f64>, usize, usize, usize, usize);

fn run_sim_for_cap(
    sym_data: &HashMap<String, SymData>,
    symbols: &[String],
    test_start: usize,
    test_end: usize,
    position_cap: usize,
) -> (WfTuple, Vec<f64>) {
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
                let dv = sd.vol.get(bar).copied().unwrap_or(0.0)
                    * sd.close.get(bar).copied().unwrap_or(0.0);
                scores.push((sym.as_str(), if dv.is_finite() && dv > 0.0 { dv } else { 0.0 }));
            }
        }
        scores.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());
        let top_syms: Vec<String> = scores.into_iter().take(position_cap).map(|(s, _)| s.to_string()).collect();

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
                            let atr_val = atr_at(&sd.high, &sd.low, &sd.close, CHAND_PERIOD, b);
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
    let win_rate = if total_trades > 0 { wins as f64 / total_trades as f64 * 100.0 } else { 0.0 };
    let pass = total_trades >= MIN_TRADES && ret > 0.0;

    ((ret, sharpe, max_dd, total_trades, win_rate, pass), equity_curve)
}

#[tokio::main]
async fn main() -> Result<()> {
    let t0 = Instant::now();
    eprintln!("==== Position Cap Hyperopt: Turtle+Chandelier ====");
    eprintln!("Sweep: {:?}, Chandelier({}, {}), 252/252 train/test\n", POSITION_CAPS, CHAND_PERIOD, CHAND_MULT);

    let loader = DataLoader::new(None, None);
    let mut all_syms: std::collections::HashSet<String> = std::collections::HashSet::new();
    for (_, syms) in UNIVERSES {
        for &s in *syms {
            all_syms.insert(s.to_string());
        }
    }

    let mut raw_cache: HashMap<String, DataFrame> = HashMap::new();
    let mut min_len = usize::MAX;
    for sym in all_syms.iter() {
        match loader.fetch_with_cache(sym.as_str(), "1d", CANDLES).await {
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

    // ── Results storage: HashMap<cap, HashMap<uni, Vec<WfTuple>>> ─────────────
    let mut results: HashMap<usize, HashMap<String, Vec<WfTuple>>> = HashMap::new();
    let mut equity_curves: HashMap<usize, HashMap<String, Vec<Vec<f64>>>> = HashMap::new();
    for &cap in &POSITION_CAPS {
        results.insert(cap, HashMap::new());
        equity_curves.insert(cap, HashMap::new());
    }

    // ── Run sweep ───────────────────────────────────────────────────────────────
    for &(label, symbols) in UNIVERSES {
        let symbols: Vec<String> = symbols.iter().map(|s| s.to_string()).collect();
        let all_loaded = symbols.iter().all(|s| sym_data_map.contains_key(s));
        if !all_loaded {
            eprintln!("{:>20} SKIPPED (missing data)", label);
            continue;
        }

        let total_windows = n.saturating_sub(TRAIN_BARS + TEST_BARS) / TEST_BARS;
        if total_windows == 0 {
            eprintln!("{:>20} SKIPPED (not enough data)", label);
            continue;
        }

        eprintln!("==== {:<18} ==== {} syms, {} windows", label, symbols.len(), total_windows);

        for wi in 0..total_windows {
            let train_end = TRAIN_BARS + wi * TEST_BARS;
            let test_start = train_end;
            let test_end = (test_start + TEST_BARS).min(n);

            if test_end.saturating_sub(test_start) < 5 { continue; }

            for &cap in &POSITION_CAPS {
                let (wf_result, eq_curve) =
                    run_sim_for_cap(&sym_data_map, &symbols, test_start, test_end, cap);

                results.get_mut(&cap).unwrap()
                    .entry(label.to_string())
                    .or_default()
                    .push(wf_result);

                equity_curves.get_mut(&cap).unwrap()
                    .entry(label.to_string())
                    .or_default()
                    .push(eq_curve);
            }
        }
    }

    // ── Aggregate and print results ─────────────────────────────────────────────
    eprintln!("\n==== RESULTS SUMMARY ====");
    let mut summary_csv = vec!["position_cap,avg_sharpe,avg_return,avg_max_dd,pass_rate,total_trades,unis_positive".to_string()];
    let mut cap_agg: HashMap<usize, CapAgg> = HashMap::new();
    for &cap in &POSITION_CAPS {
        cap_agg.insert(cap, (Vec::new(), Vec::new(), Vec::new(), 0, 0, 0, 0));
    }

    for &cap in &POSITION_CAPS {
        let cap_res = results.get(&cap).unwrap();
        let mut all_sharpes = Vec::new();
        let mut all_returns = Vec::new();
        let mut all_dds = Vec::new();
        let mut all_trades = 0usize;
        let mut all_passed = 0usize;
        let mut all_total = 0usize;
        let mut unis_positive = 0usize;

        for (_uni, windows) in cap_res {
            let u_avg_sh = windows.iter().map(|w| w.1).sum::<f64>() / windows.len().max(1) as f64;
            if u_avg_sh > 0.0 { unis_positive += 1; }

            for w in windows {
                all_sharpes.push(w.1);
                all_returns.push(w.0);
                all_dds.push(w.2);
                all_trades += w.3;
                if w.5 { all_passed += 1; }
                all_total += 1;
            }
        }

        let avg_sh = all_sharpes.iter().sum::<f64>() / all_sharpes.len().max(1) as f64;
        let avg_ret = all_returns.iter().sum::<f64>() / all_returns.len().max(1) as f64;
        let avg_dd = all_dds.iter().sum::<f64>() / all_dds.len().max(1) as f64;
        let pass_rate = all_passed as f64 / all_total.max(1) as f64 * 100.0;

        eprintln!("  CAP={} | avg_sharpe={:.3} | avg_ret={:+.1}% | avg_dd={:.1}% | pass={:.0}% | {} pos/9 unis | {} trades",
            cap, avg_sh, avg_ret, avg_dd, pass_rate, unis_positive, all_trades);

        summary_csv.push(format!("{},{:.4},{:.2},{:.2},{:.2},{},{}", cap, avg_sh, avg_ret, avg_dd, pass_rate, all_trades, unis_positive));
    }

    // ── Write summary CSV ─────────────────────────────────────────────────────
    let summary_path = "snapshots/position_cap_sweep_summary.csv";
    {
        let mut sf = File::create(summary_path)?;
        for line in &summary_csv { writeln!(sf, "{}", line)?; }
    }
    eprintln!("\n  Summary CSV: {}", summary_path);

    // ── Write per-universe detail CSV ──────────────────────────────────────────
    let mut detail_csv = vec!["position_cap,universe,window,return_pct,sharpe,max_dd_pct,trades,win_rate_pct,pass".to_string()];
    for &cap in &POSITION_CAPS {
        let cap_res = results.get(&cap).unwrap();
        for (_uni, windows) in cap_res {
            for (wi, w) in windows.iter().enumerate() {
                detail_csv.push(format!("{},{},{},{:.2},{:.4},{:.2},{},{:.2},{}", cap, uni, wi, w.0, w.1, w.2, w.3, w.4, w.5));
            }
        }
    }
    let detail_path = "snapshots/position_cap_sweep_detail.csv";
    {
        let mut df_f = File::create(detail_path)?;
        for line in &detail_csv { writeln!(df_f, "{}", line)?; }
    }
    eprintln!("  Detail CSV: {}", detail_path);

    // ── Build and write equity curves ──────────────────────────────────────────
    // One CSV per cap value, and one combined CSV for all caps
    let max_steps = equity_curves.get(&POSITION_CAPS[0]).map(|m| {
        m.values().map(|wins| wins.iter().map(|eq| eq.len()).max().unwrap_or(0)).max().unwrap_or(0)
    }).unwrap_or(0);

    let mut combined_steps = vec![1.0_f64; max_steps];

    for &cap in &POSITION_CAPS {
        let cap_eqs = equity_curves.get(&cap).unwrap();
        // Merge all windows: for each cap, compound all window equity curves sequentially
        let mut merged: Vec<f64> = vec![1.0_f64];
        for (_, windows_eqs) in cap_eqs {
            for eq in windows_eqs {
                if eq.len() > 1 {
                    // compound from last value
                    let last_val = *merged.last().unwrap_or(&1.0_f64);
                    for &v in eq.iter().skip(1) {
                        merged.push(last_val * v);
                    }
                }
            }
        }

        // Write individual equity CSV
        let eq_path = format!("snapshots/position_cap_equity_cap{}.csv", cap);
        {
            let mut f = File::create(&eq_path)?;
            writeln!(f, "step,equity")?;
            for (i, &v) in merged.iter().enumerate() {
                writeln!(f, "{},{:.6}", i, v)?;
            }
        }
        eprintln!("  Equity CSV: {}", eq_path);

        // Truncate to max_steps for combined comparison
        for i in 0..max_steps.min(merged.len()) {
            combined_steps[i] = merged[i];
        }
    }

    // Combined equity CSV (all caps in columns)
    let combined_eq_path = "snapshots/position_cap_all_equity.csv";
    {
        let mut f = File::create(combined_eq_path)?;
        // Build merged equity for each cap up to max_steps
        let mut cap_merged: Vec<Vec<f64>> = Vec::new();
        for &cap in &POSITION_CAPS {
            let cap_eqs = equity_curves.get(&cap).unwrap();
            let mut merged: Vec<f64> = vec![1.0_f64];
            for (_, windows_eqs) in cap_eqs {
                for eq in windows_eqs {
                    if eq.len() > 1 {
                        let last_val = *merged.last().unwrap_or(&1.0_f64);
                        for &v in eq.iter().skip(1) {
                            merged.push(last_val * v);
                        }
                    }
                }
            }
            // Pad to max_steps
            while merged.len() < max_steps {
                merged.push(*merged.last().unwrap_or(&1.0_f64));
            }
            cap_merged.push(merged);
        }

        writeln!(f, "step,{}", POSITION_CAPS.iter().map(|c| format!("PCAP_{}", c)).collect::<Vec<_>>().join(","))?;
        for i in 0..max_steps {
            let vals: Vec<String> = cap_merged.iter().map(|m| format!("{:.6}", m[i.min(m.len()-1)])).collect();
            writeln!(f, "{},{}", i, vals.join(","))?;
        }
        eprintln!("  Combined equity CSV: {}", combined_eq_path);
    }

    // ── JSON summary ───────────────────────────────────────────────────────────
    let json_path = "snapshots/position_cap_sweep_summary.json";
    {
        let mut f = File::create(json_path)?;
        writeln!(f, "{{")?;
        writeln!(f, "  \"parameter\": \"POSITION_CAP\",")?;
        writeln!(f, "  \"values\": [")?;
        let mut first = true;
        for &cap in &POSITION_CAPS {
            let cap_res = results.get(&cap).unwrap();
            let mut all_sharpes = Vec::new();
            let mut all_returns = Vec::new();
            let mut all_dds = Vec::new();
            let mut all_trades = 0usize;
            let mut all_passed = 0usize;
            let mut all_total = 0usize;
            let mut unis_positive = 0usize;

            for (_uni, windows) in cap_res {
                let u_avg_sh = windows.iter().map(|w| w.1).sum::<f64>() / windows.len().max(1) as f64;
                if u_avg_sh > 0.0 { unis_positive += 1; }
                for w in windows {
                    all_sharpes.push(w.1);
                    all_returns.push(w.0);
                    all_dds.push(w.2);
                    all_trades += w.3;
                    if w.5 { all_passed += 1; }
                    all_total += 1;
                }
            }

            let avg_sh = all_sharpes.iter().sum::<f64>() / all_sharpes.len().max(1) as f64;
            let avg_ret = all_returns.iter().sum::<f64>() / all_returns.len().max(1) as f64;
            let avg_dd = all_dds.iter().sum::<f64>() / all_dds.len().max(1) as f64;
            let pass_rate = all_passed as f64 / all_total.max(1) as f64 * 100.0;

            if !first { writeln!(f, ",")?; }
            first = false;
            writeln!(f, "    {{ \"value\": {}, \"avg_sharpe\": {:.4}, \"avg_return\": {:.2}, \"avg_dd\": {:.2}, \"pass_rate\": {:.2}, \"trades\": {}, \"unis_positive\": {} }}",
                cap, avg_sh, avg_ret, avg_dd, pass_rate, all_trades, unis_positive)?;
        }
        writeln!(f, "  ]")?;
        writeln!(f, "}}")?;
        eprintln!("  JSON: {}", json_path);
    }

    eprintln!("\n  Total runtime: {:.1}s", t0.elapsed().as_secs_f64());
    Ok(())
}
