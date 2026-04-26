//! S6: Turtle-Only Exit Walk-Forward
//!
//! Hypothesis: Turtle breakout (EP=21) with ONLY Turtle ATR(24, 2.0) exit.
//! NO Chandelier exit.
//!
//! Baseline: Turtle+Chandelier (EP=21, CHAND_P=7, M=2.30, HM=12)
//! S6:       Turtle-only        (EP=21, TurtleATR_P=24, M=2.0, HM=12)

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

const TURTLE_ENTRY: usize = 21;
const TURTLE_ATR_PERIOD: usize = 24;
const TURTLE_ATR_MULT: f64 = 2.00;
const ATR_ENTRY_MULT: f64 = 0.00;
const VOL_LOOKBACK: usize = 1;

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

const CSV_OUT: &str = "snapshots/s6_turtle_only_exit_wf.csv";
const MD_OUT: &str = "snapshots/s6_turtle_only_exit_wf.md";

struct SymData {
    close: Vec<f64>,
    high: Vec<f64>,
    low: Vec<f64>,
    vol: Vec<f64>,
}

/// ATR — same as original walk-forward (True Range variant)
fn atr_at(high: &[f64], low: &[f64], close: &[f64], period: usize, idx: usize) -> f64 {
    if idx < period {
        return 0.0;
    }
    let mut trs = Vec::with_capacity(period);
    for i in (idx + 1 - period)..=idx {
        let h = high[i];
        let l = low[i];
        let c0 = close[i.saturating_sub(1)];
        trs.push((h - l).max((h - c0).abs()).max((l - c0).abs()));
    }
    trs.iter().sum::<f64>() / period as f64
}

fn rolling_avg(vals: &[f64], window: usize, idx: usize) -> f64 {
    if idx < window {
        vals[idx]
    } else {
        vals[idx + 1 - window..=idx].iter().sum::<f64>() / window as f64
    }
}

/// Turtle signal — EXACTLY matches turtle_chandelier_walkforward.rs
/// Range is [idx+1-entry_period .. idx) — excludes current bar
fn turtle_signal(close: &[f64], high: &[f64], low: &[f64], entry_period: usize, atr_period: usize, atr_mult: f64, idx: usize) -> bool {
    if idx < entry_period + 1 {
        return false;
    }
    let start = idx + 1 - entry_period;
    let mut max_close = f64::NEG_INFINITY;
    for i in start..idx {
        if let Some(&c) = close.get(i) {
            max_close = max_close.max(c);
        }
    }
    if let Some(&curr_close) = close.get(idx) {
        let breakout = curr_close > max_close;
        if breakout && atr_mult > 0.0 {
            let atr_val = atr_at(high, low, close, atr_period, idx);
            return curr_close >= max_close + atr_mult * atr_val;
        }
        breakout
    } else {
        false
    }
}

fn annualised_sharpe(daily_rets: &[f64]) -> f64 {
    if daily_rets.len() < 2 {
        return 0.0;
    }
    let mn: f64 = daily_rets.iter().sum::<f64>() / daily_rets.len() as f64;
    let sd = (daily_rets.iter().map(|x| (x - mn).powi(2)).sum::<f64>() / daily_rets.len() as f64).sqrt();
    if sd == 0.0 {
        return 0.0;
    }
    mn * 365.0_f64.sqrt() / sd
}

