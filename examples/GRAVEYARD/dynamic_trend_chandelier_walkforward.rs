//! DynamicTrend + Chandelier Dual-Exit Walk-Forward
//!
//! SIGNAL:   EMA fast/slow crossover + RSI filter  (from DynamicTrend research)
//! EXIT:     Chandelier(28, 2.15) dual ATR(24, 2.0)  (from Turtle+Chandelier production params)
//! PURPOSE:  Test whether EMA crossover signal outperforms Turtle breakout signal
//!            when paired with the same production-quality exit.
//!
//! HYPOTHESIS: If EMA crossover is genuinely different from Turtle breakout AND
//!             the Chandelier exit is the quality driver, then EMA signal + Chandelier
//!             should produce comparable (or better) Sharpe vs Turtle+Chandelier.
//!
//! UNIVERSES:  Base5, NoDOGE, LargeCaps5, Legacy4  (same as DynamicTrend hyperopt)
//! METHOD:     Walk-forward 252/252, OOS only
//! FEES:       20bp taker each side (conservative, same as Turtle harness)

use anyhow::Result;
use krypto::data::loader::DataLoader;
use krypto::features::indicators::FeatureEngine;

use std::collections::HashMap;
use std::fs::File;
use std::io::Write;
use std::time::Instant;

const CANDLES: u32 = 3000;
const TRAIN_BARS: usize = 252;
const TEST_BARS: usize = 252;
const HOLD_MAX: usize = 45;

// DynamicTrend signal params (validated: ema_fast=60 optimal)
const EMA_FAST: usize = 60;  // from dynamic_trend_walkforward.rs: ef=60 wins on Base5
const EMA_SLOW: usize = 100;
const RSI_FILTER: f64 = 50.0;  // RSI > 50 to confirm bullish regime

// Chandelier dual-exit params (production frozen)
const CHAND_PERIOD: usize = 28;
const CHAND_MULT: f64 = 2.15;   // hyperopt 2026-04-16
const TURTLE_ATR_PERIOD: usize = 24;  // hyperopt 2026-04-16
const TURTLE_ATR_MULT: f64 = 2.00;   // hyperopt 2026-04-12
const POSITION_CAP: usize = 3;
const MIN_TRADES: usize = 3;
const TAKER_FEE: f64 = 0.001;

// Turtle+Chandelier reference params (for comparison column)
const TURTLE_ENTRY: usize = 21;

const UNIVERSES: &[(&str, &[&str])] = &[
    ("Base5",        &["BTCUSDT","ETHUSDT","SOLUSDT","XRPUSDT","DOGEUSDT","ADAUSDT"]),
    ("NoDOGE",       &["BTCUSDT","ETHUSDT","SOLUSDT","XRPUSDT","ADAUSDT"]),
    ("LargeCaps5",   &["BTCUSDT","ETHUSDT","SOLUSDT","XRPUSDT","BNBUSDT","ADAUSDT"]),
    ("Legacy4",      &["BTCUSDT","ETHUSDT","XRPUSDT","LTCUSDT","EOSUSDT"]),
];

struct SymData {
    close: Vec<f64>,
    high: Vec<f64>,
    low: Vec<f64>,
    open: Vec<f64>,
    vol: Vec<f64>,
}

fn ema_at(data: &[f64], period: usize, end_idx: usize) -> f64 {
    if end_idx < period { return data.get(end_idx.min(data.len().saturating_sub(1))).copied().unwrap_or(0.0); }
    let start = end_idx.saturating_sub(period);
    let window = &data[start..end_idx.min(data.len())];
    if window.is_empty() { return data[end_idx.min(data.len().saturating_sub(1))]; }
    let alpha = 2.0 / (period as f64 + 1.0);
    let mut ema_val = window[0];
    for &v in &window[1..] { ema_val = v * alpha + ema_val * (1.0 - alpha); }
    ema_val
}

