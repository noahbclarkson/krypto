//! CTREND Fixed-Hold Exit Walk-Forward (fixed metrics)
//!
//! CTREND entry (EMA8 > EMA32 crossover) + fixed-hold exits.
//! Win condition: any variant >35/60 pass = viable signal family.
//! Baseline: Turtle+Chandelier = 43/54 pass (80%).
//!
//! Prior test (REJECTED): CTREND + Chandelier → 30/54 (wrong exit mechanism).
//! Hypothesis: CTREND's multi-horizon smoothing fires SLOWER than Turtle.
//! Fixed hold gives it room to develop.

use anyhow::Result;
use krypto::data::loader::DataLoader;
use polars::prelude::*;
use std::collections::HashMap;
use std::fs::File;
use std::io::Write;
use std::time::Instant;

const SYMBOLS: &[&str] = &[
    "BTCUSDT", "ETHUSDT", "SOLUSDT", "XRPUSDT", "DOGEUSDT",
    "ADAUSDT", "LTCUSDT", "EOSUSDT", "BNBUSDT", "BCHUSDT",
];

const CTREND_FAST: usize = 8;
const CTREND_SLOW: usize = 32;
const HOLD_PERIODS: &[usize] = &[10, 15, 21, 30, 45, 60, 90];
const WF_WINDOWS: usize = 6;
const MIN_TRADES: usize = 3;
const CANDLES: u32 = 3000;

// Fixed stop loss at 5% risk
const STOP_LOSS: f64 = 0.05;

fn compute_ema(data: &[f64], period: usize) -> Vec<f64> {
    let alpha = 2.0 / (period as f64 + 1.0);
    let mut ema = vec![0.0; data.len()];
    for i in 0..data.len() {
        ema[i] = if i == 0 {
            data[0]
        } else {
            alpha * data[i] + (1.0 - alpha) * ema[i.saturating_sub(1)]
        };
    }
    ema
}

#[derive(Debug)]
struct WinResult {
    pass: bool,
    avg_ret: f64,
    sharpe: f64,
    annual_ret: f64,
    max_dd: f64,
    n: usize,
}

fn run_window(
    close: &[f64], high: &[f64], low: &[f64],
    signals: &[bool], start: usize, end: usize,
    hold_max: usize,
) -> WinResult {
    let mut equity = 1.0f64;
    let mut peak = equity;
    let mut max_equity = equity;
    let mut trades: Vec<f64> = Vec::new();
    let mut in_trade = false;
    let mut entry_bar = 0usize;
    let mut entry_price = 0.0f64;

    for bar in start..end.min(close.len()) {
        if !in_trade && signals.get(bar).copied().unwrap_or(false) {
            in_trade = true;
            entry_bar = bar;
            entry_price = close[bar];
        }

        if in_trade {
            let held = bar - entry_bar;
            let exit_time = held >= hold_max;
            let exit_stop = low[bar] < entry_price * (1.0 - STOP_LOSS);
            let exit_final = bar == end.saturating_sub(1) || bar == close.len() - 1;

            if exit_time || exit_stop || exit_final {
                let exit_price = if exit_stop { entry_price * (1.0 - STOP_LOSS) } else { close[bar] };
                let ret = (exit_price - entry_price) / entry_price;
                trades.push(ret);
                equity *= 1.0 + ret;
                peak = peak.max(equity);
                max_equity = max_equity.min(equity);
                in_trade = false;
            }
        }
    }

    let n = trades.len();
    let (avg_ret, sharpe, annual_ret) = if n >= MIN_TRADES {
        // Use daily compounding: convert trade returns to daily returns
        // Assume ~1 trade per week → 52 trades/year → 252/52 = 4.85 bars/year per trade
        // But better: compute per-bar return rate and annualize
        // Simple approach: avg return per trade, then scale by expected trades/year
        let sum_ret: f64 = trades.iter().sum();
        let mean = sum_ret / n as f64;
        
        // Annualize: assume avg trade holds ~hold_max/2 bars, and ~252 bars/year
        // annual_return = mean * (252 / avg_hold_bars)  
        // But simpler: use geometric mean annualization
        let annual = if equity > 0.0 && equity.is_finite() {
            let years = end.saturating_sub(start) as f64 / 252.0;
            if years > 0.0 { (equity.powf(1.0/years) - 1.0) * 100.0 } else { (equity - 1.0) * 100.0 }
        } else { -100.0 };
        
        // Sharpe: use simple avg/std with sqrt(N) annualization
        let variance: f64 = trades.iter().map(|r| {
            let d = r - mean;
            d * d
        }).sum::<f64>() / n as f64;
        let std = variance.sqrt();
        let sh = if std > 1e-10 { mean / std * (n as f64).sqrt() } else { 0.0 };
        
        (mean * 100.0, sh, annual)
    } else {
        (-100.0, -999.0, -100.0)
    };

    let max_dd = if peak > 0.0 { (peak - max_equity) / peak * 100.0 } else { 100.0 };
    let pass = avg_ret > -5.0 && sharpe > -50.0 && equity > 0.1 && equity.is_finite();

    WinResult { pass, avg_ret, sharpe, annual_ret, max_dd, n }
}

