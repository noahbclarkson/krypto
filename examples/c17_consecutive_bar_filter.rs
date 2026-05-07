//! C17: Consecutive-Bar Momentum Filter
//! Baseline: Turtle fires on first close above max(close, EP).
//! Consecutive: requires close bar t and bar t-1 BOTH above max(close, EP).
//!
//! Exports: snapshots/c17_summary.csv, snapshots/c17_equity_timeseries.csv

use anyhow::Result;
use krypto::data::loader::DataLoader;
use std::collections::HashMap;
use std::fs::File;
use std::io::Write;

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
const VOL_LOOKBACK: usize = 92;

const REGIME_ATR_PERIOD: usize = 17;
const REGIME_LOOKBACK: usize = 41;
const ATR_RANK_T: f64 = 5.0;
const HEDGE_ATR_PCT: f64 = 0.45;
const HEDGE_SIZE_MULT: f64 = 0.40;
const HEDGE_ATR_PERIOD: usize = 38;
const HEDGE_LOOKBACK: usize = 252;

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
        let h = high[i];
        let l = low[i];
        let c0 = close[i.saturating_sub(1)];
        trs.push((h - l).max((h - c0).abs()).max((l - c0).abs()));
    }
    trs.iter().sum::<f64>() / period as f64
}

fn rolling_avg(vals: &[f64], window: usize, idx: usize) -> f64 {
    if idx < window { return 0.0; }
    vals[idx.saturating_sub(window - 1)..=idx].iter().sum::<f64>() / window as f64
}

fn btc_atr_pct(btc_data: &SymData, period: usize, lookback: usize, idx: usize) -> f64 {
    if idx < period.max(lookback) { return 50.0; }
    let curr_atr = atr_at(&btc_data.high, &btc_data.low, &btc_data.close, period, idx);
    let mut hist = Vec::new();
    for j in (idx + 1 - lookback)..=idx {
        if j >= period {
            hist.push(atr_at(&btc_data.high, &btc_data.low, &btc_data.close, period, j));
        }
    }
    if hist.is_empty() { return 50.0; }
    let count = hist.iter().filter(|&&x| x < curr_atr).count();
    (count as f64 / hist.len() as f64) * 100.0
}

fn annualised_sharpe(daily_rets: &[f64]) -> f64 {
    if daily_rets.is_empty() { return 0.0; }
    let mean = daily_rets.iter().sum::<f64>() / daily_rets.len() as f64;
    let var = daily_rets.iter().map(|x| (x - mean).powi(2)).sum::<f64>() / daily_rets.len() as f64;
    if var == 0.0 { return 0.0; }
    (mean / var.sqrt()) * (365.0_f64).sqrt()
}

fn max_dd_from(equity: &[f64]) -> f64 {
    let mut max_dd = 0.0;
    let mut peak = 1.0;
    for &val in equity {
        if val > peak { peak = val; }
        let dd = 1.0 - val / peak;
        if dd > max_dd { max_dd = dd; }
    }
    max_dd * 100.0
}

struct WfResult {
    equity: f64,
    sharpe: f64,
    dd: f64,
    trades: usize,
    equity_curve: Vec<f64>,
}

// ── Shared trade execution ─────────────────────────────────────────────────────
fn execute_trade(sd: &SymData, entry_px: f64, entry_bar_next: usize, max_bar: usize,
                 size_mult: f64) -> (f64, usize) {
    let mut highest_high = sd.high[entry_bar_next];
    let mut exit_bar = max_bar;
    let mut atr_buf: std::collections::VecDeque<f64> = std::collections::VecDeque::new();

    for b in entry_bar_next..=max_bar {
        if sd.high[b] > highest_high { highest_high = sd.high[b]; }
        let c0 = sd.close[b.saturating_sub(1)];
        let tr = (sd.high[b] - sd.low[b]).max((sd.high[b] - c0).abs()).max((sd.low[b] - c0).abs());
        atr_buf.push_back(tr);
        if atr_buf.len() > TURTLE_ATR_PERIOD { atr_buf.pop_front(); }
        if atr_buf.len() == TURTLE_ATR_PERIOD {
            let atr = atr_buf.iter().sum::<f64>() / TURTLE_ATR_PERIOD as f64;
            let turtle_stop = highest_high - TURTLE_ATR_MULT * atr;
            if sd.low[b] <= turtle_stop { exit_bar = b; break; }
        }
    }

    if let Some(&exit_px) = sd.close.get(exit_bar) {
        let exit = exit_px * (1.0 - TAKER_FEE);
        let pct_ret = exit / entry_px - 1.0;
        let gross_ret = pct_ret * size_mult;
        let bars_held = (exit_bar as i64 - entry_bar_next as i64).max(1) as usize;
        (gross_ret, bars_held)
    } else {
        let exit_px = sd.close[sd.close.len() - 1];
        let exit = exit_px * (1.0 - TAKER_FEE);
        let pct_ret = exit / entry_px - 1.0;
        let gross_ret = pct_ret * size_mult;
        (gross_ret, max_bar - entry_bar_next + 1)
    }
}

