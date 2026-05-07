//! T84: HEDGE_LOOKBACK extensive hyperopt under CURRENT production params.
//!
//! Background:
//!   HEDGE_LOOKBACK=252 is hardcoded in `src/live/bot.rs` — never systematically
//!   optimized under current production params.
//!   UNDER CURRENT params: HEDGE_ATR_PERIOD=38 (T75 winner), HEDGE_ATR_PCT=0.45,
//!   HEDGE_SIZE_MULT=0.25 (T83 winner), ATR_RANK(AP=17/LB=41/T=5).
//!
//! Sweep:
//!   HEDGE_LOOKBACK ∈ {21, 42, 63, 84, 105, 126, 147, 168, 189, 210,
//!                     252, 294, 336, 378, 420, 504, 630}
//!   17 values × 9 universes × 7 WF windows
//!
//! Output:
//!   snapshots/t84_hedge_lookback_summary.csv
//!   snapshots/t84_hedge_lookback_windows.csv

use anyhow::Result;
use krypto::data::loader::DataLoader;
use std::collections::{HashMap, HashSet, VecDeque};
use std::fs::File;
use std::io::Write;

const CANDLES: u32 = 3000;
const TRAIN_BARS: usize = 252;
const TEST_BARS: usize = 252;
const WARMUP_BARS: usize = 300;
const MIN_TRADES: usize = 3;

// Current production params (from config.rs, HOF)
const TURTLE_EP: usize = 21;
const TURTLE_ATR_PERIOD: usize = 24;
const TURTLE_ATR_MULT: f64 = 2.0;
const ATR_ENTRY_MULT: f64 = 0.00;
const HOLD_MAX: usize = 15;          // T81 winner (2026-05-07)
const POSITION_CAP: usize = 3;
const REGIME_ATR_PERIOD: usize = 17;  // T78 winner
const REGIME_LOOKBACK: usize = 41;    // T78 winner
const ATR_RANK_THRESHOLD: f64 = 5.0;

// Hedge params (FIXED — not swept here)
const HEDGE_ATR_PERIOD: usize = 38;  // T75 winner
const HEDGE_ATR_PCT: f64 = 0.45;     // T67 winner
const HEDGE_SIZE_MULT: f64 = 0.25;   // T83 winner
const FEE: f64 = 0.0004;

const LB_VALUES: &[usize] = &[
    21, 42, 63, 84, 105, 126, 147, 168, 189, 210,
    252, 294, 336, 378, 420, 504, 630,
];

const SUMMARY_OUT: &str = "/home/ubuntu/.openclaw/workspace-krypto/krypto/snapshots/t84_hedge_lookback_summary.csv";
const WINDOWS_OUT: &str = "/home/ubuntu/.openclaw/workspace-krypto/krypto/snapshots/t84_hedge_lookback_windows.csv";

const UNIVERSES: &[(&str, &[&str])] = &[
    ("Base5", &["BTCUSDT", "ETHUSDT", "SOLUSDT", "XRPUSDT", "DOGEUSDT", "ADAUSDT"]),
    ("NoDOGE", &["BTCUSDT", "ETHUSDT", "SOLUSDT", "XRPUSDT", "ADAUSDT"]),
    ("LargeCaps5", &["BTCUSDT", "ETHUSDT", "SOLUSDT", "XRPUSDT", "ADAUSDT"]),
    ("LowVolume5", &["LTCUSDT", "EOSUSDT", "BCHUSDT", "NEOUSDT", "QTUMUSDT"]),
    ("Legacy3", &["BTCUSDT", "XRPUSDT", "LTCUSDT"]),
    ("Legacy4", &["BTCUSDT", "XRPUSDT", "LTCUSDT", "ETHUSDT"]),
    ("HighBeta3", &["SOLUSDT", "DOGEUSDT", "ADAUSDT"]),
    ("MidCaps5", &["AVAXUSDT", "MATICUSDT", "DOTUSDT", "LINKUSDT", "UNIUSDT"]),
    ("All10", &["BTCUSDT","ETHUSDT","SOLUSDT","XRPUSDT","DOGEUSDT","ADAUSDT","LTCUSDT","EOSUSDT","BCHUSDT","BNBUSDT"]),
];

