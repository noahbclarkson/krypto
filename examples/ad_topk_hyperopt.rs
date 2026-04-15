//! A/D TOP_K Hyperparameter Optimization
//!
//! TARGET: Audit the hardcoded TOP_K=2 parameter in A/D Accumulation/Distribution.
//! TOP_K controls the ranking pool size: how many symbols are ranked by A/D momentum,
//! and the top-1 from that pool is entered.
//!
//! KEY INSIGHT: A/D is LONG-ONLY (shorts computed but never used).
//! TOP_K determines the candidate pool for the single long position:
//!   TOP_K=1: Only the single highest-momentum symbol is considered.
//!   TOP_K=2-6: The top-K symbols are ranked, top-1 enters.
//!   TOP_K=6 (full universe): All symbols ranked, top-1 enters.
//!
//! HYPOTHESIS: Smaller TOP_K = stricter selection = higher signal quality.
//! Larger TOP_K = more trades = more exposure but potentially lower quality.
//!
//! SWEEP: TOP_K ∈ {1, 2, 3, 4, 5, 6, 8, 10} (8 values)
//! AD_PERIOD: 47 (already validated winner)
//! HOLD_BARS: 54 (already validated winner)
//! UNIVERSES: 9 harsh universes
//! METHOD: Walk-forward 252 train / 252 test
//!
//! ALSO: exports equity curves for baseline + winners to CSV
//! for Python charting (comparison_chart.png).
//!
//! Usage:
//!   cargo run --release --example ad_topk_hyperopt 2>&1 | tee snapshots/ad_topk_hyperopt.log

use anyhow::Result;
use krypto::data::loader::DataLoader;
use polars::prelude::*;
use std::collections::HashMap;
use std::fs::File;
use std::io::Write;
use std::time::Instant;

// ── Constants ───────────────────────────────────────────────────────────────
const CANDLES: u32 = 3000;
const TRAIN_BARS: usize = 252;
const TEST_BARS: usize = 252;
const TAKER_FEE: f64 = 0.001;
const MIN_TRADES: usize = 3;
const AD_PERIOD: usize = 5; // hyperopt winner 2026-04-13 (was 47)
const HOLD_BARS: usize = 54;  // already validated winner

// ── The sweep values ─────────────────────────────────────────────────────────
const TOP_K_VALUES: &[usize] = &[1, 2, 3, 4, 5, 6, 8, 10];

// ── 9 Harsh Universes ────────────────────────────────────────────────────────
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

// ── Helpers ──────────────────────────────────────────────────────────────────
fn fmt_f64(v: f64, width: usize, decimals: usize) -> String {
    let s = format!("{:.width$}", v, width = decimals + if v < 0.0 { 1 } else { 0 });
    format!("{:>width$}", s, width = width.max(s.len()))
}
fn fmt_int<T: std::fmt::Display>(v: T, width: usize) -> String {
    format!("{:>width$}", v, width = width)
}

// ── Data structures ──────────────────────────────────────────────────────────
#[derive(Clone)]
struct WindowResult {
    wi: usize,
    ret: f64,
    sharpe: f64,
    max_dd: f64,
    trades: usize,
    passed: bool,
    equity_final: f64,
}

#[derive(Clone)]
struct UniverseResult {
    name: String,
    pass_rate: f64,
    avg_ret: f64,
    avg_sharpe: f64,
    window_results: Vec<WindowResult>,
}

#[derive(Clone)]
struct KConfigResult {
    top_k: usize,
    total_windows: usize,
    passed_windows: usize,
    avg_oos_ret: f64,
    avg_oos_sharpe: f64,
    worst_dd: f64,
    total_trades: usize,
    per_universe_results: Vec<UniverseResult>,
}

// ── A/D momentum extraction ───────────────────────────────────────────────────
struct AdSymData {
    open: Vec<f64>,
    close: Vec<f64>,
    high: Vec<f64>,
    low: Vec<f64>,
    volume: Vec<f64>,
    ad_momentum: Vec<f64>,
}

