//! T69 LB Sweep: REGIME_LOOKBACK extensive step-1 optimization
//!
//! REGIME_LOOKBACK was set to 42 via coarse step-5 sweep (LB=5,10,15,...,200).
//! Never tested step-1 dense across the full integer range [5..=200].
//! This is the assumption-removal mission for this session.
//!
//! Outputs:
//! - snapshots/lb_sweep_summary.csv   — pass/Sharpe/Ret/DD per LB value
//! - snapshots/lb_sweep_timeseries.csv — per-window equity per LB value (for charting)
//! - charts/lb_comparison_chart.png    — log-scale equity curves

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
const VOL_LOOKBACK: usize = 92;
const REGIME_ATR_PERIOD: usize = 17;
const ATR_RANK_T: f64 = 5.0;
const HEDGE_ATR_PCT: f64 = 0.45;
const HEDGE_SIZE_MULT: f64 = 0.40;

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
    let mut peak = 1.0_f64;
    for &val in equity {
        if val > peak { peak = val; }
        let dd = 1.0 - val / peak;
        if dd > max_dd { max_dd = dd; }
    }
    max_dd * 100.0
}

struct SimResult {
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
    regime_lookback: usize,
) -> SimResult {
    let mut equity = 1.0_f64;
    let mut peak = equity;
    let mut total_trades = 0usize;
    let mut daily_rets = Vec::new();

    let mut bar = test_start;
    while bar + 2 < test_end {
        let btc = sym_data.get("BTCUSDT");
        let btc_pct = if let Some(b) = btc {
            btc_atr_pct(b, REGIME_ATR_PERIOD, regime_lookback, bar)
        } else {
            50.0
        };

        if btc_pct < ATR_RANK_T {
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
            bar += 1;
            continue;
        }

        let mut entered = false;
        for sym in &top_syms {
            if let Some(sd) = sym_data.get(sym) {
                if bar >= TURTLE_ENTRY + 1 && bar < sd.close.len() {
                    // turtle breakout check
                    let start = bar - TURTLE_ENTRY;
                    let max_close = sd.close[start..bar].iter().fold(f64::NEG_INFINITY, |a, &b| a.max(b));
                    if sd.close[bar] > max_close {
                        if ATR_ENTRY_MULT > 0.0 {
                            let atr = atr_at(&sd.high, &sd.low, &sd.close, TURTLE_ATR_PERIOD, bar);
                            if sd.close[bar] < max_close + atr * ATR_ENTRY_MULT {
                                continue;
                            }
                        }

                        let entry_px = sd.close[bar];
                        let mut size_mult = 1.0;
                        if let Some(b) = sym_data.get("BTCUSDT") {
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
                                let pct_idx = (HEDGE_ATR_PCT * hist.len() as f64) as usize;
                                let pct_threshold = hist[pct_idx.min(hist.len().saturating_sub(1))];
                                if atr_21 > pct_threshold {
                                    size_mult = HEDGE_SIZE_MULT;
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
                            bar = exit_bar + 1;
                            entered = true;
                            break;
                        }
                    }
                }
            }
        }
        if !entered {
            bar += 1;
        }
    }

    SimResult {
        equity,
        sharpe: annualised_sharpe(&daily_rets),
        dd: max_dd_from(&[1.0, equity]),
        trades: total_trades,
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    println!("Loading data...");
    let loader = DataLoader::new(None, None);
    let mut sym_data = HashMap::new();
    let mut min_len = usize::MAX;

    let all_symbols = UNIVERSES.iter()
        .flat_map(|(_, s)| s.iter())
        .collect::<std::collections::HashSet<_>>();
    for &sym in &all_symbols {
        let df = loader.fetch_data(sym, "1d", CANDLES).await?;
        let close = df.column("close")?.f64()?.into_no_null_iter().collect::<Vec<_>>();
        let high = df.column("high")?.f64()?.into_no_null_iter().collect::<Vec<_>>();
        let low = df.column("low")?.f64()?.into_no_null_iter().collect::<Vec<_>>();
        let vol = df.column("volume")?.f64()?.into_no_null_iter().collect::<Vec<_>>();
        if close.len() < min_len { min_len = close.len(); }
        sym_data.insert(sym.to_string(), SymData { close, high, low, vol });
    }

    let windows = (min_len.saturating_sub(TRAIN_BARS)) / TEST_BARS;
    if windows == 0 {
        println!("Not enough data for walk-forward");
        return Ok(());
    }
    println!("Data loaded. {} bars, {} walk-forward windows", min_len, windows);

    // Extensive LB range: step 1 from 5 to 200
    let lb_values: Vec<usize> = (5..=200).collect();
    let n_lb = lb_values.len();
    println!("Sweeping LB ∈ [5..=200] step 1 ({} values) × {} universes × {} windows",
        n_lb, UNIVERSES.len(), windows);

    // Per-LB accumulators
    let mut lb_passes: Vec<usize> = vec![0; n_lb];
    let mut lb_sharpes: Vec<f64> = vec![0.0; n_lb];
    let mut lb_rets: Vec<f64> = vec![0.0; n_lb];
    let mut lb_dds: Vec<f64> = vec![0.0; n_lb];
    let mut lb_trades: Vec<usize> = vec![0; n_lb];
    let mut lb_pos_universes: Vec<usize> = vec![0; n_lb];

    // Per-window equity per LB (for charting Base5 aggregated equity across windows)
    // We'll store Base5 compound equity across windows
    let mut lb_base5_compound: Vec<f64> = vec![1.0; n_lb];

    // Also track Base5 per-window equity for each LB
    let mut lb_base5_window_equity: Vec<Vec<f64>> = vec![vec![1.0; windows]; n_lb];

    for (lb_idx, &lb) in lb_values.iter().enumerate() {
        for (u_name, u_syms) in UNIVERSES {
            let syms: Vec<String> = u_syms.iter().map(|&s| s.to_string()).collect();
            let mut u_sharpes = vec![];
            let mut u_rets = vec![];
            let mut u_dds = vec![];
            let mut u_trades = vec![];
            let mut u_pos = 0;

            for w in 0..windows {
                let start = min_len - (windows - w) * TEST_BARS - TRAIN_BARS;
                let end = start + TEST_BARS + TRAIN_BARS;
                let res = run_sim(&sym_data, &syms, start + TRAIN_BARS, end, lb);

                u_sharpes.push(res.sharpe);
                u_rets.push((res.equity - 1.0) * 100.0);
                u_dds.push(res.dd);
                u_trades.push(res.trades);
                if res.trades >= MIN_TRADES && res.sharpe > 0.0 {
                    u_pos += 1;
                    lb_passes[lb_idx] += 1;
                }
                lb_sharpes[lb_idx] += res.sharpe;
                lb_rets[lb_idx] += (res.equity - 1.0) * 100.0;
                lb_dds[lb_idx] += res.dd;
                lb_trades[lb_idx] += res.trades;

                if *u_name == "Base5" {
                    lb_base5_compound[lb_idx] *= res.equity;
                    lb_base5_window_equity[lb_idx][w] = res.equity;
                }
            }

            if u_pos > 0 {
                lb_pos_universes[lb_idx] += 1;
            }
        }
    }

    let total_runs = UNIVERSES.len() * windows;
    let baseline_lb_idx = lb_values.iter().position(|&v| v == 42).unwrap();

    // Write summary CSV
    {
        let mut f = File::create("snapshots/lb_sweep_summary.csv")?;
        writeln!(f, "lb,pass,pass_pct,avg_sharpe,avg_ret,avg_dd,total_trades,pos_universes,base5_equity")?;
        for i in 0..n_lb {
            let pass_pct = (lb_passes[i] as f64 / total_runs as f64) * 100.0;
            let n = total_runs as f64;
            writeln!(f, "{},{},{:.2},{:.4},{:.4},{:.4},{},{},{:.6}",
                lb_values[i], lb_passes[i], pass_pct,
                lb_sharpes[i] / n,
                lb_rets[i] / n,
                lb_dds[i] / n,
                lb_trades[i],
                lb_pos_universes[i],
                lb_base5_compound[i])?;
        }
    }

    // Write time-series CSV for charting
    {
        // Header: window, LB=5, LB=10, ... LB=200
        let mut f = File::create("snapshots/lb_sweep_timeseries.csv")?;
        write!(f, "window")?;
        // Sample every 5th LB for readability + the baseline
        let sample_lbs: Vec<usize> = (5..=200).step_by(5).collect();
        for &slb in &sample_lbs {
            write!(f, ",lb_{}", slb)?;
        }
        // Always include baseline LB=42 and best-performing LB
        writeln!(f)?;

        // Find best LB by Sharpe
        let mut best_sharpe_idx = 0;
        let mut best_sharpe = f64::NEG_INFINITY;
        for i in 0..n_lb {
            let s = lb_sharpes[i] / total_runs as f64;
            if s > best_sharpe {
                best_sharpe = s;
                best_sharpe_idx = i;
            }
        }
        let best_lb = lb_values[best_sharpe_idx];

        // Add best_sharpe and baseline to sample list if not already there
        let mut chart_lbs: Vec<usize> = sample_lbs.clone();
        if !chart_lbs.contains(&42) { chart_lbs.push(42); }
        if !chart_lbs.contains(&best_lb) { chart_lbs.push(best_lb); }
        chart_lbs.sort();

        // Rebuild header
        let mut f = File::create("snapshots/lb_sweep_timeseries.csv")?;
        write!(f, "window")?;
        for &slb in &chart_lbs {
            write!(f, ",lb_{}", slb)?;
        }
        writeln!(f)?;

        for w in 0..windows {
            write!(f, "{}", w)?;
            for &slb in &chart_lbs {
                let lb_idx = lb_values.iter().position(|&v| v == slb).unwrap();
                write!(f, ",{:.6}", lb_base5_window_equity[lb_idx][w])?;
            }
            writeln!(f)?;
        }

        // Also save chart_lbs for Python script
        let mut cf = File::create("snapshots/lb_chart_lbs.txt")?;
        for &slb in &chart_lbs {
            writeln!(cf, "{}", slb)?;
        }

        // Also write base5 compound equity per LB to a separate CSV
        let mut cf2 = File::create("snapshots/lb_sweep_base5_compound.csv")?;
        writeln!(cf2, "lb,compound_equity")?;
        for i in 0..n_lb {
            writeln!(cf2, "{},{:.6}", lb_values[i], lb_base5_compound[i])?;
        }
    }

    let n = total_runs as f64;
    let baseline_pass = lb_passes[baseline_lb_idx];
    let baseline_sharpe = lb_sharpes[baseline_lb_idx] / n;
    let best_lb = lb_values[lb_sharpes.iter()
        .enumerate()
        .max_by(|(_, a), (_, b)| a.partial_cmp(b).unwrap())
        .unwrap().0];
    let best_lb_idx = lb_values.iter().position(|&v| v == best_lb).unwrap();
    let best_sharpe_val = lb_sharpes[best_lb_idx] / n;
    let best_pass = lb_passes[best_lb_idx];
    let best_equity = lb_base5_compound[best_lb_idx];

    println!("\n=== REGIME_LOOKBACK Sweep Results ===");
    println!("Baseline LB=42: {} pass ({:.1}%), Sharpe {:.4}", baseline_pass, (baseline_pass as f64/total_runs as f64)*100.0, baseline_sharpe);
    println!("Best LB={}: {} pass ({:.1}%), Sharpe {:.4}, Base5 equity {:.2}x",
        best_lb, best_pass, (best_pass as f64/total_runs as f64)*100.0, best_sharpe_val, best_equity);
    println!("Winner: LB={}", best_lb);

    // Save winner info
    {
        let mut f = File::create("snapshots/lb_sweep_winner.txt")?;
        writeln!(f, "{}", best_lb)?;
    }

    Ok(())
}