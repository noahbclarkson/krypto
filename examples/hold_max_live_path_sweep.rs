//! HOLD_MAX Extensive Hyperopt — Live Turtle-Only Path
//!
//! Sweep HOLD_MAX ∈ [5..=100 step 5] (20 values) across 9 universes × 7 windows
//! using the ACTUAL live bot path (matches src/live/bot.rs):
//!   - Turtle breakout entry (EP=21)
//!   - ATR_RANK(AP=12, LB=42, T=24) regime gate
//!   - USDT 30% size overlay when BTC 21d ATR > 75th pct of 252d history
//!   - Turtle ATR trailing stop (AP=24, M=2.0)
//!   - HOLD_MAX enforced independently of ATR warmup
//!   - Fee: 0.10% taker each side
//!
//! PRIOR: HM=12 was found on CHAND(7,2.30) dual-exit harness (2026-04-21).
//! This tests HM under the ACTUAL live Turtle-only path with ATR_RANK filter.

use anyhow::Result;
use krypto::data::loader::DataLoader;
use std::collections::HashMap;
use std::fs::File;
use std::io::Write;

const CANDLES: u32 = 3000;
const TRAIN_BARS: usize = 252;
const TEST_BARS: usize = 252;
const TAKER_FEE: f64 = 0.001;
const POSITION_CAP: usize = 3;
const MIN_TRADES: usize = 3;

const TURTLE_ENTRY: usize = 21;
const TURTLE_ATR_PERIOD: usize = 24;
const TURTLE_ATR_MULT: f64 = 2.00;
const ATR_ENTRY_MULT: f64 = 0.00;
const VOL_LOOKBACK: usize = 8;

const REGIME_ATR_PERIOD: usize = 12;
const REGIME_LOOKBACK: usize = 42;
const ATR_RANK_T: f64 = 24.0;

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

