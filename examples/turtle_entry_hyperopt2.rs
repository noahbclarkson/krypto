//! Hyperparameter Optimization: TURTLE_ENTRY Sweep (Detailed)
//!
//! Sweeping the Turtle entry period from 5 to 100 across 9 universes.

use anyhow::Result;
use krypto::data::loader::DataLoader;
use krypto::features::indicators::FeatureEngine;
use polars::prelude::*;
use std::collections::HashMap;

const TRAIN_BARS: usize = 252;
const TEST_BARS: usize = 252;
const HOLD_BARS: usize = 21;
const TAKER_FEE: f64 = 0.001;
const CANDLES: u32 = 3000;
const MIN_TRADES: usize = 3;

const LOAD_SYMBOLS: &[&str] = &[
    "BTCUSDT", "ETHUSDT", "SOLUSDT", "XRPUSDT", "DOGEUSDT", "ADAUSDT", "LTCUSDT", "BNBUSDT",
    "EOSUSDT", "BCHUSDT",
];
const UNIVERSES: &[(&str, &[&str])] = &[
    (
        "Base5",
        &["ETHUSDT", "SOLUSDT", "XRPUSDT", "DOGEUSDT", "ADAUSDT"],
    ),
    ("NoDOGE", &["ETHUSDT", "SOLUSDT", "XRPUSDT", "ADAUSDT"]),
    ("Legacy4", &["ETHUSDT", "XRPUSDT", "LTCUSDT", "EOSUSDT"]),
    (
        "Legacy5BNB",
        &["ETHUSDT", "XRPUSDT", "LTCUSDT", "BNBUSDT", "EOSUSDT"],
    ),
    (
        "OldGuardNoBNB",
        &["ETHUSDT", "XRPUSDT", "LTCUSDT", "EOSUSDT", "BCHUSDT"],
    ),
    (
        "LargeCaps5",
        &["ETHUSDT", "SOLUSDT", "XRPUSDT", "BNBUSDT", "ADAUSDT"],
    ),
    ("Legacy3", &["XRPUSDT", "LTCUSDT", "EOSUSDT"]),
    (
        "LowVolume5",
        &["XRPUSDT", "LTCUSDT", "EOSUSDT", "BCHUSDT", "ADAUSDT"],
    ),
    ("OldGuard4", &["XRPUSDT", "LTCUSDT", "EOSUSDT", "BCHUSDT"]),
];

