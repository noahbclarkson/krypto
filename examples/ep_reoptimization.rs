//! EP Re-Optimization: Turtle Entry Period
//!
//! HYPOTHESIS: EP was swept in 2026-04-10 with Chandelier P=45/M=2.5.
//! Current params are P=11/M=2.25 — much tighter exit. With a tighter
//! exit, the optimal entry breakout period may differ.
//!
//! Sweep: EP ∈ [5..55] step 1 (51 values)
//! Base: CHAND_PERIOD=11, CHAND_MULT=2.25, ATR_PERIOD=24 (current production)
//! Universe: Base5 + NoDOGE (9-universe would be slow; validate winner separately)
//! Walk-forward: 252/252 (same as production harness)
//! Exports: CSV per-EP metrics + equity curves for selected EPs

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
const HOLD_MAX: usize = 45;
const TAKER_FEE: f64 = 0.001;
const POSITION_CAP: usize = 3;
const MIN_TRADES: usize = 3;
const CHAND_PERIOD: usize = 11;   // Current production
const CHAND_MULT: f64 = 2.25;    // Current production
const TURTLE_ATR_PERIOD: usize = 24; // Current production
const TURTLE_ATR_MULT: f64 = 2.00;
const VOL_LOOKBACK: usize = 2;

// EP sweep range
const EP_MIN: usize = 5;
const EP_MAX: usize = 55;

// Universes to test (fast set: Base5 + NoDOGE = 12 windows)
const UNIVERSES: &[(&str, &[&str])] = &[
    ("Base5",  &["BTCUSDT","ETHUSDT","SOLUSDT","XRPUSDT","DOGEUSDT","ADAUSDT"]),
    ("NoDOGE", &["BTCUSDT","ETHUSDT","SOLUSDT","XRPUSDT","ADAUSDT"]),
    ("LargeCaps5", &["BTCUSDT","ETHUSDT","SOLUSDT","XRPUSDT","BNBUSDT","ADAUSDT"]),
    ("Legacy4", &["BTCUSDT","ETHUSDT","XRPUSDT","LTCUSDT","EOSUSDT"]),
    ("Legacy3", &["BTCUSDT","XRPUSDT","LTCUSDT","EOSUSDT"]),
];

const METRICS_CSV: &str = "snapshots/ep_reopt_metrics.csv";
const EQUITY_CSV: &str = "snapshots/ep_reopt_equity.csv";

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

