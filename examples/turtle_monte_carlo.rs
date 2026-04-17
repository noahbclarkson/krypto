//! Monte Carlo overfitting test for Turtle+Chandelier
//! NOTE: Renamed from ctrend_monte_carlo.rs — tests Turtle, not CTREND
//! Block-shuffle returns within year blocks to break strategy edge while preserving market structure.
//! Run N permutations per symbol. If real Sharpe > 95th percentile of shuffles → genuine edge.
//!
//! Usage: cargo run --example ctrend_monte_carlo --profile sweep

use std::collections::HashMap;
use krypto::data::loader::DataLoader;
use polars::prelude::*;
use rand::prelude::*;
use rand::rngs::StdRng;
use rand::SeedableRng;

const SYMBOLS: [&str; 5] = ["BTCUSDT", "ETHUSDT", "SOLUSDT", "XRPUSDT", "DOGEUSDT"];
const CANDLES: u32 = 2000;
const N_PERMS: usize = 100;

const EP: usize = 21;
const CHAND_P: usize = 20;
const CHAND_M: f64 = 2.15;
const HOLD_MAX: usize = 45;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("=== Monte Carlo Overfitting Test: Turtle+Chandelier ===\n");
    let loader = DataLoader::new(None, None);

    let mut rng = StdRng::from_entropy();
    let mut all_results: Vec<SimResult> = vec![];

    for sym in SYMBOLS {
        let df = match loader.fetch_with_cache(sym, "1d", CANDLES).await {
            Ok(df) => df,
            Err(e) => { eprintln!("WARNING: {} load failed: {}", sym, e); continue; }
        };

        let close_v: Vec<f64> = df.column("close")?.f64()?.into_iter().filter_map(|x| x).collect();
        let high_v: Vec<f64> = df.column("high")?.f64()?.into_iter().filter_map(|x| x).collect();
        let low_v: Vec<f64>  = df.column("low")?.f64()?.into_iter().filter_map(|x| x).collect();

        if close_v.len() < 100 { continue; }

        // Real Sharpe
        let real_sharpe = turtle_sharpe(&close_v, &high_v, &low_v);

        // Shuffled distribution
        let mut shuffled: Vec<f64> = vec![];
        for _ in 0..N_PERMS {
            let mut sc = close_v.clone();
            block_shuffle(&mut sc, &mut rng);
            shuffled.push(turtle_sharpe(&sc, &high_v, &low_v));
        }
        shuffled.sort_by(|a, b| a.partial_cmp(b).unwrap());

        let p95 = shuffled[(N_PERMS as f64 * 0.95) as usize];
        let p50 = shuffled[N_PERMS / 2];
        let beat = shuffled.iter().filter(|&&s| s >= real_sharpe).count();

        println!("{}", sym);
        println!("  Real Sharpe:         {:.3}", real_sharpe);
        println!("  Median shuffled:    {:.3}", p50);
        println!("  95th pct shuffled:   {:.3}", p95);
        println!("  Shuffles >= real:   {}/{} ({:.0}%)", beat, N_PERMS, beat as f64 / N_PERMS as f64 * 100.0);

        let verdict = if real_sharpe > p95 {
            "✅ GENUINE"
        } else if real_sharpe > p50 {
            "⚠️ MARGINAL"
        } else {
            "❌ SUSPECT"
        };
        println!("  Verdict: {}", verdict);
        println!();

        all_results.push(SimResult { sym: sym.to_string(), real_sharpe, p50, p95, beat });
    }

    // Aggregate
    if all_results.is_empty() {
        println!("No data loaded."); return Ok(());
    }

    let avg_real = all_results.iter().map(|r| r.real_sharpe).sum::<f64>() / all_results.len() as f64;
    let avg_p50  = all_results.iter().map(|r| r.p50).sum::<f64>() / all_results.len() as f64;
    println!("=== Aggregate ({} symbols) ===", all_results.len());
    println!("Average real Sharpe:    {:.3}", avg_real);
    println!("Average median shuffled:{:.3}", avg_p50);
    println!("Real / Median ratio:    {:.2}x", avg_real / avg_p50);

    if avg_real > avg_p50 * 1.5 {
        println!("\n✅ EDGE IS GENUINE — strategy significantly outperforms block-shuffled baseline");
    } else if avg_real > avg_p50 {
        println!("\n⚠️ EDGE IS MARGINAL — strategy beats shuffled but not dramatically");
    } else {
        println!("\n❌ EDGE MAY BE OVERFIT — shuffled baseline matches or beats real strategy");
    }

    Ok(())
}

struct SimResult {
    sym: String,
    real_sharpe: f64,
    p50: f64,
    p95: f64,
    beat: usize,
}

// Block-shuffle within ~365-bar year blocks (preserves autocorrelation)
fn block_shuffle(data: &mut [f64], rng: &mut StdRng) {
    let n = data.len();
    let block_size = 365;
    let n_blocks = n / block_size;

    for i in 0..n_blocks {
        let start = i * block_size;
        let end = (start + block_size).min(n);
        let len = end - start;
        if len < 2 { continue; }

        // Fisher-Yates
        let mut j = len;
        while j > 1 {
            let k = rng.gen_range(0..j);
            data.swap(start + k, start + j - 1);
            j -= 1;
        }
    }
}

fn atr(high: &[f64], low: &[f64], close: &[f64], period: usize, idx: usize) -> f64 {
    if idx < period { return 0.0; }
    let mut true_ranges = vec![];
    for i in (idx + 1 - period..=idx).rev() {
        let tr = high[i].max(close[i]) - low[i].min(close[i]);
        true_ranges.push(tr);
    }
    true_ranges.iter().sum::<f64>() / period as f64
}

fn ema(vals: &[f64], period: usize) -> Vec<f64> {
    let alpha = 2.0 / (period as f64 + 1.0);
    let mut ema = vec![vals[0]; vals.len()];
    for i in 1..vals.len() {
        ema[i] = alpha * vals[i] + (1.0 - alpha) * ema[i-1];
    }
    ema
}

fn turtle_sharpe(close: &[f64], high: &[f64], low: &[f64]) -> f64 {
    let n = close.len();
    if n < EP + HOLD_MAX + 10 { return 0.0; }

    // ATR series
    let mut atr_vals = vec![0.0; n];
    for i in EP..n {
        atr_vals[i] = atr(high, low, close, CHAND_P, i);
    }

    // EMA for ATR smoothing
    let atr_ema = ema(&atr_vals, 5);

    let mut returns: Vec<f64> = vec![];
    let mut position: Option<(usize, f64)> = None; // (entry_bar, entry_price)

    for bar in EP..n {
        let entry = *close[..bar].iter().take(EP).max_by(|a, b| a.partial_cmp(b).unwrap()).unwrap_or(&0.0);
        let is_long = close[bar] >= entry;

        let stop = close[bar] - CHAND_M * atr_ema[bar];

        if let Some((entry_bar, entry_price)) = position {
            let held = bar - entry_bar;
            let should_exit = held >= HOLD_MAX || close[bar] < stop;

            if should_exit {
                returns.push(close[bar] / entry_price - 1.0);
                position = None;
                if is_long { position = Some((bar, close[bar])); }
            }
        } else if is_long {
            position = Some((bar, close[bar]));
        }
    }

    if returns.len() < 5 { return 0.0; }
    let mean = returns.iter().sum::<f64>() / returns.len() as f64;
    let std = (returns.iter().map(|&r| (r - mean).powi(2)).sum::<f64>() / returns.len() as f64).sqrt();
    if std == 0.0 { return 0.0; }
    mean / std * (252.0_f64.sqrt())
}