fn extract_ad_data(df: &DataFrame) -> Result<AdSymData> {
    let close: Vec<f64> = df
        .column("close")?
        .f64()?
        .into_iter()
        .map(|v| v.unwrap_or(0.0))
        .collect();
    let high: Vec<f64> = if df.column("high").is_ok() {
        df.column("high")?
            .f64()?
            .into_iter()
            .map(|v| v.unwrap_or(0.0))
            .collect()
    } else {
        close.clone()
    };
    let low: Vec<f64> = if df.column("low").is_ok() {
        df.column("low")?
            .f64()?
            .into_iter()
            .map(|v| v.unwrap_or(0.0))
            .collect()
    } else {
        close.clone()
    };
    let volume: Vec<f64> = if df.column("volume").is_ok() {
        df.column("volume")?
            .f64()?
            .into_iter()
            .map(|v| v.unwrap_or(0.0))
            .collect()
    } else {
        vec![1.0; close.len()]
    };
    let open: Vec<f64> = if df.column("open").is_ok() {
        df.column("open")?
            .f64()?
            .into_iter()
            .map(|v| v.unwrap_or(0.0))
            .collect()
    } else {
        close.clone()
    };

    let n = close.len();
    let mut ad = vec![0.0; n];
    for i in 1..n {
        let hl = high[i] - low[i];
        if hl > 0.0 {
            let mf = (close[i] - low[i]) - (high[i] - close[i]);
            ad[i] = ad[i - 1] + (mf / hl) * volume[i];
        } else {
            ad[i] = ad[i - 1];
        }
    }

    // A/D momentum: rate of change over AD_PERIOD bars
    let mut ad_momentum = vec![0.0; n];
    for i in AD_PERIOD..n {
        ad_momentum[i] = ad[i] - ad[i - AD_PERIOD];
    }

    Ok(AdSymData { open, close, high, low, volume, ad_momentum })
}

// ── Backtest for one config, one universe, one window ─────────────────────────
fn run_ad_backtest(
    data: &HashMap<String, AdSymData>,
    start: usize,
    end: usize,
    top_k: usize,
) -> Option<WindowResult> {
    if end <= start || end - start < HOLD_BARS + AD_PERIOD + 2 {
        return None;
    }

    let sym0 = data.keys().next()?;
    let n0 = data.get(sym0).unwrap().open.len();
    let eff_end = end.min(n0);

    let mut equity = 1.0_f64;
    let mut peak = equity;
    let mut max_dd = 0.0_f64;
    let mut trades = 0usize;
    let mut rets: Vec<f64> = Vec::new();
    let mut pos: Option<(String, usize, f64)> = None;

    let sym_list: Vec<String> = data.keys().cloned().collect();
    let mut bar = start;

    while bar + 1 < eff_end {
        if pos.is_none() {
            let mut candidates: Vec<(&String, f64)> = Vec::new();

            for sym in &sym_list {
                let sd = data.get(sym).unwrap();
                let idx = bar.saturating_sub(1);
                if idx < AD_PERIOD {
                    continue;
                }
                let mom = *sd.ad_momentum.get(idx).unwrap_or(&0.0);
                if mom > 0.0 {
                    candidates.push((sym, mom));
                }
            }

            candidates.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
            let top_candidates: Vec<String> = candidates
                .iter()
                .take(top_k.min(candidates.len()))
                .map(|(s, _)| (*s).clone())
                .collect();

            if !top_candidates.is_empty() {
                let sym = &top_candidates[0];
                let sd = data.get(sym).unwrap();
                let entry_price = *sd.open.get(bar).unwrap_or(&0.0);
                if entry_price > 0.0 {
                    pos = Some((sym.clone(), bar, entry_price));
                }
            }

            bar += 1;
            continue;
        }

        let (sym, entry_bar, entry_price) = pos.as_ref().unwrap();
        let sd = data.get(sym).unwrap();
        let current_bar = bar;

        if current_bar >= entry_bar + HOLD_BARS || current_bar >= eff_end - 1 {
            let exit_price = *sd
                .close
                .get(current_bar)
                .unwrap_or(&sd.close[sd.close.len() - 1]);
            if *entry_price > 0.0 && exit_price > 0.0 {
                let gross = (exit_price / entry_price - 1.0) - TAKER_FEE;
                equity *= 1.0 + gross;
                trades += 1;
                rets.push(gross);
            }
            pos = None;
        }

        peak = peak.max(equity);
        max_dd = max_dd.min(equity / peak - 1.0);

        bar += 1;
    }

    // Close open position
    if let Some((sym, entry_bar, entry_price)) = pos {
        let sd = data.get(&sym).unwrap();
        let exit_price = *sd
            .close
            .get((end - 1).min(sd.close.len() - 1))
            .unwrap_or(&sd.close[sd.close.len() - 1]);
        if entry_price > 0.0 && exit_price > 0.0 {
            let gross = (exit_price / entry_price - 1.0) - TAKER_FEE;
            equity *= 1.0 + gross;
            trades += 1;
            rets.push(gross);
        }
    }

    peak = peak.max(equity);
    max_dd = max_dd.min(equity / peak - 1.0);

    let ret = (equity - 1.0) * 100.0;
    let sh = if rets.is_empty() {
        0.0
    } else {
        let mean = rets.iter().sum::<f64>() / rets.len() as f64;
        let var = rets.iter().map(|r| (r - mean).powi(2)).sum::<f64>() / rets.len() as f64;
        let sd = var.sqrt();
        if sd == 0.0 {
            0.0
        } else {
            mean / sd * (252.0_f64.sqrt())
        }
    };

    Some(WindowResult {
        wi: 0,
        ret,
        sharpe: sh,
        max_dd: max_dd * 100.0,
        trades,
        passed: trades >= MIN_TRADES && ret > 0.0,
        equity_final: equity,
    })
}

