//! Volume Ranking Lookback Hyperopt — Turtle+Chandelier
//!
//! Hypothesis: Ranking symbols by current-bar volume alone is noisy.
//! A rolling average volume (smoothing) may produce more stable top-N rankings.
//!
//! Parameter: VOL_LOOKBACK ∈ {1, 2, 3, 5, 7, 10, 14, 21}
//!   VOL_LOOKBACK=1 (current bar, no smoothing) = current default
//!   Higher values = smoother volume signal
//!
//! Strategy: Turtle+Chandelier(EP=21, P=28, M=2.0, ATR=25, CAP=3, HM=45)
//! Walk-forward: 252-bar train / 252-bar test
//! Fees: 0.1% taker each side
//!
//! Output: snapshots/vol_lookback_hyperopt.csv (metrics per window per vol_lb)
//!         snapshots/vol_lookback_aggregate.csv (equity curves per vol_lb)

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
const CHAND_PERIOD: usize = 20;  // hyperopt 2026-04-16: full sweep, +1.55% Sharpe vs CP=28
const CHAND_MULT: f64 = 2.15;     // hyperopt 2026-04-16: fine sweep, saturation plateau M≥2.15
const TURTLE_ENTRY: usize = 21;
const TURTLE_ATR_PERIOD: usize = 24;
const TURTLE_ATR_MULT: f64 = 2.00;

const VOL_LOOKBACK_VALS: &[usize] = &[1, 2, 3, 5, 7, 10, 14, 21];

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
    // Pre-computed rolling average volume: rolling_vol[bar] = avg vol over VOL_LOOKBACK bars ending at bar
    // We'll recompute per vol_lookback value
}

