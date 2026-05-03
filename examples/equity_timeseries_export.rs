//! T50: Equity Time-Series Export for ATR_RANK Threshold Comparison
//!
//! Exports daily equity time-series for Baseline (T=0), Runner-ups (T=5, T=30),
//! and Winner (T=24) across all 9 universes × 7 WF windows.
//! Charts: `charts/plot_atr_rank_equity_comparison.py`

use std::collections::{HashMap, VecDeque};
use std::fs::File;
use std::io::Write;

use anyhow::Result;

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
const VOL_LOOKBACK: usize = 8;
const REGIME_ATR_PERIOD: usize = 12;
const REGIME_LOOKBACK: usize = 42;

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

// T=0 (baseline), T=5 (old default), T=24 (winner), T=30 (runner-up), T=77 (high-threshold candidate)
const THRESHOLDS: &[f64] = &[0.0, 5.0, 24.0, 30.0, 77.0];

struct SymData {
    close: Vec<f64>,
    high: Vec<f64>,
    low: Vec<f64>,
    vol: Vec<f64>,
}

fn atr_at(high: &[f64], low: &[f64], close: &[f64], period: usize, idx: usize) -> f64 {
    if idx < period { return 0.0; }
    let mut sum = 0.0_f64;
    for i in (idx + 1 - period)..=idx {
        let c0 = close[i.saturating_sub(1)];
        sum += (high[i] - low[i]).max((high[i] - c0).abs()).max((low[i] - c0).abs());
    }
    sum / period as f64
}

