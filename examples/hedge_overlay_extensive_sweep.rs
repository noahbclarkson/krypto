//! T68: Hedge Overlay Hyperopt — Extensive Parameter Sweep
//!
//! AUDIT FINDING: The USDT hedge overlay in bot.rs has NEVER been optimized:
//! - Hedge percentile threshold: hardcoded 0.75 (75th pct)
//! - Hedge lookback: hardcoded 252 bars
//! - SIZE_MULT: hardcoded 0.70 (position reduction when triggered)
//!
//! All three are magic numbers pulled from thin air.
//!
//! This harness sweeps:
//!   hedge_pct ∈ {0.50, 0.55, 0.60, 0.65, 0.70, 0.75, 0.80, 0.85, 0.90, 0.95}
//!   hedge_lookback ∈ {63, 126, 189, 252, 504}
//!   SIZE_MULT = 0.70 (confirmed inert in prior sweep — fixed)
//!
//! Strategy: Turtle breakout + ATR_RANK(17, 42, 5.0) entry gate + Turtle-only exit
//! Matches live_compatible_wf.rs exactly (production params).
//!
//! Exports: snapshots/hedge_overlay_sweep.csv (all 50 runs)
//!          snapshots/hedge_overlay_pct_summary.csv (aggregated by hedge_pct)
//!          snapshots/hedge_overlay_equity_curves.csv (top 4 configs + baseline)

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
const VOL_LOOKBACK: usize = 96;

const REGIME_ATR_PERIOD: usize = 17;
const REGIME_LOOKBACK: usize = 42;
const ATR_RANK_T: f64 = 5.0;

const SIZE_MULT: f64 = 0.70; // confirmed inert — fixed

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

#[derive(Clone)]
struct SymData {
    close: Vec<f64>,
    high: Vec<f64>,
    low: Vec<f64>,
    vol: Vec<f64>,
}

