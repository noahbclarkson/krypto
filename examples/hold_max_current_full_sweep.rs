//! HOLD_MAX current-production full-range hyperopt (2026-04-30)
//!
//! Parameter audited: HOLD_MAX, a live/HOF production constant.
//! Rationale: HOLD_MAX=12 is documented from older optimization context; this reruns the
//! full logical integer range under current production params:
//! EP=21, Chandelier(7,2.30), TurtleATR(24,2.0), ATR_ENTRY_MULT=0.00,
//! VOL_LOOKBACK=8, CAP=3.
//!
//! Sweep: HOLD_MAX 1..=100 step 1 × 9 universes × walk-forward windows.
//! Outputs:
//! - snapshots/hold_max_current_full_sweep.csv
//! - snapshots/hold_max_current_full_summary.csv
//! - snapshots/hold_max_current_full_equity.csv

use anyhow::Result;
use krypto::data::loader::DataLoader;
use polars::prelude::DataFrame;
use std::collections::{HashMap, HashSet};
use std::fs::File;
use std::io::Write;
use std::time::Instant;

const CANDLES: u32 = 3000;
const TRAIN_BARS: usize = 252;
const TEST_BARS: usize = 252;
const HOLD_BASELINE: usize = 12;
const TAKER_FEE: f64 = 0.001;
const POSITION_CAP: usize = 3;
const MIN_TRADES: usize = 3;
const CHAND_PERIOD: usize = 7;
const CHAND_MULT: f64 = 2.30;
const TURTLE_ENTRY: usize = 21;
const TURTLE_ATR_PERIOD: usize = 24;
const TURTLE_ATR_MULT: f64 = 2.00;
const ATR_ENTRY_MULT: f64 = 0.00;
const VOL_LOOKBACK: usize = 8;

const CSV_METRICS: &str = "snapshots/hold_max_current_full_sweep.csv";
const CSV_SUMMARY: &str = "snapshots/hold_max_current_full_summary.csv";
const CSV_EQUITY: &str = "snapshots/hold_max_current_full_equity.csv";

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

#[derive(Clone)]
struct SymData {
    close: Vec<f64>,
    high: Vec<f64>,
    low: Vec<f64>,
    vol: Vec<f64>,
}

struct WfResult {
    ret: f64,
    sharpe: f64,
    max_dd: f64,
    trades: usize,
    win_rate: f64,
    pass: bool,
    equity_final: f64,
    equity_curve: Vec<f64>,
}

fn atr_at(high: &[f64], low: &[f64], close: &[f64], period: usize, idx: usize) -> f64 {
    if idx < period { return 0.0; }
    let mut sum = 0.0;
    for i in (idx + 1 - period)..=idx {
        let h = high.get(i).copied().unwrap_or(0.0);
        let l = low.get(i).copied().unwrap_or(0.0);
        let c0 = close.get(i.saturating_sub(1)).copied().unwrap_or(0.0);
        sum += (h - l).max((h - c0).abs()).max((l - c0).abs());
    }
    sum / period as f64
}

fn rolling_avg(vals: &[f64], window: usize, idx: usize) -> f64 {
    if vals.is_empty() { return 0.0; }
    if idx < window { return vals.get(idx).copied().unwrap_or(0.0); }
    vals[idx + 1 - window..=idx].iter().sum::<f64>() / window as f64
}

fn turtle_signal(close: &[f64], high: &[f64], low: &[f64], idx: usize) -> bool {
    if idx < TURTLE_ENTRY + 1 { return false; }
    let start = idx + 1 - TURTLE_ENTRY;
    let mut max_close = f64::NEG_INFINITY;
    for i in start..idx {
        max_close = max_close.max(close.get(i).copied().unwrap_or(f64::NEG_INFINITY));
    }
    let curr_close = close.get(idx).copied().unwrap_or(0.0);
    let breakout = curr_close > max_close;
    if breakout && ATR_ENTRY_MULT > 0.0 {
        let atr_val = atr_at(high, low, close, TURTLE_ATR_PERIOD, idx);
        curr_close >= max_close + ATR_ENTRY_MULT * atr_val
    } else {
        breakout
    }
}

fn annualised_sharpe(daily_rets: &[f64]) -> f64 {
    if daily_rets.len() < 2 { return 0.0; }
    let mn = daily_rets.iter().sum::<f64>() / daily_rets.len() as f64;
    let sd = (daily_rets.iter().map(|x| (x - mn).powi(2)).sum::<f64>() / daily_rets.len() as f64).sqrt();
    if sd == 0.0 { return 0.0; }
    mn * 365.0_f64.sqrt() / sd
}

fn max_dd_from(equity: &[f64]) -> f64 {
    let mut peak = f64::NEG_INFINITY;
    let mut max_dd = 0.0_f64;
    for &e in equity {
        peak = peak.max(e);
        if peak > 0.0 {
            max_dd = max_dd.max((peak - e) / peak);
        }
    }
    max_dd * 100.0
}

