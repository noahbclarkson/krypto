//! =========================================================
//! HYPERPARAMETER OPTIMIZATION: MACD Signal EMA Period
//! =========================================================
//!
//! TARGET: MACD signal EMA period (currently hardcoded at 9 — Gerald Appel's 1980s default)
//!         The fast/slow periods were optimized in prior sessions (14/30), but signal was fixed.
//! SWEEP:  5 to 30 in steps of 1 → 26 values (entire logical range for daily MACD)
//! UNIVERSES: All 9 harsh universes
//! METHOD:    Walk-forward 252/252 + 15 CPCV resamples + equity curve export
//! METRIC:    Chronology-first (quarter passes > resample passes > Sharpe > return)
//!
//! Baseline: (fast=14, slow=30, signal=9)
//! Rule: pick the most ROBUST across windows and universes, not the best single-window return.

use anyhow::Result;
use krypto::data::loader::DataLoader;
use krypto::features::indicators::FeatureEngine;
use polars::prelude::*;
use std::collections::HashMap;
use std::fs::File;
use std::io::Write;

const CANDLES: u32 = 3000;
const TRAIN_BARS: usize = 252;
const TEST_BARS: usize = 252;
const HOLD_BARS: usize = 21;
const TAKER_FEE: f64 = 0.001;
const MIN_TRADES: usize = 3;
const TOP_K: usize = 3;
const CPCV_RESAMPLES: usize = 15;

// Validated: fast=12 (Gerald Appel classic), slow=65 (hyperopt-2026-04-14, commit fb9b426)
const FAST_PERIOD: usize = 12;
const SLOW_PERIOD: usize = 65;
const SMA200_PERIOD: usize = 200;

// Signal periods: 5 to 30 in steps of 1 (26 values)
const SIGNAL_START: usize = 5;
const SIGNAL_END: usize = 30;

// Baseline (9) for comparison
const SIGNAL_BASELINE: usize = 9;

// 9 Harsh universes
const S_BASE5: [&str; 6] = [
    "BTCUSDT", "ETHUSDT", "SOLUSDT", "XRPUSDT", "DOGEUSDT", "ADAUSDT",
];
const S_NODOGE: [&str; 6] = [
    "BTCUSDT", "ETHUSDT", "SOLUSDT", "XRPUSDT", "ADAUSDT", "BNBUSDT",
];
const S_L4: [&str; 6] = ["BTCUSDT", "ETHUSDT", "XRPUSDT", "LTCUSDT", "_", "_"];
const S_L5BNB: [&str; 6] = ["BTCUSDT", "ETHUSDT", "XRPUSDT", "LTCUSDT", "BNBUSDT", "_"];
const S_OGNM: [&str; 6] = ["BTCUSDT", "ETHUSDT", "XRPUSDT", "LTCUSDT", "EOSUSDT", "_"];
const S_LCAPS: [&str; 6] = ["BTCUSDT", "ETHUSDT", "BNBUSDT", "XRPUSDT", "ADAUSDT", "_"];
const S_L3: [&str; 3] = ["BTCUSDT", "ETHUSDT", "XRPUSDT"];
const S_LVOL: [&str; 5] = ["XRPUSDT", "LTCUSDT", "EOSUSDT", "BCHUSDT", "ADAUSDT"];
const S_OG4: [&str; 4] = ["XRPUSDT", "LTCUSDT", "EOSUSDT", "BCHUSDT"];

const UNIVERSES: &[(&str, &[&str])] = &[
    ("Base5", &S_BASE5),
    ("NoDOGE", &S_NODOGE),
    ("Legacy4", &S_L4),
    ("Legacy5BNB", &S_L5BNB),
    ("OldGuardNoBNB", &S_OGNM),
    ("LargeCaps5", &S_LCAPS),
    ("Legacy3", &S_L3),
    ("LowVolume5", &S_LVOL),
    ("OldGuard4", &S_OG4),
];

struct SymbolData {
    close: Vec<f64>,
    open: Vec<f64>,
    macd_line: Vec<f64>,
    macd_signal: Vec<f64>,
    sma200: Vec<f64>,
    macd_sigs: Vec<i32>,
}

