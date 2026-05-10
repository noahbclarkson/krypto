//! T88: HOLD_MAX extensive hyperopt under exact-live Turtle-only config.
//!
//! CURRENT live config (src/live/config.rs, settled 2026-05-07):
//!   Turtle-only, AP=17, LB=41, T=5.0, EP=21, ATR_ENTRY_MULT=0.00
//!
//! Prior T81: HM=15 won under exact-live path (46/60 pass, Sharpe 1.439)
//! BUT T81 step=5 grid skipped HM=15 in favor of HM=12 baseline.
//! This harness fills the gap: HM = 1..=100 step 1 (100 values).
//!
//! Output:
//!   snapshots/t88_hm_summary.csv
//!   snapshots/t88_hm_windows.csv
//!   snapshots/t88_hm_equity.csv  (Base5 equity time-series per HM)

use anyhow::Result;
use krypto::data::loader::DataLoader;
use krypto::live::config::{
    ATR_ENTRY_MULT, ATR_RANK_THRESHOLD, HEDGE_ATR_PCT, HEDGE_ATR_PERIOD,
    HEDGE_LOOKBACK, HEDGE_SIZE_MULT, POSITION_CAP,
    REGIME_ATR_PERIOD, REGIME_LOOKBACK, TURTLE_ATR_MULT, TURTLE_ATR_PERIOD,
    TURTLE_EP,
};
use std::collections::{HashMap, VecDeque};
use std::fs::File;
use std::io::Write;

const CANDLES: u32 = 3000;
const TRAIN_BARS: usize = 252;
const TEST_BARS: usize = 252;
const WARMUP_BARS: usize = 300;
const MIN_TRADES: usize = 3;
const FEE: f64 = 0.0004;
const FRESHNESS_COOLDOWN: usize = 0;

const HM_MIN: usize = 1;
const HM_MAX: usize = 100;
const BASELINE_HM: usize = 12;

const SUMMARY_OUT: &str = "/home/ubuntu/.openclaw/workspace-krypto/krypto/snapshots/t88_hm_summary.csv";
const WINDOWS_OUT: &str = "/home/ubuntu/.openclaw/workspace-krypto/krypto/snapshots/t88_hm_windows.csv";
const EQUITY_OUT: &str = "/home/ubuntu/.openclaw/workspace-krypto/krypto/snapshots/t88_hm_equity.csv";

const ALL_SYMBOLS: &[&str] = &[
    "BTCUSDT", "ETHUSDT", "SOLUSDT", "XRPUSDT", "DOGEUSDT", "ADAUSDT",
    "BNBUSDT", "LTCUSDT", "EOSUSDT", "BCHUSDT", "NEOUSDT", "QTUMUSDT",
];

const UNIVERSE_SYMBOLS: &[&[&str]] = &[
    &["BTCUSDT", "ETHUSDT", "SOLUSDT", "XRPUSDT", "DOGEUSDT", "ADAUSDT"],
    &["BTCUSDT", "ETHUSDT", "BNBUSDT", "XRPUSDT", "ADAUSDT"],
    &["BTCUSDT", "ETHUSDT", "SOLUSDT", "XRPUSDT", "ADAUSDT"],
    &["LTCUSDT", "EOSUSDT", "BCHUSDT", "NEOUSDT", "QTUMUSDT"],
    &["BTCUSDT", "XRPUSDT", "LTCUSDT", "EOSUSDT"],
    &["BTCUSDT", "XRPUSDT", "LTCUSDT", "ETHUSDT"],
    &["BTCUSDT", "XRPUSDT", "LTCUSDT"],
    &["BTCUSDT", "ETHUSDT", "SOLUSDT", "XRPUSDT", "DOGEUSDT"],
    &["BTCUSDT", "ETHUSDT", "LTCUSDT"],
];

const UNIVERSE_NAMES: &[&str] = &[
    "Base5", "LargeCaps5", "NoDOGE", "LowVolume5", "Legacy4",
    "OldGuard4", "Legacy3", "HighVolume5", "OldGuard3",
];

struct SymData {
    close: Vec<f64>,
    high: Vec<f64>,
    low: Vec<f64>,
    dates: Vec<String>,
}

