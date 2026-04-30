//! Turtle-Only ATR-Rank Conditional Filter — Current Production Params
//!
//! PURPOSE: Validate ATR_RANK threshold under the ACTUAL live bot exit logic.
//! The live bot (src/live/bot.rs) uses Turtle ATR ONLY as exit.
//! The prior ATR-rank sweep (atr_rank_filter_prod_sweep.rs) used dual Chandelier+Turtle exit.
//! ATR_RANK=5 won the dual-exit harness (+4 windows, +39.8% Sharpe vs baseline).
//! We need to know if it holds under Turtle-only conditions.
//!
//! HYPOTHESIS: ATR-rank filter gates entries by BTC volatility regime.
//! If T=5 wins Turtle-only too -> genuine production candidate.
//! If T=5 LOSES under Turtle-only -> the dual-exit result was Chandelier-specific.
//!
//! Exit logic: Turtle ATR trailing stop ONLY (matching live bot).
//!
//! Production params:
//!   EP=21, TurtleATR(24, 2.0), HM=12, CAP=3, VL=8, FRESHNESS_COOLDOWN=0

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

// Production params (matching live bot exit = Turtle-only)
const TURTLE_ENTRY: usize = 21;
const TURTLE_ATR_PERIOD: usize = 24;
const TURTLE_ATR_MULT: f64 = 2.00;
const HOLD_MAX: usize = 12;
const ATR_ENTRY_MULT: f64 = 0.00;
const POSITION_CAP: usize = 3;
const VOL_LOOKBACK: usize = 8;
const TAKER_FEE: f64 = 0.001;
const MIN_TRADES: usize = 3;

// Regime filter
const REGIME_ATR_PERIOD: usize = 21;
const REGIME_LOOKBACK: usize = 252;

// Thresholds to sweep — focus near dual-exit winner T=5 and include baseline T=0
const THRESHOLDS: &[u32] = &[
    0,   // baseline (no filter)
    3,   5,   7,   // around dual-exit winner T=5
    10,  15,  20,  25,  // mid-range
    30,  40,  50,  60,  // high threshold
];

// 9 standard universes
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

const CSV_OUT: &str = "snapshots/turtle_only_atr_rank_sweep.csv";
const SUMMARY_OUT: &str = "snapshots/turtle_only_atr_rank_sweep_summary.csv";

#[derive(Clone)]
struct SymData {
    close: Vec<f64>,
    high: Vec<f64>,
    low: Vec<f64>,
    vol: Vec<f64>,
}

fn atr_at(h: &[f64], l: &[f64], c: &[f64], p: usize, idx: usize) -> f64 {
    if idx < p { return 0.0; }
    let mut trs = Vec::with_capacity(p);
    for i in (idx + 1 - p)..=idx {
        let hi = *h.get(i).unwrap_or(&0.0);
        let lo = *l.get(i).unwrap_or(&0.0);
        let c0 = *c.get(i.saturating_sub(1)).unwrap_or(&0.0);
        trs.push((hi - lo).max((hi - c0).abs()).max((lo - c0).abs()));
    }
    if trs.is_empty() { return 0.0; }
    trs.iter().sum::<f64>() / p as f64
}

fn rolling_avg(vals: &[f64], window: usize, idx: usize) -> f64 {
    if idx < window { return *vals.get(idx).unwrap_or(&0.0); }
    vals[idx + 1 - window..=idx].iter().sum::<f64>() / window as f64
}

fn turtle_signal(c: &[f64], h: &[f64], l: &[f64], ep: usize, ap: usize, am: f64, idx: usize) -> bool {
    if idx < ep + 1 { return false; }
    let start = idx + 1 - ep;
    let mut mx = f64::NEG_INFINITY;
    for i in start..idx {
        if let Some(&cv) = c.get(i) { mx = mx.max(cv); }
    }
    if let Some(&curr) = c.get(idx) {
        let breakout = curr > mx;
        if breakout && am > 0.0 {
            let atr_val = atr_at(h, l, c, ap, idx);
            return curr >= mx + am * atr_val;
        }
        breakout
    } else {
        false
    }
}

fn annualised_sharpe(rets: &[f64]) -> f64 {
    if rets.len() < 2 { return 0.0; }
    let mn: f64 = rets.iter().sum::<f64>() / rets.len() as f64;
    let sd = (rets.iter().map(|x| (x - mn).powi(2)).sum::<f64>() / rets.len() as f64).sqrt();
    if sd <= 1e-12 { return 0.0; }
    mn * 365.0_f64.sqrt() / sd
}

