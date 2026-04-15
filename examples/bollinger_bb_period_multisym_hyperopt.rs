//! Hyperparameter Optimization: BollingerReversion BB_PERIOD (Multi-Symbol)
//!
//! TARGET: Bollinger Band Period (BB_PERIOD)
//! CONTEXT: Prior `bollinger_bb_period_hyperopt.rs` only tested on DOGE (an extreme outlier).
//!          This runs the FULL logical range (5-200 step 5 = 40 values) across
//!          ALL 5 FDUSD symbols and exports equity curves for charting.
//!
//! Exports:
//!   - snapshots/bollinger_bb_multisym_results.csv
//!   - snapshots/bollinger_bb_equity_p<PERIOD>.csv (top configs)
//!   - snapshots/bollinger_bb_multisym_meta.json

use anyhow::Result;
use krypto::{
    algo::{strategies::BollingerReversion, SignalGenerator},
    backtest::engine::Backtester,
    data::loader::DataLoader,
    features::indicators::FeatureEngine,
};
use polars::prelude::*;
use std::collections::HashMap;
use std::fs::File;
use std::io::Write;

const CANDLES: u32 = 2500;
const CAPITAL: f64 = 10_000.0;
const TAKER_FEE: f64 = 0.001;
const ATR_MULT: f64 = 0.30;
const QUICK_BARS: usize = 1000;
const TRAIN_BARS: usize = 252;
const TEST_BARS: usize = 252;
const MIN_TRADES: usize = 10;

const SYMBOLS: &[&str] = &["BTCFDUSD", "ETHFDUSD", "SOLFDUSD", "XRPFDUSD", "DOGEFDUSD"];
const BB_PERIODS: &[usize] = &[
    5, 10, 15, 20, 25, 30, 35, 40, 45, 50, 55, 60, 65, 70, 75, 80,
    85, 90, 95, 100, 105, 110, 115, 120, 125, 130, 135, 140, 145, 150,
    155, 160, 165, 170, 175, 180, 185, 190, 195, 200,
];
const BASELINE_PERIOD: usize = 30;

fn daily_returns(equity: &[f64]) -> Vec<f64> {
    if equity.len() < 2 { return vec![]; }
    equity.iter().zip(equity.iter().skip(1))
        .map(|(p, c)| if *p > 0.0 { (c - p) / p } else { 0.0 })
        .collect()
}

fn ann_sharpe(rets: &[f64]) -> f64 {
    if rets.len() < 2 { return 0.0; }
    let n = rets.len() as f64;
    let mn = rets.iter().sum::<f64>() / n;
    let sd = (rets.iter().map(|x| (x - mn).powi(2)).sum::<f64>() / n).sqrt();
    if sd == 0.0 { return 0.0; }
    mn * 365.0_f64.sqrt() / sd
}

fn max_dd(equity: &[f64]) -> f64 {
    let mut peak = f64::NEG_INFINITY;
    let mut dd = 0.0_f64;
    for &e in equity {
        if e > peak { peak = e; }
        let d = (peak - e) / peak;
        if d > dd { dd = d; }
    }
    dd * 100.0
}

