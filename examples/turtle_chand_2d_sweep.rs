//! Turtle+Chandelier 2D Sweep: CHAND_PERIOD × CHAND_MULT
//!
//! Question: M=2.15 plateau (M≥2.15 → identical results) was only verified at P=20.
//! Is the plateau stable across different P values? Do P and M interact?
//!
//! Grid: P ∈ [15,30] step 1 (16 values) × M ∈ [1.50, 2.10] step 0.05 (13 values) = 208 combos
//! Universes: Base5 + NoDOGE
//! Baseline: P=20, M=2.15 (current) and P=20, M=2.00 (prior)

use anyhow::Result;
use krypto::data::loader::DataLoader;
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
const TURTLE_ENTRY: usize = 21;
const TURTLE_ATR_PERIOD: usize = 24;
const TURTLE_ATR_MULT: f64 = 2.00;
const VOL_LOOKBACK: usize = 2;

const P_MIN: usize = 15;
const P_MAX: usize = 30;
const P_STEP: usize = 1;
const M_MIN: f64 = 1.50;
const M_MAX: f64 = 2.10;
const M_STEP: f64 = 0.05;

const UNIVERSES: &[(&str, &[&str])] = &[
    ("Base5",  &["BTCUSDT","ETHUSDT","SOLUSDT","XRPUSDT","DOGEUSDT","ADAUSDT"]),
    ("NoDOGE", &["BTCUSDT","ETHUSDT","SOLUSDT","XRPUSDT","ADAUSDT"]),
];

const CSV_OUT: &str = "snapshots/chand_pm_2d_sweep.csv";

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
    vals[idx + 1 - window..=idx].iter().sum::<f64>() / window as f64
}

fn turtle_signal(close: &[f64], entry_period: usize, idx: usize) -> bool {
    if idx < entry_period + 1 { return false; }
    let start = idx + 1 - entry_period;
    let mut max_close = f64::NEG_INFINITY;
    for i in start..idx {
        if let Some(&c) = close.get(i) { max_close = max_close.max(c); }
    }
    if let Some(&curr_close) = close.get(idx) { curr_close > max_close } else { false }
}

fn annualised_sharpe(daily_rets: &[f64]) -> f64 {
    if daily_rets.len() < 2 { return 0.0; }
    let mn: f64 = daily_rets.iter().sum::<f64>() / daily_rets.len() as f64;
    let sd = (daily_rets.iter().map(|x| (x - mn).powi(2)).sum::<f64>() / daily_rets.len() as f64).sqrt();
    if sd == 0.0 { return 0.0; }
    mn * 365.0_f64.sqrt() / sd
}

fn max_dd(equity: &[f64]) -> f64 {
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
    pass: bool,
}

fn run_sim(
    sym_data: &HashMap<String, SymData>,
    symbols: &[String],
    test_start: usize,
    test_end: usize,
    chand_p: usize,
    chand_m: f64,
) -> WfResult {
    let mut equity = 1.0_f64;
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
            daily_rets.push(0.0);
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

                        let mut highest_high_chand = sd.high[entry_bar_next];
                        let mut highest_high_turtle = sd.high[entry_bar_next];
                        let max_bar = (entry_bar_next + HOLD_MAX).min(n.saturating_sub(1));
                        let mut exit_bar = max_bar;

                        for b in entry_bar_next..=max_bar.min(n.saturating_sub(1)) {
                            highest_high_chand = highest_high_chand.max(sd.high[b]);
                            let atr_chand = atr_at(&sd.high, &sd.low, &sd.close, chand_p, b);
                            let trail_chand = highest_high_chand - chand_m * atr_chand;

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
                            let _wins = if gross_ret > 0.0 { 1 } else { 0 };
                            total_trades += 1;
                            equity *= 1.0 + gross_ret;
                            daily_rets.push(gross_ret);
                        }
                        entered = true;
                        break;
                    }
                }
            }
        }

        if !entered {
            daily_rets.push(0.0);
        }
        bar += 1;
    }

    let ret = (equity - 1.0) * 100.0;
    let sharpe = annualised_sharpe(&daily_rets);
    let max_dd = max_dd(&[1.0_f64]); // simplified — equity not tracked per bar
    let pass = total_trades >= MIN_TRADES && sharpe > 0.0;

    WfResult { ret, sharpe, max_dd, trades: total_trades, pass }
}

