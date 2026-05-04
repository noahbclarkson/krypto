//! T62: AP (REGIME_ATR_PERIOD) Held-Out Validation
//!
//! Tests top-5 AP candidates from the 2026-05-04 OOS sweep against PRE-2021 data.
//! AP=63 won the OOS sweep (56/63 pass vs AP=12's 55/63) but was promoted WITHOUT
//! held-out validation — same anti-overfit pattern as EP=24 (reverted 2026-04-26).
//!
//! Decision rule:
//! - AP=12 wins held-out → REVERT config.rs, AP=12 remains default
//! - AP=63 wins held-out → keep AP=63 in config.rs
//!
//! Data: last 50% of each symbol's available history = held-out (never in any sweep)

use anyhow::Result;
use krypto::data::loader::DataLoader;

use std::collections::HashMap;
use std::fs::File;
use std::io::Write;

const CANDLES: u32 = 4000;
const MIN_TRADES: usize = 3;
const TAKER_FEE: f64 = 0.001;

const TURTLE_ENTRY: usize = 21;
const TURTLE_ATR_PERIOD: usize = 24;
const TURTLE_ATR_MULT: f64 = 2.0;
const ATR_ENTRY_MULT: f64 = 0.00;
const HOLD_MAX: usize = 12;
const POSITION_CAP: usize = 3;
const VOL_LOOKBACK: usize = 96;
const REGIME_LOOKBACK: usize = 42;
const ATR_RANK_T: f64 = 5.0;

const TEST_APS: &[usize] = &[7, 12, 17, 37, 63];

const SYMBOLS: &[&str] = &[
    "BTCUSDT", "ETHUSDT", "SOLUSDT", "XRPUSDT", "DOGEUSDT",
    "ADAUSDT", "LTCUSDT", "BNBUSDT", "EOSUSDT", "BCHUSDT",
];