fn rolling_avg(vals: &[f64], window: usize, idx: usize) -> f64 {
    if idx + 1 < window { return vals[idx]; } // not enough history, use current
    let start = idx + 1 - window;
    let slice = &vals[start..=idx];
    slice.iter().sum::<f64>() / window as f64
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

fn turtle_signal(close: &[f64], _high: &[f64], entry_period: usize, idx: usize) -> bool {
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

struct WfResult {
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
    vol_lookback: usize,
) -> WfResult {
    let mut equity = 1.0_f64;
    let mut equity_curve = vec![1.0_f64];
    let mut peak = equity;
    let mut wins = 0usize;
    let mut total_trades = 0usize;
    let mut daily_rets = Vec::new();

    let mut bar = test_start;
    while bar + 2 < test_end {
        // Rank symbols by ROLLING AVERAGE volume (not current bar volume)
        let mut scores: Vec<(&str, f64)> = Vec::new();
        for sym in symbols {
            if let Some(sd) = sym_data.get(sym) {
                if bar >= sd.close.len() { continue; }
                // Use rolling average volume
                let rol_vol = rolling_avg(&sd.vol, vol_lookback, bar);
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
                    if turtle_signal(&sd.close, &sd.high, TURTLE_ENTRY, bar) {
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
    let mut peak_eq = f64::NEG_INFINITY;
    let mut max_dd = 0.0_f64;
    for &e in &equity_curve {
        if e > peak_eq { peak_eq = e; }
        let dd = (peak_eq - e) / peak_eq;
        if dd > max_dd { max_dd = dd; }
    }
    max_dd *= 100.0;
    let win_rate = if total_trades > 0 { wins as f64 / total_trades as f64 * 100.0 } else { 0.0 };
    let pass = total_trades >= 3 && ret > 0.0;

    WfResult { ret, sharpe, max_dd, trades: total_trades, win_rate, pass, equity_curve }
}

struct VbStats {
    sharpes: Vec<f64>,
    rets: Vec<f64>,
    pass_count: usize,
    total_count: usize,
    worst_dd: f64,
}

#[tokio::main]
async fn main() -> Result<()> {
    let t0 = Instant::now();
    eprintln!("==== Volume Ranking Lookback Hyperopt ====\n");
    eprintln!("Sweeping VOL_LOOKBACK ∈ {:?} (current default = 1, no smoothing)\n", VOL_LOOKBACK_VALS);

    let loader = DataLoader::new(None, None);
    let mut all_syms: std::collections::HashSet<String> = std::collections::HashSet::new();
    for (_, syms) in UNIVERSES {
        for &s in *syms { all_syms.insert(s.to_string()); }
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
            let n_min = n.min(df.height());
            let close_col = df.column("close")?.f64()?;
            let high_col = df.column("high")?.f64()?;
            let low_col = df.column("low")?.f64()?;
            let vol_col = df.column("volume")?.f64()?;
            sym_data_map.insert(sym.clone(), SymData {
                close: close_col.into_iter().filter_map(|x| x).take(n_min).collect(),
                high:  high_col.into_iter().filter_map(|x| x).take(n_min).collect(),
                low:   low_col.into_iter().filter_map(|x| x).take(n_min).collect(),
                vol:   vol_col.into_iter().filter_map(|x| x).take(n_min).collect(),
            });
        }
    }
    eprintln!("Loaded {} symbols, {} bars\n", sym_data_map.len(), n);

    // Stats per VOL_LOOKBACK
    let mut vb_stats: HashMap<usize, VbStats> = HashMap::new();
    for &vb in VOL_LOOKBACK_VALS {
        vb_stats.insert(vb, VbStats {
            sharpes: vec![],
            rets: vec![],
            pass_count: 0,
            total_count: 0,
            worst_dd: 0.0,
        });
    }

    // Aggregate equity per VOL_LOOKBACK
    let mut step_equities: HashMap<usize, Vec<f64>> = HashMap::new();
    for &vb in VOL_LOOKBACK_VALS {
        step_equities.insert(vb, vec![1.0_f64]);
    }

    // Metrics CSV
    let csv_path = "snapshots/vol_lookback_hyperopt.csv";
    let mut csv_lines = vec!["vol_lookback,universe,window,ret_pct,sharpe,max_dd_pct,trades,win_rate_pct,pass".to_string()];

    for &(label, symbols) in UNIVERSES {
        let symbols: Vec<String> = symbols.iter().map(|s| s.to_string()).collect();
        if !symbols.iter().all(|s| sym_data_map.contains_key(s)) { continue; }

        let total_windows = n.saturating_sub(TRAIN_BARS + TEST_BARS) / TEST_BARS;
        if total_windows == 0 { continue; }

        for wi in 0..total_windows {
            let test_start = TRAIN_BARS + wi * TEST_BARS;
            let test_end = (test_start + TEST_BARS).min(n);
            if test_end.saturating_sub(test_start) < 5 { continue; }

            for &vb in VOL_LOOKBACK_VALS {
                let r = run_sim(&sym_data_map, &symbols, test_start, test_end, vb);

                csv_lines.push(format!(
                    "{},{},{},{:.2},{:.4},{:.2},{},{:.2},{}",
                    vb, label, wi, r.ret, r.sharpe, r.max_dd, r.trades, r.win_rate, r.pass
                ));

                let stats = vb_stats.get_mut(&vb).unwrap();
                stats.sharpes.push(r.sharpe);
                stats.rets.push(r.ret);
                stats.total_count += 1;
                if r.pass { stats.pass_count += 1; }
                if r.max_dd > stats.worst_dd { stats.worst_dd = r.max_dd; }

                let agg = step_equities.get_mut(&vb).unwrap();
                for &eq_val in &r.equity_curve {
                    let last = *agg.last().unwrap_or(&1.0);
                    agg.push(last * eq_val);
                }
            }
        }
    }

    let mut f = File::create(csv_path)?;
    for line in &csv_lines { writeln!(f, "{}", line)?; }
    eprintln!("  Metrics CSV: {}\n", csv_path);

    // Write aggregate equity CSV
    let combined_path = "snapshots/vol_lookback_aggregate.csv";
    let max_steps = step_equities.values().map(|v| v.len()).fold(0, |a, b| a.max(b));
    let mut cf = File::create(combined_path)?;
    let header = format!("step,{}", VOL_LOOKBACK_VALS.iter().map(|x| format!("vb_{}", x)).collect::<Vec<_>>().join(","));
    writeln!(cf, "{}", header)?;
    for step in 0..max_steps {
        let mut row = vec![format!("{}", step)];
        for &vb in VOL_LOOKBACK_VALS {
            let agg = step_equities.get(&vb).unwrap();
            let eq = if step < agg.len() { agg[step] } else { *agg.last().unwrap() };
            row.push(format!("{:.6}", eq));
        }
        writeln!(cf, "{}", row.join(","))?;
    }
    eprintln!("  Aggregate equity CSV: {}\n", combined_path);

    // Summary
    eprintln!("{}", "=".repeat(70));
    eprintln!("  Volume Ranking Lookback Hyperopt Summary — Turtle+Chandelier");
    eprintln!("  9 universes x 6 windows = 54 windows per VOL_LOOKBACK value");
    eprintln!("{}", "=".repeat(70));
    eprintln!("{:>6} | {:>8} | {:>10} | {:>10} | {:>10} | {:>10}", "VB", "PassRate", "AvgSharpe", "AvgRet", "WorstDD", "Pass/Total");
    eprintln!("{}", "-".repeat(70));

    let mut best_vb = 1usize;
    let mut best_pass_rate = 0.0f64;
    let mut best_sharpe = f64::NEG_INFINITY;

    for &vb in VOL_LOOKBACK_VALS {
        let stats = vb_stats.get(&vb).unwrap();
        let avg_sharpe: f64 = stats.sharpes.iter().sum::<f64>() / stats.sharpes.len().max(1) as f64;
        let avg_ret: f64 = stats.rets.iter().sum::<f64>() / stats.rets.len().max(1) as f64;
        let pr = stats.pass_count as f64 / stats.total_count as f64 * 100.0;

        eprintln!(
            "{:>6} | {:>7.1}% | {:>10.4} | {:>+9.1}% | {:>9.1}% | {}/{}",
            vb, pr, avg_sharpe, avg_ret, stats.worst_dd, stats.pass_count, stats.total_count
        );

        if pr > best_pass_rate { best_pass_rate = pr; best_vb = vb; }
        if avg_sharpe > best_sharpe { best_sharpe = avg_sharpe; }
    }

    eprintln!("{}", "-".repeat(70));

    let curr_stats = vb_stats.get(&1).unwrap();
    let curr_pr = curr_stats.pass_count as f64 / curr_stats.total_count as f64 * 100.0;
    let curr_avg_sh = curr_stats.sharpes.iter().sum::<f64>() / curr_stats.sharpes.len().max(1) as f64;
    let winner_stats = vb_stats.get(&best_vb).unwrap();
    let winner_pr = winner_stats.pass_count as f64 / winner_stats.total_count as f64 * 100.0;
    let winner_avg_sh = winner_stats.sharpes.iter().sum::<f64>() / winner_stats.sharpes.len().max(1) as f64;

    eprintln!("  Current default (VB=1, no smoothing): pass={:.1}%, Sharpe={:.4}", curr_pr, curr_avg_sh);
    eprintln!("  Winner       (VB={:02}):               pass={:.1}%, Sharpe={:.4}", best_vb, winner_pr, winner_avg_sh);
    if best_vb != 1 {
        eprintln!("  DELTA: {:+.1}% pass rate, {:+.4} Sharpe", winner_pr - curr_pr, winner_avg_sh - curr_avg_sh);
    } else {
        eprintln!("  No improvement — current bar volume (VB=1) is already optimal");
    }
    eprintln!("\n  Runtime: {:?}", t0.elapsed());

    Ok(())
}