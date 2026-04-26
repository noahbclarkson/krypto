//! =========================================================
//! ATR_PERIOD RE-OPTIMIZATION — Current Production Params
//! =========================================================
//!
//! Background:
//!   - Prior sweeps (2026-04-12/16): ATR=24 won with STALE params:
//!     EP=21, CHAND_PERIOD=28, CHAND_MULT=2.0, HOLD_MAX=45
//!   - Current production: EP=24, CHAND_PERIOD=7, CHAND_MULT=2.30, HOLD_MAX=12
//!   - With tight CHAND_PERIOD=7, the dual-exit dynamics have changed.
//!     Chandelier fires at ~bar 12-15. Turtle ATR fires less frequently.
//!     ATR period sensitivity may shift.
//!
//! Target: TURTLE_ATR_PERIOD — ATR lookback for Turtle ATR trailing stop exit.
//!   Extended sweep 10-60 step 2 (26 values) + fine 18-30 step 1 (13 values)
//!   Total: 39 values × 9 universes × 6 windows
//!
//! Design:
//!   - Fixed: EP=24, CHAND_PERIOD=7, CHAND_MULT=2.30, CAP=3, HOLD_MAX=12, ATR_ENTRY_MULT=0.00
//!   - Walk-forward: 252 train / 252 test across all 9 universes
//!   - Exports: summary CSV + equity curves for key configs

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
const HOLD_MAX: usize = 12;
const TAKER_FEE: f64 = 0.001;
const POSITION_CAP: usize = 3;
const MIN_TRADES: usize = 3;
const EP: usize = 21;
const CHAND_PERIOD: usize = 7;
const CHAND_MULT: f64 = 2.30;
const ATR_ENTRY_MULT: f64 = 0.00;
const TURTLE_ATR_MULT: f64 = 2.00;

// Extended sweep: step 2 from 10 to 60
const ATR_VALUES: &[usize] = &[
    10, 12, 14, 16, 18, 20, 22, 24, 26, 28, 30,
    32, 34, 36, 38, 40, 42, 44, 46, 48, 50, 52, 54, 56, 58, 60,
];

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

const CSV_OUT: &str = "snapshots/turtle_atr_prod_sweep.csv";
const MD_OUT: &str = "snapshots/turtle_atr_prod_sweep.md";
const CSV_EQ: &str = "snapshots/turtle_atr_prod_equity.csv";
const CSV_KEY_EQ: &str = "snapshots/turtle_atr_prod_key_equity.csv";

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

fn chandelier_exit(high: &[f64], low: &[f64], close: &[f64], period: usize, mult: f64, entry_price: f64, peak_close: f64, bar: usize) -> Option<f64> {
    let atr_val = atr_at(high, low, close, period, bar);
    let stop = peak_close - mult * atr_val;
    Some(stop)
}