const PERIODS: &[(&str, usize)] = &[
    ("P1-2019", 0),
    ("P2-2019", 180),
    ("P3-2020", 365),
    ("P4-2020", 545),
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

fn btc_atr_pct(btc: &SymData, period: usize, lookback: usize, idx: usize) -> f64 {
    if idx < period.max(lookback) { return 50.0; }
    let curr_atr = atr_at(&btc.high, &btc.low, &btc.close, period, idx);
    let mut hist = Vec::with_capacity(lookback);
    for j in (idx + 1 - lookback)..=idx {
        if j >= period {
            hist.push(atr_at(&btc.high, &btc.low, &btc.close, period, j));
        }
    }
    if hist.is_empty() { return 50.0; }
    let count = hist.iter().filter(|&&x| x < curr_atr).count();
    (count as f64 / hist.len() as f64) * 100.0
}

fn turtle_signal(close: &[f64], ep: usize, idx: usize) -> bool {
    if idx < ep { return false; }
    let max_close = close[idx - ep..idx].iter().fold(f64::NEG_INFINITY, |a, &b| a.max(b));
    close[idx] > max_close
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

fn run_sim(
    sym_data: &HashMap<String, SymData>,
    test_start: usize,
    test_end: usize,
    ap: usize,
) -> (f64, f64, f64, usize) {
    let mut equity = 1.0_f64;
    let mut equity_curve = vec![1.0_f64];
    let mut peak = equity;
    let mut total_trades = 0usize;
    let mut daily_rets = Vec::new();

    let mut bar = test_start;
    while bar + 2 < test_end {
        let btc = sym_data.get("BTCUSDT");
        let btc_pct = if let Some(b) = btc {
            btc_atr_pct(b, ap, REGIME_LOOKBACK, bar)
        } else {
            50.0
        };

        if btc_pct < ATR_RANK_T {
            equity_curve.push(equity);
            bar += 1;
            continue;
        }

        let mut scores: Vec<(&str, f64)> = Vec::new();
        for (sym, sd) in sym_data {
            if bar >= sd.close.len() { continue; }
            let rol_vol = rolling_avg(&sd.vol, VOL_LOOKBACK, bar);
            let price = sd.close.get(bar).copied().unwrap_or(0.0);
            let dv = rol_vol * price;
            scores.push((sym.as_str(), if dv.is_finite() && dv > 0.0 { dv } else { 0.0 }));
        }
        scores.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());
        let top_syms: Vec<&str> = scores.into_iter().take(POSITION_CAP).map(|(s, _)| s).collect();

        if top_syms.is_empty() {
            equity_curve.push(equity);
            bar += 1;
            continue;
        }

        let mut entered = false;
        for sym in &top_syms {
            if let Some(sd) = sym_data.get(*sym) {
                if bar >= TURTLE_ENTRY + 1 && bar < sd.close.len() {
                    if turtle_signal(&sd.close, TURTLE_ENTRY, bar) {
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

    (equity, annualised_sharpe(&daily_rets), max_dd_from(&equity_curve), total_trades)
}

#[tokio::main]
async fn main() -> Result<()> {
    println!("Loading data for AP held-out validation...");
    let loader = DataLoader::new(None, None);
    let mut sym_data = HashMap::new();

    for &sym in SYMBOLS {
        match loader.fetch_data(sym, "1d", CANDLES).await {
            Ok(df) => {
                let close = df.column("close")?.f64()?.into_no_null_iter().collect::<Vec<_>>();
                let high = df.column("high")?.f64()?.into_no_null_iter().collect::<Vec<_>>();
                let low = df.column("low")?.f64()?.into_no_null_iter().collect::<Vec<_>>();
                let vol = df.column("volume")?.f64()?.into_no_null_iter().collect::<Vec<_>>();
                sym_data.insert(sym.to_string(), SymData { close, high, low, vol });
                println!("  {}: {} bars", sym, sym_data[sym].close.len());
            }
            Err(e) => {
                println!("  Skipping {}: {}", sym, e);
            }
        }
    }

    let min_len = sym_data.values()
        .map(|sd| sd.close.len())
        .min()
        .unwrap_or(0);

    if min_len < 400 {
        println!("Insufficient data: {} bars", min_len);
        return Ok(());
    }

    // Held-out split: use last 50% of data (the data the sweep never touched)
    let held_out_start = min_len / 2;
    let held_out_len = min_len - held_out_start;
    let n_periods = 4;
    let period_len = held_out_len / n_periods;

    println!("\nMin data: {} bars. Held-out: {} bars from idx {}", min_len, held_out_len, held_out_start);
    println!("Period length: {} bars, {} periods", period_len, n_periods);

    let csv_path = "snapshots/ap_held_out_comprehensive.csv";
    let mut csv_file = File::create(csv_path)?;
    writeln!(csv_file, "period,ap,equity,sharpe,dd,trades,passed")?;

    let mut period_equities: HashMap<usize, Vec<f64>> = HashMap::new();

    for (period_name, period_offset) in PERIODS {
        println!("\n--- {} ---", period_name);
        let test_start = held_out_start + period_offset;
        let test_end = (test_start + period_len).min(min_len.saturating_sub(1));

        if test_end <= test_start + MIN_TRADES + 10 {
            println!("  Skipping {}: insufficient test bars", period_name);
            continue;
        }

        for &ap in TEST_APS {
            let (equity, sharpe, dd, trades) = run_sim(&sym_data, test_start, test_end, ap);
            let passed = if trades >= MIN_TRADES && sharpe > 0.0 { 1 } else { 0 };
            println!("  AP={:2}: equity={:8.4}, Sharpe={:7.3}, DD={:5.1}%, trades={:4}, pass={}",
                     ap, equity, sharpe, dd, trades, passed);

            writeln!(csv_file, "{},AP={},{},{},{},{},{}", period_name, ap, equity, sharpe, dd, trades, passed)?;
            period_equities.entry(ap).or_default().push(equity);
        }
    }

    // Compute per-AP aggregate across periods
    println!("\n=== Held-Out Summary ===");
    let summary_path = "snapshots/ap_held_out_comprehensive_summary.csv";
    let mut summary_file = File::create(summary_path)?;
    writeln!(summary_file, "ap,total_pass,total_tests,pass_pct,avg_equity_product,avg_sharpe,avg_dd,total_trades")?;

    let mut best_ap = 0usize;
    let mut best_pass_pct = 0.0f64;

    for &ap in TEST_APS {
        let equities = period_equities.get(&ap).cloned().unwrap_or_default();
        let n = equities.len();
        if n == 0 { continue; }

        // Read back CSV to compute aggregate
        let content = std::fs::read_to_string(csv_path)?;
        let mut pass_count = 0usize;
        let mut total_sharpe = 0.0f64;
        let mut total_dd = 0.0f64;
        let mut total_trades = 0usize;

        for line in content.lines().skip(1) {
            let parts: Vec<&str> = line.split(',').collect();
            if parts.len() >= 7 && parts[1] == format!("AP={}", ap) {
                let trades: usize = parts[5].parse().unwrap_or(0);
                let sharpe: f64 = parts[3].parse().unwrap_or(0.0);
                let dd: f64 = parts[4].parse().unwrap_or(0.0);
                if trades >= MIN_TRADES && sharpe > 0.0 { pass_count += 1; }
                total_sharpe += sharpe;
                total_dd += dd;
                total_trades += trades;
            }
        }

        let pass_pct = pass_count as f64 / n as f64;
        let avg_equity_product = equities.iter().product::<f64>().powf(1.0 / n as f64);
        let avg_sharpe = total_sharpe / n as f64;
        let avg_dd = total_dd / n as f64;

        println!("AP={:2}: {}/{} pass ({:.1}%), equity={:.4}x, Sharpe={:.3}, DD={:.1}%, trades={}",
                 ap, pass_count, n, pass_pct * 100.0, avg_equity_product, avg_sharpe, avg_dd, total_trades);

        writeln!(summary_file, "{},{},{},{:.4},{:.3},{:.1},{},{}",
                 ap, pass_count, n, pass_pct, avg_equity_product, avg_sharpe, avg_dd, total_trades)?;

        if pass_pct > best_pass_pct || (pass_pct == best_pass_pct && avg_sharpe > 0.0) {
            best_pass_pct = pass_pct;
            best_ap = ap;
        }
    }

    println!("\n=== Held-Out Winner ===");
    println!("Best AP: {} ({:.1}%% pass rate)", best_ap, best_pass_pct * 100.0);

    // Write markdown report
    let md_path = "snapshots/ap_held_out_comprehensive.md";
    let mut md = File::create(md_path)?;
    writeln!(md, "# AP Held-Out Validation Results")?;
    writeln!(md, "")?;
    writeln!(md, "**Date:** 2026-05-04")?;
    writeln!(md, "**Purpose:** Validate AP=63 vs AP=12 on pre-sweep held-out data")?;
    writeln!(md, "**Context:** AP=63 won OOS (56/63 vs 55/63, +1.197 Sharpe) but was promoted without held-out validation")?;
    writeln!(md, "**Anti-overfit pattern:** Same as EP=24 (3rd sequential opt, won +1 window OOS, failed held-out 25/29 vs 27/29)")?;
    writeln!(md, "")?;
    writeln!(md, "## Held-Out Summary")?;
    writeln!(md, "")?;
    writeln!(md, "| AP | Pass | Pass% | Avg Equity | Avg Sharpe | Avg DD% | Trades |")?;
    writeln!(md, "|----|------|-------|------------|------------|---------|--------|")?;

    let content = std::fs::read_to_string(summary_path)?;
    for line in content.lines().skip(1) {
        let parts: Vec<&str> = line.split(',').collect();
        if parts.len() >= 8 {
            let ap_val = parts[0];
            let p_pass = parts[1];
            let p_total = parts[2];
            let p_pct = parts[3].parse::<f64>().unwrap_or(0.0) * 100.0;
            let p_equity = parts[4].parse::<f64>().unwrap_or(1.0);
            let p_sharpe = parts[5].parse::<f64>().unwrap_or(0.0);
            let p_dd = parts[6].parse::<f64>().unwrap_or(0.0);
            writeln!(md, "| {} | {}/{} | {:.1}%% | {:.4}x | {:.3} | {:.1}%% | {} |",
                ap_val, p_pass, p_total, p_pct, p_equity, p_sharpe, p_dd, parts[7])?;
        }
    }

    writeln!(md, "")?;
    writeln!(md, "## Decision")?;
    writeln!(md, "")?;
    writeln!(md, "**Winner: AP={best_ap}** (held-out)")?;
    writeln!(md, "")?;
    if best_ap == 12 {
        writeln!(md, "AP=12 wins held-out → **REVERT** `config.rs` to AP=12. AP=63 was a same-harness artifact.")?;
    } else {
        writeln!(md, "AP={best_ap} wins held-out → keep in `config.rs`. Promotion was correct.")?;
    }

    println!("\nResults: {}", csv_path);
    println!("Summary: {}", summary_path);
    println!("Report: {}", md_path);

    Ok(())
}