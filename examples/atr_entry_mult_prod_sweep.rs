//! ATR_ENTRY_MULT Hyperopt — Fine-Grained Sweep on Production Params
//!
//! Mission: Re-sweep ATR_ENTRY_MULT with CURRENT production params
//! (CHAND_P=11, CHAND_M=2.25, EP=24, ATR_PERIOD=24, HOLD_MAX=12)
//!
//! Prior sweep (2026-04-13, stale P=15/M=1.50/EP=21):
//!   mult=0.0 won at 52.4% pass rate
//!   Any non-zero degraded: 0.1→34.9%, 0.2→30.2%, 1.0→38.1%
//!   But that was on WRONG engine — needs re-validation with current params.
//!
//! Hypothesis: With tighter Chandelier (P=11/M=2.25) exits ~bar 12-15,
//! ATR entry filter may have a different optimal value or be equally useless.
//!
//! Sweep: mult ∈ {0.0, 0.05, 0.1, 0.15, 0.2, 0.25, 0.3, 0.4, 0.5, 0.75, 1.0}
//! 11 values × 9 universes × up to 6 windows = 594 window-runs
//! Baseline: mult=0.0 (no filter) — the confirmed winner from prior sweep

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
const HOLD_MAX: usize = 12; // hyperopt 2026-04-21: HM=12 wins +71.4% Sharpe vs HM=45 (2.72 vs 1.59). Production verified 40/54 pass (74.1%), Sharpe 4.00.
const TAKER_FEE: f64 = 0.001;
const POSITION_CAP: usize = 3; // hyperopt 2026-04-11: CAP=3 wins over CAP=2
const MIN_TRADES: usize = 3;

// Current production params (hyperopt 2026-04-20/21)
const CHAND_PERIOD: usize = 11;   // hyperopt 2026-04-20
const CHAND_MULT: f64 = 2.25;     // hyperopt 2026-04-20
const TURTLE_ENTRY: usize = 24;   // hyperopt 2026-04-20 re-opt
const TURTLE_ATR_PERIOD: usize = 24; // hyperopt 2026-04-16 fine
const TURTLE_ATR_MULT: f64 = 2.00;
const VOL_LOOKBACK: usize = 2;
const FRESHNESS_COOLDOWN: usize = 0;

