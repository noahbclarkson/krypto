//! T44: REGIME_ATR_PERIOD Fine Sweep
//!
//! Hyperparameter: `REGIME_ATR_PERIOD` — the ATR lookback period used in the
//! BTC ATR percentile rank calculation for the entry gate.
//!
//! Current production default: AP=12 (never fine-swept, only coarse values tested)
//! This sweep tests the region around AP=12 at step 1-2 to determine if a tighter
//! or looser regime detection window improves robustness.
//!
//! ATR_RANK_T is fixed at 24.0 (calibrated for AP=12). T must be fixed because
//! the percentile rank distribution changes with AP, making T and AP interdependent.
//! A joint sweep would be 10×20 = 200 values × 63 windows = 12,600 runs — too slow
//! for this session. The fine single-parameter sweep around AP=12 is sufficient to
//! determine if AP=12 is the robustness optimum.
//!
//! Test harness: live_compatible_wf (Turtle-only exit, corrected semantics, 2026-05-01)
//! Fixed params: EP=21, TURTLE_ATR_P=24, TURTLE_ATR_M=2.0, HOLD_MAX=12,
//!                POSITION_CAP=3, VOL_LOOKBACK=96, REGIME_LOOKBACK=42, ATR_RANK_T=24.0

use anyhow::Result;
use krypto::data::loader::DataLoader;
use std::collections::HashMap;
use std::fs::File;
use std::io::Write;

const CANDLES: u32 = 3000;
const TRAIN_BARS: usize = 252;
const TEST_BARS: usize = 252;
const MIN_TRADES: usize = 3;

// Fixed production params (NOT being swept)
const TURTLE_ENTRY: usize = 21;
const TURTLE_ATR_PERIOD: usize = 24;
const TURTLE_ATR_MULT: f64 = 2.00;
const ATR_ENTRY_MULT: f64 = 0.00;
const HOLD_MAX: usize = 12;
const POSITION_CAP: usize = 3;
const VOL_LOOKBACK: usize = 96;
const REGIME_LOOKBACK: usize = 42;
const ATR_RANK_T: f64 = 24.0; // Fixed — calibrated for AP=12
const TAKER_FEE: f64 = 0.001;

// Sweep parameter values
const AP_VALUES: &[usize] = &[6, 8, 10, 11, 12, 13, 14, 16, 18, 20];

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

