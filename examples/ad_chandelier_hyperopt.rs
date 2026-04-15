//! =========================================================
//! HYPERPARAMETER OPTIMIZATION: A/D Dual-Hat Chandelier Exit
//! =========================================================
//!
//! STRATEGY: A/D Momentum Ranking + Turtle Entry + Chandelier Exit
//!   - Rank top-K symbols by A/D momentum (AD_PERIOD bars)
//!   - Enter on Turtle breakout (close > max_close over EP bars)
//!   - Exit via Chandelier ATR trailing stop
//!   - Hold 54 bars max
//!
//! TARGET: Chandelier(period, mult) for A/D dual-hat
//!   - A/D currently uses CHAND_PERIOD=45, CHAND_MULT=2.5 (untested legacy default)
//!   - Turtle+Chandelier production uses P=28, M=2.0 (validated)
//!   - Question: should A/D dual-hat use the SAME exit params as Turtle+Chandelier?
//!
//! SWEEP:
//!   CHAND_PERIOD ∈ {10, 15, 20, 25, 28, 30, 35, 40, 45, 50, 60, 75, 100} (13 values)
//!   CHAND_MULT ∈ {1.0, 1.5, 2.0, 2.5, 3.0, 3.5, 4.0} (7 values)
//!   FIXED: AD_PERIOD=5, EP=21, HOLD=54, TOP_K=8, CAP=3, FEE=0.1%
//!   → 91 combinations across 9 universes, 252/252 walk-forward
//!
//! VALIDATION: Walk-forward 252 train / 252 test × 9 universes
//! METRIC: Global avg Sharpe, pass rate, worst DD, trade count
//! EXPORTS: CSV results + equity curves for Python charting

use anyhow::Result;
use krypto::data::loader::DataLoader;
use polars::prelude::*;
use std::collections::HashMap;
use std::fs::File;
use std::io::Write;
use std::time::Instant;

const CANDLES: u32 = 3000;
const TRAIN_BARS: usize = 252;
const TEST_BARS: usize = 252;
const HOLD_MAX: usize = 54;
const TAKER_FEE: f64 = 0.001;
const MIN_TRADES: usize = 3;

// Fixed strategy params (already validated)
const AD_PERIOD: usize = 5;   // full 1-100 sweep winner
const EP: usize = 21;          // Turtle entry (validated 2026-04-10)
const TOP_K: usize = 8;         // A/D TOP_K hyperopt winner (2026-04-12)
const POSITION_CAP: usize = 3;  // position cap (from Turtle+Chandelier)

// Sweep: Chandelier parameters
const CHAND_PERIODS: &[usize] = &[10, 15, 20, 25, 28, 30, 35, 40, 45, 50, 60, 75, 100];
const CHAND_MULTIPLES: &[f64] = &[1.0, 1.5, 2.0, 2.5, 3.0, 3.5, 4.0];

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

const CSV_OUT: &str = "snapshots/ad_chandelier_sweep.csv";
const EQUITY_OUT: &str = "snapshots/ad_chandelier_equity.csv";
const SUMMARY_MD: &str = "snapshots/ad_chandelier_sweep_summary.md";

struct SymData {
    close: Vec<f64>,
    high: Vec<f64>,
    low: Vec<f64>,
    open: Vec<f64>,
    vol: Vec<f64>,
    ad_line: Vec<f64>,
    ad_momentum: Vec<f64>,
}

