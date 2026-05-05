//! REGIME_ATR_PERIOD Extensive Sweep — Turtle-Only Live Path
//!
//! Extensively sweep REGIME_ATR_PERIOD (AP) ∈ [1..=80 step 1] using the
//! ACTUAL Turtle-only live exit path (matches src/live/bot.rs + live_compatible_wf.rs).
//!
//! PRIOR: ap_hyperopt.rs used AP=17 from held-out validation as winner.
//! PRIOR: live_compatible_wf.rs hardcodes AP=17 with no extensive sweep.
//! This harness tests AP UNDER THE ACTUAL LIVE CONDITIONS with proper fee accounting.
//!
//! Range: 80 values × 9 universes × 7 windows = 5,040 runs

use anyhow::Result;
use krypto::data::loader::DataLoader;
use std::collections::HashMap;
use std::fs::File;
use std::io::Write;

const CANDLES: u32 = 3000;
const TRAIN_BARS: usize = 252;
const TEST_BARS: usize = 252;
const HOLD_MAX: usize = 12;
const TAKER_FEE: f64 = 0.001;
const POSITION_CAP: usize = 3;
const MIN_TRADES: usize = 3;

const TURTLE_ENTRY: usize = 21;
const TURTLE_ATR_PERIOD: usize = 24;
const TURTLE_ATR_MULT: f64 = 2.00;
const ATR_ENTRY_MULT: f64 = 0.00;
// VL=96 — production default from live_compatible_wf.rs
const VOL_LOOKBACK: usize = 96;

// ATR_RANK fixed at production default T=5.0 (settled 2026-05-04)
const ATR_RANK_T: f64 = 5.0;
// REGIME_LOOKBACK fixed at production default LB=42 (settled 2026-05-02)
const REGIME_LOOKBACK: usize = 42;

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
        let h = high[i];
        let l = low[i];
        let c0 = close[i.saturating_sub(1)];
        trs.push((h - l).max((h - c0).abs()).max((l - c0).abs()));
    }
    trs.iter().sum::<f64>() / period as f64
}

fn rolling_avg(vals: &[f64], window: usize, idx: usize) -> f64 {
    if idx < window { return 0.0; }
    vals[idx.saturating_sub(window - 1)..=idx].iter().sum::<f64>() / window as f64
}

fn turtle_signal(close: &[f64], high: &[f64], low: &[f64], entry_period: usize, atr_period: usize, atr_mult: f64, idx: usize) -> bool {
    if idx < entry_period { return false; }
    let start = idx - entry_period;
    let max_close = close[start..idx].iter().fold(f64::NEG_INFINITY, |a, &b| a.max(b));
    if close[idx] > max_close {
        if atr_mult > 0.0 {
            let atr = atr_at(high, low, close, atr_period, idx);
            if close[idx] < max_close + atr * atr_mult { return false; }
        }
        true
    } else { false }
}

/// Compute BTC ATR percentile rank at bar idx using given AP and fixed LB=42.
/// Returns 0-100 percentile.
fn btc_atr_pct(btc_data: &SymData, ap: usize, lb: usize, idx: usize) -> f64 {
    let warmup = ap.max(lb) + 1;
    if idx < warmup { return 50.0; }
    let curr_atr = atr_at(&btc_data.high, &btc_data.low, &btc_data.close, ap, idx);
    let mut hist = Vec::with_capacity(lb);
    for j in (idx + 1 - lb)..=idx {
        if j >= ap {
            hist.push(atr_at(&btc_data.high, &btc_data.low, &btc_data.close, ap, j));
        }
    }
    if hist.is_empty() { return 50.0; }
    let count = hist.iter().filter(|&&x| x < curr_atr).count();
    (count as f64 / hist.len() as f64) * 100.0
}

fn annualised_sharpe(daily_rets: &[f64]) -> f64 {
    if daily_rets.is_empty() { return 0.0; }
    let mean = daily_rets.iter().sum::<f64>() / daily_rets.len() as f64;
    let var = daily_rets.iter().map(|x| (x - mean).powi(2)).sum::<f64>() / daily_rets.len() as f64;
    if var == 0.0 { return 0.0; }
    (mean / var.sqrt()) * (365.0_f64).sqrt()
}