fn hedge_mult(btc_data: &SymData, bar: usize) -> f64 {
    if bar < 252 + 21 { return 1.0; }
    let atr_21 = atr_at(&btc_data.high, &btc_data.low, &btc_data.close, 21, bar);
    let mut hist = Vec::new();
    for j in (bar + 1 - 252)..=bar {
        let h = btc_data.high[j];
        let l = btc_data.low[j];
        let c0 = btc_data.close[j.saturating_sub(1)];
        hist.push((h - l).max((h - c0).abs()).max((l - c0).abs()));
    }
    hist.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let pct_idx = (HEDGE_ATR_PCT * hist.len() as f64) as usize;
    let pct_threshold = hist[pct_idx.min(hist.len().saturating_sub(1))];
    if atr_21 > pct_threshold { HEDGE_SIZE_MULT } else { 1.0 }
}

// ── Baseline: 1-bar confirmation ────────────────────────────────────────────────
fn run_sim_base(sym_data: &HashMap<String, SymData>, symbols: &[String],
                test_start: usize, test_end: usize) -> WfResult {
    let mut equity = 1.0_f64;
    let mut equity_curve = vec![1.0_f64];
    let mut peak = equity;
    let mut total_trades = 0usize;
    let mut daily_rets = Vec::new();

    let mut bar = test_start;
    while bar + 2 < test_end {
        let btc = sym_data.get("BTCUSDT");
        let btc_pct = btc.map(|b| btc_atr_pct(b, REGIME_ATR_PERIOD, REGIME_LOOKBACK, bar))
                        .unwrap_or(50.0);
        if btc_pct < ATR_RANK_T {
            equity_curve.push(equity);
            bar += 1;
            continue;
        }

        // DV ranking
        let mut scores: Vec<(&str, f64)> = Vec::new();
        for sym in symbols.iter() {
            if let Some(sd) = sym_data.get(sym) {
                if bar >= sd.close.len() { continue; }
                let rol_vol = rolling_avg(&sd.vol, VOL_LOOKBACK, bar);
                let price = sd.close[bar];
                let dv = rol_vol * price;
                if dv.is_finite() && dv > 0.0 { scores.push((sym.as_str(), dv)); }
            }
        }
        scores.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());
        let top_syms: Vec<String> = scores.into_iter().take(POSITION_CAP).map(|(s, _)| s.to_string()).collect();

        if top_syms.is_empty() { equity_curve.push(equity); bar += 1; continue; }

        let mut entered = false;
        for sym in &top_syms {
            if let Some(sd) = sym_data.get(sym) {
                if bar >= TURTLE_ENTRY + 1 && bar < sd.close.len() {
                    let max_close = sd.close[bar.saturating_sub(TURTLE_ENTRY)..bar].iter()
                                    .fold(f64::NEG_INFINITY, |a, &b| a.max(b));
                    if sd.close[bar] > max_close {
                        let entry_px = sd.close[bar];
                        let size_mult = btc.map(|b| hedge_mult(b, bar)).unwrap_or(1.0);
                        let entry = entry_px * (1.0 + TAKER_FEE);
                        let entry_bar_next = bar + 1;
                        let n = sd.close.len();
                        let max_bar = (entry_bar_next + HOLD_MAX).min(n.saturating_sub(1));
                        let (gross_ret, bars_held) = execute_trade(sd, entry, entry_bar_next, max_bar, size_mult);

                        total_trades += 1;
                        equity *= 1.0 + gross_ret;
                        let avg_daily = gross_ret / bars_held as f64;
                        for _ in 0..bars_held { daily_rets.push(avg_daily); }
                        if equity > peak { peak = equity; }
                        equity_curve.push(equity);
                        bar = if let Some(&exit_px) = sd.close.get(max_bar) { max_bar } else { bar } + 1;
                        entered = true;
                        break;
                    }
                }
            }
        }
        if !entered { equity_curve.push(equity); bar += 1; }
    }

    WfResult { equity, sharpe: annualised_sharpe(&daily_rets), dd: max_dd_from(&equity_curve),
               trades: total_trades, equity_curve }
}