#[derive(Clone, Debug)]
struct PeriodResult {
    signal_period: usize,
    quarter_passes: usize,
    windows: usize,
    resample_passes: usize,
    avg_ret: f64,
    avg_sharpe: f64,
    avg_dd: f64,
    total_trades: usize,
    wins: usize,
    equity_curve: Vec<f64>,
}

struct UniverseResult {
    name: String,
    period_results: Vec<PeriodResult>,
    best_period: usize,
    baseline_period: usize,
}

#[tokio::main]
async fn main() -> Result<()> {
    let t0 = std::time::Instant::now();

    println!("\n========================================================");
    println!("  HYPEROPT: MACD Signal Period (5–30, step 1)");
    println!(
        "  Fast={}, Slow={} (fixed from prior hyperopt)",
        FAST_PERIOD, SLOW_PERIOD
    );
    println!("  Baseline signal=9");
    println!("========================================================\n");

    // Load data for all symbols
    let loader = DataLoader::new(None, None);
    let mut all_sym_set: std::collections::HashSet<&str> = std::collections::HashSet::new();
    for (_, syms) in UNIVERSES {
        for &s in *syms {
            if s != "_" {
                all_sym_set.insert(s);
            }
        }
    }
    let all_symbols: Vec<&str> = all_sym_set.into_iter().collect();

    let mut cache: HashMap<String, DataFrame> = HashMap::new();
    let mut min_len = usize::MAX;
    for s in &all_symbols {
        match loader.fetch_with_cache(s, "1d", CANDLES).await {
            Ok(raw) => match FeatureEngine::add_technicals(&raw, None) {
                Ok(df) => {
                    let n = df.height();
                    min_len = min_len.min(n);
                    cache.insert(s.to_string(), df);
                }
                Err(_) => {}
            },
            Err(_) => {}
        }
    }

    let n = min_len.saturating_sub(42).min(2900);
    for (_, df) in &mut cache {
        if df.height() > n {
            *df = df.slice(0, n);
        }
    }

    let windows = (n.saturating_sub(TRAIN_BARS + 42)) / TEST_BARS;
    let n_signals = SIGNAL_END - SIGNAL_START + 1;
    println!(
        "  Data: {} bars, {} test windows, {} signal values",
        n, windows, n_signals
    );

    let mut all_results: Vec<UniverseResult> = Vec::new();
    let mut global_best_period = SIGNAL_BASELINE;
    let mut global_best_score = f64::NEG_INFINITY;

    for (uni_name, syms) in UNIVERSES {
        println!("\n─── Universe: {uni_name} ───");

        let valid_syms: Vec<String> = syms
            .iter()
            .filter(|&&s| s != "_" && cache.contains_key(s))
            .map(|s| s.to_string())
            .collect();
        if valid_syms.len() < 2 {
            println!("  [SKIP - not enough data]");
            continue;
        }

        let mut period_results: Vec<PeriodResult> = Vec::with_capacity(n_signals);

        for sig_p in SIGNAL_START..=SIGNAL_END {
            let result = run_signal_period(&cache, &valid_syms, n, windows, sig_p).await?;
            period_results.push(result);
        }

        let best = period_results
            .iter()
            .max_by(|a, b| score(a).partial_cmp(&score(b)).unwrap())
            .cloned()
            .unwrap();
        let best_period = best.signal_period;

        let baseline = period_results
            .iter()
            .find(|r| r.signal_period == SIGNAL_BASELINE)
            .cloned()
            .unwrap_or_else(|| period_results[0].clone());
        let baseline_period = baseline.signal_period;

        let s = score(&best);
        if s > global_best_score {
            global_best_score = s;
            global_best_period = best_period;
        }

        println!(
            "  Best signal={:2}  Q-passes={:2}/{:2}  Res={:3}/{:3}  Sharpe={:6.2}  Ret={:8.1}%",
            best.signal_period,
            best.quarter_passes,
            best.windows,
            best.resample_passes,
            CPCV_RESAMPLES * best.windows,
            best.avg_sharpe,
            best.avg_ret
        );
        println!(
            "  Base  signal={:2}  Q-passes={:2}/{:2}  Res={:3}/{:3}  Sharpe={:6.2}  Ret={:8.1}%",
            baseline.signal_period,
            baseline.quarter_passes,
            baseline.windows,
            baseline.resample_passes,
            CPCV_RESAMPLES * baseline.windows,
            baseline.avg_sharpe,
            baseline.avg_ret
        );

        all_results.push(UniverseResult {
            name: uni_name.to_string(),
            period_results,
            best_period,
            baseline_period,
        });
    }

    // Print aggregate summary
    println!("\n\n═══════════════════════════════════════════════════════");
    println!("  GLOBAL RESULTS: MACD Signal Period (5–30)");
    println!("═══════════════════════════════════════════════════════");

    let mut period_wins: HashMap<usize, usize> = HashMap::new();
    let mut period_quarter_passes: HashMap<usize, (usize, usize)> = HashMap::new();

    for u in &all_results {
        *period_wins.entry(u.best_period).or_insert(0) += 1;
        let best = u
            .period_results
            .iter()
            .find(|r| r.signal_period == u.best_period)
            .unwrap();
        let base = u
            .period_results
            .iter()
            .find(|r| r.signal_period == SIGNAL_BASELINE)
            .unwrap();
        let e1 = period_quarter_passes.entry(u.best_period).or_insert((0, 0));
        e1.0 += best.quarter_passes;
        e1.1 += best.windows;
        let e2 = period_quarter_passes
            .entry(SIGNAL_BASELINE)
            .or_insert((0, 0));
        e2.0 += base.quarter_passes;
        e2.1 += base.windows;
    }

    println!("\n  Period | Univ Wins | Avg Q-Pass | Avg Sharpe");
    let mut rows: Vec<(usize, usize, f64, f64)> = Vec::new();
    for sig_p in SIGNAL_START..=SIGNAL_END {
        let uw = *period_wins.get(&sig_p).unwrap_or(&0);
        let (tq, tw) = *period_quarter_passes.get(&sig_p).unwrap_or(&(0, 0));
        let avg_q = tq as f64 / tw.max(1) as f64;
        let all_pr: Vec<_> = all_results
            .iter()
            .flat_map(|u| u.period_results.iter())
            .filter(|r| r.signal_period == sig_p)
            .collect();
        let avg_sharpe = if all_pr.is_empty() {
            0.0
        } else {
            all_pr.iter().map(|r| r.avg_sharpe).sum::<f64>() / all_pr.len() as f64
        };
        rows.push((sig_p, uw, avg_q, avg_sharpe));
    }
    rows.sort_by(|a, b| {
        let sa = a.2 * 10.0 + a.3 * 0.5;
        let sb = b.2 * 10.0 + b.3 * 0.5;
        sb.partial_cmp(&sa).unwrap()
    });
    for (p, uw, aq, sh) in &rows {
        let marker = if *p == SIGNAL_BASELINE {
            " ← BASELINE"
        } else if *p == global_best_period {
            " ← BEST"
        } else {
            ""
        };
        println!(
            "  {:5}  |     {:2}      |    {:5.2}    | {:6.2}{}",
            p, uw, aq, sh, marker
        );
    }

    export_equity_curves(&all_results)?;

    let elapsed = t0.elapsed();
    println!("\n  Total time: {:.1}s", elapsed.as_secs_f64());

    save_snapshots(&all_results, global_best_period, elapsed)?;

    Ok(())
}

