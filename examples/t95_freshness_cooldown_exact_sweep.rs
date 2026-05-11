//! T95: FRESHNESS_COOLDOWN Extensive Hyperopt — Exact Live-Bot Path
//!
//! Parameter: FRESHNESS_COOLDOWN ∈ [0..=100 step 1] (101 values)
//! - Controls bars to wait after exit before re-entry on the same symbol
//! - Currently hardcoded = 0 (disabled) in bot.rs — NEVER validated on exact-live path
//!
//! Using the EXACT live-bot semantics from live_bot_exact_equity.rs:
//! - Current-inclusive Turtle entry window (equality allowed)
//! - ATR_RANK gate (AP=17, LB=41, T=5.0)
//! - USDT hedge overlay (HEDGE_ATR_PERIOD=38, LB=252, PCT=0.45, SIZE=0.25)
//! - Turtle ATR-only exit (highest_high - 2.0 * ATR)
//! - HOLD_MAX timeout enforced even without ATR warmup
//! - Economic mark-to-market equity accounting
//!
//! Outputs:
//! - snapshots/t95_fc_sweep_summary.csv   — aggregate metrics per FC value
//! - snapshots/t95_fc_sweep_windows.csv  — per-universe per-window breakdown
//! - snapshots/t95_fc_equity_{fc}.csv     — daily equity curve for key FC values
//! - Chart: charts/t95_fc_comparison.png  — equity curves for baseline + winner + runner-ups

use anyhow::Result;
use krypto::data::loader::DataLoader;
use krypto::live::config::{
    ATR_ENTRY_MULT, ATR_RANK_THRESHOLD, HEDGE_ATR_PCT, HEDGE_ATR_PERIOD,
    HEDGE_LOOKBACK, HEDGE_SIZE_MULT, HOLD_MAX, POSITION_CAP, REGIME_ATR_PERIOD,
    REGIME_LOOKBACK, TURTLE_ATR_MULT, TURTLE_ATR_PERIOD, TURTLE_EP,
};
use std::collections::{HashMap, VecDeque};
use std::fs::File;
use std::io::Write;

const CANDLES: u32 = 3000;
const WARMUP_BARS: usize = 300;
const BASE_SYMBOLS: [&str; 6] = ["BTCUSDT", "ETHUSDT", "SOLUSDT", "XRPUSDT", "DOGEUSDT", "ADAUSDT"];

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

const TRAIN_BARS: usize = 252;
const TEST_BARS: usize = 252;
const MIN_TRADES: usize = 5;

const FC_VALUES: &[usize] = &[
    0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10,
    11, 12, 13, 14, 15, 16, 17, 18, 19, 20,
    21, 22, 23, 24, 25, 26, 27, 28, 29, 30,
    31, 32, 33, 34, 35, 36, 37, 38, 39, 40,
    41, 42, 43, 44, 45, 46, 47, 48, 49, 50,
    51, 52, 53, 54, 55, 56, 57, 58, 59, 60,
    61, 62, 63, 64, 65, 66, 67, 68, 69, 70,
    71, 72, 73, 74, 75, 76, 77, 78, 79, 80,
    81, 82, 83, 84, 85, 86, 87, 88, 89, 90,
    91, 92, 93, 94, 95, 96, 97, 98, 99, 100,
];

#[derive(Clone)]
struct SymData {
    close: Vec<f64>,
    high: Vec<f64>,
    low: Vec<f64>,
    dates: Vec<String>,
}

#[derive(Clone)]
struct PositionState {
    entry_bar: usize,
    entry_price: f64,
    entry_exec: f64,
    size: f64,
    highest_high: f64,
    lowest_low: f64,
    bars_held: usize,
    atr_buf: VecDeque<f64>,
}

#[derive(Clone)]
struct TradeRecord {
    sym_idx: usize,
    pct_ret: f64,
    equity_mult: f64,
    bars_held: usize,
    exit_reason: String,
    hedge_active: bool,
}