// ── Consecutive: 2-bar confirmation ────────────────────────────────────────────
fn run_sim_cons(sym_data: &HashMap<String, SymData>, symbols: &[String],
                test_start: usize, test_end: usize) -> WfResult {
    let mut equity = 1.0_f64;
    let mut equity_curve = vec![1.0_f64];
    let mut peak = equity;
    let mut total_trades = 0usize;
    let mut daily_rets = Vec::new();

    // pending: (symbol, entry_price)
    let mut pending: Option<(String, f64)> = None;

    let mut bar = test_start;
    while bar + 2 < test_end {
        let btc = sym_data.get("BTCUSDT");
        let btc_pct = btc.map(|b| btc_atr_pct(b, REGIME_ATR_PERIOD, REGIME_LOOKBACK, bar))
                        .unwrap_or(50.0);

        // ── resolve pending from previous bar ──────────────────────────────────
        if let Some((ref sym, entry_px)) = pending {
            if let Some(sd) = sym_data.get(sym.as_str()) {
                let max_close = sd.close[bar.saturating_sub(TURTLE_ENTRY)..bar].iter()
                                .fold(f64::NEG_INFINITY, |a, &b| a.max(b));
                if sd.close[bar] > max_close {
                    // Confirmed: enter
                    let size_mult = btc.map(|b| hedge_mult(b, bar)).unwrap_or(1.0);
                    let entry_bar_next = bar + 1;
                    let n = sd.close.len();
                    let max_bar = (entry_bar_next + HOLD_MAX).min(n.saturating_sub(1));
                    let (gross_ret, bars_held) = execute_trade(sd, entry_px, entry_bar_next, max_bar, size_mult);

                    total_trades += 1;
                    equity *= 1.0 + gross_ret;
                    let avg_daily = gross_ret / bars_held as f64;
                    for _ in 0..bars_held { daily_rets.push(avg_daily); }
                    if equity > peak { peak = equity; }
                    equity_curve.push(equity);
                    bar = max_bar + 1;
                    pending = None;
                    continue;
                }
                // Not confirmed: entry rejected, clear pending and fall through
                pending = None;
            } else {
                pending = None;
            }
        }

        // ── new entry scan ────────────────────────────────────────────────────
        if btc_pct >= ATR_RANK_T {
            let mut scores: Vec<(&str, f64)> = Vec::new();
            for sym in symbols.iter() {
                if let Some(sd) = sym_data.get(sym) {
                    if bar >= sd.close.len() { continue; }
                    let rol_vol = rolling_avg(&sd.vol, VOL_LOOKBACK, bar);
                    let price = sd.close[bar];
                    let dv = rol_vol * price;
                    if dv.is_finite() && dv > 0.0 { scores.push((sym.as_str(), dv)); }
                }
            }
            scores.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());
            let top_syms: Vec<String> = scores.into_iter().take(POSITION_CAP).map(|(s, _)| s.to_string()).collect();

            for sym in &top_syms {
                if let Some(sd) = sym_data.get(sym.as_str()) {
                    if bar >= TURTLE_ENTRY + 1 && bar < sd.close.len() {
                        let max_close = sd.close[bar.saturating_sub(TURTLE_ENTRY)..bar].iter()
                                        .fold(f64::NEG_INFINITY, |a, &b| a.max(b));
                        if sd.close[bar] > max_close {
                            let entry_px = sd.close[bar] * (1.0 + TAKER_FEE);
                            pending = Some((sym.clone(), entry_px));
                            break;
                        }
                    }
                }
                if pending.is_some() { break; }
            }
        }

        equity_curve.push(equity);
        bar += 1;
    }

    WfResult { equity, sharpe: annualised_sharpe(&daily_rets), dd: max_dd_from(&equity_curve),
               trades: total_trades, equity_curve }
}