fn score(r: &PeriodResult) -> f64 {
    let q_ratio = r.quarter_passes as f64 / r.windows.max(1) as f64;
    let r_ratio = r.resample_passes as f64 / (CPCV_RESAMPLES * r.windows).max(1) as f64;
    q_ratio * 10.0 + r_ratio * 2.0 + r.avg_sharpe * 0.5 + (r.avg_ret / 1000.0).min(5.0)
}

async fn run_signal_period(
    cache: &HashMap<String, DataFrame>,
    symbols: &[String],
    n: usize,
    n_windows: usize,
    signal_period: usize,
) -> Result<PeriodResult> {
    let mut quarter_passes = 0usize;
    let mut total_ret = 0.0f64;
    let mut total_sharpe = 0.0f64;
    let mut total_dd = 0.0f64;
    let mut total_trades = 0usize;
    let mut total_wins = 0usize;
    let mut resample_passes = 0usize;
    let mut window_equities: Vec<f64> = vec![1.0; n_windows];

    // Pre-compute all symbol data for this signal period
    let mut symbol_data: HashMap<String, SymbolData> = HashMap::new();
    for sym in symbols {
        if let Some(df) = cache.get(sym) {
            if let Ok(sd) = compute_symbol_data(df, signal_period) {
                symbol_data.insert(sym.clone(), sd);
            }
        }
    }

    for wi in 0..n_windows {
        let train_end = TRAIN_BARS + wi * TEST_BARS;
        let test_start = train_end;
        let test_end = (train_end + TEST_BARS).min(n - HOLD_BARS - 1);
        if test_end <= test_start + MIN_TRADES * 3 {
            continue;
        }

        let (pret, psharpe, pdd, ptrades, pwins, _) =
            run_portfolio_window(&symbol_data, symbols, test_start, test_end)?;

        total_ret += pret;
        total_sharpe += psharpe;
        total_dd += pdd.abs();
        total_trades += ptrades;
        total_wins += pwins;
        window_equities[wi] = (1.0 + pret / 100.0).max(-0.5);
        if pret > 0.0 {
            quarter_passes += 1;
        }

        // CPCV resamples
        for ri in 0..CPCV_RESAMPLES {
            let offset = ri * (TEST_BARS / CPCV_RESAMPLES);
            let rs = test_start.saturating_add(offset);
            let re = (rs + TEST_BARS).min(n - HOLD_BARS - 1);
            if re <= rs + MIN_TRADES * 3 {
                continue;
            }
            let (rret, _, _, _, _, _) = run_portfolio_window(&symbol_data, symbols, rs, re)?;
            if rret > 0.0 {
                resample_passes += 1;
            }
        }
    }

    let n_valid = quarter_passes.max(1);
    Ok(PeriodResult {
        signal_period,
        quarter_passes,
        windows: n_windows,
        resample_passes,
        avg_ret: total_ret / n_valid as f64,
        avg_sharpe: total_sharpe / n_valid as f64,
        avg_dd: total_dd / n_valid as f64,
        total_trades,
        wins: total_wins,
        equity_curve: window_equities,
    })
}