fn run_sim(
    sym_data: &HashMap<String, SymData>,
    symbols: &[String],
    test_start: usize,
    test_end: usize,
    hold_max: usize,
) -> WfResult {
    let mut equity = 1.0_f64;
    let mut equity_curve = vec![1.0_f64];
    let mut wins = 0usize;
    let mut total_trades = 0usize;
    let mut daily_rets = Vec::new();

    let mut bar = test_start;
    while bar + 2 < test_end {
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
            equity_curve.push(equity);
            bar += 1;
            continue;
        }

        let mut entered = false;
        for sym in &top_syms {
            if let Some(sd) = sym_data.get(sym) {
                if bar >= TURTLE_ENTRY + 1 && bar < sd.close.len() && turtle_signal(&sd.close, &sd.high, &sd.low, bar) {
                    let entry_px = sd.close[bar];
                    let entry = entry_px * (1.0 - TAKER_FEE);
                    let entry_bar_next = bar + 1;
                    let n = sd.close.len();
                    if entry_bar_next >= n { continue; }

                    let mut highest_high_chand = sd.high[entry_bar_next];
                    let mut lowest_low_turtle = sd.low[entry_bar_next];
                    // Match the authoritative turtle_chandelier_walkforward harness: an entry
                    // near the end of a test window may exit after test_end if HOLD_MAX/stop
                    // carries it forward. Do not clamp max_bar to test_end here.
                    let max_bar = (entry_bar_next + hold_max).min(n.saturating_sub(1));
                    let mut exit_bar = max_bar;

                    for b in entry_bar_next..=max_bar {
                        highest_high_chand = highest_high_chand.max(sd.high[b]);
                        let atr_chand = atr_at(&sd.high, &sd.low, &sd.close, CHAND_PERIOD, b);
                        let trail_chand = highest_high_chand - CHAND_MULT * atr_chand;

                        lowest_low_turtle = lowest_low_turtle.min(sd.low[b]);
                        let atr_turtle = atr_at(&sd.high, &sd.low, &sd.close, TURTLE_ATR_PERIOD, b);
                        let trail_turtle = lowest_low_turtle - TURTLE_ATR_MULT * atr_turtle;

                        if sd.close[b] < trail_chand || sd.close[b] < trail_turtle {
                            exit_bar = b;
                            break;
                        }
                    }

                    let exit_px = sd.close.get(exit_bar).copied().unwrap_or(entry_px);
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

        if !entered {
            equity_curve.push(equity);
            bar += 1;
        }
    }

    let ret = (equity - 1.0) * 100.0;
    let sharpe = annualised_sharpe(&daily_rets);
    let max_dd = max_dd_from(&equity_curve);
    let win_rate = if total_trades > 0 { wins as f64 / total_trades as f64 * 100.0 } else { 0.0 };
    let pass = total_trades >= MIN_TRADES && ret > 0.0;
    WfResult { ret, sharpe, max_dd, trades: total_trades, win_rate, pass, equity_final: equity, equity_curve }
}

fn write_lines(path: &str, lines: &[String]) -> Result<()> {
    let mut f = File::create(path)?;
    for line in lines { writeln!(f, "{}", line)?; }
    Ok(())
}

#[tokio::main]
async fn main() -> Result<()> {
    let t0 = Instant::now();
    let hold_values: Vec<usize> = (1..=100).collect();
    println!("==== HOLD_MAX current-production full sweep ====");
    println!("Range: 1..=100 step 1 ({} values)", hold_values.len());
    println!("Params: EP={}, CHAND({},{:.2}), ATR({},{:.2}), ATR_ENTRY_MULT={:.2}, CAP={}, VL={}",
        TURTLE_ENTRY, CHAND_PERIOD, CHAND_MULT, TURTLE_ATR_PERIOD, TURTLE_ATR_MULT, ATR_ENTRY_MULT, POSITION_CAP, VOL_LOOKBACK);

    let loader = DataLoader::new(None, None);
    let mut all_syms: HashSet<String> = HashSet::new();
    for (_, syms) in UNIVERSES {
        for &s in *syms { all_syms.insert(s.to_string()); }
    }

    let mut raw_cache: HashMap<String, DataFrame> = HashMap::new();
    let mut min_len = usize::MAX;
    for sym in all_syms.iter() {
        match loader.fetch_with_cache(sym, "1d", CANDLES).await {
            Ok(df) => {
                min_len = min_len.min(df.height());
                raw_cache.insert(sym.clone(), df);
            }
            Err(e) => eprintln!("WARNING: {} load failed: {}", sym, e),
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
                high: col_vec!("high"),
                low: col_vec!("low"),
                vol: col_vec!("volume"),
            });
        }
    }
    println!("Loaded {} symbols, {} aligned bars", sym_data_map.len(), n);

    let mut metric_lines = vec!["hold_max,universe,window,return_pct,sharpe,max_dd_pct,trades,win_rate_pct,pass,equity_final".to_string()];
    let mut equity_lines = vec!["hold_max,universe,window,step,equity".to_string()];
    let mut summary_lines = vec!["hold_max,global_pass,global_total,pass_rate_pct,avg_sharpe,avg_return_pct,avg_max_dd_pct,total_trades,avg_win_rate_pct".to_string()];

    let mut pass: HashMap<usize, usize> = HashMap::new();
    let mut total: HashMap<usize, usize> = HashMap::new();
    let mut trades: HashMap<usize, usize> = HashMap::new();
    let mut sh_sum: HashMap<usize, f64> = HashMap::new();
    let mut ret_sum: HashMap<usize, f64> = HashMap::new();
    let mut dd_sum: HashMap<usize, f64> = HashMap::new();
    let mut wr_sum: HashMap<usize, f64> = HashMap::new();

    for &(label, symbols) in UNIVERSES {
        let symbols: Vec<String> = symbols.iter().map(|s| s.to_string()).collect();
        if !symbols.iter().all(|s| sym_data_map.contains_key(s)) {
            eprintln!("{} skipped: missing symbol data", label);
            continue;
        }
        let total_windows = n.saturating_sub(TRAIN_BARS + TEST_BARS) / TEST_BARS;
        println!("{}: {} windows", label, total_windows);

        for wi in 0..total_windows {
            let test_start = TRAIN_BARS + wi * TEST_BARS;
            let test_end = (test_start + TEST_BARS).min(n);
            if test_end.saturating_sub(test_start) < 5 { continue; }

            for &hm in &hold_values {
                let r = run_sim(&sym_data_map, &symbols, test_start, test_end, hm);
                metric_lines.push(format!("{},{},{},{:.4},{:.6},{:.4},{},{:.4},{},{:.8}",
                    hm, label, wi, r.ret, r.sharpe, r.max_dd, r.trades, r.win_rate, r.pass, r.equity_final));
                for (step, &eq) in r.equity_curve.iter().enumerate() {
                    equity_lines.push(format!("{},{},{},{},{:.8}", hm, label, wi, step, eq));
                }
                *pass.entry(hm).or_insert(0) += if r.pass { 1 } else { 0 };
                *total.entry(hm).or_insert(0) += 1;
                *trades.entry(hm).or_insert(0) += r.trades;
                *sh_sum.entry(hm).or_insert(0.0) += r.sharpe;
                *ret_sum.entry(hm).or_insert(0.0) += r.ret;
                *dd_sum.entry(hm).or_insert(0.0) += r.max_dd;
                *wr_sum.entry(hm).or_insert(0.0) += r.win_rate;
            }
        }
    }

    let mut ranking: Vec<(usize, f64, usize, usize, f64, f64, usize, f64)> = Vec::new();
    for &hm in &hold_values {
        let gt = total.get(&hm).copied().unwrap_or(0);
        if gt == 0 { continue; }
        let gp = pass.get(&hm).copied().unwrap_or(0);
        let avg_sh = sh_sum[&hm] / gt as f64;
        let avg_ret = ret_sum[&hm] / gt as f64;
        let avg_dd = dd_sum[&hm] / gt as f64;
        let total_trades = trades[&hm];
        let avg_wr = wr_sum[&hm] / gt as f64;
        let pr = gp as f64 / gt as f64 * 100.0;
        summary_lines.push(format!("{},{},{},{:.4},{:.6},{:.4},{:.4},{},{:.4}",
            hm, gp, gt, pr, avg_sh, avg_ret, avg_dd, total_trades, avg_wr));
        ranking.push((hm, avg_sh, gp, gt, avg_ret, avg_dd, total_trades, avg_wr));
    }
    ranking.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());

    write_lines(CSV_METRICS, &metric_lines)?;
    write_lines(CSV_SUMMARY, &summary_lines)?;
    write_lines(CSV_EQUITY, &equity_lines)?;

    println!("\n==== Top 15 HOLD_MAX values by avg Sharpe ====");
    for (rank, (hm, sh, gp, gt, ret, dd, tr, wr)) in ranking.iter().take(15).enumerate() {
        let marker = if *hm == HOLD_BASELINE { " <- baseline" } else if rank == 0 { " <- winner" } else { "" };
        println!("#{:02} HM={:3} sh={:8.4} pass={:2}/{} ({:5.1}%) ret={:+8.2}% dd={:6.2}% trades={:5} wr={:5.1}%{}",
            rank + 1, hm, sh, gp, gt, *gp as f64 / *gt as f64 * 100.0, ret, dd, tr, wr, marker);
    }
    if let Some(base) = ranking.iter().find(|x| x.0 == HOLD_BASELINE) {
        println!("\nBaseline HM={} rank: #{} | sh={:.4} pass={}/{} ret={:+.2}% dd={:.2}%",
            HOLD_BASELINE,
            ranking.iter().position(|x| x.0 == HOLD_BASELINE).unwrap_or(usize::MAX) + 1,
            base.1, base.2, base.3, base.4, base.5);
    }
    println!("\nWrote: {}", CSV_METRICS);
    println!("Wrote: {}", CSV_SUMMARY);
    println!("Wrote: {}", CSV_EQUITY);
    println!("Elapsed: {:.1}s", t0.elapsed().as_secs_f64());
    Ok(())
}
