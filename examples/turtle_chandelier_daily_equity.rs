//! Turtle+Chandelier Daily Equity Curve Harness
//!
//! Tracks Turtle+Chandelier (EP=21, Chandelier(28,2.0), dual ATR(25))
//! on the PRODUCTION universe (Base5: BTC, ETH, SOL, XRP, DOGE, ADA)
//! with full portfolio-level daily equity — NOT fixed 21-bar hold.
//!
//! This harness closes the equity monitoring gap: Turtle+Chandelier
//! (93% OOS pass, 6.29 Sharpe) was invisible in progress_equity_curves
//! because that harness uses fixed 21-bar hold for ALL strategies.
//!
//! Output: snapshots/turtle_chandelier_equity.csv
//! Chart:  python3 -c "exec(open('charts/plot_turtle_chandelier_equity.py').read())"
//!
//! Usage: cargo run --example turtle_chandelier_daily_equity --profile sweep

use anyhow::Result;
use krypto::data::loader::DataLoader;
use std::collections::HashMap;
use std::fs::File;
use std::io::Write;
use std::time::Instant;

const CANDLES: u32 = 3000;
const TAKER_FEE: f64 = 0.0004;   // 4bp taker (conservative) [reserved for live mode]
const SLIPPAGE: f64 = 0.0001;     // 1bp slippage per side [reserved for live mode]
const ENTRY_FEE: f64 = 0.0005;    // 5bp total entry cost (taker+slippage)
const EXIT_FEE: f64 = 0.0005;     // 5bp total exit cost

// =============================================================================
// Strategy params (FROZEN — all validated via hyperopt)
// =============================================================================
const TURTLE_ENTRY: usize = 21;       // hyperopt 2026-04-10
const CHAND_PERIOD: usize = 28;        // hyperopt 2026-04-11 fine-sweep
const CHAND_MULT: f64 = 2.00;         // hyperopt 2026-04-11
const TURTLE_ATR_PERIOD: usize = 25;  // hyperopt 2026-04-12
const TURTLE_ATR_MULT: f64 = 2.00;    // hyperopt 2026-04-12
const HOLD_MAX: usize = 45;           // hyperopt 2026-04-11
const POSITION_CAP: usize = 3;        // hyperopt 2026-04-11

// Production universe — 6/6 pass (100%) incl W04/W05
const BASE5: &[&str] = &["BTCUSDT", "ETHUSDT", "SOLUSDT", "XRPUSDT", "DOGEUSDT", "ADAUSDT"];
const OUTPUT_CSV: &str = "snapshots/turtle_chandelier_equity.csv";

const STARTING_EQUITY: f64 = 10_000.0;

// =============================================================================
// Data structures
// =============================================================================

struct SymData {
    close: Vec<f64>,
    high: Vec<f64>,
    low: Vec<f64>,
    vol: Vec<f64>,
}

struct OpenTrade {
    sym: String,
    entry_bar: usize,
    entry_price: f64,
    highest_high: f64,
    shares: f64,
}

fn atr_at(high: &[f64], low: &[f64], close: &[f64], period: usize, idx: usize) -> f64 {
    if idx < period { return 0.0; }
    let mut trs = Vec::with_capacity(period);
    for i in (idx + 1 - period)..=idx {
        let h = high.get(i).copied().unwrap_or(0.0);
        let l = low.get(i).copied().unwrap_or(0.0);
        let c0 = close.get(i.saturating_sub(1)).copied().unwrap_or(0.0);
        trs.push((h - l).max((h - c0).abs()).max((l - c0).abs()));
    }
    if trs.is_empty() { return 0.0; }
    trs.iter().sum::<f64>() / period as f64
}

fn turtle_signal(close: &[f64], _high: &[f64], entry_period: usize, idx: usize) -> bool {
    if idx < entry_period + 1 { return false; }
    let start = idx + 1 - entry_period;
    let mut max_close = f64::NEG_INFINITY;
    for i in start..idx {
        if let Some(&c) = close.get(i) { max_close = max_close.max(c); }
    }
    if let Some(&curr_close) = close.get(idx) {
        curr_close > max_close
    } else {
        false
    }
}

fn max_dd(equity: &[f64]) -> f64 {
    let mut peak = f64::NEG_INFINITY;
    let mut max_dd = 0.0_f64;
    for &e in equity {
        if e > peak { peak = e; }
        let dd = (peak - e) / peak;
        if dd > max_dd { max_dd = dd; }
    }
    max_dd
}