#[tokio::main]
async fn main() -> Result<()> {
    let loader = DataLoader::new(None, None);
    let mut cache: HashMap<String, DataFrame> = HashMap::new();
    let mut min_len = usize::MAX;
    for s in LOAD_SYMBOLS {
        let raw = loader.fetch_with_cache(s, "1d", CANDLES).await?;
        let df = FeatureEngine::add_technicals(&raw, None)?;
        min_len = min_len.min(df.height());
        cache.insert(s.to_string(), df);
    }
    let n = min_len.min(2800);
    for df in cache.values_mut() {
        if df.height() > n {
            *df = df.slice(0, n);
        }
    }

    // Logical integer range: 5 to 100 in steps of 2 (or 1, let's do 1 for 10-30, then 5)
    let mut periods: Vec<usize> = (5..=30).collect();
    periods.extend((35..=100).step_by(5));

    let mut best_score = -999.0;
    let mut best_p = 20;
    let mut base_score = 0.0;

    let mut eq_curves: HashMap<usize, Vec<f64>> = HashMap::new();

    for &p in &periods {
        let mut total_sh = 0.0;
        let mut passes = 0;
        let mut qp = 0;
        let mut total_wins = 0;
        let mut uni_eqs: Vec<f64> = vec![];

        for &(uni_name, syms) in UNIVERSES {
            let mut u_eq = vec![1.0; n];
            let mut u_rets = vec![];
            for &sym in syms {
                let df = cache.get(sym).unwrap();
                let close = df.column("close").unwrap().f64().unwrap();
                let high = df.column("high").unwrap().f64().unwrap();

                let mut pos = false;
                let mut entry_px = 0.0;
                let mut entry_bar = 0;

                for i in 0..n {
                    if i < p {
                        continue;
                    }

                    if pos && i >= entry_bar + HOLD_BARS {
                        let exit_px = close.get(i).unwrap_or(0.0);
                        if entry_px > 0.0 && exit_px > 0.0 {
                            let ret = (exit_px / entry_px - 1.0) - TAKER_FEE;
                            u_rets.push(ret);
                        }
                        pos = false;
                    }

                    if !pos {
                        let mut hh = 0.0;
                        for j in i - p..i {
                            let h = high.get(j).unwrap_or(0.0);
                            if h > hh {
                                hh = h;
                            }
                        }
                        let c = close.get(i).unwrap_or(0.0);
                        if c > hh {
                            pos = true;
                            entry_px = c;
                            entry_bar = i;
                        }
                    }
                }
            }

            let sh = if u_rets.len() > MIN_TRADES {
                let m = u_rets.iter().sum::<f64>() / u_rets.len() as f64;
                let v =
                    u_rets.iter().map(|x| (x - m).powi(2)).sum::<f64>() / (u_rets.len() - 1) as f64;
                m / v.sqrt()
            } else {
                0.0
            };

            total_sh += sh;
            if sh > 0.0 {
                passes += 1;
            }
        }

        let avg_sh = total_sh / UNIVERSES.len() as f64;
        let score = passes as f64 + avg_sh;
        println!(
            "Period={:3} | Score: {:.2} | Avg Sharpe: {:.2} | Passes: {}/9",
            p, score, avg_sh, passes
        );

        if p == 20 {
            base_score = score;
        }
        if score > best_score {
            best_score = score;
            best_p = p;
        }
    }

    println!(
        "Best: {} (Score {:.2}) vs Base 20 (Score {:.2})",
        best_p, best_score, base_score
    );

    // Rerun to get full equity curves for Baseline, Winner, and Runnerups
    let targets = vec![
        20,
        best_p,
        best_p.saturating_add(2),
        best_p.saturating_sub(2),
    ];
    let syms = UNIVERSES[0].1; // Base5

    let mut csv_lines = vec!["step,Baseline(20),Winner,RunnerUp1,RunnerUp2".to_string()];

    // Collect data per target
    let mut eq_data: HashMap<usize, Vec<f64>> = HashMap::new();
    for &p in &targets {
        let mut eq = vec![1.0; n];
        let mut daily_rets = vec![0.0; n];
        for &sym in syms {
            let df = cache.get(sym).unwrap();
            let close = df.column("close").unwrap().f64().unwrap();
            let high = df.column("high").unwrap().f64().unwrap();
            let mut pos = false;
            let mut entry_px = 0.0;
            let mut entry_bar = 0;
            for i in 0..n {
                if i < p {
                    continue;
                }
                if pos && i >= entry_bar + HOLD_BARS {
                    let exit_px = close.get(i).unwrap_or(0.0);
                    if entry_px > 0.0 && exit_px > 0.0 {
                        let ret = (exit_px / entry_px - 1.0) - TAKER_FEE;
                        // distribute ret at exit bar for charting
                        daily_rets[i] += ret / syms.len() as f64;
                    }
                    pos = false;
                }
                if !pos {
                    let mut hh = 0.0;
                    for j in i - p..i {
                        let h = high.get(j).unwrap_or(0.0);
                        if h > hh {
                            hh = h;
                        }
                    }
                    let c = close.get(i).unwrap_or(0.0);
                    if c > hh {
                        pos = true;
                        entry_px = c;
                        entry_bar = i;
                    }
                }
            }
        }
        let mut current_eq = 1.0;
        for i in 0..n {
            current_eq *= 1.0 + daily_rets[i];
            eq[i] = current_eq;
        }
        eq_data.insert(p, eq);
    }

    for i in 0..n {
        let b = eq_data.get(&20).unwrap()[i];
        let w = eq_data.get(&best_p).unwrap()[i];
        let r1 = eq_data.get(&targets[2]).unwrap()[i];
        let r2 = eq_data.get(&targets[3]).unwrap()[i];
        csv_lines.push(format!("{},{:.4},{:.4},{:.4},{:.4}", i, b, w, r1, r2));
    }

    std::fs::write("snapshots/turtle_entry_curves.csv", csv_lines.join("\n"))?;
    println!("Wrote snapshots/turtle_entry_curves.csv");

    Ok(())
}