fn tr_at(sd: &SymData, idx: usize) -> f64 {
    let pc = if idx == 0 { sd.close[idx] } else { sd.close[idx - 1] };
    (sd.high[idx] - sd.low[idx])
        .max((sd.high[idx] - pc).abs())
        .max((sd.low[idx] - pc).abs())
}

fn atr_at(sd: &SymData, period: usize, idx: usize) -> f64 {
    if period == 0 || idx < period || idx >= sd.close.len() { return 0.0; }
    let start = idx + 1 - period;
    let mut sum = 0.0;
    for i in start..=idx { sum += tr_at(sd, i); }
    sum / period as f64
}

fn btc_atr_percentile(btc: &SymData, atr_period: usize, lookback: usize, idx: usize) -> f64 {
    let len = idx + 1;
    if len <= atr_period.max(lookback) + 1 { return 50.0; }
    let curr_atr = atr_at(btc, atr_period, idx);
    let curr_close = btc.close[idx];
    if curr_atr <= 0.0 || curr_close <= 0.0 { return 50.0; }
    let curr_pct = curr_atr / curr_close;
    let start = idx.saturating_sub(lookback);
    let mut below = 0usize;
    let mut total = 0usize;
    for i in start..idx {
        let close = btc.close[i];
        if close <= 0.0 { continue; }
        let hist_atr = atr_at(btc, atr_period, i);
        if hist_atr <= 0.0 { continue; }
        if hist_atr / close < curr_pct { below += 1; }
        total += 1;
    }
    if total == 0 { 50.0 } else { (below as f64 / total as f64) * 100.0 }
}

fn hedge_active(btc: &SymData, idx: usize) -> bool {
    let n = idx + 1;
    if n < HEDGE_LOOKBACK + HEDGE_ATR_PERIOD { return false; }
    let mut trs = Vec::with_capacity(HEDGE_ATR_PERIOD);
    for i in (n - HEDGE_ATR_PERIOD)..n { trs.push(tr_at(btc, i)); }
    let hedge_atr = trs.iter().sum::<f64>() / HEDGE_ATR_PERIOD as f64;
    let mut hist = Vec::with_capacity(HEDGE_LOOKBACK);
    for j in 1..=HEDGE_LOOKBACK {
        let hist_idx = n.saturating_sub(j);
        if hist_idx == 0 { break; }
        hist.push(tr_at(btc, hist_idx));
    }
    hist.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let pct_idx = (HEDGE_ATR_PCT * hist.len() as f64) as usize;
    hist.get(pct_idx).is_some_and(|&threshold| hedge_atr > threshold)
}

fn live_bot_entry_signal(sd: &SymData, idx: usize) -> bool {
    let len = idx + 1;
    if len < TURTLE_EP + 1 { return false; }
    let ws = len - TURTLE_EP;
    let max_close = sd.close[ws..=idx].iter().fold(f64::NEG_INFINITY, |a, &b| a.max(b));
    if sd.close[idx] < max_close { return false; }
    if ATR_ENTRY_MULT > 0.0 && len >= TURTLE_ATR_PERIOD + 1 {
        let atr = atr_at(sd, TURTLE_ATR_PERIOD, idx);
        let threshold = max_close + atr * ATR_ENTRY_MULT;
        if sd.close[idx] < threshold { return false; }
    }
    true
}

fn seed_atr_buf(sd: &SymData, idx: usize) -> VecDeque<f64> {
    let len = idx + 1;
    let avail = len.min(TURTLE_ATR_PERIOD);
    let start = len.saturating_sub(avail);
    let mut atr_buf = VecDeque::with_capacity(TURTLE_ATR_PERIOD);
    for offset in 0..avail {
        let b_idx = start + offset;
        if b_idx >= len { break; }
        let pc = if offset == 0 { sd.close[b_idx] } else { sd.close[start + offset - 1] };
        let tr = (sd.high[b_idx] - sd.low[b_idx])
            .max((sd.high[b_idx] - pc).abs())
            .max((sd.low[b_idx] - pc).abs());
        atr_buf.push_back(tr);
    }
    atr_buf
}

