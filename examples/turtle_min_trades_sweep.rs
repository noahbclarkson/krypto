//! =========================================================
//! HYPERPARAMETER OPTIMIZATION: MIN_TRADES
//! =========================================================
//!
//! Background:
//!   MIN_TRADES is a hardcoded threshold: a walk-forward window must have
//!   >= MIN_TRADES trades to be considered valid (pass/fail filter).
//!   Currently set to 3 with NO documented justification.
//!   Affects pass/fail determination AND whether a window contributes equity.
//!
//! Target: MIN_TRADES ∈ {1, 2, 3, 5, 7, 10, 15, 20}
//!   - MIN_TRADES=1:   all windows qualify (no filter)
//!   - MIN_TRADES=10:  thin windows with <10 trades excluded
//!   - MIN_TRADES=20:  very few windows qualify
//!
//! Strategy: Turtle+Chandelier DUAL_EXIT (current best config)
//!   EP=21, Chandelier(28,2.0), Turtle_ATR(25,2.0), CAP=3, HM=45
//!   Walk-forward: 252 train / 252 test, all 9 universes

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
const EP: usize = 21;
const CHAND_PERIOD: usize = 28;
const CHAND_MULT: f64 = 2.00;
const TURTLE_ATR_PERIOD: usize = 24;
const TURTLE_ATR_MULT: f64 = 2.00;

// MIN_TRADES values to sweep
const MIN_TRADES_VALUES: &[usize] = &[1, 2, 3, 5, 7, 10, 15, 20];

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

const CSV_OUT: &str = "snapshots/turtle_min_trades_sweep.csv";

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