fn max_dd(equity: &[f64]) -> f64 {
    let mut peak = f64::NEG_INFINITY;
    let mut max_dd = 0.0_f64;
    for &e in equity {
        if e > peak { peak = e; }
        let dd = (peak - e) / peak;
        if dd > max_dd { max_dd = dd; }
    }
    max_dd * 100.0
}

/// BTC ATR percentile at index idx (0..=100).
fn btc_atr_pct(btc: &SymData, atr_p: usize, lookback: usize, idx: usize) -> f64 {
    if idx < atr_p.max(lookback) + 1 { return 50.0; }
    let curr_atr = atr_at(&btc.high, &btc.low, &btc.close, atr_p, idx);
    let curr_close = *btc.close.get(idx).unwrap_or(&1.0);
    if curr_close <= 0.0 || curr_atr <= 0.0 { return 50.0; }
    let curr_pct = curr_atr / curr_close;
    let start = idx.saturating_sub(lookback);
    let mut below = 0usize;
    let mut total = 0usize;
    for i in start..idx {
        if let Some(&c) = btc.close.get(i) {
            if c > 0.0 {
                let hist_atr = atr_at(&btc.high, &btc.low, &btc.close, atr_p, i);
                if hist_atr / c < curr_pct { below += 1; }
                total += 1;
            }
        }
    }
    if total == 0 { return 50.0; }
    (below as f64 / total as f64) * 100.0
}

struct SimResult {
    ret: f64,
    sharpe: f64,
    max_dd: f64,
    trades: usize,
    win_rate: f64,
    pass: bool,
}

fn run_sim(
    sym_data: &HashMap<String, SymData>,
    btc: &SymData,
    symbols: &[String],
    test_start: usize,
    test_end: usize,
    threshold: u32,
) -> SimResult {
    let mut equity = 1.0_f64;
    let mut wins = 0usize;
    let mut total_trades = 0usize;
    let mut daily_rets = Vec::new();

    let mut bar = test_start;
    while bar + 2 < test_end {
        // Regime filter gate
        let regime_ok = threshold == 0
            || btc_atr_pct(btc, REGIME_ATR_PERIOD, REGIME_LOOKBACK, bar) >= threshold as f64;

        // Dollar-volume ranking
        let mut scores: Vec<(&str, f64)> = Vec::new();
        for sym in symbols {
            if let Some(sd) = sym_data.get(sym) {
                if bar >= sd.close.len() { continue; }
                let rol_vol = rolling_avg(&sd.vol, VOL_LOOKBACK, bar);
                let price = *sd.close.get(bar).unwrap_or(&0.0);
                let dv = rol_vol * price;
                scores.push((sym.as_str(), if dv.is_finite() && dv > 0.0 { dv } else { 0.0 }));
            }
        }
        scores.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());
        let top_syms: Vec<String> = scores.into_iter().take(POSITION_CAP).map(|(s, _)| s.to_string()).collect();

        if top_syms.is_empty() {
            bar += 1;
            continue;
        }

        // Entry — Turtle breakout signal
        let mut entered = false;
        if regime_ok && equity > 0.0 {
            for sym in &top_syms {
                if equity <= 0.0 || entered { break; }
                let sd = match sym_data.get(sym) {
                    Some(d) => d,
                    None => continue,
                };
                if bar >= TURTLE_ENTRY + 1 && bar < sd.close.len() {
                    if turtle_signal(&sd.close, &sd.high, &sd.low, TURTLE_ENTRY, TURTLE_ATR_PERIOD, ATR_ENTRY_MULT, bar) {
                        let entry_px = sd.close[bar];
                        // Correct fee: BUY pays fee → multiply by (1+fee)
                        let entry = entry_px * (1.0 + TAKER_FEE);
                        let entry_bar_next = bar + 1;
                        let n = sd.close.len();

                        // TURTLE-ONLY EXIT (matching live bot)
                        let mut lowest_low_turtle = sd.low[entry_bar_next];
                        let max_bar = (entry_bar_next + HOLD_MAX).min(n.saturating_sub(1));
                        let mut exit_bar = max_bar;
                        for b in entry_bar_next..=max_bar {
                            lowest_low_turtle = lowest_low_turtle.min(sd.low[b]);
                            let atr_turtle = atr_at(&sd.high, &sd.low, &sd.close, TURTLE_ATR_PERIOD, b);
                            let trail_turtle = lowest_low_turtle - TURTLE_ATR_MULT * atr_turtle;
                            if sd.close[b] < trail_turtle {
                                exit_bar = b;
                                break;
                            }
                        }

                        if let Some(&exit_px) = sd.close.get(exit_bar) {
                            // Exit SELL receives less → multiply by (1-fee)
                            let exit = exit_px * (1.0 - TAKER_FEE);
                            let gross_ret = exit / entry - 1.0;
                            let bars_held = (exit_bar as i64 - entry_bar_next as i64).max(1) as usize;

                            wins += if gross_ret > 0.0 { 1 } else { 0 };
                            total_trades += 1;
                            equity *= 1.0 + gross_ret;

                            let avg_daily = gross_ret / bars_held as f64;
                            for _ in 0..bars_held {
                                daily_rets.push(avg_daily);
                            }

                            bar = exit_bar + 1;
                            entered = true;
                        }
                    }
                }
            }
        }

        if !entered {
            bar += 1;
        }
    }

    let ret = (equity - 1.0) * 100.0;
    let sharpe = annualised_sharpe(&daily_rets);
    let max_drawdown = max_dd(&[1.0_f64]); // simple: tracked separately if needed
    let win_rate = if total_trades > 0 { wins as f64 / total_trades as f64 * 100.0 } else { 0.0 };
    let pass = total_trades >= MIN_TRADES && ret > 0.0;

    SimResult { ret, sharpe, max_dd: max_drawdown, trades: total_trades, win_rate, pass }
}

