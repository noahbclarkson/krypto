//! T22: Exit Attribution — which exit fires first?
//! Chandelier ATR (trailing max_high - CHAND_MULT * ATR) vs Turtle ATR (trailing min_low - TURTLE_ATR_MULT * ATR) vs HOLD_MAX
//!
//! HYPOTHESIS: If Chandelier fires >90% of trades, TURTLE_ATR hyperopts (period, mult)
//! are curve-fitting noise on a parameter that barely matters.
//!
//! Production params: EP=21, CHAND_P=7, CHAND_M=2.30, ATR_P=24, ATR_M=2.0, HM=12, CAP=3
//! Run: cargo run --example exit_attribution_walkforward --profile sweep

use anyhow::Result;
use krypto::data::loader::DataLoader;
use polars::prelude::*;
use std::collections::HashMap;
use std::time::Instant;

const CANDLES: u32 = 3000;
const TRAIN_BARS: usize = 252;
const TEST_BARS: usize = 252;

// === PRODUCTION PARAMS (frozen) ===
const TURTLE_ENTRY: usize = 21;      // EP=21 confirmed 2026-04-26
const CHAND_PERIOD: usize = 7;       // CP=7 confirmed dense sweep 2026-04-21
const CHAND_MULT: f64 = 2.30;        // CM=2.30 confirmed 71-value dense sweep 2026-04-25
const TURTLE_ATR_PERIOD: usize = 24; // ATR=24 confirmed fine sweep 2026-04-16
const TURTLE_ATR_MULT: f64 = 2.00;  // AM=2.00 confirmed null-result sweep 2026-04-12
const HOLD_MAX: usize = 12;          // HM=12 confirmed 2026-04-21
const POSITION_CAP: usize = 3;       // CAP=3 confirmed 2026-04-27
const MIN_TRADES: usize = 3;
const TAKER_FEE: f64 = 0.0004;       // 4bp conservative taker fee
const VOL_LOOKBACK: usize = 8;       // VL=8 confirmed 2026-04-28