fn max_dd_from(equity: &[f64]) -> f64 {
    let mut max_dd = 0.0;
    let mut peak = 1.0;
    for &val in equity {
        if val > peak { peak = val; }
        let dd = 1.0 - val / peak;
        if dd > max_dd { max_dd = dd; }
    }
    max_dd * 100.0
}

#[derive(Default)]
struct WfResult {
    equity: f64,
    sharpe: f64,
    dd: f64,
    trades: usize,
}

struct AggResult {
    ap: usize,
    pass_count: usize,
    total_windows: usize,
    avg_sharpe: f64,
    avg_return: f64,
    avg_dd: f64,
    total_trades: usize,
    positive_universes: usize,
}

fn run_sim(
    sym_data: &HashMap<String, SymData>,
    symbols: &[String],
    regime_ap: usize,
    test_start: usize,
    test_end: usize,
    equity_curve: &mut Vec<f64>,
) -> WfResult {
    let mut equity = 1.0;
    let mut peak = 1.0;
    let mut total_trades = 0usize;
    let mut daily_rets = Vec::new();
    let mut bar = test_start;

    while bar + 2 < test_end {
        let btc = sym_data.get("BTCUSDT");
        let btc_pct = if let Some(b) = btc {
            btc_atr_pct(b, regime_ap, REGIME_LOOKBACK, bar)
        } else {
            50.0
        };

        if btc_pct < ATR_RANK_T {
            equity_curve.push(equity);
            bar += 1;
            continue;
        }

        // Dollar-volume ranking
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
                        let entry = entry_px * (1.0 + TAKER_FEE);
                        let entry_bar_next = bar + 1;
                        let n = sd.close.len();

                        let mut highest_high = sd.high[entry_bar_next];
                        let max_bar = (entry_bar_next + HOLD_MAX).min(n.saturating_sub(1));
                        let mut exit_bar = max_bar;

                        let mut atr_buf: std::collections::VecDeque<f64> = std::collections::VecDeque::new();

                        for b in entry_bar_next..=max_bar {
                            if sd.high[b] > highest_high { highest_high = sd.high[b]; }
                            let c0 = sd.close[b.saturating_sub(1)];
                            let tr = (sd.high[b] - sd.low[b]).max((sd.high[b] - c0).abs()).max((sd.low[b] - c0).abs());
                            atr_buf.push_back(tr);
                            if atr_buf.len() > TURTLE_ATR_PERIOD { atr_buf.pop_front(); }

                            if atr_buf.len() == TURTLE_ATR_PERIOD {
                                let atr = atr_buf.iter().sum::<f64>() / TURTLE_ATR_PERIOD as f64;
                                let turtle_stop = highest_high - TURTLE_ATR_MULT * atr;
                                if sd.low[b] <= turtle_stop {
                                    exit_bar = b;
                                    break;
                                }
                            }
                        }

                        if let Some(&exit_px) = sd.close.get(exit_bar) {
                            let exit = exit_px * (1.0 - TAKER_FEE);
                            let pct_ret = exit / entry - 1.0;
                            let bars_held = (exit_bar as i64 - entry_bar_next as i64).max(1) as usize;

                            total_trades += 1;
                            equity *= 1.0 + pct_ret;

                            let avg_daily = pct_ret / bars_held as f64;
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

    WfResult {
        equity,
        sharpe: annualised_sharpe(&daily_rets),
        dd: max_dd_from(equity_curve),
        trades: total_trades,
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    println!("=== REGIME_ATR_PERIOD Extensive Sweep (Turtle-Only Live Path) ===");
    println!("AP range: 1..=80 step 1 (80 values)");
    println!("9 universes × 7 windows = 63 windows per AP value");
    println!("Fixes: LB=42, T=5.0, EP=21, ATR(24,2.0), HM=12, CAP=3, VL=96");
    println!("Expected runtime: ~20-30s (sweep profile)");
    println!();

    let loader = DataLoader::new(None, None);
    let mut sym_data: HashMap<String, SymData> = HashMap::new();
    let mut min_len = usize::MAX;

    let all_symbols: std::collections::HashSet<&str> = UNIVERSES.iter().flat_map(|(_, s)| s.iter().copied()).collect();
    for sym in all_symbols {
        let df = loader.fetch_data(sym, "1d", CANDLES).await?;
        let close = df.column("close")?.f64()?.into_no_null_iter().collect::<Vec<_>>();
        let high = df.column("high")?.f64()?.into_no_null_iter().collect::<Vec<_>>();
        let low = df.column("low")?.f64()?.into_no_null_iter().collect::<Vec<_>>();
        let vol = df.column("volume")?.f64()?.into_no_null_iter().collect::<Vec<_>>();
        if close.len() < min_len { min_len = close.len(); }
        sym_data.insert(sym.to_string(), SymData { close, high, low, vol });
    }

    let windows = (min_len.saturating_sub(TRAIN_BARS)) / TEST_BARS;
    println!("Loaded {} symbols, {} total bars, {} windows", sym_data.len(), min_len, windows);
    if windows == 0 { return Ok(()); }

    let ap_values: Vec<usize> = (1..=80).collect();
    let run_count = ap_values.len() * UNIVERSES.len() * windows;

    let mut detail_file = File::create("snapshots/ap_turtle_sweep_detail.csv")?;
    writeln!(detail_file, "ap,universe,window,equity,sharpe,dd,trades")?;

    let mut agg_map: HashMap<usize, AggResult> = HashMap::new();
    for &ap in &ap_values {
        agg_map.insert(ap, AggResult {
            ap,
            pass_count: 0,
            total_windows: 0,
            avg_sharpe: 0.0,
            avg_return: 0.0,
            avg_dd: 0.0,
            total_trades: 0,
            positive_universes: 0,
        });
    }

    // Store equity curves for baseline, winner, runner-ups
    let mut all_equity_curves: HashMap<(usize, String, usize), Vec<f64>> = HashMap::new();

    for (uni_name, u_syms) in UNIVERSES {
        let syms: Vec<String> = u_syms.iter().map(|&s| s.to_string()).collect();
        for w in 0..windows {
            let start = min_len - (windows - w) * TEST_BARS - TRAIN_BARS;
            let end = start + TEST_BARS + TRAIN_BARS;
            let test_start = start + TRAIN_BARS;

            for &ap in &ap_values {
                let mut equity_curve = Vec::new();
                let result = run_sim(&sym_data, &syms, ap, test_start, end, &mut equity_curve);

                let passed = if result.trades >= MIN_TRADES && result.sharpe > 0.0 { 1 } else { 0 };
                writeln!(detail_file, "{},{},{},{:.6},{:.4},{:.2},{}",
                    ap, uni_name, w, result.equity, result.sharpe, result.dd, result.trades)?;

                if let Some(agg) = agg_map.get_mut(&ap) {
                    agg.avg_sharpe += result.sharpe;
                    agg.avg_return += (result.equity - 1.0) * 100.0;
                    agg.avg_dd += result.dd;
                    agg.total_trades += result.trades;
                    agg.pass_count += passed;
                    agg.total_windows += 1;
                }
            }
        }
    }

    // Per-universe positive count
    for (uni_name, u_syms) in UNIVERSES {
        let syms: Vec<String> = u_syms.iter().map(|&s| s.to_string()).collect();
        for &ap in &ap_values {
            let mut pos = false;
            for w in 0..windows {
                let start = min_len - (windows - w) * TEST_BARS - TRAIN_BARS;
                let end = start + TEST_BARS + TRAIN_BARS;
                let test_start = start + TRAIN_BARS;
                let mut equity_curve = Vec::new();
                let result = run_sim(&sym_data, &syms, ap, test_start, end, &mut equity_curve);
                if result.equity > 1.0 {
                    pos = true;
                    break;
                }
            }
            if let Some(agg) = agg_map.get_mut(&ap) {
                if pos {
                    agg.positive_universes += 1;
                }
            }
        }
    }

    let n_universes = UNIVERSES.len();
    let total_windows = n_universes * windows;

    let mut summary_file = File::create("snapshots/ap_turtle_sweep_summary.csv")?;
    writeln!(summary_file, "ap,pass_count,total_windows,pass_rate,avg_sharpe,avg_return,avg_dd,total_trades,positive_universes")?;

    let mut sorted_aps: Vec<usize> = ap_values.clone();
    sorted_aps.sort_by(|&a, &b| {
        let agg_a = agg_map.get(&a).unwrap();
        let agg_b = agg_map.get(&b).unwrap();
        agg_b.pass_count.cmp(&agg_a.pass_count)
            .then(agg_b.positive_universes.cmp(&agg_a.positive_universes))
            .then(agg_b.avg_sharpe.partial_cmp(&agg_a.avg_sharpe).unwrap())
    });

    let mut winner_ap = 17usize;
    let mut winner_pass_cnt = 0usize;

    for &ap in &sorted_aps {
        let agg = agg_map.get(&ap).unwrap();
        let pass_rate = agg.pass_count as f64 / total_windows as f64 * 100.0;
        let norm_sharpe = agg.avg_sharpe / total_windows as f64;
        let norm_ret = agg.avg_return / total_windows as f64;
        let norm_dd = agg.avg_dd / total_windows as f64;

        writeln!(summary_file, "{},{},{},{:.1}%,{:.4},{:.2}%,{:.2}%,{},{}",
            agg.ap, agg.pass_count, total_windows, pass_rate,
            norm_sharpe, norm_ret, norm_dd, agg.total_trades, agg.positive_universes)?;

        if agg.pass_count > winner_pass_cnt {
            winner_pass_cnt = agg.pass_count;
            winner_ap = ap;
        }
    }

    let runner1 = *sorted_aps.get(1).unwrap_or(&17);
    let runner2 = *sorted_aps.get(2).unwrap_or(&17);

    // Write equity curves for baseline, winner, runner-ups
    for ap in [17, winner_ap, runner1, runner2] {
        let fname = format!("snapshots/ap_turtle_sweep_eq_{:03}.csv", ap);
        let mut f = File::create(&fname)?;
        writeln!(f, "universe,window,step,equity")?;
        for (uni_name, u_syms) in UNIVERSES {
            let syms: Vec<String> = u_syms.iter().map(|&s| s.to_string()).collect();
            for w in 0..windows {
                let start = min_len - (windows - w) * TEST_BARS - TRAIN_BARS;
                let end = start + TEST_BARS + TRAIN_BARS;
                let test_start = start + TRAIN_BARS;
                let mut equity_curve = Vec::new();
                let _result = run_sim(&sym_data, &syms, ap, test_start, end, &mut equity_curve);
                for (step, &eq_val) in equity_curve.iter().enumerate() {
                    writeln!(f, "{},{},{},{:.6}", uni_name, w, step, eq_val)?;
                }
            }
        }
    }

    let baseline_pass = agg_map.get(&17).unwrap().pass_count;
    let winner_pass = agg_map.get(&winner_ap).unwrap().pass_count;
    let baseline_sharpe = agg_map.get(&17).unwrap().avg_sharpe / total_windows as f64;
    let winner_sharpe = agg_map.get(&winner_ap).unwrap().avg_sharpe / total_windows as f64;

    println!();
    println!("=== TOP 10 AP VALUES BY PASS RATE ===");
    println!("{:>4} {:>6} {:>8} {:>10} {:>12} {:>10}", "AP", "Pass", "Pass%", "AvgSharpe", "AvgReturn%", "AvgDD%");
    for &ap in sorted_aps.iter().take(10) {
        let agg = agg_map.get(&ap).unwrap();
        let pass_rate = agg.pass_count as f64 / total_windows as f64 * 100.0;
        let ns = agg.avg_sharpe / total_windows as f64;
        let nr = agg.avg_return / total_windows as f64;
        let nd = agg.avg_dd / total_windows as f64;
        println!("{:>4} {:>6} {:>7.1}% {:>10.3} {:>11.2}% {:>9.2}%", ap, agg.pass_count, pass_rate, ns, nr, nd);
    }

    println!();
    println!("Baseline (AP=17):  {} / {} ({:.1}%) pass, Sharpe {:.3}",
        baseline_pass, total_windows,
        baseline_pass as f64 / total_windows as f64 * 100.0, baseline_sharpe);
    println!("Winner (AP={}): {} / {} ({:.1}%) pass, Sharpe {:.3}, ΔSharpe {:+.3}",
        winner_ap, winner_pass, total_windows,
        winner_pass as f64 / total_windows as f64 * 100.0, winner_sharpe, winner_sharpe - baseline_sharpe);
    println!("Winner delta: {} more passes vs baseline", winner_pass as isize - baseline_pass as isize);
    println!();
    println!("Runner-up 1: AP={} ({} passes)", runner1, agg_map.get(&runner1).unwrap().pass_count);
    println!("Runner-up 2: AP={} ({} passes)", runner2, agg_map.get(&runner2).unwrap().pass_count);

    // Generate markdown report
    let mut md_file = File::create("snapshots/ap_turtle_sweep_report.md")?;
    writeln!(md_file, "# REGIME_ATR_PERIOD Extensive Sweep — Turtle-Only Live Path")?;
    writeln!(md_file)?;
    writeln!(md_file, "**Date:** 2026-05-05")?;
    writeln!(md_file, "**Scope:** AP ∈ [1..=80 step 1] × 9 universes × {} WF windows = {} runs", windows, run_count)?;
    writeln!(md_file, "**Harness:** Turtle-only live exit (matches `src/live/bot.rs`)")?;
    writeln!(md_file)?;
    writeln!(md_file, "## Fixed Params")?;
    writeln!(md_file, "| Param | Value | Notes |")?;
    writeln!(md_file, "|-------|-------|-------|")?;
    writeln!(md_file, "| REGIME_LOOKBACK | {} | Production default (settled 2026-05-02) |", REGIME_LOOKBACK)?;
    writeln!(md_file, "| ATR_RANK_THRESHOLD | {} | Production default (settled 2026-05-04) |", ATR_RANK_T)?;
    writeln!(md_file, "| VOL_LOOKBACK | {} | Production default |", VOL_LOOKBACK)?;
    writeln!(md_file, "| TURTLE_ENTRY | {} | Production default |", TURTLE_ENTRY)?;
    writeln!(md_file, "| TURTLE_ATR_P | {} | Production default |", TURTLE_ATR_PERIOD)?;
    writeln!(md_file, "| TURTLE_ATR_M | {} | Production default |", TURTLE_ATR_MULT)?;
    writeln!(md_file, "| HOLD_MAX | {} | Production default |", HOLD_MAX)?;
    writeln!(md_file, "| POSITION_CAP | {} | Production default |", POSITION_CAP)?;
    writeln!(md_file)?;
    writeln!(md_file, "## Top 10 Results (by pass rate)")?;
    writeln!(md_file, "| AP | Pass | Pass% | AvgSharpe | AvgReturn% | AvgDD% |")?;
    writeln!(md_file, "|----|------|-------|-----------|------------|--------|")?;
    for &ap in sorted_aps.iter().take(10) {
        let agg = agg_map.get(&ap).unwrap();
        let pass_rate = agg.pass_count as f64 / total_windows as f64 * 100.0;
        let ns = agg.avg_sharpe / total_windows as f64;
        let nr = agg.avg_return / total_windows as f64;
        let nd = agg.avg_dd / total_windows as f64;
        let badge = if ap == winner_ap { " **WINNER**" } else if ap == 17 { " (baseline)" } else { "" };
        writeln!(md_file, "| {}{} | {} | {:.1}% | {:.3} | {:.2}% | {:.2}% |", ap, badge, agg.pass_count, pass_rate, ns, nr, nd)?;
    }
    writeln!(md_file)?;
    writeln!(md_file, "## Winner vs Baseline")?;
    writeln!(md_file, "| Metric | Baseline (AP=17) | Winner (AP={}) | Delta |", winner_ap)?;
    writeln!(md_file, "|--------|-----------------|--------------|-------|")?;
    writeln!(md_file, "| Pass | {} / {} | {} / {} | {:+} |",
        baseline_pass, total_windows, winner_pass, total_windows,
        winner_pass as isize - baseline_pass as isize)?;
    writeln!(md_file, "| Pass% | {:.1}% | {:.1}% | {:+.1}pp |",
        baseline_pass as f64 / total_windows as f64 * 100.0,
        winner_pass as f64 / total_windows as f64 * 100.0,
        (winner_pass as f64 - baseline_pass as f64) / total_windows as f64 * 100.0)?;
    writeln!(md_file, "| Avg Sharpe | {:.3} | {:.3} | {:+.3} |", baseline_sharpe, winner_sharpe, winner_sharpe - baseline_sharpe)?;
    writeln!(md_file, "| Avg Return% | {:.2}% | {:.2}% | {:+.2}pp |",
        agg_map.get(&17).unwrap().avg_return / total_windows as f64,
        agg_map.get(&winner_ap).unwrap().avg_return / total_windows as f64,
        (agg_map.get(&winner_ap).unwrap().avg_return - agg_map.get(&17).unwrap().avg_return) / total_windows as f64)?;
    writeln!(md_file, "| Positive Univs | {} / {} | {} / {} | {:+} |",
        agg_map.get(&17).unwrap().positive_universes, n_universes,
        agg_map.get(&winner_ap).unwrap().positive_universes, n_universes,
        agg_map.get(&winner_ap).unwrap().positive_universes as isize - agg_map.get(&17).unwrap().positive_universes as isize)?;
    writeln!(md_file)?;
    let recommendation = if winner_ap == 17 {
        "No change — AP=17 remains production default.".to_string()
    } else {
        format!("Update production: AP=17 → AP={}", winner_ap)
    };
    writeln!(md_file, "## Recommendation")?;
    writeln!(md_file, "{}", recommendation)?;
    writeln!(md_file)?;
    writeln!(md_file, "## Files")?;
    writeln!(md_file, "- `snapshots/ap_turtle_sweep_detail.csv` — per-window detail")?;
    writeln!(md_file, "- `snapshots/ap_turtle_sweep_summary.csv` — aggregated by AP")?;
    writeln!(md_file, "- `snapshots/ap_turtle_sweep_eq_017.csv` — baseline (AP=17) equity time-series")?;
    writeln!(md_file, "- `snapshots/ap_turtle_sweep_eq_{:03}.csv` — winner equity time-series", winner_ap)?;
    writeln!(md_file, "- `snapshots/ap_turtle_sweep_eq_{:03}.csv` — runner-up 1 equity", runner1)?;
    writeln!(md_file, "- `snapshots/ap_turtle_sweep_eq_{:03}.csv` — runner-up 2 equity", runner2)?;

    println!();
    println!("Done. Files written:");
    println!("  snapshots/ap_turtle_sweep_detail.csv — per-window detail");
    println!("  snapshots/ap_turtle_sweep_summary.csv — aggregated by AP");
    println!("  snapshots/ap_turtle_sweep_eq_017.csv — baseline equity");
    println!("  snapshots/ap_turtle_sweep_eq_{:03}.csv — winner equity", winner_ap);
    println!("  snapshots/ap_turtle_sweep_eq_{:03}.csv — runner-up 1", runner1);
    println!("  snapshots/ap_turtle_sweep_eq_{:03}.csv — runner-up 2", runner2);
    println!("  snapshots/ap_turtle_sweep_report.md — full report");

    Ok(())
}