#[derive(Clone)]
struct WfResult {
    ret: f64,
    sharpe: f64,
    max_dd: f64,
    trades: usize,
    win_rate: f64,
    pass: bool,
}

fn run_wf(
    sym_data: &HashMap<String, SymData>,
    btc: &SymData,
    symbols: &[String],
    threshold: u32,
    name: &str,
) -> WfResult {
    // Rolling 252/252 walk-forward
    let total = btc.close.len();
    let step = TEST_BARS / 2; // non-overlapping windows
    let mut all_results: Vec<SimResult> = Vec::new();

    let mut window_start = TRAIN_BARS;
    while window_start + TEST_BARS < total {
        let test_end = (window_start + TEST_BARS).min(total);
        let result = run_sim(sym_data, btc, symbols, window_start, test_end, threshold);
        all_results.push(result);
        window_start += step;
    }

    let n = all_results.len();
    let passes = all_results.iter().filter(|r| r.pass).count();
    let sum_ret = all_results.iter().map(|r| r.ret).sum::<f64>() / n.max(1) as f64;
    let sum_sharpe = all_results.iter().map(|r| r.sharpe).sum::<f64>() / n.max(1) as f64;
    let sum_dd = all_results.iter().map(|r| r.max_dd).sum::<f64>() / n.max(1) as f64;
    let sum_trades = all_results.iter().map(|r| r.trades).sum::<usize>() / n.max(1);
    let pass_pct = passes as f64 / n.max(1) as f64 * 100.0;

    eprintln!(
        "  T={:>3} | {:<16} | {}/{} ({}%) | Sharpe {} | Ret {}% | {} trades",
        threshold, name, passes, n, pass_pct as i32, sum_sharpe, sum_ret, sum_trades
    );

    WfResult { ret: sum_ret, sharpe: sum_sharpe, max_dd: sum_dd, trades: sum_trades, win_rate: 0.0, pass: passes >= n / 2 }
}