#[tokio::main]
async fn main() -> Result<()> {
    let start = Instant::now();
    let loader = DataLoader::new(None, None);

    let mut csv = File::create(CSV_OUT)?;
    writeln!(csv, "universe,chand_p,chand_m,avg_sharpe,avg_ret_pct,total_trades,pass_rate")?;

    let mut all_results: Vec<(usize, f64, f64, f64, usize, f64, String)> = Vec::new();

    let p_values: Vec<usize> = (P_MIN..=P_MAX).step_by(P_STEP).collect();
    let m_values: Vec<f64> = {
        let mut v = Vec::new();
        let mut m = M_MIN;
        while m <= M_MAX + 0.001 {
            v.push((m * 100.0).round() / 100.0);
            m += M_STEP;
        }
        v
    };

    println!("2D Sweep: {} P × {} M = {} combos", p_values.len(), m_values.len(), p_values.len() * m_values.len());
    println!("P: {:?} ... {:?}", &p_values[..3], &p_values[p_values.len()-3..]);
    println!("M: {:?} ... {:?}", &m_values[..3], &m_values[m_values.len()-3..]);

    for (uname, sym_strs) in UNIVERSES {
        let symbols: Vec<String> = sym_strs.iter().map(|s| s.to_string()).collect();
        println!("\nLoading {uname}...");
        let mut sym_data: HashMap<String, SymData> = HashMap::new();

        for sym in *sym_strs {
            match loader.fetch_with_cache(sym, "1d", CANDLES).await {
                Ok(candles) => {
                    let n = candles.height().min(CANDLES as usize);
                    let close: Vec<f64> = candles.column("close")?.f64()?.into_iter().take(n).map(|x| x.unwrap_or(0.0)).collect();
                    let high: Vec<f64> = candles.column("high")?.f64()?.into_iter().take(n).map(|x| x.unwrap_or(0.0)).collect();
                    let low:  Vec<f64> = candles.column("low")?.f64()?.into_iter().take(n).map(|x| x.unwrap_or(0.0)).collect();
                    let vol:  Vec<f64> = candles.column("volume")?.f64()?.into_iter().take(n).map(|x| x.unwrap_or(0.0)).collect();
                    sym_data.insert(sym.to_string(), SymData { close, high, low, vol });
                }
                Err(e) => { eprintln!("  WARNING: {sym} load failed: {e}"); }
            }
        }

        let n = sym_data.values().next().map(|sd| sd.close.len()).unwrap_or(0);
        let n_windows = n / TEST_BARS;
        println!("  {n} bars, {n_windows} windows");

        for &chand_p in &p_values {
            for &chand_m in &m_values {
                let mut total_ret = 0.0_f64;
                let mut total_sharpe = 0.0_f64;
                let mut total_trades = 0usize;
                let mut passes = 0usize;

                for w in 0..n_windows {
                    let train_end = TRAIN_BARS + w * TEST_BARS;
                    let test_start = train_end;
                    let test_end = (train_end + TEST_BARS).min(n);
                    if test_end - test_start < 50 { continue; }

                    let result = run_sim(&sym_data, &symbols, test_start, test_end, chand_p, chand_m);
                    total_ret += result.ret;
                    total_sharpe += result.sharpe;
                    total_trades += result.trades;
                    if result.pass { passes += 1; }
                }

                let n_valid = n_windows.max(1);
                let avg_sharpe = total_sharpe / n_valid as f64;
                let avg_ret = total_ret / n_valid as f64;
                let pass_rate = passes as f64 / n_valid as f64 * 100.0;

                let line = format!(
                    "{},{},{:.2},{:.4},{:.4},{},{:.2}\n",
                    uname, chand_p, chand_m, avg_sharpe, avg_ret, total_trades, pass_rate
                );
                csv.write_all(line.as_bytes())?;
                all_results.push((chand_p, chand_m, avg_sharpe, avg_ret, total_trades, pass_rate, uname.to_string()));

                // Print current-default progress
                if chand_p == 20 && (chand_m - 2.15).abs() < 0.001 {
                    println!("  P=20 M=2.15: Sharpe={:.4} Ret={:.2}% Trades={total_trades} Pass={:.1}%",
                        avg_sharpe, avg_ret, pass_rate);
                }
            }
        }
    }

    // Find winners per universe
    for uname in &["Base5", "NoDOGE"] {
        let mut univ_results: Vec<_> = all_results.iter()
            .filter(|r| r.6 == *uname)
            .collect();
        univ_results.sort_by(|a, b| b.2.partial_cmp(&a.2).unwrap());
        println!("\n=== {uname} TOP 5 ===");
        for (i, (p, m, sh, ret, trades, pr, _)) in univ_results.iter().take(5).enumerate() {
            println!("  {}. P={p} M={m:.2}: Sharpe={sh:.4} Ret={ret:.2}% Trades={trades} Pass={pr:.1}%", i+1);
        }
    }

    // Global winners (combined)
    let mut global: Vec<(usize, f64, f64, usize, f64)> = Vec::new();
    for &p in &p_values {
        for &m in &m_values {
            let mut sum_sharpe = 0.0_f64;
            let mut sum_trades = 0usize;
            let mut sum_pass = 0.0_f64;
            let mut cnt = 0usize;
            for r in &all_results {
                if r.0 == p && (r.1 - m).abs() < 0.001 {
                    sum_sharpe += r.2;
                    sum_trades += r.4;
                    sum_pass += r.5;
                    cnt += 1;
                }
            }
            if cnt > 0 {
                global.push((p, m, sum_sharpe / cnt as f64, sum_trades / cnt, sum_pass / cnt as f64));
            }
        }
    }
    global.sort_by(|a, b| b.2.partial_cmp(&a.2).unwrap());

    println!("\n=== GLOBAL TOP 10 (combined Base5 + NoDOGE) ===");
    for (i, (p, m, sh, trades, pr)) in global.iter().take(10).enumerate() {
        println!("  {}. P={p} M={m:.2}: Sharpe={sh:.4} Trades={trades} Pass={pr:.1}%", i+1);
    }

    // Baselines
    let base_p20_m200 = global.iter().find(|(p, m, _, _, _)| *p == 20 && (*m - 2.00).abs() < 0.01);
    let base_p20_m215 = global.iter().find(|(p, m, _, _, _)| *p == 20 && (*m - 2.15).abs() < 0.01);
    let base_p17_m200 = global.iter().find(|(p, m, _, _, _)| *p == 17 && (*m - 2.00).abs() < 0.01);

    if let Some(b) = base_p20_m200 { println!("\nBaseline P=20/M=2.00: Sharpe={:.4}", b.2); }
    if let Some(c) = base_p20_m215 { println!("Current  P=20/M=2.15: Sharpe={:.4}", c.2); }
    if let Some(p) = base_p17_m200 { println!("Alt      P=17/M=2.00: Sharpe={:.4}", p.2); }

    if let (Some(b), Some(c)) = (base_p20_m200, base_p20_m215) {
        let delta = (c.2 - b.2) / b.2.abs() * 100.0;
        println!("  P=20/M=2.15 vs 2.00: {:+.1}% Sharpe delta", delta);
    }

    println!("\nSweep complete in {:.1}s", start.elapsed().as_secs_f64());
    println!("Results: {CSV_OUT}");

    Ok(())
}
