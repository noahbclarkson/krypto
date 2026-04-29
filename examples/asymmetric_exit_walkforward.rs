//! T27: Asymmetric Exit Architecture Walk-Forward
//!
//! HYPOTHESIS: Trend-following payoff is convex — big wins, small losses.
//! Asymmetric exits: cut losers FAST (ATR×0.5 hard stop), let winners RUN (Chandelier×3.0).
//!
//! 3 configs tested:
//!   A: Baseline — Chandelier(7,2.30) + TurtleATR(24,2.0) dual exit [current production]
//!   B: Asym hard+soft — ATR×0.5 hard stop + Chandelier(7,3.0) + TurtleATR(24,2.0)
//!   C: Asym hard-only — ATR×0.5 hard stop + TurtleATR(24,2.0) [no Chandelier]
//!
//! All use: EP=21, ATR_P=24, ATR_M=2.0, HM=12, CAP=3, EM=0.00, VL=8

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

// Shared params
const TURTLE_ENTRY: usize = 21;
const TURTLE_ATR_PERIOD: usize = 24;
const TURTLE_ATR_MULT: f64 = 2.00;
const HOLD_MAX: usize = 12;
const ATR_ENTRY_MULT: f64 = 0.00;
const POSITION_CAP: usize = 3;
const VOL_LOOKBACK: usize = 8;
const TAKER_FEE: f64 = 0.001;
const MIN_TRADES: usize = 3;

// Chandelier params per config
const CHAND_P_BASELINE: usize = 7;
const CHAND_M_BASELINE: f64 = 2.30;
const CHAND_M_LOOSE: f64 = 3.00; // for asym hard+soft

// Hard stop: ATR×0.5
const HARD_STOP_MULT: f64 = 0.50;