fn mtm_equity(realized_equity: f64, positions: &HashMap<String, PositionState>, data: &HashMap<String, SymData>, idx: usize, fee: f64) -> f64 {
    let mut open_ret = 0.0;
    for (sym, pos) in positions {
        if let Some(sd) = data.get(sym) {
            if idx < sd.close.len() {
                let liq_exec = sd.close[idx] * (1.0 - fee);
                open_ret += pos.size * (liq_exec / pos.entry_exec - 1.0);
            }
        }
    }
    realized_equity * (1.0 + open_ret)
}

fn annualised_sharpe(equity: &[f64]) -> f64 {
    if equity.len() < 2 { return 0.0; }
    let mut rets = Vec::with_capacity(equity.len() - 1);
    for w in equity.windows(2) { if w[0] > 0.0 { rets.push(w[1] / w[0] - 1.0); } }
    if rets.is_empty() { return 0.0; }
    let mean = rets.iter().sum::<f64>() / rets.len() as f64;
    let var = rets.iter().map(|x| (x - mean).powi(2)).sum::<f64>() / rets.len() as f64;
    if var <= 0.0 { 0.0 } else { (mean / var.sqrt()) * (365.0_f64).sqrt() }
}

fn max_dd(equity: &[f64]) -> f64 {
    let mut peak = 1.0;
    let mut max_dd = 0.0;
    for &eq in equity {
        if eq > peak { peak = eq; }
        if peak > 0.0 {
            let dd = 1.0 - eq / peak;
            if dd > max_dd { max_dd = dd; }
        }
    }
    max_dd * 100.0
}

/// Run exact-live backtest with a specific FC value.
/// Returns (final_equity, trades, equity_curve, open_count, trade_count)
fn run_backtest(
    data: &HashMap<String, SymData>,
    symbols: &[String],
    btc: &SymData,
    test_start: usize,
    test_end: usize,
    fc: usize,
) -> (f64, Vec<TradeRecord>, Vec<f64>, usize, usize) {
    let fee = 0.0004_f64;
    let mut realized_equity = 1.0_f64;
    let n_bars = test_end - test_start;
    let mut equity_curve = Vec::with_capacity(n_bars);

    let mut positions: HashMap<String, PositionState> = HashMap::new();
    let mut trades: Vec<TradeRecord> = Vec::new();
    let mut last_exit_bar: HashMap<String, usize> = HashMap::new();

    for offset in 0..n_bars {
        let idx = test_start + offset;
        let mut new_positions: HashMap<String, PositionState> = HashMap::new();

        for (sym_idx, sym) in symbols.iter().enumerate() {
            let sd = match data.get(sym) { Some(s) => s, None => continue };

            // Existing position: exit check
            if let Some(mut pos) = positions.remove(sym) {
                if idx > pos.entry_bar {
                    if sd.high[idx] > pos.highest_high { pos.highest_high = sd.high[idx]; }
                    if sd.low[idx] < pos.lowest_low { pos.lowest_low = sd.low[idx]; }
                    pos.bars_held += 1;

                    let prev_close = sd.close[idx];
                    let tr = (sd.high[idx] - sd.low[idx])
                        .max((sd.high[idx] - prev_close).abs())
                        .max((sd.low[idx] - prev_close).abs());
                    pos.atr_buf.push_back(tr);
                    if pos.atr_buf.len() > TURTLE_ATR_PERIOD { pos.atr_buf.pop_front(); }

                    let mut exit_reason: Option<String> = None;
                    if pos.bars_held >= HOLD_MAX {
                        exit_reason = Some("HOLD_MAX".to_string());
                    } else if pos.atr_buf.len() >= TURTLE_ATR_PERIOD {
                        let atr = pos.atr_buf.iter().sum::<f64>() / TURTLE_ATR_PERIOD as f64;
                        let turtle_stop = pos.highest_high - TURTLE_ATR_MULT * atr;
                        if atr > 0.0 && sd.low[idx] <= turtle_stop {
                            exit_reason = Some("TURTLE_ATR".to_string());
                        }
                    }

                    if let Some(reason) = exit_reason {
                        let exit_price = sd.close[idx];
                        let exit_exec = exit_price * (1.0 - fee);
                        let pct_ret = exit_exec / pos.entry_exec - 1.0;
                        let equity_mult = 1.0 + pos.size * pct_ret;
                        realized_equity *= equity_mult;
                        trades.push(TradeRecord {
                            sym_idx,
                            pct_ret,
                            equity_mult,
                            bars_held: idx.saturating_sub(pos.entry_bar),
                            exit_reason: reason,
                            hedge_active: pos.size < (1.0 / POSITION_CAP as f64),
                        });
                        last_exit_bar.insert(sym.clone(), idx);
                    } else {
                        new_positions.insert(sym.clone(), pos);
                    }
                } else {
                    new_positions.insert(sym.clone(), pos);
                }
                continue;
            }

            // Flat: check entry
            if new_positions.len() >= POSITION_CAP { continue; }

            // Freshness cooldown filter
            if let Some(&last_exit) = last_exit_bar.get(sym) {
                if offset.saturating_sub(last_exit.saturating_sub(test_start)) < fc { continue; }
            }

            if !live_bot_entry_signal(sd, idx) { continue; }

            let btc_pct = btc_atr_percentile(btc, REGIME_ATR_PERIOD, REGIME_LOOKBACK, idx);
            if btc_pct < ATR_RANK_THRESHOLD { continue; }

            let hedge = hedge_active(btc, idx);
            let mut size = 1.0 / POSITION_CAP as f64;
            if hedge { size *= HEDGE_SIZE_MULT; }

            new_positions.insert(sym.clone(), PositionState {
                entry_bar: idx,
                entry_price: sd.close[idx],
                entry_exec: sd.close[idx] * (1.0 + fee),
                size,
                highest_high: sd.high[idx],
                lowest_low: sd.low[idx],
                bars_held: 0,
                atr_buf: seed_atr_buf(sd, idx),
            });
        }

        positions = new_positions;
        equity_curve.push(mtm_equity(realized_equity, &positions, data, idx, fee));
    }

    let open_count = positions.len();
    let trade_count = trades.len();
    (realized_equity, trades, equity_curve, open_count, trade_count)
}

