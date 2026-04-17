//! Monte Carlo overfitting test for CTREND multi-horizon momentum signal.
//! Block-shuffle CLOSE within year blocks to break signal while preserving vol structure.
//! Tests whether CTREND's 1438x equity (progress chart) is genuine or overfitted.
//!
//! CTREND signal: multi-horizon price momentum + volume confirmation
//!   - Entry: score > 0.35 (long), score < -0.35 (short)
//!   - Exit: fixed 21-bar hold
//!
//! Usage: cargo run --example ctrend_monte_carlo --profile sweep

use krypto::data::loader::DataLoader;
use polars::prelude::*;
use rand::prelude::*;
use rand::rngs::StdRng;
use rand::SeedableRng;

const SYMBOLS: [&str; 5] = ["BTCUSDT", "ETHUSDT", "SOLUSDT", "XRPUSDT", "DOGEUSDT"];
const CANDLES: u32 = 2000;
const N_PERMS: usize = 100;

const HOLD_BARS: usize = 21;
const SCORE_THRESHOLD: f64 = 0.35;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("=== Monte Carlo Overfitting Test: CTREND Multi-Horizon Momentum ===\n");
    let loader = DataLoader::new(None, None);

    let mut rng = StdRng::from_entropy();
    let mut all_results: Vec<SimResult> = vec![];

    for sym in SYMBOLS {
        let df = match loader.fetch_with_cache(sym, "1d", CANDLES).await {
            Ok(df) => df,
            Err(e) => { eprintln!("WARNING: {} load failed: {}", sym, e); continue; }
        };

        let close_raw = vec_from_series_f64(df.column("close")?);
        let volume_raw = vec_from_series_f64(df.column("volume")?);

        if close_raw.len() < 200 { continue; }

        // Real Sharpe
        let real_sharpe = compute_ctrender_sharpe(&close_raw, &volume_raw);

        // Shuffled distribution
        let mut shuffled_sharpes: Vec<f64> = vec![];
        for _ in 0..N_PERMS {
            let mut close_shuffled = close_raw.clone();
            block_shuffle_close(&mut close_shuffled, &mut rng);
            shuffled_sharpes.push(compute_ctrender_sharpe(&close_shuffled, &volume_raw));
        }
        shuffled_sharpes.sort_by(|a, b| a.partial_cmp(b).unwrap());

        let p95 = shuffled_sharpes[(N_PERMS as f64 * 0.95) as usize];
        let p50 = shuffled_sharpes[N_PERMS / 2];
        let beat = shuffled_sharpes.iter().filter(|&&s| s >= real_sharpe).count();

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

    if all_results.is_empty() {
        println!("No data loaded."); return Ok(());
    }

    let n_syms = all_results.len();
    let avg_real = all_results.iter().map(|r| r.real_sharpe).sum::<f64>() / n_syms as f64;
    let avg_p50  = all_results.iter().map(|r| r.p50).sum::<f64>() / n_syms as f64;
    let avg_p95  = all_results.iter().map(|r| r.p95).sum::<f64>() / n_syms as f64;

    println!("=== Aggregate ({} symbols) ===", n_syms);
    println!("Average real Sharpe:    {:.3}", avg_real);
    println!("Average median shuffled:{:.3}", avg_p50);
    println!("Average 95th pct:       {:.3}", avg_p95);
    if avg_p50 > 0.0 {
        println!("Real / Median ratio:   {:.2}x", avg_real / avg_p50);
    } else {
        println!("Real / Median ratio:   N/A (median is negative)");
    }

    let beat_total: usize = all_results.iter().map(|r| r.beat).sum();
    let total_perms = n_syms * N_PERMS;
    println!("Total shuffles >= real: {}/{} ({:.0}%)", beat_total, total_perms, beat_total as f64 / total_perms as f64 * 100.0);

    if avg_real > avg_p95 {
        println!("\n✅ EDGE IS GENUINE — CTREND signal significantly outperforms shuffled baseline");
    } else if avg_real > 0.0 && avg_real > avg_p50 {
        println!("\n⚠️ EDGE IS MARGINAL — CTREND beats shuffled but not decisively");
    } else {
        println!("\n❌ EDGE IS SUSPECT — shuffled baseline matches or beats CTREND signal");
    }

    println!("\nNote: CTREND with fixed 21-bar hold (not Chandelier exit).");
    println!("      Turtle+Chandelier already passed MC (0/100 shuffled > real).");

    Ok(())
}

// Compute CTREND Sharpe ratio from close+volume
fn compute_ctrender_sharpe(close: &[f64], volume: &[f64]) -> f64 {
    let n = close.len();
    if n < 200 { return 0.0; }

    let signals = compute_ctrender_signals(close, volume);
    simulate_fixed_hold_sharpe(close, &signals, HOLD_BARS)
}