fn turtle_signal(close: &[f64], high: &[f64], low: &[f64], entry_period: usize, idx: usize) -> bool {
    if idx < entry_period + 1 { return false; }
    let start = idx + 1 - entry_period;
    let mut max_close = f64::NEG_INFINITY;
    for i in start..idx {
        if let Some(&c) = close.get(i) { max_close = max_close.max(c); }
    }
    if let Some(&curr_close) = close.get(idx) {
        curr_close > max_close
    } else {
        false
    }
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

fn run_sim(
    sym_data: &HashMap<String, SymData>,
    symbols: &[String],
    atr_period: usize,
    test_start: usize,
    test_end: usize,
) -> (f64, f64, f64, usize, f64, bool, Vec<f64>) {
    let mut equity = 1.0_f64;
    let mut equity_curve = vec![1.0_f64];
    let mut wins = 0usize;
    let mut total_trades = 0usize;
    let mut daily_rets = Vec::new();
    let mut bar = test_start;

    while bar + 2 < test_end {
        // Dollar-volume ranking
        let mut scores: Vec<(&str, f64)> = Vec::new();
        for sym in symbols {
            if let Some(sd) = sym_data.get(sym) {
                if bar >= sd.close.len() { continue; }
                let vol_now = sd.vol.get(bar).copied().unwrap_or(0.0);
                let price = sd.close.get(bar).copied().unwrap_or(0.0);
                let dv = vol_now * price;
                scores.push((sym.as_str(), if dv.is_finite() && dv > 0.0 { dv } else { 0.0 }));
            }
        }
        scores.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());
        let top_syms: Vec<String> = scores.into_iter().take(POSITION_CAP).map(|(s, _)| s.to_string()).collect();

        if top_syms.is_empty() {
            equity_curve.push(equity);
            bar += 1;
            continue;
        }

        // Turtle breakout entry
        let mut entered = false;
        for sym in &top_syms {
            if let Some(sd) = sym_data.get(sym) {
                if bar >= EP + 1 && bar < sd.close.len() {
                    if turtle_signal(&sd.close, &sd.high, &sd.low, EP, bar) {
                        let entry_px = sd.close[bar];
                        let entry = entry_px * (1.0 - TAKER_FEE);
                        let entry_bar_next = bar + 1;
                        let n = sd.close.len();

                        let mut hold_bars = 0usize;
                        let mut peak_close = entry_px;
                        let mut atr_turtle_stop = entry - TURTLE_ATR_MULT * atr_at(&sd.high, &sd.low, &sd.close, atr_period, bar);
                        let mut exited = false;
                        let mut win = false;

                        for b in entry_bar_next..n.min(test_end) {
                            hold_bars += 1;
                            let curr_close = sd.close[b];
                            if curr_close > peak_close { peak_close = curr_close; }

                            // Chandelier trailing stop
                            let chand_stop = if let Some(cs) = chandelier_exit(&sd.high, &sd.low, &sd.close, CHAND_PERIOD, CHAND_MULT, entry_px, peak_close, b) {
                                Some(cs)
                            } else { None };

                            // Turtle ATR stop
                            let atr_turtle_val = atr_at(&sd.high, &sd.low, &sd.close, atr_period, b);
                            let new_turtle_stop = entry_px - TURTLE_ATR_MULT * atr_turtle_val;
                            if new_turtle_stop > atr_turtle_stop { atr_turtle_stop = new_turtle_stop; }

                            let exit = sd.low[b] < atr_turtle_stop || chand_stop.map(|cs| sd.low[b] < cs).unwrap_or(false);
                            let max_hold = hold_bars >= HOLD_MAX;

                            if exit || max_hold {
                                let exit_px = if exit { sd.high[b] } else { sd.close[b] };
                                let exit_price = exit_px * (1.0 - TAKER_FEE);
                                let ret = (exit_price - entry) / entry;
                                equity *= 1.0 + ret;
                                let prev = *daily_rets.last().unwrap_or(&1.0);
                                daily_rets.push(prev * (1.0 + ret));
                                total_trades += 1;
                                if ret > 0.0 { wins += 1; win = true; }
                                exited = true;
                                break;
                            }
                        }
                        if !exited {
                            let exit_px = sd.close[(n - 1).min(test_end - 1)];
                            let exit_price = exit_px * (1.0 - TAKER_FEE);
                            let ret = (exit_price - entry) / entry;
                            equity *= 1.0 + ret;
                            let prev = *daily_rets.last().unwrap_or(&1.0);
                            daily_rets.push(prev * (1.0 + ret));
                            total_trades += 1;
                            if ret > 0.0 { wins += 1; }
                        }
                        entered = true;
                        break;
                    }
                }
            }
        }
        equity_curve.push(equity);
        bar += 1;
    }

    let ret_pct = (equity - 1.0) * 100.0;
    let sharpe = annualised_sharpe(&daily_rets);
    let max_dd = max_dd_from(&equity_curve);
    let win_rate = if total_trades > 0 { wins as f64 / total_trades as f64 * 100.0 } else { 0.0 };
    let pass = total_trades >= MIN_TRADES && sharpe > 0.0;

    (ret_pct, sharpe, max_dd, total_trades, win_rate, pass, equity_curve)
}

