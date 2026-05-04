//! AP Hyperopt — REGIME_ATR_PERIOD on Turtle-Only Live Path
//!
//! REGIME_ATR_PERIOD (AP) controls the smoothing of BTC's ATR used in the
//! ATR percentile rank regime gate. Higher AP = smoother ATR = fewer regime
//! transitions = less frequent gate triggering.
//!
//! Prior result (2026-05-02): AP=64 won 55/63 pass vs AP=12 at 53/63 on the
//! same harness. This has the SAME PATTERN as EP=24 (in-sample inflation).
//! We need a proper robustness-first selection: test AP 1..=80,
//! select based on pass rate, then validate against held-out data.
//!
//! EXPORTS: per-window equity curves for baseline (AP=12) and top-5 AP candidates
//! so we can chart them properly.

use anyhow::Result;
use krypto::data::loader::DataLoader;
use polars::prelude::*;
use rayon::prelude::*;
use std::collections::HashMap;
use std::fs::File;
use std::io::Write;
use std::sync::Mutex;

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
const REGIME_LOOKBACK: usize = 42;
const ATR_RANK_T: f64 = 5.0;

// AP sweep range: 1..=80 step 1
const AP_MIN: usize = 1;
const AP_MAX: usize = 80;
const AP_STEP: usize = 1;

// Baseline for comparison
const AP_BASELINE: usize = 12;

// Equity export: top-N candidates + baseline
const EXPORT_TOP_N: usize = 5;

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
    let mut peak = equity.first().copied().unwrap_or(1.0);
    for &val in equity {
        if val > peak { peak = val; }
        let dd = 1.0 - val / peak;
        if dd > max_dd { max_dd = dd; }
    }
    max_dd * 100.0
}

#[derive(Default, Clone)]
struct WfResult {
    equity: f64,
    sharpe: f64,
    dd: f64,
    trades: usize,
    equity_curve: Vec<f64>, // per-bar equity during test window
}

impl WfResult {
    fn passes(&self) -> bool {
        self.trades >= MIN_TRADES && self.sharpe > 0.0
    }
}

/// Run simulation for a SINGLE universe, SINGLE AP value, ALL windows.
/// Returns per-window results and per-universe aggregate.
fn run_universe_ap(
    sym_data: &HashMap<String, SymData>,
    symbols: &[String],
    ap: usize,
    n_windows: usize,
    min_len: usize,
) -> Vec<WfResult> {
    let mut results = Vec::with_capacity(n_windows);
    
    for w in 0..n_windows {
        let start = min_len - (n_windows - w) * TEST_BARS - TRAIN_BARS;
        let end = start + TEST_BARS + TRAIN_BARS;
        let test_start = start + TRAIN_BARS;
        
        let res = run_sim_full_equity(sym_data, symbols, ap, test_start, end);
        results.push(res);
    }
    
    results
}

