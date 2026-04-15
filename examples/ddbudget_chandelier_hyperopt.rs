//! DDBudget 3-Sleeve Chandelier Hyperparameter Optimization
//!
//! PURPOSE: Replace the fixed 54-bar hold in the A/D sleeve with Chandelier ATR exit,
//! and find the optimal Chandelier (period, multiplier) pair via walk-forward validation
//! across all 9 universes.
//!
//! KNOWN ISSUE (from PLAN/MEMORY): The fixed 54-bar hold was identified as the single
//! biggest execution flaw in the system. Chandelier(period=45, mult=2.5) was validated
//! in chandelier_exit_wf.rs (beats Fixed54 on 4/9 universes) but NEVER INTEGRATED into
//! the DDBudget 3-sleeve harness.
//!
//! This harness:
//! - Adds Chandelier ATR exit as the exit logic for the A/D sleeve (replacing fixed HOLD_AD)
//! - MACD+Regime and SmallVol sleeves still use fixed HOLD_OTHER=21 bars
//! - Sweeps Chandelier period: 15-80 in steps of 5 (14 values)
//! - Sweeps Chandelier multiplier: 1.5-5.0 in steps of 0.25 (15 values)
//! - Baseline: Fixed54 hold (same as existing DDBudget 3-sleeve walk-forward)
//! - Walks forward: 252-bar train / 252-bar test across all 9 universes
//! - Exports equity curves for Baseline + Top-3 configs for charting

use anyhow::Result;
use krypto::data::loader::DataLoader;
use polars::prelude::*;
use std::collections::HashMap;
use std::fs::OpenOptions;
use std::io::Write;
use std::time::Instant;

const CANDLES: u32 = 3000;
const TRAIN_BARS: usize = 252;
const TEST_BARS: usize = 252;
const HOLD_FIXED: usize = 54;
const HOLD_OTHER: usize = 21;
const AD_PERIOD: usize = 5; // hyperopt winner 2026-04-13 (was 47)
const TAKER_FEE: f64 = 0.001;
const WARMUP: usize = 200;
const MIN_TRADES: usize = 3;
const MIN_BARS_CHAND: usize = 5;

const CHAND_PERIOD_MIN: usize = 15;
const CHAND_PERIOD_MAX: usize = 80;
const CHAND_PERIOD_STEP: usize = 5;
const CHAND_MULT_MIN: f64 = 1.5;
const CHAND_MULT_MAX: f64 = 5.0;
const CHAND_MULT_STEP: f64 = 0.25;

const UNIVERSES: &[(&str, &[&str])] = &[
    ("Base5", &["BTCUSDT", "ETHUSDT", "SOLUSDT", "XRPUSDT", "DOGEUSDT", "ADAUSDT"]),
    ("NoDOGE", &["BTCUSDT", "ETHUSDT", "SOLUSDT", "XRPUSDT", "ADAUSDT"]),
    ("Legacy4", &["BTCUSDT", "ETHUSDT", "XRPUSDT", "LTCUSDT", "EOSUSDT"]),
    ("Legacy5BNB", &["BTCUSDT", "ETHUSDT", "XRPUSDT", "LTCUSDT", "BNBUSDT", "EOSUSDT"]),
    ("OldGuardNoBNB", &["BTCUSDT", "ETHUSDT", "XRPUSDT", "LTCUSDT", "EOSUSDT", "BCHUSDT"]),
    ("LargeCaps5", &["BTCUSDT", "ETHUSDT", "SOLUSDT", "XRPUSDT", "BNBUSDT", "ADAUSDT"]),
    ("Legacy3", &["BTCUSDT", "XRPUSDT", "LTCUSDT", "EOSUSDT"]),
    ("LowVolume5", &["XRPUSDT", "LTCUSDT", "EOSUSDT", "BCHUSDT", "ADAUSDT"]),
    ("OldGuard4", &["BTCUSDT", "XRPUSDT", "LTCUSDT", "EOSUSDT", "BCHUSDT"]),
];

fn calc_sharpe(daily_rets: &[f64]) -> f64 {
    if daily_rets.len() < 5 { return 0.0; }
    let mean = daily_rets.iter().sum::<f64>() / daily_rets.len() as f64;
    let var = daily_rets.iter().map(|r| { let x = r - mean; x * x }).sum::<f64>()
        / daily_rets.len().max(1) as f64;
    let std = var.sqrt();
    if std < 1e-9 { return 0.0; }
    mean * 365.0 / (std * (365.0_f64).sqrt())
}

fn calc_max_dd_from(equity: &[f64], mut peak: f64) -> (f64, f64) {
    let mut max_dd = 0.0_f64;
    for &e in equity {
        if e > peak { peak = e; }
        let dd = (1.0 - e / peak) * 100.0;
        if dd > max_dd { max_dd = dd; }
    }
    (max_dd, peak)
}

fn true_range(h: f64, l: f64, prev_c: f64) -> f64 {
    (h - l).abs().max((h - prev_c).abs()).max((l - prev_c).abs())
}