#[derive(Default, Clone)]
struct AggStats {
    pass: usize,
    total: usize,
    sharpe: f64,
    ret: f64,
    dd: f64,
    trades: usize,
    wins: f64,
}

struct SimResult {
    final_equity: f64,
    sharpe: f64,
    max_dd_pct: f64,
    trades: usize,
    win_rate_pct: f64,
    return_pct: f64,
    equity_curve: Vec<f64>,
}

fn tr_at(sd: &SymData, idx: usize) -> f64 {
    let pc = if idx == 0 { sd.close[idx] } else { sd.close[idx - 1] };
    (sd.high[idx] - sd.low[idx])
        .max((sd.high[idx] - pc).abs())
        .max((sd.low[idx] - pc).abs())
}

fn atr_at(sd: &SymData, period: usize, idx: usize) -> f64 {
    if period == 0 || idx < period || idx >= sd.close.len() {
        return 0.0;
    }
    let start = idx + 1 - period;
    let mut sum = 0.0_f64;
    for i in start..=idx {
        let pc = if i == 0 { sd.close[i] } else { sd.close[i - 1] };
        sum += (sd.high[i] - sd.low[i])
            .max((sd.high[i] - pc).abs())
            .max((sd.low[i] - pc).abs());
    }
    sum / period as f64
}

fn btc_atr_percentile(btc: &SymData, atr_period: usize, lookback: usize, idx: usize) -> f64 {
    let len = btc.close.len();
    if len <= atr_period.max(lookback) + 1 {
        return 50.0;
    }
    let curr_atr = atr_at(btc, atr_period, idx);
    let curr_close = btc.close[idx];
    if curr_atr <= 0.0 || curr_close <= 0.0 {
        return 50.0;
    }
    let curr_pct = curr_atr / curr_close;
    let start = idx.saturating_sub(lookback);
    let mut below = 0usize;
    let mut total = 0usize;
    for i in start..idx {
        let close = btc.close[i];
        if close <= 0.0 {
            continue;
        }
        let hist_atr = atr_at(btc, atr_period, i);
        if hist_atr <= 0.0 {
            continue;
        }
        if hist_atr / close < curr_pct {
            below += 1;
        }
        total += 1;
    }
    if total == 0 {
        50.0
    } else {
        (below as f64 / total as f64) * 100.0
    }
}

fn hedge_active(btc: &SymData, _idx: usize) -> bool {
    let len = btc.close.len();
    if len < HEDGE_ATR_PERIOD + HEDGE_LOOKBACK {
        return false;
    }
    let n = len;
    let mut trs = Vec::with_capacity(HEDGE_ATR_PERIOD);
    for i in (n - HEDGE_ATR_PERIOD)..n {
        trs.push(tr_at(btc, i));
    }
    let hedge_atr = trs.iter().sum::<f64>() / HEDGE_ATR_PERIOD as f64;
    let mut hist: Vec<f64> = Vec::with_capacity(HEDGE_LOOKBACK);
    for j in 1..=HEDGE_LOOKBACK {
        let idx_j = n.saturating_sub(j);
        if idx_j == 0 {
            break;
        }
        hist.push(tr_at(btc, idx_j));
    }
    if hist.is_empty() {
        return false;
    }
    hist.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let pct_idx = (HEDGE_ATR_PCT * hist.len() as f64) as usize;
    hist.get(pct_idx).map(|&pct_threshold| hedge_atr > pct_threshold).unwrap_or(false)
}

fn live_entry_signal(sd: &SymData, idx: usize) -> bool {
    let len = idx + 1;
    if len < TURTLE_EP + 1 {
        return false;
    }
    let ws = len - TURTLE_EP;
    let max_close = sd.close[ws..=idx]
        .iter()
        .fold(f64::NEG_INFINITY, |a, &b| a.max(b));
    sd.close[idx] >= max_close
}