const ATR_ENTRY_VALUES: &[f64] = &[
    0.0, 0.05, 0.1, 0.15, 0.2, 0.25, 0.3, 0.4, 0.5, 0.75, 1.0,
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

const OUTPUT_CSV: &str = "snapshots/atr_entry_mult_prod.csv";
const EQUITY_CSV: &str = "snapshots/atr_entry_mult_equity.csv";
const SUMMARY_CSV: &str = "snapshots/atr_entry_mult_summary.csv";


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

fn turtle_signal(close: &[f64], _high: &[f64], entry_period: usize, idx: usize) -> bool {
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

struct WfResult {
    ret: f64,
    sharpe: f64,
    max_dd: f64,
    trades: usize,
    win_rate: f64,
    pass: bool,
    equity_curve: Vec<f64>,
}

fn run_sim(
    sym_data: &HashMap<String, SymData>,
    symbols: &[String],
    test_start: usize,
    test_end: usize,
    atr_mult: f64,
) -> WfResult {
    let mut equity = 1.0_f64;
    let mut equity_curve = vec![1.0_f64];
    let mut peak = equity;
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
                let rol_vol = rolling_avg(&sd.vol, VOL_LOOKBACK, bar);
                let price = sd.close.get(bar).copied().unwrap_or(0.0);
                let dv = rol_vol * price;
                scores.push((sym.as_str(), if dv.is_finite() && dv > 0.0 { dv } else { 0.0 }));
            }
        }
        scores.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());
        let top_syms: Vec<String> = scores.into_iter().take(POSITION_CAP).map(|(s, _)| s.to_string()).collect();

        if top_syms.is_empty() {
            daily_rets.push(0.0);
            equity_curve.push(equity);
            bar += 1;
            continue;
        }

        // Turtle breakout entry with optional ATR filter
        let mut entry_price = None;
        let mut entry_sym = None;
        for sym in &top_syms {
            if let Some(sd) = sym_data.get(sym) {
                if bar >= TURTLE_ENTRY + 1 && bar < sd.close.len() {
                    // Check turtle breakout
                    let start = bar + 1 - TURTLE_ENTRY;
                    let mut max_close = f64::NEG_INFINITY;
                    for i in start..bar {
                        if let Some(&c) = sd.close.get(i) {
                            max_close = max_close.max(c);
                        }
                    }
                    if let Some(&curr_close) = sd.close.get(bar) {
                        let breakout = curr_close > max_close;
                        // ATR filter check
                        let passes_filter = if atr_mult > 0.0 && breakout {
                            let atr_val = atr_at(&sd.high, &sd.low, &sd.close, TURTLE_ATR_PERIOD, bar);
                            curr_close >= max_close + atr_mult * atr_val
                        } else {
                            breakout
                        };
                        if passes_filter {
                            entry_price = Some(curr_close);
                            entry_sym = Some(sym.clone());
                            break;
                        }
                    }
                }
            }
        }

        if let Some(ep) = entry_price {
            let esym = entry_sym.unwrap();
            let entry_bar_next = bar + 1;
            let n = sym_data.get(&esym).map(|s| s.close.len()).unwrap_or(0);

            // Track positions: Chandelier + Turtle ATR dual exit, HOLD_MAX
            let mut active = true;
            let mut tb = entry_bar_next;
            let mut highest_high_chand = sym_data.get(&esym).map(|s| s.high[entry_bar_next.min(s.high.len()-1)]).unwrap_or(0.0);
            let mut highest_high_turtle = highest_high_chand;

            while tb < test_end && active {
                if let Some(sd) = sym_data.get(&esym) {
                    if tb >= sd.close.len() { break; }

                    let close_tb = sd.close[tb];
                    let high_tb = sd.high[tb];

                    // Update highest high for both exits
                    highest_high_chand = highest_high_chand.max(high_tb);
                    highest_high_turtle = highest_high_turtle.max(high_tb);

                    // Chandelier ATR stop
                    let atr_chand = atr_at(&sd.high, &sd.low, &sd.close, CHAND_PERIOD, tb);
                    let trail_chand = highest_high_chand - CHAND_MULT * atr_chand;

                    // Turtle ATR stop
                    let atr_turtle = atr_at(&sd.high, &sd.low, &sd.close, TURTLE_ATR_PERIOD, tb);
                    let trail_turtle = (ep - TURTLE_ATR_MULT * atr_turtle).max(0.0);

                    let triggered = high_tb <= trail_chand || high_tb <= trail_turtle;
                    let hm_triggered = (tb - bar) >= HOLD_MAX;

                    if triggered || hm_triggered {
                        let exit_price = if triggered { trail_chand.min(trail_turtle).min(close_tb) } else { close_tb };
                        let gross_ret = exit_price / ep - 1.0;
                        let fee = exit_price * TAKER_FEE + ep * TAKER_FEE;
                        equity *= 1.0 + gross_ret - fee / ep;
                        wins += if gross_ret > 0.0 { 1 } else { 0 };
                        total_trades += 1;
                        if equity > peak { peak = equity; }
                        active = false;

                        let bars_held = (tb as i64 - entry_bar_next as i64).max(1) as usize;
                        let avg_daily = gross_ret / bars_held as f64;
                        for _ in 0..bars_held {
                            daily_rets.push(avg_daily);
                        }
                        equity_curve.push(equity);
                        bar = tb + 1;
                        break;
                    }
                }
                tb += 1;
            }

            if active {
                daily_rets.push(0.0);
                equity_curve.push(equity);
                bar += 1;
            }
        } else {
            daily_rets.push(0.0);
            equity_curve.push(equity);
            bar += 1;
        }
    }

    let ret = (equity - 1.0) * 100.0;
    let sharpe = annualised_sharpe(&daily_rets);
    let max_dd = max_dd_from(&equity_curve);
    let win_rate = if total_trades > 0 { wins as f64 / total_trades as f64 * 100.0 } else { 0.0 };
    let pass = total_trades >= MIN_TRADES && ret > 0.0;

    WfResult { ret, sharpe, max_dd, trades: total_trades, win_rate, pass, equity_curve }
}