fn atr_at(high: &[f64], low: &[f64], close: &[f64], period: usize, idx: usize) -> f64 {
    if idx < period { return 0.0; }
    let mut tr_sum = 0.0_f64;
    for i in idx.saturating_sub(period - 1)..=idx {
        let pc = if i > 0 { close[i - 1] } else { close[0] };
        tr_sum += true_range(high[i], low[i], pc);
    }
    tr_sum / period as f64
}

fn ddhard_exposure(dd_pct: f64) -> f64 {
    if dd_pct > 30.0 { 0.3 } else if dd_pct > 15.0 { 0.6 } else { 1.0 }
}

fn ad_signal(
    close: &[f64], high: &[f64], low: &[f64], vol: &[f64],
    period: usize, train_end: usize, bar: usize,
) -> Option<f64> {
    let warmup = WARMUP.max(period);
    if bar < warmup || bar > train_end { return None; }

    let alpha1 = 2.0 / (period as f64 + 1.0);
    let alpha2 = 2.0 / (period as f64 * 2.0 + 1.0);

    let mut ema1 = 0.0_f64;
    let mut ema2 = 0.0_f64;
    for j in warmup..=bar.min(train_end) {
        let r = high[j] - low[j];
        let m = if r > 1e-9 { ((close[j] - low[j]) - (high[j] - close[j])) / r } else { 0.0 };
        ema1 = alpha1 * m * vol[j] + (1.0 - alpha1) * ema1;
        ema2 = alpha2 * m * vol[j] + (1.0 - alpha2) * ema2;
    }
    let ad_now = ema1 - ema2;

    let mut sum = 0.0_f64;
    let mut cnt = 0usize;
    for j in warmup..train_end {
        let r = high[j] - low[j];
        let m = if r > 1e-9 { ((close[j] - low[j]) - (high[j] - close[j])) / r } else { 0.0 };
        let ea1 = alpha1 * m * vol[j] + (1.0 - alpha1) * 0.0;
        let ea2 = alpha2 * m * vol[j] + (1.0 - alpha2) * 0.0;
        sum += ea1 - ea2;
        cnt += 1;
    }
    let ad_mean = if cnt > 0 { sum / cnt as f64 } else { 0.0 };
    Some(ad_now - ad_mean)
}

fn macd_signal(
    close: &[f64], fast: usize, slow: usize, sig: usize,
    train_end: usize, bar: usize,
) -> Option<f64> {
    let warmup = WARMUP.max(slow).max(sig);
    if bar < warmup || bar > train_end { return None; }

    let ef_alp = 2.0 / (fast as f64 + 1.0);
    let es_alp = 2.0 / (slow as f64 + 1.0);
    let em_alp = 2.0 / (sig as f64 + 1.0);

    let mut ef = 0.0_f64;
    let mut es = 0.0_f64;
    let mut sig_line = 0.0_f64;

    for j in warmup..=bar.min(train_end) {
        ef = ef_alp * close[j] + (1.0 - ef_alp) * ef;
        es = es_alp * close[j] + (1.0 - es_alp) * es;
        let ml = ef - es;
        sig_line = em_alp * ml + (1.0 - em_alp) * sig_line;
    }

    let sma_start = (bar.saturating_sub(200)).max(warmup);
    let mut sma_sum = 0.0_f64;
    let mut sma_cnt = 0usize;
    for j in sma_start..=bar.min(train_end) {
        sma_sum += close[j];
        sma_cnt += 1;
    }
    let sma200 = if sma_cnt > 0 { sma_sum / sma_cnt as f64 } else { close[bar] };
    let regime = if close[bar] > sma200 { 1.0 } else { -1.0 };
    let macd_diff = ef - es - sig_line;
    Some(regime * macd_diff)
}

fn smallvol_rank(
    close: &[f64], vol: &[f64],
    all_close: &HashMap<&str, &[f64]>, all_vol: &HashMap<&str, &[f64]>,
    train_end: usize, bar: usize,
) -> Option<f64> {
    let warmup = WARMUP;
    if bar < warmup || bar > train_end { return None; }

    let my_dv = close[bar] * vol.get(bar).copied().unwrap_or(0.0);
    let mut dvs: Vec<f64> = Vec::new();
    for (sym, c) in all_close {
        if let Some(v) = all_vol.get(sym) {
            dvs.push(c[bar.min(c.len() - 1)] * v.get(bar.min(v.len() - 1)).copied().unwrap_or(0.0));
        }
    }
    if dvs.is_empty() { return Some(0.0); }
    dvs.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let pos = dvs.iter().position(|&x| x >= my_dv).unwrap_or(dvs.len() - 1);
    Some((dvs.len() - pos) as f64)
}

struct SymData {
    close: Vec<f64>, open: Vec<f64>,
    high: Vec<f64>, low: Vec<f64>, vol: Vec<f64>,
}

struct WfResult {
    ret: f64, sharpe: f64, max_dd: f64,
    trades: usize, win_rate: f64, pass: bool,
    equity_curve: Vec<f64>,
}

