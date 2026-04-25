//! ATR_ENTRY_MULT Hyperparameter Sweep with Equity Curve Export
//!
//! Sweeps ATR_ENTRY_MULT ∈ [0.00..2.00 step 0.05] = 41 values
//! × 9 universes × up to 6 windows = ~2214 runs
//! Exports time-series equity curves for key configs

use anyhow::Result;
use krypto::data::loader::DataLoader;
use polars::prelude::*;
use std::collections::HashMap;
use std::path::Path;
use std::time::Instant;

const TRAIN_BARS: usize = 252;
const TEST_BARS: usize = 252;
const HOLD_MAX: usize = 12;
const TAKER_FEE: f64 = 0.001;
const POSITION_CAP: usize = 3;
const MIN_TRADES: usize = 3;

// Production params (frozen during this sweep)
const CHAND_PERIOD: usize = 7;
const CHAND_MULT: f64 = 2.30;
const TURTLE_ENTRY: usize = 24;
const TURTLE_ATR_PERIOD: usize = 24;
const TURTLE_ATR_MULT: f64 = 2.00;
const VOL_LOOKBACK: usize = 1;

// Sweep range
const ATR_EM_MIN: f64 = 0.00;
const ATR_EM_MAX: f64 = 2.00;
const ATR_EM_STEP: f64 = 0.05; // 41 values

// Equity curve export configs
const EQUITY_CONFIGS: &[f64] = &[0.00, 0.50, 0.85, 0.90, 1.00, 1.50];

const UNIVERSES: &[(&str, &[&str])] = &[
    ("Base5",        &["BTCUSDT","ETHUSDT","SOLUSDT","XRPUSDT","DOGEUSDT","ADAUSDT"]),
    ("NoDOGE",       &["BTCUSDT","ETHUSDT","SOLUSDT","XRPUSDT","ADAUSDT"]),
    ("LargeCaps5",   &["BTCUSDT","ETHUSDT","BNBUSDT","SOLUSDT","XRPUSDT"]),
    ("LowVolume5",   &["LTCUSDT","EOSUSDT","BCHUSDT","LINKUSDT","AVAXUSDT"]),
    ("Legacy3",      &["BTCUSDT","XRPUSDT","LTCUSDT"]),
    ("Legacy4",      &["BTCUSDT","XRPUSDT","LTCUSDT","ETHUSDT"]),
    ("Legacy5",      &["BTCUSDT","XRPUSDT","LTCUSDT","ETHUSDT","ADAUSDT"]),
    ("OldGuard4",    &["BTCUSDT","ETHUSDT","XRPUSDT","LTCUSDT"]),
    ("OldGuard5",    &["BTCUSDT","ETHUSDT","XRPUSDT","LTCUSDT","ADAUSDT"]),
];

const SWEEP_CSV:      &str = "snapshots/atr_entry_mult_sweep.csv";
const SWEEP_MD:       &str = "snapshots/atr_entry_mult_sweep.md";
const EQUITY_CSV:     &str = "snapshots/atr_entry_mult_sweep_equity.csv";
const MEAN_EQUITY_CSV: &str = "snapshots/atr_entry_mult_mean_equity.csv";
const SUMMARY_CSV:    &str = "snapshots/atr_entry_mult_sweep_summary.csv";

struct SymData {
    close: Vec<f64>,
    high: Vec<f64>,
    low: Vec<f64>,
    vol: Vec<f64>,
}