#[tokio::main]
async fn main() -> Result<()> {
    println!("C17 Consecutive-Bar Filter — 9 universes × 7 windows\n");

    let loader = DataLoader::new(None, None);
    let mut sym_data: HashMap<String, SymData> = HashMap::new();
    let mut min_len = usize::MAX;

    let all_syms: std::collections::HashSet<&str> = UNIVERSES.iter()
        .flat_map(|(_, s)| s.iter().copied()).collect();

    for &sym in &all_syms {
        let df = loader.fetch_data(sym, "1d", CANDLES).await?;
        let close = df.column("close")?.f64()?.into_no_null_iter().collect::<Vec<_>>();
        let high = df.column("high")?.f64()?.into_no_null_iter().collect::<Vec<_>>();
        let low  = df.column("low")?.f64()?.into_no_null_iter().collect::<Vec<_>>();
        let vol  = df.column("volume")?.f64()?.into_no_null_iter().collect::<Vec<_>>();
        if close.len() < min_len { min_len = close.len(); }
        sym_data.insert(sym.to_string(), SymData { close, high, low, vol });
    }

    let windows = (min_len.saturating_sub(TRAIN_BARS)) / TEST_BARS;
    println!("Data: {} bars, {} windows\n", min_len, windows);

    let mut csv_f = File::create("/home/ubuntu/.openclaw/workspace-krypto/krypto/snapshots/c17_summary.csv")?;
    writeln!(csv_f, "universe,base_equity,cons_equity,eq_delta_pct,sh_base,sh_cons,sh_delta,dd_base,dd_cons,tr_base,tr_cons,pass_base,pass_cons,total_win")?;

    let mut all_sh_base = Vec::new();
    let mut all_sh_cons = Vec::new();
    let mut global_base_pass = 0usize;
    let mut global_cons_pass = 0usize;
    let mut total_windows = 0usize;
    let mut global_base_agg = 1.0_f64;
    let mut global_cons_agg = 1.0_f64;

    for (u_name, u_syms) in UNIVERSES {
        let syms: Vec<String> = u_syms.iter().map(|s| s.to_string()).collect();
        let mut base_pass = 0usize;
        let mut cons_pass = 0usize;
        let mut sh_base_list = Vec::new();
        let mut sh_cons_list = Vec::new();
        let mut eq_base_list = Vec::new();
        let mut eq_cons_list = Vec::new();
        let mut last_dd_base = 0.0_f64;
        let mut last_dd_cons = 0.0_f64;

        for w in 0..windows {
            let start = min_len - (windows - w) * TEST_BARS - TRAIN_BARS;
            let end = start + TEST_BARS + TRAIN_BARS;
            let test_s = start + TRAIN_BARS;

            let r_base = run_sim_base(&sym_data, &syms, test_s, end);
            let r_cons = run_sim_cons(&sym_data, &syms, test_s, end);

            eq_base_list.push(r_base.equity);
            eq_cons_list.push(r_cons.equity);
            sh_base_list.push(r_base.sharpe);
            sh_cons_list.push(r_cons.sharpe);
            last_dd_base = r_base.dd;
            last_dd_cons = r_cons.dd;

            if r_base.trades >= MIN_TRADES && r_base.sharpe > 0.0 { base_pass += 1; }
            if r_cons.trades >= MIN_TRADES && r_cons.sharpe > 0.0 { cons_pass += 1; }

            println!("  [{}/{}] base: {:.3}x sh={:.2} tr={} | cons: {:.3}x sh={:.2} tr={}",
                u_name, w, r_base.equity, r_base.sharpe, r_base.trades,
                r_cons.equity, r_cons.sharpe, r_cons.trades);
        }

        let n = windows as f64;
        let agg_base = eq_base_list.iter().fold(1.0_f64, |a, &b| a * b).powf(1.0/n);
        let agg_cons = eq_cons_list.iter().fold(1.0_f64, |a, &b| a * b).powf(1.0/n);
        let sh_base = sh_base_list.iter().sum::<f64>() / n;
        let sh_cons = sh_cons_list.iter().sum::<f64>() / n;

        global_base_pass += base_pass;
        global_cons_pass += cons_pass;
        total_windows += windows;
        all_sh_base.push(sh_base);
        all_sh_cons.push(sh_cons);
        global_base_agg *= agg_base;
        global_cons_agg *= agg_cons;

        let eq_delta = (agg_cons/agg_base - 1.0)*100.0;
        let sh_delta = sh_cons - sh_base;
        writeln!(csv_f, "{},{:.6},{:.6},{:.2},{:.4},{:.4},{:.4},{:.1},{:.1},{},{},{},{},{}",
            u_name, agg_base, agg_cons, eq_delta,
            sh_base, sh_cons, sh_delta,
            last_dd_base, last_dd_cons,
            base_pass * 5, cons_pass * 5,
            base_pass, cons_pass, windows)?;

        println!("  [{} AGG] base: {:.4}x sh={:.3} {}/{} | cons: {:.4}x sh={:.3} {}/{} | delta={:.1}%",
            u_name, agg_base, sh_base, base_pass, windows,
            agg_cons, sh_cons, cons_pass, windows,
            ((agg_cons/agg_base - 1.0)*100.0));
    }

    let n_u = UNIVERSES.len() as f64;
    let avg_sh_base = all_sh_base.iter().sum::<f64>() / n_u;
    let avg_sh_cons = all_sh_cons.iter().sum::<f64>() / n_u;
    let base_pct = global_base_pass as f64 / total_windows as f64 * 100.0;
    let cons_pct = global_cons_pass as f64 / total_windows as f64 * 100.0;

    println!("\n=== GLOBAL ===");
    println!("Baseline:   {}/{} pass ({:.1}%), agg_eq={:.4}x, avg_sharpe={:.4}",
        global_base_pass, total_windows, base_pct, global_base_agg, avg_sh_base);
    println!("Consecutive:{}/{} pass ({:.1}%), agg_eq={:.4}x, avg_sharpe={:.4}",
        global_cons_pass, total_windows, cons_pct, global_cons_agg, avg_sh_cons);
    let eq_delta = (global_cons_agg/global_base_agg - 1.0)*100.0;
    let sh_delta = avg_sh_cons - avg_sh_base;
    println!("delta equity: {:.1}% | delta sharpe: {:.4}", eq_delta, sh_delta);

    // Equity time-series
    let base5_syms: Vec<String> = UNIVERSES[0].1.iter().map(|s| s.to_string()).collect();
    let mut eq_f = File::create("/home/ubuntu/.openclaw/workspace-krypto/krypto/snapshots/c17_equity_timeseries.csv")?;
    writeln!(eq_f, "window,bar,base_equity,cons_equity")?;
    for w in 0..windows {
        let start = min_len - (windows - w) * TEST_BARS - TRAIN_BARS;
        let end = start + TEST_BARS + TRAIN_BARS;
        let r_base = run_sim_base(&sym_data, &base5_syms, start + TRAIN_BARS, end);
        let r_cons = run_sim_cons(&sym_data, &base5_syms, start + TRAIN_BARS, end);
        let len = r_base.equity_curve.len().min(r_cons.equity_curve.len());
        for bi in 0..len {
            writeln!(eq_f, "{},{},{:.6},{:.6}", w, bi, r_base.equity_curve[bi], r_cons.equity_curve[bi])?;
        }
    }

    println!("\n=== VERDICT ===");
    if cons_pct > base_pct && avg_sh_cons > avg_sh_base {
        println!("PROMOTE: consecutive improves BOTH pass ({:.0}%→{:.0}%) AND Sharpe ({:.3}→{:.3})",
            base_pct, cons_pct, avg_sh_base, avg_sh_cons);
    } else if cons_pct > base_pct {
        println!("CLOSE: pass improves ({:.0}%→{:.0}%) but Sharpe drops ({:.3}→{:.3})",
            base_pct, cons_pct, avg_sh_base, avg_sh_cons);
    } else if avg_sh_cons > avg_sh_base {
        println!("CLOSE: Sharpe improves ({:.3}→{:.3}) but pass drops ({:.0}%→{:.0}%)",
            avg_sh_base, avg_sh_cons, base_pct, cons_pct);
    } else {
        println!("REJECT: degrades both — pass {:.0}%→{:.0}%, Sharpe {:.3}→{:.3}",
            base_pct, cons_pct, avg_sh_base, avg_sh_cons);
    }

    println!("\nFiles: snapshots/c17_summary.csv, snapshots/c17_equity_timeseries.csv");
    Ok(())
}