const UNIVERSES: &[(&str, &[&str])] = &[
    ("Base5",        &["BTCUSDT","ETHUSDT","SOLUSDT","XRPUSDT","DOGEUSDT","ADAUSDT"]),
    ("NoDOGE",       &["BTCUSDT","ETHUSDT","SOLUSDT","XRPUSDT","ADAUSDT"]),
    ("LargeCaps5",   &["BTCUSDT","ETHUSDT","SOLUSDT","XRPUSDT","BNBUSDT","ADAUSDT"]),
    ("Legacy4",      &["BTCUSDT","ETHUSDT","XRPUSDT","LTCUSDT","EOSUSDT"]),
    ("Legacy5BNB",   &["BTCUSDT","ETHUSDT","XRPUSDT","LTCUSDT","BNBUSDT","EOSUSDT"]),
    ("Legacy3",      &["BTCUSDT","XRPUSDT","LTCUSDT","EOSUSDT"]),
    ("LowVolume5",   &["XRPUSDT","LTCUSDT","EOSUSDT","BCHUSDT","ADAUSDT"]),
    ("FTXAlts",      &["BTCUSDT","ETHUSDT","SOLUSDT","AVAXUSDT","MATICUSDT"]),
    ("DeGen5",       &["DOGEUSDT","SHIBUSDT","AVAXUSDT","MATICUSDT","DOTUSDT"]),
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExitReason {
    Chandelier,
    TurtleATR,
    HoldMax,
}

struct SymData {
    close: Vec<f64>,
    high: Vec<f64>,
    low: Vec<f64>,
    vol: Vec<f64>,
}

struct TradeResult {
    exit_reason: ExitReason,
    bars_held: usize,
    gross_ret: f64,
}

struct WfResult {
    ret: f64,
    sharpe: f64,
    max_dd: f64,
    trades: usize,
    chandelier_fires: usize,
    turtle_fires: usize,
    holdmax_fires: usize,
    chandelier_avg_ret: f64,
    turtle_avg_ret: f64,
    holdmax_avg_ret: f64,
    win_rate: f64,
    pass: bool,
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

fn turtle_signal(close: &[f64], idx: usize) -> bool {
    if idx < TURTLE_ENTRY + 1 { return false; }
    let start = idx + 1 - TURTLE_ENTRY;
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
    mn * 252.0_f64.sqrt() / sd
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
    test_start: usize,
    test_end: usize,
) -> WfResult {
    let mut equity = 1.0_f64;
    let mut equity_curve = vec![1.0_f64];
    let mut peak = equity;
    let mut wins = 0usize;
    let mut total_trades = 0usize;
    let mut daily_rets = Vec::new();
    let mut trades: Vec<TradeResult> = Vec::new();

    let mut bar = test_start;
    while bar + 2 < test_end {
        // Rank symbols by dollar volume
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

        // Turtle breakout entry
        let mut entered = false;
        for sym in &top_syms {
            if let Some(sd) = sym_data.get(sym) {
                if bar >= TURTLE_ENTRY + 1 && bar < sd.close.len() {
                    if turtle_signal(&sd.close, bar) {
                        let entry_px = sd.close[bar];
                        let entry = entry_px * (1.0 - TAKER_FEE);
                        let entry_bar_next = bar + 1;
                        let n = sd.close.len();

                        // DUAL EXIT with attribution
                        let mut highest_high_chand = sd.high[entry_bar_next];
                        let mut lowest_low_turtle = sd.low[entry_bar_next];
                        let max_bar = (entry_bar_next + HOLD_MAX).min(n.saturating_sub(1));
                        let mut exit_bar = max_bar;
                        let mut exit_reason = ExitReason::HoldMax;

                        for b in entry_bar_next..=max_bar.min(n.saturating_sub(1)) {
                            highest_high_chand = highest_high_chand.max(sd.high[b]);
                            let atr_chand = atr_at(&sd.high, &sd.low, &sd.close, CHAND_PERIOD, b);
                            let trail_chand = highest_high_chand - CHAND_MULT * atr_chand;

                            lowest_low_turtle = lowest_low_turtle.min(sd.low[b]);
                            let atr_turtle = atr_at(&sd.high, &sd.low, &sd.close, TURTLE_ATR_PERIOD, b);
                            let trail_turtle = lowest_low_turtle - TURTLE_ATR_MULT * atr_turtle;

                            // Track which fires first
                            let chand_triggered = sd.close[b] < trail_chand;
                            let turtle_triggered = sd.close[b] < trail_turtle;

                            if chand_triggered {
                                exit_bar = b;
                                exit_reason = ExitReason::Chandelier;
                                break;
                            } else if turtle_triggered {
                                exit_bar = b;
                                exit_reason = ExitReason::TurtleATR;
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

                            trades.push(TradeResult {
                                exit_reason,
                                bars_held,
                                gross_ret,
                            });

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

    let chand_fires = trades.iter().filter(|t| t.exit_reason == ExitReason::Chandelier).count();
    let turtle_fires = trades.iter().filter(|t| t.exit_reason == ExitReason::TurtleATR).count();
    let hm_fires = trades.iter().filter(|t| t.exit_reason == ExitReason::HoldMax).count();

    let chand_avg_ret = if chand_fires > 0 {
        trades.iter().filter(|t| t.exit_reason == ExitReason::Chandelier).map(|t| t.gross_ret).sum::<f64>() / chand_fires as f64
    } else { 0.0 };
    let turtle_avg_ret = if turtle_fires > 0 {
        trades.iter().filter(|t| t.exit_reason == ExitReason::TurtleATR).map(|t| t.gross_ret).sum::<f64>() / turtle_fires as f64
    } else { 0.0 };
    let hm_avg_ret = if hm_fires > 0 {
        trades.iter().filter(|t| t.exit_reason == ExitReason::HoldMax).map(|t| t.gross_ret).sum::<f64>() / hm_fires as f64
    } else { 0.0 };

    WfResult {
        ret, sharpe, max_dd, trades: total_trades,
        chandelier_fires: chand_fires,
        turtle_fires,
        holdmax_fires: hm_fires,
        chandelier_avg_ret: chand_avg_ret * 100.0,
        turtle_avg_ret: turtle_avg_ret * 100.0,
        holdmax_avg_ret: hm_avg_ret * 100.0,
        win_rate, pass,
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    let t0 = Instant::now();
    eprintln!("==== T22: Exit Attribution Walk-Forward ====");
    eprintln!("Params: EP={}, Chand({}, {}), TurtleATR({}, {}), HM={}, CAP={}",
        TURTLE_ENTRY, CHAND_PERIOD, CHAND_MULT, TURTLE_ATR_PERIOD, TURTLE_ATR_MULT, HOLD_MAX, POSITION_CAP);

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

    let mut all_records = Vec::new();
    let mut global_trades = 0usize;
    let mut global_chand = 0usize;
    let mut global_turtle = 0usize;
    let mut global_hm = 0usize;
    let mut global_pass = 0usize;
    let mut global_total = 0usize;

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

        for wi in 0..total_windows {
            let train_end = TRAIN_BARS + wi * TEST_BARS;
            let test_start = train_end;
            let test_end = (test_start + TEST_BARS).min(n);
            if test_end.saturating_sub(test_start) < 5 { continue; }

            let r = run_sim(&sym_data_map, &symbols, test_start, test_end);

            let total = r.trades;
            let chand_pct = if total > 0 { r.chandelier_fires as f64 / total as f64 * 100.0 } else { 0.0 };
            let turtle_pct = if total > 0 { r.turtle_fires as f64 / total as f64 * 100.0 } else { 0.0 };
            let hm_pct = if total > 0 { r.holdmax_fires as f64 / total as f64 * 100.0 } else { 0.0 };

            let thin = if r.trades < MIN_TRADES { "THIN" } else { "   " };
            let result = if r.pass { "PASS" } else { "FAIL" };
            eprintln!("  W{:02} | {:+8.1}% sh={:6.2} DD={:5.1}% {:4}t  CHAND={:5.1}% TURTLE={:5.1}% HOLDMAX={:5.1}% {} {}",
                wi, r.ret, r.sharpe, r.max_dd, r.trades,
                chand_pct, turtle_pct, hm_pct, thin, result);

            global_trades += r.trades;
            global_chand += r.chandelier_fires;
            global_turtle += r.turtle_fires;
            global_hm += r.holdmax_fires;
            if r.pass { global_pass += 1; }
            global_total += 1;

            all_records.push((
                label.to_string(),
                wi,
                r.ret,
                r.sharpe,
                r.max_dd,
                r.trades,
                r.win_rate,
                r.chandelier_fires,
                r.turtle_fires,
                r.holdmax_fires,
                chand_pct,
                turtle_pct,
                hm_pct,
                r.chandelier_avg_ret,
                r.turtle_avg_ret,
                r.holdmax_avg_ret,
                r.pass,
            ));
        }
    }

    // Print global summary
    eprintln!("\n============================ T22 GLOBAL SUMMARY ============================");
    eprintln!("Total windows: {}", global_total);
    eprintln!("Total trades:  {}", global_trades);
    eprintln!("");
    let chand_pct_g = if global_trades > 0 { global_chand as f64 / global_trades as f64 * 100.0 } else { 0.0 };
    let turtle_pct_g = if global_trades > 0 { global_turtle as f64 / global_trades as f64 * 100.0 } else { 0.0 };
    let hm_pct_g = if global_trades > 0 { global_hm as f64 / global_trades as f64 * 100.0 } else { 0.0 };
    eprintln!("Chandelier fires:  {:>5} ({:5.1}%)  [avg ret: --]", global_chand, chand_pct_g);
    eprintln!("TurtleATR fires:    {:>5} ({:5.1}%)  [avg ret: --]", global_turtle, turtle_pct_g);
    eprintln!("HoldMax fires:     {:>5} ({:5.1}%)  [avg ret: --]", global_hm, hm_pct_g);
    eprintln!("Pass rate: {}/{} ({:.1}%)", global_pass, global_total,
        if global_total > 0 { global_pass as f64 / global_total as f64 * 100.0 } else { 0.0 });

    // Weighted avg returns per exit type
    let chand_avg_wt = if global_chand > 0 {
        all_records.iter()
            .filter(|r| r.7 > 0)
            .map(|r| r.13 * r.7 as f64)
            .sum::<f64>() / global_chand as f64
    } else { 0.0 };
    let turtle_avg_wt = if global_turtle > 0 {
        all_records.iter()
            .filter(|r| r.8 > 0)
            .map(|r| r.14 * r.8 as f64)
            .sum::<f64>() / global_turtle as f64
    } else { 0.0 };
    let hm_avg_wt = if global_hm > 0 {
        all_records.iter()
            .filter(|r| r.9 > 0)
            .map(|r| r.15 * r.9 as f64)
            .sum::<f64>() / global_hm as f64
    } else { 0.0 };
    eprintln!("\nWeighted avg returns:");
    eprintln!("  Chandelier: {:.2}%", chand_avg_wt);
    eprintln!("  TurtleATR:  {:.2}%", turtle_avg_wt);
    eprintln!("  HoldMax:    {:.2}%", hm_avg_wt);

    // V E R D I C T
    eprintln!("\n============================ T22 VERDICT ============================");
    eprintln!("Total trades analyzed: {}", global_trades);
    if chand_pct_g >= 90.0 {
        eprintln!("\n!!! CHANDELIER DOMINATES ({:.0}%) !!!", chand_pct_g);
        eprintln!("TurtleATR hyperopts (period, mult) are likely curve-fitting noise.");
        eprintln!("The TURTLE_ATR_PERIOD=24 and TURTLE_ATR_MULT=2.00 hyperopts were tuned");
        eprintln!("on a parameter that fires <10% of the time. These results are unreliable.");
    } else if turtle_pct_g >= 20.0 {
        eprintln!("\nOK: TurtleATR contributes meaningfully ({:.0}%)", turtle_pct_g);
        eprintln!("TURTLE_ATR hyperopts are defensible.");
    } else if turtle_pct_g >= 5.0 {
        eprintln!("\nCAUTION: TurtleATR fires {:.0}% of trades — minority but non-trivial.", turtle_pct_g);
        eprintln!("TURTLE_ATR hyperopts are weak signal, still defensible as secondary exit tuning.");
    } else {
        eprintln!("\nNOTE: TurtleATR fires only {:.1}% — mostly decorative.", turtle_pct_g);
        eprintln!("TURTLE_ATR hyperopts are marginal but not meaningless.");
    }

    // Export CSV
    let mut csv_lines = vec!["universe,window,return_pct,sharpe,max_dd_pct,trades,win_rate_pct,chandelier_fires,turtle_fires,holdmax_fires,chandelier_pct,turtle_pct,holdmax_pct,chandelier_avg_ret,turtle_avg_ret,holdmax_avg_ret,pass".to_string()];
    for r in &all_records {
        csv_lines.push(format!("{},{},{:.2},{:.4},{:.2},{},{:.2},{},{},{},{:.2},{:.2},{:.2},{:.4},{:.4},{:.4},{}",
            r.0, r.1, r.2, r.3, r.4, r.5, r.6, r.7, r.8, r.9, r.10, r.11, r.12, r.13, r.14, r.15, r.16));
    }
    let csv = csv_lines.join("\n");
    std::fs::write("snapshots/t22_exit_attribution.csv", &csv)?;
    eprintln!("\nWrote snapshots/t22_exit_attribution.csv");
    eprintln!("Elapsed: {:.1}s", t0.elapsed().as_secs_f64());
    Ok(())
}