fn col_vec(df: &DataFrame, name: &str, limit: usize) -> Result<Vec<f64>> {
    let chunked = df.column(name)?.f64()?;
    Ok(chunked.into_iter().filter_map(|x| x).take(limit).collect())
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

fn turtle_signal(close: &[f64], high: &[f64], low: &[f64],
                 entry_period: usize, atr_period: usize, atr_mult: f64, idx: usize) -> bool {
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

fn max_dd_from(curve: &[f64]) -> f64 {
    let mut peak = f64::NEG_INFINITY;
    let mut max_dd = 0.0_f64;
    for &v in curve {
        if v > peak { peak = v; }
        let dd = (peak - v) / peak;
        if dd > max_dd { max_dd = dd; }
    }
    max_dd * 100.0
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

fn run_window(
    symbols: &[&str],
    sym_data: &HashMap<&str, SymData>,
    test_start: usize,
    test_end: usize,
    atr_em: f64,
) -> WfResult {
    let mut equity = 1.0_f64;
    let mut equity_curve = vec![1.0_f64];
    let mut peak = equity;
    let mut wins = 0usize;
    let mut total_trades = 0usize;

    let mut bar = test_start;
    while bar + 2 < test_end {
        let mut scores: Vec<(&str, f64)> = Vec::new();
        for &sym in symbols {
            if let Some(sd) = sym_data.get(sym) {
                if bar >= sd.close.len() { continue; }
                let rol_vol = rolling_avg(&sd.vol, VOL_LOOKBACK, bar);
                let price = *sd.close.get(bar).unwrap_or(&0.0);
                let dv = rol_vol * price;
                scores.push((sym, if dv.is_finite() && dv > 0.0 { dv } else { 0.0 }));
            }
        }
        scores.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());
        let top_syms: Vec<&str> = scores.into_iter().take(POSITION_CAP).map(|(s, _)| s).collect();

        if top_syms.is_empty() {
            equity_curve.push(equity);
            bar += 1;
            continue;
        }

        let mut entered = false;
        for &sym in &top_syms {
            if let Some(sd) = sym_data.get(sym) {
                if bar >= TURTLE_ENTRY + 1 && bar < sd.close.len() {
                    if turtle_signal(&sd.close, &sd.high, &sd.low,
                                     TURTLE_ENTRY, TURTLE_ATR_PERIOD, atr_em, bar) {
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
                            let atr_chand = atr_at(&sd.high, &sd.low, &sd.close, CHAND_PERIOD, b);
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
    let sharpe = annualised_sharpe(
        &equity_curve.windows(2).map(|w| w[1] / w[0] - 1.0).collect::<Vec<_>>()
    );
    let max_dd = max_dd_from(&equity_curve);
    let win_rate = if total_trades > 0 { wins as f64 / total_trades as f64 * 100.0 } else { 0.0 };
    let pass = total_trades >= MIN_TRADES && ret > 0.0;

    WfResult { ret, sharpe, max_dd, trades: total_trades, win_rate, pass, equity_curve }
}

fn main_() -> Result<()> {
    let t0 = Instant::now();

    // Collect all ATR values
    let mut atr_values: Vec<f64> = vec![];
    let mut v = ATR_EM_MIN;
    while v <= ATR_EM_MAX + 1e-9 {
        atr_values.push((v * 100.0).round() / 100.0);
        v += ATR_EM_STEP;
    }
    let n_atr = atr_values.len();

    eprintln!("==== ATR_ENTRY_MULT Sweep: {:.2}..{:.2} step {:.2} ({} values) ====",
              ATR_EM_MIN, ATR_EM_MAX, ATR_EM_STEP, n_atr);
    eprintln!("Params: EP={}, Chand({},{}), ATR({},{}), HM={}, CAP={}",
              TURTLE_ENTRY, CHAND_PERIOD, CHAND_MULT, TURTLE_ATR_PERIOD, TURTLE_ATR_MULT, HOLD_MAX, POSITION_CAP);
    eprintln!("Universes: {}, Windows: up to 6", UNIVERSES.len());

    // ── Load data ──────────────────────────────────────────────────────────
    // DataLoader uses cache_key(symbol, interval) = "data/cache/{symbol}_{interval}.parquet"
    // e.g. "BTCUSDT" + "1d" → "data/cache/btcusdt_1d.parquet"
    eprint!("Loading parquet files... ");

    let mut all_sym_data: HashMap<&str, HashMap<&str, SymData>> = HashMap::new();

    for &(uname, symbols) in UNIVERSES {
        let mut sym_map = HashMap::new();
        for &sym in symbols {
            // DataLoader.load_parquet(path) expects a Path
            let path = Path::new("data/cache").join(format!("{}_1d.parquet", sym.to_lowercase()));
            match DataLoader::load_parquet(&path) {
                Ok(df) => {
                    // Limit to last 2800 bars for consistent data length
                    let n = df.height().min(2800);
                    if let (Ok(close), Ok(high), Ok(low), Ok(vol)) =
                        (col_vec(&df, "close", n),
                         col_vec(&df, "high", n),
                         col_vec(&df, "low", n),
                         col_vec(&df, "volume", n))
                    {
                        sym_map.insert(sym, SymData { close, high, low, vol });
                    }
                }
                Err(e) => { eprintln!("WARNING: {} load failed: {}", sym, e); }
            }
        }
        all_sym_data.insert(uname, sym_map);
    }
    eprintln!("done ({}/{} universes loaded)",
        all_sym_data.iter().filter(|(_, m)| !m.is_empty()).count(), UNIVERSES.len());

    // ── Global accumulators ─────────────────────────────────────────────────
    // Using String keys to avoid f64 Eq+Hash requirement
    let mut global_pass: HashMap<String, usize> = HashMap::new();
    let mut global_total: HashMap<String, usize> = HashMap::new();
    let mut global_sharpe_sum: HashMap<String, f64> = HashMap::new();
    let mut global_ret_sum: HashMap<String, f64> = HashMap::new();
    let mut global_dd_sum: HashMap<String, f64> = HashMap::new();
    let mut global_trades_sum: HashMap<String, usize> = HashMap::new();

    for &em in &atr_values {
        let key = format!("{:.2}", em);
        global_pass.insert(key.clone(), 0);
        global_total.insert(key.clone(), 0);
        global_sharpe_sum.insert(key.clone(), 0.0);
        global_ret_sum.insert(key.clone(), 0.0);
        global_dd_sum.insert(key.clone(), 0.0);
        global_trades_sum.insert(key.clone(), 0);
    }

    // Equity curves storage for EQUITY_CONFIGS
    let mut equity_curves: HashMap<String, HashMap<String, Vec<Vec<f64>>>> = HashMap::new();
    for &em in EQUITY_CONFIGS {
        let key = format!("{:.2}", em);
        let mut u_map = HashMap::new();
        for &(uname, _) in UNIVERSES {
            u_map.insert(uname.to_string(), vec![]);
        }
        equity_curves.insert(key, u_map);
    }

    let mut detail_lines: Vec<String> = vec![
        "atr_entry_mult,universe,window,train_end,test_end,ret_pct,sharpe,trades,win_rate_pct,max_dd_pct,pass".to_string()
    ];

    // ── Run sweep ──────────────────────────────────────────────────────────
    let total_runs = UNIVERSES.len() * 6 * n_atr;
    let mut run_idx = 0usize;

    for &(uname, symbols) in UNIVERSES {
        eprintln!("\n--- Universe: {} ---", uname);

        if let Some(sym_map) = all_sym_data.get(uname) {
            let min_len = symbols.iter()
                .filter_map(|s| sym_map.get(s))
                .map(|sd| sd.close.len())
                .min()
                .unwrap_or(0);

            let n_windows = (min_len.saturating_sub(TRAIN_BARS)) / TEST_BARS;
            let n_windows = n_windows.min(6);

            for w in 0..n_windows {
                let train_end = TRAIN_BARS + w * TEST_BARS;
                let test_start = train_end;
                let test_end = (train_end + TEST_BARS).min(min_len);

                if test_end <= test_start + 30 { continue; }

                for &em in &atr_values {
                    let result = run_window(symbols, sym_map, test_start, test_end, em);
                    let key = format!("{:.2}", em);

                    *global_total.get_mut(&key).unwrap() += 1;
                    if result.pass { *global_pass.get_mut(&key).unwrap() += 1; }
                    *global_sharpe_sum.get_mut(&key).unwrap() += result.sharpe;
                    *global_ret_sum.get_mut(&key).unwrap() += result.ret;
                    *global_dd_sum.get_mut(&key).unwrap() += result.max_dd;
                    *global_trades_sum.get_mut(&key).unwrap() += result.trades;

                    if EQUITY_CONFIGS.contains(&em) {
                        if let Some(u_map) = equity_curves.get_mut(&key) {
                            if let Some(curves) = u_map.get_mut(uname) {
                                curves.push(result.equity_curve);
                            }
                        }
                    }

                    detail_lines.push(format!("{:.2},{},{},{},{},{:0.4},{:0.4},{},{:.2},{:0.4},{}",
                        em, uname, w, train_end, test_end,
                        result.ret, result.sharpe, result.trades,
                        result.win_rate, result.max_dd,
                        if result.pass { "true" } else { "false" }));
                }

                run_idx += n_atr;
                eprint!("\r  W{} [{:.0}%][{} runs]  ", w,
                        run_idx as f64 / total_runs as f64 * 100.0, run_idx);
            }
        }
    }
    eprintln!();

    // ── Build summaries ────────────────────────────────────────────────────
    let mut summary_data: Vec<(f64, usize, usize, f64, f64, f64, f64, usize)> = vec![];

    for &em in &atr_values {
        let key = format!("{:.2}", em);
        let pass = *global_pass.get(&key).unwrap();
        let total = *global_total.get(&key).unwrap();
        let pass_pct = if total > 0 { pass as f64 / total as f64 * 100.0 } else { 0.0 };
        let avg_sharpe = if total > 0 { *global_sharpe_sum.get(&key).unwrap() / total as f64 } else { 0.0 };
        let avg_ret = if total > 0 { *global_ret_sum.get(&key).unwrap() / total as f64 } else { 0.0 };
        let avg_dd = if total > 0 { *global_dd_sum.get(&key).unwrap() / total as f64 } else { 0.0 };
        let total_trades = *global_trades_sum.get(&key).unwrap();

        summary_data.push((em, pass, total, pass_pct, avg_sharpe, avg_ret, avg_dd, total_trades));
    }

    summary_data.sort_by(|a, b| b.4.partial_cmp(&a.4).unwrap());

    // ── Write sweep CSV ────────────────────────────────────────────────────
    std::fs::write(SWEEP_CSV, detail_lines.join("\n"))?;
    eprintln!("Wrote {} ({} rows)", SWEEP_CSV, detail_lines.len() - 1);

    // ── Write summary CSV ───────────────────────────────────────────────────
    let mut summary_lines: Vec<String> = vec![
        "atr_entry_mult,global_pass,global_total,pass_pct,avg_sharpe,avg_return_pct,avg_max_dd_pct,total_trades".to_string()
    ];
    for (em, pass, total, pass_pct, avg_sharpe, avg_ret, avg_dd, trades) in &summary_data {
        summary_lines.push(format!("{:.2},{},{},{:.2},{:0.4},{:0.4},{:.2},{}",
            em, pass, total, pass_pct, avg_sharpe, avg_ret, avg_dd, trades));
    }
    std::fs::write(SUMMARY_CSV, summary_lines.join("\n"))?;
    eprintln!("Wrote {} ({} ATR values)", SUMMARY_CSV, summary_data.len());

    // ── Write mean equity curves ──────────────────────────────────────────
    let mut mean_eq_lines: Vec<String> = vec!["atr_entry_mult,bar,mean_equity".to_string()];
    for &em in EQUITY_CONFIGS {
        let key = format!("{:.2}", em);
        if let Some(u_map) = equity_curves.get(&key) {
            let all_curves: Vec<&Vec<f64>> = u_map.values().flat_map(|v| v.iter()).collect();
            if all_curves.is_empty() { continue; }
            let max_len = all_curves.iter().map(|c| c.len()).max().unwrap();

            for step in (0..max_len).step_by(2) {
                let mut sum = 0.0_f64;
                let mut count = 0usize;
                for curve in &all_curves {
                    if let Some(&v) = curve.get(step) { sum += v; count += 1; }
                }
                let mean = if count > 0 { sum / count as f64 } else { 1.0 };
                mean_eq_lines.push(format!("{:.2},{},{:0.8}", em, step, mean));
            }
        }
    }
    std::fs::write(MEAN_EQUITY_CSV, mean_eq_lines.join("\n"))?;
    eprintln!("Wrote {} (for {} configs)", MEAN_EQUITY_CSV, EQUITY_CONFIGS.len());

    // ── Write full equity curves CSV ──────────────────────────────────────
    let mut eq_lines: Vec<String> = vec!["atr_entry_mult,universe,window_idx,bar_step,equity".to_string()];
    for &em in EQUITY_CONFIGS {
        let key = format!("{:.2}", em);
        if let Some(u_map) = equity_curves.get(&key) {
            for (uname, curves) in u_map {
                for (wi, curve) in curves.iter().enumerate() {
                    for (bi, &eq) in curve.iter().enumerate() {
                        eq_lines.push(format!("{:.2},{},{},{},{:0.8}", em, uname, wi, bi, eq));
                    }
                }
            }
        }
    }
    std::fs::write(EQUITY_CSV, eq_lines.join("\n"))?;
    eprintln!("Wrote {} ({} lines)", EQUITY_CSV, eq_lines.len());

    // ── Print top results ─────────────────────────────────────────────────
    eprintln!("\n=== TOP 15 ATR_ENTRY_MULT by avg Sharpe ===");
    eprintln!("{:>8} {:>6} {:>7} {:>12} {:>12} {:>10} {:>10}",
             "ATR_EM", "PASS", "TOTAL", "AVG_SHARPE", "AVG_RET%", "AVG_MAXDD%", "TRADES");
    for (em, pass, total, pass_pct, avg_sharpe, avg_ret, avg_dd, trades) in summary_data.iter().take(15) {
        eprintln!("{:>8.2} {:>6} {:>7} {:>12.4} {:>12.4} {:>10.2} {:>10}",
                 em, pass, total, avg_sharpe, avg_ret, avg_dd, trades);
    }

    // ── Write markdown report ─────────────────────────────────────────────
    let mut md = format!(
        "# ATR_ENTRY_MULT Hyperparameter Sweep\n\n\
        **Date:** 2026-04-25\n\
        **Range:** ATR_ENTRY_MULT ∈ [{:.2}..{:.2}] step {:.2} ({} values)\n\
        **Universes:** 9 × up to 6 windows\n\
        **Frozen params:** EP={}, Chand({},{}), ATR({},{}), HM={}, CAP={}\n\n\
        ## Results (sorted by avg Sharpe)\n\n\
        | ATR_EM | Pass | Total | Pass% | Avg Sharpe | Avg Ret% | Avg DD% | Trades |\n\
        |--------|------|-------|-------|------------|----------|---------|--------|\n",
        ATR_EM_MIN, ATR_EM_MAX, ATR_EM_STEP, n_atr,
        TURTLE_ENTRY, CHAND_PERIOD, CHAND_MULT, TURTLE_ATR_PERIOD, TURTLE_ATR_MULT, HOLD_MAX, POSITION_CAP
    );
    for (em, pass, total, pass_pct, avg_sharpe, avg_ret, avg_dd, trades) in summary_data.iter().take(20) {
        md.push_str(&format!("| {:.2} | {} | {} | {:.1}% | {:0.4} | {:0.2}% | {:0.2}% | {} |\n",
            em, pass, total, pass_pct, avg_sharpe, avg_ret, avg_dd, trades));
    }
    md.push_str("\n## Charts\n\n");
    md.push_str("- Equity curves: `charts/atr_entry_mult_sweep_comparison.png`\n");
    md.push_str("- Mean equity: `snapshots/atr_entry_mult_mean_equity.csv`\n");
    md.push_str("- Sweep data: `snapshots/atr_entry_mult_sweep.csv`\n\n");

    if let Some((best_em, best_pass, _, best_pass_pct, best_sharpe, best_ret, best_dd, best_trades)) = summary_data.first() {
        md.push_str(&format!(
            "**Winner:** ATR_ENTRY_MULT={:.2} — {}% pass, Sharpe={:0.4}, Avg Ret={:0.2}%, DD={:0.2}%, {} trades\n\n",
            best_em, best_pass_pct, best_sharpe, best_ret, best_dd, best_trades
        ));
    }

    std::fs::write(SWEEP_MD, &md)?;
    eprintln!("Wrote {}", SWEEP_MD);

    let elapsed = t0.elapsed();
    eprintln!("\n=== DONE in {:.1}s ===", elapsed.as_secs_f64());

    Ok(())
}

fn main() { main_().unwrap(); }