fn compute_symbol_data(df: &DataFrame, signal_period: usize) -> Result<SymbolData> {
    let n = df.height();
    let close_ch = df.column("close")?.f64()?;
    let open_ch = df.column("open")?.f64()?;

    let close: Vec<f64> = (0..n).map(|i| close_ch.get(i).unwrap_or(0.0)).collect();
    let open: Vec<f64> = (0..n).map(|i| open_ch.get(i).unwrap_or(0.0)).collect();

    let alpha_fast = 2.0 / (FAST_PERIOD as f64 + 1.0);
    let alpha_slow = 2.0 / (SLOW_PERIOD as f64 + 1.0);
    let alpha_sig = 2.0 / (signal_period as f64 + 1.0);

    let mut ema_fast = vec![0.0; n];
    let mut ema_slow = vec![0.0; n];
    if n > 0 {
        ema_fast[0] = close[0];
        ema_slow[0] = close[0];
    }
    for i in 1..n {
        ema_fast[i] = close[i] * alpha_fast + ema_fast[i - 1] * (1.0 - alpha_fast);
        ema_slow[i] = close[i] * alpha_slow + ema_slow[i - 1] * (1.0 - alpha_slow);
    }

    let mut macd_line = vec![0.0; n];
    for i in 0..n {
        macd_line[i] = ema_fast[i] - ema_slow[i];
    }

    let mut macd_signal = vec![0.0; n];
    if n > 0 {
        macd_signal[0] = macd_line[0];
    }
    for i in 1..n {
        macd_signal[i] = macd_line[i] * alpha_sig + macd_signal[i - 1] * (1.0 - alpha_sig);
    }

    // SMA200 for regime
    let mut sma200 = vec![0.0; n];
    for i in SMA200_PERIOD..n {
        let mut sum = 0.0_f64;
        for j in (i - SMA200_PERIOD)..i {
            sum += close[j];
        }
        sma200[i] = sum / SMA200_PERIOD as f64;
    }

    // MACD+Regime signals
    let warmup = SLOW_PERIOD.max(signal_period).max(SMA200_PERIOD);
    let mut sigs = vec![0i32; n];
    for i in warmup..n {
        let price = close[i];
        let m = macd_line[i];
        let ms = macd_signal[i];
        let s = sma200[i];
        if s <= 0.0 {
            continue;
        }
        if m > ms && price > s {
            sigs[i] = 1;
        } else if m < ms && price < s {
            sigs[i] = -1;
        }
    }

    Ok(SymbolData {
        close,
        open,
        macd_line,
        macd_signal,
        sma200,
        macd_sigs: sigs,
    })
}

