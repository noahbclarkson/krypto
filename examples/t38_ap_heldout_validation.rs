//! T38-held-out: Pre-2026 held-out validation of REGIME_ATR_PERIOD=64 vs AP=12
//!
//! Motivation: AP=64 was found on the same 7-window walk-forward harness as AP=12.
//! The Sharpe cliff (6.19 at AP=64 → 3.07 at AP=65) is characteristic of overfitting.
//! This harness tests AP=64 vs AP=12 on a pre-2026 held-out split.

use anyhow::Result;
use krypto::data::loader::DataLoader;
use std::collections::HashMap;

const CANDLES: u32 = 3000;
const HOLD_MAX: usize = 12;
const TAKER_FEE: f64 = 0.001;
const MIN_TRADES: usize = 3;
const TURTLE_ENTRY: usize = 21;
const TURTLE_ATR_PERIOD: usize = 24;
const TURTLE_ATR_MULT: f64 = 2.00;
const ATR_ENTRY_MULT: f64 = 0.00;
const REGIME_LOOKBACK: usize = 42;
const ATR_RANK_T: f64 = 24.0;
const TEST_APS: [usize; 2] = [12, 64];
const SYMBOLS: [&str; 6] = ["BTCUSDT","ETHUSDT","SOLUSDT","XRPUSDT","DOGEUSDT","ADAUSDT"];

struct SymData { close: Vec<f64>, high: Vec<f64>, low: Vec<f64>, vol: Vec<f64> }

fn atr_at(high: &[f64], low: &[f64], close: &[f64], period: usize, idx: usize) -> f64 {
    if idx < period { return 0.0; }
    let mut trs = Vec::with_capacity(period);
    for i in (idx + 1 - period)..=idx {
        let h = high[i];
        let l = low[i];
        let c0 = close[i.saturating_sub(1)];
        trs.push((h - l).max((h - c0).abs()).max((l - c0).abs()));
    }
    trs.iter().sum::<f64>() / period as f64
}

fn btc_atr_pct(btc_close: &[f64], btc_high: &[f64], btc_low: &[f64], period: usize, lookback: usize, idx: usize) -> f64 {
    if idx < period.max(lookback) { return 50.0; }
    let curr_atr = atr_at(btc_high, btc_low, btc_close, period, idx);
    let mut hist = Vec::new();
    for j in (idx + 1 - lookback)..=idx {
        if j >= period {
            hist.push(atr_at(btc_high, btc_low, btc_close, period, j));
        }
    }
    if hist.is_empty() { return 50.0; }
    let count = hist.iter().filter(|&&x| x < curr_atr).count();
    (count as f64 / hist.len() as f64) * 100.0
}

fn annualised_sharpe(daily_rets: &[f64]) -> f64 {
    if daily_rets.is_empty() { return 0.0; }
    let mean = daily_rets.iter().sum::<f64>() / daily_rets.len() as f64;
    let var = daily_rets.iter().map(|x| (x - mean).powi(2)).sum::<f64>() / daily_rets.len() as f64;
    if var == 0.0 { return 0.0; }
    (mean / var.sqrt()) * (365.0_f64).sqrt()
}

fn turtle_signal(close: &[f64], high: &[f64], low: &[f64], entry_period: usize, atr_period: usize, atr_mult: f64, idx: usize) -> bool {
    if idx < entry_period { return false; }
    let max_close = close[idx - entry_period..idx].iter().fold(f64::NEG_INFINITY, |a, &b| a.max(b));
    if close[idx] > max_close {
        if atr_mult > 0.0 {
            let atr = atr_at(high, low, close, atr_period, idx);
            if atr > 0.0 && close[idx] < max_close + atr * atr_mult { return false; }
        }
        return true;
    }
    false
}

fn run_backtest(
    close: &[f64], high: &[f64], low: &[f64],
    btc_close: &[f64], btc_high: &[f64], btc_low: &[f64],
    regime_atr_period: usize, train_start: usize, test_start: usize,
) -> (bool, f64, f64, f64, usize) {
    let mut daily_rets = Vec::new();
    let mut pos_entered = false;
    let mut entry_price = 0.0f64;
    let mut bars_held = 0usize;
    let mut highest_high = 0.0f64;
    let mut peak_equity = 1.0f64;
    let mut max_dd = 0.0f64;
    let mut equity = 1.0f64;
    let mut trades = 0usize;

    for i in test_start..close.len() {
        let btc_rank = btc_atr_pct(btc_close, btc_high, btc_low, regime_atr_period, REGIME_LOOKBACK, i);
        let passes_regime = btc_rank >= ATR_RANK_T;

        if !pos_entered && i >= train_start && passes_regime {
            if turtle_signal(close, high, low, TURTLE_ENTRY, TURTLE_ATR_PERIOD, ATR_ENTRY_MULT, i) {
                pos_entered = true;
                entry_price = close[i] * (1.0 + TAKER_FEE);
                bars_held = 0;
                highest_high = high[i];
                trades += 1;
            }
        }

        if pos_entered {
            bars_held += 1;
            highest_high = highest_high.max(high[i]);
            let exit_price = close[i] * (1.0 - TAKER_FEE);
            let ret = (exit_price - entry_price) / entry_price;
            daily_rets.push(ret);
            equity *= 1.0 + ret;
            peak_equity = peak_equity.max(equity);
            let dd = (peak_equity - equity) / peak_equity;
            max_dd = max_dd.max(dd);

            let atr = atr_at(high, low, close, TURTLE_ATR_PERIOD, i);
            let trail_stop = highest_high - atr * TURTLE_ATR_MULT;
            let hit_trail = atr > 0.0 && close[i] < trail_stop;
            if bars_held >= HOLD_MAX || hit_trail {
                pos_entered = false;
                entry_price = 0.0;
            }
        }
    }
    let sharpe = annualised_sharpe(&daily_rets);
    let ret = (equity - 1.0) * 100.0;
    let pass = daily_rets.len() >= MIN_TRADES && sharpe > 0.0;
    (pass, sharpe, ret, max_dd * 100.0, trades)
}