#[tokio::main]
async fn main() -> Result<()> {
    println!("=== T95: FRESHNESS_COOLDOWN Extensive Hyperopt (Exact Live Path) ===");
    println!("FC values: 0..=100 step 1 (101 values)");
    println!("9 universes × 6 walk-forward windows = 54 windows per FC");
    println!("\nExact live params: EP={}, ATR({},{}), HM={}, CAP={}", TURTLE_EP, TURTLE_ATR_PERIOD, TURTLE_ATR_MULT, HOLD_MAX, POSITION_CAP);
    println!("ATR_RANK: AP={}, LB={}, T={:.1}", REGIME_ATR_PERIOD, REGIME_LOOKBACK, ATR_RANK_THRESHOLD);
    println!("Hedge: HAP={}, HLB={}, HPCT={:.2}, HSIZE={:.2}\n", HEDGE_ATR_PERIOD, HEDGE_LOOKBACK, HEDGE_ATR_PCT, HEDGE_SIZE_MULT);

    let loader = DataLoader::new(None, None);
    let mut raw_data: HashMap<String, SymData> = HashMap::new();

    for sym in BASE_SYMBOLS {
        let df = loader.fetch_data(sym, "1d", CANDLES).await?;
        let close = df.column("close")?.f64()?.into_no_null_iter().collect::<Vec<_>>();
        let high = df.column("high")?.f64()?.into_no_null_iter().collect::<Vec<_>>();
        let low = df.column("low")?.f64()?.into_no_null_iter().collect::<Vec<_>>();
        let time_col = df.column("time").ok();
        let dates: Vec<String> = (0..close.len())
            .map(|i| {
                time_col.and_then(|s| s.get(i).ok())
                    .map(|v| v.to_string())
                    .unwrap_or_default()
            })
            .collect();
        raw_data.insert(sym.to_string(), SymData { close, high, low, dates });
    }

    // Align all symbols by common timestamps
    let mut common_dates = raw_data.get("BTCUSDT").expect("BTC").dates.clone();
    common_dates.retain(|d| raw_data.values().all(|sd| sd.dates.iter().any(|x| x == d)));
    common_dates.sort();
    common_dates.dedup();

    let mut data: HashMap<String, SymData> = HashMap::new();
    for (sym, sd) in &raw_data {
        let index_by_date: HashMap<String, usize> = sd.dates.iter().enumerate().map(|(i, d)| (d.clone(), i)).collect();
        let mut close = Vec::with_capacity(common_dates.len());
        let mut high = Vec::with_capacity(common_dates.len());
        let mut low = Vec::with_capacity(common_dates.len());
        let mut dates = Vec::with_capacity(common_dates.len());
        for d in &common_dates {
            if let Some(&i) = index_by_date.get(d) {
                close.push(sd.close[i]);
                high.push(sd.high[i]);
                low.push(sd.low[i]);
                dates.push(d.clone());
            }
        }
        data.insert(sym.clone(), SymData { close, high, low, dates });
    }

    let btc = data.get("BTCUSDT").expect("BTC").clone();
    let test_end = common_dates.len();
    let n_windows = 6;

    let summary_path = "/home/ubuntu/.openclaw/workspace-krypto/krypto/snapshots/t95_fc_sweep_summary.csv";
    let window_path = "/home/ubuntu/.openclaw/workspace-krypto/krypto/snapshots/t95_fc_sweep_windows.csv";
    let mut summary_file = File::create(summary_path)?;
    let mut window_file = File::create(window_path)?;
    writeln!(summary_file, "fc,pass_count,total_windows,pass_pct,avg_sharpe,avg_return_pct,avg_max_dd_pct,total_trades,win_rate_pct")?;
    writeln!(window_file, "fc,universe,window,equity,sharpe,max_dd,trades,win_rate,pass")?;

    // Per-FC summary
    let mut fc_pass: HashMap<usize, f64> = HashMap::new();
    let mut fc_sharpe: HashMap<usize, f64> = HashMap::new();
    let mut fc_return: HashMap<usize, f64> = HashMap::new();
    let mut fc_dd: HashMap<usize, f64> = HashMap::new();
    let mut fc_trades: HashMap<usize, usize> = HashMap::new();
    let mut fc_wins: HashMap<usize, usize> = HashMap::new();

    for &fc in FC_VALUES {
        let mut total_pass = 0usize;
        let mut total_runs = 0usize;
        let mut total_sharpe = 0.0_f64;
        let mut total_return = 0.0_f64;
        let mut total_dd = 0.0_f64;
        let mut total_trades = 0usize;
        let mut total_wins = 0usize;

        for &(uname, symbols) in UNIVERSES {
            let sym_list: Vec<String> = symbols.iter().map(|s| s.to_string()).collect();

            for w in 0..n_windows {
                let train_start = WARMUP_BARS + w * TEST_BARS;
                let test_start = train_start + TRAIN_BARS;
                let test_end_w = (test_start + TEST_BARS).min(test_end);

                if test_end_w - test_start < 60 { continue; }

                let (equity, trades, eq_curve, _, tc) = run_backtest(&data, &sym_list, &btc, test_start, test_end_w, fc);

                if tc < MIN_TRADES { continue; }

                let ret = (equity - 1.0) * 100.0;
                let sharpe = annualised_sharpe(&eq_curve);
                let maxdd = max_dd(&eq_curve);
                let wins = trades.iter().filter(|t| t.pct_ret > 0.0).count();
                let win_rate = if tc > 0 { wins as f64 / tc as f64 * 100.0 } else { 0.0 };
                let pass = if sharpe > 0.0 && equity > 1.0 { 1 } else { 0 };

                total_runs += 1;
                if pass == 1 { total_pass += 1; }
                total_sharpe += sharpe;
                total_return += ret;
                total_dd += maxdd;
                total_trades += tc;
                total_wins += wins;

                writeln!(window_file, "{},{},{},{:.6},{:.4},{:.2},{},{:.1},{}", fc, uname, w, equity, sharpe, maxdd, tc, win_rate, pass)?;
            }
        }

        let n_uni = total_runs.max(1) as f64;
        let pass_pct = total_pass as f64 / n_uni * 100.0;
        let avg_sharpe = total_sharpe / 9.0_f64;
        let avg_return = total_return / 9.0_f64;
        let avg_dd = total_dd / 9.0_f64;
        let win_rate = if total_trades > 0 { total_wins as f64 / total_trades as f64 * 100.0 } else { 0.0 };

        writeln!(summary_file, "{},{},{},{:.2},{:.4},{:.2},{:.2},{},{:.2}",
            fc, total_pass, total_runs, pass_pct, avg_sharpe, avg_return, avg_dd, total_trades, win_rate)?;

        fc_pass.insert(fc, pass_pct);
        fc_sharpe.insert(fc, avg_sharpe);
        fc_return.insert(fc, avg_return);
        fc_dd.insert(fc, avg_dd);
        fc_trades.insert(fc, total_trades);
        fc_wins.insert(fc, total_wins);
    }

    // Find top-3 FC values by pass rate, then Sharpe
    let mut sorted: Vec<usize> = FC_VALUES.to_vec();
    sorted.sort_by(|&a, &b| {
        let pa = fc_pass.get(&a).copied().unwrap_or(0.0);
        let pb = fc_pass.get(&b).copied().unwrap_or(0.0);
        let sa = fc_sharpe.get(&a).copied().unwrap_or(0.0);
        let sb = fc_sharpe.get(&b).copied().unwrap_or(0.0);
        pb.partial_cmp(&pa).unwrap()
            .then_with(|| sb.partial_cmp(&sa).unwrap())
    });

    println!("\nTop 10 by pass rate + Sharpe:");
    for fc in sorted.iter().take(10) {
        let pass = fc_pass.get(fc).copied().unwrap_or(0.0);
        let sharpe = fc_sharpe.get(fc).copied().unwrap_or(0.0);
        println!("  FC={:3}: pass={:5.1}%, Sharpe={:.4}", *fc, pass, sharpe);
    }

    // Export equity curves for baseline (FC=0) + winner + runner-ups
    let mut key_fcs: Vec<usize> = vec![0];
    for fc in sorted.iter().take(3) {
        if !key_fcs.contains(fc) { key_fcs.push(*fc); }
    }
    key_fcs.sort();

    let base_symbols: Vec<String> = BASE_SYMBOLS.iter().map(|s| s.to_string()).collect();
    let test_start_full = WARMUP_BARS;

    for fc in key_fcs.iter() {
        let (equity, trades, eq_curve, _, tc) = run_backtest(&data, &base_symbols, &btc, test_start_full, test_end, *fc);

        let eq_path = format!("/home/ubuntu/.openclaw/workspace-krypto/krypto/snapshots/t95_fc_equity_{}.csv", fc);
        let mut f = File::create(&eq_path)?;
        writeln!(f, "bar,date,equity")?;
        for (offset, eq) in eq_curve.iter().enumerate() {
            let idx = test_start_full + offset;
            let date = btc.dates.get(idx).cloned().unwrap_or_default();
            writeln!(f, "{},{},{:.10}", idx, date, eq)?;
        }

        let sharpe = annualised_sharpe(&eq_curve);
        let maxdd = max_dd(&eq_curve);
        let ret = (equity - 1.0) * 100.0;
        let wins = trades.iter().filter(|t| t.pct_ret > 0.0).count();
        let win_rate = if tc > 0 { wins as f64 / tc as f64 * 100.0 } else { 0.0 };
        println!("\nFC={:3}: equity={:.4}x, Sharpe={:.3}, MaxDD={:.1}, ret={:.1}, trades={}, win={:.1}", *fc, equity, sharpe, maxdd, ret, tc, win_rate);
    }

    println!("\n=== Sweep complete ===");
    println!("Summary: {}", summary_path);
    println!("Windows: {}", window_path);
    println!("Equity curves: snapshots/t95_fc_equity_{{fc}}.csv");
    println!("Key FC values charted: {:?}", key_fcs);

    Ok(())
}