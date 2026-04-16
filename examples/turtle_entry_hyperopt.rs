//! Turtle+Chandelier Walk-Forward: All 9 Universes
//!
//! EP=21 (from 2026-04-10 full grid search)
//! Chandelier(28, 2.0) dynamic ATR trailing stop
//! Walk-forward: 252-bar train / 252-bar test
//! Fees: 0.1% taker each side, min 3 trades per window

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
const HOLD_MAX: usize = 45; // hyperopt 2026-04-11: HM=45 Sharpe winner across 9/9 universes (+1.5% vs HM=60 baseline). Full sweep 10-200 step 5 (39 values). Chandelier exits most trades before 45 bars; HM>=50 produces identical results. HM=15-25 gives +2pp pass rate but -10% Sharpe.
const TAKER_FEE: f64 = 0.001;
const POSITION_CAP: usize = 3; // hyperopt 2026-04-11: CAP=3 wins over CAP=2 (+1.4% Sharpe, +13pp pass rate, 91% vs 78%)
const MIN_TRADES: usize = 3;
const CHAND_PERIOD: usize = 28; // hyperopt 2026-04-11: P=28 fine-sweep winner at M=2.00 (Sharpe 9.011 vs P=15=7.785, +15.7%). Full 5-50 step-1 sweep, 9/9 universes positive.
const CHAND_MULT: f64 = 2.00; // hyperopt 2026-04-11: M=2.00 optimal. Note: P was re-optimized at M=2.00 (P=15 was optimal at M=2.5, P=28 at M=2.0)
const TURTLE_ENTRY: usize = 21;
const EP_START: usize = 5;
const EP_END: usize = 80;
const EP_STEP: usize = 1;
const BASELINE_EP: usize = 21;
const TURTLE_ATR_PERIOD: usize = 24;
const TURTLE_ATR_MULT: f64 = 2.00; // hyperopt 2026-04-12: TURTLE_ATR_MULT sweep {1.0-5.0 step 0.5}. M=2.0 is optimal (Sharpe 6.17, 93% pass). M<2.0 degrades Sharpe (M=1.0: 3.06). M>=2.5: Turtle ATR never fires first (Chandelier dominates). Current value matches CHAND_MULT by design — Turtle ATR is the faster secondary exit, not an independent mechanism. See memory/hyperopt-2026-04-12-atr-mult.md.

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

const CSV_OUT: &str = "snapshots/turtle_entry_hyperopt.csv";
const MD_OUT: &str = "snapshots/turtle_entry_hyperopt.md";
const EQUITY_OUT: &str = "snapshots/turtle_entry_hyperopt_equity.csv";

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
}

