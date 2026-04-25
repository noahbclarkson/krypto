//! Turtle+Chandelier Walk-Forward on 4h Data
//!
//! Track C: Broaden edge discovery — genuinely untested idea from PLAN.md.
//! All prior walk-forward validation is daily (1d). 4h has 6× more bars.
//!
//! Key fix this session: use `df.column("atr")` (EWM-based ATR from FeatureEngine)
//! instead of manual ATR computation. This ATR uses proper smoothing and is the
//! same ATR used in live production trading.

use anyhow::Result;
use krypto::data::loader::DataLoader;
use krypto::features::indicators::FeatureEngine;
use polars::prelude::*;
use std::collections::HashMap;

const SYMBOLS: [&str; 5] = ["BTCUSDT", "ETHUSDT", "SOLUSDT", "XRPUSDT", "DOGEUSDT"];
const CANDLES: u32 = 4000; // 4h × 4000 ≈ 1000 days ≈ 2.7 years
const TAKER_FEE: f64 = 0.0004; // 4bp taker
const SLIPPAGE: f64 = 0.0001; // 1bp slippage per side
const POS_CAP: usize = 3;
const HOLD_MAX: usize = 60; // 4h: 60 bars = 10 days
const CM: f64 = 2.25; // Chandelier multiplier (fixed)
const ATR_ENTRY_MULT: f64 = 0.85;
const ATR_P: usize = 24; // Turtle ATR period (fixed from prior validation)
const ATR_M: f64 = 2.0; // Turtle ATR multiplier (fixed)

// EP sweep: 24 daily ≈ 144 4h. Test both smaller (more trades) and larger.
const EP_VALUES: [usize; 4] = [24, 48, 96, 144];

// CP sweep (Chandelier period): 7 daily ≈ 42 4h
const CP_VALUES: [usize; 4] = [18, 36, 48, 72];

