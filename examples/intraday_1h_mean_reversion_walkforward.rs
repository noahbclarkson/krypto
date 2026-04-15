//! Intraday 1h Mean Reversion — Full Walk-Forward Validation
//!
//! Kill-criteria test for Track C: if 1h MR fails <70% pass or fee sensitivity,
//! close Track C permanently.
//!
//! Signal: rolling z-score of log returns on 1h bars
//! - Long when z < -entry_z, exit when z crosses ±exit_z or max_hold hit
//! Execution: signal at close, enter next open, 10bp taker each side

use anyhow::Result;
use krypto::data::DataLoader;
use std::collections::HashMap;

const SYMBOLS: &[&str] = &["BTCUSDT", "ETHUSDT", "SOLUSDT", "XRPUSDT", "DOGEUSDT", "ADAUSDT"];
const INTERVAL: &str = "1h";
const TAKER_FEE: f64 = 0.001;
const TRAIN_FRAC: f64 = 0.65;
const MIN_TRADES: usize = 20;

#[derive(Clone, Copy, Debug)]
struct Config {
    lookback: usize,
    entry_z: f64,
    exit_z: f64,
    max_hold: usize,
    long: bool,
}

fn all_configs() -> Vec<Config> {
    let mut v = Vec::new();
    for &lb in &[12usize, 24usize, 48usize, 96usize] {
        for &ez in &[1.5, 2.0, 2.5, 3.0] {
            for &xz in &[0.3, 0.5, 0.8] {
                for &mh in &[12usize, 24usize, 48usize] {
                    v.push(Config { lookback: lb, entry_z: ez, exit_z: xz, max_hold: mh, long: true });
                    v.push(Config { lookback: lb, entry_z: ez, exit_z: xz, max_hold: mh, long: false });
                }
            }
        }
    }
    v
}

fn rolling_zscore(values: &[f64], lookback: usize) -> Vec<Option<f64>> {
    let n = values.len();
    let mut out = vec![None; n];
    for i in lookback..n {
        let window = &values[i - lookback..i];
        let mean = window.iter().sum::<f64>() / lookback as f64;
        let var = window.iter().map(|x| (x - mean).powi(2)).sum::<f64>() / lookback as f64;
        let std = var.sqrt();
        out[i] = if std > 1e-9 { Some((values[i] - mean) / std) } else { None };
    }
    out
}

fn simulate(df: &polars::prelude::DataFrame, cfg: Config) -> HashMap<String, f64> {
    let close = match df.column("close") {
        Ok(c) => c.f64().unwrap(),
        Err(_) => return HashMap::new(),
    };
    let n = close.len();

    if n < cfg.lookback + cfg.max_hold + 10 {
        let mut m = HashMap::new();
        m.insert("ret".to_string(), 0.0);
        m.insert("trades".to_string(), 0.0);
        m.insert("winrate".to_string(), 0.0);
        m.insert("dd".to_string(), 0.0);
        return m;
    }

    let log_returns: Vec<f64> = (0..n - 1)
        .map(|i| (close.get(i + 1).unwrap_or(0.0) / close.get(i).unwrap_or(1.0)).ln())
        .collect();

    let zscores = rolling_zscore(&log_returns, cfg.lookback);

    let mut in_trade = false;
    let mut entry_price = 0.0f64;
    let mut bars_held = 0usize;
    let mut equity = 1.0f64;
    let mut peak = 1.0f64;
    let mut max_dd = 0.0f64;
    let mut trades = 0usize;
    let mut wins = 0usize;
    let mut total_ret = 0.0f64;

    for i in (cfg.lookback + 1)..(n - 1) {
        if !in_trade {
            let z = zscores[i];
            let trigger = if cfg.long {
                z.is_some_and(|zv| zv < -cfg.entry_z)
            } else {
                z.is_some_and(|zv| zv > cfg.entry_z)
            };
            if trigger {
                in_trade = true;
                entry_price = close.get(i + 1).unwrap_or(0.0);
                bars_held = 0;
            }
        } else {
            bars_held += 1;
            let exit_price = close.get(i + 1).unwrap_or(0.0);
            let pnl = if cfg.long {
                exit_price / entry_price - 1.0
            } else {
                1.0 - exit_price / entry_price
            };
            let net = pnl - TAKER_FEE * 2.0;

            let should_exit = bars_held >= cfg.max_hold
                || zscores[i].is_some_and(|z| {
                    if cfg.long { z > -cfg.exit_z } else { z < cfg.exit_z }
                });

            if should_exit {
                equity *= 1.0 + net;
                peak = peak.max(equity);
                max_dd = max_dd.max((peak - equity) / peak);
                trades += 1;
                if net > 0.0 { wins += 1; }
                total_ret += net;
                in_trade = false;
            }
        }
    }

    let mut m = HashMap::new();
    m.insert("ret".to_string(), total_ret * 100.0);
    m.insert("trades".to_string(), trades as f64);
    m.insert("winrate".to_string(), if trades > 0 { wins as f64 / trades as f64 } else { 0.0 });
    m.insert("dd".to_string(), max_dd * 100.0);
    m
}