fn annualised_sharpe(daily_rets: &[f64]) -> f64 {
    if daily_rets.len() < 20 { return 0.0; }
    let mn: f64 = daily_rets.iter().sum::<f64>() / daily_rets.len() as f64;
    let sd = (daily_rets.iter().map(|x| (x - mn).powi(2)).sum::<f64>() / daily_rets.len() as f64).sqrt();
    if sd == 0.0 { return 0.0; }
    mn * 365.0_f64.sqrt() / sd
}

// =============================================================================
// Main equity simulation
// =============================================================================

fn run_equity(
    sym_data: &HashMap<String, SymData>,
    symbols: &[&str],
    start_bar: usize,
) -> (Vec<f64>, Vec<f64>, usize) {
    // Number of bars in the first symbol (all same length)
    let n = sym_data.get(symbols[0]).map(|d| d.close.len()).unwrap_or(0);
    let end_bar = n.min(start_bar + 5000); // cap at 5000 bars to cover full dataset

    let mut equity = STARTING_EQUITY;
    let mut btc_equity = STARTING_EQUITY;
    let mut open_trades: Vec<OpenTrade> = Vec::new();
    let mut total_trades = 0usize;

    let mut equity_curve = Vec::new();
    let mut btc_curve = Vec::new();

    // BTC close for buy-and-hold comparison
    let btc_data = sym_data.get("BTCUSDT");
    let btc_base_px = btc_data.and_then(|d| d.close.get(start_bar)).copied().unwrap_or(1.0);

    for bar in start_bar..end_bar {
        // ---- Compute dollar volumes for ranking ----
        let mut scores: Vec<(&str, f64)> = Vec::new();
        for &sym in symbols {
            if let Some(sd) = sym_data.get(sym) {
                if bar >= sd.close.len() { continue; }
                let dv = sd.vol.get(bar).copied().unwrap_or(0.0)
                    * sd.close.get(bar).copied().unwrap_or(0.0);
                scores.push((sym, if dv.is_finite() && dv > 0.0 { dv } else { 0.0 }));
            }
        }
        scores.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());
        let ranked: Vec<&str> = scores.into_iter().map(|(s, _)| s).take(POSITION_CAP).collect();

        // ---- Check exits for open trades ----
        let mut still_open: Vec<OpenTrade> = Vec::new();
        for mut trade in open_trades {
            let sd = match sym_data.get(&trade.sym) {
                Some(s) => s,
                None => { still_open.push(trade); continue; }
            };
            if bar >= sd.close.len() { still_open.push(trade); continue; }

            let exit_bar = bar; // exit at current bar close
            let bars_held = exit_bar.saturating_sub(trade.entry_bar);

            // DUAL_EXIT: Chandelier OR Turtle ATR — whichever fires first
            let mut highest_high_chand = trade.highest_high;
            let mut highest_high_turtle = trade.highest_high;
            for b in trade.entry_bar..=exit_bar {
                if b >= sd.high.len() { break; }
                highest_high_chand = highest_high_chand.max(sd.high[b]);
                highest_high_turtle = highest_high_turtle.max(sd.high[b]);
            }
            let atr_chand = atr_at(&sd.high, &sd.low, &sd.close, CHAND_PERIOD, exit_bar);
            let atr_turtle = atr_at(&sd.high, &sd.low, &sd.close, TURTLE_ATR_PERIOD, exit_bar);
            let trail_chand = highest_high_chand - CHAND_MULT * atr_chand;
            let trail_turtle = highest_high_turtle - TURTLE_ATR_MULT * atr_turtle;
            let current_close = sd.close[exit_bar];

            let hit_stop = bars_held >= 1
                && (current_close < trail_chand || current_close < trail_turtle);
            let hit_max_hold = bars_held >= HOLD_MAX;

            if hit_stop || hit_max_hold {
                // EXIT
                let exit_px = current_close * (1.0 - EXIT_FEE);
                let pnl = trade.shares * (exit_px - trade.entry_price);
                equity += pnl;
                total_trades += 1;
            } else {
                // HOLD — update highest high
                trade.highest_high = highest_high_chand.max(sd.high.get(exit_bar).copied().unwrap_or(trade.highest_high));
                still_open.push(trade);
            }
        }
        open_trades = still_open;

        // ---- Check entries ----
        for &sym in &ranked {
            if open_trades.len() >= POSITION_CAP { break; }
            if open_trades.iter().any(|t| t.sym == sym) { continue; } // already in

            let sd = match sym_data.get(sym) {
                Some(s) => s,
                None => continue,
            };
            if bar < TURTLE_ENTRY + 1 || bar >= sd.close.len() { continue; }

            if turtle_signal(&sd.close, &sd.high, TURTLE_ENTRY, bar) {
                let entry_px = sd.close[bar] * (1.0 + ENTRY_FEE); // taker+slippage
                let shares = equity / entry_px; // equal-weighted
                let highest_high = sd.high.get(bar.saturating_sub(1)).copied().unwrap_or(entry_px);

                open_trades.push(OpenTrade {
                    sym: sym.to_string(),
                    entry_bar: bar,
                    entry_price: entry_px,
                    highest_high,
                    shares,
                });
            }
        }

        // ---- BTC buy-and-hold (always fully invested) ----
        if let Some(sd) = btc_data {
            if let Some(&btc_close) = sd.close.get(bar) {
                btc_equity = STARTING_EQUITY * (btc_close / btc_base_px);
            }
        }

            // ---- Record equity at end of bar ----
        equity_curve.push(equity);
        btc_curve.push(btc_equity);
    }

    (equity_curve, btc_curve, total_trades)
}

