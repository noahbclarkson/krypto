//! USDT Hedge Overlay + Slippage Sensitivity Sweep (Track A: Trust the Lab)
//!
//! PURPOSE: Test how the DDBudget 3-sleeve strategy responds to:
//!   1. USDT hedge overlay (reduce position when BTC 21d-vol > 75th pct of 252d history)
//!   2. Execution slippage (0, 5, 10 bps per trade on top of 0.1% taker fee)
//!
//! Configurations tested:
//!   A) BASELINE — no hedge, 0 bps slippage (current benchmark)
//!   B) HEDGE_ONLY — 70% position when BTC vol > 75th pct, 0 bps slippage
//!   C) SLIPPAGE — no hedge, 5 bps slippage
//!   D) HEDGE+SLIP — 70% position + 5 bps slippage
//!
//! The hedge logic: when BTC's 21-bar realized vol ranks above the 75th percentile
//! of the prior 252 bars, reduce ALL sleeve positions to 70% (30% sits in USDT at 0%).
//! This should reduce exposure during prolonged bear markets (W05 FTX) where BTC vol
//! is persistently elevated.
//!
//! Walk-forward: 252/252 across all 9 universes.

use anyhow::Result;
use krypto::data::loader::DataLoader;
use polars::prelude::*;
use std::collections::HashMap;
use std::io::Write;
use std::time::Instant;

// ── Config ────────────────────────────────────────────────────────────────────

const CANDLES: u32 = 3000;
const TRAIN_BARS: usize = 252;
const TEST_BARS: usize = 252;
const AD_PERIOD: usize = 5; // hyperopt winner 2026-04-13 (was 47)
const HOLD_OTHER: usize = 21;
const MIN_BARS_CHAND: usize = 5;
const CHAND_PERIOD: usize = 28;
const CHAND_MULT: f64 = 2.00;
const TAKER_FEE: f64 = 0.001;
const POSITION_CAP: usize = 2;
const WARMUP: usize = 200;
const MIN_TRADES: usize = 3;
const VOL_WINDOW: usize = 21;
const VOL_HIST: usize = 252;
const VOL_PCT_THRESHOLD: f64 = 0.75;
const HEDGE_RATIO: f64 = 0.70;

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

// ── Mode ──────────────────────────────────────────────────────────────────────

#[derive(Clone, Copy, PartialEq)]
enum Mode {
    Baseline,
    HedgeOnly,
    Slippage5bps,
    HedgeSlippage,
}

impl Mode {
    fn label(&self) -> &'static str {
        match self {
            Mode::Baseline => "BASE",
            Mode::HedgeOnly => "HEDGE",
            Mode::Slippage5bps => "SLIP5",
            Mode::HedgeSlippage => "H+S",
        }
    }
    fn hedge(&self) -> bool {
        matches!(self, Mode::HedgeOnly | Mode::HedgeSlippage)
    }
    fn slippage_bps(&self) -> f64 {
        match self {
            Mode::Slippage5bps | Mode::HedgeSlippage => 0.0005,
            _ => 0.0,
        }
    }
}

// ── Data struct ───────────────────────────────────────────────────────────────

struct SymData {
    close: Vec<f64>,
    high: Vec<f64>,
    low: Vec<f64>,
    vol: Vec<f64>,
}

// ── Math helpers (identical to DDBudget walkforward) ──────────────────────────

fn ddhard_exposure(dd_pct: f64) -> f64 {
    if dd_pct > 30.0 { 0.30 } else if dd_pct > 15.0 { 0.60 } else { 1.0 }
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
    exit_bar
}

