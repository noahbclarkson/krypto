//! USDT Hedge Size_Mult Hyperopt
//!
//! Tests: size_mult ∈ {0.50, 0.55, ..., 1.00} step 0.05 (11 values)
//! Strategy: Turtle-only exit (live bot path)
//! Harness: Base5 × 7 walk-forward windows
//! Metric: Pass rate, Sharpe, return, drawdown per size_mult value
//!
//! Exports:
//!   snapshots/usdt_hedge_base5_sm{val}.csv — equity per window
//!   snapshots/usdt_hedge_sweep_summary.csv — full results table

use anyhow::Result;
use krypto::data::loader::DataLoader;
use std::collections::HashMap;
use std::fs::File;
use std::io::Write;
use std::fmt::Write as FmtWrite;

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
const VOL_LOOKBACK: usize = 96; // production default

const REGIME_ATR_PERIOD: usize = 12;
const REGIME_LOOKBACK: usize = 42;
const ATR_RANK_T: f64 = 24.0;

const BASE5: &[&str] = &["BTCUSDT","ETHUSDT","SOLUSDT","XRPUSDT","DOGEUSDT","ADAUSDT"];
const WINDOWS: usize = 7;

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
    if idx <= period.max(lookback) { return 50.0; }
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

fn annualised_sharpe(daily_rets: &[f64]) -> f64 {
    if daily_rets.is_empty() { return 0.0; }
    let mean = daily_rets.iter().sum::<f64>() / daily_rets.len() as f64;
    let var = daily_rets.iter().map(|x| (x - mean).powi(2)).sum::<f64>() / daily_rets.len() as f64;
    if var == 0.0 { return 0.0; }
    (mean / var.sqrt()) * (365.0_f64).sqrt()
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
    size_mult: f64,
) -> WfResult {
    let mut equity = 1.0_f64;
    let mut equity_curve = vec![1.0_f64];
    let mut peak = equity;
    let mut total_trades = 0usize;
    let mut daily_rets = Vec::new();

    let mut bar = test_start;
    while bar + 2 < test_end {
        let btc = sym_data.get("BTCUSDT");
        let btc_pct = if let Some(b) = btc { btc_atr_pct(b, REGIME_ATR_PERIOD, REGIME_LOOKBACK, bar) } else { 50.0 };

        if btc_pct < ATR_RANK_T {
            equity_curve.push(equity);
            bar += 1;
            continue;
        }

        // Dollar-volume ranking
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
                    if turtle_signal(&sd.close, &sd.high, &sd.low, TURTLE_ENTRY, TURTLE_ATR_PERIOD, ATR_ENTRY_MULT, bar) {
                        let entry_px = sd.close[bar];

                        // USDT hedge: size_mult applied to this position
                        let sm = if let Some(b) = btc {
                            if bar >= 252 + 21 {
                                let atr_21 = atr_at(&b.high, &b.low, &b.close, 21, bar);
                                let mut hist = Vec::with_capacity(252);
                                for j in (bar + 1 - 252)..=bar {
                                    let h = b.high[j];
                                    let l = b.low[j];
                                    let c0 = b.close[j.saturating_sub(1)];
                                    hist.push((h - l).max((h - c0).abs()).max((l - c0).abs()));
                                }
                                hist.sort_by(|a, b| a.partial_cmp(b).unwrap());
                                let pct_75 = hist[(0.75 * hist.len() as f64) as usize];
                                if atr_21 > pct_75 {
                                    size_mult
                                } else {
                                    1.0
                                }
                            } else { 1.0 }
                        } else { 1.0 };

                        let entry = entry_px * (1.0 + TAKER_FEE);
                        let entry_bar_next = bar + 1;
                        let n = sd.close.len();

                        let mut highest_high = sd.high[entry_bar_next];
                        let max_bar = (entry_bar_next + HOLD_MAX).min(n.saturating_sub(1));
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
                                if sd.low[b] <= turtle_stop {
                                    exit_bar = b;
                                    break;
                                }
                            }
                        }

                        if let Some(&exit_px) = sd.close.get(exit_bar) {
                            let exit = exit_px * (1.0 - TAKER_FEE);
                            let pct_ret = exit / entry - 1.0;
                            let gross_ret = pct_ret * sm;

                            let bars_held = (exit_bar as i64 - entry_bar_next as i64).max(1) as usize;
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

    WfResult {
        equity,
        sharpe: annualised_sharpe(&daily_rets),
        dd: max_dd_from(&equity_curve),
        trades: total_trades,
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    println!("Loading data for USDT hedge size_mult sweep...");
    let loader = DataLoader::new(None, None);
    let mut sym_data = HashMap::new();

    for &sym in BASE5 {
        let df = loader.fetch_data(sym, "1d", CANDLES).await?;
        let close = df.column("close")?.f64()?.into_no_null_iter().collect::<Vec<_>>();
        let high = df.column("high")?.f64()?.into_no_null_iter().collect::<Vec<_>>();
        let low = df.column("low")?.f64()?.into_no_null_iter().collect::<Vec<_>>();
        let vol = df.column("volume")?.f64()?.into_no_null_iter().collect::<Vec<_>>();
        sym_data.insert(sym.to_string(), SymData { close, high, low, vol });
    }

    let min_len = sym_data.values().map(|d| d.close.len()).min().unwrap_or(0);
    let n_windows = (min_len.saturating_sub(TRAIN_BARS)) / TEST_BARS;
    if n_windows == 0 { return Ok(()); }

    // Extensive size_mult range: 0.50 to 1.00 step 0.05 = 11 values
    let size_mults: Vec<f64> = (0..=10).map(|i| 0.50 + i as f64 * 0.05).collect();

    // Summary CSV
    let summary_path = "snapshots/usdt_hedge_sweep_summary.csv";
    let mut sum_f = File::create(summary_path)?;
    writeln!(sum_f, "size_mult,pass_rate,avg_sharpe,avg_return,avg_dd,total_trades")?;

    let mut all_results: Vec<(f64, usize, f64, f64, f64, usize)> = Vec::new();

    for &sm in &size_mults {
        let mut passes = 0usize;
        let mut sharpes = vec![];
        let mut rets = vec![];
        let mut dds = vec![];
        let mut total_trades = 0usize;
        let mut window_equities = vec![];

        for w in 0..n_windows {
            let start = min_len - (n_windows - w) * TEST_BARS - TRAIN_BARS;
            let end = start + TEST_BARS + TRAIN_BARS;
            let res = run_sim(&sym_data, &BASE5.iter().map(|&s| s.to_string()).collect::<Vec<_>>(),
                              start + TRAIN_BARS, end, sm);
            if res.trades >= MIN_TRADES && res.sharpe > 0.0 { passes += 1; }
            sharpes.push(res.sharpe);
            rets.push((res.equity - 1.0) * 100.0);
            dds.push(res.dd);
            total_trades += res.trades;
            window_equities.push(res.equity);
        }

        let n = n_windows as f64;
        let avg_sharpe = sharpes.iter().sum::<f64>() / n;
        let avg_ret = rets.iter().sum::<f64>() / n;
        let avg_dd = dds.iter().sum::<f64>() / n;
        let pass_rate = (passes as f64 / n) * 100.0;

        println!("sm={}: {}/{} pass ({:.1}%), Sharpe={:.3}, Ret={:.1}%, DD={:.1}%, Trades={}",
                sm, passes, n_windows, pass_rate, avg_sharpe, avg_ret, avg_dd, total_trades);

        writeln!(sum_f, "{},{:.1},{},{},{},{}",
                sm, pass_rate, avg_sharpe, avg_ret, avg_dd, total_trades)?;

        all_results.push((sm, passes, avg_sharpe, avg_ret, avg_dd, total_trades));

        // Export per-size_mult equity CSV
        let csv_path = format!("snapshots/usdt_hedge_base5_sm{:.2}.csv", sm);
        let mut f = File::create(&csv_path)?;
        writeln!(f, "window,equity")?;
        for (w_idx, &eq) in window_equities.iter().enumerate() {
            writeln!(f, "{},{:.6}", w_idx, eq)?;
        }
        println!("  Exported equity to {}", csv_path);
    }

    // Find robustness winner (highest pass rate, then highest Sharpe)
    all_results.sort_by(|a, b| {
        let pass_cmp = b.1.cmp(&a.1);
        if pass_cmp != std::cmp::Ordering::Equal { return pass_cmp; }
        b.2.partial_cmp(&a.2).unwrap()
    });

    let winner = all_results[0].0;
    println!("\n=== ROBUSTNESS WINNER: size_mult={} ===", winner);

    // Full per-universe report
    println!("\n=== Final Summary ===");
    println!("size_mult | pass | Sharpe | Ret% | DD% | Trades");
    println!("----------|-------|--------|-------|-----|-------");
    for r in &all_results {
        println!("{}     | {}/{} | {:.3} | {:.1}% | {:.1}% | {}",
                r.0, r.1, n_windows, r.2, r.3, r.4, r.5);
    }

    println!("\nSummary written to {}", summary_path);
    println!("Equity CSVs: snapshots/usdt_hedge_base5_sm*.csv");

    Ok(())
}