fn run_portfolio_window(
    symbol_data: &HashMap<String, SymbolData>,
    symbols: &[String],
    test_start: usize,
    test_end: usize,
) -> Result<(f64, f64, f64, usize, usize, Vec<f64>)> {
    let mut portfolio_value = 1.0f64;
    let mut equity_curve = vec![1.0f64];
    let mut total_trades = 0usize;
    let mut total_wins = 0usize;
    let mut peak = 1.0f64;
    let mut max_dd = 0.0f64;
    let mut daily_rets: Vec<f64> = Vec::new();

    let mut i = test_start;
    while i + HOLD_BARS + 1 < test_end {
        // Collect signals at bar i
        let mut candidates: Vec<(&str, f64)> = Vec::new();
        for sym in symbols {
            if let Some(sd) = symbol_data.get(sym) {
                if i < sd.macd_sigs.len() && i < sd.close.len() {
                    let sig = sd.macd_sigs[i];
                    let price = sd.close[i];
                    if sig != 0 && price > 0.0 {
                        // Use hist as strength proxy
                        let str_val = (sd.macd_line[i] - sd.macd_signal[i]).abs();
                        candidates.push((sym.as_str(), str_val));
                    }
                }
            }
        }

        if candidates.is_empty() {
            i += 1;
            continue;
        }

        // Sort by signal strength - take top K
        candidates.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());
        let selected: Vec<_> = candidates.into_iter().take(TOP_K).collect();

        // Execute trades
        for (sym, _) in &selected {
            if let Some(sd) = symbol_data.get(*sym) {
                let entry = *sd.open.get(i + 1).unwrap_or(&sd.close[i]);
                let mut exit_price = None;
                for j in (i + 1)..(i + HOLD_BARS + 1).min(sd.open.len()) {
                    if let Some(&e) = sd.close.get(j) {
                        exit_price = Some(e);
                        break;
                    }
                }
                let exit = exit_price.unwrap_or(entry);
                if entry <= 0.0 {
                    continue;
                }

                let gross = (exit - entry) / entry;
                let net = gross - 2.0 * TAKER_FEE;
                let ret = net * portfolio_value / TOP_K as f64;
                portfolio_value *= 1.0 + ret;
                total_trades += 1;
                if gross > 0.0 {
                    total_wins += 1;
                }

                peak = peak.max(portfolio_value);
                max_dd = max_dd.max((peak - portfolio_value) / peak);
            }
        }

        let prev_equity = if equity_curve.len() >= 2 {
            equity_curve[equity_curve.len() - 2]
        } else {
            1.0
        };
        if prev_equity > 0.0 {
            daily_rets.push((portfolio_value - prev_equity) / prev_equity);
        }
        equity_curve.push(portfolio_value);
        i += 1;
    }

    let n_rets = daily_rets.len().max(1) as f64;
    let mean_ret = daily_rets.iter().sum::<f64>() / n_rets;
    let std_ret = (daily_rets
        .iter()
        .map(|r| (r - mean_ret).powi(2))
        .sum::<f64>()
        / n_rets)
        .sqrt();
    let sharpe = if std_ret > 0.0 {
        (mean_ret / std_ret) * (252.0_f64.sqrt())
    } else {
        0.0
    };
    let total_return = (portfolio_value - 1.0) * 100.0;

    Ok((
        total_return,
        sharpe,
        -max_dd * 100.0,
        total_trades,
        total_wins,
        equity_curve,
    ))
}