#[tokio::main]
async fn main() -> Result<()> {
    let loader = DataLoader::new(None, None);
    let mut sym_data: HashMap<String, SymData> = HashMap::new();
    let mut min_len = usize::MAX;

    for sym in SYMBOLS {
        let df = loader.fetch_data(sym, "1d", CANDLES).await?;
        let close = df.column("close")?.f64()?.into_no_null_iter().collect::<Vec<_>>();
        let high = df.column("high")?.f64()?.into_no_null_iter().collect::<Vec<_>>();
        let low = df.column("low")?.f64()?.into_no_null_iter().collect::<Vec<_>>();
        let vol = df.column("volume")?.f64()?.into_no_null_iter().collect::<Vec<_>>();
        if close.len() < min_len { min_len = close.len(); }
        sym_data.insert(sym.to_string(), SymData { close, high, low, vol });
    }

    let btc = sym_data.get("BTCUSDT").unwrap();
    let n = btc.close.len();
    let train_end = n - 252;
    let test_start = train_end;

    println!("Data: {} bars. Train 0..{}. Held-out (pre-2026): {}..{}", n, train_end, test_start, n);
    println!();

    let mut results = Vec::new();
    for &ap in &TEST_APS {
        let mut total_trades = 0usize;
        let mut pass_count = 0usize;
        let mut sharpe_sum = 0.0f64;
        let mut ret_sum = 0.0f64;
        let mut dd_sum = 0.0f64;

        for sym in SYMBOLS {
            let d = sym_data.get(sym).unwrap();
            let (pass, sharpe, ret, dd, trades) = run_backtest(
                &d.close, &d.high, &d.low, &btc.close, &btc.high, &btc.low, ap, train_end, test_start
            );
            println!("AP={:2} | {:8} | {} | Sharpe={} | Ret={:.1}% | DD={:.1}% | trades={}",
                ap, sym, if pass { "PASS" } else { "FAIL" }, format_sharpe(sharpe), ret, dd, trades);
            total_trades += trades;
            if pass { pass_count += 1; }
            sharpe_sum += sharpe;
            ret_sum += ret;
            dd_sum += dd;
        }
        let n_s = SYMBOLS.len() as f64;
        println!("AP={:2} SUMMARY: {}/{} pass | Avg Sharpe={} | Avg Ret={:.1}% | Avg DD={:.1}% | trades={}",
            ap, pass_count, SYMBOLS.len(), format_sharpe(sharpe_sum/n_s), ret_sum/n_s, dd_sum/n_s, total_trades);
        println!();
        results.push((ap, pass_count, sharpe_sum/n_s, ret_sum/n_s, dd_sum/n_s, total_trades));
    }

    let (ap_a, _pass_a, sharpe_a, ret_a, dd_a, _) = results[0];
    let (ap_b, _pass_b, sharpe_b, ret_b, dd_b, _) = results[1];
    println!("=== HELD-OUT VERDICT ===");
    if sharpe_b > sharpe_a {
        let pct = (sharpe_b / sharpe_a - 1.0) * 100.0;
        println!("AP={} WINS: Sharpe={} vs {} (+{:.1}%), Ret={:.1}% vs {:.1}%, DD={:.1}% vs {:.1}%",
            ap_b, format_sharpe(sharpe_b), format_sharpe(sharpe_a), pct, ret_b, ret_a, dd_b, dd_a);
        println!("AP={} RECOMMENDED (held-out validated)", ap_b);
    } else {
        let pct = (sharpe_a / sharpe_b - 1.0) * 100.0;
        println!("AP={} does NOT beat AP={} on held-out: Sharpe={} vs {} ({:.1}% worse)",
            ap_b, ap_a, format_sharpe(sharpe_b), format_sharpe(sharpe_a), pct);
        println!("AP={} was likely overfit. Revert to plateau (AP=39-42) or AP=12.", ap_b);
    }
    Ok(())
}

fn format_sharpe(s: f64) -> String { format!("{:.3}", s) }