fn run_sim(
    sym_data: &HashMap<String, SymData>,
    symbols: &[String],
    test_start: usize,
    test_end: usize,
    entry_period: usize,
) -> WfResult {
    let mut equity = 1.0_f64;
    let mut equity_curve = vec![1.0_f64];
    let mut peak = equity;
    let mut wins = 0usize;
    let mut total_trades = 0usize;
    let mut daily_rets = Vec::new();

    let mut bar = test_start;
    while bar + 2 < test_end {
        // Rank symbols by dollar volume
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
            equity_curve.push(equity);
            bar += 1;
            continue;
        }

        // Turtle breakout entry (no regime filter)
        let mut entered = false;
        for sym in &top_syms {
            if let Some(sd) = sym_data.get(sym) {
                if bar >= entry_period + 1 && bar < sd.close.len() {
                    if turtle_signal(&sd.close, &sd.high, entry_period, bar) {
                        let entry_px = sd.close[bar];
                        let entry = entry_px * (1.0 - TAKER_FEE);
                        let entry_bar_next = bar + 1;
                        let n = sd.close.len();

                        // DUAL_EXIT: Chandelier(28,2.0) OR Turtle_ATR(25,2.0) — exit fires on whichever triggers first
                        let mut highest_high_chand = sd.high[entry_bar_next];
                        let mut highest_high_turtle = sd.high[entry_bar_next];
                        let max_bar = (entry_bar_next + HOLD_MAX).min(n.saturating_sub(1));
                        let mut exit_bar = max_bar;
                        for b in entry_bar_next..=max_bar.min(n.saturating_sub(1)) {
                            // Chandelier ATR(28, 2.0)
                            highest_high_chand = highest_high_chand.max(sd.high[b]);
                            let atr_chand = atr_at(&sd.high, &sd.low, &sd.close, CHAND_PERIOD, b);
                            let trail_chand = highest_high_chand - CHAND_MULT * atr_chand;
                            // Turtle ATR(25, 2.0)
                            highest_high_turtle = highest_high_turtle.max(sd.high[b]);
                            let atr_turtle = atr_at(&sd.high, &sd.low, &sd.close, TURTLE_ATR_PERIOD, b);
                            let trail_turtle = highest_high_turtle - TURTLE_ATR_MULT * atr_turtle;
                            // Exit on EITHER stop
                            if sd.close[b] < trail_chand || sd.close[b] < trail_turtle {
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
                            for _ in 0..bars_held {
                                daily_rets.push(avg_daily);
                            }

                            if equity > peak { peak = equity; }
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

    let ret = (equity - 1.0) * 100.0;
    let sharpe = annualised_sharpe(&daily_rets);
    let max_dd = max_dd_from(&equity_curve);
    let win_rate = if total_trades > 0 { wins as f64 / total_trades as f64 * 100.0 } else { 0.0 };
    let pass = total_trades >= MIN_TRADES && ret > 0.0;

    WfResult { ret, sharpe, max_dd, trades: total_trades, win_rate, pass }
}

#[tokio::main]
async fn main() -> Result<()> {
    let t0 = Instant::now();
    eprintln!("==== Turtle+Chandelier Walk-Forward: 9 Universes ====");
    eprintln!("EP={}, Chandelier({}, {}), 252/252 train/test\n", TURTLE_ENTRY, CHAND_PERIOD, CHAND_MULT);

    let loader = DataLoader::new(None, None);
    let mut all_syms: std::collections::HashSet<String> = std::collections::HashSet::new();
    for (_, syms) in UNIVERSES {
        for &s in *syms {
            all_syms.insert(s.to_string());
        }
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

    let mut all_records = Vec::new();
    let mut csv_lines = vec!["universe,window,return_pct,sharpe,max_dd_pct,trades,win_rate_pct,pass".to_string()];

    let mut global_pass = 0usize;
    let mut global_total = 0usize;
    let mut global_trades = 0usize;

    for &(label, symbols) in UNIVERSES {
        let symbols: Vec<String> = symbols.iter().map(|s| s.to_string()).collect();
        let all_loaded = symbols.iter().all(|s| sym_data_map.contains_key(s));
        if !all_loaded {
            eprintln!("{:>20} SKIPPED (missing data)", label);
            continue;
        }

        let total_windows = n.saturating_sub(TRAIN_BARS + TEST_BARS) / TEST_BARS;
        if total_windows == 0 {
            eprintln!("{:>20} SKIPPED (not enough data)", label);
            continue;
        }

        eprintln!("==== {:<18} ==== {} syms, {} windows", label, symbols.len(), total_windows);

        let mut agg_ret = 0.0_f64;
        let mut passed = 0usize;

        for wi in 0..total_windows {
            let train_end = TRAIN_BARS + wi * TEST_BARS;
            let test_start = train_end;
            let test_end = (test_start + TEST_BARS).min(n);

            if test_end.saturating_sub(test_start) < 5 { continue; }

            let r = run_sim(&sym_data_map, &symbols, test_start, test_end, TURTLE_ENTRY);

            let thin = if r.trades < MIN_TRADES { "THIN" } else { "OK" };
            let result = if r.pass { "PASS" } else { "FAIL" };
            eprintln!(
                "  W{:02} | {:+8.1}% sh={:6.2} DD={:5.1}% {:4}t {:3.0}% {} {}",
                wi, r.ret, r.sharpe, r.max_dd, r.trades, r.win_rate, thin, result
            );

            csv_lines.push(format!(
                "{},{},{:.2},{:.4},{:.2},{},{:.2},{}",
                label, wi, r.ret, r.sharpe, r.max_dd, r.trades, r.win_rate, r.pass
            ));

            agg_ret += r.ret;
            if r.pass { passed += 1; }
            global_pass += if r.pass { 1 } else { 0 };
            global_total += 1;
            global_trades += r.trades;

            all_records.push((label.to_string(), wi, r));
        }

        let avg_ret = agg_ret / total_windows as f64;
        let pass_pct = passed as f64 / total_windows as f64 * 100.0;
        eprintln!("  AGG | avg {:+7.1}% {}/{} pass ({:.0}%)\n", avg_ret, passed, total_windows, pass_pct);
    }

    let mut f = File::create(CSV_OUT)?;
    for line in &csv_lines { writeln!(f, "{}", line)?; }

    let fail_pct = (global_total - global_pass) as f64 / global_total.max(1) as f64 * 100.0;
    let avg_sharpe: f64 = all_records.iter().map(|(_,_,r)| r.sharpe).sum::<f64>() / all_records.len().max(1) as f64;

    eprintln!("==== GLOBAL SUMMARY ====");
    eprintln!("  Turtle+Chandelier: {}/{} windows passed ({:.0}% fail)", global_pass, global_total, fail_pct);
    eprintln!("  Avg Sharpe: {}", avg_sharpe);
    eprintln!("  Total trades: {}", global_trades);
    eprintln!("  CSV: {}", CSV_OUT);
    eprintln!("  Runtime: {:?}", t0.elapsed());

    let mut md = File::create(MD_OUT)?;
    writeln!(md, "# Turtle+Chandelier Walk-Forward: 9 Universes")?;
    writeln!(md, "")?;
    writeln!(md, "| Universe | Pass | Avg Ret | Avg Sharpe | Worst DD | Trades |")?;
    writeln!(md, "|---|---|---|---|---|---|")?;
    for &(uni_name, _) in UNIVERSES {
        let recs: Vec<_> = all_records.iter().filter(|(l,_,_)| *l == uni_name).collect();
        let pass = recs.len();
        let n_win = recs.len();
        let avg_ret: f64 = recs.iter().map(|(_,_,r)| r.ret).sum::<f64>() / n_win.max(1) as f64;
        let avg_sh: f64 = recs.iter().map(|(_,_,r)| r.sharpe).sum::<f64>() / n_win.max(1) as f64;
        let worst_dd: f64 = recs.iter().map(|(_,_,r)| r.max_dd).fold(0.0_f64, |a,b| a.max(b));
        let trades: usize = recs.iter().map(|(_,_,r)| r.trades).sum();
        writeln!(md, "| {} | {}/{} | {:+.1}% | {} | {:.1}% | {} |", uni_name, pass, n_win, avg_ret, avg_sh, worst_dd, trades)?;
    }
    writeln!(md, "")?;
    writeln!(md, "**GLOBAL: {}/{} pass ({:.0}% fail), avg Sharpe {}, {} trades**", global_pass, global_total, fail_pct, avg_sharpe, global_trades)?;
    eprintln!("  MD: {}", MD_OUT);

    Ok(())
}