fn run_bt(df: &DataFrame, bb_period: usize, stop_pct: f64) -> (f64, f64, f64, usize, f64, Vec<f64>) {
    let mut strat = BollingerReversion::new();
    strat.bb_period = bb_period;
    strat.bb_std = 2.5;
    strat.rsi_filter = 20.0;

    let signals = match strat.predict(df) {
        Ok(s) => s,
        Err(_) => return (0.0, 0.0, 0.0, 0, 0.0, vec![]),
    };

    let start = df.height().saturating_sub(QUICK_BARS);
    let df_q = df.slice(start as i64, QUICK_BARS);
    let sig_q = signals.slice(start as i64, QUICK_BARS);

    let bt = Backtester::new(CAPITAL, TAKER_FEE, 0.0);
    match bt.run(&df_q, &sig_q, stop_pct, 0.0) {
        Ok(r) => {
            let eq = r.equity_curve;
            let sh = ann_sharpe(&daily_returns(&eq));
            (r.total_return_pct, sh, max_dd(&eq), r.total_trades, r.win_rate, eq)
        }
        Err(_) => (0.0, 0.0, 0.0, 0, 0.0, vec![]),
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    println!("\n=== BollingerReversion BB_PERIOD Hyperopt ===");
    println!("  {} periods (5-200 step 5) x {} FDUSD symbols\n", BB_PERIODS.len(), SYMBOLS.len());

    std::fs::create_dir_all("snapshots")?;
    std::fs::create_dir_all("charts")?;

    let loader = DataLoader::new(None, None);

    // Load data
    let mut cache: HashMap<String, DataFrame> = HashMap::new();
    for sym in SYMBOLS {
        print!("  Loading {}... ", sym);
        let raw = loader.fetch_data(sym, "1d", CANDLES).await?;
        let df = FeatureEngine::add_technicals(&raw, None)?;
        println!("ok ({} bars)", df.height());
        cache.insert(sym.to_string(), df);
    }

    // Compute per-symbol stop_pct
    let mut stop_pcts: HashMap<String, f64> = HashMap::new();
    for sym in SYMBOLS {
        let df = cache.get(*sym).unwrap();
        let atr = df.column("atr").ok()
            .and_then(|s| s.f64().ok())
            .and_then(|ca| ca.get(ca.len().saturating_sub(1)))
            .unwrap_or(0.0);
        let close = df.column("close").ok()
            .and_then(|s| s.f64().ok())
            .and_then(|ca| ca.get(ca.len().saturating_sub(1)))
            .unwrap_or(1.0);
        let sp = if close > 0.0 { (atr * ATR_MULT / close).clamp(0.005, 0.30) } else { 0.015 };
        stop_pcts.insert(sym.to_string(), sp);
        println!("  {} stop: {:.2}%", sym, sp * 100.0);
    }

    // Phase 1: Quick sweep
    println!("\n--- Phase 1: Quick Sweep ---\n");

    let mut sym_agg: HashMap<usize, (f64, f64, f64, usize)> = HashMap::new();

    for &period in BB_PERIODS {
        for sym in SYMBOLS {
            let df = cache.get(*sym).unwrap();
            let sp = stop_pcts.get(*sym).unwrap();
            let (ret, sharpe, dd, trades, _wr, _eq) = run_bt(df, period, *sp);

            let e = sym_agg.entry(period).or_insert((0.0, 0.0, 0.0, 0));
            e.0 += ret;
            e.1 += sharpe;
            e.2 += dd;
            e.3 += trades;
        }
    }

    // Compute averages and sort by avg Sharpe
    let n_syms = SYMBOLS.len() as f64;
    let mut ranked: Vec<(usize, f64, f64, f64, usize)> = Vec::new();
    for &period in BB_PERIODS {
        if let Some(&(tr, ts, td, tt)) = sym_agg.get(&period) {
            ranked.push((period, tr / n_syms, ts / n_syms, td / n_syms, tt));
        }
    }
    ranked.sort_by(|a, b| b.2.partial_cmp(&a.2).unwrap_or(std::cmp::Ordering::Equal));

    // Print top 15
    println!("  RANK  PERIOD   AVG_RET%  AVG_SHARPE  AVG_DD%  TRADES");
    println!("  {}", "-".repeat(58));
    for (i, (period, avg_ret, avg_sh, avg_dd, trades)) in ranked.iter().take(15).enumerate() {
        let tag = if *period == BASELINE_PERIOD { " <BASE" }
                  else if i == 0 { " *WIN!" }
                  else { "" };
        println!("  {:>4}  {:>5}  {:>+9.1}  {:>+10.3}  {:>6.1}  {:>6}{}",
            i + 1, period, avg_ret, avg_sh, avg_dd, trades, tag);
    }

    let winner = ranked[0].0;
    let runner1 = ranked[1].0;
    let runner2 = ranked[2].0;
    let base_rank = ranked.iter().position(|r| r.0 == BASELINE_PERIOD).map(|p| p + 1).unwrap_or(0);
    let base_sh = ranked.iter().find(|r| r.0 == BASELINE_PERIOD).map(|r| r.2).unwrap_or(0.0);

    println!("\n  * WINNER:    P={} Sharpe={:.3}", winner, ranked[0].2);
    println!("    Runner 1:  P={} Sharpe={:.3}", runner1, ranked[1].2);
    println!("    Runner 2:  P={} Sharpe={:.3}", runner2, ranked[2].2);
    println!("    Baseline:  P={} rank=#{} Sharpe={:.3}", BASELINE_PERIOD, base_rank, base_sh);

    // Phase 2: Walk-forward validation for top-3 + baseline
    println!("\n--- Phase 2: Walk-Forward Validation ---\n");

    let wf_configs = [winner, runner1, runner2, BASELINE_PERIOD];
    let mut wf_summary: Vec<(usize, usize, f64, f64, usize)> = Vec::new();

    for &period in &wf_configs {
        let mut n_pass = 0usize;
        let mut tot_ret = 0.0f64;
        let mut tot_sh = 0.0f64;
        let mut tot_trades = 0usize;

        for sym in SYMBOLS {
            let df_full = cache.get(*sym).unwrap();
            let n = df_full.height();
            let n_wf = n.saturating_sub(TRAIN_BARS + TEST_BARS) / TEST_BARS;
            let sp = stop_pcts.get(*sym).unwrap();

            let mut sym_ret = 0.0f64;
            let mut sym_sh = 0.0f64;
            let mut sym_trades = 0usize;

            for wi in 0..n_wf {
                let t_start = TRAIN_BARS + wi * TEST_BARS;
                let t_end = (t_start + TEST_BARS).min(n);
                if t_end <= t_start { continue; }

                let df_test = df_full.slice(t_start as i64, t_end - t_start);

                let mut strat = BollingerReversion::new();
                strat.bb_period = period;
                strat.bb_std = 2.5;
                strat.rsi_filter = 20.0;

                let signals = match strat.predict(&df_test) { Ok(s) => s, Err(_) => continue };

                let bt = Backtester::new(CAPITAL, TAKER_FEE, 0.0);
                if let Ok(r) = bt.run(&df_test, &signals, *sp, 0.0) {
                    if r.total_trades >= MIN_TRADES {
                        sym_ret += r.total_return_pct;
                        sym_sh += ann_sharpe(&daily_returns(&r.equity_curve));
                        sym_trades += r.total_trades;
                    }
                }
            }

            if n_wf > 0 {
                let avg = sym_ret / n_wf as f64;
                let avg_s = sym_sh / n_wf as f64;
                tot_ret += avg;
                tot_sh += avg_s;
                tot_trades += sym_trades;
                if avg > 0.0 && avg_s > 0.5 { n_pass += 1; }
            }
        }

        wf_summary.push((period, n_pass, tot_ret / SYMBOLS.len() as f64, tot_sh / SYMBOLS.len() as f64, tot_trades));
    }

    println!("  PERIOD  PASS  AVG_RET%  AVG_SHARPE  TRADES");
    println!("  {}", "-".repeat(48));
    for (period, n_pass, avg_ret, avg_sh, trades) in &wf_summary {
        let tag = if *period == winner { " *WIN" }
                  else if *period == BASELINE_PERIOD { " BASE" }
                  else { "" };
        println!("  {:>5}  {}/{}  {:>+8.1}  {:>+10.3}  {:>6}{}",
            period, n_pass, SYMBOLS.len(), avg_ret, avg_sh, trades, tag);
    }

    // Export equity curves for chart configs
    println!("\n--- Exporting equity curves ---\n");

    for &period in &wf_configs {
        let fname = format!("snapshots/bollinger_bb_equity_p{}.csv", period);
        let mut f = File::create(&fname)?;
        writeln!(f, "symbol,day,equity")?;
        for sym in SYMBOLS {
            let df = cache.get(*sym).unwrap();
            let sp = stop_pcts.get(*sym).unwrap();
            let (_, _, _, _, _, equity) = run_bt(df, period, *sp);
            for (di, &eq) in equity.iter().enumerate() {
                writeln!(f, "{},{},{:.4}", sym, di, eq)?;
            }
        }
        println!("  -> {}", fname);
    }

    // Export results CSV
    let mut csv = File::create("snapshots/bollinger_bb_multisym_results.csv")?;
    writeln!(csv, "period,avg_ret,avg_sharpe,avg_dd,total_trades,rank")?;
    for (rank, row) in ranked.iter().enumerate() {
        writeln!(csv, "{},{:.4},{:.4},{:.4},{},{}", row.0, row.1, row.2, row.3, row.4, rank + 1)?;
    }
    println!("  -> snapshots/bollinger_bb_multisym_results.csv");

    // Export meta
    let meta = format!(
        "{{\"winner\":{},\"runner1\":{},\"runner2\":{},\"baseline\":{},\"n_periods\":{},\"n_symbols\":{}}}",
        winner, runner1, runner2, BASELINE_PERIOD, BB_PERIODS.len(), SYMBOLS.len()
    );
    std::fs::write("snapshots/bollinger_bb_multisym_meta.json", meta)?;

    println!("\n=== DONE ===");
    println!("  Chart: python3 charts/plot_bb_period_multisym.py\n");
    Ok(())
}