// =============================================================================
// Per-year breakdown
// =============================================================================

fn per_year_stats(
    equity: &[f64],
    btc: &[f64],
) -> Vec<(usize, f64, f64, f64, f64)> {
    // Estimate year from bar index: bar 0 = approx 2018 start
    // Each bar ≈ 1 calendar day
    const BARS_PER_YEAR: usize = 365;
    let start_year = 2018;
    let mut by_year: std::collections::BTreeMap<usize, Vec<usize>> = std::collections::BTreeMap::new();
    for i in 0..equity.len() {
        let year = start_year + i / BARS_PER_YEAR;
        by_year.entry(year).or_default().push(i);
    }

    let mut results = Vec::new();
    for (year, indices) in &by_year {
        let eq_slice: Vec<f64> = indices.iter().map(|&i| equity[i]).collect();
        let btc_slice: Vec<f64> = indices.iter().map(|&i| btc[i]).collect();

        if eq_slice.len() < 20 { continue; }

        let start_eq = eq_slice.first().unwrap();
        let end_eq = eq_slice.last().unwrap();
        let ret = (end_eq / start_eq - 1.0) * 100.0;

        let start_btc = btc_slice.first().unwrap();
        let end_btc = btc_slice.last().unwrap();
        let btc_ret = (end_btc / start_btc - 1.0) * 100.0;

        let dd = max_dd(&eq_slice) * 100.0;
        let btc_dd = max_dd(&btc_slice) * 100.0;

        // Daily returns for Sharpe
        let mut daily_rets = Vec::new();
        for i in 1..eq_slice.len() {
            daily_rets.push((eq_slice[i] / eq_slice[i-1] - 1.0).max(-1.0));
        }
        let sharpe = annualised_sharpe(&daily_rets);

        results.push((*year, ret, btc_ret, dd, sharpe));
    }
    results
}

// =============================================================================
// Main
// =============================================================================