fn turtle_signal(close: &[f64], entry_period: usize, idx: usize) -> bool {
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

fn run_sim_at_min_trades(
    sym_data: &HashMap<String, SymData>,
    symbols: &[String],
    test_start: usize,
    test_end: usize,
    min_trades: usize,
) -> (f64, f64, f64, usize, f64, bool) {
    let mut equity = 1.0_f64;
    let mut wins = 0usize;
    let mut total_trades = 0usize;
    let mut daily_rets = Vec::new();

    let mut bar = test_start;
    while bar + 2 < test_end {
        let mut scores: Vec<(&str, f64)> = Vec::new();
        for sym in symbols {
            if let Some(sd) = sym_data.get(sym) {
                if bar >= sd.close.len() { continue; }
                let dv = sd.vol.get(bar).copied().unwrap_or(0.0)
                    * sd.close.get(bar).copied().unwrap_or(0.0);
                scores.push((sym.as_str(), if dv.is_finite() && dv > 0.0 { dv } else { 0.0 }));
            }
        }
        scores.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());
        let top_syms: Vec<String> = scores.into_iter().take(POSITION_CAP).map(|(s, _)| s.to_string()).collect();

        if top_syms.is_empty() {
            bar += 1;
            continue;
        }

        let mut entered = false;
        for sym in &top_syms {
            if let Some(sd) = sym_data.get(sym) {
                if bar >= EP + 1 && bar < sd.close.len() {
                    if turtle_signal(&sd.close, EP, bar) {
                        let entry_px = sd.close[bar];
                        let entry = entry_px * (1.0 - TAKER_FEE);
                        let entry_bar_next = bar + 1;
                        let n = sd.close.len();

                        let mut highest_high_chand = sd.high[entry_bar_next];
                        let mut highest_high_turtle = sd.high[entry_bar_next];
                        let max_bar = (entry_bar_next + HOLD_MAX).min(n.saturating_sub(1));
                        let mut exit_bar = max_bar;
                        for b in entry_bar_next..=max_bar.min(n.saturating_sub(1)) {
                            highest_high_chand = highest_high_chand.max(sd.high[b]);
                            let atr_chand = atr_at(&sd.high, &sd.low, &sd.close, CHAND_PERIOD, b);
                            let trail_chand = highest_high_chand - CHAND_MULT * atr_chand;
                            highest_high_turtle = highest_high_turtle.max(sd.high[b]);
                            let atr_turtle = atr_at(&sd.high, &sd.low, &sd.close, TURTLE_ATR_PERIOD, b);
                            let trail_turtle = highest_high_turtle - TURTLE_ATR_MULT * atr_turtle;
                            if sd.close[b] < trail_chand || sd.close[b] < trail_turtle {
                                exit_bar = b;
                                break;
                            }
                        }

                        if let Some(&exit_px) = sd.close.get(exit_bar) {
                            let exit = exit_px * (1.0 - TAKER_FEE);
                            let gross_ret = exit / entry - 1.0;
                            wins += if gross_ret > 0.0 { 1 } else { 0 };
                            total_trades += 1;
                            equity *= 1.0 + gross_ret;
                            let bars_held = (exit_bar as i64 - entry_bar_next as i64).max(1) as usize;
                            let avg_daily = gross_ret / bars_held as f64;
                            for _ in 0..bars_held {
                                daily_rets.push(avg_daily);
                            }
                            entered = true;
                            break;
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
    let max_dd = 0.0_f64; // not needed for MIN_TRADES sweep
    let win_rate = if total_trades > 0 { wins as f64 / total_trades as f64 * 100.0 } else { 0.0 };
    let pass = total_trades >= min_trades && ret > 0.0;

    (ret, sharpe, max_dd, total_trades, win_rate, pass)
}

#[tokio::main]
async fn main() -> Result<()> {
    let t0 = Instant::now();
    eprintln!("=== MIN_TRADES Sweep ===");
    eprintln!("Values: {:?}", MIN_TRADES_VALUES);

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

    // Pre-compute results for each (universe, window) at MIN_TRADES=3 (current default)
    // Then re-evaluate pass/fail for each MIN_TRADES value
    // This is more efficient than re-running the full simulation
    
    // Actually, we need to re-run because the pass/fail condition depends on min_trades
    // But we can reuse the equity calculations
    // Let's just run the full simulation for each MIN_TRADES value
    
    let mut csv_lines = vec!["min_trades,universe,window,return_pct,sharpe,max_dd_pct,trades,win_rate_pct,pass".to_string()];

    for &min_t in MIN_TRADES_VALUES {
        eprintln!("\n===== MIN_TRADES={} =====", min_t);
        let mut global_pass = 0usize;
        let mut global_total = 0usize;
        let mut global_trades = 0usize;
        let mut global_sharpe_sum = 0.0_f64;
        let mut global_ret_sum = 0.0_f64;

        for &(label, symbols) in UNIVERSES {
            let symbols: Vec<String> = symbols.iter().map(|s| s.to_string()).collect();
            if !symbols.iter().all(|s| sym_data_map.contains_key(s)) { continue; }

            let total_windows = n.saturating_sub(TRAIN_BARS + TEST_BARS) / TEST_BARS;
            if total_windows == 0 { continue; }

            let mut uni_pass = 0usize;
            let mut uni_total = 0usize;

            for wi in 0..total_windows {
                let test_start = TRAIN_BARS + wi * TEST_BARS;
                let test_end = (test_start + TEST_BARS).min(n);
                if test_end.saturating_sub(test_start) < 5 { continue; }

                let (ret, sh, dd, trades, wr, pass) =
                    run_sim_at_min_trades(&sym_data_map, &symbols, test_start, test_end, min_t);

                csv_lines.push(format!("{},{},{},{:.2},{:.4},{:.2},{},{:.2},{}",
                    min_t, label, wi, ret, sh, dd, trades, wr, pass));

                uni_total += 1;
                global_total += 1;
                global_trades += trades;
                global_sharpe_sum += sh;
                global_ret_sum += ret;
                if pass { global_pass += 1; uni_pass += 1; }
            }

            eprintln!("  {}: {}/{} pass ({:.0}%)", label, uni_pass, uni_total, uni_pass as f64/uni_total as f64*100.0);
        }

        let avg_sh = global_sharpe_sum / global_total as f64;
        let avg_ret = global_ret_sum / global_total as f64;
        let pass_pct = global_pass as f64 / global_total as f64 * 100.0;
        eprintln!("  GLOBAL: {}/{} pass ({:.1}%), Sharpe={:.4}, Ret={:.1}, {} trades",
            global_pass, global_total, pass_pct, avg_sh, avg_ret, global_trades);
    }

    let mut f = File::create(CSV_OUT)?;
    for line in &csv_lines { writeln!(f, "{}", line)?; }
    eprintln!("\nWrote: {}", CSV_OUT);

    // Compute summary
    let mut summary_lines = vec!["min_trades,global_pass,global_total,pass_pct,avg_sharpe,avg_ret,total_trades".to_string()];
    for &min_t in MIN_TRADES_VALUES {
        let rows: Vec<_> = csv_lines.iter().skip(1)
            .filter(|l| l.starts_with(&format!("{},", min_t)))
            .collect();
        let g_pass = rows.iter().filter(|l| l.contains(",true")).count();
        let g_total = rows.len();
        let g_sh: f64 = rows.iter().filter_map(|l| l.split(',').nth(4)).filter_map(|s| s.parse::<f64>().ok()).sum::<f64>() / g_total as f64;
        let g_ret: f64 = rows.iter().filter_map(|l| l.split(',').nth(3)).filter_map(|s| s.parse::<f64>().ok()).sum::<f64>() / g_total as f64;
        let g_trades: usize = rows.iter().filter_map(|l| l.split(',').nth(6)).filter_map(|s| s.parse::<usize>().ok()).sum();
        summary_lines.push(format!("{},{},{},{:.2},{:.4},{:.1},{}",
            min_t, g_pass, g_total, g_pass as f64/g_total as f64*100.0, g_sh, g_ret, g_trades));
    }

    eprintln!("\n=== Summary ===");
    eprintln!("min_trades | pass_pct | avg_sharpe | avg_ret | trades");
    for line in summary_lines.iter().skip(1) {
        eprintln!("  {}", line);
    }

    let mut sf = File::create("snapshots/turtle_min_trades_summary.csv")?;
    for line in &summary_lines { writeln!(sf, "{}", line)?; }
    eprintln!("Wrote: snapshots/turtle_min_trades_summary.csv");
    eprintln!("Done in {:.1}s", t0.elapsed().as_secs_f64());

    Ok(())
}