fn turtle_signal(
    close: &[f64], high: &[f64], low: &[f64],
    entry_period: usize, atr_period: usize, atr_mult: f64, idx: usize,
) -> bool {
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

fn btc_atr_pct(
    btc_data: &SymData,
    period: usize,
    lookback: usize,
    idx: usize,
) -> f64 {
    if idx < period.max(lookback) { return 50.0; }
    let curr_atr = atr_at(&btc_data.high, &btc_data.low, &btc_data.close, period, idx);
    if curr_atr <= 0.0 { return 50.0; }
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

struct WfResult {
    equity: f64,
    sharpe: f64,
    dd: f64,
    trades: usize,
}

fn run_sim(
    sym_data: &HashMap<String, SymData>,
    symbols: &[String],
    regime_atr_period: usize,
    test_start: usize,
    test_end: usize,
) -> WfResult {
    let mut equity = 1.0_f64;
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

                        // USDT hedge overlay
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
                            let tr = (sd.high[b] - sd.low[b])
                                .max((sd.high[b] - c0).abs())
                                .max((sd.low[b] - c0).abs());
                            atr_buf.push_back(tr);
                            if atr_buf.len() > TURTLE_ATR_PERIOD { atr_buf.pop_front(); }

                            if atr_buf.len() == TURTLE_ATR_PERIOD {
                                let atr_val = atr_buf.iter().sum::<f64>() / TURTLE_ATR_PERIOD as f64;
                                let turtle_stop = highest_high - TURTLE_ATR_MULT * atr_val;
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

    WfResult {
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

    let all_symbols: std::collections::HashSet<_> =
        UNIVERSES.iter().flat_map(|(_, s)| s.iter().copied()).collect();
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
    println!("Test windows: {}", windows);
    if windows == 0 { return Ok(()); }

    // -------------------------------------------------------------------------
    // SWEEP: collect per-AP results
    // -------------------------------------------------------------------------
    let mut summary_rows: Vec<Vec<String>> = Vec::new();
    // header
    summary_rows.push(vec![
        "ap".to_string(),
        "global_pass".to_string(),
        "global_total".to_string(),
        "global_pass_pct".to_string(),
        "avg_sharpe".to_string(),
        "avg_ret_pct".to_string(),
        "avg_dd_pct".to_string(),
        "total_trades".to_string(),
        "base5_pass".to_string(),
        "base5_total".to_string(),
        "base5_sharpe".to_string(),
        "positive_universes".to_string(),
    ]);

    // Per-AP per-universe detailed results
    let mut detail_rows: Vec<Vec<String>> = Vec::new();
    detail_rows.push(vec![
        "ap".to_string(), "universe".to_string(), "window".to_string(),
        "equity".to_string(), "sharpe".to_string(), "dd_pct".to_string(), "trades".to_string(),
    ]);

    // Equity curves for selected APs (baseline AP=12, winner, runner-ups)
    let mut equity_windows: Vec<(usize, Vec<f64>)> = Vec::new(); // (AP, window equities)

    for &ap in AP_VALUES {
        println!("\n=== AP={} ===", ap);

        let mut global_passes = 0usize;
        let mut global_total = 0usize;
        let mut global_sharpes = vec![];
        let mut global_rets = vec![];
        let mut base5_passes = 0usize;
        let mut base5_sharpes = vec![];
        let mut total_trades_all = 0usize;
        let mut positive_universes = 0usize;

        let mut window_equities: Vec<f64> = Vec::new();

        for (u_idx, (u_name, u_syms)) in UNIVERSES.iter().enumerate() {
            let syms: Vec<String> = u_syms.iter().map(|&s| s.to_string()).collect();
            let mut u_passes = 0usize;
            let mut u_sharpes = vec![];
            let mut u_rets = vec![];
            let mut u_trades = vec![];

            for w in 0..windows {
                let start = min_len - (windows - w) * TEST_BARS - TRAIN_BARS;
                let end = start + TEST_BARS + TRAIN_BARS;
                let res = run_sim(&sym_data, &syms, ap, start + TRAIN_BARS, end);

                let passed = res.trades >= MIN_TRADES && res.sharpe > 0.0;
                if passed { u_passes += 1; }
                global_total += 1;
                if passed { global_passes += 1; }

                global_sharpes.push(res.sharpe);
                u_sharpes.push(res.sharpe);
                u_rets.push((res.equity - 1.0) * 100.0);
                u_trades.push(res.trades);
                total_trades_all += res.trades;

                // Detail row
                detail_rows.push(vec![
                    ap.to_string(),
                    u_name.to_string(),
                    w.to_string(),
                    format!("{:.6}", res.equity),
                    format!("{:.4}", res.sharpe),
                    format!("{:.2}", res.dd),
                    res.trades.to_string(),
                ]);

                if u_idx == 0 {
                    window_equities.push(res.equity);
                }
            }

            let n = windows as f64;
            let avg_sharpe = u_sharpes.iter().sum::<f64>() / n;
            let avg_ret = u_rets.iter().sum::<f64>() / n;
            let pos = u_sharpes.iter().filter(|&&s| s > 0.0).count();
            if pos == windows { positive_universes += 1; }
            if u_idx == 0 { base5_passes = u_passes; }
            if u_idx == 0 { base5_sharpes = u_sharpes.clone(); }

            println!(
                "  {}: {}/{} pass, Sharpe {:.2}, Ret {:.1}%, Trades {}",
                u_name, u_passes, windows,
                avg_sharpe, avg_ret,
                u_trades.iter().sum::<usize>()
            );
        }

        let n_total = global_total as f64;
        let avg_sharpe = global_sharpes.iter().sum::<f64>() / n_total;
        let avg_ret = global_rets.iter().sum::<f64>() / n_total;
        let base5_avg_sharpe = base5_sharpes.iter().sum::<f64>() / windows as f64;

        println!(
            "  GLOBAL: {}/{} pass ({:.1}%), Sharpe {:.3}, Ret {:.1}%, Trades {}, +Uni {}",
            global_passes, global_total,
            (global_passes as f64 / n_total) * 100.0,
            avg_sharpe, avg_ret, total_trades_all, positive_universes
        );

        summary_rows.push(vec![
            ap.to_string(),
            global_passes.to_string(),
            global_total.to_string(),
            format!("{:.1}", (global_passes as f64 / n_total) * 100.0),
            format!("{:.4}", avg_sharpe),
            format!("{:.2}", avg_ret),
            "0.00".to_string(), // dd not tracked globally here
            total_trades_all.to_string(),
            base5_passes.to_string(),
            windows.to_string(),
            format!("{:.4}", base5_avg_sharpe),
            positive_universes.to_string(),
        ]);

        equity_windows.push((ap, window_equities));
    }

    // -------------------------------------------------------------------------
    // Write sweep summary CSV
    // -------------------------------------------------------------------------
    let summary_path = "snapshots/regime_ap_sweep_summary.csv";
    let mut f = File::create(summary_path)?;
    for row in &summary_rows {
        writeln!(f, "{}", row.join(","))?;
    }
    println!("\nWrote {}", summary_path);

    let detail_path = "snapshots/regime_ap_sweep_detail.csv";
    let mut f = File::create(detail_path)?;
    for row in &detail_rows {
        writeln!(f, "{}", row.join(","))?;
    }
    println!("Wrote {}", detail_path);

    // -------------------------------------------------------------------------
    // Export equity curves for baseline (AP=12) + winner + runner-ups
    // -------------------------------------------------------------------------
    // Compute winner and runner-ups by global pass rate then Sharpe
    let mut ap_results: Vec<(usize, usize, f64)> = AP_VALUES.iter().copied()
        .map(|ap| {
            let row = summary_rows.iter()
                .find(|r| r[0] == ap.to_string())
                .unwrap();
            let passes: usize = row[1].parse().unwrap();
            let sharpe: f64 = row[4].parse().unwrap();
            (ap, passes, sharpe)
        })
        .collect();
    ap_results.sort_by(|a, b| {
        b.1.cmp(&a.1)
            .then_with(|| b.2.partial_cmp(&a.2).unwrap())
    });

    let baseline_ap = 12;
    let winner_ap = ap_results[0].0;
    let runnerup1_ap = ap_results[1].0;
    let runnerup2_ap = ap_results.get(2).map(|&(ap, _, _)| ap).unwrap_or(winner_ap);

    let selected_aps: std::collections::HashSet<usize> =
        [baseline_ap, winner_ap, runnerup1_ap, runnerup2_ap]
        .iter()
        .copied()
        .collect();

    let equity_csv_path = "snapshots/regime_ap_sweep_equity.csv";
    let mut f = File::create(equity_csv_path)?;
    writeln!(f, "window,{}", selected_aps.iter().map(|&ap| format!("ap{}", ap)).collect::<Vec<_>>().join(","))?;

    // Get window count
    let n_windows = equity_windows[0].1.len();
    for w in 0..n_windows {
        let mut row = vec![w.to_string()];
        for &(ap, ref equities) in &equity_windows {
            if selected_aps.contains(&ap) {
                row.push(format!("{:.6}", equities[w]));
            }
        }
        writeln!(f, "{}", row.join(","))?;
    }
    println!("Wrote equity CSV with APs: {:?}", selected_aps);

    // Also write Base5 aggregate equity (compounded across windows) for selected APs
    let agg_csv_path = "snapshots/regime_ap_sweep_aggregate_equity.csv";
    let mut f = File::create(agg_csv_path)?;
    writeln!(f, "window,{}", selected_aps.iter().map(|&ap| format!("ap{}", ap)).collect::<Vec<_>>().join(","))?;

    let mut agg_equities: HashMap<usize, f64> = selected_aps.iter().copied()
        .map(|ap| (ap, 1.0_f64))
        .collect();

    for w in 0..n_windows {
        let mut row = vec![w.to_string()];
        for &(ap, ref equities) in &equity_windows {
            if selected_aps.contains(&ap) {
                agg_equities.insert(ap, agg_equities[&ap] * equities[w]);
                row.push(format!("{:.6}", agg_equities[&ap]));
            }
        }
        writeln!(f, "{}", row.join(","))?;
    }
    println!("Wrote aggregate equity CSV");

    // -------------------------------------------------------------------------
    // Summary printout
    // -------------------------------------------------------------------------
    println!("\n=== REGIME_ATR_PERIOD SWEEP RESULTS ===");
    println!("{:<6} {:>6} {:>10} {:>10} {:>10} {:>8}",
        "AP", "Pass", "Pass%", "AvgSharpe", "Base5Sharpe", "Trades");
    for row in &summary_rows[1..] {
        println!("{:<6} {:>6} {:>10.1} {:>10.4} {:>10.4} {:>8}",
            row[0], row[1], row[3].parse::<f64>().unwrap(),
            row[4].parse::<f64>().unwrap(),
            row[10].parse::<f64>().unwrap(),
            row[7]);
    }
    println!("\nBaseline AP=12, Winner AP={}, Runner-ups: {}, {}",
        winner_ap, runnerup1_ap, runnerup2_ap);

    // Find global winner
    let best = ap_results[0];
    let baseline_row = summary_rows.iter().find(|r| r[0] == "12").unwrap();
    let best_row = summary_rows.iter().find(|r| r[0] == best.0.to_string()).unwrap();
    let delta_pass = best_row[1].parse::<usize>().unwrap() as isize
        - baseline_row[1].parse::<usize>().unwrap() as isize;
    let delta_sharpe = best_row[4].parse::<f64>().unwrap()
        - baseline_row[4].parse::<f64>().unwrap();

    println!("\nWINNER: AP={} | {} windows, Sharpe {:.4} (delta vs AP=12: {:+.3})",
        best.0, best.1, best.2, delta_sharpe);

    if delta_pass != 0 || delta_sharpe.abs() > 0.05 {
        println!("AP={} vs AP=12: {} windows, Sharpe {:+.4}",
            best.0, delta_pass, delta_sharpe);
    } else {
        println!("AP={} is statistically equivalent to AP=12 — no default change", best.0);
    }

    Ok(())
}