fn rolling_avg(vals: &[f64], window: usize, idx: usize) -> f64 {
    if idx < window { return 0.0; }
    vals[idx.saturating_sub(window - 1)..=idx].iter().sum::<f64>() / window as f64
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

fn turtle_signal(
    close: &[f64], high: &[f64], low: &[f64],
    entry_period: usize, atr_period: usize, atr_mult: f64, idx: usize,
) -> bool {
    if idx < entry_period { return false; }
    let start = idx - entry_period;
    let max_close = close[start..idx].iter().fold(f64::NEG_INFINITY, |a, &b| a.max(b));
    if close[idx] <= max_close { return false; }
    if atr_mult > 0.0 {
        let atr = atr_at(high, low, close, atr_period, idx);
        if close[idx] < max_close + atr * atr_mult { return false; }
    }
    true
}

fn max_dd(equity: &[f64]) -> f64 {
    let mut peak = 1.0_f64;
    let mut max_dd = 0.0_f64;
    for &e in equity {
        if e > peak { peak = e; }
        let dd = 1.0 - e / peak;
        if dd > max_dd { max_dd = dd; }
    }
    max_dd * 100.0
}

fn annualised_sharpe(daily_equity: &[f64]) -> f64 {
    if daily_equity.len() < 10 { return 0.0; }
    let rets: Vec<f64> = daily_equity.windows(2).map(|w| w[1] / w[0] - 1.0).collect();
    let mean = rets.iter().sum::<f64>() / rets.len() as f64;
    let var = rets.iter().map(|x| (x - mean).powi(2)).sum::<f64>() / rets.len() as f64;
    if var == 0.0 { return 0.0; }
    (mean / var.sqrt()) * (365.0_f64).sqrt()
}

fn run_sim_for_t(
    sym_data: &HashMap<String, SymData>,
    symbols: &[String],
    btc_data: &SymData,
    t_thresh: f64,
    test_start: usize,
    test_end: usize,
) -> (f64, usize, f64, Vec<f64>) {
    let mut equity = 1.0_f64;
    let mut daily_equity = vec![1.0_f64];
    let mut peak = equity;
    let mut total_trades = 0usize;
    let mut bar = test_start;

    while bar + 2 < test_end {
        let btc_pct = btc_atr_pct(btc_data, REGIME_ATR_PERIOD, REGIME_LOOKBACK, bar);

        if btc_pct < t_thresh {
            daily_equity.push(equity);
            bar += 1;
            continue;
        }

        // Dollar-volume ranking
        let mut scores: Vec<(&str, f64)> = Vec::new();
        for sym in symbols {
            if let Some(sd) = sym_data.get(sym) {
                if bar >= sd.close.len() { continue; }
                let rol_vol = rolling_avg(&sd.vol, VOL_LOOKBACK, bar);
                let price = *sd.close.get(bar).unwrap_or(&0.0);
                let dv = rol_vol * price;
                scores.push((sym.as_str(), if dv.is_finite() && dv > 0.0 { dv } else { 0.0 }));
            }
        }
        scores.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());
        let top_syms: Vec<String> = scores.into_iter().take(POSITION_CAP).map(|(s, _)| s.to_string()).collect();

        if top_syms.is_empty() {
            daily_equity.push(equity);
            bar += 1;
            continue;
        }

        let mut entered = false;
        for sym in &top_syms {
            if let Some(sd) = sym_data.get(sym) {
                if bar < TURTLE_ENTRY + 1 || bar >= sd.close.len() { continue; }
                if !turtle_signal(&sd.close, &sd.high, &sd.low, TURTLE_ENTRY, TURTLE_ATR_PERIOD, ATR_ENTRY_MULT, bar) {
                    continue;
                }

                let entry_px = sd.close[bar];
                let mut size_mult = 1.0;
                if bar >= 252 + 21 {
                    let atr_21 = atr_at(&btc_data.high, &btc_data.low, &btc_data.close, 21, bar);
                    let mut hist = Vec::with_capacity(252);
                    for j in (bar + 1 - 252)..=bar {
                        let h = btc_data.high[j];
                        let l = btc_data.low[j];
                        let c0 = btc_data.close[j.saturating_sub(1)];
                        hist.push((h - l).max((h - c0).abs()).max((l - c0).abs()));
                    }
                    hist.sort_by(|a, b| a.partial_cmp(b).unwrap());
                    let pct_75 = hist[(0.75 * hist.len() as f64) as usize];
                    if atr_21 > pct_75 { size_mult = 0.70; }
                }

                let entry = entry_px * (1.0 + TAKER_FEE);
                let entry_bar_next = bar + 1;
                let n = sd.close.len();

                let mut highest_high = sd.high[entry_bar_next];
                let max_bar = (entry_bar_next + HOLD_MAX).min(n.saturating_sub(1));
                let mut exit_bar = max_bar;

                let mut atr_buf: VecDeque<f64> = VecDeque::new();
                for b in entry_bar_next..=max_bar {
                    if sd.high[b] > highest_high { highest_high = sd.high[b]; }
                    let c0 = sd.close[b.saturating_sub(1)];
                    let tr = (sd.high[b] - sd.low[b])
                        .max((sd.high[b] - c0).abs())
                        .max((sd.low[b] - c0).abs());
                    atr_buf.push_back(tr);
                    if atr_buf.len() > TURTLE_ATR_PERIOD { atr_buf.pop_front(); }

                    if atr_buf.len() == TURTLE_ATR_PERIOD {
                        let atr = atr_buf.iter().sum::<f64>() / TURTLE_ATR_PERIOD as f64;
                        let turtle_stop = highest_high - TURTLE_ATR_MULT * atr;
                        if sd.low[b] <= turtle_stop {
                            exit_bar = b;
                            break;
                        }
                    }
                }

                if let Some(&exit_px) = sd.close.get(exit_bar) {
                    let exit = exit_px * (1.0 - TAKER_FEE);
                    let pct_ret = exit / entry - 1.0;
                    equity *= 1.0 + pct_ret * size_mult;
                    total_trades += 1;
                    if equity > peak { peak = equity; }
                    daily_equity.push(equity);
                    bar = exit_bar + 1;
                    entered = true;
                    break;
                }
            }
        }
        if !entered {
            daily_equity.push(equity);
            bar += 1;
        }
    }

    (equity, total_trades, max_dd(&daily_equity), daily_equity)
}

