//! T66: HEDGE_SIZE_MULT Extensive Sweep
//!
//! Audit finding: HEDGE_SIZE_MULT=0.70 is a hardcoded magic number never independently tested.
//! The 2026-05-05 hedge threshold sweep (PCT=0..=100) held HEDGE_SIZE_MULT=0.70 constant.
//! We need to test the full logical range of size multipliers.
//!
//! Current defaults: HEDGE_ATR_PCT=0.45, HEDGE_SIZE_MULT=0.70
//! Test range: HEDGE_SIZE_MULT ∈ {0.30, 0.40, 0.50, 0.55, 0.60, 0.65, 0.70, 0.75, 0.80, 0.85, 0.90, 0.95, 1.00}
//! Strategy: Turtle-only + ATR_RANK(AP=17, LB=42, T=5) + USDT hedge (PCT=45)
//!
//! Sweeps 13 values × 9 universes × 7 WF windows = 819 simulations.
//! Exports per-bar timeseries CSV and summary for charting.

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
const REGIME_LOOKBACK: usize = 42;
const ATR_RANK_T: f64 = 5.0;

const HEDGE_ATR_PCT: f64 = 0.45; // winner from 2026-05-05 sweep

const HEDGE_SIZE_VALS: &[f64] = &[
    0.30, 0.40, 0.50, 0.55, 0.60, 0.65,
    0.70, 0.75, 0.80, 0.85, 0.90, 0.95, 1.00,
];

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