// ── Signal functions (identical to DDBudget walkforward) ─────────────────────

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
        let c = close[j]; let h = high[j]; let l = low[j]; let v = vol[j];
        let range = h - l;
        let mul = if range > 1e-9 { ((c - l) - (h - c)) / range } else { 0.0 };
        ema1 = alpha1 * mul * v + (1.0 - alpha1) * ema1;
        ema2 = alpha2 * mul * v + (1.0 - alpha2) * ema2;
    }
    let ad_now = ema1 - ema2;
    let mut sum = 0.0_f64;
    let mut cnt = 0usize;
    for j in warmup..train_end {
        let c = close[j]; let h = high[j]; let l = low[j]; let v = vol[j];
        let range = h - l;
        let mul = if range > 1e-9 { ((c - l) - (h - c)) / range } else { 0.0 };
        let ea1 = alpha1 * mul * v + (1.0 - alpha1) * 0.0;
        let ea2 = alpha2 * mul * v + (1.0 - alpha2) * 0.0;
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
    let mut ef = 0.0_f64; let mut es = 0.0_f64;
    let mut macd_line = 0.0_f64; let mut sig_line = 0.0_f64;
    for j in warmup..=bar.min(train_end) {
        ef = ef_alp * close[j] + (1.0 - ef_alp) * ef;
        es = es_alp * close[j] + (1.0 - es_alp) * es;
        let ml = ef - es;
        sig_line = em_alp * ml + (1.0 - em_alp) * sig_line;
        macd_line = ml;
    }
    let sma_start = bar.saturating_sub(200).max(warmup);
    let mut sma_sum = 0.0_f64;
    let mut sma_cnt = 0usize;
    for j in sma_start..=bar.min(train_end) {
        sma_sum += close[j]; sma_cnt += 1;
    }
    let sma200 = if sma_cnt > 0 { sma_sum / sma_cnt as f64 } else { close[bar] };
    let regime = if close[bar] > sma200 { 1.0 } else { -1.0 };
    Some(regime * (macd_line - sig_line))
}

