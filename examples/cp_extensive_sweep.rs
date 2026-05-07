//! CHAND_PERIOD Extensive Sweep (3-100 step 1)
//!
//! PRIOR: CP=7 won a sparse sweep {5,7,9,11,...,59} step=2 — even periods never tested.
//! This sweeps ALL integers CP ∈ [3..100] step 1 to find the true optimum.
//!
//! Fixed params: EP=21, CHAND_MULT=2.30, ATR=24, ATR_MULT=2.0, ATR_ENTRY_MULT=0.00,
//!               HOLD_MAX=12, POSITION_CAP=3, VOL_LOOKBACK=92, ATR_RANK_T=5.0,
//!               REGIME_AP=17, REGIME_LB=41
//!
//! Grid: 98 values × 9 universes × 6 windows = 5,292 runs
//! Export: snapshots/cp_extensive_summary.csv, snapshots/cp_extensive_equity.csv

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

const TURTLE_EP: usize = 21;
const TURTLE_ATR_PERIOD: usize = 24;
const TURTLE_ATR_MULT: f64 = 2.00;
const CHAND_MULT: f64 = 2.30;
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

// Execute one trade: dual Chandelier + Turtle ATR exit
fn execute_trade(sd: &SymData, entry_px: f64, entry_bar_next: usize,
                 max_bar: usize, chand_p: usize, size_mult: f64) -> (f64, usize) {
    let mut highest_high = sd.high[entry_bar_next];
    let mut exit_bar = max_bar;
    let mut atr_buf: std::collections::VecDeque<f64> = std::collections::VecDeque::new();

    for b in entry_bar_next..=max_bar {
        if sd.high[b] > highest_high { highest_high = sd.high[b]; }
        let c0 = sd.close[b.saturating_sub(1)];
        let tr = (sd.high[b] - sd.low[b]).max((sd.high[b] - c0).abs()).max((sd.low[b] - c0).abs());
        atr_buf.push_back(tr);
        if atr_buf.len() > TURTLE_ATR_PERIOD { atr_buf.pop_front(); }

        let atr_ready = atr_buf.len() == TURTLE_ATR_PERIOD;
        let chand_ready = atr_buf.len() >= chand_p;

        // Chandelier stop
        let chand_stop = if chand_ready {
            let slice: Vec<f64> = atr_buf.iter().copied().collect();
            let sum: f64 = slice.iter().sum();
            let chand_atr = sum / chand_p as f64;
            highest_high - CHAND_MULT * chand_atr
        } else {
            f64::MAX
        };

        // Turtle ATR stop
        let turtle_stop = if atr_ready {
            let atr = atr_buf.iter().sum::<f64>() / TURTLE_ATR_PERIOD as f64;
            highest_high - TURTLE_ATR_MULT * atr
        } else {
            f64::MAX
        };

        let min_stop = chand_stop.min(turtle_stop);
        if sd.low[b] <= min_stop {
            exit_bar = b;
            break;
        }
    }

    let exit_idx = exit_bar.min(sd.close.len().saturating_sub(1));
    let exit_px = sd.close[exit_idx] * (1.0 - TAKER_FEE);
    let pct_ret = exit_px / entry_px - 1.0;
    let bars_held = (exit_bar as i64 - entry_bar_next as i64).max(1) as usize;
    (pct_ret * size_mult, bars_held)
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

fn run_sim(sym_data: &HashMap<String, SymData>, symbols: &[String],
           test_start: usize, test_end: usize, chand_p: usize) -> WfResult {
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
        let top_syms: Vec<String> = scores.into_iter().take(POSITION_CAP)
                                          .map(|(s, _)| s.to_string()).collect();

        if top_syms.is_empty() { equity_curve.push(equity); bar += 1; continue; }

        let mut entered = false;
        for sym in &top_syms {
            if let Some(sd) = sym_data.get(sym) {
                if bar >= TURTLE_EP + 1 && bar < sd.close.len() {
                    let max_close = sd.close[bar.saturating_sub(TURTLE_EP)..bar].iter()
                                    .fold(f64::NEG_INFINITY, |a, &b| a.max(b));
                    if sd.close[bar] > max_close {
                        let entry_px = sd.close[bar] * (1.0 + TAKER_FEE);
                        let size_mult = btc.map(|b| hedge_mult(b, bar)).unwrap_or(1.0);
                        let entry_bar_next = bar + 1;
                        let n = sd.close.len();
                        let max_bar = (entry_bar_next + HOLD_MAX).min(n.saturating_sub(1));
                        let (gross_ret, bars_held) =
                            execute_trade(sd, entry_px, entry_bar_next, max_bar, chand_p, size_mult);

                        total_trades += 1;
                        equity *= 1.0 + gross_ret;
                        let avg_daily = gross_ret / bars_held as f64;
                        for _ in 0..bars_held { daily_rets.push(avg_daily); }
                        if equity > peak { peak = equity; }
                        equity_curve.push(equity);
                        bar = max_bar + 1;
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

#[tokio::main]
async fn main() -> Result<()> {
    println!("CHAND_PERIOD Extensive Sweep: 98 values (3-100 step 1) × 9 universes × 6 windows\n");

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
    let n_w = UNIVERSES.len() * windows;
    println!("Data: {} bars, {} windows, {} total OOS runs\n", min_len, windows, n_w);

    // CP sweep: 3-100 step 1
    let cp_values: Vec<usize> = (3..=100).step_by(1).collect();
    println!("Sweeping {} CHAND_PERIOD values: {}..{}", cp_values.len(), cp_values[0], cp_values[cp_values.len()-1]);

    let mut eq_csv_f = File::create("/home/ubuntu/.openclaw/workspace-krypto/krypto/snapshots/cp_extensive_equity.csv")?;
    writeln!(eq_csv_f, "cp,universe,window,bar,equity")?;

    // Global results: (cp, avg_sharpe, pass_pct, pass_cnt, agg_eq, avg_ret, avg_dd, total_trades)
    let mut global_results: Vec<(usize, f64, f64, usize, f64)> = Vec::new();

    for &cp in &cp_values {
        let mut all_sh = Vec::new();
        let mut all_pass = 0usize;
        let mut global_agg_eq = 1.0_f64;

        for (u_name, u_syms) in UNIVERSES {
            let syms: Vec<String> = u_syms.iter().map(|s| s.to_string()).collect();
            let mut sh_list = Vec::new();
            let mut eq_list = Vec::new();

            for w in 0..windows {
                let start = min_len - (windows - w) * TEST_BARS - TRAIN_BARS;
                let end = start + TEST_BARS + TRAIN_BARS;
                let test_s = start + TRAIN_BARS;

                let r = run_sim(&sym_data, &syms, test_s, end, cp);

                sh_list.push(r.sharpe);
                eq_list.push(r.equity);
                if r.trades >= MIN_TRADES && r.sharpe > 0.0 { all_pass += 1; }

                writeln!(eq_csv_f, "{},{},{},{},{:.6}", cp, u_name, w, 0, 1.0)?;
                for (bi, &eq) in r.equity_curve.iter().enumerate() {
                    writeln!(eq_csv_f, "{},{},{},{},{:.6}", cp, u_name, w, bi, eq)?;
                }
            }

            let n = windows as f64;
            let agg_eq = eq_list.iter().fold(1.0_f64, |a, &b| a * b).powf(1.0/n);
            let sh_u = sh_list.iter().sum::<f64>() / n;
            all_sh.push(sh_u);
            global_agg_eq *= agg_eq;
        }

        let n_u = UNIVERSES.len() as f64;
        let avg_sh = all_sh.iter().sum::<f64>() / n_u;
        let pass_pct = all_pass as f64 / n_w as f64 * 100.0;
        global_results.push((cp, avg_sh, pass_pct, all_pass, global_agg_eq));

        if cp % 10 == 0 || cp <= 10 {
            println!("  CP={:3}: Sharpe={:.4}, pass={:.1}% ({}/{}), equity={:.4}",
                cp, avg_sh, pass_pct, all_pass, n_w, global_agg_eq);
        }
    }

    // Sort by Sharpe descending
    global_results.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());

    println!("\n=== TOP 15 CHAND_PERIOD VALUES (by Sharpe) ===");
    for (i, entry) in global_results.iter().take(15).enumerate() {
        let (cp, sh, pp, pc, eq) = entry;
        println!("  #{:2}: CP={:3}, Sharpe={:.4}, pass={:.1}% ({}/{}), equity={:.4}",
            i + 1, cp, sh, pp, pc, n_w, eq);
    }

    // Write sorted summary CSV
    let mut sum_f = File::create("/home/ubuntu/.openclaw/workspace-krypto/krypto/snapshots/cp_extensive_summary.csv")?;
    writeln!(sum_f, "cp,sharpe,pass_pct,pass_cnt,total_windows,equity_mult")?;
    for (cp, sh, pp, pc, eq) in &global_results {
        writeln!(sum_f, "{},{:.4},{:.2},{},{},{:.4}", cp, sh, pp, pc, n_w, eq)?;
    }

    // Also write in CP order for charting
    let mut ordered_f = File::create("/home/ubuntu/.openclaw/workspace-krypto/krypto/snapshots/cp_extensive_ordered.csv")?;
    writeln!(ordered_f, "cp,sharpe,pass_pct,pass_cnt,equity_mult")?;
    let mut ordered = global_results.clone();
    ordered.sort_by_key(|e| e.0);
    for (cp, sh, pp, pc, eq) in &ordered {
        writeln!(ordered_f, "{},{:.4},{:.2},{},{:.4}", cp, sh, pp, pc, eq)?;
    }

    println!("\nFiles:");
    println!("  snapshots/cp_extensive_summary.csv  — sorted by Sharpe");
    println!("  snapshots/cp_extensive_ordered.csv  — sorted by CP (in order)");
    println!("  snapshots/cp_extensive_equity.csv   — equity time-series");
    Ok(())
}