#[derive(Clone)]
struct SymData {
    close: Vec<f64>,
    high: Vec<f64>,
    low: Vec<f64>,
}

fn tr_at(sd: &SymData, idx: usize) -> f64 {
    let pc = if idx == 0 { sd.close[idx] } else { sd.close[idx - 1] };
    (sd.high[idx] - sd.low[idx])
        .max((sd.high[idx] - pc).abs())
        .max((sd.low[idx] - pc).abs())
}

fn atr_at(sd: &SymData, period: usize, idx: usize) -> f64 {
    if period == 0 || idx < period { return 0.0; }
    let mut sum = 0.0;
    for i in (idx + 1 - period)..=idx {
        sum += tr_at(sd, i);
    }
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

// Hedge active: hedge ATR > HEDGE_ATR_PCT percentile of its TR history
fn hedge_active(btc: &SymData, idx: usize, hedge_period: usize) -> bool {
    let n = idx + 1;
    if hedge_period == 0 || n < 252 + hedge_period { return false; }
    let mut trs = Vec::with_capacity(hedge_period);
    for i in (n - hedge_period)..n {
        trs.push(tr_at(btc, i));
    }
    let curr_atr = trs.iter().sum::<f64>() / hedge_period as f64;
    let mut hist = Vec::with_capacity(252);
    for j in 1..=252 {
        let hist_idx = n.saturating_sub(j);
        if hist_idx == 0 { break; }
        hist.push(tr_at(btc, hist_idx));
    }
    hist.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let pct_idx = (HEDGE_ATR_PCT * hist.len() as f64) as usize;
    hist.get(pct_idx).is_some_and(|&threshold| curr_atr > threshold)
}

fn live_entry_signal(sd: &SymData, idx: usize) -> bool {
    let len = idx + 1;
    if len < TURTLE_EP + 1 { return false; }
    // current-inclusive max-close window, equality allowed (mirrors bot.rs)
    let ws = len - TURTLE_EP;
    let max_close = sd.close[ws..=idx].iter().fold(f64::NEG_INFINITY, |a, &b| a.max(b));
    sd.close[idx] >= max_close
}

fn seed_atr_buf(sd: &SymData, idx: usize) -> VecDeque<f64> {
    let len = idx + 1;
    let avail = len.min(TURTLE_ATR_PERIOD);
    let start = len.saturating_sub(avail);
    let mut atr_buf = VecDeque::with_capacity(TURTLE_ATR_PERIOD);
    for offset in 0..avail {
        let b_idx = start + offset;
        let pc = if offset == 0 { sd.close[b_idx] } else { sd.close[start + offset - 1] };
        let tr = (sd.high[b_idx] - sd.low[b_idx])
            .max((sd.high[b_idx] - pc).abs())
            .max((sd.low[b_idx] - pc).abs());
        atr_buf.push_back(tr);
    }
    atr_buf
}

fn annualised_sharpe_from_equity(equity: &[f64]) -> f64 {
    if equity.len() < 2 { return 0.0; }
    let mut rets = Vec::with_capacity(equity.len() - 1);
    for w in equity.windows(2) {
        if w[0] > 0.0 { rets.push(w[1] / w[0] - 1.0); }
    }
    if rets.is_empty() { return 0.0; }
    let mean = rets.iter().sum::<f64>() / rets.len() as f64;
    let var = rets.iter().map(|x| (x - mean).powi(2)).sum::<f64>() / rets.len() as f64;
    if var <= 0.0 { 0.0 } else { (mean / var.sqrt()) * 365.0_f64.sqrt() }
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

fn run_sim(
    data: &HashMap<String, SymData>,
    symbols: &[String],
    test_start: usize,
    test_end: usize,
    hedge_lb: usize,
) -> (f64, f64, f64, usize, usize, Vec<f64>) {
    // Use BTC as regime/hedge reference (first symbol)
    let btc = data.get("BTCUSDT").unwrap();
    let mut realized_equity = 1.0_f64;
    let mut equity_curve: Vec<f64> = Vec::with_capacity(test_end.saturating_sub(test_start));
    let mut positions: HashMap<String, PositionState> = HashMap::new();
    let mut last_exit_bar: HashMap<String, usize> = HashMap::new();
    let mut trades = 0usize;
    let mut wins = 0usize;

    #[derive(Clone)]
    struct PositionState {
        entry_bar: usize,
        entry_exec: f64,
        size: f64,
        highest_high: f64,
        lowest_low: f64,
        bars_held: usize,
        atr_buf: VecDeque<f64>,
    }

    for idx in test_start..test_end {
        for sym in symbols {
            let sd = match data.get(sym) {
                Some(sd) => sd,
                None => continue,
            };
            if idx >= sd.close.len() { continue; }

            // === EXIT ===
            if let Some(mut pos) = positions.remove(sym) {
                if idx > pos.entry_bar {
                    if sd.high[idx] > pos.highest_high { pos.highest_high = sd.high[idx]; }
                    if sd.low[idx] < pos.lowest_low { pos.lowest_low = sd.low[idx]; }
                    pos.bars_held += 1;

                    let pc = sd.close[idx];
                    let tr = (sd.high[idx] - sd.low[idx])
                        .max((sd.high[idx] - pc).abs())
                        .max((sd.low[idx] - pc).abs());
                    pos.atr_buf.push_back(tr);
                    if pos.atr_buf.len() > TURTLE_ATR_PERIOD { pos.atr_buf.pop_front(); }

                    let mut exit = false;
                    if pos.bars_held >= HOLD_MAX {
                        exit = true;
                    } else if pos.atr_buf.len() == TURTLE_ATR_PERIOD {
                        let atr = pos.atr_buf.iter().sum::<f64>() / TURTLE_ATR_PERIOD as f64;
                        let stop = pos.highest_high - TURTLE_ATR_MULT * atr;
                        if sd.low[idx] <= stop { exit = true; }
                    }

                    if exit {
                        let exit_exec = sd.close[idx] * (1.0 - FEE);
                        let trade_ret = exit_exec / pos.entry_exec - 1.0;
                        realized_equity *= 1.0 + pos.size * trade_ret;
                        trades += 1;
                        if trade_ret > 0.0 { wins += 1; }
                        last_exit_bar.insert(sym.clone(), idx + 1);
                    } else {
                        positions.insert(sym.clone(), pos);
                    }
                } else {
                    positions.insert(sym.clone(), pos);
                }
            } else {
                // === ENTRY ===
                let current_positions = positions.len();
                if current_positions >= POSITION_CAP { continue; }

                if let Some(&last_exit) = last_exit_bar.get(sym) {
                    let bars_since_exit = (idx + 1).saturating_sub(last_exit);
                    if bars_since_exit < FRESHNESS_COOLDOWN { continue; }
                }

                // Regime gate
                let btc_pct = btc_atr_percentile(btc, REGIME_ATR_PERIOD, REGIME_LOOKBACK, idx);
                if btc_pct < ATR_RANK_THRESHOLD { continue; }
                if !live_entry_signal(sd, idx) { continue; }

                // Position size — apply hedge if active
                let mut size = 1.0 / POSITION_CAP as f64;
                if hedge_active(btc, idx, hedge_lb) {
                    size *= HEDGE_SIZE_MULT;
                }
                let entry_exec = sd.close[idx] * (1.0 + FEE);
                positions.insert(sym.clone(), PositionState {
                    entry_bar: idx,
                    entry_exec,
                    size,
                    highest_high: sd.high[idx],
                    lowest_low: sd.low[idx],
                    bars_held: 0,
                    atr_buf: seed_atr_buf(sd, idx),
                });
            }
        }

        // equity M2M
        let mut open_ret = 0.0;
        for (sym, pos) in &positions {
            if let Some(sd) = data.get(sym) {
                if idx < sd.close.len() {
                    let liq_exec = sd.close[idx] * (1.0 - FEE);
                    open_ret += pos.size * (liq_exec / pos.entry_exec - 1.0);
                }
            }
        }
        equity_curve.push(realized_equity * (1.0 + open_ret));
    }

    // Liquidate at end
    if test_end > test_start {
        let last_idx = test_end - 1;
        for (sym, pos) in positions {
            if let Some(sd) = data.get(&sym) {
                if last_idx < sd.close.len() {
                    let exit_exec = sd.close[last_idx] * (1.0 - FEE);
                    let trade_ret = exit_exec / pos.entry_exec - 1.0;
                    realized_equity *= 1.0 + pos.size * trade_ret;
                    trades += 1;
                    if trade_ret > 0.0 { wins += 1; }
                }
            }
        }
        if let Some(last) = equity_curve.last_mut() { *last = realized_equity; }
    }

    let sh = annualised_sharpe_from_equity(&equity_curve);
    let dd = max_dd(&equity_curve);
    (realized_equity, sh, dd, trades, wins, equity_curve)
}

const FRESHNESS_COOLDOWN: usize = 0;

#[tokio::main]
async fn main() -> Result<()> {
    println!("=== T84: HEDGE_LOOKBACK EXTENSIVE SWEEP ===");
    println!("Sweep LB ∈ {:?}", LB_VALUES);
    println!("Fixed: HEDGE_ATR_PERIOD={}, HEDGE_ATR_PCT={:.2}, HEDGE_SIZE_MULT={:.2}",
        HEDGE_ATR_PERIOD, HEDGE_ATR_PCT, HEDGE_SIZE_MULT);
    println!("Strategy: EP={}, ATR({},{}), HM={}, CAP={}, ATR_RANK(AP={},LB={},T={})",
        TURTLE_EP, TURTLE_ATR_PERIOD, TURTLE_ATR_MULT, HOLD_MAX, POSITION_CAP,
        REGIME_ATR_PERIOD, REGIME_LOOKBACK, ATR_RANK_THRESHOLD);

    let loader = DataLoader::new(None, None);
    let mut all_syms: HashSet<String> = HashSet::new();
    for (_, syms) in UNIVERSES {
        for &s in *syms { all_syms.insert(s.to_string()); }
    }

    let mut data: HashMap<String, SymData> = HashMap::new();
    for sym in &all_syms {
        let df = loader.fetch_with_cache(sym, "1d", CANDLES).await?;
        let close = df.column("close")?.f64()?.into_no_null_iter().collect();
        let high = df.column("high")?.f64()?.into_no_null_iter().collect();
        let low  = df.column("low")?.f64()?.into_no_null_iter().collect();
        data.insert(sym.clone(), SymData { close, high, low });
    }
    println!("Loaded {} symbols", data.len());

    // Write windows CSV header
    let mut wcsv = File::create(WINDOWS_OUT)?;
    writeln!(wcsv, "hedge_lb,universe,window,final_equity,return_pct,sharpe,max_dd_pct,trades,win_rate_pct,pass")?;

    #[derive(Default)]
    struct Agg { pass: usize, total: usize, trades: usize, wins: usize, sharpe_sum: f64, ret_sum: f64, dd_sum: f64 }
    let mut agg: HashMap<usize, Agg> = HashMap::new();

    for &hedge_lb in LB_VALUES {
        print!("LB={:4} ... ", hedge_lb);
        std::io::stdout().flush()?;

        for (u_name, u_syms) in UNIVERSES {
            let symbols: Vec<String> = u_syms.iter().map(|&s| s.to_string()).collect();
            let min_len = symbols.iter()
                .filter_map(|s| data.get(s).map(|d| d.close.len()))
                .min().unwrap_or(0);
            let windows = (min_len.saturating_sub(WARMUP_BARS)) / TEST_BARS;
            let mut wins = 0usize;
            let mut total_trades = 0usize;
            let mut total_sharpe = 0.0_f64;
            let mut total_ret = 0.0_f64;
            let mut total_dd = 0.0_f64;
            let mut any = false;

            for w in 0..windows {
                let start = WARMUP_BARS + w * TEST_BARS;
                let test_start = start + TRAIN_BARS;
                let test_end = (test_start + TEST_BARS).min(min_len);
                if test_end <= test_start + 10 { continue; }

                let (eq, sh, dd, tr, wn, _eq_curve) = run_sim(&data, &symbols, test_start, test_end, hedge_lb);
                let ret_pct = (eq - 1.0) * 100.0;
                let win_rate = if tr > 0 { wn as f64 / tr as f64 * 100.0 } else { 0.0 };
                let pass = tr >= MIN_TRADES && sh > 0.0 && eq > 1.0;
                writeln!(wcsv, "{},{},{},{:.8},{:.4},{:.6},{:.4},{},{:.4},{}",
                    hedge_lb, u_name, w, eq, ret_pct, sh, dd, tr, win_rate, pass)?;

                wins += wn;
                total_trades += tr;
                total_sharpe += sh;
                total_ret += ret_pct;
                total_dd += dd;
                any = true;
            }

            if any {
                let entries = windows;
                agg.entry(hedge_lb).or_default();
                let a = agg.get_mut(&hedge_lb).unwrap();
                a.total += 1;
                if total_trades >= MIN_TRADES && total_sharpe > 0.0 && total_ret > -50.0 {
                    a.pass += 1;
                }
                a.trades += total_trades;
                a.wins += wins;
                a.sharpe_sum += total_sharpe / entries as f64;
                a.ret_sum += total_ret / entries as f64;
                a.dd_sum += total_dd / entries as f64;
            }
        }

        let a = agg.get(&hedge_lb).unwrap();
        let avg_s = if a.total > 0 { a.sharpe_sum / a.total as f64 } else { 0.0 };
        let avg_r = if a.total > 0 { a.ret_sum / a.total as f64 } else { 0.0 };
        let avg_d = if a.total > 0 { a.dd_sum / a.total as f64 } else { 0.0 };
        let avg_wr = if a.trades > 0 { a.wins as f64 / a.trades as f64 * 100.0 } else { 0.0 };
        println!("pass={:2}/{}  sh={:7.3}  ret={:+7.1}%  dd={:5.1}%  tr={}  wr={:.0}%",
            a.pass, a.total, avg_s, avg_r, avg_d, a.trades, avg_wr);
    }

    // Write summary
    let mut scsv = File::create(SUMMARY_OUT)?;
    writeln!(scsv, "hedge_lb,universes_passed,total_universes,pass_pct,avg_sharpe,avg_return_pct,avg_dd_pct,total_trades,win_rate_pct")?;
    let mut sorted: Vec<_> = agg.iter().collect();
    sorted.sort_by_key(|(&lb, _)| lb);
    for (&hedge_lb, a) in sorted {
        let pass_pct = if a.total > 0 { a.pass as f64 / a.total as f64 * 100.0 } else { 0.0 };
        let avg_s = if a.total > 0 { a.sharpe_sum / a.total as f64 } else { 0.0 };
        let avg_r = if a.total > 0 { a.ret_sum / a.total as f64 } else { 0.0 };
        let avg_d = if a.total > 0 { a.dd_sum / a.total as f64 } else { 0.0 };
        let wr = if a.trades > 0 { a.wins as f64 / a.trades as f64 * 100.0 } else { 0.0 };
        writeln!(scsv, "{},{},{},{:.2},{:.6},{:.4},{:.4},{},{:.4}",
            hedge_lb, a.pass, a.total, pass_pct, avg_s, avg_r, avg_d, a.trades, wr)?;
    }
    scsv.flush()?;
    println!("\nWritten: {} {}", SUMMARY_OUT, WINDOWS_OUT);

    // Find winner (pass-rate first, then Sharpe)
    let mut best @ (_, best_pass, best_sharpe) = (0usize, 0usize, -999.9_f64);
    for (&lb, a) in &agg {
        if a.pass > best_pass || (a.pass == best_pass && a.sharpe_sum > best_sharpe) {
            best = (lb, a.pass, a.sharpe_sum);
        }
    }
    println!("\n=== WINNER: HEDGE_LOOKBACK={} ({} universes pass, Sharpe={:.3}) ===", best.0, best.1, best.2);

    Ok(())
}
