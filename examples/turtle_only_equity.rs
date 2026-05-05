//! T59: Turtle-Only Daily Equity Curve
//!
//! The live bot (src/live/bot.rs) uses Turtle-only exit:
//!   entry: Turtle breakout (close > max close over EP bars)
//!   exit:  Turtle ATR trailing stop (highest_high - ATR_MULT * ATR)
//!          + HOLD_MAX enforced independently of ATR warmup
//!   gate:  ATR_RANK(AP=17, LB=42, T=5) — BTC ATR percentile rank entry filter
//!
//! This harness produces the ACTUAL daily compounded equity curve for
//! the production strategy, matching live bot exactly. This is NOT the
//! dual Chandelier+Turtle path (progress_equity_curves.rs).
//!
//! Output:
//!   snapshots/turtle_only_equity.csv  — daily equity for charting
//!   snapshots/turtle_only_equity.md   — summary metrics

use anyhow::Result;
use chrono::Utc;
use krypto::data::loader::DataLoader;
use std::collections::HashMap;
use std::fs::File;
use std::io::Write;

// ── Production params (from src/live/config.rs) ──────────────────────────
const TURTLE_ENTRY: usize = 21;
const TURTLE_ATR_PERIOD: usize = 24;
const TURTLE_ATR_MULT: f64 = 2.00;
const ATR_ENTRY_MULT: f64 = 0.00;
const HOLD_MAX: usize = 12;
const POSITION_CAP: usize = 3;
const VOL_LOOKBACK: usize = 92; // 2026-05-05 dense AP17 sweep robust winner
const TAKER_FEE: f64 = 0.001;

// Regime filter: AP=17 confirmed from held-out 2026-05-04; T=5 production
const REGIME_ATR_PERIOD: usize = 17;
const REGIME_LOOKBACK: usize = 42;
const ATR_RANK_THRESHOLD: f64 = 5.0;

// ── Data config ───────────────────────────────────────────────────────────
const CANDLES: u32 = 3000;
const WARMUP_BARS: usize = 300;

// ── Universe ─────────────────────────────────────────────────────────────
const BASE_SYMBOLS: [&str; 6] = ["BTCUSDT", "ETHUSDT", "SOLUSDT", "XRPUSDT", "DOGEUSDT", "ADAUSDT"];

struct SymData {
    close: Vec<f64>,
    high: Vec<f64>,
    low: Vec<f64>,
    vol: Vec<f64>,
}

fn rolling_avg(vals: &[f64], window: usize, idx: usize) -> f64 {
    if idx < window { return 0.0; }
    vals[idx + 1 - window..=idx].iter().sum::<f64>() / window as f64
}

// Classic ATR (Wilder-style)
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

// BTC ATR percentile rank for regime entry gate
fn btc_atr_pct(btc_data: &SymData, period: usize, lookback: usize, idx: usize) -> f64 {
    if idx < lookback + period { return 50.0; }
    let curr = atr_at(&btc_data.high, &btc_data.low, &btc_data.close, period, idx);
    if curr <= 0.0 { return 50.0; }
    let start = idx + 1 - lookback - period;
    let end = idx + 1 - period;
    if end <= start { return 50.0; }
    let mut hist: Vec<f64> = (start..=end)
        .map(|i| atr_at(&btc_data.high, &btc_data.low, &btc_data.close, period, i))
        .collect();
    if hist.iter().all(|&x| x <= 0.0) { return 50.0; }
    hist.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let count = hist.iter().filter(|&&x| x < curr).count();
    (count as f64 / hist.len() as f64) * 100.0
}

// Turtle entry signal: close > max(close) over EP bars
fn turtle_signal(close: &[f64], entry_period: usize, idx: usize) -> bool {
    if idx < entry_period { return false; }
    let start = idx - entry_period;
    let mut max_close = f64::NEG_INFINITY;
    for i in start..idx {
        if let Some(&c) = close.get(i) {
            max_close = max_close.max(c);
        }
    }
    close.get(idx).copied().is_some_and(|c| c > max_close)
}

