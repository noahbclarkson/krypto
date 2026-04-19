//! Turtle+Chandelier + ATR Percentile Regime Filter Walk-Forward
//!
//! HYPOTHESIS: BTC's 20-bar ATR percentile (vs 252-bar history) is a regime detector.
//! When BTC ATR percentile < threshold -> skip new Turtle entries (don't exit existing ones).
//!
//! DIFFERENT from vol-contingent Chandelier (GRAVEYARD 2026-04-12):
//!   Vol-contingent Chandelier changed the STOP MULTIPLIER dynamically.
//!   ATR percentile regime filter does NOT touch the stop -- only gates entries.

use anyhow::Result;
use krypto::data::loader::DataLoader;
use polars::prelude::*;
use std::collections::HashMap;
use std::fs::File;
use std::io::Write;
use std::time::Instant;

const CANDLES: u32 = 3000;
const TRAIN_BARS: usize = 252;
const TEST_BARS: usize = 252;
const HOLD_MAX: usize = 45;
const TAKER_FEE: f64 = 0.001;
const POSITION_CAP: usize = 3;
const MIN_TRADES: usize = 3;

// Production Chandelier params (frozen 2026-04-19)
const CHAND_PERIOD: usize = 15;
const CHAND_MULT: f64 = 1.50;
const TURTLE_ENTRY: usize = 21;
const TURTLE_ATR_PERIOD: usize = 24;
const TURTLE_ATR_MULT: f64 = 2.00;

// Regime filter params (what we're testing)
const REGIME_ATR_PERIOD: usize = 20;
const REGIME_LOOKBACK: usize = 252;

// Threshold values to sweep: 0 = baseline (no filter)
const THRESHOLDS: &[u32] = &[0, 20, 30, 40, 50, 60, 70];

const UNIVERSES: &[(&str, &[&str])] = &[
    ("Base5",        &["BTCUSDT","ETHUSDT","SOLUSDT","XRPUSDT","DOGEUSDT","ADAUSDT"]),
    ("NoDOGE",       &["BTCUSDT","ETHUSDT","SOLUSDT","XRPUSDT","ADAUSDT"]),
    ("Legacy4",      &["BTCUSDT","ETHUSDT","XRPUSDT","LTCUSDT","EOSUSDT"]),
    ("Legacy5BNB",   &["BTCUSDT","ETHUSDT","XRPUSDT","LTCUSDT","BNBUSDT","EOSUSDT"]),
    ("OldGuardNoBNB",&["BTCUSDT","ETHUSDT","XRPUSDT","LTCUSDT","EOSUSDT","BCHUSDT"]),
    ("LargeCaps5",   &["BTCUSDT","ETHUSDT","SOLUSDT","XRPUSDT","BNBUSDT","ADAUSDT"]),
    ("Legacy3",      &["BTCUSDT","XRPUSDT","LTCUSDT","EOSUSDT"]),
    ("LowVolume5",   &["XRPUSDT","LTCUSDT","EOSUSDT","BCHUSDT","ADAUSDT"]),
    ("OldGuard4",    &["BTCUSDT","XRPUSDT","LTCUSDT","EOSUSDT","BCHUSDT"]),
];

const CSV_OUT: &str = "snapshots/turtle_atr_regime_filter_results.csv";
const EQUITY_CSV_OUT: &str = "snapshots/turtle_atr_regime_filter_equity.csv";

#[derive(Clone)]
struct SymData {
    close: Vec<f64>,
    high: Vec<f64>,
    low: Vec<f64>,
    vol: Vec<f64>,
}

fn atr_at(high: &[f64], low: &[f64], close: &[f64], period: usize, idx: usize) -> f64 {
    if idx < period { return 0.0; }
    let mut trs = Vec::with_capacity(period);
    for i in (idx + 1 - period)..=idx {
        let h = high.get(i).copied().unwrap_or(0.0);
        let l = low.get(i).copied().unwrap_or(0.0);
        let c0 = close.get(i.saturating_sub(1)).copied().unwrap_or(0.0);
        trs.push((h - l).max((h - c0).abs()).max((l - c0).abs()));
    }
    if trs.is_empty() { return 0.0; }
    trs.iter().sum::<f64>() / period as f64
}