fn smallvol_rank(
    close: &[f64], vol: &[f64],
    all_close: &HashMap<&str, &[f64]>, all_vol: &HashMap<&str, &[f64]>,
    train_end: usize, bar: usize,
) -> Option<f64> {
    if bar < WARMUP || bar > train_end { return None; }
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

// ── Vol percentile computation ────────────────────────────────────────────────

/// Compute BTC 21-bar realized vol percentile vs 252-bar history at each bar.
/// Returns vec where pct[i] = fraction of past vol_window values ≤ current vol.
fn compute_btc_vol_pct(btc_close: &[f64]) -> Vec<f64> {
    let n = btc_close.len();
    let mut pct = vec![0.5; n];
    for i in VOL_HIST..n {
        // Current 21-bar realized vol
        let mut cur_sq = 0.0_f64;
        for j in i.saturating_sub(VOL_WINDOW)..i {
            if j > 0 && btc_close[j - 1] > 0.0 {
                let r = (btc_close[j] - btc_close[j - 1]) / btc_close[j - 1];
                cur_sq += r * r;
            }
        }
        let cur_vol = (cur_sq / VOL_WINDOW as f64).sqrt();

        // Historical vol distribution (252 windows of 21-bar vol)
        let mut hist: Vec<f64> = Vec::with_capacity(VOL_HIST);
        for w in i.saturating_sub(VOL_HIST)..i.saturating_sub(VOL_WINDOW) {
            let mut sq = 0.0_f64;
            for j in w..(w + VOL_WINDOW).min(n) {
                if j > 0 && btc_close[j - 1] > 0.0 {
                    let r = (btc_close[j] - btc_close[j - 1]) / btc_close[j - 1];
                    sq += r * r;
                }
            }
            hist.push((sq / VOL_WINDOW as f64).sqrt());
        }
        if !hist.is_empty() {
            let count_below = hist.iter().filter(|&&v| v <= cur_vol).count();
            pct[i] = count_below as f64 / hist.len() as f64;
        }
    }
    pct
}

// ── Per-window result ─────────────────────────────────────────────────────────

#[derive(Clone, Copy)]
struct WfResult {
    ret: f64,
    max_dd: f64,
    trades: usize,
    pass: bool,
}

// ── Core simulation ───────────────────────────────────────────────────────────

fn run_sim(
    sym_data: &HashMap<String, SymData>,
    symbols: &[String],
    btc_vol_pct: &[f64],
    train_end: usize,
    test_start: usize,
    test_end: usize,
    mode: Mode,
    start_equity: f64,
    peak_equity: f64,
) -> (WfResult, f64, f64) {
    let all_close: HashMap<&str, &[f64]> = sym_data.iter().map(|(k, v)| (k.as_str(), v.close.as_slice())).collect();
    let all_vol_map: HashMap<&str, &[f64]> = sym_data.iter().map(|(k, v)| (k.as_str(), v.vol.as_slice())).collect();

    let mut ad_eq = 1.0_f64; let mut mc_eq = 1.0_f64; let mut sm_eq = 1.0_f64;
    let mut ad_pk = 1.0_f64; let mut mc_pk = 1.0_f64; let mut sm_pk = 1.0_f64;

    let mut equity = start_equity;
    let mut equity_curve = vec![start_equity];
    let mut total_trades = 0usize;
    let slippage = mode.slippage_bps();

    let mut bar = test_start;
    while bar + HOLD_OTHER + 2 < test_end {
        // ── BTC vol hedge check ────────────────────────────────────────────
        let vol_pct = btc_vol_pct.get(bar).copied().unwrap_or(0.5);
        let hedge_mult = if mode.hedge() && vol_pct > VOL_PCT_THRESHOLD {
            HEDGE_RATIO
        } else {
            1.0
        };

        // ── Compute signals per sleeve ─────────────────────────────────────
        let mut ad_scores: Vec<(&str, f64)> = Vec::new();
        let mut mc_scores: Vec<(&str, f64)> = Vec::new();
        let mut sm_scores: Vec<(&str, f64)> = Vec::new();

        for sym in symbols {
            let Some(sd) = sym_data.get(sym) else { continue };
            if bar >= sd.close.len() { continue; }
            if let Some(s) = ad_signal(&sd.close, &sd.high, &sd.low, &sd.vol, AD_PERIOD, train_end, bar) {
                ad_scores.push((sym.as_str(), s));
            }
            if let Some(s) = macd_signal(&sd.close, 12, 26, 9, train_end, bar) {
                mc_scores.push((sym.as_str(), s));
            }
            if let Some(s) = smallvol_rank(&sd.close, &sd.vol, &all_close, &all_vol_map, train_end, bar) {
                sm_scores.push((sym.as_str(), s));
            }
        }

        ad_scores.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());
        mc_scores.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());
        sm_scores.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());

        let ad_top: Vec<(&str, f64)> = ad_scores.iter().take(POSITION_CAP).cloned().collect();
        let mc_top: Vec<(&str, f64)> = mc_scores.iter().take(POSITION_CAP).cloned().collect();
        let sm_top: Vec<(&str, f64)> = sm_scores.iter().take(POSITION_CAP).cloned().collect();

        if ad_top.is_empty() && mc_top.is_empty() && sm_top.is_empty() {
            bar += 1;
            continue;
        }

        // ── Sleeve DD-hard weights ─────────────────────────────────────────
        let ad_dd = if ad_pk > 0.0 { (1.0 - ad_eq / ad_pk) * 100.0 } else { 0.0 };
        let mc_dd = if mc_pk > 0.0 { (1.0 - mc_eq / mc_pk) * 100.0 } else { 0.0 };
        let sm_dd = if sm_pk > 0.0 { (1.0 - sm_eq / sm_pk) * 100.0 } else { 0.0 };
        let ad_w = ddhard_exposure(ad_dd);
        let mc_w = ddhard_exposure(mc_dd);
        let sm_w = ddhard_exposure(sm_dd);
        let tot_w = ad_w + mc_w + sm_w;
        let ad_w = ad_w / tot_w.max(1e-9);
        let mc_w = mc_w / tot_w.max(1e-9);
        let sm_w = sm_w / tot_w.max(1e-9);

        // ── Execute trades ─────────────────────────────────────────────────
        let mut port_ret = 0.0_f64;
        let mut max_ad_hold = 0usize;

        // AD sleeve (Chandelier exit)
        let mut ad_rets = Vec::new();
        for (sym, _) in ad_top {
            let Some(sd) = sym_data.get(sym) else { continue };
            let entry_bar = bar + 1;
            let entry = sd.close.get(entry_bar).copied().unwrap_or(0.0);
            let exit_bar = chandelier_exit(&sd.high, &sd.low, &sd.close, entry_bar, test_end, CHAND_PERIOD, CHAND_MULT);
            let exit = sd.close.get(exit_bar).copied().unwrap_or(entry);
            if entry > 0.0 && exit > 0.0 && exit_bar > entry_bar {
                let r = (exit / entry - 1.0) - 2.0 * TAKER_FEE - 2.0 * slippage;
                ad_rets.push(r);
                ad_pk = ad_pk.max(ad_eq * (1.0 + r));
                total_trades += 1;
                max_ad_hold = max_ad_hold.max(exit_bar - bar);
            }
        }
        if !ad_rets.is_empty() {
            let avg = ad_rets.iter().sum::<f64>() / ad_rets.len() as f64;
            ad_eq *= 1.0 + avg;
            port_ret += ad_w * avg * hedge_mult;
        }

        // MACD sleeve (fixed hold)
        let mut mc_rets = Vec::new();
        for (sym, _) in mc_top {
            let Some(sd) = sym_data.get(sym) else { continue };
            let entry = sd.close.get(bar + 1).copied().unwrap_or(0.0);
            let exit = sd.close.get(bar + 1 + HOLD_OTHER).copied().unwrap_or(0.0);
            if entry > 0.0 && exit > 0.0 {
                let r = (exit / entry - 1.0) - 2.0 * TAKER_FEE - 2.0 * slippage;
                mc_rets.push(r);
                mc_pk = mc_pk.max(mc_eq * (1.0 + r));
                total_trades += 1;
            }
        }
        if !mc_rets.is_empty() {
            let avg = mc_rets.iter().sum::<f64>() / mc_rets.len() as f64;
            mc_eq *= 1.0 + avg;
            port_ret += mc_w * avg * hedge_mult;
        }

        // Small vol sleeve (fixed hold)
        let mut sm_rets = Vec::new();
        for (sym, _) in sm_top {
            let Some(sd) = sym_data.get(sym) else { continue };
            let entry = sd.close.get(bar + 1).copied().unwrap_or(0.0);
            let exit = sd.close.get(bar + 1 + HOLD_OTHER).copied().unwrap_or(0.0);
            if entry > 0.0 && exit > 0.0 {
                let r = (exit / entry - 1.0) - 2.0 * TAKER_FEE - 2.0 * slippage;
                sm_rets.push(r);
                sm_pk = sm_pk.max(sm_eq * (1.0 + r));
                total_trades += 1;
            }
        }
        if !sm_rets.is_empty() {
            let avg = sm_rets.iter().sum::<f64>() / sm_rets.len() as f64;
            sm_eq *= 1.0 + avg;
            port_ret += sm_w * avg * hedge_mult;
        }

        equity *= 1.0 + port_ret;
        equity_curve.push(equity);
        bar += max_ad_hold.max(HOLD_OTHER).max(1);
    }

    let ret = (equity / start_equity - 1.0) * 100.0;
    let mut max_dd = 0.0_f64;
    let mut current_peak = peak_equity;
    for &e in &equity_curve {
        if e > current_peak { current_peak = e; }
        let dd = (1.0 - e / current_peak) * 100.0;
        max_dd = max_dd.max(dd);
    }

    (
        WfResult { ret, max_dd, trades: total_trades, pass: total_trades >= MIN_TRADES && ret > 0.0 },
        equity,
        current_peak,
    )
}