// ── Run one TOP_K config across one universe ──────────────────────────────────
fn run_k_on_universe(
    syms: &[&str],
    cache: &HashMap<String, DataFrame>,
    top_k: usize,
) -> Result<UniverseResult> {
    let mut ad_data: HashMap<String, AdSymData> = HashMap::new();
    for s in syms {
        if let Some(df) = cache.get(*s) {
            if let Ok(ad) = extract_ad_data(df) {
                ad_data.insert(s.to_string(), ad);
            }
        }
    }

    let sym0 = ad_data.keys().next().unwrap();
    let n = ad_data.get(sym0).unwrap().open.len();
    let total_windows = n.saturating_sub(TRAIN_BARS + AD_PERIOD) / TEST_BARS;

    let mut window_results: Vec<WindowResult> = Vec::new();

    for wi in 0..total_windows {
        let train_end = TRAIN_BARS + wi * TEST_BARS;
        let tstart = train_end;
        let tend = (tstart + TEST_BARS).min(n);
        if tend.saturating_sub(tstart) < HOLD_BARS + AD_PERIOD + 2 {
            continue;
        }

        if let Some(mut wr) = run_ad_backtest(&ad_data, tstart, tend, top_k) {
            wr.wi = wi;
            window_results.push(wr);
        }
    }

    let valid: Vec<_> = window_results.iter().filter(|w| w.trades >= MIN_TRADES).collect();
    let avg_ret = if !valid.is_empty() {
        valid.iter().map(|w| w.ret).sum::<f64>() / valid.len() as f64
    } else { 0.0 };
    let avg_sharpe = if !valid.is_empty() {
        valid.iter().map(|w| w.sharpe).sum::<f64>() / valid.len() as f64
    } else { 0.0 };
    let pass_rate = if !window_results.is_empty() {
        window_results.iter().filter(|w| w.passed).count() as f64 / window_results.len() as f64 * 100.0
    } else { 0.0 };

    Ok(UniverseResult {
        name: syms[0].to_string(),
        pass_rate,
        avg_ret,
        avg_sharpe,
        window_results,
    })
}

