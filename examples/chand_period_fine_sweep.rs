//! CHAND_PERIOD Fine-Grained Hyperopt: CP in [1..60] step=1 (60 values)
//!
//! PRIOR WORK:
//!   - CP step=2 coarse sweep: CP=7 won (Sharpe 5.908, 43/54 pass) — but 6 and 8 were never tested
//!   - All sweeps used either stale params or inconsistent fee models
//!
//! THIS SWEEP: 60 values × 9 universes × 6 windows = 3,240 window-runs
//!   Correct fee model: entry_px*(1+fee), exit_px*(1-fee)
//!   Current production params: EP=21, CM=2.30, ATR_P=24, ATR_M=2.0, HM=12, CAP=3, VL=8, EM=0.00

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

const CHAND_MULT: f64 = 2.30;
const TURTLE_ENTRY: usize = 21;
const TURTLE_ATR_PERIOD: usize = 24;
const TURTLE_ATR_MULT: f64 = 2.00;
const ATR_ENTRY_MULT: f64 = 0.00;
const VOL_LOOKBACK: usize = 8;

const CP_START: usize = 1;
const CP_END: usize = 60;
const N_CP: usize = CP_END - CP_START + 1; // 18

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

const CSV_METRICS: &str = "snapshots/chand_period_fine_metrics.csv";
const CSV_EQUITY:  &str = "snapshots/chand_period_fine_equity.csv";
const CSV_SUMMARY: &str = "snapshots/chand_period_fine_summary.csv";