fn seed_atr_buf(sd: &SymData, idx: usize) -> VecDeque<f64> {
    let len = idx + 1;
    let avail = len.min(TURTLE_ATR_PERIOD);
    let start = len.saturating_sub(avail);
    let mut atr_buf = VecDeque::with_capacity(TURTLE_ATR_PERIOD);
    for offset in 0..avail {
        let b_idx = start + offset;
        let pc = if offset == 0 {
            sd.close[b_idx]
        } else {
            sd.close[start + offset - 1]
        };
        let tr = (sd.high[b_idx] - sd.low[b_idx])
            .max((sd.high[b_idx] - pc).abs())
            .max((sd.low[b_idx] - pc).abs());
        atr_buf.push_back(tr);
    }
    atr_buf
}

fn annualised_sharpe_from_equity(equity: &[f64]) -> f64 {
    if equity.len() < 10 {
        return 0.0;
    }
    let mut daily_rets: Vec<f64> = Vec::with_capacity(equity.len() - 1);
    for i in 1..equity.len() {
        if equity[i - 1] > 0.0 && equity[i] > 0.0 {
            daily_rets.push((equity[i] - equity[i - 1]) / equity[i - 1]);
        }
    }
    if daily_rets.is_empty() {
        return 0.0;
    }
    let mean = daily_rets.iter().sum::<f64>() / daily_rets.len() as f64;
    let var: f64 = daily_rets.iter().map(|r| (r - mean).powi(2)).sum::<f64>() / daily_rets.len() as f64;
    let std = var.sqrt();
    if std == 0.0 {
        return 0.0;
    }
    mean / std * 252.0_f64.sqrt()
}

fn max_dd(equity: &[f64]) -> f64 {
    let mut peak = f64::NEG_INFINITY;
    let mut max_dd = 0.0_f64;
    for &eq in equity {
        if eq > peak {
            peak = eq;
        }
        let dd = (peak - eq) / peak;
        if dd > max_dd {
            max_dd = dd;
        }
    }
    max_dd * 100.0
}

fn mark_to_market_equity(
    realized: f64,
    positions: &HashMap<String, PositionState>,
    data: &HashMap<String, SymData>,
    _fee: f64,
) -> f64 {
    let mut mm_equity = realized;
    for (sym, pos) in positions {
        if let Some(sd) = data.get(sym) {
            let cur_close = sd.close.last().copied().unwrap_or(pos.entry_price);
            let pnl = (cur_close - pos.entry_price) / pos.entry_price * pos.size;
            mm_equity += pnl;
        }
    }
    mm_equity
}

struct PositionState {
    entry_price: f64,
    size: f64,
    highest_high: f64,
    lowest_low: f64,
    bars_held: usize,
    atr_buf: VecDeque<f64>,
}