// ── Main ──────────────────────────────────────────────────────────────────────

#[tokio::main]
async fn main() -> Result<()> {
    let t0 = Instant::now();
    println!("═══ USDT Hedge + Slippage Sensitivity Sweep ═══\n");
    println!("Track A: Trust the Lab");
    println!("Config: DDBudget 3-sleeve, Chandelier({}, {}), 252/252 walk-forward", CHAND_PERIOD, CHAND_MULT);
    println!("Modes: BASE | HEDGE (vol>75th→{}%) | SLIP5 (5bps) | H+S", (HEDGE_RATIO * 100.0) as u8);
    println!("");

    // Load data
    let loader = DataLoader::new(None, None);
    let mut all_syms: std::collections::HashSet<String> = std::collections::HashSet::new();
    for (_, syms) in UNIVERSES {
        for &s in *syms { all_syms.insert(s.to_string()); }
    }

    let mut raw_cache: HashMap<String, DataFrame> = HashMap::new();
    let mut min_len = usize::MAX;
    for sym in all_syms.iter() {
        match loader.fetch_with_cache(sym.as_str(), "1d", CANDLES).await {
            Ok(df) => { min_len = min_len.min(df.height()); raw_cache.insert(sym.clone(), df); }
            Err(e) => eprintln!("  WARN: {} load failed: {}", sym, e),
        }
    }

    let n = min_len.min(2800);
    let mut sym_data_map: HashMap<String, SymData> = HashMap::new();
    for sym in &all_syms {
        if let Some(df) = raw_cache.get(sym) {
            let n_min = df.height().min(n);
            let extract = |name: &str| -> Vec<f64> {
                df.column(name).unwrap().f64().unwrap().into_iter().filter_map(|x| x).take(n_min).collect()
            };
            sym_data_map.insert(sym.clone(), SymData {
                close: extract("close"), high: extract("high"), low: extract("low"), vol: extract("volume"),
            });
        }
    }

    // Compute BTC vol percentile (global signal)
    let btc_close = sym_data_map.get("BTCUSDT").map(|sd| sd.close.clone()).unwrap_or_default();
    let btc_vol_pct = compute_btc_vol_pct(&btc_close);
    println!("Loaded {} symbols, {} bars, BTC vol pct computed\n", sym_data_map.len(), n);

    // Run per-universe walk-forward across all modes
    let modes = [Mode::Baseline, Mode::HedgeOnly, Mode::Slippage5bps, Mode::HedgeSlippage];
    let mut csv_lines = vec!["universe,window,mode,return_pct,max_dd_pct,trades,pass".to_string()];
    let mut global_stats: HashMap<String, (usize, usize, f64, f64)> = HashMap::new(); // mode → (pass, total, avg_ret, avg_dd)

    for &(label, symbols) in UNIVERSES {
        let symbols: Vec<String> = symbols.iter().map(|s| s.to_string()).collect();
        if !symbols.iter().all(|s| sym_data_map.contains_key(s)) { continue; }

        let total_windows = n.saturating_sub(TRAIN_BARS + TEST_BARS) / TEST_BARS;
        if total_windows == 0 { continue; }

        println!("═══ {:<18} ═══ {} syms, {} windows", label, symbols.len(), total_windows);
        println!("{:<4} {:>5} | {:>9} {:>7} {:>4} {:>3} | {:>9} {:>7} {:>4} {:>3} | {:>9} {:>7} {:>4} {:>3} | {:>9} {:>7} {:>4} {:>3}",
            "W", "bar", "BASE%", "DD%", "trd", "P", "HEDGE%", "DD%", "trd", "P", "SLIP5%", "DD%", "trd", "P", "H+S%", "DD%", "trd", "P");
        println!("{}", "-".repeat(130));

        let mut window_results: Vec<[Option<WfResult>; 4]> = Vec::new();

        for wi in 0..total_windows {
            let train_end = TRAIN_BARS + wi * TEST_BARS;
            let test_start = train_end;
            let test_end = (test_start + TEST_BARS).min(n);
            if test_end.saturating_sub(test_start) < HOLD_OTHER + 5 { continue; }

            let mut results: [Option<WfResult>; 4] = [None; 4];
            // We need to chain equity for each mode independently
            let mut mode_equity = [1.0_f64; 4];
            let mut mode_peak = [1.0_f64; 4];

            for (mi, &mode) in modes.iter().enumerate() {
                // For chaining, we need to run modes independently
                // Since run_sim chains equity, we need separate state per mode
                // Hack: run each mode independently (don't chain across windows for simplicity in comparison)
                let (r, eq, pk) = run_sim(
                    &sym_data_map, &symbols, &btc_vol_pct,
                    train_end, test_start, test_end,
                    mode, 1.0, 1.0,
                );
                results[mi] = Some(r);
                mode_equity[mi] = eq;
                mode_peak[mi] = pk;
            }

            let mut row_str = format!("{:<4} {:>5} |", format!("W{:02}", wi), test_start);
            for (mi, &mode) in modes.iter().enumerate() {
                if let Some(ref r) = results[mi] {
                    let mark = if r.pass { "✅" } else { "❌" };
                    row_str.push_str(&format!(" {:>+9.1} {:>7.1} {:>4} {:>3} |",
                        r.ret, -r.max_dd, r.trades, mark));
                    csv_lines.push(format!("{},{},{},{:.2},{:.2},{},{}",
                        label, wi, mode.label(), r.ret, r.max_dd, r.trades,
                        if r.pass { "PASS" } else { "FAIL" }));
                }
            }
            println!("{}", row_str);
            window_results.push(results);
        }

        // Universe summary
        for (mi, &mode) in modes.iter().enumerate() {
            let results: Vec<_> = window_results.iter().filter_map(|r| r[mi].as_ref()).collect();
            if results.is_empty() { continue; }
            let pass_count = results.iter().filter(|r| r.pass).count();
            let total = results.len();
            let avg_ret = results.iter().map(|r| r.ret).sum::<f64>() / total as f64;
            let avg_dd = results.iter().map(|r| r.max_dd).sum::<f64>() / total as f64;
            let key = format!("{}_{}", label, mode.label());
            global_stats.insert(key, (pass_count, total, avg_ret, avg_dd));
        }
        println!("");
    }

    // ── Global comparison ──────────────────────────────────────────────────
    println!("\n═══ GLOBAL COMPARISON ═══");
    println!("{:<20} {:>5} {:>5} {:>10} {:>10} {:>10}", "Config", "Pass", "Total", "Rate%", "AvgRet%", "AvgDD%");
    println!("{}", "-".repeat(65));

    for &mode in &modes {
        let mut pass = 0usize; let mut total = 0usize;
        let mut sum_ret = 0.0_f64; let mut sum_dd = 0.0_f64;
        for (key, &(p, t, r, d)) in &global_stats {
            if key.ends_with(mode.label()) {
                pass += p; total += t; sum_ret += r; sum_dd += d;
            }
        }
        if total > 0 {
            println!("{:<20} {:>5} {:>5} {:>9.0}% {:>+10.1} {:>10.1}",
                mode.label(), pass, total, pass as f64 / total as f64 * 100.0,
                sum_ret / total as f64, -(sum_dd / total as f64));
        }
    }

    // ── Bear window analysis ───────────────────────────────────────────────
    println!("\n═══ BEAR WINDOW ANALYSIS (W01, W05) ═══");
    println!("{:<20} {:>12} {:>12} {:>12} {:>12}", "Config", "W01 ret%", "W01 DD%", "W05 ret%", "W05 DD%");
    println!("{}", "-".repeat(70));
    // W01 = window index 0 or 1, W05 ≈ window 4 (depends on universe)
    // We'll look at Base5 results specifically
    for &mode in &modes {
        let key_prefix = format!("Base5_{}", mode.label());
        // Find W01 (wi=0) and W05 (wi=4) from CSV lines
        let mut w01_ret = 0.0_f64; let mut w01_dd = 0.0_f64;
        let mut w05_ret = 0.0_f64; let mut w05_dd = 0.0_f64;
        for line in &csv_lines {
            let parts: Vec<&str> = line.split(',').collect();
            if parts.len() >= 7 && parts[0] == "Base5" && parts[3] == mode.label() {
                match parts[1] {
                    "0" => { w01_ret = parts[4].parse::<f64>().unwrap_or(0.0); w01_dd = parts[5].parse::<f64>().unwrap_or(0.0); }
                    "4" => { w05_ret = parts[4].parse::<f64>().unwrap_or(0.0); w05_dd = parts[5].parse::<f64>().unwrap_or(0.0); }
                    _ => {}
                }
            }
        }
        println!("{:<20} {:>+12.1} {:>12.1} {:>+12.1} {:>12.1}",
            mode.label(), w01_ret, -w01_dd, w05_ret, -w05_dd);
    }

    // Write CSV
    let csv_path = "snapshots/hedge_slippage_sweep.csv";
    let mut f = std::fs::OpenOptions::new().create(true).write(true).truncate(true).open(csv_path)?;
    for line in &csv_lines { writeln!(f, "{}", line)?; }
    println!("\nCSV: {}", csv_path);
    println!("Runtime: {:.1}s", t0.elapsed().as_secs_f64());

    // ── Verdict ────────────────────────────────────────────────────────────
    println!("\n═══ VERDICT ═══");
    // Compare hedge vs baseline
    let mut base_pass_cnt = 0usize; let mut base_total_cnt = 0usize;
    let mut hedge_pass_cnt = 0usize; let mut hedge_total_cnt = 0usize;
    let mut base_dd_sum = 0.0_f64; let mut hedge_dd_sum = 0.0_f64;
    for (key, &(p, t, r, d)) in &global_stats {
        if key.ends_with("BASE") { base_pass_cnt += p; base_total_cnt += t; base_dd_sum += d; }
        if key.ends_with("HEDGE") { hedge_pass_cnt += p; hedge_total_cnt += t; hedge_dd_sum += d; }
    }
    let base_rate = if base_total_cnt > 0 { base_pass_cnt as f64 / base_total_cnt as f64 } else { 0.0 };
    if base_total_cnt > 0 && hedge_total_cnt > 0 {
        let hedge_rate = hedge_pass_cnt as f64 / hedge_total_cnt as f64;
        let dd_improvement = base_dd_sum - hedge_dd_sum;
        if dd_improvement > 0.0 && hedge_rate >= base_rate - 0.05 {
            println!("✅ USDT hedge reduces avg DD by {:.1}pp while maintaining pass rate ({:.0}% vs {:.0}%)",
                dd_improvement / base_total_cnt as f64, hedge_rate * 100.0, base_rate * 100.0);
        } else if dd_improvement > 0.0 {
            println!("⚠️  USDT hedge reduces DD but hurts pass rate ({:.0}% vs {:.0}%)",
                hedge_rate * 100.0, base_rate * 100.0);
        } else {
            println!("❌ USDT hedge does NOT improve drawdown — the vol signal is not discriminative enough");
        }
    }

    // Slippage verdict
    let mut slip_pass = 0usize; let mut slip_total = 0usize;
    for (key, &(p, t, _r, _d)) in &global_stats {
        if key.ends_with("SLIP5") { slip_pass += p; slip_total += t; }
    }
    if base_total_cnt > 0 && slip_total > 0 {
        let slip_rate = slip_pass as f64 / slip_total as f64;
        if slip_rate >= base_rate - 0.05 {
            println!("✅ Edge survives 5bps slippage (pass rate {:.0}% vs {:.0}%)",
                slip_rate * 100.0, base_rate * 100.0);
        } else {
            println!("❌ Edge is SIGNIFICANTLY degraded by 5bps slippage ({:.0}% vs {:.0}%)",
                slip_rate * 100.0, base_rate * 100.0);
        }
    }

    Ok(())
}
