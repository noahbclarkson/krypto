//! Drawdown-Triggered Position Sizing Prototype
//!
//! Adds a BTC drawdown trigger to Turtle+Chandelier:
//!   When BTC drops > D threshold in any rolling W-bar window →
//!   reduce position exposure by SIZE_REDUCTION for next COOLDOWN bars.
//!
//! Runs BASE vs TRIGGER side-by-side across all 9 universes.
//! Reports: W05 drawdown improvement vs false-signal cost in other windows.
//!
//! Track A/B: Risk management audit + leader stress test.

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
const HOLD_MAX: usize = 45; // hyperopt 2026-04-11: HM=45 winner
const TAKER_FEE: f64 = 0.001;
const POSITION_CAP: usize = 3;
const MIN_TRADES: usize = 3;
const CHAND_PERIOD: usize = 28;
const CHAND_MULT: f64 = 2.00;
const TURTLE_ENTRY: usize = 21;

// Drawdown trigger parameters
const DD_LOOKBACK: usize = 7;          // Rolling lookback for BTC drop
const DD_THRESHOLD: f64 = -0.15;       // 15% drop triggers
const COOLDOWN: usize = 21;            // Bars to keep reduced position after trigger
const SIZE_REDUCTION: f64 = 0.50;      // 50% position reduction

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

const CSV_OUT: &str = "snapshots/drawdown_trigger_9way_wf.csv";
const MD_OUT: &str = "snapshots/drawdown_trigger_9way_wf.md";

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
struct WfResult {
    ret: f64,
    sharpe: f64,
    max_dd: f64,
    trades: usize,
    win_rate: f64,
    pass: bool,
    trigger_fires: usize,  // How many times the drawdown trigger fired
    trigger_bars: usize,   // Total bars under trigger
}