#[derive(Clone, Copy)]
struct WfResult {
    equity: f64,
    sharpe: f64,
    dd: f64,
    trades: usize,
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

fn turtle_signal(
    close: &[f64], high: &[f64], low: &[f64],
    entry_period: usize, atr_period: usize, atr_mult: f64, idx: usize
) -> bool {
    if idx < entry_period { return false; }
    let start = idx - entry_period;
    let max_close = close[start..idx].iter().fold(f64::NEG_INFINITY, |a, &b| a.max(b));
    if close[idx] > max_close {
        if atr_mult > 0.0 {
            let atr = atr_at(high, low, close, atr_period, idx);
            if close[idx] < max_close + atr * atr_mult {
                return false;
            }
        }
        return true;
    }
    false
}

fn btc_atr_pct(btc_data: &SymData, period: usize, lookback: usize, idx: usize) -> f64 {
    if idx < period.max(lookback) { return 50.0; }
    let curr_atr = atr_at(&btc_data.high, &btc_data.low, &btc_data.close, period, idx);
    let mut hist = Vec::with_capacity(lookback);
    for j in (idx + 1 - lookback)..=idx {
        if j >= period {
            hist.push(atr_at(&btc_data.high, &btc_data.low, &btc_data.close, period, j));
        }
    }
    if hist.is_empty() { return 50.0; }
    let count = hist.iter().filter(|&&x| x < curr_atr).count();
    100.0 * count as f64 / hist.len() as f64
}

/// Compute hedge overlay ATR percentile for a given lookback
fn hedge_atr_pct(btc_data: &SymData, atr_period: usize, lookback: usize, idx: usize) -> f64 {
    if idx < atr_period.max(lookback) { return 50.0; }
    let curr_atr = atr_at(&btc_data.high, &btc_data.low, &btc_data.close, atr_period, idx);
    let mut hist = Vec::with_capacity(lookback);
    for j in (idx + 1 - lookback)..=idx {
        if j >= atr_period {
            hist.push(atr_at(&btc_data.high, &btc_data.low, &btc_data.close, atr_period, j));
        }
    }
    if hist.is_empty() { return 50.0; }
    hist.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let count = hist.iter().filter(|&&x| x < curr_atr).count();
    100.0 * count as f64 / hist.len() as f64
}

fn annualised_sharpe(daily_rets: &[f64]) -> f64 {
    if daily_rets.len() < 10 { return 0.0; }
    let mean = daily_rets.iter().sum::<f64>() / daily_rets.len() as f64;
    let std = (daily_rets.iter().map(|r| { let d = r - mean; d * d }).sum::<f64>() / daily_rets.len() as f64).sqrt();
    if std == 0.0 { 0.0 } else { mean / std * (252.0_f64.sqrt()) }
}

fn max_dd_from(equity: &[f64]) -> f64 {
    let mut peak = f64::NEG_INFINITY;
    let mut max_dd = 0.0;
    for &e in equity {
        if e > peak { peak = e; }
        let dd = (peak - e) / peak;
        if dd > max_dd { max_dd = dd; }
    }
    max_dd
}

fn run_sim(
    sym_data: &HashMap<String, SymData>,
    symbols: &[String],
    test_start: usize,
    test_end: usize,
    hedge_pct: f64,
    hedge_lookback: usize,
) -> WfResult {
    let mut equity = 1.0_f64;
    let mut equity_curve = vec![1.0_f64];
    let mut peak = equity;
    let mut total_trades = 0usize;
    let mut daily_rets = Vec::new();

    let warmup = REGIME_ATR_PERIOD + REGIME_LOOKBACK + 2;
    let hedge_warmup = hedge_lookback + 21 + 2;
    let bar_start = test_start.max(warmup).max(hedge_warmup);

    let mut bar = bar_start;
    while bar + 2 < test_end {
        let btc = sym_data.get("BTCUSDT");
        let btc_pct = if let Some(b) = btc { btc_atr_pct(b, REGIME_ATR_PERIOD, REGIME_LOOKBACK, bar) } else { 50.0 };

        if btc_pct < ATR_RANK_T {
            equity_curve.push(equity);
            bar += 1;
            continue;
        }

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

                        // Hedge overlay: parameterised
                        let mut size_mult = 1.0;
                        if let Some(b) = btc {
                            if bar >= hedge_lookback + 21 {
                                let btc_pct_rank = hedge_atr_pct(b, 21, hedge_lookback, bar);
                                if btc_pct_rank > hedge_pct * 100.0 {
                                    size_mult = SIZE_MULT;
                                }
                            }
                        }

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
                            let gross_ret = pct_ret * size_mult;

                            let bars_held = (exit_bar as i64 - entry_bar_next as i64).max(1) as usize;

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

    WfResult {
        equity,
        sharpe: annualised_sharpe(&daily_rets),
        dd: max_dd_from(&equity_curve),
        trades: total_trades,
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    println!("T68: Hedge Overlay Extensive Sweep");
    println!("=================================");
    println!("Parameters being tested:");
    println!("  hedge_pct: {{0.50, 0.55, 0.60, 0.65, 0.70, 0.75, 0.80, 0.85, 0.90, 0.95}}");
    println!("  hedge_lookback: {{63, 126, 189, 252, 504}}");
    println!("  SIZE_MULT: 0.70 (fixed — confirmed inert)");
    println!();

    // Load data
    println!("Loading data...");
    let loader = DataLoader::new(None, None);
    let mut sym_data = HashMap::new();
    let mut min_len = usize::MAX;

    let all_symbols = UNIVERSES.iter()
        .flat_map(|(_, s)| s.iter())
        .collect::<std::collections::HashSet<_>>();

    for &sym in &all_symbols {
        let df = loader.fetch_data(sym, "1d", CANDLES).await?;
        let close = df.column("close")?.f64()?.into_no_null_iter().collect::<Vec<_>>();
        let high = df.column("high")?.f64()?.into_no_null_iter().collect::<Vec<_>>();
        let low  = df.column("low")?.f64()?.into_no_null_iter().collect::<Vec<_>>();
        let vol  = df.column("volume")?.f64()?.into_no_null_iter().collect::<Vec<_>>();
        if close.len() < min_len { min_len = close.len(); }
        sym_data.insert(sym.to_string(), SymData { close, high, low, vol });
    }
    println!("Loaded {} symbols, {} bars each", sym_data.len(), min_len);

    let windows = (min_len.saturating_sub(TRAIN_BARS)) / TEST_BARS;
    println!("Walk-forward windows: {}", windows);

    // Config grid: 10 pct × 5 lookback = 50 configs
    let pct_values: [f64; 10] = [0.50, 0.55, 0.60, 0.65, 0.70, 0.75, 0.80, 0.85, 0.90, 0.95];
    let lookback_values: [usize; 5] = [63, 126, 189, 252, 504];

    // Results: config → (avg_sharpe, avg_return, avg_dd, pass_count, total_runs, avg_trades)
    let mut config_results: HashMap<String, (f64, f64, f64, usize, usize, f64)> = HashMap::new();

    // Also store per-universe for equity curve tracking
    // baseline = pct=0.75, lb=252 (current live bot values)
    let baseline_pct = 0.75_f64;
    let baseline_lb = 252_usize;

    let total_configs = pct_values.len() * lookback_values.len();
    let mut config_idx = 0;

    for &hedge_pct in &pct_values {
        for &hedge_lb in &lookback_values {
            config_idx += 1;
            let label = format!("pct={:.2}_lb={}", hedge_pct, hedge_lb);

            let mut pass_count = 0usize;
            let mut total_runs = 0usize;
            let mut sum_sharpe = 0.0_f64;
            let mut sum_return = 0.0_f64;
            let mut sum_dd = 0.0_f64;
            let mut sum_trades = 0.0_f64;

            print!("\r[{}/{}] Testing {}", config_idx, total_configs, label);
            std::io::stdout().flush().unwrap();

            for (_, u_syms) in UNIVERSES {
                let syms: Vec<String> = u_syms.iter().map(|&s| s.to_string()).collect();

                for w in 0..windows {
                    let start = min_len - (windows - w) * TEST_BARS - TRAIN_BARS;
                    let end = start + TRAIN_BARS + TEST_BARS;
                    let test_start = start + TRAIN_BARS;

                    let res = run_sim(&sym_data, &syms, test_start, end, hedge_pct, hedge_lb);

                    let passed = res.trades >= MIN_TRADES && res.sharpe > 0.0 && res.equity > 1.0;
                    if passed { pass_count += 1; }
                    total_runs += 1;
                    sum_sharpe += res.sharpe;
                    sum_return += res.equity - 1.0;
                    sum_dd += res.dd;
                    sum_trades += res.trades as f64;
                }
            }

            let n = UNIVERSES.len() * windows;
            let n_f = n as f64;
            config_results.insert(label.clone(), (
                sum_sharpe / n_f,
                sum_return / n_f,
                sum_dd / n_f,
                pass_count,
                total_runs,
                sum_trades / n_f,
            ));
        }
    }

    println!("\n\n=== SWEEP COMPLETE ===\n");

    // Sort by pass rate, then Sharpe
    let mut sorted: Vec<_> = config_results.iter().collect();
    sorted.sort_by(|a, b| {
        let pa = a.1.3 as f64 / a.1.4 as f64;
        let pb = b.1.3 as f64 / b.1.4 as f64;
        pb.partial_cmp(&pa).unwrap()
            .then_with(|| b.1.0.partial_cmp(&a.1.0).unwrap())
    });

    // Write full sweep CSV
    let mut csv = File::create("snapshots/hedge_overlay_sweep.csv")?;
    writeln!(csv, "config,hedge_pct,hedge_lookback,avg_sharpe,avg_return,avg_dd,pass_count,total_runs,pass_pct,avg_trades")?;
    for (label, (sh, re, dd, pa, to, tr)) in &sorted {
        let parts: Vec<&str> = label.split('_').collect();
        let pct: f64 = parts[0].trim_start_matches("pct=").parse().unwrap_or(0.0);
        let lb: usize = parts[1].trim_start_matches("lb=").parse().unwrap_or(0);
        let pp = *pa as f64 / *to as f64 * 100.0;
        writeln!(csv, "{},{:.2},{},{:.6},{:.6},{:.6},{},{},{:.2},{:.1}", label, pct, lb, sh, re, dd, pa, to, pp, tr)?;
    }

    // Aggregate by pct
    // Aggregate by pct (use String key to avoid f64 Hash issues)
    let mut pct_agg: HashMap<String, (f64, f64, f64, usize, usize, f64)> = HashMap::new();
    for (label, (sh, re, dd, pa, to, tr)) in &config_results {
        let parts: Vec<&str> = label.split('_').collect();
        let pct_key = parts[0].trim_start_matches("pct=").to_string();
        let entry = pct_agg.entry(pct_key).or_insert((0.0, 0.0, 0.0, 0, 0, 0.0));
        entry.0 += sh;
        entry.1 += re;
        entry.2 += dd;
        entry.3 += pa;
        entry.4 += to;
        entry.5 += tr;
    }
    let n_lb = lookback_values.len() as f64;
    let mut pct_summary: Vec<(f64, f64, f64, f64, usize, usize, f64)> = pct_agg.into_iter()
        .map(|(pct_key, (sh, re, dd, pa, to, tr))| {
            let pct: f64 = pct_key.trim_start_matches("pct=").parse().unwrap_or(0.0);
            (pct, sh/n_lb, re/n_lb, dd/n_lb, pa, to, tr/n_lb)
        })
        .collect();
    pct_summary.sort_by(|a, b| {
        let pa = a.4 as f64 / a.5 as f64;
        let pb = b.4 as f64 / b.5 as f64;
        pb.partial_cmp(&pa).unwrap()
            .then_with(|| b.1.partial_cmp(&a.1).unwrap())
    });

    let mut pct_csv = File::create("snapshots/hedge_overlay_pct_summary.csv")?;
    writeln!(pct_csv, "hedge_pct,avg_sharpe,avg_return,avg_dd,pass_count,total_runs,pass_pct,avg_trades")?;
    for (pct, sh, re, dd, pa, to, tr) in &pct_summary {
        let pp = *pa as f64 / *to as f64 * 100.0;
        writeln!(pct_csv, "{:.2},{:.6},{:.6},{:.6},{},{},{:.2},{:.1}", pct, sh, re, dd, pa, to, pp, tr)?;
    }

    // Top configs
    println!("\n=== TOP 10 BY PASS RATE + SHARPE ===");
    println!("{:30} {:>8} {:>8} {:>10} {:>10} {:>8} {:>8}", "Config", "Pct%", "LB", "Sharpe", "Return%", "DD%", "Trades");
    for (label, (sh, re, dd, pa, to, tr)) in sorted.iter().take(10) {
        let parts: Vec<&str> = label.split('_').collect();
        let pct_v: f64 = parts[0].trim_start_matches("pct=").parse().unwrap_or(0.0);
        let lb_v: usize = parts[1].trim_start_matches("lb=").parse().unwrap_or(0);
        let pp = *pa as f64 / *to as f64 * 100.0;
        println!("{:30} {:.0} {:>8} {:.3} {:>10.1}% {:>8.1}% {:.0}",
            label, pct_v * 100.0, lb_v, sh, re * 100.0, dd * 100.0, tr);
    }

    println!("\n=== AGGREGATED BY HEDGE PERCENTILE (FIXED LB) ===");
    println!("{:>8} {:>10} {:>10} {:>8} {:>10} {:>8} {:>8}", "Pct", "Sharpe", "Return%", "Pass%", "DD%", "Trades", "Conf");
    for (pct, sh, re, dd, pa, to, tr) in &pct_summary {
        let pp_pct = *pa as f64 / *to as f64 * 100.0;
        // Count how many lookback configs for this pct pass (at passing threshold)
        let passing_lb = sorted.iter()
            .filter(|(l, (_, _, _, p, _, _))| {
                let lparts: Vec<&str> = l.split('_').collect();
                let lp: f64 = lparts[0].trim_start_matches("pct=").parse().unwrap_or(0.0);
                (lp * 100.0).round() == (pct * 100.0).round() && *p > 0
            })
            .count();
        println!("pct={:.0}% pass={:.0}% sharpe={:.3} ret={:.1}% dd={:.1}% trades={:.0} lb_configs={}/{}",
            pct * 100.0, pp_pct, sh, re * 100.0, dd * 100.0, tr, passing_lb, lookback_values.len());
    }


    // Equity curves for baseline, winner, runner-ups
    let targets: Vec<(f64, usize, &str)> = vec![
        (baseline_pct, baseline_lb, "BASELINE (pct=0.75, lb=252)"),
    ];
    // Add top 3 from sweep (if different from baseline)
    let mut added = 0;
    for (label, _) in sorted.iter() {
        if added >= 3 { break; }
        let parts: Vec<&str> = label.split('_').collect();
        let p: f64 = parts[0].trim_start_matches("pct=").parse().unwrap_or(0.0);
        let l: usize = parts[1].trim_start_matches("lb=").parse().unwrap_or(0);
        if p == baseline_pct && l == baseline_lb { continue; }
        let desc = if added == 0 { "WINNER" }
                   else if added == 1 { "RUNNER-UP-1" }
                   else { "RUNNER-UP-2" };
        println!("\nEquity curve for {}: pct={:.2}, lb={}", desc, p, l);
        added += 1;
    }

    // Actually generate equity curves
    let mut equity_csv = File::create("snapshots/hedge_overlay_equity_curves.csv")?;
    writeln!(equity_csv, "bar,equity,config,label")?;

    // Baseline
    {
        let syms: Vec<String> = UNIVERSES[0].1.iter().map(|&s| s.to_string()).collect();
        let mut agg_equity = 1.0_f64;
        let mut bar_num = 0usize;

        for w in 0..windows {
            let start = min_len - (windows - w) * TEST_BARS - TRAIN_BARS;
            let end = start + TRAIN_BARS + TEST_BARS;
            let test_start = start + TRAIN_BARS;
            let res = run_sim(&sym_data, &syms, test_start, end, baseline_pct, baseline_lb);
            agg_equity *= res.equity;
            writeln!(equity_csv, "{},{},{},{}", bar_num, agg_equity, "BASELINE", "BASELINE (0.75/252)")?;
            bar_num += 1;
        }
        println!("Baseline equity curve: {} windows, final equity {:.4}x", windows, agg_equity);
    }

    // Winner and runner-ups
    added = 0;
    for (label, _) in sorted.iter() {
        if added >= 3 { break; }
        let parts: Vec<&str> = label.split('_').collect();
        let p: f64 = parts[0].trim_start_matches("pct=").parse().unwrap_or(0.0);
        let l: usize = parts[1].trim_start_matches("lb=").parse().unwrap_or(0);
        if p == baseline_pct && l == baseline_lb { continue; }
        let desc = if added == 0 { "WINNER" } else if added == 1 { "RUNNER-UP-1" } else { "RUNNER-UP-2" };

        let syms: Vec<String> = UNIVERSES[0].1.iter().map(|&s| s.to_string()).collect();
        let mut agg_equity = 1.0_f64;
        let mut bar_num = 0usize;

        for w in 0..windows {
            let start = min_len - (windows - w) * TEST_BARS - TRAIN_BARS;
            let end = start + TRAIN_BARS + TEST_BARS;
            let test_start = start + TRAIN_BARS;
            let res = run_sim(&sym_data, &syms, test_start, end, p, l);
            agg_equity *= res.equity;
            writeln!(equity_csv, "{},{},{},{}", bar_num, agg_equity, label, desc)?;
            bar_num += 1;
        }
        println!("{} equity curve (pct={:.2}, lb={}): {:.4}x", desc, p, l, agg_equity);
        added += 1;
    }

    // Winner info
    let (best_label, (best_sh, best_re, best_dd, best_pa, best_to, best_tr)) = &sorted[0];
    let best_pp = *best_pa as f64 / *best_to as f64 * 100.0;
    println!("\n=== WINNER ===");
    println!("{}: pass={}/{} ({:.0}%), Sharpe={:.3}, Ret={:.1}%, DD={:.1}%",
        best_label, best_pa, best_to, best_pp, best_sh, best_re * 100.0, best_dd * 100.0);

    println!("\nOutput files:");
    println!("  snapshots/hedge_overlay_sweep.csv");
    println!("  snapshots/hedge_overlay_pct_summary.csv");
    println!("  snapshots/hedge_overlay_equity_curves.csv");

    Ok(())
}