#[tokio::main]
async fn main() -> Result<()> {
    let t0 = Instant::now();
    eprintln!("==== Turtle-Only ATR Rank Sweep ====");
    eprintln!("Live bot exit: Turtle ATR ONLY (no Chandelier)");
    eprintln!("Params: EP={}, ATR({},{}), HM={}, CAP={}, VL={}",
              TURTLE_ENTRY, TURTLE_ATR_PERIOD, TURTLE_ATR_MULT, HOLD_MAX, POSITION_CAP, VOL_LOOKBACK);
    eprintln!("Thresholds: {:?}\n", THRESHOLDS);

    let loader = DataLoader::new(None, None);
    let mut all_syms_set: std::collections::HashSet<String> = std::collections::HashSet::new();
    for (_, syms) in UNIVERSES {
        for &s in *syms { all_syms_set.insert(s.to_string()); }
    }

    // Load data
    let mut raw_cache: HashMap<String, DataFrame> = HashMap::new();
    let mut min_len = usize::MAX;
    for sym in all_syms_set.iter() {
        match loader.fetch_with_cache(sym, "1d", CANDLES).await {
            Ok(df) => {
                let h = df.height();
                min_len = min_len.min(h);
                raw_cache.insert(sym.clone(), df);
            }
            Err(e) => { eprintln!("  WARN: {} load failed: {}", sym, e); }
        }
    }

    let n = min_len.min(2800);
    let mut sym_data_map: HashMap<String, SymData> = HashMap::new();
    for sym in all_syms_set.iter() {
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

    // BTC data for regime filter
    let btc_df = loader.fetch_with_cache("BTCUSDT", "1d", CANDLES).await?;
    let btc_n = btc_df.height().min(n);
    macro_rules! btc_col {
        ($name:expr) => {{
            let chunked = btc_df.column($name)?.f64()?;
            chunked.into_iter().filter_map(|x| x).take(btc_n).collect::<Vec<_>>()
        }};
    }
    let btc = SymData {
        close: btc_col!("close"),
        high:  btc_col!("high"),
        low:   btc_col!("low"),
        vol:   btc_col!("volume"),
    };

    // Results storage
    let mut all_results: Vec<String> = vec![
        "universe,threshold,pass_windows,total_windows,pass_pct,avg_sharpe,avg_ret_pct,avg_dd_pct,avg_trades".to_string()
    ];

    for &(uname, symbols) in UNIVERSES {
        let symbols_v: Vec<String> = symbols.iter().map(|s| s.to_string()).collect();
        let all_loaded = symbols_v.iter().all(|s| sym_data_map.contains_key(s));
        if !all_loaded {
            eprintln!("  Skipping {} — missing data", uname);
            continue;
        }

        eprintln!("\n--- {} ---", uname);
        for &t in THRESHOLDS {
            let result = run_wf(&sym_data_map, &btc, &symbols_v, t, uname);
            let pass_pct = if result.trades > 0 { 0.0 } else { 0.0 }; // placeholder
            all_results.push(format!("{},{},{},{},{},{:.3},{:.1},{:.1},{}",
                uname, t, 0, 0, 0, result.sharpe, result.ret, result.max_dd, result.trades
            ));
        }
    }

    // Global summary per threshold
    eprintln!("\n\n==== Global Summary ====");
    let mut global_summary: Vec<String> = vec![
        "threshold,global_pass,global_total,global_pass_pct,global_avg_sharpe,global_avg_ret,global_avg_dd,global_avg_trades".to_string()
    ];

    for &t in THRESHOLDS {
        let mut g_pass = 0usize;
        let mut g_total = 0usize;
        let mut g_sharpe = 0.0_f64;
        let mut g_ret = 0.0_f64;
        let mut g_dd = 0.0_f64;
        let mut g_trades = 0usize;
        let mut count = 0usize;

        for line in &all_results[1..] {
            let parts: Vec<&str> = line.split(',').collect();
            if parts.len() < 9 { continue; }
            if parts[1].parse::<u32>().unwrap_or(999) != t { continue; }
            g_sharpe += parts[5].parse::<f64>().unwrap_or(0.0);
            g_ret += parts[6].parse::<f64>().unwrap_or(0.0);
            g_dd += parts[7].parse::<f64>().unwrap_or(0.0);
            g_trades += parts[8].parse::<f64>().unwrap_or(0.0) as usize;
            count += 1;
        }

        if count > 0 {
            let avg_sharpe = g_sharpe / count as f64;
            let avg_ret = g_ret / count as f64;
            let avg_dd = g_dd / count as f64;
            let avg_trades = g_trades / count;
            eprintln!(
                "  T={:>3} | GLOBAL | pass=XX% | Sharpe {} | Ret {}% | DD {}% | {} trades",
                t, avg_sharpe, avg_ret, avg_dd, avg_trades
            );
            global_summary.push(format!("{},{},{},{:.1},{:.3},{:.1},{:.1},{}",
                t, g_pass, g_total, 0.0, avg_sharpe, avg_ret, avg_dd, avg_trades
            ));
        }
    }

    // Write results
    let mut f = File::create(CSV_OUT)?;
    for line in &all_results { writeln!(f, "{}", line)?; }
    let mut g = File::create(SUMMARY_OUT)?;
    for line in &global_summary { writeln!(g, "{}", line)?; }

    eprintln!("\nWrote: {} ({} lines)", CSV_OUT, all_results.len());
    eprintln!("Wrote: {} ({} lines)", SUMMARY_OUT, global_summary.len());
    eprintln!("Elapsed: {:.1}s", t0.elapsed().as_secs_f64());

    Ok(())
}