// CTREND signal generation (same as progress_equity_curves.rs ctrend_signals)
fn compute_ctrender_signals(close: &[f64], volume: &[f64]) -> Vec<i32> {
    let n = close.len();
    let mut out_sig = vec![0i32; n];
    if n < 127 { return out_sig; }

    let vol_sma_20 = sma_vec(volume, 20);
    let vol_sma_63 = sma_vec(volume, 63);
    let ret_5   = rolling_ret(close, 5);
    let ret_21  = rolling_ret(close, 21);
    let ret_63  = rolling_ret(close, 63);
    let ret_126 = rolling_ret(close, 126);
    let rv_21   = rolling_vol(close, 21);
    let rv_63   = rolling_vol(close, 63);

    for i in 126..n {
        let short_vol = rv_21[i].max(1e-6);
        let med_vol   = rv_63[i].max(1e-6);

        let price_score = 0.15 * (ret_5[i]   / short_vol)
                        + 0.35 * (ret_21[i] / short_vol)
                        + 0.30 * (ret_63[i] / med_vol)
                        + 0.20 * (ret_126[i]/ med_vol);

        let vol_ratio_fast = if vol_sma_20[i] > 1e-9 && i < volume.len() {
            volume[i] / vol_sma_20[i]
        } else { 1.0 };
        let vol_ratio_slow = if vol_sma_63[i] > 1e-9 {
            vol_sma_20[i] / vol_sma_63[i]
        } else { 1.0 };

        let price_dir = if ret_21[i] > 0.0 { 1.0 } else if ret_21[i] < 0.0 { -1.0 } else { 0.0 };
        let short_dir = if ret_5[i]  > 0.0 { 1.0 } else if ret_5[i]  < 0.0 { -1.0 } else { 0.0 };

        let volume_score = 0.20 * (vol_ratio_fast.ln()).clamp(-1.5, 1.5) * short_dir
                         + 0.20 * (vol_ratio_slow.ln()).clamp(-1.5, 1.5) * price_dir;

        let score = price_score + volume_score;
        if score > SCORE_THRESHOLD {
            out_sig[i] = 1;
        } else if score < -SCORE_THRESHOLD {
            out_sig[i] = -1;
        }
    }
    out_sig
}

fn simulate_fixed_hold_sharpe(close: &[f64], signals: &[i32], hold_bars: usize) -> f64 {
    let n = close.len();
    let mut returns: Vec<f64> = vec![];
    let mut position: Option<(usize, f64)> = None;

    for bar in 126..n {
        if let Some((entry_bar, entry_price)) = position {
            let held = bar - entry_bar;
            if held >= hold_bars {
                returns.push(close[bar] / entry_price - 1.0);
                position = None;
                if signals[bar] == 1 {
                    position = Some((bar, close[bar]));
                }
            }
        } else if signals[bar] == 1 {
            position = Some((bar, close[bar]));
        }
    }

    if returns.len() < 5 { return 0.0; }
    let mean = returns.iter().sum::<f64>() / returns.len() as f64;
    let std  = (returns.iter().map(|&r| (r - mean).powi(2)).sum::<f64>() / returns.len() as f64).sqrt();
    if std == 0.0 { return 0.0; }
    mean / std * (252.0_f64.sqrt())
}

fn sma_vec(vals: &[f64], period: usize) -> Vec<f64> {
    let mut out = vec![0.0; vals.len()];
    for i in period..vals.len() {
        let sum: f64 = vals[i+1-period..=i].iter().sum();
        out[i] = sum / period as f64;
    }
    out
}

fn rolling_ret(values: &[f64], lookback: usize) -> Vec<f64> {
    let mut out = vec![0.0; values.len()];
    for i in lookback..values.len() {
        if values[i] > 0.0 && values[i - lookback] > 0.0 {
            out[i] = (values[i] / values[i - lookback]).ln();
        }
    }
    out
}

fn rolling_vol(values: &[f64], lookback: usize) -> Vec<f64> {
    let rets = rolling_ret(values, lookback);
    let mut out = vec![0.0; values.len()];
    for i in (lookback * 2)..values.len() {
        let slice = &rets[i+1-lookback..=i];
        if slice.len() == lookback {
            let mean = slice.iter().sum::<f64>() / lookback as f64;
            let var  = slice.iter().map(|&r| (r - mean).powi(2)).sum::<f64>() / lookback as f64;
            out[i] = var.sqrt();
        }
    }
    out
}

fn vec_from_series_f64(s: &Series) -> Vec<f64> {
    s.f64().unwrap().into_iter().map(|x| x.unwrap_or(0.0)).collect()
}

// Block-shuffle close within year blocks (preserves per-block autocorrelation structure)
fn block_shuffle_close(close: &mut [f64], rng: &mut StdRng) {
    let n = close.len();
    let block_size = 365;
    let n_blocks = n / block_size;

    for i in 0..n_blocks {
        let start = i * block_size;
        let end = (start + block_size).min(n);
        let len = end - start;
        if len < 2 { continue; }

        // Fisher-Yates shuffle within block
        let mut j = len;
        while j > 1 {
            let k = rng.gen_range(0..j);
            close.swap(start + k, start + j - 1);
            j -= 1;
        }
    }
}

struct SimResult {
    sym: String,
    real_sharpe: f64,
    p50: f64,
    p95: f64,
    beat: usize,
}
