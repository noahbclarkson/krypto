//! FRESHNESS_COOLDOWN Hyperopt Sweep
//!
//! Parameter: FRESHNESS_COOLDOWN (cd) — bars to wait after exit before re-entry
//! Range: 0 to 70 in steps of 5 (15 values)
//! Universe: Base5 (BTC, ETH, SOL, XRP, DOGE, ADA) — production universe
//! Windows: 6 (all available)
//!
//! GAP DISCOVERED: The walk-forward harness has NO freshness filter, but
//! the live bot (bot.rs) uses cd=10. This creates a live/backtest gap.
//! This harness closes that gap and finds the optimal cd value.
//!
//! Key question: Is cd=10 (current live default) actually optimal OOS?

use anyhow::Result;
use krypto::data::loader::DataLoader;
use std::collections::HashMap;
use std::fs::File;
use std::io::Write;
use std::time::Instant;

const CANDLES: u32 = 3000;
const TRAIN_BARS: usize = 252;
const TEST_BARS: usize = 252;
// --- Frozen strategy params (from turtle_chandelier_walkforward.rs) ---
const TURTLE_ENTRY: usize = 21;
const CHAND_PERIOD: usize = 20;
const CHAND_MULT: f64 = 2.15;
const TURTLE_ATR_PERIOD: usize = 24;
const TURTLE_ATR_MULT: f64 = 2.00;
const HOLD_MAX: usize = 45;
const POSITION_CAP: usize = 3;
const TAKER_FEE: f64 = 0.001;
const MIN_TRADES: usize = 3;
const VOL_LOOKBACK: usize = 2;

// Base5 — production universe
const SYMBOLS: &[&str] = &["BTCUSDT", "ETHUSDT", "SOLUSDT", "XRPUSDT", "DOGEUSDT", "ADAUSDT"];

// FRESHNESS_COOLDOWN sweep values: 0 to 70 step 5
const CD_VALUES: &[usize] = &[0, 5, 10, 15, 20, 25, 30, 35, 40, 45, 50, 55, 60, 65, 70];
const CD_COUNT: usize = 15;

const CSV_OUT: &str = "snapshots/freshness_sweep.csv";
const EQUITY_CSV: &str = "snapshots/freshness_sweep_equity.csv";

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
    equity_curve: Vec<f64>,
}