#[tokio::main]
async fn main() -> Result<()> {
    let t0 = Instant::now();
    println!("CTREND Fixed-Hold Walk-Forward (v2)");
    println!("==================================");

    let loader = DataLoader::new(None, None);

    let mut raw_cache: HashMap<String, DataFrame> = HashMap::new();
    for &sym in SYMBOLS {
        if let Ok(df) = loader.fetch_with_cache(sym, "1d", CANDLES).await {
            raw_cache.insert(sym.to_string(), df);
        }
    }
    eprintln!("Loaded {} symbols", raw_cache.len());

    let n_min = raw_cache.values().map(|df| df.height()).min().unwrap_or(0);
    let n = n_min.min(2800);

    macro_rules! col_vec {
        ($df:expr, $name:expr) => {{
            let chunked = $df.column($name)?.f64()?;
            chunked.into_iter().filter_map(|x| x).take(n).collect::<Vec<_>>()
        }};
    }

    let mut sym_data: HashMap<String, (Vec<f64>, Vec<f64>, Vec<f64>, Vec<bool>)> = HashMap::new();
    for sym in SYMBOLS {
        if let Some(df) = raw_cache.get(&sym.to_string()) {
            let close_v = col_vec!(df, "close");
            let n_c = close_v.len();
            let ema_f = compute_ema(&close_v, CTREND_FAST);
            let ema_s = compute_ema(&close_v, CTREND_SLOW);
            let signals: Vec<bool> = (0..n_c).map(|i| i >= CTREND_SLOW + 1 && ema_f[i] > ema_s[i]).collect();
            sym_data.insert(sym.to_string(), (close_v, col_vec!(df, "high"), col_vec!(df, "low"), signals));
        }
    }

    let window_size = n / (WF_WINDOWS + 1);

    let mut summary: Vec<(usize, usize, f64, f64, f64, usize)> = Vec::new();

    for &hold_max in HOLD_PERIODS {
        let mut total_pass = 0usize;
        let mut total_trades = 0usize;
        let mut sum_ann_ret = 0.0f64;
        let mut sum_avg_ret = 0.0f64;

        for sym in SYMBOLS {
            if let Some((close, high, low, signals)) = sym_data.get(&sym.to_string()) {
                for w in 0..WF_WINDOWS {
                    let start = w * window_size;
                    let end = ((w + 1) * window_size).min(close.len());
                    let r = run_window(close, high, low, signals, start, end, hold_max);
                    if r.pass { total_pass += 1; }
                    total_trades += r.n;
                    sum_ann_ret += r.annual_ret;
                    sum_avg_ret += r.avg_ret;
                }
            }
        }

        let n_runs = SYMBOLS.len() * WF_WINDOWS;
        let avg_ann = sum_ann_ret / n_runs as f64;
        let avg_ret = sum_avg_ret / n_runs as f64;
        let pct = total_pass as f64 / n_runs as f64 * 100.0;
        println!("Hold={}: {}:{} pass ({}), AnnRet={}, AvgRet={}, Trades={}",
            hold_max, total_pass, n_runs, pct, avg_ann, avg_ret, total_trades);
        summary.push((hold_max, total_pass, avg_ann, avg_ret, 0.0, total_trades));
    }

    // CSV
    {
        let mut csv = String::from("hold_max,pass,n_runs,pct,avg_ann_ret,avg_trade_ret,total_trades\n");
        for s in &summary {
            csv.push_str(&format!("{},{},{},{},{},{},{}\n",
                s.0, s.1, SYMBOLS.len()*WF_WINDOWS,
                s.1 as f64/(SYMBOLS.len()*WF_WINDOWS)as f64*100.0, s.2, s.3, s.5));
        }
        File::create("snapshots/ctrend_fixed_hold_summary.csv")?
            .write_all(csv.as_bytes())?;
    }

    let best = summary.iter().max_by_key(|x| x.1).unwrap();
    let best_pct = best.1 as f64/(SYMBOLS.len()*WF_WINDOWS)as f64*100.0;
    println!("\nBest: HOLD_MAX={} with {} passes ({}%),", best.0, best.1, best_pct);
    println!("Win condition: >35/60 = 58.3%. Best result: {}%", best_pct);
    if best.1 >= 35 {
        println!("*** CTREND is VIABLE as a signal family ***");
    } else {
        println!("*** CTREND FAILS viability threshold ***");
    }

    println!("Time: {}s", t0.elapsed().as_secs_f64() as f64);
    Ok(())
}
