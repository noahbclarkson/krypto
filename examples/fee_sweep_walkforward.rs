//! Fee sensitivity sweep for Turtle+Chandelier validated strategy.
//!
//! Audit target: the historical harness hardcoded `TAKER_FEE = 0.001`, while live
//! config uses `0.0004`. Older harnesses also applied the same `(1 - fee)` factor
//! to entry and exit, which algebraically cancels the fee impact. This harness
//! sweeps the actual taker-fee assumption with correct long-side fee accounting:
//! entry cost = close * (1 + fee), exit proceeds = close * (1 - fee).

use anyhow::Result;
use krypto::data::loader::DataLoader;
use polars::prelude::*;
use std::collections::{HashMap, HashSet};
use std::fs::File;
use std::io::Write;
use std::time::Instant;

const CANDLES: u32 = 3000;
const TRAIN_BARS: usize = 252;
const TEST_BARS: usize = 252;
const HOLD_MAX: usize = 12;
const POSITION_CAP: usize = 3;
const MIN_TRADES: usize = 3;
const CHAND_PERIOD: usize = 7;
const CHAND_MULT: f64 = 2.30;
const TURTLE_ENTRY: usize = 21;
const TURTLE_ATR_PERIOD: usize = 24;
const TURTLE_ATR_MULT: f64 = 2.00;
const ATR_ENTRY_MULT: f64 = 0.00;
const VOL_LOOKBACK: usize = 8;

// Extensive cost sweep: 0 to 20 bps per side in 1 bp increments.
// Baseline historical harness = 10 bps/side. Live config = 4 bps/side.
const FEE_SWEEP: &[f64] = &[
    0.0000, 0.0001, 0.0002, 0.0003, 0.0004, 0.0005, 0.0006, 0.0007, 0.0008, 0.0009, 0.0010, 0.0011,
    0.0012, 0.0013, 0.0014, 0.0015, 0.0016, 0.0017, 0.0018, 0.0019, 0.0020,
];

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

const METRICS_CSV: &str = "snapshots/fee_sweep_walkforward_metrics.csv";
const SUMMARY_CSV: &str = "snapshots/fee_sweep_walkforward_summary.csv";
const EQUITY_CSV: &str = "snapshots/fee_sweep_walkforward_equity.csv";

struct SymData {
    close: Vec<f64>,
    high: Vec<f64>,
    low: Vec<f64>,
    vol: Vec<f64>,
}

fn atr_at(high: &[f64], low: &[f64], close: &[f64], period: usize, idx: usize) -> f64 {
    if idx < period {
        return 0.0;
    }
    let mut sum = 0.0;
    for i in (idx + 1 - period)..=idx {
        let h = high.get(i).copied().unwrap_or(0.0);
        let l = low.get(i).copied().unwrap_or(0.0);
        let c0 = close.get(i.saturating_sub(1)).copied().unwrap_or(0.0);
        sum += (h - l).max((h - c0).abs()).max((l - c0).abs());
    }
    sum / period as f64
}

fn rolling_avg(vals: &[f64], window: usize, idx: usize) -> f64 {
    if idx < window {
        return vals.get(idx).copied().unwrap_or(0.0);
    }
    let start = idx + 1 - window;
    vals[start..=idx].iter().sum::<f64>() / window as f64
}

fn turtle_signal(close: &[f64], high: &[f64], low: &[f64], idx: usize) -> bool {
    if idx < TURTLE_ENTRY + 1 {
        return false;
    }
    let start = idx + 1 - TURTLE_ENTRY;
    let max_close = close[start..idx]
        .iter()
        .fold(f64::NEG_INFINITY, |a, &b| a.max(b));
    let curr_close = close.get(idx).copied().unwrap_or(0.0);
    let breakout = curr_close > max_close;
    if breakout && ATR_ENTRY_MULT > 0.0 {
        let atr_val = atr_at(high, low, close, TURTLE_ATR_PERIOD, idx);
        curr_close >= max_close + ATR_ENTRY_MULT * atr_val
    } else {
        breakout
    }
}

fn annualised_sharpe(daily_rets: &[f64]) -> f64 {
    if daily_rets.len() < 2 {
        return 0.0;
    }
    let mn: f64 = daily_rets.iter().sum::<f64>() / daily_rets.len() as f64;
    let sd =
        (daily_rets.iter().map(|x| (x - mn).powi(2)).sum::<f64>() / daily_rets.len() as f64).sqrt();
    if sd == 0.0 {
        0.0
    } else {
        mn * 365.0_f64.sqrt() / sd
    }
}