#[tokio::main]
async fn main() -> Result<()> {
    let t0 = Instant::now();
    println!("ATR_ENTRY_MULT Hyperopt — Production Params");
    println!("CHAND(11,2.25)/EP=24/ATR(24)/HM=12");
    println!("Sweep: {:?}", ATR_ENTRY_VALUES);
    println!("Expected: 11 mults × 9 universes × ~6 windows = ~594 window-runs
");

    let loader = DataLoader::new(None, None);

    // Pre-load all symbol data
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
            let col_vec = |name: &str| -> Vec<f64> {
                let chunked = df.column(name).unwrap().f64().unwrap();
                chunked.into_iter().filter_map(|x| x).take(n_min).collect::<Vec<_>>()
            };
            sym_data_map.insert(sym.clone(), SymData {
                close: col_vec("close"),
                high:  col_vec("high"),
                low:   col_vec("low"),
                vol:   col_vec("volume"),
            });
        }
    }
    println!("Loaded {} symbols, {} bars
", sym_data_map.len(), n);

    let mut out_csv = File::create(OUTPUT_CSV)?;
    writeln!(out_csv, "mult,universe,window,return_pct,sharpe,max_dd_pct,trades,win_rate_pct,pass")?;

    let mut equity_csv = File::create(EQUITY_CSV)?;
    writeln!(equity_csv, "mult,universe,window,bar,equity")?;

    let mut summary: Vec<(f64, f64, f64, usize, usize)> = Vec::new();

    for &mult in ATR_ENTRY_VALUES {
        let mut mult_pass = 0usize;
        let mut mult_sharpes = vec![];
        let mut mult_returns = vec![];

        for &(uname, symbols) in UNIVERSES {
            let symbols: Vec<String> = symbols.iter().map(|s| s.to_string()).collect();
            let all_loaded = symbols.iter().all(|s| sym_data_map.contains_key(s));
            if !all_loaded { eprintln!("{:>20} SKIPPED (missing data)", uname); continue; }

            let total_windows = n.saturating_sub(TRAIN_BARS + TEST_BARS) / TEST_BARS;
            if total_windows == 0 { continue; }

            for wi in 0..total_windows {
                let train_end = TRAIN_BARS + wi * TEST_BARS;
                let test_start = train_end;
                let test_end = (test_start + TEST_BARS).min(n);
                if test_end.saturating_sub(test_start) < 5 { continue; }

                let r = run_sim(&sym_data_map, &symbols, test_start, test_end, mult);
                let eq_curve = r.equity_curve;
                let pass_str = if r.pass { "true" } else { "false" };
                writeln!(out_csv, "{},{},{},{:.2},{:.4},{:.2},{},{:.2},{}",
                    mult, uname, wi, r.ret, r.sharpe, r.max_dd, r.trades, r.win_rate, pass_str)?;

                if r.pass { mult_pass += 1; }
                for (bi, &eq) in eq_curve.iter().enumerate() {
                    if bi % 5 == 0 {
                        writeln!(equity_csv, "{},{},{},{},{:.6}", mult, uname, wi, bi, eq)?;
                    }
                }
                mult_sharpes.push(r.sharpe);
                mult_returns.push(r.ret);
            }
        }

        let avg_sharpe = mult_sharpes.iter().sum::<f64>() / mult_sharpes.len().max(1) as f64;
        let avg_return = mult_returns.iter().sum::<f64>() / mult_returns.len().max(1) as f64;
        let mult_total = UNIVERSES.len() * 6;
        summary.push((mult, avg_sharpe, avg_return, mult_pass, mult_total));

        let pass_rate = mult_pass as f64 / mult_total as f64 * 100.0;
        println!("  mult={:.2}  avg_sharpe={:.4}  avg_ret={:.2}%  pass={}/{} ({:.1}%)",
            mult, avg_sharpe, avg_return, mult_pass, mult_total, pass_rate);
    }

    let mut sum_csv = File::create(SUMMARY_CSV)?;
    writeln!(sum_csv, "mult,avg_sharpe,avg_return,pass_count,total_runs,pass_rate_pct")?;
    summary.sort_by(|a, b| {
        let cmp_pass = b.3.cmp(&a.3);
        if cmp_pass != std::cmp::Ordering::Equal { return cmp_pass; }
        b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal)
    });
    for (mult, avg_sh, avg_ret, pass, total) in &summary {
        let pass_rate = *pass as f64 / (*total as f64) * 100.0;
        writeln!(sum_csv, "{:.2},{:.4},{:.2},{},{},{:.2}", mult, avg_sh, avg_ret, pass, total, pass_rate)?;
    }

    println!("\n========== SUMMARY (sorted by pass rate, then Sharpe) ==========");
    println!("{:<8} {:<12} {:<12} {:<8} {:<8}", "mult", "avg_sharpe", "avg_return", "pass", "pass%");
    println!("{}", "-".repeat(50));
    for (mult, avg_sh, avg_ret, pass, total) in &summary {
        let pass_rate = *pass as f64 / (*total as f64) * 100.0;
        println!("{:.2}     {:.4}        {:.2}%        {}/{}      {:.1}%", mult, avg_sh, avg_ret, pass, total, pass_rate);
    }

    println!("\nTotal runtime: {:.1}s", t0.elapsed().as_secs_f64());
    println!("Output: {}", OUTPUT_CSV);
    println!("Equity: {}", EQUITY_CSV);
    println!("Summary: {}", SUMMARY_CSV);

    Ok(())
}