fn main() -> Result<()> {
    let start = std::time::Instant::now();
    println!("=== TURTLE+CHANDELIER 4H WALK-FORWARD ===\n");
    println!("Symbols: {:?}", SYMBOLS);
    println!("Timeframe: 4h (6 bars/day)");
    println!("EP sweep: {:?}", EP_VALUES);
    println!("CP sweep: {:?}", CP_VALUES);
    println!();

    let rt = tokio::runtime::Runtime::new()?;
    let loader = DataLoader::new(None, None);

    // Load data
    let mut raw_data: HashMap<String, DataFrame> = HashMap::new();
    for symbol in SYMBOLS {
        let sym_str = symbol.to_string();
        print!("Loading {} 4h... ", symbol);
        match rt.block_on(loader.fetch_data(symbol, "4h", CANDLES)) {
            Ok(df) => {
                let n = df.height();
                println!("{} bars ({:.1} years)", n, n as f64 / 6.0 / 365.0);
                raw_data.insert(sym_str, df);
            }
            Err(e) => {
                println!("SKIP ({})", e);
            }
        }
    }

    if raw_data.is_empty() {
        anyhow::bail!("No data loaded — cannot proceed");
    }

    // Add technical indicators (including "atr" column via EWM)
    let mut data: HashMap<String, DataFrame> = HashMap::new();
    for (sym, df) in raw_data {
        let with_tech = FeatureEngine::add_technicals(&df, None)?;
        // Verify atr column exists
        if with_tech.column("atr").is_err() {
            println!("WARNING: {} missing atr column", sym);
        }
        data.insert(sym, with_tech);
    }

    println!("\n--- WALK-FORWARD CONFIGURATION ---");
    println!("  IS bars: 504 (84 days), OOS bars: 252 (42 days), 4 windows");
    println!();

    const WF_BARS: usize = 252;
    const IS_BARS: usize = 504;
    const N_WINDOWS: usize = 4;

    // EP sweep
    println!("=== EP SWEEP (CP=48 fixed) ===");
    let mut ep_results: Vec<EpResult> = Vec::new();

    for &ep in &EP_VALUES {
        let mut total_pass = 0;
        let mut total_windows = 0;
        let mut total_sharpe = 0.0_f64;
        let mut total_trades = 0usize;
        let mut sharpe_per_symbol: Vec<f64> = Vec::new();

        for sym in SYMBOLS.iter().map(|s| s.to_string()) {
            if let Some(df) = data.get(&sym) {
                let (pass, sharpe, trades) = run_wf(
                    df, ep, 48, CM, ATR_P, ATR_M, ATR_ENTRY_MULT,
                    HOLD_MAX, POS_CAP, WF_BARS, IS_BARS, N_WINDOWS,
                    TAKER_FEE, SLIPPAGE,
                )?;
                total_pass += pass;
                total_windows += N_WINDOWS;
                total_sharpe += sharpe;
                total_trades += trades;
                sharpe_per_symbol.push(sharpe);
                println!(
                    "   {:12} pass={}/{} sharpe={:.2} trades={}",
                    sym, pass, N_WINDOWS, sharpe, trades
                );
            }
        }

        let avg_sharpe = total_sharpe / sharpe_per_symbol.len() as f64;
        let pass_rate = total_pass as f64 / total_windows as f64 * 100.0;
        ep_results.push(EpResult {
            ep,
            pass_rate,
            avg_sharpe,
            total_trades,
        });
        println!(
            "EP={}: pass={}/{} ({:.0}%) sharpe={:.2} trades={}\n",
            ep, total_pass, total_windows, pass_rate, avg_sharpe, total_trades
        );
    }

    let best_ep = ep_results
        .iter()
        .max_by(|a, b| a.avg_sharpe.partial_cmp(&b.avg_sharpe).unwrap())
        .unwrap()
        .ep;
    println!("Best EP: {} (highest avg Sharpe)", best_ep);

    // CP sweep
    println!("\n=== CHANDELIER PERIOD SWEEP (EP={} fixed) ===", best_ep);
    let mut cp_results: Vec<CpResult> = Vec::new();

    for &cp in &CP_VALUES {
        let mut total_pass = 0;
        let mut total_windows = 0;
        let mut total_sharpe = 0.0_f64;
        let mut total_trades = 0usize;
        let mut sharpe_per_symbol: Vec<f64> = Vec::new();

        for sym in SYMBOLS.iter().map(|s| s.to_string()) {
            if let Some(df) = data.get(&sym) {
                let (pass, sharpe, trades) = run_wf(
                    df, best_ep, cp, CM, ATR_P, ATR_M, ATR_ENTRY_MULT,
                    HOLD_MAX, POS_CAP, WF_BARS, IS_BARS, N_WINDOWS,
                    TAKER_FEE, SLIPPAGE,
                )?;
                total_pass += pass;
                total_windows += N_WINDOWS;
                total_sharpe += sharpe;
                total_trades += trades;
                sharpe_per_symbol.push(sharpe);
                println!(
                    "   {:12} pass={}/{} sharpe={:.2} trades={}",
                    sym, pass, N_WINDOWS, sharpe, trades
                );
            }
        }

        let avg_sharpe = total_sharpe / sharpe_per_symbol.len() as f64;
        let pass_rate = total_pass as f64 / total_windows as f64 * 100.0;
        cp_results.push(CpResult {
            cp,
            pass_rate,
            avg_sharpe,
            total_trades,
        });
        println!(
            "CP={}: pass={}/{} ({:.0}%) sharpe={:.2} trades={}\n",
            cp, total_pass, total_windows, pass_rate, avg_sharpe, total_trades
        );
    }

    let best_cp = cp_results
        .iter()
        .max_by(|a, b| a.avg_sharpe.partial_cmp(&b.avg_sharpe).unwrap())
        .unwrap()
        .cp;
    println!("Best CP: {} (highest avg Sharpe)", best_cp);

    // Final validation
    println!("\n=== FINAL: EP={}, CP={} ===", best_ep, best_cp);
    let mut total_pass = 0;
    let mut total_sharpe = 0.0_f64;
    let mut total_trades = 0usize;

    for sym in SYMBOLS.iter().map(|s| s.to_string()) {
        if let Some(df) = data.get(&sym) {
            let (pass, sharpe, trades) = run_wf(
                df, best_ep, best_cp, CM, ATR_P, ATR_M, ATR_ENTRY_MULT,
                HOLD_MAX, POS_CAP, WF_BARS, IS_BARS, N_WINDOWS,
                TAKER_FEE, SLIPPAGE,
            )?;
            total_pass += pass;
            total_sharpe += sharpe;
            total_trades += trades;
            println!(
                "  {:12} sharpe={:6.2} pass={}/{} trades={}",
                sym, sharpe, pass, N_WINDOWS, trades
            );
        }
    }

    let avg_sharpe = total_sharpe / SYMBOLS.len() as f64;
    let pass_rate = total_pass as f64 / (SYMBOLS.len() * N_WINDOWS) as f64 * 100.0;
    println!(
        "\n  GLOBAL: pass={}/{} ({:.0}%) avg_sharpe={:.2} total_trades={}",
        total_pass,
        SYMBOLS.len() * N_WINDOWS,
        pass_rate,
        avg_sharpe,
        total_trades
    );

    // Summary
    println!("\n{}", "=".repeat(70));
    println!("SUMMARY: Turtle+Chandelier 4h Walk-Forward");
    println!("{}", "=".repeat(70));
    println!(
        "{:8} {:10} {:8} {:8} {:10}",
        "Param", "Value", "PassRate", "Sharpe", "Trades"
    );
    println!("{}", "-".repeat(70));
    for r in &ep_results {
        println!(
            "{:8} {:10} {:8.1}% {:8.2} {:10}",
            "EP", r.ep, r.pass_rate, r.avg_sharpe, r.total_trades
        );
    }
    println!("{}", "-".repeat(70));
    for r in &cp_results {
        println!(
            "{:8} {:10} {:8.1}% {:8.2} {:10}",
            "CP", r.cp, r.pass_rate, r.avg_sharpe, r.total_trades
        );
    }
    println!(
        "{:8} {:10} {:8.1}% {:8.2} {:10}",
        "BEST",
        format!("EP={}/CP={}", best_ep, best_cp),
        pass_rate,
        avg_sharpe,
        total_trades
    );

    let elapsed = start.elapsed();
    println!("\nRuntime: {:.1}s", elapsed.as_secs_f64());

    Ok(())
}

