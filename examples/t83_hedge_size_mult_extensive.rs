//! T83: HEDGE_SIZE_MULT extensive hyperopt under CURRENT exact-live semantics.
//!
//! Audit finding: HEDGE_SIZE_MULT=0.55 is production-live and material, but older
//! sweep docs conflict (T66 selected 0.40 on a different harness; bot.rs comment
//! still referenced 0.40). This run isolates the live-path hedge size dial after
//! T81 HOLD_MAX=15.
//!
//! CURRENT live config (src/live/config.rs):
//!   Turtle-only, AP=17, LB=41, T=5.0, EP=21, ATR_ENTRY_MULT=0.00, HM=15,
//!   HEDGE_ATR_PERIOD=38, HEDGE_LOOKBACK=252, HEDGE_ATR_PCT=0.45.
//!
//! Sweep: HEDGE_SIZE_MULT = 0.10..=1.00 step 0.05 (19 values) × 9 universes × WF windows.
//! Robustness ranking: pass rate first, daily equity Sharpe second; return is not the sole selector.
//!
//! Output:
//!   snapshots/t83_hedge_size_mult_summary.csv
//!   snapshots/t83_hedge_size_mult_windows.csv
//!   snapshots/t83_hedge_size_mult_equity.csv  (Base5 per-HSM equity time-series)