// ── Simulation: Turtle-only daily equity ────────────────────────────────
fn simulate_turtle_only(
    sym_data: &HashMap<String, SymData>,
    btc_data: &SymData,
    symbols: &[String],
    test_start: usize,
    test_end: usize,
) -> (f64, usize, Vec<f64>, Vec<f64>) {
    let mut equity = 1.0_f64;
    let mut peak = equity;
    let mut equity_curve = vec![1.0_f64; test_end - test_start];
    let mut daily_rets = Vec::new();
    let mut total_trades = 0usize;

    let mut bar = test_start;
    while bar + 2 < test_end {
        // Regime entry gate: BTC ATR percentile rank filter
        let btc_pct = btc_atr_pct(btc_data, REGIME_ATR_PERIOD, REGIME_LOOKBACK, bar);
        if btc_pct < ATR_RANK_THRESHOLD {
            equity_curve[bar - test_start] = equity;
            bar += 1;
            continue;
        }

        // Dollar-volume ranking for entry decision
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
            equity_curve[bar - test_start] = equity;
            bar += 1;
            continue;
        }

        // Try Turtle breakout entry
        let mut entered = false;
        for sym in &top_syms {
            if let Some(sd) = sym_data.get(sym) {
                if bar >= TURTLE_ENTRY + 1 && bar < sd.close.len() {
                    if turtle_signal(&sd.close, TURTLE_ENTRY, bar) {
                        let entry_px = sd.close[bar];
                        let entry = entry_px * (1.0 + TAKER_FEE);
                        let entry_bar_next = bar + 1;
                        let n = sd.close.len();

                        // Turtle ATR trailing stop
                        let mut highest_high = sd.high[entry_bar_next];
                        let max_bar = (entry_bar_next + HOLD_MAX).min(n.saturating_sub(1));
                        let mut exit_bar = max_bar;

                        // Seed ATR buffer from bars before entry_bar_next
                        let warm_start = entry_bar_next.saturating_sub(TURTLE_ATR_PERIOD);
                        let mut atr_buf: std::collections::VecDeque<f64> =
                            std::collections::VecDeque::with_capacity(TURTLE_ATR_PERIOD);
                        for b in warm_start..entry_bar_next {
                            if b > 0 {
                                let c0 = sd.close[b.saturating_sub(1)];
                                let tr = (sd.high[b] - sd.low[b])
                                    .max((sd.high[b] - c0).abs())
                                    .max((sd.low[b] - c0).abs());
                                atr_buf.push_back(tr);
                            }
                        }

                        for b in entry_bar_next..=max_bar {
                            if sd.high[b] > highest_high {
                                highest_high = sd.high[b];
                            }
                            let c0 = sd.close[b.saturating_sub(1)];
                            let tr = (sd.high[b] - sd.low[b])
                                .max((sd.high[b] - c0).abs())
                                .max((sd.low[b] - c0).abs());
                            atr_buf.push_back(tr);
                            if atr_buf.len() > TURTLE_ATR_PERIOD {
                                atr_buf.pop_front();
                            }

                            // Turtle ATR exit (fires when buffer warm)
                            if atr_buf.len() == TURTLE_ATR_PERIOD {
                                let atr = atr_buf.iter().sum::<f64>() / TURTLE_ATR_PERIOD as f64;
                                let turtle_stop = highest_high - TURTLE_ATR_MULT * atr;
                                if sd.low[b] <= turtle_stop {
                                    exit_bar = b;
                                    break;
                                }
                            }

                            // HOLD_MAX enforced independently of ATR warmup
                            if b >= entry_bar_next + HOLD_MAX {
                                exit_bar = b;
                                break;
                            }
                        }

                        if let Some(&exit_px) = sd.close.get(exit_bar) {
                            let exit = exit_px * (1.0 - TAKER_FEE);
                            let pct_ret = exit / entry - 1.0;

                            total_trades += 1;
                            equity *= 1.0 + pct_ret;

                            let bars_held = (exit_bar as i64 - entry_bar_next as i64).max(1) as usize;
                            let avg_daily = pct_ret / bars_held as f64;
                            for _ in 0..bars_held {
                                daily_rets.push(avg_daily);
                            }

                            let max_bar_fill = (test_start + equity_curve.len() - 1).min(exit_bar);
                            for b in entry_bar_next..=max_bar_fill {
                                equity_curve[b - test_start] = equity;
                            }

                            if equity > peak { peak = equity; }
                            bar = exit_bar;
                            // Safety: prevent bar from exceeding test_end
                            if bar >= test_end { bar = test_end.saturating_sub(1); }
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
        if bar < test_start + equity_curve.len() {
            equity_curve[bar - test_start] = equity;
        }
    }

    (equity, total_trades, equity_curve, daily_rets)
}

// ── Metrics ───────────────────────────────────────────────────────────────
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

// ── Main ─────────────────────────────────────────────────────────────────
#[tokio::main]
async fn main() -> Result<()> {
    println!("=== T59: TURTLE-ONLY DAILY EQUITY ===");
    println!("Params: EP={}, ATR({},{}), HM={}, CAP={}, VL={}",
             TURTLE_ENTRY, TURTLE_ATR_PERIOD, TURTLE_ATR_MULT, HOLD_MAX, POSITION_CAP, VOL_LOOKBACK);
    println!("Regime: AP={}, LB={}, T={}", REGIME_ATR_PERIOD, REGIME_LOOKBACK, ATR_RANK_THRESHOLD);
    println!("Fee: {:.1}bps each side\n", TAKER_FEE * 10000.0);

    let loader = DataLoader::new(None, None);
    let mut sym_data = HashMap::new();
    let mut min_len = usize::MAX;

    for &sym in &BASE_SYMBOLS {
        let df = loader.fetch_data(sym, "1d", CANDLES).await?;
        let close = df.column("close")?.f64()?.into_no_null_iter().collect::<Vec<_>>();
        let high = df.column("high")?.f64()?.into_no_null_iter().collect::<Vec<_>>();
        let low = df.column("low")?.f64()?.into_no_null_iter().collect::<Vec<_>>();
        let vol = df.column("volume")?.f64()?.into_no_null_iter().collect::<Vec<_>>();
        if close.len() < min_len { min_len = close.len(); }
        sym_data.insert(sym.to_string(), SymData { close, high, low, vol });
    }

    let btc = sym_data.get("BTCUSDT").expect("BTCUSDT must be in universe");
    let symbols: Vec<String> = BASE_SYMBOLS.iter().map(|&s| s.to_string()).collect();
    let test_start = WARMUP_BARS;
    let test_end = min_len;

    let (final_equity, total_trades, equity_curve, daily_rets) =
        simulate_turtle_only(&sym_data, btc, &symbols, test_start, test_end);

    let sharpe = annualised_sharpe(&daily_rets);
    let max_dd = max_dd_from(&equity_curve);
    let days = test_end - test_start;
    let annual_ret = (final_equity.powf(365.0 / days as f64) - 1.0) * 100.0;

    println!("--- Full-History Metrics (Turtle-only) ---");
    println!("  Period: {} trading days", days);
    println!("  Total trades: {}", total_trades);
    println!("  Final equity: {:.2}x ({:.1}%)", final_equity, (final_equity - 1.0) * 100.0);
    println!("  Annualised return: {:.1}%", annual_ret);
    println!("  Annualised Sharpe: {:.2}", sharpe);
    println!("  Max drawdown: {:.1}%\n", max_dd);

    // Per-year decomposition
    println!("--- Per-Year Breakdown ---");

    // Reload BTC for date strings
    let btc_df = loader.fetch_data("BTCUSDT", "1d", CANDLES).await?;
    let btc_close_all = btc_df.column("close")?.f64()?.into_no_null_iter().collect::<Vec<_>>();
    let dates: Vec<String> = if let Ok(dt_col) = btc_df.column("datetime") {
        dt_col.str()?.into_no_null_iter().map(|s| s.to_string()).collect()
    } else {
        vec![String::new(); btc_close_all.len()]
    };

    let mut year_groups: std::collections::HashMap<i32, Vec<f64>> = Default::default();
    for (i, &eq) in equity_curve.iter().enumerate() {
        let bar_idx = test_start + i;
        if bar_idx >= btc_close_all.len() { break; }
        let year = if !dates.is_empty() && bar_idx < dates.len() && !dates[bar_idx].is_empty() {
            dates[bar_idx][..4].parse::<i32>().unwrap_or(2020)
        } else {
            2020 + (i / 365) as i32
        };
        year_groups.entry(year).or_default().push(eq);
    }

    let mut years: Vec<i32> = year_groups.keys().copied().collect();
    years.sort_unstable();
    println!("{:<6} {:>10} {:>10} {:>10} {:>10}", "Year", "Equity", "Return", "Sharpe", "MaxDD");
    for year in &years {
        let eqs = &year_groups[year];
        if eqs.len() < 30 { continue; }
        let start_eq = *eqs.first().unwrap();
        let end_eq = *eqs.last().unwrap();
        let year_ret = (end_eq / start_eq - 1.0) * 100.0;

        let mut peak = start_eq;
        let mut max_dd_yr = 0.0_f64;
        let mut daily_yr_rets = Vec::new();
        for (i, &eq) in eqs.iter().enumerate() {
            if eq > peak { peak = eq; }
            let dd = 1.0 - eq / peak;
            if dd > max_dd_yr { max_dd_yr = dd; }
            if i > 0 {
                let daily_ret = eqs[i] / eqs[i - 1] - 1.0;
                daily_yr_rets.push(daily_ret);
            }
        }
        let sharpe_yr = annualised_sharpe(&daily_yr_rets);
        println!("{:<6} {:>9.1}x {:>9.1}% {:>10.2} {:>9.1}%",
            year, end_eq, year_ret, sharpe_yr, max_dd_yr * 100.0);
    }

    // Export CSV
    let mut csv = String::from("day,equity\n");
    for (i, &eq) in equity_curve.iter().enumerate() {
        csv.push_str(&format!("{},{:.6}\n", i, eq));
    }
    File::create("snapshots/turtle_only_equity.csv")?.write_all(csv.as_bytes())?;
    println!("\nCSV: snapshots/turtle_only_equity.csv");

    // Markdown report
    let dt = Utc::now().format("%Y-%m-%d %H:%M UTC").to_string();
    let md_path = "snapshots/turtle_only_equity.md";
    let mut f = File::create(md_path)?;
    writeln!(f, "# T59: Turtle-Only Daily Equity\n").unwrap();
    writeln!(f, "Generated: {}\n", dt).unwrap();
    writeln!(f, "## Production Params\n").unwrap();
    writeln!(f, "| Parameter | Value |").unwrap();
    writeln!(f, "|-----------|-------|").unwrap();
    writeln!(f, "| TURTLE_ENTRY | {} |", TURTLE_ENTRY).unwrap();
    writeln!(f, "| TURTLE_ATR_PERIOD | {} |", TURTLE_ATR_PERIOD).unwrap();
    writeln!(f, "| TURTLE_ATR_MULT | {:.2} |", TURTLE_ATR_MULT).unwrap();
    writeln!(f, "| HOLD_MAX | {} |", HOLD_MAX).unwrap();
    writeln!(f, "| POSITION_CAP | {} |", POSITION_CAP).unwrap();
    writeln!(f, "| VOL_LOOKBACK | {} |", VOL_LOOKBACK).unwrap();
    writeln!(f, "| REGIME_ATR_PERIOD | {} |", REGIME_ATR_PERIOD).unwrap();
    writeln!(f, "| REGIME_LOOKBACK | {} |", REGIME_LOOKBACK).unwrap();
    writeln!(f, "| ATR_RANK_THRESHOLD | {} |", ATR_RANK_THRESHOLD).unwrap();
    writeln!(f, "| TAKER_FEE | {:.1}bps |\n", TAKER_FEE * 10000.0).unwrap();
    writeln!(f, "## Full-History Results\n").unwrap();
    writeln!(f, "| Metric | Value |").unwrap();
    writeln!(f, "|--------|-------|").unwrap();
    writeln!(f, "| Final equity | {:.2}x ({:.1}%) |", final_equity, (final_equity - 1.0) * 100.0).unwrap();
    writeln!(f, "| Annualised Sharpe | {:.2} |", sharpe).unwrap();
    writeln!(f, "| Annualised return | {:.1}% |", annual_ret).unwrap();
    writeln!(f, "| Max drawdown | {:.1}% |", max_dd).unwrap();
    writeln!(f, "| Total trades | {} |", total_trades).unwrap();
    writeln!(f, "| Trading days | {} |\n", days).unwrap();
    writeln!(f, "## Comparison\n").unwrap();
    writeln!(f, "- Dual Chandelier+Turtle (progress harness): 619.9x / Sharpe 1.19").unwrap();
    writeln!(f, "- Turtle-only (this harness): {:.2}x / Sharpe {:.2}", final_equity, sharpe).unwrap();
    writeln!(f, "\n**Live bot path matches:** `examples/turtle_only_equity.rs`").unwrap();
    println!("Report: {}", md_path);

    Ok(())
}