#[tokio::main]
async fn main() -> Result<()> {
    let start_time = Instant::now();
    eprintln!("=== ATR_PERIOD Re-Optimization: Current Production Params ===");
    eprintln!("Params: EP={}, CHAND({},{}/{}), HM={}, ATR_EM={}, ATR_MULT={}",
        EP, CHAND_PERIOD, CHAND_MULT, 0, HOLD_MAX, ATR_ENTRY_MULT, TURTLE_ATR_MULT);
    eprintln!("ATR values: {:?} ({} values)", ATR_VALUES, ATR_VALUES.len());
    eprintln!("Universes: {} ({} symbols total)", UNIVERSES.len(), UNIVERSES.iter().map(|(_, s)| s.len()).sum::<usize>());

    // Pre-load all symbol data ONCE (before sweep loop)
    let loader = DataLoader::new(None, None);
    let mut all_syms: std::collections::HashSet<String> = std::collections::HashSet::new();
    for (_, syms) in UNIVERSES {
        for &s in *syms { all_syms.insert(s.to_string()); }
    }

    let mut raw_cache: HashMap<String, DataFrame> = HashMap::new();
    let mut min_len = usize::MAX;
    for sym in all_syms.iter() {
        match loader.fetch_with_cache(sym.as_str(), "1d", CANDLES).await {
            Ok(df) => {
                let bars = df.height();
                min_len = min_len.min(bars);
                raw_cache.insert(sym.clone(), df);
                eprintln!("  {}: {} bars", sym, bars);
            }
            Err(e) => { eprintln!("  WARN: {} load failed: {}", sym, e); }
        }
    }
    let n = min_len.min(2800);

    // Build SymData for each symbol
    let mut sym_data_map: HashMap<String, SymData> = HashMap::new();
    for sym in all_syms.iter() {
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
    eprintln!("\nLoaded {} symbols, {} bars. Starting sweep...\n", sym_data_map.len(), n);

    let mut csv_f = File::create(CSV_OUT)?;
    let _eq_f = File::create(CSV_EQ)?;
    let mut key_eq_f = File::create(CSV_KEY_EQ)?;
    writeln!(csv_f, "atr_period,universe,window,return_pct,sharpe,max_dd_pct,trades,win_rate_pct,pass")?;
    writeln!(key_eq_f, "atr_period,universe,window,bar,equity")?;

    // Key configs for detailed equity tracking
    let key_atrs: Vec<usize> = vec![16, 20, 24, 28, 32];

    let mut all_results: Vec<(usize, String, usize, f64, f64, f64, usize, f64, bool)> = Vec::new();
    let mut atr_global: HashMap<usize, (usize, f64, usize)> = HashMap::new(); // atr -> (wins, total_sharpe, total_trades)

    for atr_val in ATR_VALUES {
        for (uname, symbols) in UNIVERSES {
            let symbols: Vec<String> = symbols.iter().map(|s| s.to_string()).collect();

            // Reuse pre-loaded data
            let mut sym_data: HashMap<String, SymData> = HashMap::new();
            for sym in &symbols {
                if let Some(sd) = sym_data_map.get(sym) {
                    sym_data.insert(sym.clone(), SymData {
                        close: sd.close.clone(),
                        high: sd.high.clone(),
                        low: sd.low.clone(),
                        vol: sd.vol.clone(),
                    });
                }
            }

            let min_len_u = sym_data.values().map(|sd| sd.close.len()).min().unwrap_or(0);
            if min_len_u < TRAIN_BARS + TEST_BARS + 100 {
                continue;
            }

            let n_windows = (min_len_u - TRAIN_BARS) / TEST_BARS;

            for w in 0..n_windows {
                let test_start = TRAIN_BARS + w * TEST_BARS;
                let test_end = test_start + TEST_BARS;

                let (ret, sharpe, max_dd, trades, win_rate, pass, equity_curve) =
                    run_sim(&sym_data, &symbols, *atr_val, test_start, test_end);

                writeln!(csv_f, "{},{},{},{:.4},{:.4},{:.4},{},{:.2},{}",
                    atr_val, uname, w, ret, sharpe, max_dd, trades, win_rate, pass)?;

                // Record for global aggregation
                let entry = atr_global.entry(*atr_val).or_insert((0, 0.0, 0));
                entry.0 += if pass { 1 } else { 0 };
                entry.1 += sharpe;
                entry.2 += trades;

                all_results.push((*atr_val, uname.to_string(), w, ret, sharpe, max_dd, trades, win_rate, pass));

                // Export equity for key configs
                if key_atrs.contains(atr_val) {
                    for (bi, &eq) in equity_curve.iter().enumerate() {
                        writeln!(key_eq_f, "{},{},{},{},{:.6}", atr_val, uname, w, bi, eq)?;
                    }
                }
            }
        }
        eprintln!("  ATR={:3}: done (elapsed {:.1}s)", atr_val, start_time.elapsed().as_secs_f32());
    }

    // Compute and display top results
    let mut global_summary: Vec<(usize, usize, f64, usize, f64, f64, f64)> = Vec::new();
    for (atr, (wins, sum_sharpe, sum_trades)) in &atr_global {
        let n = all_results.iter().filter(|(a, _, _, _, _, _, _, _, _)| a == atr).count();
        let avg_sharpe = sum_sharpe / n as f64;
        let avg_trades = *sum_trades as f64 / n as f64;
        let pass_rate = *wins as f64 / n as f64 * 100.0;
        global_summary.push((*atr, *wins, avg_sharpe, n, avg_trades, pass_rate, sum_sharpe / *wins.max(&1) as f64));
    }

    global_summary.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| b.2.partial_cmp(&a.2).unwrap_or(std::cmp::Ordering::Equal)));

    eprintln!("\n=== GLOBAL RESULTS (sorted by pass rate, then Sharpe) ===");
    eprintln!("{:>6} | {:>4} | {:>7} | {:>6} | {:>7}", "ATR", "Pass", "AvgSharpe", "#Wins", "AvgTrades");
    eprintln!("{}", "-".repeat(50));
    for (atr, wins, avg_sh, n, avg_tr, pr, _) in &global_summary {
        eprintln!("{:>6} | {:>4} | {:>7.4} | {:>6} | {:>7.1}", atr, wins, avg_sh, n, avg_tr);
    }

    // Markdown report
    let mut md_f = File::create(MD_OUT)?;
    writeln!(md_f, "# ATR_PERIOD Re-Optimization — Current Production Params")?;
    writeln!(md_f, "\nGenerated: 2026-04-25")?;
    writeln!(md_f, "\nParams: EP={}, CHAND_PERIOD={}, CHAND_MULT={}, HOLD_MAX={}, ATR_ENTRY_MULT={}, TURTLE_ATR_MULT={}",
        EP, CHAND_PERIOD, CHAND_MULT, HOLD_MAX, ATR_ENTRY_MULT, TURTLE_ATR_MULT)?;
    writeln!(md_f, "\nSweep: ATR∈{:?} ({} values) × 9 universes × ~6 windows", ATR_VALUES, ATR_VALUES.len())?;
    writeln!(md_f, "\n## Global Summary (sorted by pass rate, then avg Sharpe)")?;
    writeln!(md_f, "\n| ATR | Pass | Avg Sharpe | Windows | Avg Trades | Pass% |")?;
    writeln!(md_f, "|-----|------|------------|---------|------------|-------|")?;
    for (atr, wins, avg_sh, n, avg_tr, pr, _) in &global_summary {
        writeln!(md_f, "| {:3} | {:4} | {:10.4} | {:7} | {:10.1} | {:5.1}% |", atr, wins, avg_sh, n, avg_tr, pr)?;
    }

    if let Some((win_atr, win_passes, win_sh, _, _, _, _)) = global_summary.first() {
        if let Some(baseline) = global_summary.iter().find(|(a, _, _, _, _, _, _)| *a == 24) {
            let delta_passes = *win_passes as isize - baseline.1 as isize;
            let delta_sharpe = win_sh - baseline.2;
            writeln!(md_f, "\n## Winner: ATR={}", win_atr)?;
            writeln!(md_f, "- Pass rate: {}/{} ({:.1}%)", win_passes, 54, *win_passes as f64 / 54.0 * 100.0)?;
            writeln!(md_f, "- Avg Sharpe: {:.4} ({:+.4} vs ATR=24 baseline)", win_sh, delta_sharpe)?;
            writeln!(md_f, "- Delta passes vs baseline: {:+}", delta_passes)?;
        }
    }

    eprintln!("\nTotal elapsed: {:.1}s", start_time.elapsed().as_secs_f32());
    eprintln!("Output: {} + {}", CSV_OUT, MD_OUT);
    eprintln!("Equity: {} (key configs: {:?})", CSV_EQ, key_atrs);

    Ok(())
}