fn rolling_avg(vals: &[f64], window: usize, idx: usize) -> f64 {
    if idx < window { return *vals.get(idx).unwrap_or(&0.0); }
    let start = idx + 1 - window;
    vals[start..=idx].iter().sum::<f64>() / window as f64
}

/// Compute BTC's ATR percentile at index idx.
/// Returns percentile (0..=100).
fn btc_atr_percentile(btc: &SymData, atr_period: usize, lookback: usize, idx: usize) -> f64 {
    if idx < atr_period.max(lookback) + 1 {
        return 50.0;
    }
    let current_atr = atr_at(&btc.high, &btc.low, &btc.close, atr_period, idx);
    let current_close = *btc.close.get(idx).unwrap_or(&1.0);
    if current_close <= 0.0 || current_atr <= 0.0 {
        return 50.0;
    }
    let current_atr_pct = current_atr / current_close;
    let start = idx.saturating_sub(lookback);
    let mut count_below = 0usize;
    let mut total = 0usize;
    for i in start..idx {
        if let Some(&c) = btc.close.get(i) {
            if c > 0.0 {
                let hist_atr = atr_at(&btc.high, &btc.low, &btc.close, atr_period, i);
                if hist_atr / c < current_atr_pct {
                    count_below += 1;
                }
                total += 1;
            }
        }
    }
    if total == 0 { return 50.0; }
    (count_below as f64 / total as f64) * 100.0
}

fn turtle_signal(close: &[f64], entry_period: usize, idx: usize) -> bool {
    if idx < entry_period + 1 { return false; }
    let start = idx + 1 - entry_period;
    let mut max_close = f64::NEG_INFINITY;
    for i in start..idx {
        if let Some(&c) = close.get(i) { max_close = max_close.max(c); }
    }
    close.get(idx).map(|&c| c > max_close).unwrap_or(false)
}

fn annualised_sharpe(daily_rets: &[f64]) -> f64 {
    if daily_rets.len() < 2 { return 0.0; }
    let mn: f64 = daily_rets.iter().sum::<f64>() / daily_rets.len() as f64;
    let sd = (daily_rets.iter().map(|x| (x - mn).powi(2)).sum::<f64>() / daily_rets.len() as f64).sqrt();
    if sd == 0.0 { return 0.0; }
    mn * 365.0_f64.sqrt() / sd
}

fn max_dd_from(equity: &[f64]) -> f64 {
    let mut peak = f64::NEG_INFINITY;
    let mut max_dd = 0.0_f64;
    for &e in equity {
        if e > peak { peak = e; }
        let dd = (peak - e) / peak;
        if dd > max_dd { max_dd = dd; }
    }
    max_dd * 100.0
}

struct WfResult {
    ret: f64,
    sharpe: f64,
    max_dd: f64,
    trades: usize,
    win_rate: f64,
    pass: bool,
    equity_curve: Vec<f64>,
}

struct SimConfig {
    threshold: u32,
}

impl SimConfig {
    fn new(threshold: u32) -> Self {
        Self { threshold }
    }
}

struct Position {
    entry_price: f64,
    shares: f64,
    entry_bar: usize,
    chand_stop: f64,
    atr_stop: f64,
}

