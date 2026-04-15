use anyhow::Result;
use krypto::data::loader::DataLoader;
use polars::prelude::*;
use std::collections::HashMap;

const SYMBOLS: &[&str] = &[
    "BTCUSDT", "ETHUSDT", "SOLUSDT", "XRPUSDT", "DOGEUSDT", "ADAUSDT",
];
const CANDLES: u32 = 3000;
const TAKER_FEE: f64 = 0.001;
const HOLD_BARS: usize = 21;
const TRAIN_BARS: usize = 252;
const TEST_BARS: usize = 252;

fn main() -> Result<()> {
    let runtime = tokio::runtime::Runtime::new()?;
    let mut cache = HashMap::new();

    runtime.block_on(async {
        let loader = DataLoader::new(None, None);
        for &sym in SYMBOLS {
            if let Ok(df) = loader.fetch_data(sym, "1d", CANDLES).await {
                cache.insert(sym.to_string(), df);
            }
        }
    });

    let sample_df = cache.values().next().unwrap();
    let n = sample_df.height();
    let mut ad_series = HashMap::new();

    let period = 47;
    let alpha1 = 2.0 / (period as f64 + 1.0);
    let alpha2 = 2.0 / (period as f64 * 2.0 + 1.0);

    for &sym in SYMBOLS {
        let df = cache.get(sym).unwrap();
        let c_series = df.column("close")?.f64()?;
        let mut c = vec![0.0; n];
        for i in 0..n {
            c[i] = c_series.get(i).unwrap_or(0.0);
        }

        let mut ema1 = vec![0.0; n];
        let mut ema2 = vec![0.0; n];
        ema1[0] = c[0];
        ema2[0] = c[0];
        for i in 1..n {
            ema1[i] = alpha1 * c[i] + (1.0 - alpha1) * ema1[i - 1];
            ema2[i] = alpha2 * c[i] + (1.0 - alpha2) * ema2[i - 1];
        }
        let mut ad = vec![0.0; n];
        for i in 0..n {
            ad[i] = ema1[i] - ema2[i];
        }
        ad_series.insert(sym, ad);
    }

    let top_k = 3;
    let mut per_sym_pnl = HashMap::new();
    let mut per_sym_trades = HashMap::new();
    for &sym in SYMBOLS {
        per_sym_pnl.insert(sym, 1.0f64);
        per_sym_trades.insert(sym, 0);
    }

    let mut book_equity = 1.0f64;
    let mut nodoge_equity = 1.0f64;
    let mut book_trades = 0;

    let mut sym_isolated_ret = HashMap::new();
    for &sym in SYMBOLS {
        sym_isolated_ret.insert(sym, 0.0);
    }

    // We walk forward exactly like the harness, step by test_bars
    for test_start in (TRAIN_BARS..n.saturating_sub(TEST_BARS)).step_by(TEST_BARS) {
        let train_end = test_start;
        let test_end = (test_start + TEST_BARS).min(n);

        let mut ad_means = HashMap::new();
        for &sym in SYMBOLS {
            let ad = &ad_series[sym];
            let mut sum = 0.0;
            let mut count = 0;
            for j in 0..train_end {
                if ad[j] != 0.0 {
                    sum += ad[j];
                    count += 1;
                }
            }
            let mean = if count > 0 { sum / count as f64 } else { 0.0 };
            ad_means.insert(sym, mean);
        }

        let mut i = test_start;
        while i < test_end.min(n.saturating_sub(HOLD_BARS + 1)) {
            let mut scores: Vec<(&str, f64)> = Vec::new();
            let mut nodoge_scores: Vec<(&str, f64)> = Vec::new();

            for &sym in SYMBOLS {
                let ad = ad_series[sym][i];
                let mean = ad_means[sym];
                if ad > mean {
                    scores.push((sym, ad));
                    if sym != "DOGEUSDT" {
                        nodoge_scores.push((sym, ad));
                    }
                }
            }

            scores.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());
            nodoge_scores.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());

            let top: Vec<_> = scores.iter().take(top_k).collect();
            let top_nodoge: Vec<_> = nodoge_scores.iter().take(top_k).collect();

            if top.is_empty() {
                i += 1;
                continue;
            }

            let mut valid_trades = 0;
            let mut day_ret_sum = 0.0;

            for &(sym, _) in &top {
                let df = cache.get(*sym).unwrap();
                let o_series = df.column("open")?.f64()?;
                let entry = o_series.get(i + 1).unwrap_or(0.0);
                let exit = o_series.get(i + 1 + HOLD_BARS).unwrap_or(0.0);
                if entry > 0.0 && exit > 0.0 {
                    let ret = (exit / entry) * (1.0 - TAKER_FEE) - 1.0;
                    day_ret_sum += ret;
                    valid_trades += 1;

                    *sym_isolated_ret.get_mut(*sym).unwrap() += ret;

                    let p = per_sym_pnl.get_mut(*sym).unwrap();
                    *p *= 1.0 + ret; // Compounded isolated
                    *per_sym_trades.get_mut(*sym).unwrap() += 1;
                }
            }

            if valid_trades > 0 {
                // For realistic portfolio return without the leverage compounding explosion
                let avg_ret = day_ret_sum / valid_trades as f64;
                book_equity *= 1.0 + avg_ret;
                book_trades += 1;
            }

            let mut nodoge_day_ret = 0.0;
            let mut nodoge_valid = 0;
            for &(sym, _) in &top_nodoge {
                let df = cache.get(*sym).unwrap();
                let o_series = df.column("open")?.f64()?;
                let entry = o_series.get(i + 1).unwrap_or(0.0);
                let exit = o_series.get(i + 1 + HOLD_BARS).unwrap_or(0.0);
                if entry > 0.0 && exit > 0.0 {
                    let ret = (exit / entry) * (1.0 - TAKER_FEE) - 1.0;
                    nodoge_day_ret += ret;
                    nodoge_valid += 1;
                }
            }
            if nodoge_valid > 0 {
                nodoge_equity *= 1.0 + (nodoge_day_ret / nodoge_valid as f64);
            }

            i += HOLD_BARS + 1; // non-overlapping
        }
    }

    println!("A/D period=3 Component Attribution (Real Strategy Execution Model, 1/k sized)\n");
    println!("| Symbol  | Isolated Sum | Realistic Compounded | Trades |");
    println!("|---------|--------------|----------------------|--------|");
    for &sym in SYMBOLS {
        let sum_ret = sym_isolated_ret[sym] * 100.0;
        let comp_ret = (per_sym_pnl[sym] - 1.0) * 100.0;
        let count = per_sym_trades[sym];
        println!(
            "| {:<7} | {:>11.1f}% | {:>19.1f}% | {:>6} |",
            sym, sum_ret, comp_ret, count
        );
    }

    println!("\nPortfolio Level (Non-overlapping, 1/k avg return):");
    println!(
        "Base5 (with DOGE) : {:>12.1f}%",
        (book_equity - 1.0) * 100.0
    );
    println!(
        "NoDOGE            : {:>12.1f}%",
        (nodoge_equity - 1.0) * 100.0
    );

    Ok(())
}