fn run_sim(
    data: &HashMap<String, SymData>,
    symbols: &[String],
    test_start: usize,
    test_end: usize,
    hold_max: usize,
) -> SimResult {
    let mut positions: HashMap<String, PositionState> = HashMap::new();
    let mut realized_equity = 1.0_f64;
    let mut equity_curve: Vec<f64> = Vec::with_capacity(test_end - test_start);
    let mut trades = 0usize;
    let mut wins = 0usize;
    let mut last_exit_bar: HashMap<String, usize> = HashMap::new();

    for idx in test_start..test_end {
        // Mark-to-market equity at open of bar
        let mm_eq = mark_to_market_equity(realized_equity, &positions, data, FEE);
        equity_curve.push(mm_eq);

        // Close/expire positions first
        let mut to_close: Vec<String> = Vec::new();
        for (sym, pos) in &positions {
            let sd = match data.get(sym) {
                Some(s) => s,
                None => continue,
            };
            // HOLD_MAX hard timeout
            if pos.bars_held >= hold_max {
                to_close.push(sym.clone());
                continue;
            }
            // Turtle ATR trailing stop
            if pos.atr_buf.len() >= TURTLE_ATR_PERIOD {
                let atr = pos.atr_buf.iter().sum::<f64>() / TURTLE_ATR_PERIOD as f64;
                let stop = pos.highest_high - TURTLE_ATR_MULT * atr;
                if sd.low[idx] <= stop {
                    to_close.push(sym.clone());
                    continue;
                }
            }
        }

        for sym in &to_close {
            if let Some(pos) = positions.remove(sym) {
                let sd = data.get(sym).unwrap();
                let exit_price = sd.close[idx];
                let ret = (exit_price - pos.entry_price) / pos.entry_price;
                realized_equity += ret * pos.size;
                realized_equity -= FEE * 2.0 * pos.size;
                trades += 1;
                if ret > 0.0 {
                    wins += 1;
                }
                last_exit_bar.insert(sym.clone(), idx);
            }
        }

        // Open new positions
        let active = positions.len();
        if active < POSITION_CAP {
            for sym in symbols {
                if positions.contains_key(sym) {
                    continue;
                }
                if active >= POSITION_CAP {
                    break;
                }
                let sd = match data.get(sym) {
                    Some(s) => s,
                    None => continue,
                };

                // Freshness
                if let Some(&last_exit) = last_exit_bar.get(sym) {
                    if idx - last_exit < FRESHNESS_COOLDOWN {
                        continue;
                    }
                }

                // Entry signal (current-inclusive EP window)
                if !live_entry_signal(sd, idx) {
                    continue;
                }

                // ATR rank gate
                if let Some(btc) = data.get("BTCUSDT") {
                    let pct = btc_atr_percentile(btc, REGIME_ATR_PERIOD, REGIME_LOOKBACK, idx);
                    if pct < ATR_RANK_THRESHOLD {
                        continue;
                    }
                }

                // Size: 1/CAP with USDT hedge
                let mut size = 1.0 / POSITION_CAP as f64;
                if let Some(btc) = data.get("BTCUSDT") {
                    if hedge_active(btc, idx) {
                        size *= HEDGE_SIZE_MULT;
                    }
                }

                let atr_buf = seed_atr_buf(sd, idx);
                positions.insert(sym.clone(), PositionState {
                    entry_price: sd.close[idx],
                    size,
                    highest_high: sd.high[idx],
                    lowest_low: sd.low[idx],
                    bars_held: 0,
                    atr_buf,
                });
            }
        }

        // Update position state for next bar
        let mut updated: HashMap<String, PositionState> = HashMap::new();
        for (sym, pos) in positions.drain() {
            let sd = data.get(&sym).unwrap();
            let tr = (sd.high[idx] - sd.low[idx])
                .max((sd.high[idx] - sd.close[idx.saturating_sub(1)]).abs())
                .max((sd.low[idx] - sd.close[idx.saturating_sub(1)]).abs());
            let mut new_buf = pos.atr_buf.clone();
            new_buf.push_back(tr);
            if new_buf.len() > TURTLE_ATR_PERIOD {
                new_buf.pop_front();
            }
            updated.insert(sym, PositionState {
                entry_price: pos.entry_price,
                size: pos.size,
                highest_high: sd.high[idx].max(pos.highest_high),
                lowest_low: sd.low[idx].min(pos.lowest_low),
                bars_held: pos.bars_held + 1,
                atr_buf: new_buf,
            });
        }
        positions = updated;
    }

    // Flush: close all at final bar
    let final_idx = test_end - 1;
    for (_sym, pos) in positions.drain() {
        let sd = data.get(&_sym).unwrap();
        let exit_price = sd.close[final_idx];
        let ret = (exit_price - pos.entry_price) / pos.entry_price;
        realized_equity += ret * pos.size;
        realized_equity -= FEE * 2.0 * pos.size;
        trades += 1;
        if ret > 0.0 {
            wins += 1;
        }
    }

    let final_eq = mark_to_market_equity(realized_equity, &positions, data, FEE);
    equity_curve.push(final_eq);

    let ret_pct = (final_eq - 1.0) * 100.0;
    let sharpe = annualised_sharpe_from_equity(&equity_curve);
    let max_dd = max_dd(&equity_curve);
    let win_rate = if trades > 0 {
        wins as f64 / trades as f64 * 100.0
    } else {
        0.0
    };

    SimResult {
        final_equity: final_eq,
        sharpe,
        max_dd_pct: max_dd,
        trades,
        win_rate_pct: win_rate,
        return_pct: ret_pct,
        equity_curve,
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    println!("T88: HOLD_MAX extensive sweep (1..=100 step 1) under exact-live Turtle-only config");
    println!("Baseline HM={}, Winner will be robustness-first", BASELINE_HM);
    println!("Output: snapshots/t88_hm_{{summary,windows,equity}}.csv\n");

    let loader = DataLoader::new(None, None);
    let mut data: HashMap<String, SymData> = HashMap::new();

    for sym in ALL_SYMBOLS {
        match loader.fetch_with_cache(sym, "1d", CANDLES).await {
            Ok(df) => {
                let close = df.column("close")?.f64()?.into_no_null_iter().collect::<Vec<_>>();
                let high = df.column("high")?.f64()?.into_no_null_iter().collect::<Vec<_>>();
                let low = df.column("low")?.f64()?.into_no_null_iter().collect::<Vec<_>>();
                let nBars = close.len();
                let dates: Vec<String> = (0..nBars)
                    .map(|i| {
                        df.column("time")
                            .ok()
                            .and_then(|s| s.get(i).ok())
                            .map(|v| v.to_string())
                            .unwrap_or_default()
                    })
                    .collect();
                data.insert(sym.to_string(), SymData { close, high, low, dates });
                println!("  {}: {} bars", sym, nBars);
            }
            Err(e) => {
                eprintln!("  WARNING: failed to load {}: {}", sym, e);
            }
        }
    }

    let mut agg: HashMap<usize, AggStats> = HashMap::new();
    let mut window_csv = File::create(WINDOWS_OUT)?;
    writeln!(window_csv, "hold_max,universe,window,return_pct,sharpe,max_dd_pct,trades,win_rate_pct,pass")?;

    for (ui, universe) in UNIVERSE_NAMES.iter().enumerate() {
        let syms: Vec<String> = UNIVERSE_SYMBOLS[ui]
            .iter()
            .map(|s| (*s).to_string())
            .collect();

        let min_len = syms
            .iter()
            .filter_map(|s| data.get(s).map(|sd| sd.close.len()))
            .min()
            .unwrap_or(0);

        if min_len < WARMUP_BARS + TEST_BARS + 10 {
            eprintln!("  Skipping {}: insufficient data ({})", universe, min_len);
            continue;
        }

        let total_windows = (min_len - WARMUP_BARS) / TEST_BARS;
        let actual_windows = total_windows.min(8);

        for w in 0..actual_windows {
            let test_start = WARMUP_BARS + w * TEST_BARS;
            let test_end = (test_start + TEST_BARS).min(min_len - 1);

            for hm in HM_MIN..=HM_MAX {
                let res = run_sim(&data, &syms, test_start, test_end, hm);
                let pass = if res.sharpe > 0.0 && res.trades >= MIN_TRADES {
                    "true"
                } else {
                    "false"
                };
                writeln!(
                    window_csv,
                    "{},{},{},{:4},{:6},{:4},{},{:4},{}",
                    hm, universe, w, res.return_pct, res.sharpe, res.max_dd_pct,
                    res.trades, res.win_rate_pct, pass
                )?;

                let a = agg.entry(hm).or_default();
                a.total += 1;
                a.sharpe += res.sharpe;
                a.ret += res.return_pct;
                a.dd += res.max_dd_pct;
                a.trades += res.trades;
                a.wins += res.win_rate_pct * res.trades as f64 / 100.0;
                if res.sharpe > 0.0 && res.trades >= MIN_TRADES {
                    a.pass += 1;
                }
            }
        }
    }

    drop(window_csv);

    // Summary
    let mut summary_csv = File::create(SUMMARY_OUT)?;
    writeln!(summary_csv, "hold_max,global_pass,global_total,pass_rate_pct,avg_sharpe,avg_return_pct,avg_max_dd_pct,total_trades,avg_win_rate_pct")?;

    let mut rows: Vec<(usize, usize, usize, f64, f64, f64, f64, usize)> = Vec::new();

    for hm in HM_MIN..=HM_MAX {
        let a = agg.get(&hm).cloned().unwrap_or_default();
        let pass_rate = if a.total > 0 {
            a.pass as f64 / a.total as f64 * 100.0
        } else {
            0.0
        };
        let avg_sharpe = if a.total > 0 { a.sharpe / a.total as f64 } else { 0.0 };
        let avg_ret = if a.total > 0 { a.ret / a.total as f64 } else { 0.0 };
        let avg_dd = if a.total > 0 { a.dd / a.total as f64 } else { 0.0 };
        let win_rate = if a.trades > 0 {
            a.wins / a.trades as f64 * 100.0
        } else {
            0.0
        };

        writeln!(
            summary_csv,
            "{},{},{},{:4},{:6},{:4},{:4},{},{:4}",
            hm, a.pass, a.total, pass_rate, avg_sharpe, avg_ret, avg_dd, a.trades, win_rate
        )?;
        rows.push((hm, a.pass, a.total, pass_rate, avg_sharpe, avg_ret, avg_dd, a.trades));
    }

    rows.sort_by(|a, b| {
        let pass_cmp = b.1.cmp(&a.1).then_with(|| b.4.partial_cmp(&a.4).unwrap());
        pass_cmp
    });

    println!("\n=== TOP 15 by pass rate + Sharpe ===");
    for (hm, pass, total, pr, sh, ret, dd, trades) in rows.iter().take(15) {
        // Use integer arithmetic to avoid libm floating-point formatting
        let pr_i = (pr * 10.0).round() as i32;
        let pr_str = format!("{}.{}", pr_i / 10, pr_i.abs() % 10);
        let sh_i = (sh * 10000.0).round() as i32;
        let sh_str = if sh_i < 0 {
            format!("-{}.{:04}", (-sh_i) / 10000, (-sh_i) % 10000)
        } else {
            format!("{}.{:04}", sh_i / 10000, sh_i % 10000)
        };
        let ret_i = (ret.abs() * 10.0).round() as i32;
        let ret_sign = if *ret < 0.0 { '-' } else { '+' }; let ret_str = format!("{}{}.{}", ret_sign, ret_i / 10, ret_i % 10);
        let dd_i = (dd * 10.0).round() as i32;
        let dd_str = format!("{}.{}", dd_i / 10, dd_i % 10);
        println!(
            "HM={:3}: {:3}/{:3} ({}) | Sharpe {} | Ret {}% | DD {}% | {} trades",
            hm, pass, total, pr_str, sh_str, ret_str, dd_str, trades
        );
    }

    let winner = rows.first().map(|(hm, ..)| *hm).unwrap_or(BASELINE_HM);
    let baseline_pass = rows.iter().find(|(hm, ..)| *hm == BASELINE_HM).map(|r| r.1).unwrap_or(0);
    let baseline_total = rows.iter().find(|(hm, ..)| *hm == BASELINE_HM).map(|r| r.2).unwrap_or(1);
    println!(
        "\nWINNER: HM={} (pass {:3}/{:3} = {:1}%) | Baseline HM={} (pass {:3}/{:3} = {:1}%)",
        winner,
        rows.first().map(|r| r.1).unwrap_or(0),
        rows.first().map(|r| r.2).unwrap_or(0),
        rows.first().map(|r| r.3).unwrap_or(0.0),
        BASELINE_HM,
        baseline_pass,
        baseline_total,
        baseline_pass as f64 / baseline_total.max(1) as f64 * 100.0
    );

    // Equity time-series for Base5
    let base5_syms: Vec<String> = vec![
        "BTCUSDT".to_string(),
        "ETHUSDT".to_string(),
        "SOLUSDT".to_string(),
        "XRPUSDT".to_string(),
        "DOGEUSDT".to_string(),
        "ADAUSDT".to_string(),
    ];
    let base5_min = base5_syms
        .iter()
        .filter_map(|s| data.get(s).map(|sd| sd.close.len()))
        .min()
        .unwrap_or(0);
    let base5_full_start = WARMUP_BARS;
    let base5_full_end = base5_min.saturating_sub(1);

    let mut eq_csv = File::create(EQUITY_OUT)?;
    writeln!(eq_csv, "hold_max,step,equity")?;

    for hm in HM_MIN..=HM_MAX {
        let res = run_sim(&data, &base5_syms, base5_full_start, base5_full_end, hm);
        for (step, &eq) in res.equity_curve.iter().enumerate() {
            writeln!(eq_csv, "{},{},{:10}", hm, step, eq)?;
        }
    }

    drop(eq_csv);

    println!("\nOutput: {}", SUMMARY_OUT);
    println!("Output: {}", WINDOWS_OUT);
    println!("Output: {}", EQUITY_OUT);

    Ok(())
}