use anyhow::Result;
use krypto::data::loader::DataLoader;
use krypto::live::config::{
    ATR_ENTRY_MULT, ATR_RANK_THRESHOLD, HEDGE_ATR_PCT, HEDGE_ATR_PERIOD, HEDGE_LOOKBACK,
    HEDGE_SIZE_MULT as CURRENT_HSM, HOLD_MAX, POSITION_CAP, REGIME_ATR_PERIOD, REGIME_LOOKBACK,
    TURTLE_ATR_MULT, TURTLE_ATR_PERIOD, TURTLE_EP,
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

// Extensive HOLD_MAX sweep: 5..=100 step 5 (20 values)
const HSM_MIN_BPS: usize = 10;
const HSM_MAX_BPS: usize = 100;
const HSM_STEP_BPS: usize = 5;

const SUMMARY_OUT: &str =
    "/home/ubuntu/.openclaw/workspace-krypto/krypto/snapshots/t83_hedge_size_mult_summary.csv";
const WINDOWS_OUT: &str =
    "/home/ubuntu/.openclaw/workspace-krypto/krypto/snapshots/t83_hedge_size_mult_windows.csv";
const EQUITY_OUT: &str =
    "/home/ubuntu/.openclaw/workspace-krypto/krypto/snapshots/t83_hedge_size_mult_equity.csv";

const UNIVERSES: &[(&str, &[&str])] = &[
    (
        "Base5",
        &[
            "BTCUSDT", "ETHUSDT", "SOLUSDT", "XRPUSDT", "DOGEUSDT", "ADAUSDT",
        ],
    ),
    (
        "NoDOGE",
        &["BTCUSDT", "ETHUSDT", "SOLUSDT", "XRPUSDT", "ADAUSDT"],
    ),
    (
        "Legacy4",
        &["BTCUSDT", "ETHUSDT", "XRPUSDT", "LTCUSDT", "EOSUSDT"],
    ),
    (
        "Legacy5BNB",
        &[
            "BTCUSDT", "ETHUSDT", "XRPUSDT", "LTCUSDT", "BNBUSDT", "EOSUSDT",
        ],
    ),
    (
        "OldGuardNoBNB",
        &[
            "BTCUSDT", "ETHUSDT", "XRPUSDT", "LTCUSDT", "EOSUSDT", "BCHUSDT",
        ],
    ),
    (
        "LargeCaps5",
        &[
            "BTCUSDT", "ETHUSDT", "SOLUSDT", "XRPUSDT", "BNBUSDT", "ADAUSDT",
        ],
    ),
    ("Legacy3", &["BTCUSDT", "XRPUSDT", "LTCUSDT", "EOSUSDT"]),
    (
        "LowVolume5",
        &["XRPUSDT", "LTCUSDT", "EOSUSDT", "BCHUSDT", "ADAUSDT"],
    ),
    (
        "OldGuard4",
        &["BTCUSDT", "XRPUSDT", "LTCUSDT", "EOSUSDT", "BCHUSDT"],
    ),
];

#[derive(Clone)]
struct SymData {
    close: Vec<f64>,
    high: Vec<f64>,
    low: Vec<f64>,
}

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

#[derive(Clone)]
struct SimResult {
    final_equity: f64,
    sharpe: f64,
    max_dd_pct: f64,
    trades: usize,
    wins: usize,
    equity_curve: Vec<f64>,
}

fn tr_at(sd: &SymData, idx: usize) -> f64 {
    let pc = if idx == 0 {
        sd.close[idx]
    } else {
        sd.close[idx - 1]
    };
    (sd.high[idx] - sd.low[idx])
        .max((sd.high[idx] - pc).abs())
        .max((sd.low[idx] - pc).abs())
}

fn atr_at(sd: &SymData, period: usize, idx: usize) -> f64 {
    if period == 0 || idx < period || idx >= sd.close.len() {
        return 0.0;
    }
    let start = idx + 1 - period;
    let mut sum = 0.0;
    for i in start..=idx {
        sum += tr_at(sd, i);
    }
    sum / period as f64
}

/// Mirrors LiveBot::btc_atr_percentile exactly.
fn btc_atr_percentile(btc: &SymData, atr_period: usize, lookback: usize, idx: usize) -> f64 {
    let len = idx + 1;
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

fn hedge_active(btc: &SymData, idx: usize) -> bool {
    let n = idx + 1;
    if HEDGE_ATR_PERIOD == 0 || n < HEDGE_LOOKBACK + HEDGE_ATR_PERIOD {
        return false;
    }
    let mut trs = Vec::with_capacity(HEDGE_ATR_PERIOD);
    for i in (n - HEDGE_ATR_PERIOD)..n {
        trs.push(tr_at(btc, i));
    }
    let curr_atr = trs.iter().sum::<f64>() / HEDGE_ATR_PERIOD as f64;
    let mut hist = Vec::with_capacity(HEDGE_LOOKBACK);
    for j in 1..=HEDGE_LOOKBACK {
        let hist_idx = n.saturating_sub(j);
        if hist_idx == 0 {
            break;
        }
        hist.push(tr_at(btc, hist_idx));
    }
    hist.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let pct_idx = (HEDGE_ATR_PCT * hist.len() as f64) as usize;
    hist.get(pct_idx)
        .is_some_and(|&threshold| curr_atr > threshold)
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
    if sd.close[idx] < max_close {
        return false;
    }
    if ATR_ENTRY_MULT > 0.0 && len >= TURTLE_ATR_PERIOD + 1 {
        let atr = atr_at(sd, TURTLE_ATR_PERIOD, idx);
        if sd.close[idx] < max_close + atr * ATR_ENTRY_MULT {
            return false;
        }
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

fn mark_to_market_equity(
    realized_equity: f64,
    positions: &HashMap<String, PositionState>,
    data: &HashMap<String, SymData>,
    idx: usize,
) -> f64 {
    let mut open_ret = 0.0;
    for (sym, pos) in positions {
        if let Some(sd) = data.get(sym) {
            if idx < sd.close.len() {
                let liquidation_exec = sd.close[idx] * (1.0 - FEE);
                open_ret += pos.size * (liquidation_exec / pos.entry_exec - 1.0);
            }
        }
    }
    realized_equity * (1.0 + open_ret)
}

fn annualised_sharpe_from_equity(equity: &[f64]) -> f64 {
    if equity.len() < 2 {
        return 0.0;
    }
    let mut rets = Vec::with_capacity(equity.len() - 1);
    for w in equity.windows(2) {
        if w[0] > 0.0 {
            rets.push(w[1] / w[0] - 1.0);
        }
    }
    if rets.is_empty() {
        return 0.0;
    }
    let mean = rets.iter().sum::<f64>() / rets.len() as f64;
    let var = rets.iter().map(|x| (x - mean).powi(2)).sum::<f64>() / rets.len() as f64;
    if var <= 0.0 {
        0.0
    } else {
        (mean / var.sqrt()) * 365.0_f64.sqrt()
    }
}

fn max_dd(equity: &[f64]) -> f64 {
    let mut peak = 1.0;
    let mut max_dd = 0.0;
    for &eq in equity {
        if eq > peak {
            peak = eq;
        }
        if peak > 0.0 {
            let dd = 1.0 - eq / peak;
            if dd > max_dd {
                max_dd = dd;
            }
        }
    }
    max_dd * 100.0
}

fn run_sim(
    data: &HashMap<String, SymData>,
    symbols: &[String],
    test_start: usize,
    test_end: usize,
    hedge_size_mult: f64,
) -> SimResult {
    let btc = match data.get("BTCUSDT") {
        Some(b) => b,
        None => data.get(&symbols[0]).unwrap(),
    };
    let mut realized_equity = 1.0_f64;
    let mut equity_curve: Vec<f64> = Vec::new();
    let mut positions: HashMap<String, PositionState> = HashMap::new();
    let mut last_exit_bar: HashMap<String, usize> = HashMap::new();
    let mut trades = 0usize;
    let mut wins = 0usize;

    for idx in test_start..test_end {
        for sym in symbols {
            let sd = match data.get(sym) {
                Some(sd) => sd,
                None => continue,
            };
            if idx >= sd.close.len() {
                continue;
            }

            if let Some(mut pos) = positions.remove(sym) {
                if idx > pos.entry_bar {
                    if sd.high[idx] > pos.highest_high {
                        pos.highest_high = sd.high[idx];
                    }
                    if sd.low[idx] < pos.lowest_low {
                        pos.lowest_low = sd.low[idx];
                    }
                    pos.bars_held += 1;

                    // bot.rs quirk: prev close = current close for TR
                    let pc = sd.close[idx];
                    let tr = (sd.high[idx] - sd.low[idx])
                        .max((sd.high[idx] - pc).abs())
                        .max((sd.low[idx] - pc).abs());
                    pos.atr_buf.push_back(tr);
                    if pos.atr_buf.len() > TURTLE_ATR_PERIOD {
                        pos.atr_buf.pop_front();
                    }

                    // HOLD_MAX always enforced — Turt le ATR conditional on warmup
                    let mut exit = false;
                    if pos.bars_held >= HOLD_MAX {
                        exit = true;
                    } else if pos.atr_buf.len() == TURTLE_ATR_PERIOD {
                        let atr = pos.atr_buf.iter().sum::<f64>() / TURTLE_ATR_PERIOD as f64;
                        let stop = pos.highest_high - TURTLE_ATR_MULT * atr;
                        if sd.low[idx] <= stop {
                            exit = true;
                        }
                    }

                    if exit {
                        let exit_exec = sd.close[idx] * (1.0 - FEE);
                        let trade_ret = exit_exec / pos.entry_exec - 1.0;
                        realized_equity *= 1.0 + pos.size * trade_ret;
                        trades += 1;
                        if trade_ret > 0.0 {
                            wins += 1;
                        }
                        last_exit_bar.insert(sym.clone(), idx + 1);
                    } else {
                        positions.insert(sym.clone(), pos);
                    }
                } else {
                    positions.insert(sym.clone(), pos);
                }
            } else {
                let current_positions = positions.len();
                if current_positions >= POSITION_CAP {
                    continue;
                }
                if let Some(&last_exit) = last_exit_bar.get(sym) {
                    let bars_since_exit = (idx + 1).saturating_sub(last_exit);
                    if bars_since_exit < FRESHNESS_COOLDOWN {
                        continue;
                    }
                }

                let btc_pct = btc_atr_percentile(btc, REGIME_ATR_PERIOD, REGIME_LOOKBACK, idx);
                if btc_pct < ATR_RANK_THRESHOLD {
                    continue;
                }
                if !live_entry_signal(sd, idx) {
                    continue;
                }

                let mut size = 1.0 / POSITION_CAP as f64;
                if hedge_active(btc, idx) {
                    size *= hedge_size_mult;
                }
                let entry_exec = sd.close[idx] * (1.0 + FEE);
                positions.insert(
                    sym.clone(),
                    PositionState {
                        entry_bar: idx,
                        entry_exec,
                        size,
                        highest_high: sd.high[idx],
                        lowest_low: sd.low[idx],
                        bars_held: 0,
                        atr_buf: seed_atr_buf(sd, idx),
                    },
                );
            }
        }

        equity_curve.push(mark_to_market_equity(
            realized_equity,
            &positions,
            data,
            idx,
        ));
    }

    // Liquidate at final bar
    if test_end > test_start {
        let last_idx = test_end - 1;
        for (sym, pos) in positions {
            if let Some(sd) = data.get(&sym) {
                if last_idx < sd.close.len() {
                    let exit_exec = sd.close[last_idx] * (1.0 - FEE);
                    let trade_ret = exit_exec / pos.entry_exec - 1.0;
                    realized_equity *= 1.0 + pos.size * trade_ret;
                    trades += 1;
                    if trade_ret > 0.0 {
                        wins += 1;
                    }
                }
            }
        }
        if let Some(last) = equity_curve.last_mut() {
            *last = realized_equity;
        }
    }

    SimResult {
        final_equity: realized_equity,
        sharpe: annualised_sharpe_from_equity(&equity_curve),
        max_dd_pct: max_dd(&equity_curve),
        trades,
        wins,
        equity_curve,
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    println!("=== T83: HEDGE_SIZE_MULT Extensive Hyperopt (Current Live Config) ===");
    println!(
        "Current config: EP={}, ATR({}, {:.2}), HSM sweep {:.2}-{:.2} step {:.2}, ATR_RANK(AP={}, LB={}, T={:.1})",
        TURTLE_EP,
        TURTLE_ATR_PERIOD,
        TURTLE_ATR_MULT,
        HSM_MIN_BPS as f64 / 100.0,
        HSM_MAX_BPS as f64 / 100.0,
        HSM_STEP_BPS as f64 / 100.0,
        REGIME_ATR_PERIOD,
        REGIME_LOOKBACK,
        ATR_RANK_THRESHOLD
    );
    println!(
        "HEDGE: P={}, LB={}, PCT={:.2}, current SM={:.2}",
        HEDGE_ATR_PERIOD, HEDGE_LOOKBACK, HEDGE_ATR_PCT, CURRENT_HSM
    );

    let loader = DataLoader::new(None, None);
    let mut all_syms = std::collections::HashSet::new();
    for (_, syms) in UNIVERSES {
        for &s in *syms {
            all_syms.insert(s.to_string());
        }
    }

    let mut data: HashMap<String, SymData> = HashMap::new();
    for sym in &all_syms {
        let df = loader.fetch_with_cache(sym, "1d", CANDLES).await?;
        let close = df
            .column("close")?
            .f64()?
            .into_no_null_iter()
            .collect::<Vec<_>>();
        let high = df
            .column("high")?
            .f64()?
            .into_no_null_iter()
            .collect::<Vec<_>>();
        let low = df
            .column("low")?
            .f64()?
            .into_no_null_iter()
            .collect::<Vec<_>>();
        data.insert(sym.clone(), SymData { close, high, low });
    }
    println!("Loaded {} symbols", data.len());

    let mut window_csv = File::create(WINDOWS_OUT)?;
    writeln!(window_csv, "hedge_size_mult,universe,window,final_equity,return_pct,sharpe,max_dd_pct,trades,win_rate_pct,pass")?;

    #[derive(Default, Clone)]
    struct Agg {
        pass: usize,
        total: usize,
        trades: usize,
        wins: usize,
        sharpe_sum: f64,
        ret_sum: f64,
        dd_sum: f64,
    }
    let mut agg: HashMap<usize, Agg> = HashMap::new();

    for hsm_bps in (HSM_MIN_BPS..=HSM_MAX_BPS).step_by(HSM_STEP_BPS) {
        let hedge_size_mult = hsm_bps as f64 / 100.0;
        for (u_name, u_syms) in UNIVERSES {
            let symbols: Vec<String> = u_syms.iter().map(|&s| s.to_string()).collect();
            let min_len = symbols
                .iter()
                .filter_map(|s| data.get(s).map(|d| d.close.len()))
                .min()
                .unwrap_or(0);
            let windows = (min_len.saturating_sub(TRAIN_BARS + WARMUP_BARS)) / TEST_BARS;
            for w in 0..windows {
                let start = WARMUP_BARS + w * TEST_BARS;
                let test_start = start + TRAIN_BARS;
                let test_end = (test_start + TEST_BARS).min(min_len);
                if test_end <= test_start + 10 {
                    continue;
                }
                let res = run_sim(&data, &symbols, test_start, test_end, hedge_size_mult);
                let ret_pct = (res.final_equity - 1.0) * 100.0;
                let win_rate = if res.trades > 0 {
                    res.wins as f64 / res.trades as f64 * 100.0
                } else {
                    0.0
                };
                let pass = res.trades >= MIN_TRADES && res.sharpe > 0.0 && res.final_equity > 1.0;
                writeln!(
                    window_csv,
                    "{},{},{},{:.8},{:.4},{:.6},{:.4},{},{:.4},{}",
                    hedge_size_mult,
                    u_name,
                    w,
                    res.final_equity,
                    ret_pct,
                    res.sharpe,
                    res.max_dd_pct,
                    res.trades,
                    win_rate,
                    pass
                )?;

                let a = agg.entry(hsm_bps).or_default();
                a.pass += pass as usize;
                a.total += 1;
                a.trades += res.trades;
                a.wins += res.wins;
                a.sharpe_sum += res.sharpe;
                a.ret_sum += ret_pct;
                a.dd_sum += res.max_dd_pct;
            }
        }
    }

    let _total_windows = UNIVERSES.len() * 7;

    let mut summary_csv = File::create(SUMMARY_OUT)?;
    writeln!(summary_csv, "hedge_size_mult,pass_count,total_windows,pass_rate_pct,avg_sharpe,avg_return_pct,avg_max_dd_pct,total_trades,win_rate_pct")?;
    let mut rows = Vec::new();
    for hsm_bps in (HSM_MIN_BPS..=HSM_MAX_BPS).step_by(HSM_STEP_BPS) {
        let hedge_size_mult = hsm_bps as f64 / 100.0;
        let a = agg.get(&hsm_bps).cloned().unwrap_or_default();
        let n = a.total.max(1) as f64;
        let pass_rate = a.pass as f64 / n * 100.0;
        let avg_sharpe = a.sharpe_sum / n;
        let avg_ret = a.ret_sum / n;
        let avg_dd = a.dd_sum / n;
        let win_rate = if a.trades > 0 {
            a.wins as f64 / a.trades as f64 * 100.0
        } else {
            0.0
        };
        writeln!(
            summary_csv,
            "{},{},{},{:.4},{:.6},{:.4},{:.4},{},{:.4}",
            hedge_size_mult,
            a.pass,
            a.total,
            pass_rate,
            avg_sharpe,
            avg_ret,
            avg_dd,
            a.trades,
            win_rate
        )?;
        rows.push((
            hsm_bps, a.pass, a.total, pass_rate, avg_sharpe, avg_ret, avg_dd, a.trades, win_rate,
        ));
    }

    rows.sort_by(|a, b| {
        b.3.partial_cmp(&a.3)
            .unwrap()
            .then_with(|| b.4.partial_cmp(&a.4).unwrap())
    });

    println!("\nTop HEDGE_SIZE_MULT values by robustness (pass rate, then Sharpe):");
    for (hsm_bps, pass, total, pr, sh, ret, dd, trades, _wr) in rows.iter().take(10) {
        println!(
            "HSM={:.2} | pass {:2}/{:2} ({:5.1}%) | Sharpe {:6.3} | Ret {:7.2}% | DD {:5.2}% | trades {:4}",
            *hsm_bps as f64 / 100.0, pass, total, pr, sh, ret, dd, trades
        );
    }

    // Export Base5 equity time-series for each HSM value
    let base5_syms: Vec<String> = UNIVERSES[0].1.iter().map(|&s| s.to_string()).collect();
    let base5_min_len = base5_syms
        .iter()
        .filter_map(|s| data.get(s).map(|d| d.close.len()))
        .min()
        .unwrap_or(0);
    let mut eq_csv = File::create(EQUITY_OUT)?;
    writeln!(eq_csv, "hedge_size_mult,step,equity")?;
    for hsm_bps in (HSM_MIN_BPS..=HSM_MAX_BPS).step_by(HSM_STEP_BPS) {
        let hedge_size_mult = hsm_bps as f64 / 100.0;
        let res = run_sim(
            &data,
            &base5_syms,
            WARMUP_BARS,
            base5_min_len,
            hedge_size_mult,
        );
        for (step, eq) in res.equity_curve.iter().enumerate() {
            writeln!(eq_csv, "{:.2},{},{:.8}", hedge_size_mult, step, eq)?;
        }
    }

    println!("\nWrote {}", SUMMARY_OUT);
    println!("Wrote {}", WINDOWS_OUT);
    println!("Wrote {}", EQUITY_OUT);

    // Best
    if let Some((best_bps, best_pass, best_total, best_pr, best_sh, _, _, _, _)) = rows.first() {
        println!(
            "\nWINNER: HSM={:.2} with {:.1}% pass ({}/{}), Sharpe {:.3}",
            *best_bps as f64 / 100.0,
            best_pr,
            best_pass,
            best_total,
            best_sh
        );
    }

    Ok(())
}