// Configs
// A = 0, B = 1, C = 2
const CONFIGS: &[(&str, usize, f64)] = &[
    ("baseline",     CHAND_P_BASELINE, CHAND_M_BASELINE), // A
    ("asym_soft",    CHAND_P_BASELINE, CHAND_M_LOOSE),     // B
    ("asym_hard",    CHAND_P_BASELINE, CHAND_M_BASELINE),  // C (special handling)
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

const CSV_OUT: &str = "snapshots/asymmetric_exit_results.csv";
const EQUITY_OUT: &str = "snapshots/asymmetric_exit_equity.csv";

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

fn turtle_signal(c: &[f64], _h: &[f64], _l: &[f64], ep: usize, _ap: usize, _am: f64, idx: usize) -> bool {
    // Note: ATR_ENTRY_MULT = 0.00 always, so just check breakout
    if idx < ep + 1 { return false; }
    let start = idx + 1 - ep;
    let mut mx = f64::NEG_INFINITY;
    for i in start..idx {
        if let Some(&cv) = c.get(i) { mx = mx.max(cv); }
    }
    c.get(idx).map(|&cv| cv > mx).unwrap_or(false)
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

struct SimResult {
    ret: f64,
    sharpe: f64,
    max_dd: f64,
    trades: usize,
    win_rate: f64,
    pass: bool,
    equity_curve: Vec<f64>,
}

/// config_id: 0=baseline, 1=asym_soft, 2=asym_hard
fn run_sim(
    sym_data: &HashMap<String, SymData>,
    symbols: &[String],
    test_start: usize,
    test_end: usize,
    config_id: usize,
) -> SimResult {
    let (chand_p, chand_m) = match config_id {
        0 => (CHAND_P_BASELINE, CHAND_M_BASELINE),
        1 => (CHAND_P_BASELINE, CHAND_M_LOOSE),
        2 => (0, 0.0), // asym_hard: no Chandelier
        _ => (CHAND_P_BASELINE, CHAND_M_BASELINE),
    };
    let has_chandelier = config_id != 2;

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
                let rol_vol = rolling_avg(&sd.vol, VOL_LOOKBACK, bar);
                let price = *sd.close.get(bar).unwrap_or(&0.0);
                let dv = rol_vol * price;
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

        // Entry — match authoritative turtle_chandelier_walkforward.rs:
        // at most ONE new trade per bar from the top dollar-volume candidates.
        let mut entered = false;
        if equity > 0.0 {
            for sym in &top_syms {
                if equity <= 0.0 || entered { break; }
                let sd = match sym_data.get(sym) {
                    Some(d) => d,
                    None => continue,
                };
                if bar >= TURTLE_ENTRY + 1 && bar < sd.close.len() {
                    if turtle_signal(&sd.close, &sd.high, &sd.low, TURTLE_ENTRY, TURTLE_ATR_PERIOD, ATR_ENTRY_MULT, bar) {
                        let entry_px = sd.close[bar];
                        let entry = entry_px * (1.0 - TAKER_FEE);
                        let entry_bar_next = bar + 1;
                        let n = sd.close.len();

                        // Compute hard stop at entry (fixed: ATR × HARD_STOP_MULT below entry)
                        let entry_atr = atr_at(&sd.high, &sd.low, &sd.close, TURTLE_ATR_PERIOD, bar);
                        let hard_stop_px = entry_px - HARD_STOP_MULT * entry_atr;

                        // Dual exit with asymmetric stops
                        let mut highest_high_chand = sd.high[entry_bar_next];
                        let mut lowest_low_turtle = sd.low[entry_bar_next];
                        let max_bar = (entry_bar_next + HOLD_MAX).min(n.saturating_sub(1));
                        let mut exit_bar = max_bar;
                        for b in entry_bar_next..=max_bar.min(n.saturating_sub(1)) {
                            let curr_close = sd.close[b];

                            // Hard stop (fires immediately)
                            if curr_close < hard_stop_px {
                                exit_bar = b;
                                break;
                            }

                            if has_chandelier {
                                // Looser Chandelier trailing stop
                                highest_high_chand = highest_high_chand.max(sd.high[b]);
                                let atr_chand = atr_at(&sd.high, &sd.low, &sd.close, chand_p, b);
                                let trail_chand = highest_high_chand - chand_m * atr_chand;
                                if curr_close < trail_chand {
                                    exit_bar = b;
                                    break;
                                }
                            }

                            // Turtle ATR trailing stop
                            lowest_low_turtle = lowest_low_turtle.min(sd.low[b]);
                            let atr_turtle = atr_at(&sd.high, &sd.low, &sd.close, TURTLE_ATR_PERIOD, b);
                            let trail_turtle = lowest_low_turtle - TURTLE_ATR_MULT * atr_turtle;
                            if curr_close < trail_turtle {
                                exit_bar = b;
                                break;
                            }
                        }

                        if let Some(&exit_px) = sd.close.get(exit_bar) {
                            let exit = exit_px * (1.0 - TAKER_FEE);
                            let gross_ret = exit / entry - 1.0;
                            let bars_held = (exit_bar as i64 - entry_bar_next as i64).max(1) as usize;
                            wins += if gross_ret > 0.0 { 1 } else { 0 };
                            total_trades += 1;
                            equity *= 1.0 + gross_ret;

                            let avg_daily = gross_ret / bars_held as f64;
                            for _ in 0..bars_held { daily_rets.push(avg_daily); }
                            equity_curve.push(equity);
                            bar = exit_bar + 1;
                            entered = true;
                            break;
                        }
                    }
                }
            }
        }

        if !entered {
            equity_curve.push(equity);
            bar += 1;
        }
    }

    let final_ret = (equity - 1.0) * 100.0;
    let sharpe = annualised_sharpe(&daily_rets);
    let max_dd_val = max_dd(&equity_curve);
    let win_rate = if total_trades > 0 { wins as f64 / total_trades as f64 * 100.0 } else { 0.0 };

    SimResult {
        ret: final_ret,
        sharpe,
        max_dd: max_dd_val,
        trades: total_trades,
        win_rate,
        pass: total_trades >= MIN_TRADES && equity > 1.0,
        equity_curve,
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    let t0 = Instant::now();

    eprintln!("=== T27: Asymmetric Exit Architecture Walk-Forward ===");
    eprintln!("Configs: A=baseline CHAND(7,2.30), B=asym_soft CHAND(7,3.0), C=asym_hard [no Chandelier]");
    eprintln!("HARD_STOP_MULT: {} (losers cut below entry)", HARD_STOP_MULT);
    eprintln!("Shared: EP={}, ATR({},{}), HM={}, CAP={}",
        TURTLE_ENTRY, TURTLE_ATR_PERIOD, TURTLE_ATR_MULT, HOLD_MAX, POSITION_CAP);
    eprintln!();

    let loader = DataLoader::new(None, None);
    let mut all_syms: std::collections::HashSet<String> = std::collections::HashSet::new();
    for (_, syms) in UNIVERSES {
        for &s in *syms { all_syms.insert(s.to_string()); }
    }

    // Load data
    let mut raw_cache: HashMap<String, DataFrame> = HashMap::new();
    let mut min_len = usize::MAX;
    for sym in all_syms.iter() {
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

    let mut all_results: Vec<(String, usize, usize, f64, f64, f64, usize, f64, bool)> = Vec::new();
    let mut equity_cols: HashMap<String, Vec<f64>> = HashMap::new();

    for &(uname, symbols) in UNIVERSES {
        let symbols: Vec<String> = symbols.iter().map(|s| s.to_string()).collect();
        let all_loaded = symbols.iter().all(|s| sym_data_map.contains_key(s));
        if !all_loaded {
            eprintln!("{:>20} SKIPPED (missing)", uname);
            continue;
        }

        let total_windows = n.saturating_sub(TRAIN_BARS + TEST_BARS) / TEST_BARS;
        if total_windows == 0 {
            eprintln!("{:>20} SKIPPED (no data)", uname);
            continue;
        }

        eprintln!("==== {:<18} ==== {} syms, {} windows", uname, symbols.len(), total_windows);

        for wi in 0..total_windows {
            let train_end = TRAIN_BARS + wi * TEST_BARS;
            let test_start = train_end;
            let test_end = (test_start + TEST_BARS).min(n);
            if test_end.saturating_sub(test_start) < 5 { continue; }

            for (config_id, (config_name, _, _)) in CONFIGS.iter().enumerate() {
                let r = run_sim(&sym_data_map, &symbols, test_start, test_end, config_id);

                let col_key = format!("{}_W{}_{}", uname, wi, config_name);
                equity_cols.insert(col_key, r.equity_curve.clone());

                all_results.push((
                    uname.to_string(), wi, config_id,
                    r.ret, r.sharpe, r.max_dd, r.trades, r.win_rate, r.pass,
                ));

                let status = if r.pass { "PASS" } else { "FAIL" };
                eprintln!(
                    "  W{} cfg={:12}: {} | {:+.2}% | Sharpe {:.3} | DD {:.2}% | {} trades",
                    wi, config_name, status, r.ret, r.sharpe, r.max_dd, r.trades
                );
            }
        }
    }

    // Aggregate by config
    eprintln!("\n\n=== AGGREGATE BY CONFIG ===");
    let mut config_summary: HashMap<usize, (usize, usize, f64, f64, f64, usize)> = HashMap::new();
    for (_, _, config_id, ret, sharpe, max_dd, trades, _, pass) in &all_results {
        let e = config_summary.entry(*config_id).or_insert((0, 0, 0.0, 0.0, 0.0, 0));
        e.0 += if *pass { 1 } else { 0 };
        e.1 += 1;
        e.2 += ret;
        e.3 += sharpe;
        e.4 += max_dd;
        e.5 += *trades;
    }

    eprintln!("\n{:<14} {:>6} {:>10} {:>10} {:>10} {:>10}",
             "Config", "PassRate", "AvgRet%", "AvgSharpe", "AvgDD%", "Trades");
    let mut rows: Vec<(usize, &str, usize, usize, f64, f64, f64, usize)> = Vec::new();
    for (config_id, (config_name, _, _)) in CONFIGS.iter().enumerate() {
        if let Some((pass, total, sum_ret, sum_sharpe, sum_dd, sum_trades)) = config_summary.get(&config_id) {
            let avg_ret = sum_ret / *total as f64;
            let avg_sharpe = sum_sharpe / *total as f64;
            let avg_dd = sum_dd / *total as f64;
            let pass_rate = *pass as f64 / *total as f64 * 100.0;
            eprintln!("{:<14} {:>3}/{:.<3}({:5.1}%) {:>10.2}% {:>10.4} {:>10.2} {:>10}",
                config_name, pass, total, pass_rate, avg_ret, avg_sharpe, avg_dd, sum_trades);
            rows.push((config_id, *config_name, *pass, *total, avg_ret, avg_sharpe, avg_dd, *sum_trades));
        }
    }

    // Winner: robustness-first
    let best = rows.iter()
        .max_by(|a, b| {
            let pa = a.2 as f64 / a.3 as f64;
            let pb = b.2 as f64 / b.3 as f64;
            match pa.partial_cmp(&pb).unwrap() {
                std::cmp::Ordering::Equal => a.5.partial_cmp(&b.5).unwrap(),
                o => o,
            }
        })
        .map(|(id, name, ..)| (*id, *name))
        .unwrap_or((0, "baseline"));
    eprintln!("\nWINNER (robustness-first): config_id={} ({})", best.0, best.1);

    // Write results CSV
    let mut f = File::create(CSV_OUT)?;
    writeln!(f, "universe,window,config,config_id,return_pct,sharpe,max_dd_pct,trades,win_rate,pass")?;
    for (uname, w, config_id, ret, sharpe, max_dd, trades, win_rate, pass) in &all_results {
        let config_name = CONFIGS[*config_id].0;
        writeln!(f, "{},{},{},{},{:.4},{:.6},{:.4},{},{:.2},{}",
            uname, w, config_name, config_id, ret, sharpe, max_dd, trades, win_rate, if *pass { 1 } else { 0 })?;
    }
    eprintln!("\nResults CSV -> {}", CSV_OUT);

    // Write equity CSV
    let max_len = equity_cols.values().map(|v| v.len()).max().unwrap_or(0);
    let mut f_eq = File::create(EQUITY_OUT)?;
    let mut keys: Vec<&String> = equity_cols.keys().collect();
    keys.sort();
    writeln!(f_eq, "step,{}", keys.iter().map(|k| k.as_str()).collect::<Vec<_>>().join(","))?;
    for step in 0..max_len {
        write!(f_eq, "{}", step)?;
        for k in &keys {
            if let Some(v) = equity_cols.get(*k).and_then(|v| v.get(step)) {
                write!(f_eq, ",{:.6}", v)?;
            } else {
                write!(f_eq, ",")?;
            }
        }
        writeln!(f_eq)?;
    }
    eprintln!("Equity CSV -> {}", EQUITY_OUT);

    eprintln!("\nDone in {:.1}s", t0.elapsed().as_secs_f64());
    Ok(())
}