/// Run simulation with a given freshness_cooldown value.
/// Returns equity curve AND summary metrics.
fn run_sim_with_cd(
    sym_data: &HashMap<String, SymData>,
    symbols: &[String],
    test_start: usize,
    test_end: usize,
    freshness_cd: usize,
) -> WfResult {
    let mut equity = 1.0_f64;
    let mut equity_curve = vec![1.0_f64];
    let mut peak = equity;
    let mut wins = 0usize;
    let mut total_trades = 0usize;
    let mut daily_rets = Vec::new();
    // Freshness filter: last exit bar per symbol
    let mut last_exit_bar: HashMap<String, usize> = HashMap::new();

    let mut bar = test_start;
    while bar + 2 < test_end {
        // Rank symbols by dollar volume
        let mut scores: Vec<(&str, f64)> = Vec::new();
        for sym in symbols {
            if let Some(sd) = sym_data.get(sym) {
                if bar >= sd.close.len() { continue; }
                // Freshness check per symbol
                if freshness_cd > 0 {
                    if let Some(&last_exit) = last_exit_bar.get(sym) {
                        if bar.saturating_sub(last_exit) < freshness_cd {
                            continue; // skip this symbol (not fresh)
                        }
                    }
                }
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

        // Turtle breakout entry (no regime filter)
        let mut entered = false;
        for sym in &top_syms {
            if let Some(sd) = sym_data.get(sym) {
                if bar >= TURTLE_ENTRY + 1 && bar < sd.close.len() {
                    // EP check: close >= max(close over last EP bars)
                    let start = bar + 1 - TURTLE_ENTRY;
                    let mut max_close = f64::NEG_INFINITY;
                    for i in start..bar {
                        if let Some(&c) = sd.close.get(i) {
                            max_close = max_close.max(c);
                        }
                    }
                    if let Some(&curr_close) = sd.close.get(bar) {
                        if curr_close > max_close {
                            // Freshness already checked above (per-symbol)
                            let entry_px = curr_close;
                            let entry = entry_px * (1.0 - TAKER_FEE);
                            let entry_bar_next = bar + 1;
                            let n = sd.close.len();

                            // DUAL_EXIT: Chandelier OR Turtle ATR — whichever fires first
                            let mut highest_high_chand = sd.high[entry_bar_next.min(n - 1)];
                            let mut highest_high_turtle = sd.high[entry_bar_next.min(n - 1)];
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
                                // Record exit bar for freshness filter
                                last_exit_bar.insert(sym.clone(), exit_bar);
                                bar = exit_bar + 1;
                                entered = true;
                                break;
                            }
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

    WfResult { ret, sharpe, max_dd, trades: total_trades, win_rate, pass, equity_curve }
}

#[tokio::main]
async fn main() -> Result<()> {
    let t0 = Instant::now();
    println!("==== FRESHNESS_COOLDOWN Hyperopt Sweep ====");
    println!("cd values: {:?}", CD_VALUES);
    println!("Universe: Base5 | Params: EP={}, Chand({},{}), ATR({},{}), HM={}, CAP={}",
             TURTLE_ENTRY, CHAND_PERIOD, CHAND_MULT, TURTLE_ATR_PERIOD, TURTLE_ATR_MULT, HOLD_MAX, POSITION_CAP);
    println!();

    let loader = DataLoader::new(None, None);

    // Load all Base5 symbols
    let mut sym_data_map: HashMap<String, SymData> = HashMap::new();
    let mut min_len = usize::MAX;
    for &sym in SYMBOLS {
        match loader.fetch_with_cache(sym, "1d", CANDLES).await {
            Ok(df) => {
                let n = df.height().min(2800);
                min_len = min_len.min(n);
                macro_rules! col_vec {
                    ($name:expr) => {{
                        let chunked = df.column($name)?.f64()?;
                        chunked.into_iter().filter_map(|x| x).take(n).collect::<Vec<_>>()
                    }};
                }
                sym_data_map.insert(sym.to_string(), SymData {
                    close: col_vec!("close"),
                    high:  col_vec!("high"),
                    low:   col_vec!("low"),
                    vol:   col_vec!("volume"),
                });
                println!("  Loaded {} ({} bars)", sym, n);
            }
            Err(e) => { eprintln!("  WARNING: {} load failed: {}", sym, e); }
        }
    }

    let n = min_len;
    let symbols: Vec<String> = SYMBOLS.iter().map(|s| s.to_string()).collect();
    let total_windows = n.saturating_sub(TRAIN_BARS + TEST_BARS) / TEST_BARS;
    println!("\nTotal bars: {}, {} windows\n", n, total_windows);

    // ---- Run sweep ----
    // Results: cd -> (total_pass, total_windows, avg_sharpe, avg_ret, avg_dd, total_trades, equity_curve_final)
    let mut results: Vec<(usize, usize, f64, f64, f64, usize, Vec<f64>)> = Vec::with_capacity(CD_COUNT);

    for &cd in CD_VALUES {
        let mut total_pass = 0usize;
        let mut sum_ret = 0.0_f64;
        let mut sum_sharpe = 0.0_f64;
        let mut sum_max_dd = 0.0_f64;
        let mut total_trades = 0usize;
        let mut window_equity_curves: Vec<Vec<f64>> = Vec::new();

        for wi in 0..total_windows {
            let train_end = TRAIN_BARS + wi * TEST_BARS;
            let test_start = train_end;
            let test_end = (test_start + TEST_BARS).min(n);
            if test_end.saturating_sub(test_start) < 5 { continue; }

            let r = run_sim_with_cd(&sym_data_map, &symbols, test_start, test_end, cd);
            sum_ret += r.ret;
            sum_sharpe += r.sharpe;
            sum_max_dd += r.max_dd;
            total_trades += r.trades;
            if r.pass { total_pass += 1; }
            window_equity_curves.push(r.equity_curve);
        }

        let nw = total_windows as f64;
        let avg_sharpe = sum_sharpe / nw;
        let avg_ret = sum_ret / nw;
        let avg_dd = sum_max_dd / nw;
        let pass_pct = total_pass as f64 / nw * 100.0;

        // Build composite equity curve (concatenate all windows)
        let mut composite = Vec::new();
        for ec in &window_equity_curves {
            composite.extend_from_slice(ec);
        }
        results.push((total_pass, total_windows, avg_sharpe, avg_ret, avg_dd, total_trades, composite));

        println!("cd={:2} | {:2}/{:2} pass ({:.1}) | avg_sharpe={} | avg_ret={} | avg_DD={} | {} trades",
            cd, total_pass, total_windows,
            format!("{:.1}", pass_pct),
            format!("{:.4}", avg_sharpe),
            format!("{:+.2}", avg_ret),
            format!("{:.2}", avg_dd),
            total_trades
        );
    }

    // Find winner by avg Sharpe
    let mut best_idx = 0usize;
    let mut best_sharpe = f64::NEG_INFINITY;
    for (i, r) in results.iter().enumerate() {
        if r.2 > best_sharpe {
            best_sharpe = r.2;
            best_idx = i;
        }
    }
    let winner_cd = CD_VALUES[best_idx];
    println!("\nWINNER: cd={} avg_sharpe={}", winner_cd, format!("{:.4}", best_sharpe));

    // ---- Write summary CSV ----
    {
        let mut f = File::create(CSV_OUT)?;
        writeln!(f, "cd,pass,total,pass_pct,avg_sharpe,avg_ret,avg_dd,total_trades")?;
        for (i, r) in results.iter().enumerate() {
            let cd = CD_VALUES[i];
            writeln!(f, "{},{},{},{},{},{},{},{}",
                     cd, r.0, r.1,
                     format!("{:.2}", r.0 as f64 / r.1 as f64 * 100.0),
                     format!("{:.4}", r.2),
                     format!("{:.2}", r.3),
                     format!("{:.2}", r.4),
                     r.5)?;
        }
        println!("Wrote: {}", CSV_OUT);
    }

    // ---- Write equity curve CSV ----
    {
        let mut f = File::create(EQUITY_CSV)?;
        writeln!(f, "cd,bar_idx,equity")?;
        for (i, r) in results.iter().enumerate() {
            let cd = CD_VALUES[i];
            for (bi, &eq) in r.6.iter().enumerate() {
                writeln!(f, "{},{},{}", cd, bi, format!("{:.6}", eq))?;
            }
        }
        println!("Wrote: {}", EQUITY_CSV);
    }

    println!("\nRuntime: {:?}", t0.elapsed());
    println!("\nNote: Freshness filter is applied PER-SYMBOL.");
    println!("  After exit at bar B, symbol S cannot be entered again until bar B + cd.");
    println!("  cd=0 means NO freshness filter (equivalent to current walk-forward harness).");
    println!("  cd=10 is the current live bot default (from hyperopt 2026-04-18).");

    Ok(())
}