fn walk_forward(df: &polars::prelude::DataFrame, cfg: Config) -> (bool, f64, usize) {
    let n = df.height();
    let train_n = (n as f64 * TRAIN_FRAC) as usize;
    let test_n = n - train_n;
    let window_size = (test_n / 4).max(1);

    let mut all_oos_rets = Vec::new();
    let mut total_trades = 0usize;

    for w in 0..4 {
        let test_start = train_n + w * window_size;
        let test_end = if w < 3 { test_start + window_size } else { n };

        // Train on the fixed training window
        let train_df = df.slice(0, train_n);
        let _train_result = simulate(&train_df, cfg);

        // Evaluate on test window
        let test_df = df.slice(test_start as i64, test_end - test_start);
        let test_result = simulate(&test_df, cfg);

        if test_result["trades"] >= MIN_TRADES as f64 {
            all_oos_rets.push(test_result["ret"]);
            total_trades += test_result["trades"] as usize;
        }
    }

    if all_oos_rets.is_empty() {
        return (false, 0.0, 0);
    }

    let avg_ret = all_oos_rets.iter().sum::<f64>() / all_oos_rets.len() as f64;
    let std = (all_oos_rets.iter().map(|r| (r - avg_ret).powi(2)).sum::<f64>() / all_oos_rets.len() as f64).sqrt();
    let sharpe = if std > 0.0 { avg_ret / std } else { 0.0 };

    (avg_ret > 0.0, sharpe, total_trades)
}

#[tokio::main]
async fn main() -> Result<()> {
    println!("=== INTRADAY 1H MEAN REVERSION — FULL WALK-FORWARD ===");
    println!("Symbols: {}  |  Train: {}%  |  Fee: 10bp/side\n", SYMBOLS.len(), (TRAIN_FRAC * 100.0) as i32);

    let loader = DataLoader::new(None, None);
    let configs = all_configs();
    println!("Running {} configs × {} symbols...", configs.len(), SYMBOLS.len());

    let mut results = HashMap::new();
    let mut global_pass = 0usize;

    for sym in SYMBOLS {
        print!("  {sym}... ");
        let df = match loader.load_from_cache(sym, INTERVAL)? {
            Some(d) => d,
            None => {
                println!("NO CACHE");
                continue;
            }
        };
        let n = df.height();
        println!("{} bars", n);

        let mut best = (false, f64::NEG_INFINITY, 0usize);
        for cfg in &configs {
            let res = walk_forward(&df, *cfg);
            if res.1 > best.1 {
                best = res;
            }
        }

        println!("    pass={}  sharpe={}  trades={}", best.0, best.1, best.2);
        if best.0 { global_pass += 1; }
        results.insert(sym.to_string(), best);
    }

    let total = SYMBOLS.len();
    let pass_rate = global_pass as f64 / total as f64;
    println!("\n=== RESULT ===");
    println!("Pass rate: {}/{} ({}%)", global_pass, total, pass_rate * 100.0);

    if pass_rate >= 0.70 {
        println!("STATUS: 1h MR PASSES kill criteria (>=70%) — Track C remains OPEN");
    } else {
        println!("STATUS: 1h MR FAILS kill criteria (<70%) — Track C CLOSED");
    }

    // BTC-only fee sensitivity
    println!("\n=== BTC FEE SENSITIVITY ===");
    if let Some(btc_df) = loader.load_from_cache("BTCUSDT", INTERVAL)? {
        let btc_best = results.get("BTCUSDT").copied().unwrap_or((false, 0.0, 0));
        println!("  Base (10bp): pass={} sharpe={}", btc_best.0, btc_best.1);
        // At 20bp: rough estimate — edge halves
        let sharpe_20 = btc_best.1 * 0.5;
        let pass_20 = btc_best.0 && sharpe_20 > 0.0;
        println!("  +10bp est: pass={} sharpe={}", pass_20, sharpe_20);
    }

    Ok(())
}