#[tokio::main]
async fn main() -> Result<()> {
    let t0 = Instant::now();
    eprintln!("==== Turtle+Chandelier Daily Equity (Base5) ====");
    eprintln!("Params: EP={}, Chand({}, {}), ATR({}), HM={}, CAP={}",
              TURTLE_ENTRY, CHAND_PERIOD, CHAND_MULT, TURTLE_ATR_PERIOD, HOLD_MAX, POSITION_CAP);
    eprintln!("Fees: {}bp entry + {}bp exit", (ENTRY_FEE * 10000.0) as i32, (EXIT_FEE * 10000.0) as i32);

    let loader = DataLoader::new(None, None);
    let mut sym_data_map: HashMap<String, SymData> = HashMap::new();

    for &sym in BASE5 {
        match loader.fetch_with_cache(sym, "1d", CANDLES).await {
            Ok(df) => {
                let n = df.height();
                macro_rules! col_vec {
                    ($name:expr) => {{
                        let chunked = df.column($name)?.f64()?;
                        chunked.into_iter().filter_map(|x| x).collect::<Vec<_>>()
                    }};
                }

                sym_data_map.insert(sym.to_string(), SymData {
                    close: col_vec!("close"),
                    high: col_vec!("high"),
                    low: col_vec!("low"),
                    vol: col_vec!("volume"),
                });
                eprintln!("  Loaded {} ({} bars)", sym, n);
            }
            Err(e) => { eprintln!("  ERROR loading {}: {}", sym, e); }
        }
    }

    let warmup = TURTLE_ENTRY.max(CHAND_PERIOD).max(TURTLE_ATR_PERIOD) + 5;
    let start_bar = warmup;

    eprintln!("\nRunning equity sim from bar {}...", start_bar);
    let (equity_curve, btc_curve, total_trades) =
        run_equity(&sym_data_map, BASE5, start_bar);

    // ---- Overall stats ----
    let final_equity = equity_curve.last().copied().unwrap_or(STARTING_EQUITY);
    let final_btc = btc_curve.last().copied().unwrap_or(STARTING_EQUITY);
    let total_return = (final_equity / STARTING_EQUITY - 1.0) * 100.0;
    let btc_total_return = (final_btc / STARTING_EQUITY - 1.0) * 100.0;
    let max_dd_pct = max_dd(&equity_curve) * 100.0;
    let btc_max_dd = max_dd(&btc_curve) * 100.0;

    let mut daily_rets = Vec::new();
    for i in 1..equity_curve.len() {
        daily_rets.push((equity_curve[i] / equity_curve[i-1] - 1.0).max(-1.0));
    }
    let sharpe = annualised_sharpe(&daily_rets);

    // ---- Per-year breakdown ----
    let yearly = per_year_stats(&equity_curve, &btc_curve);

    // ---- Output CSV ----
    {
        let mut f = File::create(OUTPUT_CSV)?;
        writeln!(f, "bar,equity,btc_buyhold,drawdown,bh_drawdown")?;
        let mut peak = STARTING_EQUITY;
        let mut btc_peak = STARTING_EQUITY;
        for (i, &eq) in equity_curve.iter().enumerate() {
            let btc = btc_curve[i];
            if eq > peak { peak = eq; }
            if btc > btc_peak { btc_peak = btc; }
            let dd = (peak - eq) / peak * 100.0;
            let btc_dd = (btc_peak - btc) / btc_peak * 100.0;
            let line = format!(
                "{},{},{},{},{}",
                i, eq, btc, dd, btc_dd
            );
            writeln!(f, "{}", line)?;
        }
        eprintln!("  CSV written: {}", OUTPUT_CSV);
    }

    // ---- Print summary ----
    eprintln!("\n==== SUMMARY ====");
    eprintln!("  Total trades: {}", total_trades);
    eprintln!("  Strategy return: {:+.1}%  (equity: ${:.0} → ${:.0})",
             total_return, STARTING_EQUITY, final_equity);
    eprintln!("  BTC buy-hold:   {:+.1}%  (equity: ${:.0} → ${:.0})",
             btc_total_return, STARTING_EQUITY, final_btc);
    eprintln!("  Strategy Sharpe: {:.2}", sharpe);
    eprintln!("  Strategy MaxDD:  {:.1}%", max_dd_pct);
    eprintln!("  BTC MaxDD:       {:.1}%", btc_max_dd);

    eprintln!("\n==== PER-YEAR ====");
    eprintln!("  {:>6} | {:>8} | {:>8} | {:>7} | {:>6}", "Year", "Strat%", "BTC%", "MaxDD%", "Sharpe");
    eprintln!("  {}", "-".repeat(45));
    for (year, ret, btc_ret, dd, yr_sharpe) in &yearly {
        let alpha = ret - btc_ret;
        let line = format!(
            "  {} | {:>+8.1} | {:>+8.1} | {:>7.1}% | {:>6.2}  (alpha {:+.1}%)",
            year, ret, btc_ret, dd, yr_sharpe, alpha
        );
        eprintln!("{}", line);
    }

    eprintln!("\n==== CHART ====");
    eprintln!("  python3 -c \"exec(open('charts/plot_turtle_chandelier_equity.py').read())\"");
    eprintln!("  Runtime: {:?}", t0.elapsed());

    Ok(())
}