/// Run simulation with optional drawdown trigger.
/// btc_close: BTC closing prices (reference asset for drawdown detection)
/// use_trigger: if true, apply drawdown-triggered position reduction
fn run_sim(
    sym_data: &HashMap<String, SymData>,
    symbols: &[String],
    btc_close: &[f64],
    test_start: usize,
    test_end: usize,
    use_trigger: bool,
) -> WfResult {
    let mut equity = 1.0_f64;
    let mut equity_curve = vec![1.0_f64];
    let mut peak = equity;
    let mut wins = 0usize;
    let mut total_trades = 0usize;
    let mut daily_rets = Vec::new();

    // Drawdown trigger state
    let mut trigger_active_until: usize = 0; // Bar until which trigger is active
    let mut trigger_fire_count = 0usize;
    let mut trigger_bar_count = 0usize;

    let mut bar = test_start;
    while bar + 2 < test_end {
        // Check BTC drawdown trigger
        let mut is_triggered = false;
        if use_trigger && bar >= DD_LOOKBACK && bar < btc_close.len() {
            if trigger_active_until > bar {
                // Still in cooldown from previous trigger
                is_triggered = true;
            } else {
                // Check rolling 7-bar drop
                let btc_now = btc_close.get(bar).copied().unwrap_or(0.0);
                let btc_then = btc_close.get(bar - DD_LOOKBACK).copied().unwrap_or(0.0);
                if btc_then > 0.0 {
                    let rolling_ret = (btc_now / btc_then) - 1.0;
                    if rolling_ret < DD_THRESHOLD {
                        // Trigger fires!
                        trigger_active_until = bar + COOLDOWN;
                        trigger_fire_count += 1;
                        is_triggered = true;
                    }
                }
            }
            if is_triggered {
                trigger_bar_count += 1;
            }
        }

        // Position size multiplier
        let size_mult = if is_triggered { SIZE_REDUCTION } else { 1.0 };

        // Rank symbols by dollar volume
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

        // Turtle breakout entry (no regime filter)
        let mut entered = false;
        for sym in &top_syms {
            if let Some(sd) = sym_data.get(sym) {
                if bar >= TURTLE_ENTRY + 1 && bar < sd.close.len() {
                    if turtle_signal(&sd.close, &sd.high, TURTLE_ENTRY, bar) {
                        let entry_px = sd.close[bar];
                        let entry = entry_px * (1.0 - TAKER_FEE);
                        let entry_bar_next = bar + 1;
                        let n = sd.close.len();

                        // Chandelier trailing stop
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
                            // Apply position size multiplier
                            let sized_ret = gross_ret * size_mult;
                            let bars_held = (exit_bar as i64 - entry_bar_next as i64).max(1) as usize;

                            wins += if gross_ret > 0.0 { 1 } else { 0 };
                            total_trades += 1;
                            equity *= 1.0 + sized_ret;

                            let avg_daily = sized_ret / bars_held as f64;
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

    WfResult { ret, sharpe, max_dd, trades: total_trades, win_rate, pass, trigger_fires: trigger_fire_count, trigger_bars: trigger_bar_count }
}

#[tokio::main]
async fn main() -> Result<()> {
    let t0 = Instant::now();
    eprintln!("==== Drawdown-Triggered Position Sizing: Turtle+Chandelier ====");
    eprintln!("EP={}, Chandelier({}, {}), 252/252 train/test", TURTLE_ENTRY, CHAND_PERIOD, CHAND_MULT);
    eprintln!("DD trigger: BTC drops >{:.0}% in {} bars → {}% position for {} bars",
        DD_THRESHOLD * -100.0, DD_LOOKBACK, SIZE_REDUCTION * 100.0, COOLDOWN);
    eprintln!();

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

    // Extract BTC close for drawdown detection
    let btc_close: Vec<f64> = sym_data_map.get("BTCUSDT")
        .map(|sd| sd.close.clone())
        .unwrap_or_default();

    eprintln!("Loaded {} symbols, {} bars, BTC ref len={}\n", sym_data_map.len(), n, btc_close.len());

    // Collect results
    struct WindowResult {
        universe: String,
        window: usize,
        base: WfResult,
        trigger: WfResult,
        dd_improvement: f64,  // Positive = trigger reduced drawdown
        ret_delta: f64,       // Positive = trigger improved return
    }

    let mut all_results: Vec<WindowResult> = Vec::new();
    let mut csv_lines = vec![
        "universe,window,base_ret,base_sharpe,base_dd,base_trades,base_pass,trigger_ret,trigger_sharpe,trigger_dd,trigger_trades,trigger_pass,trigger_fires,trigger_bars,dd_improvement,ret_delta".to_string()
    ];

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

            let base = run_sim(&sym_data_map, &symbols, &btc_close, test_start, test_end, false);
            let trigger = run_sim(&sym_data_map, &symbols, &btc_close, test_start, test_end, true);

            let dd_improvement = base.max_dd - trigger.max_dd;
            let ret_delta = trigger.ret - base.ret;

            let base_tag = if base.pass { "PASS" } else { "FAIL" };
            let trig_tag = if trigger.pass { "PASS" } else { "FAIL" };
            let better = if dd_improvement > 0.0 { "DD↓" } else if dd_improvement < 0.0 { "DD↑" } else { "=" };

            eprintln!(
                "  W{:02} | BASE {:+8.1}% DD={:5.1}% {:3}t {} | TRIG {:+8.1}% DD={:5.1}% {:3}t {} | {:5.1}% DDΔ {} {}fires",
                wi,
                base.ret, base.max_dd, base.trades, base_tag,
                trigger.ret, trigger.max_dd, trigger.trades, trig_tag,
                dd_improvement, better, trigger.trigger_fires,
            );

            csv_lines.push(format!(
                "{},{},{:.2},{:.4},{:.2},{},{},{:.2},{:.4},{:.2},{},{},{},{},{:.2},{:.2}",
                label, wi,
                base.ret, base.sharpe, base.max_dd, base.trades, base.pass,
                trigger.ret, trigger.sharpe, trigger.max_dd, trigger.trades, trigger.pass,
                trigger.trigger_fires, trigger.trigger_bars,
                dd_improvement, ret_delta,
            ));

            all_results.push(WindowResult {
                universe: label.to_string(),
                window: wi,
                base,
                trigger,
                dd_improvement,
                ret_delta,
            });
        }
        eprintln!();
    }

    // ===== SUMMARY =====
    eprintln!("\n========== GLOBAL SUMMARY ==========");

    // Per-universe summary
    eprintln!("\n| Universe | Base Pass | Trig Pass | Base Avg DD | Trig Avg DD | DD Δ | Base Avg Ret | Trig Avg Ret |");
    eprintln!("|---|---|---|---|---|---|---|---|");

    let mut global_base_pass = 0usize;
    let mut global_trig_pass = 0usize;
    let mut global_total = 0usize;
    let mut _global_base_dd_improvement = 0.0_f64;
    let mut global_trig_dd_reduction_count = 0usize;
    let mut global_trig_dd_worsening_count = 0usize;

    for &(label, _) in UNIVERSES {
        let recs: Vec<_> = all_results.iter().filter(|r| r.universe == label).collect();
        if recs.is_empty() { continue; }

        let base_pass = recs.iter().filter(|r| r.base.pass).count();
        let trig_pass = recs.iter().filter(|r| r.trigger.pass).count();
        let base_avg_dd: f64 = recs.iter().map(|r| r.base.max_dd).sum::<f64>() / recs.len() as f64;
        let trig_avg_dd: f64 = recs.iter().map(|r| r.trigger.max_dd).sum::<f64>() / recs.len() as f64;
        let base_avg_ret: f64 = recs.iter().map(|r| r.base.ret).sum::<f64>() / recs.len() as f64;
        let trig_avg_ret: f64 = recs.iter().map(|r| r.trigger.ret).sum::<f64>() / recs.len() as f64;
        let dd_improvement = base_avg_dd - trig_avg_dd;

        global_base_pass += base_pass;
        global_trig_pass += trig_pass;
        global_total += recs.len();

        for r in &recs {
            if r.dd_improvement > 0.0 {
                global_trig_dd_reduction_count += 1;
                _global_base_dd_improvement += r.dd_improvement;
            } else if r.dd_improvement < 0.0 {
                global_trig_dd_worsening_count += 1;
            }
        }

        eprintln!("| {} | {}/{} | {}/{} | {:.1}% | {:.1}% | {:+.1}% | {:+.1}% | {:+.1}% |",
            label, base_pass, recs.len(), trig_pass, recs.len(),
            base_avg_dd, trig_avg_dd, dd_improvement, base_avg_ret, trig_avg_ret);
    }

    // W05 focus: which window indices correspond to the 2022-23 bear period?
    // With 252/252 splits, windows are sequential 252-bar chunks.
    // We need to identify W05 by checking which windows have the worst base drawdown.
    // Instead of hardcoding, let's just report the worst-DD windows.
    eprintln!("\n===== WORST-DD WINDOWS (Bear Market Stress) =====");
    let mut by_dd: Vec<_> = all_results.iter().collect();
    by_dd.sort_by(|a, b| b.base.max_dd.partial_cmp(&a.base.max_dd).unwrap());

    eprintln!("| Universe | Window | Base DD | Trig DD | DD Δ | Base Ret | Trig Ret | Fires |");
    eprintln!("|---|---|---|---|---|---|---|---|");
    for r in by_dd.iter().take(20) {
        if r.base.max_dd < 10.0 { break; }
        eprintln!("| {} | W{:02} | {:.1}% | {:.1}% | {:+.1}% | {:+.1}% | {:+.1}% | {} |",
            r.universe, r.window, r.base.max_dd, r.trigger.max_dd,
            r.dd_improvement, r.base.ret, r.trigger.ret, r.trigger.trigger_fires);
    }

    // Trigger frequency analysis
    eprintln!("\n===== TRIGGER FIRING ANALYSIS =====");
    let windows_with_fires: Vec<_> = all_results.iter().filter(|r| r.trigger.trigger_fires > 0).collect();
    eprintln!("Windows with trigger fires: {}/{}", windows_with_fires.len(), all_results.len());

    if !windows_with_fires.is_empty() {
        let avg_fires: f64 = windows_with_fires.iter().map(|r| r.trigger.trigger_fires as f64).sum::<f64>()
            / windows_with_fires.len() as f64;
        let avg_dd_improvement_when_fired: f64 = windows_with_fires.iter()
            .map(|r| r.dd_improvement).sum::<f64>() / windows_with_fires.len() as f64;

        eprintln!("Avg fires per triggered window: {:.1}", avg_fires);
        eprintln!("Avg DD improvement when trigger fires: {:+.1}%", avg_dd_improvement_when_fired);

        let helped = windows_with_fires.iter().filter(|r| r.dd_improvement > 0.0).count();
        let hurt = windows_with_fires.iter().filter(|r| r.dd_improvement < 0.0).count();
        eprintln!("Trigger helped DD: {}/{} ({:.0}%)", helped, windows_with_fires.len(),
            helped as f64 / windows_with_fires.len() as f64 * 100.0);
        eprintln!("Trigger hurt DD: {}/{} ({:.0}%)", hurt, windows_with_fires.len(),
            hurt as f64 / windows_with_fires.len() as f64 * 100.0);

        // Ret impact when trigger fires
        let avg_ret_delta_when_fired: f64 = windows_with_fires.iter()
            .map(|r| r.ret_delta).sum::<f64>() / windows_with_fires.len() as f64;
        eprintln!("Avg return delta when trigger fires: {:+.1}%", avg_ret_delta_when_fired);
    }

    // Windows without fires
    let windows_without_fires: Vec<_> = all_results.iter().filter(|r| r.trigger.trigger_fires == 0).collect();
    if !windows_without_fires.is_empty() {
        let avg_base_ret: f64 = windows_without_fires.iter().map(|r| r.base.ret).sum::<f64>() / windows_without_fires.len() as f64;
        eprintln!("\nWindows without trigger fires: {}", windows_without_fires.len());
        eprintln!("Avg base return (no-fire windows): {:+.1}%", avg_base_ret);
    }

    // Global summary
    let fail_pct = (global_total - global_base_pass) as f64 / global_total.max(1) as f64 * 100.0;
    let trig_fail_pct = (global_total - global_trig_pass) as f64 / global_total.max(1) as f64 * 100.0;

    eprintln!("\n===== BOTTOM LINE =====");
    eprintln!("BASE:  {}/{} pass ({:.0}% fail)", global_base_pass, global_total, fail_pct);
    eprintln!("TRIG:  {}/{} pass ({:.0}% fail)", global_trig_pass, global_total, trig_fail_pct);
    eprintln!("DD improved in {} windows, worsened in {} windows",
        global_trig_dd_reduction_count, global_trig_dd_worsening_count);

    // Write CSV
    let mut f = File::create(CSV_OUT)?;
    for line in &csv_lines { writeln!(f, "{}", line)?; }
    eprintln!("\nCSV: {}", CSV_OUT);

    // Write MD report
    let mut md = File::create(MD_OUT)?;
    writeln!(md, "# Drawdown-Triggered Position Sizing: Turtle+Chandelier")?;
    writeln!(md, "")?;
    writeln!(md, "## Parameters")?;
    writeln!(md, "- Strategy: Turtle+Chandelier(EP={}, P={}, M={})", TURTLE_ENTRY, CHAND_PERIOD, CHAND_MULT)?;
    writeln!(md, "- Drawdown trigger: BTC drops >{:.0}% in {} bars → {:.0}% position for {} bars",
        DD_THRESHOLD * -100.0, DD_LOOKBACK, SIZE_REDUCTION * 100.0, COOLDOWN)?;
    writeln!(md, "- Walk-forward: 252/252 train/test, {} universes", UNIVERSES.len())?;
    writeln!(md, "")?;

    writeln!(md, "## Per-Universe Summary")?;
    writeln!(md, "| Universe | Base Pass | Trig Pass | Base Avg DD | Trig Avg DD | DD Δ | Base Avg Ret | Trig Avg Ret |")?;
    writeln!(md, "|---|---|---|---|---|---|---|---|")?;
    for &(label, _) in UNIVERSES {
        let recs: Vec<_> = all_results.iter().filter(|r| r.universe == label).collect();
        if recs.is_empty() { continue; }
        let base_pass = recs.iter().filter(|r| r.base.pass).count();
        let trig_pass = recs.iter().filter(|r| r.trigger.pass).count();
        let base_avg_dd: f64 = recs.iter().map(|r| r.base.max_dd).sum::<f64>() / recs.len() as f64;
        let trig_avg_dd: f64 = recs.iter().map(|r| r.trigger.max_dd).sum::<f64>() / recs.len() as f64;
        let base_avg_ret: f64 = recs.iter().map(|r| r.base.ret).sum::<f64>() / recs.len() as f64;
        let trig_avg_ret: f64 = recs.iter().map(|r| r.trigger.ret).sum::<f64>() / recs.len() as f64;
        writeln!(md, "| {} | {}/{} | {}/{} | {:.1}% | {:.1}% | {:+.1}% | {:+.1}% | {:+.1}% |",
            label, base_pass, recs.len(), trig_pass, recs.len(),
            base_avg_dd, trig_avg_dd, base_avg_dd - trig_avg_dd, base_avg_ret, trig_avg_ret)?;
    }

    writeln!(md, "")?;
    writeln!(md, "## Worst-DD Windows")?;
    writeln!(md, "| Universe | Window | Base DD | Trig DD | DD Δ | Base Ret | Trig Ret | Fires |")?;
    writeln!(md, "|---|---|---|---|---|---|---|---|")?;
    for r in by_dd.iter().take(20) {
        if r.base.max_dd < 10.0 { break; }
        writeln!(md, "| {} | W{:02} | {:.1}% | {:.1}% | {:+.1}% | {:+.1}% | {:+.1}% | {} |",
            r.universe, r.window, r.base.max_dd, r.trigger.max_dd,
            r.dd_improvement, r.base.ret, r.trigger.ret, r.trigger.trigger_fires)?;
    }

    writeln!(md, "")?;
    writeln!(md, "## Global")?;
    writeln!(md, "- BASE: {}/{} pass ({:.0}% fail)", global_base_pass, global_total, fail_pct)?;
    writeln!(md, "- TRIG: {}/{} pass ({:.0}% fail)", global_trig_pass, global_total, trig_fail_pct)?;
    writeln!(md, "- DD improved: {} windows, DD worsened: {} windows",
        global_trig_dd_reduction_count, global_trig_dd_worsening_count)?;

    eprintln!("MD: {}", MD_OUT);
    eprintln!("Runtime: {:?}", t0.elapsed());

    Ok(())
}