fn compute_ad(high: &[f64], low: &[f64], close: &[f64], vol: &[f64]) -> Vec<f64> {
    let n = close.len();
    let mut ad = vec![0.0; n];
    for i in 0..n {
        let h = high[i];
        let l = low[i];
        let c = close[i];
        let v = vol[i];
        let hl = h - l;
        let mult = if hl > 1e-9 { ((c - l) - (h - c)) / hl } else { 0.0 };
        ad[i] = if i == 0 { mult * v } else { ad[i - 1] + mult * v };
    }
    ad
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

fn turtle_signal(close: &[f64], entry_period: usize, idx: usize) -> bool {
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

fn annualised_sharpe(daily_rets: &[f64]) -> f64 {
    if daily_rets.len() < 2 { return 0.0; }
    let mn: f64 = daily_rets.iter().sum::<f64>() / daily_rets.len() as f64;
    let sd = (daily_rets.iter().map(|x| (x - mn).powi(2)).sum::<f64>() / daily_rets.len() as f64).sqrt();
    if sd == 0.0 { return 0.0; }
    mn * 365.0_f64.sqrt() / sd
}

fn max_dd_from(equity: &[f64]) -> f64 {
    let mut peak = f64::NEG_INFINITY;
    let mut max_dd = 0.0_f64;
    for &e in equity {
        if e > peak { peak = e; }
        let dd = (peak - e) / peak;
        if dd > max_dd { max_dd = dd; }
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

/// Run A/D dual-hat simulation with specified Chandelier parameters.
/// Strategy: Rank top-K by A/D momentum, enter on Turtle breakout, exit via Chandelier.
fn run_sim(
    sym_data: &HashMap<String, SymData>,
    symbols: &[String],
    test_start: usize,
    test_end: usize,
    chand_period: usize,
    chand_mult: f64,
) -> WfResult {
    let mut equity = 1.0_f64;
    let mut equity_curve = vec![1.0_f64];
    let mut wins = 0usize;
    let mut total_trades = 0usize;
    let mut daily_rets = Vec::new();

    let mut bar = test_start;
    while bar + 2 < test_end {
        // Rank symbols by dollar volume for position selection
        let mut vol_scores: Vec<(&str, f64)> = Vec::new();
        for sym in symbols {
            if let Some(sd) = sym_data.get(sym) {
                if bar >= sd.close.len() { continue; }
                let dv = sd.vol.get(bar).copied().unwrap_or(0.0)
                    * sd.close.get(bar).copied().unwrap_or(0.0);
                vol_scores.push((sym.as_str(), if dv.is_finite() && dv > 0.0 { dv } else { 0.0 }));
            }
        }
        vol_scores.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());
        let vol_top: Vec<String> = vol_scores.into_iter().take(POSITION_CAP).map(|(s, _)| s.to_string()).collect();

        if vol_top.is_empty() {
            equity_curve.push(equity);
            bar += 1;
            continue;
        }

        // Among vol-ranked symbols, pick the one with highest A/D momentum
        let mut best_sym: Option<String> = None;
        let mut best_mom = f64::NEG_INFINITY;
        for sym in &vol_top {
            if let Some(sd) = sym_data.get(sym) {
                if bar >= AD_PERIOD + 1 && bar < sd.ad_momentum.len() {
                    let mom = sd.ad_momentum[bar];
                    if mom > best_mom {
                        best_mom = mom;
                        best_sym = Some(sym.clone());
                    }
                }
            }
        }

        let mut entered = false;
        if let Some(sym) = best_sym {
            if let Some(sd) = sym_data.get(&sym) {
                // Turtle breakout entry check
                if bar >= EP + 1 && bar < sd.close.len() {
                    if turtle_signal(&sd.close, EP, bar) {
                        let entry_px = sd.close[bar];
                        let entry = entry_px * (1.0 - TAKER_FEE);
                        let entry_bar_next = bar + 1;
                        let n = sd.close.len();

                        let mut highest_high = sd.high[entry_bar_next];
                        let max_bar = (entry_bar_next + HOLD_MAX).min(n.saturating_sub(1));
                        let mut exit_bar = max_bar;

                        for b in entry_bar_next..=max_bar.min(n.saturating_sub(1)) {
                            highest_high = highest_high.max(sd.high[b]);
                            let atr_val = atr_at(&sd.high, &sd.low, &sd.close, chand_period, b);
                            let trail = highest_high - chand_mult * atr_val;
                            if sd.close[b] < trail {
                                exit_bar = b;
                                break;
                            }
                        }

                        if let Some(&exit_px) = sd.close.get(exit_bar) {
                            let exit = exit_px * (1.0 - TAKER_FEE);
                            let gross_ret = exit / entry - 1.0;
                            let bars_held = (exit_bar as i64 - entry_bar_next as i64).max(1) as usize;

                            wins += if gross_ret > 0.0 { 1 } else { 0 };
                            total_trades += 1;
                            equity *= 1.0 + gross_ret;

                            let avg_daily = gross_ret / bars_held as f64;
                            for _ in 0..bars_held {
                                daily_rets.push(avg_daily);
                            }

                            equity_curve.push(equity);
                            bar = exit_bar + 1;
                            entered = true;
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

    let ret = (equity - 1.0) * 100.0;
    let sharpe = annualised_sharpe(&daily_rets);
    let max_dd = max_dd_from(&equity_curve);
    let win_rate = if total_trades > 0 { wins as f64 / total_trades as f64 * 100.0 } else { 0.0 };
    let pass = total_trades >= MIN_TRADES && ret > 0.0;

    WfResult { ret, sharpe, max_dd, trades: total_trades, win_rate, pass, equity_curve }
}

#[tokio::main]
async fn main() -> Result<()> {
    let t0 = Instant::now();
    eprintln!("==== A/D Dual-Hat Chandelier Hyperopt ====");
    eprintln!("Strategy: A/D momentum rank (TOP_K={}) + Turtle entry (EP={}) + Chandelier exit", TOP_K, EP);
    eprintln!("Fixed: AD_PERIOD={}, HOLD={}, CAP={}", AD_PERIOD, HOLD_MAX, POSITION_CAP);
    eprintln!("Sweep: {} periods × {} multiples = {} combos", CHAND_PERIODS.len(), CHAND_MULTIPLES.len(), CHAND_PERIODS.len() * CHAND_MULTIPLES.len());
    eprintln!("Universes: 9 | Walk-forward: {}/{}\n", TRAIN_BARS, TEST_BARS);

    let loader = DataLoader::new(None, None);
    let mut all_syms: std::collections::HashSet<String> = std::collections::HashSet::new();
    for (_, syms) in UNIVERSES {
        for &s in *syms { all_syms.insert(s.to_string()); }
    }

    let mut raw_cache: HashMap<String, DataFrame> = HashMap::new();
    let mut min_len = usize::MAX;
    for sym in all_syms.iter() {
        match loader.fetch_with_cache(sym.as_str(), "1d", CANDLES).await {
            Ok(df) => {
                min_len = min_len.min(df.height());
                raw_cache.insert(sym.clone(), df);
            }
            Err(e) => { eprintln!("  WARNING: {} load failed: {}", sym, e); }
        }
    }

    let n = min_len.min(2800);
    let mut sym_data_map: HashMap<String, SymData> = HashMap::new();
    for sym in &all_syms {
        if let Some(df) = raw_cache.get(sym) {
            let n_min = df.height().min(n);
            macro_rules! col_vec {
                ($name:expr) => {{
                    let chunked = df.column($name)?.f64()?;
                    chunked.into_iter().filter_map(|x| x).take(n_min).collect::<Vec<_>>()
                }};
            }
            let close = col_vec!("close");
            let high = col_vec!("high");
            let low = col_vec!("low");
            let open = col_vec!("open");
            let vol = col_vec!("volume");
            let ad_line = compute_ad(&high, &low, &close, &vol);
            let mut ad_momentum = vec![0.0; close.len()];
            for i in AD_PERIOD..close.len() {
                ad_momentum[i] = ad_line[i] - ad_line[i - AD_PERIOD];
            }
            sym_data_map.insert(sym.clone(), SymData { close, high, low, open, vol, ad_line, ad_momentum });
        }
    }
    eprintln!("Loaded {} symbols, {} bars\n", sym_data_map.len(), n);

    // ── CSV setup ─────────────────────────────────────────────────────────────
    let mut csv_lines = vec!["chand_period,chand_mult,universe,window,return_pct,sharpe,max_dd_pct,trades,win_rate_pct,pass".to_string()];
    let mut equity_lines = vec!["chand_period,chand_mult,universe,window,step,equity".to_string()];

    // ── Global result accumulators ──────────────────────────────────────────────
    let mut global_results: Vec<(usize, f64, usize, usize, f64, f64, f64, usize)> = Vec::new();

    // ── Run sweep ──────────────────────────────────────────────────────────────
    for &chand_p in CHAND_PERIODS {
        for &chand_m in CHAND_MULTIPLES {
            let label = format!("P={}, M={}", chand_p, chand_m);
            eprintln!("===== {} =====", label);

            let mut g_pass = 0usize;
            let mut g_total = 0usize;
            let mut g_sharpe_sum = 0.0_f64;
            let mut g_ret_sum = 0.0_f64;
            let mut g_dd_max = 0.0_f64;
            let mut g_trades = 0usize;

            for &(uni_name, symbols) in UNIVERSES {
                let symbols: Vec<String> = symbols.iter().map(|s| s.to_string()).collect();
                let all_loaded = symbols.iter().all(|s| sym_data_map.contains_key(s));
                if !all_loaded { continue; }

                let total_windows = n.saturating_sub(TRAIN_BARS + TEST_BARS) / TEST_BARS;
                if total_windows == 0 { continue; }

                for wi in 0..total_windows {
                    let train_end = TRAIN_BARS + wi * TEST_BARS;
                    let test_start = train_end;
                    let test_end = (test_start + TEST_BARS).min(n);
                    if test_end.saturating_sub(test_start) < 5 { continue; }

                    let r = run_sim(&sym_data_map, &symbols, test_start, test_end, chand_p, chand_m);

                    g_pass += if r.pass { 1 } else { 0 };
                    g_total += 1;
                    g_sharpe_sum += r.sharpe;
                    g_ret_sum += r.ret;
                    g_dd_max = g_dd_max.max(r.max_dd);
                    g_trades += r.trades;

                    csv_lines.push(format!(
                        "{},{},{},{},{:.2},{:.4},{:.2},{},{:.2},{}",
                        chand_p, chand_m, uni_name, wi, r.ret, r.sharpe, r.max_dd, r.trades, r.win_rate, r.pass
                    ));

                    // Sample equity curve: every 10 steps
                    for (step, &eq) in r.equity_curve.iter().enumerate() {
                        if step % 10 == 0 {
                            equity_lines.push(format!("{},{},{},{},{},{:.6}", chand_p, chand_m, uni_name, wi, step, eq));
                        }
                    }
                }
            }

            let avg_sh = g_sharpe_sum / g_total.max(1) as f64;
            let avg_ret = g_ret_sum / g_total.max(1) as f64;
            let pass_pct = g_pass as f64 / g_total.max(1) as f64 * 100.0;
            eprintln!("  GLOBAL: {}/{} pass ({:.0}%) | sh={:+.3} ret={:+.1}% DD={:.1}% {}tr",
                g_pass, g_total, pass_pct, avg_sh, avg_ret, g_dd_max, g_trades);

            global_results.push((chand_p, chand_m, g_pass, g_total, avg_sh, avg_ret, g_dd_max, g_trades));
        }
    }

    // ── Write results ─────────────────────────────────────────────────────────
    let mut f = File::create(CSV_OUT)?;
    for line in &csv_lines { writeln!(f, "{}", line)?; }
    let mut ef = File::create(EQUITY_OUT)?;
    for line in &equity_lines { writeln!(ef, "{}", line)?; }

    // ── Ranking ────────────────────────────────────────────────────────────────
    global_results.sort_by(|a, b| b.4.partial_cmp(&a.4).unwrap());

    eprintln!("\n==== GLOBAL RANKING (by avg Sharpe) ====");
    for (i, (p, m, g_pass, g_total, avg_sh, avg_ret, worst_dd, g_trades)) in global_results.iter().enumerate() {
        let pass_pct = *g_pass as f64 / (*g_total as f64).max(1.0_f64) * 100.0;
        eprintln!("  {:>2}. P={:>3}, M={:.1} | {}/{} ({:.0}%) | sh={:+.3} ret={:+.1}% DD={:.1}% {}tr",
            i+1, *p, *m, *g_pass, *g_total, pass_pct, *avg_sh, *avg_ret, *worst_dd, *g_trades);
    }

    // ── Write markdown summary ────────────────────────────────────────────────
    let mut md = File::create(SUMMARY_MD)?;
    writeln!(md, "# A/D Dual-Hat Chandelier Hyperopt — 2026-04-12")?;
    writeln!(md, "")?;
    writeln!(md, "## Strategy")?;
    writeln!(md, "A/D momentum ranking + Turtle entry + Chandelier exit")?;
    writeln!(md, "- A/D period: {} (validated winner)", AD_PERIOD)?;
    writeln!(md, "- Turtle EP: {} | TOP_K: {} | CAP: {} | HOLD: {}", EP, TOP_K, POSITION_CAP, HOLD_MAX)?;
    writeln!(md, "- Fee: {}% taker each side", TAKER_FEE * 100.0)?;
    writeln!(md, "")?;
    writeln!(md, "## Sweep")?;
    writeln!(md, "- CHAND_PERIOD: {:?}", CHAND_PERIODS)?;
    writeln!(md, "- CHAND_MULT: {:?}", CHAND_MULTIPLES)?;
    writeln!(md, "- Total combinations: {} × {} = {}", CHAND_PERIODS.len(), CHAND_MULTIPLES.len(), CHAND_PERIODS.len() * CHAND_MULTIPLES.len())?;
    writeln!(md, "- Universes: 9 | Walk-forward: {}/{}", TRAIN_BARS, TEST_BARS)?;
    writeln!(md, "")?;
    writeln!(md, "## Global Ranking (by avg Sharpe)")?;
    writeln!(md, "| Rank | P | M | Pass | Avg Sharpe | Avg Ret | Worst DD | Trades |")?;
    writeln!(md, "|------|---|---|------|------------|---------|-----------|--------|")?;

    for (i, (p, m, g_pass, g_total, avg_sh, avg_ret, worst_dd, g_trades)) in global_results.iter().enumerate() {
        let pass_pct = *g_pass as f64 / (*g_total as f64).max(1.0_f64) * 100.0;
        writeln!(md, "| {} | {} | {:.1} | {}/{} ({:.0}%) | {:+.3} | {:+.1}% | {:.1}% | {} |",
            i+1, *p, *m, *g_pass, *g_total, pass_pct, *avg_sh, *avg_ret, *worst_dd, *g_trades)?;
    }

    eprintln!("\n==== DONE in {:?} ====", t0.elapsed());
    eprintln!("CSV: {}", CSV_OUT);
    eprintln!("Equity: {}", EQUITY_OUT);
    eprintln!("Summary: {}", SUMMARY_MD);

    Ok(())
}