fn chandelier_exit(
    high: &[f64], low: &[f64], close: &[f64],
    entry_bar: usize, test_end: usize,
    chand_period: usize, chand_mult: f64,
) -> usize {
    let n = high.len();
    let max_exit = test_end.min(n - 1);
    let min_exit = (entry_bar + MIN_BARS_CHAND).max(entry_bar + 1);

    let mut highest_high = high[entry_bar];
    let mut exit_bar = max_exit;

    for bar in min_exit..=max_exit {
        if bar >= n { break; }
        highest_high = highest_high.max(high[bar]);
        let atr_val = atr_at(high, low, close, chand_period, bar);
        let exit_price = highest_high - chand_mult * atr_val;
        if close[bar] < exit_price {
            exit_bar = bar;
            break;
        }
    }
    exit_bar.min(max_exit)
}

struct SimOut {
    equity: f64,
    equity_curve: Vec<f64>,
    total_trades: usize,
    wins: usize,
}

fn run_sim_fixed(
    sym_data: &HashMap<String, SymData>,
    symbols: &[String],
    train_end: usize, test_start: usize, test_end: usize,
    start_equity: f64, peak_equity: f64,
) -> (WfResult, SimOut, f64) {
    let all_close: HashMap<&str, &[f64]> = sym_data.iter()
        .map(|(k, v)| (k.as_str(), v.close.as_slice())).collect();
    let all_vol: HashMap<&str, &[f64]> = sym_data.iter()
        .map(|(k, v)| (k.as_str(), v.vol.as_slice())).collect();

    let mut ad_eq = 1.0_f64; let mut mc_eq = 1.0_f64; let mut sm_eq = 1.0_f64;
    let mut ad_pk = 1.0_f64; let mut mc_pk = 1.0_f64; let mut sm_pk = 1.0_f64;

    let mut equity = start_equity;
    let mut equity_curve = vec![start_equity];
    let mut wins = 0usize;
    let mut total_trades = 0usize;

    let mut bar = test_start;
    while bar + 1 + HOLD_FIXED.max(HOLD_OTHER) + 2 < test_end {
        let mut ad_scores: Vec<(&str, f64)> = Vec::new();
        let mut mc_scores: Vec<(&str, f64)> = Vec::new();
        let mut sm_scores: Vec<(&str, f64)> = Vec::new();

        for sym in symbols {
            let Some(sd) = sym_data.get(sym) else { continue };
            if bar >= sd.close.len() { continue; }

            if let Some(score) = ad_signal(&sd.close, &sd.high, &sd.low, &sd.vol, AD_PERIOD, train_end, bar) {
                ad_scores.push((sym.as_str(), score));
            }
            if let Some(score) = macd_signal(&sd.close, 12, 26, 9, train_end, bar) {
                mc_scores.push((sym.as_str(), score));
            }
            if let Some(score) = smallvol_rank(&sd.close, &sd.vol, &all_close, &all_vol, train_end, bar) {
                sm_scores.push((sym.as_str(), score));
            }
        }

        ad_scores.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());
        mc_scores.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());
        sm_scores.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());

        if ad_scores.is_empty() && mc_scores.is_empty() && sm_scores.is_empty() {
            bar += 1; continue;
        }

        let ad_dd = if ad_pk > 0.0 { (1.0 - ad_eq / ad_pk) * 100.0 } else { 0.0 };
        let mc_dd = if mc_pk > 0.0 { (1.0 - mc_eq / mc_pk) * 100.0 } else { 0.0 };
        let sm_dd = if sm_pk > 0.0 { (1.0 - sm_eq / sm_pk) * 100.0 } else { 0.0 };

        let ad_w0 = ddhard_exposure(ad_dd);
        let mc_w0 = ddhard_exposure(mc_dd);
        let sm_w0 = ddhard_exposure(sm_dd);
        let tot = (ad_w0 + mc_w0 + sm_w0).max(1e-9);
        let ad_w = ad_w0 / tot;
        let mc_w = mc_w0 / tot;
        let sm_w = sm_w0 / tot;

        // AD sleeve (fixed hold)
        let mut ad_rets = Vec::new();
        for &(sym, _) in ad_scores.iter() {
            let Some(sd) = sym_data.get(sym) else { continue };
            let entry = sd.close.get(bar + 1).copied().unwrap_or(0.0);
            let exit = sd.close.get(bar + 1 + HOLD_FIXED).copied().unwrap_or(0.0);
            if entry > 0.0 && exit > 0.0 {
                let r = (exit / entry - 1.0) - 2.0 * TAKER_FEE;
                ad_rets.push(r);
                ad_pk = ad_pk.max(ad_eq * (1.0 + r));
                total_trades += 1;
                if r > 0.0 { wins += 1; }
            }
        }

        let mut port_ret = 0.0_f64;
        if !ad_rets.is_empty() {
            let avg = ad_rets.iter().sum::<f64>() / ad_rets.len() as f64;
            ad_eq *= 1.0 + avg;
            port_ret += ad_w * avg;
        }

        // MACD sleeve
        let mut mc_rets = Vec::new();
        for &(sym, _) in mc_scores.iter() {
            let Some(sd) = sym_data.get(sym) else { continue };
            let entry = sd.close.get(bar + 1).copied().unwrap_or(0.0);
            let exit = sd.close.get(bar + 1 + HOLD_OTHER).copied().unwrap_or(0.0);
            if entry > 0.0 && exit > 0.0 {
                let r = (exit / entry - 1.0) - 2.0 * TAKER_FEE;
                mc_rets.push(r);
                mc_pk = mc_pk.max(mc_eq * (1.0 + r));
                total_trades += 1;
                if r > 0.0 { wins += 1; }
            }
        }
        if !mc_rets.is_empty() {
            let avg = mc_rets.iter().sum::<f64>() / mc_rets.len() as f64;
            mc_eq *= 1.0 + avg;
            port_ret += mc_w * avg;
        }

        // SmallVol sleeve
        let mut sm_rets = Vec::new();
        for &(sym, _) in sm_scores.iter() {
            let Some(sd) = sym_data.get(sym) else { continue };
            let entry = sd.close.get(bar + 1).copied().unwrap_or(0.0);
            let exit = sd.close.get(bar + 1 + HOLD_OTHER).copied().unwrap_or(0.0);
            if entry > 0.0 && exit > 0.0 {
                let r = (exit / entry - 1.0) - 2.0 * TAKER_FEE;
                sm_rets.push(r);
                sm_pk = sm_pk.max(sm_eq * (1.0 + r));
                total_trades += 1;
                if r > 0.0 { wins += 1; }
            }
        }
        if !sm_rets.is_empty() {
            let avg = sm_rets.iter().sum::<f64>() / sm_rets.len() as f64;
            sm_eq *= 1.0 + avg;
            port_ret += sm_w * avg;
        }

        equity *= 1.0 + port_ret;
        equity_curve.push(equity);
        bar += HOLD_FIXED + 1;
    }

    let ret = (equity / start_equity - 1.0) * 100.0;
    let (max_dd, new_peak) = calc_max_dd_from(&equity_curve, peak_equity);
    let win_rate = if total_trades > 0 { wins as f64 / total_trades as f64 * 100.0 } else { 0.0 };
    let pass = total_trades >= MIN_TRADES && ret > 0.0;

    let result = WfResult { ret, sharpe: 0.0, max_dd, trades: total_trades, win_rate, pass, equity_curve: vec![] };
    let out = SimOut { equity, equity_curve, total_trades, wins };
    (result, out, new_peak)
}