fn rsi_at(data: &[f64], period: usize, idx: usize) -> f64 {
    if idx < period + 1 { return 50.0; }
    let start = idx.saturating_sub(period);
    let mut gains = 0.0_f64;
    let mut losses = 0.0_f64;
    for i in (start+1)..=idx {
        let diff = data.get(i).unwrap_or(&0.0) - data.get(i-1).unwrap_or(&0.0);
        if diff > 0.0 { gains += diff; } else { losses -= diff; }
    }
    if losses < 1e-9 { return 100.0; }
    let avg_gain = gains / period as f64;
    let avg_loss = losses / period as f64;
    100.0 - (100.0 / (1.0 + avg_gain / avg_loss))
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

// ── DYNAMICTREND + CHANDELIER ──────────────────────────────────────────────

fn dynamic_trend_signal(close: &[f64], idx: usize) -> bool {
    if idx < EMA_SLOW.max(EMA_FAST) + 5 || idx >= close.len() { return false; }
    let ef = ema_at(close, EMA_FAST, idx);
    let es = ema_at(close, EMA_SLOW, idx);
    let rsi = rsi_at(close, 14, idx);
    rsi >= RSI_FILTER && ef > es
}

fn run_dt_chand_sim(
    sym_data: &HashMap<String, SymData>,
    symbols: &[String],
    test_start: usize,
    test_end: usize,
) -> (f64, f64, f64, usize, f64) {
    let mut equity = 1.0_f64;
    let mut wins = 0usize;
    let mut total_trades = 0usize;
    let mut daily_rets = Vec::new();
    let mut equity_curve = vec![1.0_f64];
    let mut peak = 1.0_f64;

    let mut bar = test_start;
    while bar + 2 < test_end {
        // Volume-ranked top-CAP symbols
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

        // Entry: DynamicTrend EMA crossover
        let mut entered = false;
        for sym in &top_syms {
            if let Some(sd) = sym_data.get(sym) {
                if dynamic_trend_signal(&sd.close, bar) {
                    let entry_px = sd.close[bar];
                    let entry = entry_px * (1.0 - TAKER_FEE);
                    let entry_bar_next = bar + 1;
                    let n = sd.close.len();

                    // Chandelier dual-exit: same as Turtle production
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
                        if equity > peak { peak = equity; }
                        equity_curve.push(equity);
                        bar = exit_bar + 1;
                        entered = true;
                        break;
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
    (ret, sharpe, max_dd, total_trades, win_rate)
}

// ── TURTLE + CHANDELIER (reference) ───────────────────────────────────────

fn turtle_signal(close: &[f64], entry_period: usize, idx: usize) -> bool {
    if idx < entry_period + 1 { return false; }
    let start = idx + 1 - entry_period;
    let mut max_close = f64::NEG_INFINITY;
    for i in start..idx {
        if let Some(&c) = close.get(i) { max_close = max_close.max(c); }
    }
    close.get(idx).map(|&c| c > max_close).unwrap_or(false)
}

fn run_turtle_chand_sim(
    sym_data: &HashMap<String, SymData>,
    symbols: &[String],
    test_start: usize,
    test_end: usize,
) -> (f64, f64, f64, usize, f64) {
    let mut equity = 1.0_f64;
    let mut wins = 0usize;
    let mut total_trades = 0usize;
    let mut daily_rets = Vec::new();
    let mut equity_curve = vec![1.0_f64];
    let mut peak = 1.0_f64;

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
    (ret, sharpe, max_dd, total_trades, win_rate)
}

// ─────────────────────────────────────────────────────────────────────────────

#[tokio::main]
async fn main() -> Result<()> {
    let t0 = Instant::now();
    std::fs::create_dir_all("snapshots")?;
    std::fs::create_dir_all("charts")?;

    println!("\n{}", "=".repeat(72));
    println!("  DynamicTrend(EMA) + Chandelier vs Turtle + Chandelier");
    println!("  Signal:  EMA({}/{}) crossover + RSI>{}", EMA_FAST, EMA_SLOW, RSI_FILTER);
    println!("  Exit:     Chandelier({}, {}) dual ATR({}, {})", CHAND_PERIOD, CHAND_MULT, TURTLE_ATR_PERIOD, TURTLE_ATR_MULT);
    println!("  Baseline: Turtle breakout EP={} + same Chandelier exit", TURTLE_ENTRY);
    println!("{}", "=".repeat(72));

    // Load data
    let mut all_syms: std::collections::HashSet<&str> = std::collections::HashSet::new();
    for &(_, syms) in UNIVERSES {
        for &s in syms { all_syms.insert(s); }
    }
    let all_syms: Vec<&str> = all_syms.into_iter().collect();
    println!("\nLoading {} symbols...", all_syms.len());

    let loader = DataLoader::new(None, None);
    let mut sym_data_map: HashMap<String, SymData> = HashMap::new();
    let mut min_len = usize::MAX;
    for &sym in &all_syms {
        let raw = loader.fetch_with_cache(sym, "1d", CANDLES).await?;
        let df = FeatureEngine::add_technicals(&raw, None)?;
        min_len = min_len.min(df.height());

        macro_rules! col_vec {
            ($col:expr) => { df.column($col)?.f64()?.into_no_null_iter().collect::<Vec<_>>() };
        }
        let sd = SymData {
            close: col_vec!("close"),
            high: col_vec!("high"),
            low: col_vec!("low"),
            open: col_vec!("open"),
            vol: col_vec!("volume"),
        };
        sym_data_map.insert(sym.to_string(), sd);
    }
    let trim_len = min_len.saturating_sub(10);
    for sd in sym_data_map.values_mut() {
        if sd.close.len() > trim_len {
            sd.close.truncate(trim_len);
            sd.high.truncate(trim_len);
            sd.low.truncate(trim_len);
            sd.open.truncate(trim_len);
            sd.vol.truncate(trim_len);
        }
    }
    println!("All symbols loaded, {} bars each.\n", trim_len);

    let mut csv_f = File::create("snapshots/dynamic_trend_chandelier_wf.csv")?;
    writeln!(csv_f, "universe,window,dt_ret_pct,dt_sharpe,dt_max_dd,dt_trades,dt_win_rate,turtle_ret_pct,turtle_sharpe,turtle_max_dd,turtle_trades,turtle_win_rate,dt_pass,turtle_pass,dt_wins")?;

    let _md_lines = vec![
        format!("# DynamicTrend + Chandelier vs Turtle + Chandelier\n"),
        format!("**Generated:** {:}\n\n", chrono::Utc::now().to_rfc3339()),
        format!("| Universe | Window | DT Ret% | DT Sharpe | DT DD% | DT Trades | Turtle Ret% | Turtle Sharpe | Turtle DD% | Turtle Trades | Δ Sharpe | Winner |\n"),
        format!("|----------|--------|---------|-----------|--------|-----------|-------------|--------------|-----------|--------------|---------|-------|\n"),
    ];

    let mut global_dt_wins = 0usize;
    let mut global_turtle_wins = 0usize;
    let mut total_windows = 0usize;

    for &(uni_name, syms) in UNIVERSES {
        let t_uni = Instant::now();
        println!("[{}] Running walk-forward...", uni_name);

        let sym_strs: Vec<String> = syms.iter().map(|s| s.to_string()).collect();
        let n = sym_strs.iter().filter_map(|s| sym_data_map.get(s).map(|sd| sd.close.len())).min().unwrap_or(0);
        let n_windows = n.saturating_sub(TRAIN_BARS + HOLD_MAX + 20) / TEST_BARS;
        if n_windows == 0 { eprintln!("  SKIP {} (n={})", uni_name, n); continue; }
        println!("  {} bars, {} windows", n, n_windows);

        let mut uni_dt_wins = 0usize;
        let mut uni_turtle_wins = 0usize;

        for wi in 0..n_windows {
            let tstart = TRAIN_BARS + wi * TEST_BARS;
            let tend = (tstart + TEST_BARS).min(n.saturating_sub(1));
            if tend.saturating_sub(tstart) < HOLD_MAX + EMA_SLOW + 10 { continue; }

            let (dt_ret, dt_sh, dt_dd, dt_trades, dt_wr) =
                run_dt_chand_sim(&sym_data_map, &sym_strs, tstart, tend);
            let (tu_ret, tu_sh, tu_dd, tu_trades, tu_wr) =
                run_turtle_chand_sim(&sym_data_map, &sym_strs, tstart, tend);

            let dt_pass = dt_trades >= MIN_TRADES && dt_ret > 0.0;
            let tu_pass = tu_trades >= MIN_TRADES && tu_ret > 0.0;
            let dt_wins = if dt_sh > tu_sh { 1 } else { 0 };

            if dt_sh > tu_sh { uni_dt_wins += 1; } else { uni_turtle_wins += 1; }

            writeln!(csv_f, "{},{},{},{:.3},{:.1},{},{:.1},{},{:.3},{:.1},{},{:.1},{},{},{}",
                     uni_name, wi, dt_ret, dt_sh, dt_dd, dt_trades, dt_wr,
                     tu_ret, tu_sh, tu_dd, tu_trades, tu_wr,
                     dt_pass as i32, tu_pass as i32, dt_wins)?;
            println!("  W{:02} | DT sh={:+.3} ret={:+6.1}% | Turtle sh={:+.3} ret={:+6.1}% | {} wins",
                     wi, dt_sh, dt_ret, tu_sh, tu_ret, if dt_sh > tu_sh { "DT" } else { "Turtle" });
        }

        let uni_total = n_windows;
        global_dt_wins += uni_dt_wins;
        global_turtle_wins += uni_turtle_wins;
        total_windows += uni_total;
        println!("  [{}] DT wins {}/{} windows ({:.0}%) in {:.1}s\n",
                 uni_name, uni_dt_wins, uni_total, uni_dt_wins as f64 / uni_total as f64 * 100.0,
                 t_uni.elapsed().as_secs_f64());
    }

    let global_dt_pct = global_dt_wins as f64 / total_windows as f64 * 100.0;
    let global_turtle_pct = global_turtle_wins as f64 / total_windows as f64 * 100.0;
    println!("\n{}", "=".repeat(72));
    println!("  GLOBAL RESULTS");
    println!("  DynamicTrend+Chandelier wins: {}/{} ({:.1}%)", global_dt_wins, total_windows, global_dt_pct);
    println!("  Turtle+Chandelier wins:       {}/{} ({:.1}%)", global_turtle_wins, total_windows, global_turtle_pct);
    println!("  {}", "=".repeat(72));

    // Summary verdict
    let verdict = if global_dt_pct >= 70.0 {
        "VALIDATED — DynamicTrend signal is a genuine alternative to Turtle breakout"
    } else if global_dt_pct >= 55.0 {
        "MARGINAL — DynamicTrend competitive but not clearly better than Turtle"
    } else {
        "REJECTED — Turtle breakout signal outperforms DynamicTrend EMA crossover"
    };
    println!("  Verdict: {}", verdict);

    println!("\n  Done in {:.1}s total", t0.elapsed().as_secs_f64());

    // Write summary
    let mut f = File::create("snapshots/dynamic_trend_chandelier_summary.md")?;
    writeln!(f, "# DynamicTrend + Chandelier Walk-Forward Summary\n")?;
    writeln!(f, "## Verdict: {}", verdict)?;
    writeln!(f, "\n## Global Results")?;
    writeln!(f, "- DynamicTrend+Chandelier wins: {}/{} ({:.1}%)", global_dt_wins, total_windows, global_dt_pct)?;
    writeln!(f, "- Turtle+Chandelier wins:       {}/{} ({:.1}%)", global_turtle_wins, total_windows, global_turtle_pct)?;
    writeln!(f, "\n## Signal: EMA({}/{}) crossover + RSI>{}, exit=Chandelier({}, {}) dual ATR({}, {})",
             EMA_FAST, EMA_SLOW, RSI_FILTER, CHAND_PERIOD, CHAND_MULT, TURTLE_ATR_PERIOD, TURTLE_ATR_MULT)?;
    writeln!(f, "\n## CSV: snapshots/dynamic_trend_chandelier_wf.csv")?;

    Ok(())
}