struct SymData {
    close: Vec<f64>,
    high:  Vec<f64>,
    low:   Vec<f64>,
    vol:   Vec<f64>,
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

fn dollar_vol(vol: &[f64], close: &[f64], idx: usize) -> f64 {
    vol.get(idx).copied().unwrap_or(0.0) * close.get(idx).copied().unwrap_or(0.0)
}

fn rolling_avg(vals: &[f64], window: usize, idx: usize) -> f64 {
    if idx < window { return *vals.get(idx).unwrap_or(&0.0); }
    vals[idx + 1 - window..=idx].iter().sum::<f64>() / window as f64
}

fn turtle_signal(close: &[f64], high: &[f64], low: &[f64],
                 entry_period: usize, atr_period: usize, atr_mult: f64, idx: usize) -> bool {
    if idx < entry_period + 1 { return false; }
    let start = idx + 1 - entry_period;
    let mut max_close = f64::NEG_INFINITY;
    for i in start..idx {
        if let Some(&c) = close.get(i) { max_close = max_close.max(c); }
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
    equity_final: f64,
    equity_curve: Vec<f64>,
}

fn run_sim(cp: usize, sym_data: &HashMap<String, SymData>, symbols: &[String],
           test_start: usize, test_end: usize) -> WfResult {
    let mut equity = 1.0_f64;
    let mut equity_curve = vec![1.0f64];
    let mut peak = equity;
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
                if bar >= TURTLE_ENTRY + 1 && bar < sd.close.len() {
                    if turtle_signal(&sd.close, &sd.high, &sd.low,
                                    TURTLE_ENTRY, TURTLE_ATR_PERIOD, ATR_ENTRY_MULT, bar) {
                        let entry_px = sd.close[bar];
                        let entry = entry_px * (1.0 + TAKER_FEE);
                        let entry_bar_next = bar + 1;
                        let n = sd.close.len();

                        let mut highest_high_chand = sd.high[entry_bar_next];
                        let mut lowest_low_turtle  = sd.low[entry_bar_next];
                        let max_bar = (entry_bar_next + HOLD_MAX).min(n.saturating_sub(1));
                        let mut exit_bar = max_bar;
                        for b in entry_bar_next..=max_bar.min(n.saturating_sub(1)) {
                            highest_high_chand = highest_high_chand.max(sd.high[b]);
                            let atr_chand = atr_at(&sd.high, &sd.low, &sd.close, cp, b);
                            let trail_chand = highest_high_chand - CHAND_MULT * atr_chand;
                            lowest_low_turtle = lowest_low_turtle.min(sd.low[b]);
                            let atr_turtle = atr_at(&sd.high, &sd.low, &sd.close, TURTLE_ATR_PERIOD, b);
                            let trail_turtle = lowest_low_turtle - TURTLE_ATR_MULT * atr_turtle;
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

    WfResult { ret, sharpe, max_dd, trades: total_trades,
               win_rate, pass, equity_final: equity, equity_curve }
}

fn write_lines(path: &str, lines: &[String]) -> std::io::Result<()> {
    let mut f = File::create(path)?;
    for l in lines { writeln!(f, "{}", l)?; }
    Ok(())
}

#[tokio::main]
async fn main() -> Result<()> {
    let t0 = Instant::now();
    eprintln!("==== CHAND_PERIOD Fine-Grained Hyperopt: CP=[{}..{}] step=1 ({} values) ====", CP_START, CP_END, N_CP);

    let loader = DataLoader::new(None, None);
    let mut all_syms: HashMap<String, ()> = HashMap::new();
    for (_, syms) in UNIVERSES {
        for &s in *syms { all_syms.insert(s.to_string(), ()); }
    }

    let mut raw_cache: HashMap<String, DataFrame> = HashMap::new();
    let mut min_len = usize::MAX;
    for sym in all_syms.keys() {
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
    for sym in all_syms.keys() {
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

    let cps: Vec<usize> = (CP_START..=CP_END).collect();

    let mut cp_global_pass:   HashMap<usize, usize> = HashMap::new();
    let mut cp_global_total:  HashMap<usize, usize> = HashMap::new();
    let mut cp_global_trades: HashMap<usize, usize> = HashMap::new();
    let mut cp_sharpe_sum:    HashMap<usize, f64>   = HashMap::new();
    let mut cp_ret_sum:      HashMap<usize, f64>   = HashMap::new();
    let mut cp_dd_sum:       HashMap<usize, f64>   = HashMap::new();
    let mut cp_winrate_sum:  HashMap<usize, f64>   = HashMap::new();
    let mut cp_window_count: HashMap<usize, usize> = HashMap::new();

    let mut metric_lines = vec!["cp,universe,window,return_pct,sharpe,max_dd_pct,trades,win_rate_pct,pass,equity_final".to_string()];
    let mut equity_lines = vec!["cp,universe,window,step,equity".to_string()];

    for &(label, symbols) in UNIVERSES {
        let symbols: Vec<String> = symbols.iter().map(|s| s.to_string()).collect();
        if !symbols.iter().all(|s| sym_data_map.contains_key(s)) {
            eprintln!("{:>20} SKIPPED (missing data)", label);
            continue;
        }

        let total_windows = n.saturating_sub(TRAIN_BARS + TEST_BARS) / TEST_BARS;
        if total_windows == 0 {
            eprintln!("{:>20} SKIPPED (not enough data)", label);
            continue;
        }

        eprintln!("==== {:<18} ====", label);

        for wi in 0..total_windows {
            let train_end = TRAIN_BARS + wi * TEST_BARS;
            let test_start = train_end;
            let test_end = (test_start + TEST_BARS).min(n);
            if test_end.saturating_sub(test_start) < 5 { continue; }

            for cp in &cps {
                let r = run_sim(*cp, &sym_data_map, &symbols, test_start, test_end);

                metric_lines.push(format!(
                    "{},{},{},{:.2},{:.4},{:.2},{},{:.2},{},{:.6}",
                    cp, label, wi, r.ret, r.sharpe, r.max_dd,
                    r.trades, r.win_rate, r.pass, r.equity_final
                ));

                for (step_idx, &eq) in r.equity_curve.iter().enumerate() {
                    equity_lines.push(format!("{},{},{},{},{:.6}", cp, label, wi, step_idx, eq));
                }

                *cp_global_pass.entry(*cp).or_insert(0) += if r.pass { 1 } else { 0 };
                *cp_global_total.entry(*cp).or_insert(0) += 1;
                *cp_global_trades.entry(*cp).or_insert(0) += r.trades;
                *cp_sharpe_sum.entry(*cp).or_insert(0.0) += r.sharpe;
                *cp_ret_sum.entry(*cp).or_insert(0.0) += r.ret;
                *cp_dd_sum.entry(*cp).or_insert(0.0) += r.max_dd;
                *cp_winrate_sum.entry(*cp).or_insert(0.0) += r.win_rate;
                *cp_window_count.entry(*cp).or_insert(0) += 1;
            }
        }
    }

    let mut summary_lines = vec!["cp,global_pass,global_total,pass_rate,avg_sharpe,avg_ret,avg_dd,total_trades,avg_win_rate".to_string()];
    for &cp in &cps {
        let gp = cp_global_pass.get(&cp).copied().unwrap_or(0);
        let gt = cp_global_total.get(&cp).copied().unwrap_or(0);
        let wc = cp_window_count.get(&cp).copied().unwrap_or(0);
        let pass_rate = if gt > 0 { gp as f64 / gt as f64 * 100.0 } else { 0.0 };
        let avg_sharpe = if wc > 0 { cp_sharpe_sum.get(&cp).copied().unwrap_or(0.0) / wc as f64 } else { 0.0 };
        let avg_ret    = if wc > 0 { cp_ret_sum.get(&cp).copied().unwrap_or(0.0) / wc as f64 } else { 0.0 };
        let avg_dd     = if wc > 0 { cp_dd_sum.get(&cp).copied().unwrap_or(0.0) / wc as f64 } else { 0.0 };
        let avg_wr     = if gt > 0 { cp_winrate_sum.get(&cp).copied().unwrap_or(0.0) / gt as f64 } else { 0.0 };
        let trades     = cp_global_trades.get(&cp).copied().unwrap_or(0);
        summary_lines.push(format!("{},{},{},{:.2},{:.4},{:.2},{:.2},{},{:.2}", cp, gp, gt, pass_rate, avg_sharpe, avg_ret, avg_dd, trades, avg_wr));
    }

    write_lines(CSV_METRICS, &metric_lines)?;
    write_lines(CSV_EQUITY,  &equity_lines)?;
    write_lines(CSV_SUMMARY, &summary_lines)?;

    let mut rows: Vec<(usize, f64, f64, f64, usize, usize)> = cps.iter().map(|&cp| {
        let gp = cp_global_pass.get(&cp).copied().unwrap_or(0);
        let gt = cp_global_total.get(&cp).copied().unwrap_or(0);
        let wc = cp_window_count.get(&cp).copied().unwrap_or(0);
        let pct   = if gt > 0 { gp as f64 / gt as f64 } else { 0.0 };
        let ash   = if wc > 0 { cp_sharpe_sum.get(&cp).copied().unwrap_or(0.0) / wc as f64 } else { 0.0 };
        let art   = if wc > 0 { cp_ret_sum.get(&cp).copied().unwrap_or(0.0) / wc as f64 } else { 0.0 };
        let trades = cp_global_trades.get(&cp).copied().unwrap_or(0);
        (cp, pct, ash, art, trades, gt)
    }).collect();

    rows.sort_by(|a, b| {
        b.1.partial_cmp(&a.1).unwrap()
            .then_with(|| b.2.partial_cmp(&a.2).unwrap())
            .then_with(|| b.3.partial_cmp(&a.3).unwrap())
    });

    eprintln!("\n==== GLOBAL SUMMARY ====");
    for (i, &(cp, pct, ash, art, trades, gt)) in rows.iter().enumerate() {
        let gp = cp_global_pass.get(&cp).unwrap_or(&0);
        eprintln!("  {}. CP={:2}: {}/{} pass ({:.1}%), Sharpe={:.4}, Ret={:.1}%, Trades={}",
            i+1, cp, gp, gt, pct*100.0, ash, art, trades);
    }

    let winner = rows[0];
    eprintln!("\nRobustness winner: CP={} ({}/{} pass, Sharpe={:.4})",
        winner.0, cp_global_pass.get(&winner.0).unwrap_or(&0), winner.5, winner.2);

    let elapsed = t0.elapsed();
    eprintln!("\nDone in {:.1}s => {}", elapsed.as_secs_f64(), CSV_SUMMARY);

    // Markdown report
    let md_path = "snapshots/chand_period_fine_summary.md";
    let mut md = File::create(md_path)?;
    writeln!(md, "# CHAND_PERIOD Fine-Grained Hyperopt — 2026-05-01 00:50 UTC")?;
    writeln!(md, "")?;
    writeln!(md, "**Range:** CP in [{}..{}] step=1 ({} values)", CP_START, CP_END, N_CP)?;
    writeln!(md, "**Params:** EP=21, CM=2.30, ATR_P=24, HM=12, CAP=3, VL=8, EM=0.00")?;
    writeln!(md, "**Fee model:** entry*(1+fee), exit*(1-fee) (corrected)")?;
    writeln!(md, "")?;
    writeln!(md, "## Results")?;
    writeln!(md, "")?;
    writeln!(md, "| CP | Pass | Pass% | Avg Sharpe | Avg Ret% | Trades |")?;
    writeln!(md, "|---|-----|-------|------------|----------|--------|")?;
    for &(cp, pct, ash, art, trades, _) in &rows {
        let gp = cp_global_pass.get(&cp).unwrap_or(&0);
        let gt = cp_global_total.get(&cp).unwrap_or(&0);
        writeln!(md, "| {} | {}/{} | {:.1}% | {:.4} | {:.1}% | {} |",
            cp, gp, gt, pct*100.0, ash, art, trades)?;
    }
    writeln!(md, "")?;
    writeln!(md, "## Robustness Winner: CP={}", winner.0)?;
    writeln!(md, "")?;
    writeln!(md, "- Pass: {}/{} ({:.1}%)", cp_global_pass.get(&winner.0).unwrap_or(&0), winner.5, winner.1*100.0)?;
    writeln!(md, "- Avg Sharpe: {:.4}", winner.2)?;
    writeln!(md, "- Avg Ret: {:.1}%", winner.3)?;
    writeln!(md, "")?;
    writeln!(md, "_Generated in {:.1}s_", elapsed.as_secs_f64())?;

    eprintln!("Report: {}", md_path);
    Ok(())
}