#[tokio::main]
async fn main() -> Result<()> {
    println!("Loading data...");
    let loader = krypto::data::loader::DataLoader::new(None, None);
    let mut sym_data = HashMap::new();

    let all_symbols: std::collections::HashSet<&str> =
        UNIVERSES.iter().flat_map(|(_, s)| s.iter().copied()).collect();

    for &sym in &all_symbols {
        let df = loader.fetch_data(sym, "1d", CANDLES).await?;
        let close = df.column("close")?.f64()?.into_no_null_iter().collect::<Vec<_>>();
        let high = df.column("high")?.f64()?.into_no_null_iter().collect::<Vec<_>>();
        let low  = df.column("low")?.f64()?.into_no_null_iter().collect::<Vec<_>>();
        let vol  = df.column("volume")?.f64()?.into_no_null_iter().collect::<Vec<_>>();
        sym_data.insert(sym.to_string(), SymData { close, high, low, vol });
    }

    let btc = sym_data.get("BTCUSDT").expect("BTCUSDT required");
    let min_len = sym_data.values().map(|s| s.close.len()).min().unwrap_or(0);
    let windows = (min_len.saturating_sub(TRAIN_BARS)) / TEST_BARS;

    println!("Loaded {} symbols. Min len={}, {} WF windows.", sym_data.len(), min_len, windows);

    // Export per-universe × per-threshold time-series equity CSVs
    // Format: window,bar_idx,equity (one row per bar per window)
    for (u_name, u_syms) in UNIVERSES {
        let syms: Vec<String> = u_syms.iter().map(|&s| s.to_string()).collect();

        for &t in THRESHOLDS {
            let csv_path = format!("snapshots/equity_ts_{}_{:.0}.csv", u_name, t);
            let mut f = File::create(&csv_path)?;
            writeln!(f, "window,bar_idx,equity")?;

            for w in 0..windows {
                let start = min_len - (windows - w) * TEST_BARS - TRAIN_BARS;
                let end = start + TRAIN_BARS + TEST_BARS;
                let (_, _, _, eq_curve) = run_sim_for_t(
                    &sym_data, &syms, btc, t, start + TRAIN_BARS, end
                );

                for (bar_idx, &eq) in eq_curve.iter().enumerate() {
                    writeln!(f, "{},{},{:.8}", w, bar_idx, eq)?;
                }
            }
            println!("Exported: {}", csv_path);
        }
    }

    // Print summary comparison table
    println!("\n=== ATR_RANK Threshold Comparison ===");
    println!("{:<14} {:>5} {:>6} {:>8} {:>8} {:>8}", "Universe", "T", "Pass", "Sharpe", "Ret%", "DD%");
    println!("{}", "-".repeat(56));

    for (u_name, u_syms) in UNIVERSES {
        let syms: Vec<String> = u_syms.iter().map(|&s| s.to_string()).collect();
        for &t in THRESHOLDS {
            let mut passes = 0usize;
            let mut sharpes = vec![];
            let mut rets = vec![];
            let mut dds = vec![];
            for w in 0..windows {
                let start = min_len - (windows - w) * TEST_BARS - TRAIN_BARS;
                let end = start + TRAIN_BARS + TEST_BARS;
                let (eq_final, trades, dd, eq_curve) = run_sim_for_t(
                    &sym_data, &syms, btc, t, start + TRAIN_BARS, end
                );
                if trades >= MIN_TRADES {
                    let rets2: Vec<f64> = eq_curve.windows(2).map(|w| w[1]/w[0]-1.0).collect();
                    let sh = annualised_sharpe(&rets2);
                    if sh > 0.0 { passes += 1; }
                    sharpes.push(sh);
                    rets.push((eq_final - 1.0) * 100.0);
                    dds.push(dd);
                }
            }
            let avg_sh = if sharpes.is_empty() { 0.0 } else { sharpes.iter().sum::<f64>() / sharpes.len() as f64 };
            let avg_ret = if rets.is_empty() { 0.0 } else { rets.iter().sum::<f64>() / rets.len() as f64 };
            let avg_dd = if dds.is_empty() { 0.0 } else { dds.iter().sum::<f64>() / dds.len() as f64 };
            println!("{:<14} {:>5.0} {:>6} {:>8.2} {:>8.1}% {:>8.1}%",
                u_name, t, passes, avg_sh, avg_ret, avg_dd);
        }
        println!();
    }

    println!("\nDone. Charts: python3 charts/plot_atr_rank_equity_comparison.py");
    Ok(())
}