const HM_VALUES: &[usize] = &[
    5, 10, 15, 20, 25, 30, 35, 40, 45, 50,
    55, 60, 65, 70, 75, 80, 85, 90, 95, 100,
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

fn turtle_signal(close: &[f64], high: &[f64], low: &[f64], entry_period: usize, atr_period: usize, atr_mult: f64, idx: usize) -> bool {
    if idx < entry_period { return false; }
    let start = idx - entry_period;
    let max_close = close[start..idx].iter().fold(f64::NEG_INFINITY, |a, &b| a.max(b));
    if close[idx] > max_close {
        if atr_mult > 0.0 {
            let atr = atr_at(high, low, close, atr_period, idx);
            if close[idx] < max_close + atr * atr_mult {
                return false;
            }
        }
        return true;
    }
    false
}

fn btc_atr_pct(btc_data: &SymData, period: usize, lookback: usize, idx: usize) -> f64 {
    if idx < period.max(lookback) { return 50.0; }
    let curr_atr = atr_at(&btc_data.high, &btc_data.low, &btc_data.close, period, idx);
    let mut hist = Vec::with_capacity(lookback);
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

#[derive(Default)]
struct WfResult {
    equity: f64,
    sharpe: f64,
    dd: f64,
    trades: usize,
}

fn run_sim(
    sym_data: &HashMap<String, SymData>,
    symbols: &[String],
    test_start: usize,
    test_end: usize,
    hold_max: usize,
) -> WfResult {
    let mut equity = 1.0_f64;
    let mut equity_curve = vec![1.0_f64];
    let mut total_trades = 0usize;
    let mut daily_rets = Vec::new();
    let mut bar = test_start;

    while bar + 2 < test_end {
        let btc = sym_data.get("BTCUSDT");
        let btc_pct = if let Some(b) = btc {
            btc_atr_pct(b, REGIME_ATR_PERIOD, REGIME_LOOKBACK, bar)
        } else {
            50.0
        };

        // Regime gate
        if btc_pct < ATR_RANK_T {
            equity_curve.push(equity);
            bar += 1;
            continue;
        }

        // DV ranking
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

        for sym in &top_syms {
            if let Some(sd) = sym_data.get(sym) {
                if bar >= TURTLE_ENTRY + 1 && bar < sd.close.len() {
                    if turtle_signal(&sd.close, &sd.high, &sd.low, TURTLE_ENTRY, TURTLE_ATR_PERIOD, ATR_ENTRY_MULT, bar) {
                        let entry_px = sd.close[bar];
                        let mut size_mult = 1.0;
                        if let Some(b) = btc {
                            if bar >= 252 + 21 {
                                let atr_21 = atr_at(&b.high, &b.low, &b.close, 21, bar);
                                let mut hist = Vec::new();
                                for j in 1..=252 {
                                    let idx = b.close.len().saturating_sub(j);
                                    if idx == 0 { break; }
                                    let bj_close = b.close[idx];
                                    let bj_high = b.high[idx];
                                    let bj_low = b.low[idx];
                                    let pcj = if idx == 0 { bj_close } else { b.close[idx - 1] };
                                    hist.push((bj_high - bj_low).max((bj_high - pcj).abs()).max((bj_low - pcj).abs()));
                                }
                                hist.sort_by(|a, b| a.partial_cmp(b).unwrap());
                                let pct_idx = (0.75 * hist.len() as f64) as usize;
                                if let Some(&pct_75) = hist.get(pct_idx) {
                                    if atr_21 > pct_75 {
                                        size_mult = 0.70;
                                    }
                                }
                            }
                        }

                        let size = size_mult / top_syms.len() as f64;
                        let entry = entry_px * (1.0 + TAKER_FEE);

                        let mut bars_held = 0usize;
                        let mut exited = false;
                        let mut highest_high = sd.high[bar];
                        let exit_bar_start = bar + 1;

                        for t in exit_bar_start..test_end.min(sd.close.len()) {
                            bars_held = t - bar;
                            if bars_held >= hold_max {
                                let exit_px = sd.close[t] * (1.0 - TAKER_FEE);
                                equity *= 1.0 + size * (exit_px / entry - 1.0);
                                total_trades += 1;
                                daily_rets.push((exit_px / entry - 1.0) * size);
                                exited = true;
                                break;
                            }

                            highest_high = highest_high.max(sd.high[t]);
                            let atr = atr_at(&sd.high, &sd.low, &sd.close, TURTLE_ATR_PERIOD, t);
                            let turtle_stop = highest_high - TURTLE_ATR_MULT * atr;

                            if sd.close[t] <= turtle_stop {
                                let exit_px = sd.close[t] * (1.0 - TAKER_FEE);
                                equity *= 1.0 + size * (exit_px / entry - 1.0);
                                total_trades += 1;
                                daily_rets.push((exit_px / entry - 1.0) * size);
                                exited = true;
                                break;
                            }
                        }

                        if !exited {
                            let last_idx = (test_end - 1).min(sd.close.len() - 1);
                            let exit_px = sd.close[last_idx] * (1.0 - TAKER_FEE);
                            equity *= 1.0 + size * (exit_px / entry - 1.0);
                            total_trades += 1;
                            daily_rets.push((exit_px / entry - 1.0) * size);
                        }
                    }
                }
            }
        }

        equity_curve.push(equity);
        bar += 1;
    }

    WfResult {
        equity,
        sharpe: annualised_sharpe(&daily_rets),
        dd: max_dd_from(&equity_curve),
        trades: total_trades,
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    println!("Loading data...");
    let loader = DataLoader::new(None, None);
    let mut sym_data: HashMap<String, SymData> = HashMap::new();
    let mut min_len = usize::MAX;

    let all_symbols: std::collections::HashSet<&str> = UNIVERSES
        .iter()
        .flat_map(|(_, s)| s.iter())
        .copied()
        .collect();

    for &sym in &all_symbols {
        let df = loader.fetch_data(sym, "1d", CANDLES).await?;
        let close: Vec<f64> = df.column("close")?.f64()?.into_no_null_iter().collect();
        let high: Vec<f64> = df.column("high")?.f64()?.into_no_null_iter().collect();
        let low: Vec<f64> = df.column("low")?.f64()?.into_no_null_iter().collect();
        let vol: Vec<f64> = df.column("volume")?.f64()?.into_no_null_iter().collect();
        if close.len() < min_len { min_len = close.len(); }
        sym_data.insert(sym.to_string(), SymData { close, high, low, vol });
    }

    if min_len < TRAIN_BARS + TEST_BARS + 100 {
        anyhow::bail!("Insufficient data length: {}", min_len);
    }

    let windows = (min_len.saturating_sub(TRAIN_BARS)) / TEST_BARS;
    eprintln!("HOLD_MAX Sweep: {} values × {} universes × {} windows = {} runs",
        HM_VALUES.len(), UNIVERSES.len(), windows, HM_VALUES.len() * UNIVERSES.len() * windows);

    // ── Export summary CSV ──────────────────────────────────────────────────
    let summary_path = "snapshots/hold_max_live_path_summary.csv";
    let mut sf = File::create(summary_path)?;
    writeln!(sf, "hm,pass_count,total_windows,total_trades,avg_return_pct,avg_sharpe,avg_dd_pct")?;

    // ── Export equity CSV (for winner + runner-ups) ────────────────────────
    let equity_path = "snapshots/hold_max_live_path_equity.csv";
    let mut ef = File::create(equity_path)?;
    writeln!(ef, "hm,window,equity")?;

    let mut results = vec![];

    for &hm in HM_VALUES {
        let mut total_pass = 0usize;
        let mut total_windows = 0usize;
        let mut total_trades = 0usize;
        let mut all_sharpe = vec![];
        let mut all_ret = vec![];

        // Per-window equity for Base5 (index 0)
        let mut window_equities: Vec<f64> = vec![1.0_f64; windows];

        for (u_idx, (_, u_syms)) in UNIVERSES.iter().enumerate() {
            let syms: Vec<String> = u_syms.iter().map(|&s| s.to_string()).collect();

            for w in 0..windows {
                let start = min_len - (windows - w) * TEST_BARS - TRAIN_BARS;
                let end = start + TEST_BARS + TRAIN_BARS;
                let res = run_sim(&sym_data, &syms, start + TRAIN_BARS, end, hm);

                let pass = if res.trades >= MIN_TRADES && res.sharpe > 0.0 { 1 } else { 0 };
                total_pass += pass;
                total_windows += 1;
                total_trades += res.trades;
                all_sharpe.push(res.sharpe);
                all_ret.push((res.equity - 1.0) * 100.0);

                // Compound Base5 equity across windows
                if u_idx == 0 {
                    window_equities[w] *= res.equity;
                }
            }
        }

        let n = total_windows as f64;
        let avg_sharpe = all_sharpe.iter().sum::<f64>() / n;
        let avg_ret = all_ret.iter().sum::<f64>() / n;

        writeln!(sf, "{},{},{},{},{:.4},{:.6},{}", hm, total_pass, total_windows, total_trades, avg_ret, avg_sharpe, 0.0)?;

        for (w, &eq) in window_equities.iter().enumerate() {
            writeln!(ef, "{},{},{:.6}", hm, w, eq)?;
        }

        results.push((hm, total_pass, total_windows, total_trades, avg_sharpe, avg_ret));
        eprintln!("HM={:3}: {}/{} pass, Sharpe={:.3}, Ret={:.1}%, Trades={}",
            hm, total_pass, total_windows, avg_sharpe, avg_ret, total_trades);
    }

    sf.flush()?;

    eprintln!("\nEquity CSV: {}", equity_path);
    eprintln!("Summary CSV: {}", summary_path);

    // ── Print top 5 by pass rate + Sharpe ───────────────────────────────────
    results.sort_by(|a, b| {
        let ra = a.1 as f64 / a.2 as f64;
        let rb = b.1 as f64 / b.2 as f64;
        let cmp = rb.partial_cmp(&ra).unwrap();
        if cmp != std::cmp::Ordering::Equal { cmp }
        else { b.4.partial_cmp(&a.4).unwrap() }
    });
    eprintln!("\n=== Results ranked by pass rate then Sharpe ===");
    for (hm, pass, tot, trades, sharpe, ret) in &results {
        let pct = (*pass as f64 / *tot as f64) * 100.0;
        eprintln!("HM={:3}: {}/{} ({:.0}% pass), Sharpe={:.3}, Ret={:.1}%, Trades={}",
            hm, pass, tot, pct, sharpe, ret, trades);
    }

    Ok(())
}
