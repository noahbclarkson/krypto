//! Hyperparameter Equity Curve Export Harness
//!
//! Exports per-bar equity time-series for ATR_RANK_THRESHOLD parameter variants
//! across all universes and windows. Generates comparison charts.
//!
//! Usage: cargo run --example hyperopt_equity_export --profile sweep

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
const VOL_LOOKBACK: usize = 96;

const REGIME_ATR_PERIOD: usize = 12;
const REGIME_LOOKBACK: usize = 42;

const N_VARIANTS: usize = 5;
const VARIANTS_T: [f64; N_VARIANTS] = [0.0, 24.0, 10.0, 30.0, 50.0];
const VARIANTS_LABELS: [&str; N_VARIANTS] = [
    "T000_baseline",
    "T024_winner",
    "T010_runnerup1",
    "T030_runnerup2",
    "T050_runnerup3",
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
            if close[idx] < max_close + atr * atr_mult { return false; }
        }
        true
    } else { false }
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
    let mut peak = 1.0_f64;
    for &val in equity {
        if val > peak { peak = val; }
        let dd = 1.0 - val / peak;
        if dd > max_dd { max_dd = dd; }
    }
    max_dd * 100.0
}

fn run_sim(
    sym_data: &HashMap<String, SymData>,
    symbols: &[String],
    test_start: usize,
    test_end: usize,
    atr_rank_t: f64,
) -> (Vec<f64>, f64, usize, f64, f64) {
    let mut equity = 1.0_f64;
    let mut equity_curve = vec![1.0_f64];
    let mut peak = equity;
    let mut total_trades = 0usize;
    let mut daily_rets = Vec::new();

    let mut bar = test_start;
    while bar + 2 < test_end {
        let btc = sym_data.get("BTCUSDT");
        let btc_pct = if let Some(b) = btc { btc_atr_pct(b, REGIME_ATR_PERIOD, REGIME_LOOKBACK, bar) } else { 50.0 };

        if btc_pct < atr_rank_t {
            equity_curve.push(equity);
            bar += 1;
            continue;
        }

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
                        let mut size_mult = 1.0;
                        if let Some(b) = btc {
                            if bar >= 252 + 21 {
                                let atr_21 = atr_at(&b.high, &b.low, &b.close, 21, bar);
                                let mut hist = Vec::new();
                                for j in (bar + 1 - 252)..=bar {
                                    let h = b.high[j];
                                    let l = b.low[j];
                                    let c0 = b.close[j.saturating_sub(1)];
                                    hist.push((h - l).max((h - c0).abs()).max((l - c0).abs()));
                                }
                                hist.sort_by(|a, b| a.partial_cmp(b).unwrap());
                                let pct_75 = hist[(0.75 * hist.len() as f64) as usize];
                                if atr_21 > pct_75 {
                                    size_mult = 0.70;
                                }
                            }
                        }

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
                            let gross_ret = pct_ret * size_mult;

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

    let sharpe = annualised_sharpe(&daily_rets);
    let ret_pct = (equity - 1.0) * 100.0;
    let dd = max_dd_from(&equity_curve);

    (equity_curve, sharpe, total_trades, ret_pct, dd)
}

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

#[tokio::main]
async fn main() -> Result<()> {
    let now = std::time::Instant::now();
    println!("=== Hyperparameter Equity Curve Export ===");
    for i in 0..N_VARIANTS {
        println!("  {}: T={}", VARIANTS_LABELS[i], VARIANTS_T[i]);
    }

    let loader = DataLoader::new(None, None);
    let mut sym_data = HashMap::new();
    let mut min_len = usize::MAX;

    let all_symbols: std::collections::HashSet<_> = UNIVERSES
        .iter().flat_map(|(_, s)| s.iter()).copied().collect();
    for &sym in &all_symbols {
        let df = loader.fetch_data(sym, "1d", CANDLES).await?;
        let close: Vec<f64> = df.column("close")?.f64()?.into_no_null_iter().collect();
        let high: Vec<f64> = df.column("high")?.f64()?.into_no_null_iter().collect();
        let low: Vec<f64>  = df.column("low")?.f64()?.into_no_null_iter().collect();
        let vol: Vec<f64>  = df.column("volume")?.f64()?.into_no_null_iter().collect();
        if close.len() < min_len { min_len = close.len(); }
        sym_data.insert(sym.to_string(), SymData { close, high, low, vol });
    }

    let n_windows = (min_len.saturating_sub(TRAIN_BARS)) / TEST_BARS;
    let n_runs = UNIVERSES.len() * n_windows;
    println!("Min data: {} bars, {} windows, {} runs", min_len, n_windows, n_runs);

    // Equity CSV header
    let mut eq_header = String::from("universe,window,bar");
    for i in 0..N_VARIANTS {
        eq_header.push_str(&format!(",equity_{}", VARIANTS_LABELS[i]));
    }

    let mut eq_file = File::create("snapshots/hyperopt_equity_timeseries.csv")?;
    writeln!(eq_file, "{}", eq_header)?;

    let mut sum_file = File::create("snapshots/hyperopt_equity_summary.csv")?;
    writeln!(sum_file, "universe,window,param_label,threshold,sharpe,trades,return_pct,dd_pct")?;

    let mut agg_pass = [0usize; N_VARIANTS];
    let mut agg_sh   = [0.0; N_VARIANTS];
    let mut agg_ret  = [0.0; N_VARIANTS];
    let mut agg_dd   = [0.0; N_VARIANTS];

    for &(uname, symbols) in UNIVERSES {
        let syms: Vec<String> = symbols.iter().map(|&s| s.to_string()).collect();
        for win in 0..n_windows {
            let start = min_len - (n_windows - win) * TEST_BARS - TRAIN_BARS;
            let end = start + TEST_BARS + TRAIN_BARS;
            let test_start = start + TRAIN_BARS;

            let mut eq_curves = Vec::with_capacity(N_VARIANTS);
            let mut results = Vec::with_capacity(N_VARIANTS);

            for i in 0..N_VARIANTS {
                let t = VARIANTS_T[i];
                let (eq, sharpe, trades, ret, dd) = run_sim(&sym_data, &syms, test_start, end, t);
                eq_curves.push(eq);
                results.push((sharpe, trades, ret, dd));
            }

            for i in 0..N_VARIANTS {
                let label = VARIANTS_LABELS[i];
                let t = VARIANTS_T[i];
                let (sharpe, trades, ret, dd) = results[i];
                writeln!(sum_file, "{},{},{},{},{:.4f},{},{:.4f},{:.4f}",
                    uname, win, label, t, sharpe, trades, ret, dd)?;
                agg_pass[i] += if trades >= MIN_TRADES && sharpe > 0.0 { 1 } else { 0 };
                agg_sh[i] += sharpe;
                agg_ret[i] += ret;
                agg_dd[i] += dd;
            }

            let max_len = eq_curves.iter().map(|e| e.len()).fold(0, |a, b| a.max(b));
            for bar_i in 0..max_len {
                write!(eq_file, "{},{},{}", uname, win, bar_i)?;
                for eq in &eq_curves {
                    let val = eq.get(bar_i).copied().unwrap_or(1.0);
                    write!(eq_file, ",{:.6}", val)?;
                }
                writeln!(eq_file)?;
            }
        }
    }

    // Aggregate report
    println!("\n=== AGGREGATE RESULTS ===");
    for i in 0..N_VARIANTS {
        let n = n_runs;
        let pass_pct_val = (agg_pass[i] as f64) / (n as f64) * 100.0;
        let avg_s = agg_sh[i] / (n as f64);
        let avg_r = agg_ret[i] / (n as f64);
        let avg_d = agg_dd[i] / (n as f64);
        let label_str = VARIANTS_LABELS[i];
        let t_val = VARIANTS_T[i];
        let pass_cnt = agg_pass[i];
        println!("  {} T={}  pass={}/{} ({})  Sharpe={}  Ret={}  DD={}",
            label_str, t_val, pass_cnt, n, pass_pct_val, avg_s, avg_r, avg_d);
    }

    println!("\nDone in {:.1}s", now.elapsed().as_secs_f64());
    println!("Files: snapshots/hyperopt_equity_timeseries.csv, snapshots/hyperopt_equity_summary.csv");

    Ok(())
}