/// Run simulation returning full per-bar equity curve for the test window.
fn run_sim_full_equity(
    sym_data: &HashMap<String, SymData>,
    symbols: &[String],
    ap: usize,
    test_start: usize,
    test_end: usize,
) -> WfResult {
    let mut equity = 1.0_f64;
    let mut equity_curve = vec![1.0_f64];
    let mut peak = equity;
    let mut total_trades = 0usize;
    let mut daily_rets = Vec::new();

    let mut bar = test_start;
    while bar + 2 < test_end {
        let btc = sym_data.get("BTCUSDT");
        let btc_pct = if let Some(b) = btc { 
            btc_atr_pct(b, ap, REGIME_LOOKBACK, bar) 
        } else { 
            50.0 
        };
        
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
                        let mut size_mult = 1.0;
                        if let Some(b) = btc {
                            if bar >= 252 + 21 {
                                let atr_21 = atr_at(&b.high, &b.low, &b.close, 21, bar);
                                let mut hist = Vec::with_capacity(252);
                                for j in (bar + 1 - 252)..=bar {
                                    let h = b.high[j];
                                    let l = b.low[j];
                                    let c0 = b.close[j.saturating_sub(1)];
                                    hist.push((h - l).max((h - c0).abs()).max((l - c0).abs()));
                                }
                                hist.sort_by(|a, b| a.partial_cmp(b).unwrap());
                                let pct_75 = hist[(0.75 * hist.len() as f64) as usize];
                                if atr_21 > pct_75 {
                                    size_mult = 0.70;
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
        equity_curve,
    }
}

struct ApResult {
    ap: usize,
    pass: usize,
    total: usize,
    sharpe: f64,
    ret: f64,
    dd: f64,
    trades: usize,
}

#[tokio::main]
async fn main() -> Result<()> {
    println!("Loading data...");
    let loader = DataLoader::new(None, None);
    let mut sym_data = HashMap::new();
    let mut min_len = usize::MAX;

    let all_symbols = UNIVERSES.iter().flat_map(|(_, s)| s.iter()).map(|&s| s).collect::<std::collections::HashSet<_>>();
    for &sym in &all_symbols {
        let df = loader.fetch_data(sym, "1d", CANDLES).await?;
        let close = df.column("close")?.f64()?.into_no_null_iter().collect::<Vec<_>>();
        let high = df.column("high")?.f64()?.into_no_null_iter().collect::<Vec<_>>();
        let low = df.column("low")?.f64()?.into_no_null_iter().collect::<Vec<_>>();
        let vol = df.column("volume")?.f64()?.into_no_null_iter().collect::<Vec<_>>();
        if close.len() < min_len { min_len = close.len(); }
        sym_data.insert(sym.to_string(), SymData { close, high, low, vol });
    }

    let n_windows = (min_len.saturating_sub(TRAIN_BARS)) / TEST_BARS;
    if n_windows == 0 { 
        println!("Not enough data for walk-forward");
        return Ok(());
    }

    // Build AP sweep list
    let ap_values: Vec<usize> = (AP_MIN..=AP_MAX).step_by(AP_STEP).collect();
    let n_aps = ap_values.len();
    println!("Sweeping {} AP values: {:?} ... {:?}", n_aps, 
        ap_values.first(), ap_values.last());
    println!("{} universes × {} windows × {} AP = {} runs per-universe",
        UNIVERSES.len(), n_windows, n_aps, UNIVERSES.len() * n_windows * n_aps);

    // Results: AP -> per-universe pass counts and aggregate stats
    let ap_results: Mutex<Vec<ApResult>> = Mutex::new(Vec::with_capacity(n_aps));
    
    // Sweep all AP values
    for &ap in &ap_values {
        let mut ap_passes = 0usize;
        let mut ap_total = 0usize;
        let mut ap_sharpes = vec![];
        let mut ap_rets = vec![];
        let mut ap_dds = vec![];
        let mut ap_trades = 0usize;

        // Run all universes in parallel
        let universe_results: Vec<(String, Vec<WfResult>)> = UNIVERSES
            .par_iter()
            .map(|(u_name, u_syms)| {
                let syms: Vec<String> = u_syms.iter().map(|&s| s.to_string()).collect();
                let results = run_universe_ap(&sym_data, &syms, ap, n_windows, min_len);
                (u_name.to_string(), results)
            })
            .collect();

        for (u_name, results) in universe_results {
            let mut u_passes = 0usize;
            for res in &results {
                if res.passes() { u_passes += 1; }
                ap_sharpes.push(res.sharpe);
                ap_rets.push((res.equity - 1.0) * 100.0);
                ap_dds.push(res.dd);
            }
            ap_total += n_windows;
            ap_passes += u_passes;
            ap_trades += results.iter().map(|r| r.trades).sum::<usize>();
        }

        let total_windows = UNIVERSES.len() * n_windows;
        let avg_sharpe = ap_sharpes.iter().sum::<f64>() / total_windows as f64;
        let avg_ret = ap_rets.iter().sum::<f64>() / total_windows as f64;
        let avg_dd = ap_dds.iter().sum::<f64>() / total_windows as f64;

        ap_results.lock().unwrap().push(ApResult {
            ap,
            pass: ap_passes,
            total: ap_total,
            sharpe: avg_sharpe,
            ret: avg_ret,
            dd: avg_dd,
            trades: ap_trades,
        });

        if ap % 10 == 0 || ap == AP_MIN || ap == AP_MAX {
            println!("AP={}: {}/{} pass ({:.1}%), Sharpe={:.3}, Ret={:.1}%, DD={:.1}%, Trades={}",
                ap, ap_passes, ap_total, 
                (ap_passes as f64 / ap_total as f64) * 100.0,
                avg_sharpe, avg_ret, avg_dd, ap_trades);
        }
    }

    // Sort by pass rate desc, then Sharpe desc
    let mut sorted = ap_results.into_inner().unwrap();
    sorted.sort_by(|a, b| {
        let ra = a.pass as f64 / a.total as f64;
        let rb = b.pass as f64 / b.total as f64;
        rb.partial_cmp(&ra).unwrap()
            .then_with(|| b.sharpe.partial_cmp(&a.sharpe).unwrap())
    });

    // Top N candidates for equity export
    let export_aps: Vec<usize> = sorted.iter()
        .take(EXPORT_TOP_N)
        .map(|r| r.ap)
        .collect();
    
    // Always include baseline
    let baseline_included = export_aps.contains(&AP_BASELINE);
    let export_aps: Vec<usize> = if baseline_included {
        export_aps
    } else {
        let mut v = export_aps;
        v.push(AP_BASELINE);
        v
    };
    
    println!("\n=== TOP {} AP CANDIDATES ===", EXPORT_TOP_N);
    for (i, r) in sorted.iter().take(10).enumerate() {
        println!("{:2}. AP={:3}: {}/{} pass ({:5.1}%), Sharpe={:7.3}, Ret={:7.1}%, DD={:5.1}%, Trades={}",
            i+1, r.ap, r.pass, r.total,
            (r.pass as f64 / r.total as f64) * 100.0,
            r.sharpe, r.ret, r.dd, r.trades);
    }

    // Export per-universe equity curves for top APs + baseline
    println!("\nExporting equity curves for APs: {:?}", export_aps);
    
    for &ap in &export_aps {
        for (u_name, u_syms) in UNIVERSES {
            let syms: Vec<String> = u_syms.iter().map(|&s| s.to_string()).collect();
            let results = run_universe_ap(&sym_data, &syms, ap, n_windows, min_len);
            
            let csv_path = format!("snapshots/ap_hyperopt/{}_ap{}_equity.csv", u_name, ap);
            std::fs::create_dir_all("snapshots/ap_hyperopt")?;
            let mut f = File::create(&csv_path)?;
            writeln!(f, "window,equity")?;
            for (w, res) in results.iter().enumerate() {
                writeln!(f, "{},{:.6}", w, res.equity)?;
            }
            
            // Per-window equity CSV for detailed analysis
            let detailed_path = format!("snapshots/ap_hyperopt/{}_ap{}_per_window.csv", u_name, ap);
            let mut g = File::create(&detailed_path)?;
            writeln!(g, "window,trade_equity,sharpe,dd,trades")?;
            for (w, res) in results.iter().enumerate() {
                writeln!(g, "{},{:.6},{:.4},{:.2},{}", w, res.equity, res.sharpe, res.dd, res.trades)?;
            }
        }
        
        // Base5 aggregate equity (compounded across windows)
        let base5_syms: Vec<String> = UNIVERSES[0].1.iter().map(|&s| s.to_string()).collect();
        let results = run_universe_ap(&sym_data, &base5_syms, ap, n_windows, min_len);
        
        let mut agg_equity = 1.0_f64;
        let agg_path = format!("snapshots/ap_hyperopt/base5_agg_ap{}.csv", ap);
        let mut f = File::create(&agg_path)?;
        writeln!(f, "window,agg_equity")?;
        for (w, res) in results.iter().enumerate() {
            agg_equity *= res.equity;
            writeln!(f, "{},{:.6}", w, agg_equity)?;
        }
        println!("  AP={}: Base5 aggregate = {:.6}x", ap, agg_equity);
    }

    // Export full sweep results
    let sweep_csv = "snapshots/ap_hyperopt_sweep.csv";
    let mut sf = File::create(sweep_csv)?;
    writeln!(sf, "ap,pass,total,pass_pct,sharpe,ret,dd,trades")?;
    for r in &sorted {
        writeln!(sf, "{},{},{},{:.4},{:.6},{:.4},{:.4},{}",
            r.ap, r.pass, r.total,
            r.pass as f64 / r.total as f64,
            r.sharpe, r.ret, r.dd, r.trades)?;
    }

    // Winner
    let winner = sorted.first().unwrap();
    println!("\n=== WINNER: AP={} ===", winner.ap);
    println!("  Pass: {}/{} ({:.1}%)", winner.pass, winner.total,
        (winner.pass as f64 / winner.total as f64) * 100.0);
    println!("  Sharpe: {:.3}", winner.sharpe);
    println!("  Return: {:.1}%", winner.ret);
    println!("  DD: {:.1}%", winner.dd);
    println!("  Trades: {}", winner.trades);
    
    // Compare to baseline
    let baseline = sorted.iter().find(|r| r.ap == AP_BASELINE).unwrap();
    println!("\n=== BASELINE (AP={}) ===", baseline.ap);
    println!("  Pass: {}/{} ({:.1}%)", baseline.pass, baseline.total,
        (baseline.pass as f64 / baseline.total as f64) * 100.0);
    println!("  Sharpe: {:.3}", baseline.sharpe);
    
    let pass_delta = winner.pass as isize - baseline.pass as isize;
    let sharpe_delta = winner.sharpe - baseline.sharpe;
    println!("\n=== DELTA vs Baseline ===");
    println!("  Pass: {:+} windows", pass_delta);
    println!("  Sharpe: {:+.3}", sharpe_delta);

    // Write summary markdown
    let md_path = "snapshots/ap_hyperopt.md";
    let mut mf = File::create(md_path)?;
    writeln!(mf, "# REGIME_ATR_PERIOD (AP) Hyperopt Results")?;
    writeln!(mf, "")?;
    writeln!(mf, "**Date:** 2026-05-04")?;
    writeln!(mf, "**Sweep:** AP ∈ [{}..={}] step {}, {} values", AP_MIN, AP_MAX, AP_STEP, n_aps)?;
    writeln!(mf, "**Universes:** {} × {} windows = {} OOS windows per AP", UNIVERSES.len(), n_windows, UNIVERSES.len() * n_windows)?;
    writeln!(mf, "**Strategy:** Turtle-only live path (EP=21, T=5, ATR_RANK=5, Turtle ATR exit)")?;
    writeln!(mf, "**Fee:** 0.10% taker (both sides)")?;
    writeln!(mf, "")?;
    writeln!(mf, "## Winner: AP={}", winner.ap)?;
    writeln!(mf, "")?;
    writeln!(mf, "| Metric | Winner (AP={}) | Baseline (AP={}) | Delta |", winner.ap, baseline.ap)?;
    writeln!(mf, "|--------|----------------|----------------|-------|")?;
    writeln!(mf, "| Pass Rate | {}/{} ({:.1}%) | {}/{} ({:.1}%) | {:+} |", 
        winner.pass, winner.total, (winner.pass as f64/winner.total as f64)*100.0,
        baseline.pass, baseline.total, (baseline.pass as f64/baseline.total as f64)*100.0,
        pass_delta)?;
    writeln!(mf, "| Avg Sharpe | {:.3} | {:.3} | {:+.3} |", winner.sharpe, baseline.sharpe, sharpe_delta)?;
    writeln!(mf, "| Avg Return | {:.1}% | {:.1}% | {:+.1}pp |", winner.ret, baseline.ret, winner.ret - baseline.ret)?;
    writeln!(mf, "| Avg DD | {:.1}% | {:.1}% | {:+.1}pp |", winner.dd, baseline.dd, winner.dd - baseline.dd)?;
    writeln!(mf, "| Trades | {} | {} | {:+} |", winner.trades, baseline.trades, winner.trades as isize - baseline.trades as isize)?;
    writeln!(mf, "")?;
    writeln!(mf, "## Top 10 AP Values")?;
    writeln!(mf, "| Rank | AP | Pass | Pass% | Sharpe | Ret% | DD% | Trades |")?;
    writeln!(mf, "|------|----|------|-------|--------|------|-----|--------|")?;
    for (i, r) in sorted.iter().take(10).enumerate() {
        writeln!(mf, "| {} | {} | {}/{} | {:.1}% | {:.3} | {:.1}% | {:.1}% | {} |",
            i+1, r.ap, r.pass, r.total,
            (r.pass as f64 / r.total as f64) * 100.0,
            r.sharpe, r.ret, r.dd, r.trades)?;
    }
    writeln!(mf, "")?;
    writeln!(mf, "## Equity Export")?;
    writeln!(mf, "")?;
    writeln!(mf, "Exported equity curves for AP values: {:?}", export_aps)?;
    writeln!(mf, "- `snapshots/ap_hyperopt/base5_agg_ap{{AP}}.csv` — Base5 aggregate equity per window")?;
    writeln!(mf, "- `snapshots/ap_hyperopt/{{UNIVERSE}}_ap{{AP}}_equity.csv` — per-universe equity per window")?;
    writeln!(mf, "- `snapshots/ap_hyperopt_sweep.csv` — full sweep results")?;
    writeln!(mf, "")?;
    writeln!(mf, "## Next Steps")?;
    if winner.ap != AP_BASELINE {
        writeln!(mf, "1. **Held-out validation** — test AP={} against pre-2021 held-out data", winner.ap)?;
        writeln!(mf, "2. **Update `live_compatible_wf.rs`** if held-out confirms AP={}", winner.ap)?;
        writeln!(mf, "3. **Update `config.rs`** if AP={} is promoted", winner.ap)?;
    } else {
        writeln!(mf, "1. AP={} is baseline — no improvement found.", AP_BASELINE)?;
        writeln!(mf, "2. Consider held-out validation of current default AP=12.")?;
    }
    println!("\nMarkdown summary: {}", md_path);

    Ok(())
}