// ── Main ─────────────────────────────────────────────────────────────────────
#[tokio::main]
async fn main() -> Result<()> {
    let start_time = Instant::now();
    println!("\n{}", "=".repeat(80));
    println!("  A/D TOP_K Hyperparameter Optimization");
    println!("  TOP_K sweep: {:?}", TOP_K_VALUES);
    println!("  AD period: {} | Hold: {} | 9 universes | WF 252/252", AD_PERIOD, HOLD_BARS);
    println!("{}", "=".repeat(80));

    let loader = DataLoader::new(None, None);
    let mut all_results: Vec<KConfigResult> = Vec::new();

    for &top_k in TOP_K_VALUES {
        println!("\n─── TOP_K = {} ───", top_k);
        let t0 = Instant::now();
        let mut per_universe: Vec<UniverseResult> = Vec::new();

        for (uname, syms) in UNIVERSES {
            let mut cache: HashMap<String, DataFrame> = HashMap::new();
            for &s in *syms {
                if s == "_" { continue; }
                match loader.fetch_with_cache(s, "1d", CANDLES).await {
                    Ok(df) => {
                        let trimmed = if df.height() > 2800 {
                            df.slice(0, 2800)
                        } else {
                            df
                        };
                        cache.insert(s.to_string(), trimmed);
                    }
                    Err(e) => eprintln!("  Warning: failed to load {}: {}", s, e),
                }
            }

            if cache.len() < 2 {
                eprintln!("  Skipping {} (insufficient data)", uname);
                continue;
            }

            let uni_start = Instant::now();
            match run_k_on_universe(syms, &cache, top_k) {
                Ok(mut ur) => {
                    ur.name = uname.to_string();
                    let passed = ur.window_results.iter().filter(|w| w.passed).count();
                    let total = ur.window_results.len();
                    println!(
                        "  {}: {}/{} passed | ret={:+.1} | sharpe={:.2} | {}s",
                        uname,
                        passed,
                        total,
                        ur.avg_ret,
                        ur.avg_sharpe,
                        uni_start.elapsed().as_secs_f64()
                    );
                    per_universe.push(ur);
                }
                Err(e) => eprintln!("  Error on {}: {}", uname, e),
            }
        }

        let total_windows: usize = per_universe.iter().map(|u| u.window_results.len()).sum();
        let passed_windows: usize = per_universe.iter().map(|u| u.window_results.iter().filter(|w| w.passed).count()).sum();
        let all_valid: Vec<_> = per_universe.iter().flat_map(|u| u.window_results.iter().filter(|w| w.trades >= MIN_TRADES)).collect();
        let avg_ret = if !all_valid.is_empty() {
            all_valid.iter().map(|w| w.ret).sum::<f64>() / all_valid.len() as f64
        } else { 0.0 };
        let avg_sharpe = if !all_valid.is_empty() {
            all_valid.iter().map(|w| w.sharpe).sum::<f64>() / all_valid.len() as f64
        } else { 0.0 };
        let worst_dd = all_valid.iter().map(|w| w.max_dd).fold(0.0_f64, |a, v| a.min(v));
        let total_trades: usize = all_valid.iter().map(|w| w.trades).sum();

        let pass_pct = if total_windows > 0 {
            passed_windows as f64 / total_windows as f64 * 100.0
        } else { 0.0 };

        println!(
            "  => K={}: {}/{} ({:.0}%) | avg_ret={:+.1} | avg_sharpe={:.3} | {} windows | {}s",
            top_k,
            passed_windows,
            total_windows,
            pass_pct,
            avg_ret,
            avg_sharpe,
            total_windows,
            t0.elapsed().as_secs_f64()
        );

        all_results.push(KConfigResult {
            top_k,
            total_windows,
            passed_windows,
            avg_oos_ret: avg_ret,
            avg_oos_sharpe: avg_sharpe,
            worst_dd,
            total_trades,
            per_universe_results: per_universe,
        });
    }

    // ── Summary table ─────────────────────────────────────────────────────────
    println!("\n{}", "=".repeat(80));
    println!("  GLOBAL SUMMARY — A/D TOP_K Sweep");
    println!("{}", "=".repeat(80));
    println!("{:>4} | {:>6} | {:>8} | {:>10} | {:>9} | {:>7}",
             "K", "Pass%", "AvgRet%", "AvgSharpe", "WorstDD%", "Trades");
    println!("{}", "-".repeat(80));

    let winner_k = all_results
        .iter()
        .max_by(|a, b| a.avg_oos_sharpe.partial_cmp(&b.avg_oos_sharpe).unwrap_or(std::cmp::Ordering::Equal))
        .map(|r| r.top_k)
        .unwrap_or(2);

    for r in &all_results {
        let pp = if r.total_windows > 0 {
            r.passed_windows as f64 / r.total_windows as f64 * 100.0
        } else { 0.0 };
        let marker = if r.top_k == winner_k { " ★" } else { "  " };
        println!(
            "{:>4}{} | {:>6.1} | {:>8.1} | {:>10.3} | {:>9.1} | {:>7}",
            r.top_k,
            marker,
            pp,
            r.avg_oos_ret,
            r.avg_oos_sharpe,
            r.worst_dd,
            r.total_trades
        );
    }

    // ── Export sweep CSV ────────────────────────────────────────────────────
    let csv_path = "snapshots/ad_topk_sweep.csv";
    {
        let mut f = File::create(csv_path)?;
        writeln!(f, "top_k,pass_pct,avg_ret_pct,avg_sharpe,worst_dd_pct,total_trades")?;
        for r in &all_results {
            let pp = if r.total_windows > 0 {
                r.passed_windows as f64 / r.total_windows as f64 * 100.0
            } else { 0.0 };
            writeln!(f, "{},{:.1},{:.1},{:.4},{:.1},{}",
                r.top_k, pp, r.avg_oos_ret, r.avg_oos_sharpe, r.worst_dd, r.total_trades)?;
        }
    }
    println!("\n  CSV: {}", csv_path);

    // ── Export equity curves (per universe, per window, per K) ───────────────
    let eq_csv_path = "snapshots/ad_topk_equity.csv";
    {
        let mut f = File::create(eq_csv_path)?;
        writeln!(f, "top_k,universe,window,equity_final")?;
        for r in &all_results {
            for ur in &r.per_universe_results {
                for wr in &ur.window_results {
                    writeln!(f, "{},{},{},{:.6}", r.top_k, ur.name, wr.wi, wr.equity_final)?;
                }
            }
        }
    }
    println!("  Equity CSV: {}", eq_csv_path);

    // ── Export per-universe per-window detail ────────────────────────────────
    let detail_csv = "snapshots/ad_topk_detail.csv";
    {
        let mut f = File::create(detail_csv)?;
        writeln!(f, "top_k,universe,window,ret_pct,sharpe,max_dd_pct,trades,passed")?;
        for r in &all_results {
            for ur in &r.per_universe_results {
                for wr in &ur.window_results {
                    writeln!(f, "{},{},{},{:.2},{:.3},{:.2},{},{}",
                        r.top_k, ur.name, wr.wi, wr.ret, wr.sharpe, wr.max_dd, wr.trades, wr.passed)?;
                }
            }
        }
    }
    println!("  Detail CSV: {}", detail_csv);

    // ── Per-universe breakdown ───────────────────────────────────────────────
    println!("\n{}", "-".repeat(80));
    println!("  PER-UNIVERSE — Avg Sharpe");
    print!("{:>16} |", "");
    for &k in TOP_K_VALUES {
        print!(" K={:>2} |", k);
    }
    println!();
    println!("{}", "-".repeat(80));
    for (uname, _syms) in UNIVERSES {
        let mut row = format!("{:>16} |", uname);
        for &k in TOP_K_VALUES {
            let sharpe = all_results
                .iter()
                .find(|r| r.top_k == k)
                .and_then(|r| r.per_universe_results.iter().find(|u| u.name == *uname))
                .map(|u| u.avg_sharpe)
                .unwrap_or(0.0);
            row.push_str(&format!(" {:>5.2} |", sharpe));
        }
        println!("{}", row);
    }

    let elapsed = start_time.elapsed();
    println!("\n  Total time: {:.1}s", elapsed.as_secs_f64());
    let winner_sharpe = all_results.iter().find(|r| r.top_k == winner_k).map(|r| r.avg_oos_sharpe).unwrap_or(0.0);
    println!("  Winner: K={} (Sharpe {:.3})", winner_k, winner_sharpe);

    // ── Save summary markdown ───────────────────────────────────────────────
    let winner = all_results.iter().find(|r| r.top_k == winner_k).cloned();
    if let Some(w) = winner {
        let wp = if w.total_windows > 0 {
            w.passed_windows as f64 / w.total_windows as f64 * 100.0
        } else { 0.0 };
        let mut table_rows = String::new();
        for r in &all_results {
            let pp = if r.total_windows > 0 {
                r.passed_windows as f64 / r.total_windows as f64 * 100.0
            } else { 0.0 };
            table_rows.push_str(&format!(
                "| {} | {:.1}% | {:.1}% | {:.3} | {:.1}% | {} |\n",
                r.top_k, pp, r.avg_oos_ret, r.avg_oos_sharpe, r.worst_dd, r.total_trades
            ));
        }
        let summary = format!(
            "# A/D TOP_K Hyperopt — 2026-04-12\n\n\
             Winner: **K={}** (Sharpe {:.3}, {:.1}% pass rate)\n\n\
             | K | Pass% | AvgRet% | AvgSharpe | WorstDD% | Trades |\n\
             |---|-------|---------|-----------|----------|--------|\n\
             {}\n\n\
             Methodology: 9-universe walk-forward 252/252, A/D period=47, hold=54 bars.\n\
             Top K is the ranking pool size: how many symbols ranked by A/D momentum before selecting top-1.\n",
            winner_k, w.avg_oos_sharpe, wp, table_rows
        );
        std::fs::write("snapshots/ad_topk_hyperopt_summary.md", summary)?;
    }

    Ok(())
}