fn run_sim(
    sym_data: &HashMap<String, SymData>,
    btc: &SymData,
    symbols: &[String],
    test_start: usize,
    test_end: usize,
    cfg: &SimConfig,
) -> WfResult {
    let mut equity = 1.0_f64;
    let mut equity_curve = vec![1.0_f64];
    let mut daily_rets = Vec::new();
    let mut wins = 0usize;
    let mut total_trades = 0usize;
    let mut positions: HashMap<String, Position> = HashMap::new();

    let mut bar = test_start;
    while bar + 1 < test_end {
        let prev_equity = equity;

        // Compute BTC ATR percentile (regime signal)
        let regime_permits_entry = if cfg.threshold == 0 {
            true
        } else {
            btc_atr_percentile(btc, REGIME_ATR_PERIOD, REGIME_LOOKBACK, bar) >= cfg.threshold as f64
        };

        // Rank symbols by dollar volume
        let mut scores: Vec<(&str, f64)> = Vec::new();
        for sym in symbols {
            if let Some(sd) = sym_data.get(sym) {
                if bar >= sd.close.len() { continue; }
                let rol_vol = rolling_avg(&sd.vol, 2, bar);
                let price = *sd.close.get(bar).unwrap_or(&0.0);
                let dv = rol_vol * price;
                scores.push((sym.as_str(), if dv.is_finite() && dv > 0.0 { dv } else { 0.0 }));
            }
        }
        scores.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());
        let top_syms: Vec<String> = scores.into_iter().take(POSITION_CAP).map(|(s, _)| s.to_string()).collect();

        // Entry logic
        if positions.len() < POSITION_CAP && regime_permits_entry {
            for sym in &top_syms {
                if positions.len() >= POSITION_CAP { break; }
                if positions.contains_key(sym) { continue; }
                let sd = match sym_data.get(sym) {
                    Some(d) => d,
                    None => continue,
                };
                let idx = bar;
                if !turtle_signal(&sd.close, TURTLE_ENTRY, idx) { continue; }
                let close_val = *sd.close.get(idx).unwrap_or(&0.0);
                if close_val <= 0.0 { continue; }

                let shares = equity / POSITION_CAP as f64 / close_val;

                // Chandelier stop
                let chand_high = sd.high[..=idx].iter().copied().fold(f64::NEG_INFINITY, f64::max);
                let chand_atr = atr_at(&sd.high, &sd.low, &sd.close, CHAND_PERIOD, idx);
                let chand_stop = chand_high - CHAND_MULT * chand_atr;

                // Turtle ATR stop
                let turtle_low = *sd.low.get(idx).unwrap_or(&close_val);
                let turtle_atr = atr_at(&sd.high, &sd.low, &sd.close, TURTLE_ATR_PERIOD, idx);
                let atr_stop = turtle_low - TURTLE_ATR_MULT * turtle_atr;

                positions.insert(sym.clone(), Position {
                    entry_price: close_val,
                    shares,
                    entry_bar: idx,
                    chand_stop,
                    atr_stop,
                });
            }
        }

        // Exit logic
        let to_remove: Vec<String> = {
            let mut exit_list = Vec::new();
            for (sym, pos) in positions.iter() {
                let sd = match sym_data.get(sym) {
                    Some(d) => d,
                    None => continue,
                };
                let idx = bar.min(sd.close.len().saturating_sub(1));
                let curr_close = *sd.close.get(idx).unwrap_or(&0.0);
                if curr_close <= 0.0 { continue; }

                // Update Chandelier stop (trailing)
                let mut curr_chand = pos.chand_stop;
                if idx >= CHAND_PERIOD {
                    let hist_high = sd.high[(idx + 1 - CHAND_PERIOD)..=idx].iter().copied().fold(f64::NEG_INFINITY, f64::max);
                    let new_atr = atr_at(&sd.high, &sd.low, &sd.close, CHAND_PERIOD, idx);
                    curr_chand = (hist_high - CHAND_MULT * new_atr).max(pos.chand_stop);
                }

                // Update Turtle ATR stop (trailing)
                let mut curr_atr = pos.atr_stop;
                if idx >= TURTLE_ATR_PERIOD {
                    let curr_low = *sd.low.get(idx).unwrap_or(&curr_close);
                    let new_tatr = atr_at(&sd.high, &sd.low, &sd.close, TURTLE_ATR_PERIOD, idx);
                    curr_atr = (curr_low - TURTLE_ATR_MULT * new_tatr).max(pos.atr_stop);
                }

                let bars_held = bar - pos.entry_bar;
                let stop_hit = curr_close <= curr_chand || curr_close <= curr_atr;
                let hold_expired = bars_held >= HOLD_MAX;

                if stop_hit || hold_expired {
                    exit_list.push(sym.clone());
                    let pnl = pos.shares * (curr_close - pos.entry_price);
                    let fees = pos.shares * curr_close * TAKER_FEE + pos.shares * pos.entry_price * TAKER_FEE;
                    equity += pnl - fees;
                    total_trades += 1;
                    if pnl > 0.0 { wins += 1; }
                }
            }
            exit_list
        };

        // Open P&L for still-open positions
        let open_pnl: f64 = positions.iter().map(|(pos_sym, pos)| {
            let sd = sym_data.get(pos_sym).unwrap();
            let idx = bar.min(sd.close.len().saturating_sub(1));
            let curr_close = *sd.close.get(idx).unwrap_or(&0.0);
            pos.shares * (curr_close - pos.entry_price)
        }).sum::<f64>();

        for sym in &to_remove {
            positions.remove(sym);
        }

        equity += open_pnl;
        let day_ret = (equity - prev_equity) / prev_equity.max(1.0);
        daily_rets.push(day_ret);
        equity_curve.push(equity);
        bar += 1;
    }

    let final_ret = (equity - 1.0) * 100.0;
    let sharpe = annualised_sharpe(&daily_rets);
    let max_dd = max_dd_from(&equity_curve);
    let win_rate = if total_trades > 0 { wins as f64 / total_trades as f64 * 100.0 } else { 0.0 };

    WfResult {
        ret: final_ret,
        sharpe,
        max_dd,
        trades: total_trades,
        win_rate,
        pass: total_trades >= MIN_TRADES && equity > 1.0,
        equity_curve,
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    let t0 = Instant::now();

    eprintln!("=== ATR Percentile Regime Filter Walk-Forward ===");
    eprintln!("Regime ATR period: {}", REGIME_ATR_PERIOD);
    eprintln!("Regime lookback: {} bars", REGIME_LOOKBACK);
    eprintln!("Thresholds: {:?}", THRESHOLDS);
    eprintln!();

    let loader = DataLoader::new(None, None);
    let mut all_syms: std::collections::HashSet<String> = std::collections::HashSet::new();
    for (_, syms) in UNIVERSES {
        for &s in *syms {
            all_syms.insert(s.to_string());
        }
    }

    // Load all data
    let mut raw_cache: HashMap<String, DataFrame> = HashMap::new();
    let mut min_len = usize::MAX;
    for sym in all_syms.iter() {
        match loader.fetch_with_cache(sym.as_str(), "1d", CANDLES).await {
            Ok(df) => {
                min_len = min_len.min(df.height());
                raw_cache.insert(sym.clone(), df);
            }
            Err(e) => { eprintln!("  WARNING: {} load failed: {}", sym, e); }
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
                high:  col_vec!("high"),
                low:   col_vec!("low"),
                vol:   col_vec!("volume"),
            });
        }
    }
    eprintln!("Loaded {} symbols, {} bars\n", sym_data_map.len(), n);

    let btc = sym_data_map.get("BTCUSDT").cloned()
        .expect("BTCUSDT data required");

    let mut all_results = Vec::new();
    let mut equity_cols: HashMap<String, Vec<f64>> = HashMap::new(); // "uname_win_thresh" -> equity

    for &(uname, symbols) in UNIVERSES {
        let symbols: Vec<String> = symbols.iter().map(|s| s.to_string()).collect();
        let all_loaded = symbols.iter().all(|s| sym_data_map.contains_key(s));
        if !all_loaded {
            eprintln!("{:>20} SKIPPED (missing data)", uname);
            continue;
        }

        let total_windows = n.saturating_sub(TRAIN_BARS + TEST_BARS) / TEST_BARS;
        if total_windows == 0 {
            eprintln!("{:>20} SKIPPED (not enough data)", uname);
            continue;
        }

        eprintln!("==== {:<18} ==== {} syms, {} windows", uname, symbols.len(), total_windows);

        for wi in 0..total_windows {
            let train_end = TRAIN_BARS + wi * TEST_BARS;
            let test_start = train_end;
            let test_end = (test_start + TEST_BARS).min(n);
            if test_end.saturating_sub(test_start) < 5 { continue; }

            for &threshold in THRESHOLDS {
                let cfg = SimConfig::new(threshold);
                let r = run_sim(&sym_data_map, &btc, &symbols, test_start, test_end, &cfg);

                let col_key = format!("{}_W{}_{}", uname, wi, threshold);
                equity_cols.insert(col_key, r.equity_curve.clone());

                all_results.push((
                    uname.to_string(),
                    wi,
                    threshold,
                    r.ret,
                    r.sharpe,
                    r.max_dd,
                    r.trades,
                    r.win_rate,
                    r.pass,
                ));

                let status = if r.pass { "PASS" } else { "FAIL" };
                eprintln!(
                    "  W{} thresh={:3}: {} | ret={:+.2}% | Sharpe={:.3} | DD={:.2}% | trades={}",
                    wi, threshold, status, r.ret, r.sharpe, r.max_dd, r.trades
                );
            }
        }
    }

    // Aggregate
    eprintln!("\n\n=== AGGREGATE RESULTS ===");
    let mut summary: HashMap<u32, (usize, usize, f64, f64, f64, usize)> = HashMap::new();
    for (uname, w, threshold, ret, sharpe, max_dd, trades, _, pass) in &all_results {
        let e = summary.entry(*threshold).or_insert((0, 0, 0.0, 0.0, 0.0, 0));
        e.0 += if *pass { 1 } else { 0 };
        e.1 += 1;
        e.2 += ret;
        e.3 += sharpe;
        e.4 += max_dd;
        e.5 += *trades;
    }

    eprintln!("\n{:<12} {:>6} {:>10} {:>10} {:>10} {:>10}",
             "Threshold", "PassRate", "AvgRet%", "AvgSharpe", "AvgDD%", "Trades");
    let mut rows: Vec<(u32, usize, usize, f64, f64, f64, usize)> = Vec::new();
    for &threshold in THRESHOLDS {
        if let Some((pass, total, sum_ret, sum_sharpe, sum_dd, sum_trades)) = summary.get(&threshold) {
            let avg_ret = sum_ret / *total as f64;
            let avg_sharpe = sum_sharpe / *total as f64;
            let avg_dd = sum_dd / *total as f64;
            let pass_rate = *pass as f64 / *total as f64 * 100.0;
            let pr_str = format!("{:.1}", pass_rate);
            let ar_str = format!("{:.2}", avg_ret);
            let sh_str = format!("{:.3}", avg_sharpe);
            let dd_str = format!("{:.2}", avg_dd);
            let st_str = format!("{}", sum_trades);
            eprintln!("{:<12} {:>3}/{:.<3} ({}% {}) {} {} {}",
                threshold, pass, total, pr_str, ar_str, sh_str, dd_str, st_str);
            rows.push((threshold, *pass, *total, avg_ret, avg_sharpe, avg_dd, *sum_trades));
        }
    }

    // Winner determination
    let best_threshold = rows.iter()
        .max_by(|a, b| {
            let score_a = a.1 as f64 / a.2 as f64 * 100.0 + a.4 * 10.0; // pass_rate + Sharpe bonus
            let score_b = b.1 as f64 / b.2 as f64 * 100.0 + b.4 * 10.0;
            score_a.partial_cmp(&score_b).unwrap()
        })
        .map(|(t, ..)| *t)
        .unwrap_or(0);

    eprintln!("\nWINNER: threshold={}", best_threshold);

    // Write CSV
    let mut f = File::create(CSV_OUT)?;
    writeln!(f, "universe,window,threshold,return_pct,sharpe,max_dd_pct,trades,win_rate,pass")?;
    for (uname, w, threshold, ret, sharpe, max_dd, trades, win_rate, pass) in &all_results {
        writeln!(f, "{},{},{},{:.4},{:.6},{:.4},{},{:.2},{}",
            uname, w, threshold, ret, sharpe, max_dd, trades, win_rate,
            if *pass { 1 } else { 0 })?;
    }
    eprintln!("\nResults -> {}", CSV_OUT);

    // Write equity CSV (baseline vs winner per universe/window)
    let max_len = equity_cols.values().map(|v| v.len()).max().unwrap_or(0);
    let mut f_eq = File::create(EQUITY_CSV_OUT)?;
    let mut keys: Vec<&String> = equity_cols.keys().collect();
    keys.sort();
    let header = format!("step,{}", keys.iter().map(|k| k.as_str()).collect::<Vec<_>>().join(","));
    writeln!(f_eq, "{}", header)?;
    for step in 0..max_len {
        let mut line = format!("{}", step);
        for k in &keys {
            if let Some(v) = equity_cols.get(*k).and_then(|v| v.get(step)) {
                line.push_str(&format!(",{:.6}", v));
            } else {
                line.push(',');
            }
        }
        writeln!(f_eq, "{}", line)?;
    }
    eprintln!("Equity -> {}", EQUITY_CSV_OUT);

    eprintln!("\nDone in {:.1}s", t0.elapsed().as_secs_f64());
    Ok(())
}