fn max_dd_from(equity: &[f64]) -> f64 {
    let mut peak = f64::NEG_INFINITY;
    let mut max_dd = 0.0_f64;
    for &e in equity {
        if e > peak {
            peak = e;
        }
        let dd = (peak - e) / peak;
        if dd > max_dd {
            max_dd = dd;
        }
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

/// Run simulation — mirrors turtle_chandelier_walkforward.rs structure
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

    let mut bar = test_start;
    while bar + 2 < test_end {
        // Rank by dollar volume
        let mut scores: Vec<(&str, f64)> = Vec::new();
        for sym in symbols {
            if let Some(sd) = sym_data.get(sym) {
                if bar >= sd.close.len() {
                    continue;
                }
                let rol_vol = rolling_avg(&sd.vol, VOL_LOOKBACK, bar);
                let price = sd.close[bar];
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
                    if turtle_signal(&sd.close, &sd.high, &sd.low, TURTLE_ENTRY, TURTLE_ATR_PERIOD, ATR_ENTRY_MULT, bar) {
                        let entry_px = sd.close[bar];
                        let entry = entry_px * (1.0 - TAKER_FEE);
                        let entry_bar_next = bar + 1;
                        let n = sd.close.len();

                        // S6: Turtle ATR ONLY exit — NO Chandelier
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

                            if equity > peak {
                                peak = equity;
                            }
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
    let win_rate = if total_trades > 0 {
        wins as f64 / total_trades as f64 * 100.0
    } else {
        0.0
    };
    let pass = total_trades >= MIN_TRADES && ret > 0.0;

    WfResult { ret, sharpe, max_dd, trades: total_trades, win_rate, pass }
}

fn fmt_f64(val: f64) -> String {
    let sign = if val < 0.0 { "" } else { "+" };
    format!("{}{:.1}", sign, val)
}

#[tokio::main]
async fn main() -> Result<()> {
    let t0 = Instant::now();
    eprintln!("==== S6: Turtle-Only Exit Walk-Forward (NO Chandelier) ====\n");

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
            Err(e) => {
                eprintln!("  WARNING: {} load failed: {}", sym, e);
            }
        }
    }

    // SAME n as original: first min_len rows, capped at 2800
    let n = min_len.min(2800);
    eprintln!("Loaded {} symbols, min_len={} bars (SOL limits)\n", raw_cache.len(), min_len);

    let mut sym_data_map: HashMap<String, SymData> = HashMap::new();
    for sym in &all_syms {
        if let Some(df) = raw_cache.get(sym) {
            // CRITICAL: take FIRST n rows (same as original walk-forward)
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

    let mut csv_rows = vec!["universe,window,ret,sharpe,max_dd,trades,win_rate,pass".to_string()];
    let mut md_lines = vec![
        "# S6: Turtle-Only Exit Walk-Forward (NO Chandelier)".to_string(),
        "".to_string(),
        "**Hypothesis:** Turtle breakout with ONLY Turtle ATR(24, 2.0) exit vs Turtle+Chandelier dual exit.".to_string(),
        "**Params:** EP=21, TurtleATR(24, 2.0), HM=12, no Chandelier.".to_string(),
        "**Baseline:** Turtle+Chandelier(7, 2.30) — 36/54 pass (33% fail)".to_string(),
        "".to_string(),
        "| Universe | Win | Ret% | Sharpe | MaxDD% | Trades | WinRate% | Pass |".to_string(),
        "|----------|-----|------|--------|--------|--------|---------|------|".to_string(),
    ];

    let mut global_pass = 0usize;
    let mut global_total = 0usize;
    let mut all_sharpe: Vec<f64> = Vec::new();
    let mut all_ret: Vec<f64> = Vec::new();
    let mut all_dd: Vec<f64> = Vec::new();
    let mut all_trades = 0usize;

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

        let sym_data: HashMap<String, SymData> = symbols.iter()
            .filter_map(|s| sym_data_map.get(s).map(|sd| (s.to_string(), SymData {
                close: sd.close.clone(),
                high:  sd.high.clone(),
                low:   sd.low.clone(),
                vol:   sd.vol.clone(),
            })))
            .collect();

        let mut u_pass = 0usize;
        let mut u_total = 0usize;
        let mut u_sharpe_sum = 0.0_f64;
        let mut u_ret_sum = 0.0_f64;
        let mut u_max_dd = 0.0_f64;
        let mut u_trades = 0usize;
        let mut last_passed = false;

        for wi in 0..total_windows {
            let train_end = TRAIN_BARS + wi * TEST_BARS;
            let test_start = train_end;
            let test_end = (test_start + TEST_BARS).min(n);

            if test_end.saturating_sub(test_start) < 5 {
                continue;
            }

            let result = run_sim(&sym_data, &symbols, test_start, test_end);
            let win_lbl = if result.pass { "PASS" } else { "FAIL" };
            eprintln!(
                "  W{:02} | {} sh={:.2} DD={:.1} {}t {:3.0}%  {}",
                wi, fmt_f64(result.ret), result.sharpe, result.max_dd, result.trades, result.win_rate, win_lbl
            );

            csv_rows.push(format!(
                "{},W{:02},{:.2},{:.4},{:.2},{},{:.1},{}",
                uname, wi, result.ret, result.sharpe, result.max_dd, result.trades, result.win_rate, result.pass
            ));

            global_pass += if result.pass { 1 } else { 0 };
            global_total += 1;
            u_pass += if result.pass { 1 } else { 0 };
            u_total += 1;
            all_sharpe.push(result.sharpe);
            all_ret.push(result.ret);
            all_dd.push(result.max_dd);
            all_trades += result.trades;
            u_sharpe_sum += result.sharpe;
            u_ret_sum += result.ret;
            u_trades += result.trades;
            if result.max_dd > u_max_dd {
                u_max_dd = result.max_dd;
            }
            last_passed = result.pass;
        }

        let u_sharpe_avg = if u_total > 0 { u_sharpe_sum / u_total as f64 } else { 0.0 };
        let u_ret_avg   = if u_total > 0 { u_ret_sum / u_total as f64 } else { 0.0 };
        let u_pass_pct  = if u_total > 0 { u_pass as f64 / u_total as f64 * 100.0 } else { 0.0 };
        let last_lbl = if last_passed { "PASS" } else { "FAIL" };
        md_lines.push(format!(
            "| **{}** | {}/{} ({:.0}%) | {} | {:.2} | {:.1} | {} | {:.0}% | {} |",
            uname, u_pass, u_total, u_pass_pct,
            fmt_f64(u_ret_avg), u_sharpe_avg, u_max_dd,
            u_trades, 0.0, last_lbl
        ));
        eprintln!(
            "  {}: {}/{} ({:.0}%) pass, avg Sharpe={:.2}\n",
            uname, u_pass, u_total, u_pass_pct, u_sharpe_avg
        );
    }

    let fail_pct = if global_total > 0 {
        (global_total - global_pass) as f64 / global_total as f64 * 100.0
    } else {
        0.0
    };
    let avg_sharpe = if !all_sharpe.is_empty() {
        all_sharpe.iter().sum::<f64>() / all_sharpe.len() as f64
    } else {
        0.0
    };
    let avg_ret = if !all_ret.is_empty() {
        all_ret.iter().sum::<f64>() / all_ret.len() as f64
    } else {
        0.0
    };
    let avg_dd = if !all_dd.is_empty() {
        all_dd.iter().sum::<f64>() / all_dd.len() as f64
    } else {
        0.0
    };

    eprintln!("\n==== S6 Summary: Turtle-Only vs Turtle+Chandelier ====");
    eprintln!(
        "Global: {}/{} windows passed ({:.0}% fail)",
        global_pass, global_total, fail_pct
    );
    eprintln!(
        "Avg Sharpe: {:.2}, Avg Ret: {}, Avg MaxDD: {:.1}",
        avg_sharpe,
        fmt_f64(avg_ret),
        avg_dd
    );
    eprintln!("Total trades: {}", all_trades);
    eprintln!("\nBaseline (Turtle+Chandelier): 36/54 pass (33% fail)");
    eprintln!(
        "S6 Delta: {} more/less passes",
        global_pass as i64 - 36
    );

    let mut f = File::create(CSV_OUT)?;
    for row in &csv_rows {
        writeln!(f, "{}", row)?;
    }
    eprintln!("\nWrote: {}", CSV_OUT);

    let mut f = File::create(MD_OUT)?;
    for line in &md_lines {
        writeln!(f, "{}", line)?;
    }
    writeln!(f, "")?;
    writeln!(f, "## Global Summary")?;
    writeln!(
        f,
        "- **{}/{} windows passed ({:.0}% fail)**",
        global_pass, global_total, fail_pct
    )?;
    writeln!(
        f,
        "- Avg Sharpe: **{:.2}** (baseline Turtle+Chandelier: 36/54 pass)",
        avg_sharpe
    )?;
    writeln!(f, "- Avg Ret: **{}**", fmt_f64(avg_ret))?;
    writeln!(f, "- Avg MaxDD: **{:.1}%**", avg_dd)?;
    writeln!(f, "- Total trades: **{}**", all_trades)?;
    writeln!(f, "")?;
    writeln!(f, "## Verdict")?;
    if global_pass >= 40 && avg_sharpe >= 4.0 {
        writeln!(
            f,
            "**Turtle-only is NON-INFERIOR.** {} pass vs 36 baseline. Removing Chandelier does not materially degrade performance.",
            global_pass
        )?;
    } else if global_pass >= 30 {
        writeln!(
            f,
            "**Turtle-only is MARGINAL** — {} pass vs 36 baseline. Chandelier may contribute in some regimes.",
            global_pass
        )?;
    } else {
        writeln!(
            f,
            "**Turtle-only FAILS** — {} pass vs 36 baseline. Chandelier meaningfully contributes. KEEP dual-exit.",
            global_pass
        )?;
    }
    writeln!(f, "\n_elapsed: {:.1}s_", t0.elapsed().as_secs_f64())?;
    eprintln!("Wrote: {}", MD_OUT);

    Ok(())
}
