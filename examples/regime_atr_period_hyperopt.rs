//! REGIME_ATR_PERIOD Hyperopt — Live Turtle-Only Path (T=24)
//!
//! Re-optimize REGIME_ATR_PERIOD (AP) under the live Turtle-only path with
//! ATR_RANK_T=24 (production default).
//!
//! Original AP=12 was found in a joint sweep on the dual Chandelier harness.
//! T was later re-optimized to 24 on the live path, but AP was NOT re-tested.
//! This harness re-tests AP∈[5..=80 step 1] × 9 universes × 7 WF windows.
//!
//! Also exports time-series equity curves for baseline (AP=12) and top candidates.
//!
//! Metrics: pass rate (primary), avg Sharpe (secondary), avg return.

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
const VOL_LOOKBACK: usize = 8;

const REGIME_LOOKBACK: usize = 42; // fixed — LB=42 was solid winner in original joint sweep
const ATR_RANK_T: f64 = 24.0;    // production default

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

struct SimResult {
    equity: f64,
    sharpe: f64,
    dd: f64,
    trades: usize,
    daily_rets: Vec<f64>,
    equity_curve: Vec<f64>,
}

// Run simulation with a specific REGIME_ATR_PERIOD
// Returns full time-series equity curve + scalars
fn run_sim(
    sym_data: &HashMap<String, SymData>,
    symbols: &[String],
    test_start: usize,
    test_end: usize,
    regime_atr_period: usize,
) -> SimResult {
    let mut equity = 1.0_f64;
    let mut equity_curve = vec![1.0_f64];
    let mut peak = equity;
    let mut total_trades = 0usize;
    let mut daily_rets = Vec::new();

    let mut bar = test_start;
    while bar + 2 < test_end {
        let btc = sym_data.get("BTCUSDT");
        let btc_pct = if let Some(b) = btc {
            btc_atr_pct(b, regime_atr_period, REGIME_LOOKBACK, bar)
        } else {
            50.0
        };

        if btc_pct < ATR_RANK_T {
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

    SimResult {
        equity,
        sharpe: annualised_sharpe(&daily_rets),
        dd: max_dd_from(&equity_curve),
        trades: total_trades,
        daily_rets,
        equity_curve,
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    let now = std::time::Instant::now();

    // AP sweep range: 5 to 80 step 1 = 76 values
    let ap_values: Vec<usize> = (5..=80).collect();
    let n_ap = ap_values.len();
    let n_universes = UNIVERSES.len();

    println!("=== REGIME_ATR_PERIOD Hyperopt (Live Turtle-Only, T=24) ===");
    println!("AP range: 5..=80 step 1 ({} values)", n_ap);
    println!("Logic: matches src/live/bot.rs exactly");
    println!("Loading data...");

    let loader = DataLoader::new(None, None);
    let mut sym_data = HashMap::new();
    let mut min_len = usize::MAX;

    let all_symbols: std::collections::HashSet<_> = UNIVERSES
        .iter().flat_map(|(_, s)| s.iter()).copied().collect();
    for &sym in &all_symbols {
        let df = loader.fetch_data(sym, "1d", CANDLES).await?;
        let close: Vec<f64> = df.column("close")?.f64()?.into_no_null_iter().collect();
        let high: Vec<f64> = df.column("high")?.f64()?.into_no_null_iter().collect();
        let low: Vec<f64> = df.column("low")?.f64()?.into_no_null_iter().collect();
        let vol: Vec<f64> = df.column("volume")?.f64()?.into_no_null_iter().collect();
        if close.len() < min_len { min_len = close.len(); }
        sym_data.insert(sym.to_string(), SymData { close, high, low, vol });
    }

    let n_windows = (min_len.saturating_sub(TRAIN_BARS)) / TEST_BARS;
    let n_total = n_universes * n_windows;
    let total_runs = n_ap * n_total;

    println!("Min data: {} bars, {} windows, {} universes", min_len, n_windows, n_universes);
    println!("Total runs: {} × {} × {} = {}", n_ap, n_universes, n_windows, total_runs);

    // Per-AP accumulators: (ap, pass_count, total_trades, sum_ret, sum_sharpe, sum_dd)
    let mut agg: Vec<(usize, usize, usize, f64, f64, f64)> =
        ap_values.iter().map(|&ap| (ap, 0, 0, 0.0, 0.0, 0.0)).collect();

    // Per-universe per-AP for per-universe breakdown
    // universe_idx × ap_idx × {pass, sharpe, ret, dd, trades}
    let mut detail_file = File::create("snapshots/regime_atr_period_sweep.csv")?;
    writeln!(detail_file, "universe,window,ap,return_pct,sharpe,max_dd_pct,trades,pass")?;

    let mut run_count = 0usize;

    for (ui, &(uname, symbols)) in UNIVERSES.iter().enumerate() {
        let syms: Vec<String> = symbols.iter().map(|&s| s.to_string()).collect();
        for win in 0..n_windows {
            let start = min_len - (n_windows - win) * TEST_BARS - TRAIN_BARS;
            let end = start + TEST_BARS + TRAIN_BARS;

            for (ti, &ap) in ap_values.iter().enumerate() {
                let res = run_sim(&sym_data, &syms, start + TRAIN_BARS, end, ap);
                let pass = if res.trades >= MIN_TRADES && res.sharpe > 0.0 { 1 } else { 0 };

                writeln!(detail_file, "{},{},{},{:.4},{:.6},{:.4},{},{}",
                    uname, win, ap, (res.equity - 1.0) * 100.0, res.sharpe, res.dd, res.trades, pass)?;

                agg[ti].1 += pass;
                agg[ti].2 += res.trades;
                agg[ti].3 += (res.equity - 1.0) * 100.0;
                agg[ti].4 += res.sharpe;
                agg[ti].5 += res.dd;
                run_count += 1;
            }
        }
    }

    // Write summary CSV
    let mut summary_file = File::create("snapshots/regime_atr_period_summary.csv")?;
    writeln!(summary_file, "ap,pass_count,total_trades,avg_return_pct,avg_sharpe,avg_dd_pct")?;
    for &(ap, pass_cnt, total_tr, sum_ret, sum_sharpe, sum_dd) in &agg {
        writeln!(summary_file, "{},{},{},{:.4},{:.6},{:.4}",
            ap, pass_cnt, total_tr, sum_ret / n_total as f64, sum_sharpe / n_total as f64, sum_dd / n_total as f64)?;
    }

    // Sort by pass desc, then sharpe desc
    let mut sorted: Vec<(usize, &(usize, usize, usize, f64, f64, f64))> =
        agg.iter().enumerate().collect();
    sorted.sort_by(|a, b| {
        b.1.1.cmp(&a.1.1)
            .then_with(|| (b.1.4 / n_total as f64).partial_cmp(&(a.1.4 / n_total as f64)).unwrap())
            .then_with(|| b.1.3.partial_cmp(&a.1.3).unwrap())
    });

    println!("\n=== TOP 20 AP VALUES ===");
    println!("{:<6} {:>6} {:>8} {:>12} {:>10} {:>10}",
        "Rank", "AP", "PASS", "AVG_RET%", "AVG_SHARPE", "AVG_DD%");
    for (rank, (ti, stats)) in sorted.iter().take(20).enumerate() {
        let ap = ap_values[*ti];
        let avg_sharpe = stats.4 / n_total as f64;
        let avg_ret = stats.3 / n_total as f64;
        let avg_dd = stats.5 / n_total as f64;
        println!("  [{:>2}] AP={:>3}: pass={:>3}/{}, sharpe={:>8.4}, ret={:>10.2}%, dd={:>7.2}%",
            rank + 1, ap, stats.1, n_total, avg_sharpe, avg_ret, avg_dd);
    }

    let (best_ap_idx, best_stats) = &sorted[0];
    let best_ap = ap_values[*best_ap_idx];
    let baseline_ap = 12_usize;
    let baseline_stats = &agg.iter().find(|(ap, _, _, _, _, _)| *ap == baseline_ap).unwrap();
    let baseline_pass = baseline_stats.1;
    let baseline_sharpe = baseline_stats.4 / n_total as f64;
    let baseline_ret = baseline_stats.3 / n_total as f64;
    let baseline_dd = baseline_stats.5 / n_total as f64;

    println!("\n>>> WINNER: AP={} — {} pass, Sharpe {:.4}, Ret {:.2}%, DD {:.2}%",
        best_ap, best_stats.1, best_stats.4 / n_total as f64,
        best_stats.3 / n_total as f64, best_stats.5 / n_total as f64);

    println!("\n>>> BASELINE: AP={} — {} pass, Sharpe {:.4}, Ret {:.2}%, DD {:.2}%",
        baseline_ap, baseline_pass, baseline_sharpe, baseline_ret, baseline_dd);

    let delta_pass = best_stats.1 as i32 - baseline_pass as i32;
    let delta_sharpe = (best_stats.4 / n_total as f64) - baseline_sharpe;
    let _ = delta_pass;
    let _ = delta_sharpe;

    // ─────────────────────────────────────────────────────────────────────────────
    // Export equity curves for top-5 APs + baseline
    // ─────────────────────────────────────────────────────────────────────────────
    let top5_aps: Vec<usize> = sorted.iter().take(5).map(|(ti, _)| ap_values[*ti]).collect();
    let baseline_ap_usize: usize = baseline_ap;

    // Per-universe per-window equity curves
    let mut eq_csv = File::create("snapshots/regime_ap_equity_detail.csv")?;
    writeln!(eq_csv, "universe,window,bar_idx,equity")?;

    for &(uname, symbols) in UNIVERSES {
        let syms: Vec<String> = symbols.iter().map(|&s| s.to_string()).collect();

        // Build equity curve for each top-5 AP + baseline, per window
        // We store (window, bar_idx, equity_for_apN) per row
        for win in 0..n_windows {
            let start = min_len - (n_windows - win) * TEST_BARS - TRAIN_BARS;
            let end = start + TEST_BARS + TRAIN_BARS;

            // Baseline (AP=12)
            let base_res = run_sim(&sym_data, &syms, start + TRAIN_BARS, end, baseline_ap_usize);
            for (bi, &eq) in base_res.equity_curve.iter().enumerate() {
                writeln!(eq_csv, "{},{},{},{:.6}", uname, win, bi, eq)?;
            }

            // Top-5 candidates
            for (_ci, &ap) in top5_aps.iter().enumerate() {
                let res = run_sim(&sym_data, &syms, start + TRAIN_BARS, end, ap);
                for (bi, &eq) in res.equity_curve.iter().enumerate() {
                    writeln!(eq_csv, "{},{},{},{:.6}", uname, win, bi, eq)?;
                }
            }
        }
    }

    // Write a simple header for the equity comparison CSV
    let mut eq_compact = File::create("snapshots/regime_ap_equity_compact.csv")?;
    writeln!(eq_compact, "universe,window,bar_idx,equity_AP{}", baseline_ap)?;
    for ap in &top5_aps {
        write!(eq_compact, ",equity_AP{}", ap)?;
    }
    writeln!(eq_compact)?;

    // Per-window per-bar equity
    for &(uname, symbols) in UNIVERSES {
        let syms: Vec<String> = symbols.iter().map(|&s| s.to_string()).collect();
        for win in 0..n_windows {
            let start = min_len - (n_windows - win) * TEST_BARS - TRAIN_BARS;
            let end = start + TEST_BARS + TRAIN_BARS;

            // Get all equity curves
            let all_res: Vec<SimResult> = std::iter::once(baseline_ap_usize)
                .chain(top5_aps.iter().copied())
                .map(|ap| run_sim(&sym_data, &syms, start + TRAIN_BARS, end, ap))
                .collect();

            let max_len = all_res.iter().map(|r| r.equity_curve.len()).max().unwrap_or(0);
            for bi in 0..max_len {
                write!(eq_compact, "{},{},{}", uname, win, bi)?;
                for res in &all_res {
                    let eq = res.equity_curve.get(bi).copied().unwrap_or(1.0);
                    write!(eq_compact, ",{:.6}", eq)?;
                }
                writeln!(eq_compact)?;
            }
        }
    }

    // Per-universe pass breakdown for winner vs baseline
    {
        let mut peruni_file = File::create("snapshots/regime_ap_per_universe.csv")?;
        writeln!(peruni_file, "universe,ap,pass_count,total,pass_rate,avg_sharpe,avg_ret")?;

        for &(uname, symbols) in UNIVERSES {
            let syms: Vec<String> = symbols.iter().map(|&s| s.to_string()).collect();

            // Baseline
            let mut base_pass = 0usize;
            let mut base_sharpes = vec![];
            let mut base_rets = vec![];
            for win in 0..n_windows {
                let start = min_len - (n_windows - win) * TEST_BARS - TRAIN_BARS;
                let end = start + TEST_BARS + TRAIN_BARS;
                let res = run_sim(&sym_data, &syms, start + TRAIN_BARS, end, baseline_ap_usize);
                if res.trades >= MIN_TRADES && res.sharpe > 0.0 { base_pass += 1; }
                base_sharpes.push(res.sharpe);
                base_rets.push((res.equity - 1.0) * 100.0);
            }
            let nw = n_windows as f64;
            writeln!(peruni_file, "{},{},{},{},{:.1}%,{:.4},{:.2}",
                uname, baseline_ap, base_pass, n_windows,
                (base_pass as f64 / nw) * 100.0,
                base_sharpes.iter().sum::<f64>() / nw,
                base_rets.iter().sum::<f64>() / nw)?;

            // Winner
            let mut win_pass = 0usize;
            let mut win_sharpes = vec![];
            let mut win_rets = vec![];
            for win in 0..n_windows {
                let start = min_len - (n_windows - win) * TEST_BARS - TRAIN_BARS;
                let end = start + TEST_BARS + TRAIN_BARS;
                let res = run_sim(&sym_data, &syms, start + TRAIN_BARS, end, best_ap);
                if res.trades >= MIN_TRADES && res.sharpe > 0.0 { win_pass += 1; }
                win_sharpes.push(res.sharpe);
                win_rets.push((res.equity - 1.0) * 100.0);
            }
            let nw = n_windows as f64;
            writeln!(peruni_file, "{},{},{},{},{:.1}%,{:.4},{:.2}",
                uname, best_ap, win_pass, n_windows,
                (win_pass as f64 / nw) * 100.0,
                win_sharpes.iter().sum::<f64>() / nw,
                win_rets.iter().sum::<f64>() / nw)?;
        }
    }

    println!("\nDone in {:.1}s ({} runs, {:.0}/sec)",
        now.elapsed().as_secs_f64(), run_count, run_count as f64 / now.elapsed().as_secs_f64());
    println!("Files:");
    println!("  snapshots/regime_atr_period_sweep.csv      — per-window detail ({} rows)", run_count);
    println!("  snapshots/regime_atr_period_summary.csv    — aggregated by AP ({} rows)", n_ap);
    println!("  snapshots/regime_ap_equity_compact.csv     — equity curves (top-5 + baseline)");
    println!("  snapshots/regime_ap_per_universe.csv      — per-universe comparison");

    // Write winner to a simple text file for easy parsing
    let mut winner_file = File::create("snapshots/regime_ap_winner.txt")?;
    writeln!(winner_file, "{}", best_ap)?;
    let mut runnerups_file = File::create("snapshots/regime_ap_runnerups.txt")?;
    for (ti, _) in sorted.iter().skip(1).take(4) {
        writeln!(runnerups_file, "{}", ap_values[*ti])?;
    }

    Ok(())
}