fn turtle_signal(close: &[f64], _high: &[f64], _low: &[f64], entry_period: usize, idx: usize) -> bool {
    if idx < entry_period { return false; }
    let start = idx - entry_period;
    let max_close = close[start..idx].iter().fold(f64::NEG_INFINITY, |a, &b| a.max(b));
    close[idx] > max_close
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

fn max_dd(equity: &[f64]) -> f64 {
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
    symbols: &[String],
    test_start: usize,
    test_end: usize,
    hedge_size_mult: f64,
) -> (f64, f64, f64, usize, Vec<f64>) {
    // returns (equity, sharpe, dd, trades, equity_curve)
    let mut equity = 1.0_f64;
    let mut peak = equity;
    let mut total_trades = 0usize;
    let mut daily_rets = Vec::new();
    let mut equity_curve = vec![1.0_f64];

    let mut bar = test_start;
    while bar + 2 < test_end {
        let btc = sym_data.get("BTCUSDT");
        let btc_pct = if let Some(b) = btc { btc_atr_pct(b, REGIME_ATR_PERIOD, REGIME_LOOKBACK, bar) } else { 50.0 };

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
                    if turtle_signal(&sd.close, &sd.high, &sd.low, TURTLE_ENTRY, bar) {
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
                                let pct_idx = (HEDGE_ATR_PCT * hist.len() as f64) as usize;
                                let pct_threshold = hist[pct_idx.min(hist.len().saturating_sub(1))];
                                if atr_21 > pct_threshold {
                                    size_mult = hedge_size_mult;
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

    let dd = max_dd(&equity_curve);
    let sh = annualised_sharpe(&daily_rets);
    (equity, sh, dd, total_trades, equity_curve)
}

#[tokio::main]
async fn main() -> Result<()> {
    println!("Loading data...");
    let loader = DataLoader::new(None, None);
    let mut sym_data = HashMap::new();
    let mut min_len = usize::MAX;

    let all_symbols = UNIVERSES.iter().flat_map(|(_, s)| s.iter()).map(|&s| s.to_string()).collect::<std::collections::HashSet<_>>();
    for sym in &all_symbols {
        let df = loader.fetch_data(&sym, "1d", CANDLES).await?;
        let close = df.column("close")?.f64()?.into_no_null_iter().collect::<Vec<_>>();
        let high = df.column("high")?.f64()?.into_no_null_iter().collect::<Vec<_>>();
        let low = df.column("low")?.f64()?.into_no_null_iter().collect::<Vec<_>>();
        let vol = df.column("volume")?.f64()?.into_no_null_iter().collect::<Vec<_>>();
        if close.len() < min_len { min_len = close.len(); }
        sym_data.insert(sym.to_string(), SymData { close, high, low, vol });
    }

    let windows = (min_len.saturating_sub(TRAIN_BARS)) / TEST_BARS;
    if windows == 0 { return Ok(()); }

    // Summary results per HEDGE_SIZE_MULT
    let mut summary_rows = vec![];
    let mut timeseries_cols: Vec<String> = vec![];
    let mut timeseries_data: Vec<Vec<f64>> = vec![];

    for &sm in HEDGE_SIZE_VALS {
        let mut total_passes = 0usize;
        let mut total = 0usize;
        let mut all_sharpes = vec![];
        let mut all_rets = vec![];
        let mut all_dds = vec![];
        let mut all_trades = vec![];
        let mut base5_equity = 1.0_f64;

        for (_, u_syms) in UNIVERSES {
            let syms: Vec<String> = u_syms.iter().map(|&s| s.to_string()).collect();
            for w in 0..windows {
                let start = min_len - (windows - w) * TEST_BARS - TRAIN_BARS;
                let end = start + TRAIN_BARS + TEST_BARS;
                let (eq, sh, dd, trades, _) = run_sim(&sym_data, &syms, start + TRAIN_BARS, end, sm);

                let pass = if trades >= MIN_TRADES && sh > 0.0 { 1 } else { 0 };
                total_passes += pass;
                total += 1;
                all_sharpes.push(sh);
                all_rets.push((eq - 1.0) * 100.0);
                all_dds.push(dd);
                all_trades.push(trades);

                if u_syms == &["BTCUSDT","ETHUSDT","SOLUSDT","XRPUSDT","DOGEUSDT","ADAUSDT"] {
                    base5_equity *= eq;
                }
            }
        }

        let n = total as f64;
        let avg_sh = all_sharpes.iter().sum::<f64>() / n;
        let avg_ret = all_rets.iter().sum::<f64>() / n;
        let avg_dd = all_dds.iter().sum::<f64>() / n;
        let tot_trades: usize = all_trades.iter().sum();
        let pass_rate = (total_passes as f64 / n) * 100.0;

        println!("SM={:.2}: {}/{} pass ({:.1}%), Sharpe {:.3}, Ret {:.1}%, DD {:.1}%, Trades {}",
            sm, total_passes, total, pass_rate, avg_sh, avg_ret, avg_dd, tot_trades);

        summary_rows.push((sm, total_passes, total, pass_rate, avg_sh, avg_ret, avg_dd, tot_trades, base5_equity));

        // Aggregate timeseries (sum equity across universes, then average)
        // For simplicity, just aggregate Base5 per-window compounded equity
        let mut ts_vals = vec![];
        for w in 0..windows {
            let start = min_len - (windows - w) * TEST_BARS - TRAIN_BARS;
            let end = start + TRAIN_BARS + TEST_BARS;
            let base5_syms: Vec<String> = UNIVERSES[0].1.iter().map(|&s| s.to_string()).collect();
            let (_, _, _, _, eq_curve) = run_sim(&sym_data, &base5_syms, start + TRAIN_BARS, end, sm);
            // Record the compound result per window
            let final_eq = *eq_curve.last().unwrap_or(&1.0);
            ts_vals.push(final_eq);
        }
        let col_name = format!("sm_{:.2}", sm).replace(".", "_");
        timeseries_cols.push(col_name);
        timeseries_data.push(ts_vals);
    }

    // Print winner
    let mut by_pass = summary_rows.clone();
    by_pass.sort_by(|a,b| b.1.cmp(&a.1)); // sort by total_passes desc
    let winner = by_pass.first().unwrap();
    println!("\nWINNER by pass rate: SM={:.2} — {}/{} pass ({:.1}%), Sharpe {:.3}",
        winner.0, winner.1, winner.2, winner.3, winner.4);

    let mut by_sharpe = summary_rows.clone();
    by_sharpe.sort_by(|a,b| b.4.partial_cmp(&a.4).unwrap_or(std::cmp::Ordering::Equal)); // sort by sharpe desc
    let sh_winner = by_sharpe.first().unwrap();
    println!("WINNER by Sharpe: SM={:.2} — Sharpe {:.3}, {}/{} pass", sh_winner.0, sh_winner.4, sh_winner.1, sh_winner.2);

    // Write summary CSV
    let mut f = File::create("snapshots/hedge_size_mult_summary.csv")?;
    writeln!(f, "hedge_size_mult,passes,total,pass_rate,avg_sharpe,avg_return,avg_dd,total_trades,base5_equity")?;
    for row in &summary_rows {
        writeln!(f, "{:.2},{},{},{:.2},{:.4},{:.2},{:.2},{},{:.6}",
            row.0, row.1, row.2, row.3, row.4, row.5, row.6, row.7, row.8)?;
    }
    println!("Written snapshots/hedge_size_mult_summary.csv");

    // Write per-window equity CSV (for charting)
    {
        let mut f = File::create("snapshots/hedge_size_mult_equity.csv")?;
        writeln!(f, "window,{}", timeseries_cols.join(","))?;
        for w in 0..windows {
            let mut row = vec![w.to_string()];
            for ts in &timeseries_data {
                row.push(format!("{:.6}", ts[w]));
            }
            writeln!(f, "{}", row.join(","))?;
        }
        println!("Written snapshots/hedge_size_mult_equity.csv");
    }

    // Write summary markdown
    let mut f = File::create("snapshots/hedge_size_mult_summary.md")?;
    writeln!(f, "# HEDGE_SIZE_MULT Extensive Sweep — T66")?;
    writeln!(f, "")?;
    writeln!(f, "**Mission:** Strip assumptions — HEDGE_SIZE_MULT=0.70 was a hardcoded magic number never independently tested.")?;
    writeln!(f, "**Harness:** Turtle-only + ATR_RANK(AP=17,LB=42,T=5) + USDT hedge (PCT=45 fixed)")?;
    writeln!(f, "**Test:** 13 values × 9 universes × 7 windows = 819 simulations")?;
    writeln!(f, "")?;
    writeln!(f, "| HEDGE_SIZE_MULT | Pass | Pass% | Sharpe | Ret% | DD% | Trades | Base5 Equity |")?;
    writeln!(f, "|----------------|------|-------|--------|------|-----|--------|--------------|")?;
    for row in &summary_rows {
        let base5_str = format!("{:.4}", row.8);
        writeln!(f, "| {:.2} | {}/{} | {:.1}% | {:.3} | {:.1}% | {:.1}% | {} | {}x |",
            row.0, row.1, row.2, row.3, row.4, row.5, row.6, row.7, base5_str)?;
    }
    writeln!(f, "")?;
    let best_pass = by_pass.first().unwrap();
    let best_sh = by_sharpe.first().unwrap();
    writeln!(f, "**Robustness winner (pass rate):** SM={:.2} → {}/{} pass ({:.1}%), Sharpe {:.3}", best_pass.0, best_pass.1, best_pass.2, best_pass.3, best_pass.4)?;
    writeln!(f, "**Sharpe winner:** SM={:.2} → Sharpe {:.3}, {}/{} pass", best_sh.0, best_sh.4, best_sh.1, best_sh.2)?;
    writeln!(f, "")?;
    writeln!(f, "**Baseline comparison:** HEDGE_SIZE_MULT=1.00 (no size reduction) is the reference.")?;
    println!("Written snapshots/hedge_size_mult_summary.md");

    Ok(())
}