fn run_sim_chandelier(
    sym_data: &HashMap<String, SymData>,
    symbols: &[String],
    train_end: usize, test_start: usize, test_end: usize,
    start_equity: f64, mut peak_equity: f64,
    chand_period: usize, chand_mult: f64,
) -> (WfResult, SimOut, f64) {
    let all_close: HashMap<&str, &[f64]> = sym_data.iter()
        .map(|(k, v)| (k.as_str(), v.close.as_slice())).collect();
    let all_vol: HashMap<&str, &[f64]> = sym_data.iter()
        .map(|(k, v)| (k.as_str(), v.vol.as_slice())).collect();

    let mut ad_eq = 1.0_f64; let mut mc_eq = 1.0_f64; let mut sm_eq = 1.0_f64;
    let mut ad_pk = 1.0_f64; let mut mc_pk = 1.0_f64; let mut sm_pk = 1.0_f64;

    let mut equity = start_equity;
    let mut equity_curve = vec![start_equity];
    let mut wins = 0usize;
    let mut total_trades = 0usize;

    let mut bar = test_start;
    while bar + 1 + HOLD_OTHER + 2 < test_end {
        let mut ad_scores: Vec<(&str, f64)> = Vec::new();
        let mut mc_scores: Vec<(&str, f64)> = Vec::new();
        let mut sm_scores: Vec<(&str, f64)> = Vec::new();

        for sym in symbols {
            let Some(sd) = sym_data.get(sym) else { continue };
            if bar >= sd.close.len() { continue; }

            if let Some(score) = ad_signal(&sd.close, &sd.high, &sd.low, &sd.vol, AD_PERIOD, train_end, bar) {
                ad_scores.push((sym.as_str(), score));
            }
            if let Some(score) = macd_signal(&sd.close, 12, 26, 9, train_end, bar) {
                mc_scores.push((sym.as_str(), score));
            }
            if let Some(score) = smallvol_rank(&sd.close, &sd.vol, &all_close, &all_vol, train_end, bar) {
                sm_scores.push((sym.as_str(), score));
            }
        }

        ad_scores.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());
        mc_scores.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());
        sm_scores.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());

        if ad_scores.is_empty() && mc_scores.is_empty() && sm_scores.is_empty() {
            bar += 1; continue;
        }

        let ad_dd = if ad_pk > 0.0 { (1.0 - ad_eq / ad_pk) * 100.0 } else { 0.0 };
        let mc_dd = if mc_pk > 0.0 { (1.0 - mc_eq / mc_pk) * 100.0 } else { 0.0 };
        let sm_dd = if sm_pk > 0.0 { (1.0 - sm_eq / sm_pk) * 100.0 } else { 0.0 };

        let ad_w0 = ddhard_exposure(ad_dd);
        let mc_w0 = ddhard_exposure(mc_dd);
        let sm_w0 = ddhard_exposure(sm_dd);
        let tot = (ad_w0 + mc_w0 + sm_w0).max(1e-9);
        let ad_w = ad_w0 / tot;
        let mc_w = mc_w0 / tot;
        let sm_w = sm_w0 / tot;

        // AD sleeve (Chandelier exit)
        let mut ad_rets = Vec::new();
        for &(sym, _) in ad_scores.iter() {
            let Some(sd) = sym_data.get(sym) else { continue };
            let n = sd.close.len();
            let entry_bar = bar + 1;
            let entry_price = sd.close.get(entry_bar).copied().unwrap_or(0.0);
            if entry_price <= 0.0 { continue; }

            let exit_bar = chandelier_exit(&sd.high, &sd.low, &sd.close, entry_bar, test_end, chand_period, chand_mult);
            let exit_price = sd.close.get(exit_bar).copied().unwrap_or(entry_price);
            let bars_held = exit_bar.saturating_sub(entry_bar);

            if exit_price > 0.0 && bars_held >= MIN_BARS_CHAND {
                let r = (exit_price / entry_price - 1.0) - 2.0 * TAKER_FEE;
                ad_rets.push(r);
                ad_pk = ad_pk.max(ad_eq * (1.0 + r));
                total_trades += 1;
                if r > 0.0 { wins += 1; }
            }
        }

        let mut port_ret = 0.0_f64;
        if !ad_rets.is_empty() {
            let avg = ad_rets.iter().sum::<f64>() / ad_rets.len() as f64;
            ad_eq *= 1.0 + avg;
            port_ret += ad_w * avg;
        }

        // MACD sleeve (fixed hold)
        let mut mc_rets = Vec::new();
        for &(sym, _) in mc_scores.iter() {
            let Some(sd) = sym_data.get(sym) else { continue };
            let entry = sd.close.get(bar + 1).copied().unwrap_or(0.0);
            let exit = sd.close.get(bar + 1 + HOLD_OTHER).copied().unwrap_or(0.0);
            if entry > 0.0 && exit > 0.0 {
                let r = (exit / entry - 1.0) - 2.0 * TAKER_FEE;
                mc_rets.push(r);
                mc_pk = mc_pk.max(mc_eq * (1.0 + r));
                total_trades += 1;
                if r > 0.0 { wins += 1; }
            }
        }
        if !mc_rets.is_empty() {
            let avg = mc_rets.iter().sum::<f64>() / mc_rets.len() as f64;
            mc_eq *= 1.0 + avg;
            port_ret += mc_w * avg;
        }

        // SmallVol sleeve (fixed hold)
        let mut sm_rets = Vec::new();
        for &(sym, _) in sm_scores.iter() {
            let Some(sd) = sym_data.get(sym) else { continue };
            let entry = sd.close.get(bar + 1).copied().unwrap_or(0.0);
            let exit = sd.close.get(bar + 1 + HOLD_OTHER).copied().unwrap_or(0.0);
            if entry > 0.0 && exit > 0.0 {
                let r = (exit / entry - 1.0) - 2.0 * TAKER_FEE;
                sm_rets.push(r);
                sm_pk = sm_pk.max(sm_eq * (1.0 + r));
                total_trades += 1;
                if r > 0.0 { wins += 1; }
            }
        }
        if !sm_rets.is_empty() {
            let avg = sm_rets.iter().sum::<f64>() / sm_rets.len() as f64;
            sm_eq *= 1.0 + avg;
            port_ret += sm_w * avg;
        }

        equity *= 1.0 + port_ret;
        equity_curve.push(equity);
        bar += HOLD_OTHER + 1;
    }

    let ret = (equity / start_equity - 1.0) * 100.0;
    let (max_dd, new_peak) = calc_max_dd_from(&equity_curve, peak_equity);
    let win_rate = if total_trades > 0 { wins as f64 / total_trades as f64 * 100.0 } else { 0.0 };
    let pass = total_trades >= MIN_TRADES && ret > 0.0;

    let result = WfResult { ret, sharpe: 0.0, max_dd, trades: total_trades, win_rate, pass, equity_curve: vec![] };
    let out = SimOut { equity, equity_curve, total_trades, wins };
    (result, out, new_peak)
}