fn max_dd_from(equity: &[f64]) -> f64 {
    let mut peak = f64::NEG_INFINITY;
    let mut max_dd = 0.0_f64;
    for &e in equity {
        if e > peak {
            peak = e;
        }
        if peak > 0.0 {
            max_dd = max_dd.max((peak - e) / peak);
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
    equity_curve: Vec<f64>,
}

fn run_sim(
    sym_data: &HashMap<String, SymData>,
    symbols: &[String],
    test_start: usize,
    test_end: usize,
    taker_fee: f64,
) -> WfResult {
    let mut equity = 1.0_f64;
    let mut equity_curve = vec![1.0_f64];
    let mut wins = 0usize;
    let mut trades = 0usize;
    let mut daily_rets = Vec::new();

    let mut bar = test_start;
    while bar + 2 < test_end {
        let mut scores: Vec<(&str, f64)> = Vec::new();
        for sym in symbols {
            if let Some(sd) = sym_data.get(sym) {
                if bar >= sd.close.len() {
                    continue;
                }
                let dv = rolling_avg(&sd.vol, VOL_LOOKBACK, bar)
                    * sd.close.get(bar).copied().unwrap_or(0.0);
                scores.push((
                    sym.as_str(),
                    if dv.is_finite() && dv > 0.0 { dv } else { 0.0 },
                ));
            }
        }
        scores.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());
        let top_syms: Vec<String> = scores
            .into_iter()
            .take(POSITION_CAP)
            .map(|(s, _)| s.to_string())
            .collect();
        if top_syms.is_empty() {
            equity_curve.push(equity);
            bar += 1;
            continue;
        }

        let mut entered = false;
        for sym in &top_syms {
            if let Some(sd) = sym_data.get(sym) {
                if bar >= TURTLE_ENTRY + 1
                    && bar < sd.close.len()
                    && turtle_signal(&sd.close, &sd.high, &sd.low, bar)
                {
                    let entry_px = sd.close[bar] * (1.0 + taker_fee);
                    let entry_bar_next = bar + 1;
                    let n = sd.close.len();
                    let mut highest_high_chand = sd.high[entry_bar_next];
                    let mut lowest_low_turtle = sd.low[entry_bar_next];
                    let max_bar = (entry_bar_next + HOLD_MAX).min(n.saturating_sub(1));
                    let mut exit_bar = max_bar;
                    for b in entry_bar_next..=max_bar.min(n.saturating_sub(1)) {
                        highest_high_chand = highest_high_chand.max(sd.high[b]);
                        let trail_chand = highest_high_chand
                            - CHAND_MULT * atr_at(&sd.high, &sd.low, &sd.close, CHAND_PERIOD, b);
                        lowest_low_turtle = lowest_low_turtle.min(sd.low[b]);
                        let trail_turtle = lowest_low_turtle
                            - TURTLE_ATR_MULT
                                * atr_at(&sd.high, &sd.low, &sd.close, TURTLE_ATR_PERIOD, b);
                        if sd.close[b] < trail_chand || sd.close[b] < trail_turtle {
                            exit_bar = b;
                            break;
                        }
                    }
                    if let Some(&exit_raw) = sd.close.get(exit_bar) {
                        let exit_px = exit_raw * (1.0 - taker_fee);
                        let trade_ret = exit_px / entry_px - 1.0;
                        let bars_held = (exit_bar as i64 - entry_bar_next as i64).max(1) as usize;
                        wins += if trade_ret > 0.0 { 1 } else { 0 };
                        trades += 1;
                        equity *= 1.0 + trade_ret;
                        for _ in 0..bars_held {
                            daily_rets.push(trade_ret / bars_held as f64);
                        }
                        equity_curve.push(equity);
                        bar = exit_bar + 1;
                        entered = true;
                        break;
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
    let win_rate = if trades > 0 {
        wins as f64 / trades as f64 * 100.0
    } else {
        0.0
    };
    let pass = trades >= MIN_TRADES && ret > 0.0;
    WfResult {
        ret,
        sharpe,
        max_dd,
        trades,
        win_rate,
        pass,
        equity_curve,
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    let t0 = Instant::now();
    eprintln!("==== Turtle+Chandelier Fee Sensitivity Sweep ====");
    eprintln!("Fees: 0..20 bps/side step 1 bp; correct entry=(1+fee), exit=(1-fee)\n");

    let loader = DataLoader::new(None, None);
    let mut all_syms: HashSet<String> = HashSet::new();
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
            Err(e) => eprintln!("WARNING: {} load failed: {}", sym, e),
        }
    }
    let n = min_len.min(2800);
    let mut sym_data: HashMap<String, SymData> = HashMap::new();
    for sym in &all_syms {
        if let Some(df) = raw_cache.get(sym) {
            let n_min = df.height().min(n);
            macro_rules! col_vec {
                ($name:expr) => {{
                    df.column($name)?
                        .f64()?
                        .into_iter()
                        .filter_map(|x| x)
                        .take(n_min)
                        .collect::<Vec<_>>()
                }};
            }
            sym_data.insert(
                sym.clone(),
                SymData {
                    close: col_vec!("close"),
                    high: col_vec!("high"),
                    low: col_vec!("low"),
                    vol: col_vec!("volume"),
                },
            );
        }
    }
    eprintln!("Loaded {} symbols, {} bars", sym_data.len(), n);

    let total_windows = n.saturating_sub(TRAIN_BARS + TEST_BARS) / TEST_BARS;
    let mut metrics = vec![
        "fee_bps,universe,window,return_pct,sharpe,max_dd_pct,trades,win_rate_pct,pass".to_string(),
    ];
    let mut summary = vec!["fee_bps,fee_per_side,pass,total,pass_rate_pct,avg_return_pct,avg_sharpe,worst_dd_pct,total_trades".to_string()];
    let mut equity_lines = vec!["fee_bps,step,equity".to_string()];

    for &fee in FEE_SWEEP {
        let fee_bps = fee * 10_000.0;
        let mut pass = 0usize;
        let mut total = 0usize;
        let mut total_trades = 0usize;
        let mut sum_ret = 0.0;
        let mut sum_sh = 0.0;
        let mut worst_dd = 0.0_f64;
        let mut composite_equity = 1.0_f64;
        let mut step = 0usize;
        equity_lines.push(format!("{:.1},{},{}", fee_bps, step, composite_equity));

        for &(label, symbols_raw) in UNIVERSES {
            let symbols: Vec<String> = symbols_raw.iter().map(|s| s.to_string()).collect();
            if !symbols.iter().all(|s| sym_data.contains_key(s)) {
                continue;
            }
            for wi in 0..total_windows {
                let test_start = TRAIN_BARS + wi * TEST_BARS;
                let test_end = (test_start + TEST_BARS).min(n);
                if test_end.saturating_sub(test_start) < 5 {
                    continue;
                }
                let r = run_sim(&sym_data, &symbols, test_start, test_end, fee);
                metrics.push(format!(
                    "{:.1},{},{},{:.2},{:.4},{:.2},{},{:.2},{}",
                    fee_bps, label, wi, r.ret, r.sharpe, r.max_dd, r.trades, r.win_rate, r.pass
                ));
                pass += if r.pass { 1 } else { 0 };
                total += 1;
                total_trades += r.trades;
                sum_ret += r.ret;
                sum_sh += r.sharpe;
                worst_dd = worst_dd.max(r.max_dd);

                // Charting OOS equity curve: concatenate Base5 windows only.
                // All 9 universes are used for metric validation above; Base5 avoids
                // the misleading overflow produced by multiplying 54 independent
                // validation windows/universes into one artificial mega-portfolio.
                if label == "Base5" {
                    let start_eq = r.equity_curve.first().copied().unwrap_or(1.0);
                    for &eq in r.equity_curve.iter().skip(1) {
                        step += 1;
                        composite_equity *= eq / start_eq;
                        equity_lines.push(format!("{:.1},{},{}", fee_bps, step, composite_equity));
                    }
                }
            }
        }
        let pass_rate = pass as f64 / total.max(1) as f64 * 100.0;
        let avg_ret = sum_ret / total.max(1) as f64;
        let avg_sh = sum_sh / total.max(1) as f64;
        eprintln!(
            "fee={:>4.1} bps | {}/{} pass ({:.1}%) | ret={:+.1}% sh={:.3} DD={:.1}% trades={}",
            fee_bps, pass, total, pass_rate, avg_ret, avg_sh, worst_dd, total_trades
        );
        summary.push(format!(
            "{:.1},{:.6},{},{},{:.2},{:.4},{:.6},{:.4},{}",
            fee_bps, fee, pass, total, pass_rate, avg_ret, avg_sh, worst_dd, total_trades
        ));
    }

    let mut f = File::create(METRICS_CSV)?;
    for l in &metrics {
        writeln!(f, "{}", l)?;
    }
    let mut f = File::create(SUMMARY_CSV)?;
    for l in &summary {
        writeln!(f, "{}", l)?;
    }
    let mut f = File::create(EQUITY_CSV)?;
    for l in &equity_lines {
        writeln!(f, "{}", l)?;
    }
    eprintln!(
        "Wrote {}, {}, {} in {:?}",
        METRICS_CSV,
        SUMMARY_CSV,
        EQUITY_CSV,
        t0.elapsed()
    );
    Ok(())
}