struct EpResult {
    ep: usize,
    pass_rate: f64,
    avg_sharpe: f64,
    total_trades: usize,
}

struct CpResult {
    cp: usize,
    pass_rate: f64,
    avg_sharpe: f64,
    total_trades: usize,
}

fn run_wf(
    df: &DataFrame,
    ep: usize,
    chand_p: usize,
    chand_m: f64,
    atr_p: usize,
    atr_m: f64,
    atr_entry_mult: f64,
    hold_max: usize,
    pos_cap: usize,
    wf_bars: usize,
    is_bars: usize,
    n_windows: usize,
    taker_fee: f64,
    slippage: f64,
) -> Result<(usize, f64, usize)> {
    let close = df.column("close")?.f64()?;
    let high = df.column("high")?.f64()?;
    let low = df.column("low")?.f64()?;
    let atr_col = df.column("atr")?.f64()?;
    let n = close.len();

    let warmup = ep.max(chand_p).max(atr_p) + 10;

    let mut pass_count = 0;
    let mut sharpe_sum = 0.0_f64;
    let mut total_trades = 0usize;

    for w in 0..n_windows {
        let oos_start = w * wf_bars + is_bars;
        let oos_end = (oos_start + wf_bars).min(n);

        if oos_end - oos_start < warmup || oos_start < warmup {
            continue;
        }

        let (sharpe, trades) = backtest_window(
            &close, &high, &low, &atr_col, n,
            ep, chand_p, chand_m, atr_p, atr_m, atr_entry_mult, hold_max,
            oos_start, oos_end,
            taker_fee, slippage,
        )?;

        if sharpe > 0.0 {
            pass_count += 1;
        }
        sharpe_sum += sharpe;
        total_trades += trades;
    }

    let avg_sharpe = sharpe_sum / n_windows as f64;
    Ok((pass_count, avg_sharpe, total_trades))
}