// ── Main ──────────────────────────────────────────────────────────────────────

#[tokio::main]
async fn main() -> Result<()> {
    let t0 = Instant::now();

    // ── Build sweep grid ───────────────────────────────────────────────────────
    let mut periods: Vec<usize> = Vec::new();
    let mut p = CHAND_PERIOD_MIN;
    while p <= CHAND_PERIOD_MAX { periods.push(p); p += CHAND_PERIOD_STEP; }

    let mut multipliers: Vec<f64> = Vec::new();
    let mut m = CHAND_MULT_MIN;
    while m <= CHAND_MULT_MAX + 0.001 {
        multipliers.push((m * 100.0).round() / 100.0);
        m += CHAND_MULT_STEP;
    }

    println!("Chandelier Hyperopt: {} periods x {} mults = {} configs",
        periods.len(), multipliers.len(), periods.len() * multipliers.len());

    // ── Load data ──────────────────────────────────────────────────────────────
    let loader = DataLoader::new(None, None);
    let mut all_syms: std::collections::HashSet<String> = std::collections::HashSet::new();
    for (_, syms) in UNIVERSES {
        for &s in *syms { all_syms.insert(s.to_string()); }
    }

    let mut raw_cache: HashMap<String, DataFrame> = HashMap::new();
    let mut min_len = usize::MAX;

    for sym in all_syms.iter() {
        match loader.fetch_with_cache(sym, "1d", CANDLES).await {
            Ok(df) => {
                let n = df.height();
                min_len = min_len.min(n);
                raw_cache.insert(sym.clone(), df);
            }
            Err(e) => { eprintln!("WARNING: {} load failed: {}", sym, e); }
        }
    }

    let n = min_len.min(2800);
    let mut sym_data_map: HashMap<String, SymData> = HashMap::new();
    for sym in &all_syms {
        if let Some(df) = raw_cache.get(sym) {
            let n_min = df.height().min(n);
            macro_rules! col_vec {
                ($name:expr) => {{
                    let chunked = df.column($name)?.f64()?;
                    chunked.into_iter().filter_map(|x| x).take(n_min).collect::<Vec<_>>()
                }};
            }
            sym_data_map.insert(sym.clone(), SymData {
                close: col_vec!("close"),
                open: col_vec!("open"),
                high: col_vec!("high"),
                low: col_vec!("low"),
                vol: col_vec!("volume"),
            });
        }
    }
    println!("Loaded {} syms, {} bars\n", sym_data_map.len(), n);

    #[derive(Clone)]
    struct ConfigResult {
        period: usize,
        mult: f64,
        label: String,
        is_baseline: bool,
        avg_sharpe: f64,
        avg_return: f64,
        avg_dd: f64,
        pass_rate: f64,
        total_trades: usize,
        win_rate: f64,
        equity_curves: HashMap<String, Vec<f64>>,
    }

    let mut all_results: Vec<ConfigResult> = Vec::new();

    // ── Baseline Fixed54 ────────────────────────────────────────────────────────
    println!("Running baseline Fixed54...");
    let mut baseline_returns = Vec::new();
    let mut baseline_pass = 0usize;
    let mut baseline_total = 0usize;
    let mut baseline_trades = 0usize;
    let mut baseline_wins = 0usize;
    let mut baseline_curves: HashMap<String, Vec<f64>> = HashMap::new();

    for &(label, symbols) in UNIVERSES {
        let symbols: Vec<String> = symbols.iter().map(|s| s.to_string()).collect();
        if !symbols.iter().all(|s| sym_data_map.contains_key(s)) { continue; }

        let total_windows = n.saturating_sub(TRAIN_BARS + TEST_BARS) / TEST_BARS;
        if total_windows == 0 { continue; }

        let mut eq_u = 1.0_f64;
        let mut pk_u = 1.0_f64;

        for wi in 0..total_windows {
            let train_end = TRAIN_BARS + wi * TEST_BARS;
            let test_start = train_end;
            let test_end = (test_start + TEST_BARS).min(n);
            if test_end.saturating_sub(test_start) < HOLD_FIXED + 5 { continue; }

            let (result, out, _pk_u) = run_sim_fixed(&sym_data_map, &symbols, train_end, test_start, test_end, eq_u, pk_u);
            eq_u = out.equity;
            baseline_total += 1;
            if result.pass { baseline_pass += 1; }
            baseline_trades += result.trades;
            baseline_wins += out.wins;
            baseline_returns.push(result.ret);

            if !out.equity_curve.is_empty() {
                let start_eq = out.equity_curve[0];
                let norm: Vec<f64> = out.equity_curve.iter().map(|&e| e / start_eq).collect();
                baseline_curves.insert(format!("{}_W{}", label, wi), norm);
            }
        }
    }

    let baseline_avg_ret = if !baseline_returns.is_empty() {
        baseline_returns.iter().sum::<f64>() / baseline_returns.len() as f64
    } else { 0.0 };
    let baseline_agg_dd = baseline_returns.iter().fold(0.0_f64, |a, &r| a.max(-r));
    let baseline_avg_sharpe = if !baseline_returns.is_empty() {
        baseline_avg_ret / baseline_agg_dd.max(1.0)
    } else { 0.0 };
    let baseline_pass_rate = if baseline_total > 0 { baseline_pass as f64 / baseline_total as f64 } else { 0.0 };
    let baseline_win_rate = if baseline_trades > 0 { baseline_wins as f64 / baseline_trades as f64 * 100.0 } else { 0.0 };

    all_results.push(ConfigResult {
        period: 54, mult: 0.0,
        label: "Baseline_Fixed54".to_string(),
        is_baseline: true,
        avg_sharpe: baseline_avg_sharpe,
        avg_return: baseline_avg_ret,
        avg_dd: baseline_agg_dd,
        pass_rate: baseline_pass_rate,
        total_trades: baseline_trades,
        win_rate: baseline_win_rate,
        equity_curves: baseline_curves,
    });
    println!("  Fixed54: Ret={:.1}%, Sharpe={:.3}, DD={:.1}%, Pass={:.0}%, Trades={}",
        baseline_avg_ret, baseline_avg_sharpe, baseline_agg_dd, baseline_pass_rate * 100.0, baseline_trades);

    // ── Chandelier Sweep ───────────────────────────────────────────────────────
    let total_configs = periods.len() * multipliers.len();
    let mut config_idx = 0usize;

    for &chand_p in &periods {
        for &chand_m in &multipliers {
            config_idx += 1;
            if config_idx % 50 == 0 || config_idx == total_configs {
                println!("  [{}/{}] period={}, mult={:.2}", config_idx, total_configs, chand_p, chand_m);
            }

            let mut cfg_returns = Vec::new();
            let mut cfg_pass = 0usize;
            let mut cfg_total = 0usize;
            let mut cfg_trades = 0usize;
            let mut cfg_wins = 0usize;
            let mut cfg_curves: HashMap<String, Vec<f64>> = HashMap::new();

            for &(label, symbols) in UNIVERSES {
                let symbols: Vec<String> = symbols.iter().map(|s| s.to_string()).collect();
                if !symbols.iter().all(|s| sym_data_map.contains_key(s)) { continue; }

                let total_windows = n.saturating_sub(TRAIN_BARS + TEST_BARS) / TEST_BARS;
                if total_windows == 0 { continue; }

                let mut eq_u = 1.0_f64;
                let mut pk_u = 1.0_f64;

                for wi in 0..total_windows {
                    let train_end = TRAIN_BARS + wi * TEST_BARS;
                    let test_start = train_end;
                    let test_end = (test_start + TEST_BARS).min(n);
                    if test_end.saturating_sub(test_start) < 10 { continue; }

                    let (result, out, _pk_u) = run_sim_chandelier(
                        &sym_data_map, &symbols, train_end, test_start, test_end,
                        eq_u, pk_u, chand_p, chand_m,
                    );
                    eq_u = out.equity;
                    cfg_total += 1;
                    if result.pass { cfg_pass += 1; }
                    cfg_trades += result.trades;
                    cfg_wins += out.wins;
                    cfg_returns.push(result.ret);

                    if !out.equity_curve.is_empty() {
                        let start_eq = out.equity_curve[0];
                        let norm: Vec<f64> = out.equity_curve.iter().map(|&e| e / start_eq).collect();
                        cfg_curves.insert(format!("{}_W{}", label, wi), norm);
                    }
                }
            }

            let cfg_avg_ret = if !cfg_returns.is_empty() { cfg_returns.iter().sum::<f64>() / cfg_returns.len() as f64 } else { 0.0 };
            let cfg_agg_dd = cfg_returns.iter().fold(0.0_f64, |a, &r| a.max(-r));
            let cfg_avg_sharpe = if !cfg_returns.is_empty() { cfg_avg_ret / cfg_agg_dd.max(1.0) } else { 0.0 };
            let cfg_pass_rate = if cfg_total > 0 { cfg_pass as f64 / cfg_total as f64 } else { 0.0 };
            let cfg_win_rate = if cfg_trades > 0 { cfg_wins as f64 / cfg_trades as f64 * 100.0 } else { 0.0 };

            all_results.push(ConfigResult {
                period: chand_p, mult: chand_m,
                label: format!("Chandelier_p{}_m{:.2}", chand_p, chand_m),
                is_baseline: false,
                avg_sharpe: cfg_avg_sharpe,
                avg_return: cfg_avg_ret,
                avg_dd: cfg_agg_dd,
                pass_rate: cfg_pass_rate,
                total_trades: cfg_trades,
                win_rate: cfg_win_rate,
                equity_curves: cfg_curves,
            });
        }
    }

    // ── Rank ───────────────────────────────────────────────────────────────────
    all_results.sort_by(|a, b| b.avg_sharpe.partial_cmp(&a.avg_sharpe).unwrap());

    println!("\nTOP 10 CONFIGURATIONS:");
    for (i, r) in all_results.iter().take(10).enumerate() {
        let lbl = if r.is_baseline { "Baseline(Fixed54)" } else { r.label.as_str() };
        println!("  #{:2} {:32} Ret={:+8.1} Sharpe={:.3} DD={:6.1} Pass={:4.0} Trades={:5}",
            i + 1, lbl, r.avg_return, r.avg_sharpe, r.avg_dd, r.pass_rate * 100.0, r.total_trades);
    }

    // ── Write sweep CSV ────────────────────────────────────────────────────────
    let sweep_csv = "snapshots/chandelier_hyperopt_sweep.csv";
    let mut csv_f = OpenOptions::new().create(true).write(true).truncate(true).open(sweep_csv)?;
    writeln!(csv_f, "rank,label,period,mult,is_baseline,avg_return_pct,avg_sharpe,avg_dd_pct,pass_rate,total_trades,win_rate")?;
    for (i, r) in all_results.iter().enumerate() {
        writeln!(csv_f, "{},{},{},{:.2},{},{:.2},{:.4},{:.2},{:.4},{},{:.2}",
            i + 1, r.label, r.period, r.mult, r.is_baseline,
            r.avg_return, r.avg_sharpe, r.avg_dd, r.pass_rate, r.total_trades, r.win_rate)?;
    }
    println!("\nSweep CSV: {}", sweep_csv);

    // ── Export equity curves for Baseline + Top 3 ─────────────────────────────
    for cfg in all_results.iter().take(4) {
        let mut univ_curves: HashMap<String, Vec<f64>> = HashMap::new();
        for (key, eq) in &cfg.equity_curves {
            let parts: Vec<&str> = key.split("_W").collect();
            if parts.len() != 2 { continue; }
            let univ = parts[0];
            univ_curves
                .entry(univ.to_string())
                .or_insert_with(Vec::new)
                .extend(eq.iter().cloned());
        }

        for (univ, combined_eq) in &univ_curves {
            let safe_label = cfg.label.replace(' ', "_").replace('.', "_").replace('-', "_");
            let filename = format!("snapshots/chandelier_equity_{}_{}.csv", safe_label, univ);
            let mut f = OpenOptions::new().create(true).write(true).truncate(true).open(&filename)?;
            writeln!(f, "step,equity")?;
            for (i, &e) in combined_eq.iter().enumerate() {
                writeln!(f, "{},{:.6}", i, e)?;
            }
        }

        // Combined all-universe
        let all_eq: Vec<f64> = univ_curves.values().flat_map(|v| v.iter().cloned()).collect();
        if !all_eq.is_empty() {
            let safe_label = cfg.label.replace(' ', "_").replace('.', "_").replace('-', "_");
            let filename = format!("snapshots/chandelier_equity_{}_ALL.csv", safe_label);
            let mut f = OpenOptions::new().create(true).write(true).truncate(true).open(&filename)?;
            writeln!(f, "step,equity")?;
            for (i, &e) in all_eq.iter().enumerate() {
                writeln!(f, "{},{:.6}", i, e)?;
            }
        }
        println!("Exported equity curves for: {}", cfg.label);
    }

    // ── Write summary ──────────────────────────────────────────────────────────
    let best = all_results.first();
    let best_label = best.map(|b| b.label.clone()).unwrap_or_default();
    let best_p = best.map(|b| b.period).unwrap_or(0);
    let best_m = best.map(|b| b.mult).unwrap_or(0.0);
    let best_ret = best.map(|b| b.avg_return).unwrap_or(0.0);
    let best_sh = best.map(|b| b.avg_sharpe).unwrap_or(0.0);
    let best_dd = best.map(|b| b.avg_dd).unwrap_or(0.0);
    let best_pass = best.map(|b| b.pass_rate).unwrap_or(0.0);
    let beat_baseline = best.filter(|b| !b.is_baseline).map(|b| b.avg_sharpe > baseline_avg_sharpe).unwrap_or(false);

    let mut summary = format!(
        "## Chandelier Hyperopt Summary (2026-04-10)\n\n\
        **Grid:** {} periods ({}..{}, step {}) x {} multipliers ({:.2}..{:.2}, step {:.2})\n\
        **Baseline (Fixed54):** Ret={:.1}%, Sharpe={:.3}, DD={:.1}%, Pass={:.0}%, Trades={}\n\
        **Winner:** {} (period={}, mult={:.2})\n\
        **  Ret={:.1}%, Sharpe={:.3}, DD={:.1}%, Pass={:.0}%\n\
        **Beat Baseline:** {}\n\n\
        **Top 5:**\n",
        periods.len(), CHAND_PERIOD_MIN, CHAND_PERIOD_MAX, CHAND_PERIOD_STEP,
        multipliers.len(), CHAND_MULT_MIN, CHAND_MULT_MAX, CHAND_MULT_STEP,
        baseline_avg_ret, baseline_avg_sharpe, baseline_agg_dd, baseline_pass_rate * 100.0, baseline_trades,
        best_label, best_p, best_m,
        best_ret, best_sh, best_dd, best_pass * 100.0,
        if beat_baseline { "YES" } else { "NO" },
    );

    for (i, r) in all_results.iter().take(5).enumerate() {
        let lbl = if r.is_baseline { "Baseline(Fixed54)".to_string() } else { r.label.clone() };
        summary.push_str(&format!(
            "{:2}. {:32} | Ret={:+7.1} | Sharpe={:.3} | DD={:5.1} | Pass={:4.0}\n",
            i + 1, lbl, r.avg_return, r.avg_sharpe, r.avg_dd, r.pass_rate * 100.0
        ));
    }

    let summary_path = "snapshots/chandelier_hyperopt_summary.md";
    std::fs::write(summary_path, &summary)?;
    println!("Summary: {}", summary_path);
    println!("\n{}", summary);
    println!("Runtime: {:.1}s\n", t0.elapsed().as_secs_f64());

    Ok(())
}