/// Run simulation for a given universe + window + EP. Returns (wf_result, equity_curve).
fn run_sim(
    sym_data: &HashMap<String, SymData>,
    symbols: &[String],
    test_start: usize,
    test_end: usize,
    ep: usize,
) -> (f64, f64, f64, usize, bool, Vec<f64>) {
    let mut equity = 1.0_f64;
    let mut equity_curve = vec![1.0_f64];
    let mut wins = 0usize;
    let mut total_trades = 0usize;
    let mut daily_rets = Vec::new();

    let mut bar = test_start;
    while bar + 2 < test_end {
        // Rank symbols by dollar volume
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
                if bar >= ep + 1 && bar < sd.close.len() {
                    if turtle_signal(&sd.close, ep, bar) {
                        let entry_px = sd.close[bar];
                        let entry = entry_px * (1.0 - TAKER_FEE);
                        let entry_bar_next = bar + 1;
                        let n = sd.close.len();

                        let mut highest_high_chand = sd.high[entry_bar_next];
                        let mut highest_high_turtle = sd.high[entry_bar_next];
                        let max_bar = (entry_bar_next + HOLD_MAX).min(n.saturating_sub(1));
                        let mut exit_bar = max_bar;
                        for b in entry_bar_next..=max_bar.min(n.saturating_sub(1)) {
                            highest_high_chand = highest_high_chand.max(sd.high[b]);
                            let atr_chand = atr_at(&sd.high, &sd.low, &sd.close, CHAND_PERIOD, b);
                            let trail_chand = highest_high_chand - CHAND_MULT * atr_chand;

                            highest_high_turtle = highest_high_turtle.max(sd.high[b]);
                            let atr_turtle = atr_at(&sd.high, &sd.low, &sd.close, TURTLE_ATR_PERIOD, b);
                            let trail_turtle = highest_high_turtle - TURTLE_ATR_MULT * atr_turtle;

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
                            for _ in 0..bars_held { daily_rets.push(avg_daily); }

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
    (ret, sharpe, max_dd, total_trades, pass, equity_curve)
}

#[tokio::main]
async fn main() -> Result<()> {
    let t0 = Instant::now();
    eprintln!("==== EP Re-Optimization Sweep ====");
    eprintln!("CHAND({},{}) ATR={} | EP {}..{} step 1", CHAND_PERIOD, CHAND_MULT, TURTLE_ATR_PERIOD, EP_MIN, EP_MAX);
    eprintln!("Universes: {:?}\n", UNIVERSES.iter().map(|(n,_)| *n).collect::<Vec<_>>());

    let loader = DataLoader::new(None, None);
    let mut all_syms: std::collections::HashSet<String> = std::collections::HashSet::new();
    for (_, syms) in UNIVERSES {
        for &s in *syms { all_syms.insert(s.to_string()); }
    }

    let mut raw_cache: HashMap<String, DataFrame> = HashMap::new();
    for sym in &all_syms {
        match loader.fetch_with_cache(sym.as_str(), "1d", CANDLES).await {
            Ok(df) => { raw_cache.insert(sym.clone(), df); }
            Err(e) => { eprintln!("  WARNING: {} load failed: {}", sym, e); }
        }
    }

    let n = raw_cache.values().map(|df| df.height()).min().unwrap_or(2800).min(2800);
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

    // Per-EP aggregate storage: ep -> (total_pass, total_windows, sum_sharpe, sum_ret, sum_dd, sum_trades)
    let mut ep_stats: Vec<(usize, usize, usize, f64, f64, f64, usize)> =
        (EP_MIN..=EP_MAX).map(|ep| (ep, 0, 0, 0.0, 0.0, 0.0, 0)).collect();

    // Equity curve accumulation for selected EPs (across windows, Base5 only)
    let baseline_ep = 21usize;
    let mut selected_ep_equities: HashMap<usize, Vec<f64>> = HashMap::new();
    for ep in [baseline_ep, EP_MIN, EP_MAX, 15, 30] {
        selected_ep_equities.insert(ep, vec![1.0]);
    }

    // We'll track equity curves for Base5 across windows (concatenated for full-period equity)
    // For a cleaner approach: full-sample single-pass equity for each EP on Base5
    // (not walk-forward — just the full simulation to visualise equity curves clearly)
    let base5_syms: Vec<String> = vec!["BTCUSDT","ETHUSDT","SOLUSDT","XRPUSDT","DOGEUSDT","ADAUSDT"]
        .into_iter().map(|s| s.to_string()).collect();

    // Determine total windows from Base5
    let base5_loaded = base5_syms.iter().all(|s| sym_data_map.contains_key(s));

    // === Main sweep: per-EP × per-universe walk-forward ===
    for (ep_idx, ep) in (EP_MIN..=EP_MAX).enumerate() {
        let mut tot_pass = 0usize;
        let mut tot_win = 0usize;
        let mut sum_sh = 0.0_f64;
        let mut sum_ret = 0.0_f64;
        let mut sum_dd = 0.0_f64;
        let mut sum_trd = 0usize;

        for &(label, syms) in UNIVERSES {
            let symbols: Vec<String> = syms.iter().map(|s| s.to_string()).collect();
            if !symbols.iter().all(|s| sym_data_map.contains_key(s)) { continue; }

            let total_windows = n.saturating_sub(TRAIN_BARS + TEST_BARS) / TEST_BARS;
            for wi in 0..total_windows {
                let train_end = TRAIN_BARS + wi * TEST_BARS;
                let test_start = train_end;
                let test_end = (test_start + TEST_BARS).min(n);
                if test_end.saturating_sub(test_start) < 5 { continue; }

                let (ret, sharpe, dd, trades, pass, _eq) = run_sim(
                    &sym_data_map, &symbols, test_start, test_end, ep
                );

                tot_win += 1;
                if pass { tot_pass += 1; }
                sum_sh += sharpe;
                sum_ret += ret;
                sum_dd += dd;
                sum_trd += trades;
            }
        }

        ep_stats[ep_idx] = (ep, tot_pass, tot_win, sum_sh, sum_ret, sum_dd, sum_trd);

        let avg_sh = if tot_win > 0 { sum_sh / tot_win as f64 } else { 0.0 };
        let avg_ret = if tot_win > 0 { sum_ret / tot_win as f64 } else { 0.0 };
        let pass_pct = if tot_win > 0 { tot_pass as f64 / tot_win as f64 * 100.0 } else { 0.0 };
        let baseline_mark = if ep == baseline_ep { " *** BASELINE" } else { "" };
        eprintln!(
            "EP={:2} | pass {}/{} ({:4.0}%) | avg_sh {:6.3} | avg_ret {:+7.1}%{}",
            ep, tot_pass, tot_win, pass_pct, avg_sh, avg_ret, baseline_mark
        );
    }

    // === Full-sample equity curves for selected EPs (Base5, not walk-forward) ===
    // Run from bar=TRAIN_BARS to bar=n (skip training period entirely for visual consistency)
    let full_eq_eps: Vec<usize> = {
        // Find winner EP (best avg sharpe)
        let mut best_ep = baseline_ep;
        let mut best_sh = f64::NEG_INFINITY;
        for &(ep, _, tot_win, sum_sh, ..) in &ep_stats {
            if tot_win > 0 {
                let avg_sh = sum_sh / tot_win as f64;
                if avg_sh > best_sh { best_sh = avg_sh; best_ep = ep; }
            }
        }
        let mut runner_up_ep = baseline_ep;
        let mut runner_up_sh = f64::NEG_INFINITY;
        for &(ep, _, tot_win, sum_sh, ..) in &ep_stats {
            if tot_win > 0 && ep != best_ep {
                let avg_sh = sum_sh / tot_win as f64;
                if avg_sh > runner_up_sh { runner_up_sh = avg_sh; runner_up_ep = ep; }
            }
        }
        eprintln!("\nWinner EP={} (avg_sh={:.3}), Runner-up EP={}", best_ep, best_sh, runner_up_ep);
        let mut v = vec![baseline_ep, best_ep, runner_up_ep, 10, 30];
        v.sort(); v.dedup();
        v
    };

    // Write equity curves CSV header
    let mut eq_lines = Vec::new();
    let headers: Vec<String> = full_eq_eps.iter().map(|ep| format!("ep_{}", ep)).collect();
    eq_lines.push(format!("step,{}", headers.join(",")));

    if base5_loaded {
        // Collect equity curves for each selected EP
        let mut ep_curves: HashMap<usize, Vec<f64>> = HashMap::new();
        for &ep in &full_eq_eps {
            let (_ret, _sh, _dd, _trades, _pass, curve) = run_sim(
                &sym_data_map, &base5_syms,
                TRAIN_BARS,  // start from post-training period
                n,
                ep
            );
            ep_curves.insert(ep, curve);
        }

        // Pad all curves to same length with last value
        let max_len = ep_curves.values().map(|c| c.len()).max().unwrap_or(1);
        for (&ep, curve) in ep_curves.iter_mut() {
            let last = *curve.last().unwrap_or(&1.0);
            while curve.len() < max_len { curve.push(last); }
            eprintln!("  EP={} full-sample equity: {:.2}x ({} steps)", ep, last, max_len);
        }

        for step in 0..max_len {
            let vals: Vec<String> = full_eq_eps.iter().map(|ep| {
                ep_curves.get(ep)
                    .and_then(|c| c.get(step))
                    .map(|v| format!("{:.6}", v))
                    .unwrap_or("1.0".to_string())
            }).collect();
            eq_lines.push(format!("{},{}", step, vals.join(",")));
        }
    }

    // === Write metrics CSV ===
    std::fs::create_dir_all("snapshots").ok();
    {
        let mut f = File::create(METRICS_CSV)?;
        writeln!(f, "ep,pass,total_windows,pass_pct,avg_sharpe,avg_return_pct,avg_dd_pct,avg_trades")?;
        for &(ep, pass, tot, sum_sh, sum_ret, sum_dd, sum_trd) in &ep_stats {
            let avg_sh = if tot > 0 { sum_sh / tot as f64 } else { 0.0 };
            let avg_ret = if tot > 0 { sum_ret / tot as f64 } else { 0.0 };
            let avg_dd = if tot > 0 { sum_dd / tot as f64 } else { 0.0 };
            let avg_trd = if tot > 0 { sum_trd / tot } else { 0 };
            let pass_pct = if tot > 0 { pass as f64 / tot as f64 * 100.0 } else { 0.0 };
            writeln!(f, "{},{},{},{:.1},{:.4},{:.2},{:.2},{}",
                ep, pass, tot, pass_pct, avg_sh, avg_ret, avg_dd, avg_trd)?;
        }
    }
    eprintln!("\nMetrics CSV → {}", METRICS_CSV);

    // === Write equity CSV ===
    {
        let mut f = File::create(EQUITY_CSV)?;
        for line in &eq_lines { writeln!(f, "{}", line)?; }
    }
    eprintln!("Equity CSV → {}", EQUITY_CSV);

    eprintln!("\n✅ Done in {:.1}s", t0.elapsed().as_secs_f64());
    Ok(())
}