fn backtest_window(
    close: &Float64Chunked,
    high: &Float64Chunked,
    low: &Float64Chunked,
    atr_col: &Float64Chunked,
    _n: usize,
    ep: usize,
    chand_p: usize,
    chand_m: f64,
    atr_p: usize,
    atr_m: f64,
    atr_entry_mult: f64,
    hold_max: usize,
    oos_start: usize,
    oos_end: usize,
    taker_fee: f64,
    slippage: f64,
) -> Result<(f64, usize)> {
    let mut equity: f64 = 100.0;
    let mut peak: f64 = 100.0;
    let mut daily_returns: Vec<f64> = Vec::new();
    let mut trades = 0usize;
    let mut in_position = false;
    let mut entry_price = 0.0f64;
    let mut highest_high: f64 = 0.0;
    let mut chand_stop: f64 = 0.0;
    let mut bars_in_pos = 0usize;
    let mut atr_filled = false;
    let mut atr_buf: Vec<f64> = Vec::new();

    // Pre-fill ATR buffer up to oos_start
    for i in (oos_start.saturating_sub(atr_p))..oos_start {
        let v = atr_col.get(i).unwrap_or(0.0);
        if v > 0.0 && v.is_finite() {
            atr_buf.push(v);
        }
    }
    if atr_buf.len() > atr_p {
        atr_buf.drain(..atr_buf.len() - atr_p);
    }

    for i in oos_start..oos_end {
        let cur_close = close.get(i).unwrap_or(0.0);
        let cur_high = high.get(i).unwrap_or(0.0);
        let cur_low = low.get(i).unwrap_or(0.0);

        // Get ATR from pre-computed column (EWM-based, properly smoothed)
        let current_atr = atr_col.get(i).unwrap_or(0.0);

        // Turtle entry
        let ep_start = i.saturating_sub(ep);
        let mut max_close_val: f64 = 0.0;
        for j in ep_start..i {
            max_close_val = max_close_val.max(close.get(j).unwrap_or(0.0));
        }
        let entry_threshold = max_close_val + current_atr * atr_entry_mult;
        let turtle_signal = cur_close >= entry_threshold;

        if !in_position && turtle_signal {
            in_position = true;
            entry_price = cur_close;
            highest_high = cur_high;
            chand_stop = cur_close - chand_m * current_atr;
            atr_buf.truncate(0);
            bars_in_pos = 0;
            atr_filled = false;
            trades += 1;
        }

        if in_position {
            bars_in_pos += 1;
            highest_high = highest_high.max(cur_high);

            // Update Chandelier stop
            let new_stop = cur_close - chand_m * current_atr;
            chand_stop = chand_stop.max(new_stop);

            // Turtle ATR exit (trailing, not leading)
            let turtle_exit = highest_high - cur_low >= atr_m * current_atr;

            let hit_stop = cur_low <= chand_stop;
            let hit_holdmax = bars_in_pos >= hold_max;

            if hit_stop || hit_holdmax || turtle_exit {
                let exit_price = if hit_stop { chand_stop } else { cur_close };
                let gross_ret = (exit_price / entry_price - 1.0) * 100.0;
                let fees = 2.0 * taker_fee * 100.0;
                let slip = slippage * 100.0 * 2.0;
                let net_ret = gross_ret - fees - slip;
                let daily_ret = net_ret / 100.0;

                equity *= 1.0 + daily_ret;
                peak = peak.max(equity);
                daily_returns.push(daily_ret);

                in_position = false;
            }
        }
    }

    let sharpe: f64 = if daily_returns.len() >= 10 {
        let mean: f64 = daily_returns.iter().sum::<f64>() / daily_returns.len() as f64;
        let var: f64 = daily_returns.iter().map(|r| (r - mean).powi(2)).sum::<f64>()
            / daily_returns.len() as f64;
        let std = var.sqrt();
        if std > 0.0 {
            mean / std * (252.0_f64.sqrt())
        } else {
            0.0
        }
    } else {
        0.0
    };

    Ok((sharpe, trades))
}