fn export_equity_curves(all_results: &[UniverseResult]) -> Result<()> {
    let csv_path = "snapshots/macd_signal_hyperopt_equity.csv";
    let mut f = File::create(csv_path)?;
    writeln!(f, "universe,signal_period,window,equity")?;
    for u in all_results {
        for pr in &u.period_results {
            for (wi, eq) in pr.equity_curve.iter().enumerate() {
                writeln!(f, "{},{},{},{}", u.name, pr.signal_period, wi, eq)?;
            }
        }
    }

    let wide_path = "snapshots/macd_signal_hyperopt_wide_equity.csv";
    let mut wf = File::create(wide_path)?;
    writeln!(
        wf,
        "universe,signal_period,total_return,avg_sharpe,avg_quarter_passes,worst_dd,total_trades"
    )?;
    for u in all_results {
        let best = u
            .period_results
            .iter()
            .find(|r| r.signal_period == u.best_period)
            .unwrap();
        let baseline = u
            .period_results
            .iter()
            .find(|r| r.signal_period == SIGNAL_BASELINE)
            .unwrap();
        writeln!(
            wf,
            "{},{},{:.2},{:.2},{:.2},{:.2},{}",
            u.name,
            u.best_period,
            best.avg_ret,
            best.avg_sharpe,
            best.quarter_passes as f64 / best.windows.max(1) as f64,
            best.avg_dd,
            best.total_trades
        )?;
        writeln!(
            wf,
            "{},{},{:.2},{:.2},{:.2},{:.2},{}",
            u.name,
            SIGNAL_BASELINE,
            baseline.avg_ret,
            baseline.avg_sharpe,
            baseline.quarter_passes as f64 / baseline.windows.max(1) as f64,
            baseline.avg_dd,
            baseline.total_trades
        )?;
    }

    println!("\n  Equity curves → {}", csv_path);
    println!("  Wide equity    → {}", wide_path);
    Ok(())
}

fn save_snapshots(
    all_results: &[UniverseResult],
    global_best: usize,
    elapsed: std::time::Duration,
) -> Result<()> {
    let ts = chrono_lite_timestamp();
    let md_path = format!("snapshots/macd_signal_hyperopt_{}.md", ts);
    let csv_path = format!("snapshots/macd_signal_hyperopt_{}.csv", ts);
    let latest_md = "snapshots/macd_signal_hyperopt_latest.md";
    let latest_csv = "snapshots/macd_signal_hyperopt_latest.csv";

    let mut csv_f = File::create(&csv_path)?;
    writeln!(csv_f, "universe,signal_period,quarter_passes,windows,resample_passes,avg_ret,avg_sharpe,avg_dd,total_trades,wins")?;
    for u in all_results {
        for pr in &u.period_results {
            writeln!(
                csv_f,
                "{},{},{},{},{},{:.2},{:.2},{:.2},{},{}",
                u.name,
                pr.signal_period,
                pr.quarter_passes,
                pr.windows,
                pr.resample_passes,
                pr.avg_ret,
                pr.avg_sharpe,
                pr.avg_dd,
                pr.total_trades,
                pr.wins
            )?;
        }
    }
    std::fs::copy(&csv_path, latest_csv)?;

    let mut md_f = File::create(&md_path)?;
    writeln!(md_f, "# MACD Signal Period Hyperopt — {}", ts)?;
    writeln!(
        md_f,
        "\nFast={}, Slow={} (fixed). Sweeping signal {}-{}",
        FAST_PERIOD, SLOW_PERIOD, SIGNAL_START, SIGNAL_END
    )?;
    writeln!(md_f, "\nGlobal Best: **signal={}**", global_best)?;
    writeln!(md_f, "Baseline: signal={}", SIGNAL_BASELINE)?;
    writeln!(md_f, "\nTime: {:.1}s", elapsed.as_secs_f64())?;
    writeln!(md_f, "\n## Per-Universe Winners")?;
    writeln!(
        md_f,
        "\n| Universe | Best Signal | Avg Ret | Avg Sharpe | Q-Passes |"
    )?;
    writeln!(
        md_f,
        "|----------|-------------|---------|-----------|----------|"
    )?;
    for u in all_results {
        let best = u
            .period_results
            .iter()
            .find(|r| r.signal_period == u.best_period)
            .unwrap();
        writeln!(
            md_f,
            "| {} | **{}** | {:6.1}% | {:6.2} | {}/{} |",
            u.name,
            best.signal_period,
            best.avg_ret,
            best.avg_sharpe,
            best.quarter_passes,
            best.windows
        )?;
    }

    std::fs::copy(&md_path, latest_md)?;
    println!("\n  Snapshots → {}", md_path);
    Ok(())
}

fn chrono_lite_timestamp() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs();
    let h = (secs % 86400) / 3600;
    let m = (secs % 3600) / 60;
    let s = secs % 60;
    format!("{:04}{:02}{:02}T{:02}{:02}{:02}", 2026, 4, 8, h, m, s)
}
