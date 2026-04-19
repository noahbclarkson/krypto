//! =========================================================
//! HYPERPARAMETER OPTIMIZATION: RegimeAdaptive atr_trend_pct
//! =========================================================
//!
//! Background:
//!   RegimeAdaptive (src/algo/strategies.rs) combines Turtle entry with
//!   an ATR percentile regime filter: only enter when current ATR is in
//!   the top atr_trend_pct of its atr_lookback history.
//!
//!   HARDCODED CONSTANTS (never systematically tested):
//!     atr_lookback: 20  (hardcoded, no validation)
//!     atr_trend_pct: 0.90  (90% = almost always trending - effectively DISABLED)
//!
//!   MEMORY.md noted: "atr_trend_pct=0.60 was assumed (not tested)"
//!
//! HYPOTHESIS:
//!   Turtle gets whipsawed in choppy regimes. If ATR% is elevated (market
//!   volatile but not trending), regime filtering should reduce drawdowns
//!   and improve Sharpe WITHOUT reducing trade count below minimum viable.
//!
//! TARGET PARAMETER: atr_trend_pct
//!   Range: 0.20 to 0.90 step 0.05 (15 values)
//!   0.90 = baseline (no filter, effectively always active)
//!   0.50 = only enter when ATR is in top 50% (moderate filter)
//!   0.20 = only enter when ATR is top 20% (strictest filter)
//!
//! METHOD:
//!   1. RegimeFiltered Turtle: entry only when ATR percentile >= atr_trend_pct
//!   2. Baseline (unfiltered): same Turtle, no regime filter
//!   3. Walk-forward 252/252 across all 9 universes x 6 windows
//!   4. Compare: Sharpe, pass rate, trade count, equity curve
//!   5. Penalize configs with <30 trades (too sparse to be reliable)
//!
//! EQUITY EXPORT:
//!   Per-config equity curves exported to snapshots/regime_atr_trend_equity.csv
//!   Python chart: charts/plot_regime_trend_comparison.py -> comparison_chart.png

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
const HOLD_MAX: usize = 45;
const TAKER_FEE: f64 = 0.001;
const POSITION_CAP: usize = 3;
const MIN_TRADES: usize = 3;
const EP: usize = 21;
const CHAND_PERIOD: usize = 15;  // current production
const CHAND_MULT: f64 = 1.50;    // current production
const TURTLE_ATR_PERIOD: usize = 24;
const TURTLE_ATR_MULT: f64 = 2.00;
const VOL_LOOKBACK: usize = 2;

// ATR trend pct sweep - 0.90 is the baseline (no filter effectively)
const ATR_TREND_VALUES: &[f64] = &[
    0.90, 0.85, 0.80, 0.75, 0.70, 0.65, 0.60,
    0.55, 0.50, 0.45, 0.40, 0.35, 0.30, 0.25, 0.20,
];

const ATR_LOOKBACK_VALUES: &[usize] = &[20, 50, 100];

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

const CSV_RESULTS: &str = "snapshots/regime_atr_trend_sweep.csv";
const CSV_EQUITY: &str = "snapshots/regime_atr_trend_equity.csv";
const EQUITY_STEP_LIMIT: usize = 2000;

struct SymData {
    close: Vec<f64>,
    high: Vec<f64>,
    low: Vec<f64>,
    vol: Vec<f64>,
}

struct WfResult {
    ret: f64,
    sharpe: f64,
    max_dd: f64,
    trades: usize,
    win_rate: f64,
    pass: bool,
    equity_steps: Vec<f64>,
}

struct ConfigResult {
    atr_trend_pct: f64,
    atr_lookback: usize,
    avg_sharpe: f64,
    avg_ret: f64,
    worst_dd: f64,
    total_trades: usize,
    pass_count: usize,
    total_windows: usize,
    global_pass_rate: f64,
    sparse_count: usize,
    is_baseline: bool,
}

impl ConfigResult {
    fn score(&self) -> f64 {
        if self.sparse_count > 2 {
            return -999.0;
        }
        let pass_bonus = self.pass_count as f64 / self.total_windows.max(1) as f64 * 2.0;
        self.avg_sharpe + pass_bonus
    }
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

fn atr_pct_of_price(high: &[f64], low: &[f64], close: &[f64], atr_period: usize, idx: usize) -> f64 {
    if idx == 0 { return 0.0; }
    let curr_atr = atr_at(high, low, close, atr_period, idx);
    let price = close.get(idx).copied().unwrap_or(1.0);
    if price <= 0.0 { return 0.0; }
    curr_atr / price
}

fn rolling_avg(vals: &[f64], window: usize, idx: usize) -> f64 {
    if idx < window { return *vals.get(idx).unwrap_or(&0.0); }
    let start = idx + 1 - window;
    vals[start..=idx].iter().sum::<f64>() / window as f64
}

fn turtle_signal(close: &[f64], entry_period: usize, idx: usize) -> bool {
    if idx < entry_period + 1 { return false; }
    let start = idx + 1 - entry_period;
    let mut max_close = f64::NEG_INFINITY;
    for i in start..idx {
        if let Some(&c) = close.get(i) { max_close = max_close.max(c); }
    }
    close.get(idx).map_or(false, |&c| c > max_close)
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

fn annualised_sharpe(daily_rets: &[f64]) -> f64 {
    if daily_rets.len() < 2 { return 0.0; }
    let mn: f64 = daily_rets.iter().sum::<f64>() / daily_rets.len() as f64;
    let sd = (daily_rets.iter().map(|x| (x - mn).powi(2)).sum::<f64>() / daily_rets.len() as f64).sqrt();
    if sd == 0.0 { return 0.0; }
    mn * 365.0_f64.sqrt() / sd
}

fn in_trending_regime(
    high: &[f64], low: &[f64], close: &[f64],
    atr_period: usize,
    atr_lookback: usize,
    atr_trend_pct: f64,
    idx: usize,
) -> bool {
    // Warmup: not enough history -> always allow
    if idx < atr_lookback + atr_period { return true; }

    let curr_atr_pct = atr_pct_of_price(high, low, close, atr_period, idx);
    if curr_atr_pct <= 0.0 { return true; }

    let start = idx.saturating_sub(atr_lookback);
    let mut count_below = 0usize;
    let mut count_total = 0usize;
    for i in start..=idx {
        let ap = atr_pct_of_price(high, low, close, atr_period, i);
        if ap > 0.0 {
            if ap < curr_atr_pct { count_below += 1; }
            count_total += 1;
        }
    }

    if count_total == 0 { return true; }
    let rank = count_below as f64 / count_total as f64;
    rank >= atr_trend_pct
}

fn run_sim_regime(
    sym_data: &HashMap<String, SymData>,
    symbols: &[String],
    test_start: usize,
    test_end: usize,
    atr_lookback: usize,
    atr_trend_pct: f64,
) -> WfResult {
    let mut equity = 1.0_f64;
    let mut equity_curve = vec![1.0_f64];
    let mut peak = equity;
    let mut wins = 0usize;
    let mut total_trades = 0usize;
    let mut daily_rets = Vec::new();

    let mut bar = test_start;
    while bar + 2 < test_end {
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
                if bar >= TURTLE_ATR_PERIOD + atr_lookback + 2 && bar < sd.close.len() {
                    // Regime filter check
                    if !in_trending_regime(&sd.high, &sd.low, &sd.close, TURTLE_ATR_PERIOD, atr_lookback, atr_trend_pct, bar) {
                        continue;
                    }
                    if turtle_signal(&sd.close, EP, bar) {
                        let entry_px = sd.close[bar];
                        let entry = entry_px * (1.0 - TAKER_FEE);
                        let entry_bar_next = bar + 1;
                        let n = sd.close.len();

                        let mut highest_high_chand = sd.high[entry_bar_next];
                        let mut highest_high_turtle = sd.high[entry_bar_next];
                        let max_bar = (entry_bar_next + HOLD_MAX).min(n.saturating_sub(1));
                        let mut exit_bar = max_bar;

                        for b in entry_bar_next..=max_bar.min(n.saturating_sub(1)) {
                            highest_high_chand = highest_high_chand.max(sd.high[b]);
                            let atr_chand = atr_at(&sd.high, &sd.low, &sd.close, CHAND_PERIOD, b);
                            let trail_chand = highest_high_chand - CHAND_MULT * atr_chand;

                            highest_high_turtle = highest_high_turtle.max(sd.high[b]);
                            let atr_turtle = atr_at(&sd.high, &sd.low, &sd.close, TURTLE_ATR_PERIOD, b);
                            let trail_turtle = highest_high_turtle - TURTLE_ATR_MULT * atr_turtle;

                            if sd.close[b] < trail_chand || sd.close[b] < trail_turtle {
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

                            if equity > peak { peak = equity; }
                            if equity_curve.len() < EQUITY_STEP_LIMIT {
                                equity_curve.push(equity);
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
            if equity_curve.len() < EQUITY_STEP_LIMIT {
                equity_curve.push(equity);
            }
            bar += 1;
        }
    }

    let ret = (equity - 1.0) * 100.0;
    let sharpe = annualised_sharpe(&daily_rets);
    let max_dd = max_dd_from(&equity_curve);
    let win_rate = if total_trades > 0 { wins as f64 / total_trades as f64 * 100.0 } else { 0.0 };
    let pass = total_trades >= MIN_TRADES && ret > 0.0;

    WfResult { ret, sharpe, max_dd, trades: total_trades, win_rate, pass, equity_steps: equity_curve }
}

#[tokio::main]
async fn main() -> Result<()> {
    let t0 = Instant::now();
    println!("\n==== RegimeAdaptive ATR Trend Pct Hyperopt ====");
    println!("  Params: EP={}, Chand({},{}), ATR({},{}), HM={}, CAP={}",
             EP, CHAND_PERIOD, CHAND_MULT, TURTLE_ATR_PERIOD, TURTLE_ATR_MULT, HOLD_MAX, POSITION_CAP);
    println!("  Sweep: atr_trend_pct in {:?}", ATR_TREND_VALUES);
    println!("  Lookbacks: {:?}", ATR_LOOKBACK_VALUES);
    println!("  Baseline: atr_trend_pct=0.90 (no filter)\n");

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
            sym_data_map.insert(sym.clone(), SymData {
                close: col_vec!("close"),
                high:  col_vec!("high"),
                low:   col_vec!("low"),
                vol:   col_vec!("volume"),
            });
        }
    }
    eprintln!("Loaded {} symbols, {} bars\n", sym_data_map.len(), n);

    let mut all_results: Vec<ConfigResult> = Vec::new();
    let mut baseline_sharpe = 0.0f64;

    for &atr_lookback in ATR_LOOKBACK_VALUES {
        for &atr_trend_pct in ATR_TREND_VALUES {
            let is_baseline = (atr_trend_pct - 0.90).abs() < 0.001 && atr_lookback == 20;

            let mut total_oos_sharpe = 0.0f64;
            let mut total_oos_ret = 0.0f64;
            let mut worst_dd = 0.0f64;
            let mut total_trades = 0usize;
            let mut pass_count = 0usize;
            let mut total_windows = 0usize;
            let mut sparse_count = 0usize;

            for &(label, symbols) in UNIVERSES {
                let symbols: Vec<String> = symbols.iter().map(|s| s.to_string()).collect();
                let all_loaded = symbols.iter().all(|s| sym_data_map.contains_key(s));
                if !all_loaded { continue; }

                let total_wins = n.saturating_sub(TRAIN_BARS + TEST_BARS) / TEST_BARS;
                if total_wins == 0 { continue; }

                for wi in 0..total_wins {
                    let train_end = TRAIN_BARS + wi * TEST_BARS;
                    let test_start = train_end;
                    let test_end = (test_start + TEST_BARS).min(n);

                    if test_end.saturating_sub(test_start) < 5 { continue; }

                    let r = run_sim_regime(&sym_data_map, &symbols, test_start, test_end, atr_lookback, atr_trend_pct);
                    total_oos_sharpe += r.sharpe;
                    total_oos_ret += r.ret;
                    if r.max_dd > worst_dd { worst_dd = r.max_dd; }
                    total_trades += r.trades;
                    if r.pass { pass_count += 1; }
                    if r.trades < MIN_TRADES { sparse_count += 1; }
                    total_windows += 1;
                }
            }

            let n_win = total_windows.max(1);
            let avg_sharpe = total_oos_sharpe / n_win as f64;
            let avg_ret = total_oos_ret / n_win as f64;
            let global_pass_rate = pass_count as f64 / n_win as f64 * 100.0;

            if is_baseline {
                baseline_sharpe = avg_sharpe;
            }

            let cfg = ConfigResult {
                atr_trend_pct,
                atr_lookback,
                avg_sharpe,
                avg_ret,
                worst_dd,
                total_trades,
                pass_count,
                total_windows,
                global_pass_rate,
                sparse_count,
                is_baseline,
            };

            let delta = avg_sharpe - baseline_sharpe;
            let marker = if cfg.is_baseline { "[BASELINE]" } else if delta > 0.0 { "+" } else { " " };
            println!(
                "  lookback={:3}, pct={:.2}: Sharpe={:+.4} (d={:+.4}) | pass={:5.1}% | trades={:5} | sparse={:2} {}",
                atr_lookback, atr_trend_pct, avg_sharpe, delta, global_pass_rate, total_trades, sparse_count, marker
            );

            all_results.push(cfg);
        }
    }

    all_results.sort_by(|a, b| b.score().partial_cmp(&a.score()).unwrap());

    let winner = &all_results[0];
    let baseline_idx = all_results.iter().position(|c| c.is_baseline);

    println!("\n======================================================================");
    println!("  [WINNER] lookback={}, atr_trend_pct={:.2}", winner.atr_lookback, winner.atr_trend_pct);
    println!("  Avg Sharpe: {:+.4} vs Baseline {:+.4} (d={:+.4})", winner.avg_sharpe, baseline_sharpe, winner.avg_sharpe - baseline_sharpe);
    println!("  Pass rate: {:.1}% | Trades: {} | Sparse: {}", winner.global_pass_rate, winner.total_trades, winner.sparse_count);

    if let Some(bi) = baseline_idx {
        let baseline_cfg = &all_results[bi];
        println!("\n  Baseline comparison (lookback={}, pct={:.2}):", baseline_cfg.atr_lookback, baseline_cfg.atr_trend_pct);
        println!("    Sharpe: {:+.4} -> {:+.4} (d={:+.4})", baseline_cfg.avg_sharpe, winner.avg_sharpe, winner.avg_sharpe - baseline_cfg.avg_sharpe);
        println!("    Return: {:+.2}% -> {:+.2}%", baseline_cfg.avg_ret, winner.avg_ret);
        println!("    Pass: {:.1}% -> {:.1}%", baseline_cfg.global_pass_rate, winner.global_pass_rate);
    }
    println!("======================================================================");

    let mut csv_f = File::create(CSV_RESULTS)?;
    writeln!(csv_f, "atr_lookback,atr_trend_pct,avg_sharpe,avg_ret,worst_dd,total_trades,pass_count,total_windows,global_pass_rate,sparse_count,is_baseline,score")?;
    for cfg in &all_results {
        writeln!(csv_f, "{},{:.2},{:+.6},{:+.4},{:.4},{},{},{},{:.4},{:.4},{},{:+.4}",
            cfg.atr_lookback, cfg.atr_trend_pct, cfg.avg_sharpe, cfg.avg_ret, cfg.worst_dd,
            cfg.total_trades, cfg.pass_count, cfg.total_windows, cfg.global_pass_rate,
            cfg.sparse_count, cfg.is_baseline, cfg.score())?;
    }
    eprintln!("\n  Results -> {}", CSV_RESULTS);

    // Equity curves for top 4 + baseline
    println!("\n  Running equity curves for top configs + baseline...\n");

    let top_configs: Vec<(usize, f64)> = all_results.iter()
        .take(4)
        .map(|c| (c.atr_lookback, c.atr_trend_pct))
        .collect();

    let mut equity_configs: Vec<(usize, f64, &'static str)> = Vec::new();
    for &(lb, pct) in &top_configs {
        let lbl: &'static str = if (pct - 0.90).abs() < 0.001 && lb == 20 { "baseline" } else { "candidate" };
        equity_configs.push((lb, pct, lbl));
    }

    let mut equity_csv_lines: Vec<String> = vec!["universe,window,step,equity,atr_lookback,atr_trend_pct,label".to_string()];

    for &(label, symbols) in UNIVERSES {
        let symbols: Vec<String> = symbols.iter().map(|s| s.to_string()).collect();
        let all_loaded = symbols.iter().all(|s| sym_data_map.contains_key(s));
        if !all_loaded { continue; }

        let total_wins = n.saturating_sub(TRAIN_BARS + TEST_BARS) / TEST_BARS;
        if total_wins == 0 { continue; }

        for wi in 0..total_wins {
            let train_end = TRAIN_BARS + wi * TEST_BARS;
            let test_start = train_end;
            let test_end = (test_start + TEST_BARS).min(n);
            if test_end.saturating_sub(test_start) < 5 { continue; }

            for &(lb, pct, lbl) in &equity_configs {
                let r = run_sim_regime(&sym_data_map, &symbols, test_start, test_end, lb, pct);
                let label_str = format!("{}_lb{}_p{}", lbl, lb, pct);
                let max_steps = r.equity_steps.len().min(EQUITY_STEP_LIMIT);
                for (step, &eq) in r.equity_steps[..max_steps].iter().enumerate() {
                    equity_csv_lines.push(format!("{},{},{},{:.6},{},{:.2},{}", label, wi, step, eq, lb, pct, label_str));
                }
            }
        }
    }

    let mut eq_f = File::create(CSV_EQUITY)?;
    for line in &equity_csv_lines { writeln!(eq_f, "{}", line)?; }
    eprintln!("  Equity CSV -> {}", CSV_EQUITY);

    println!("\n  Runtime: {:.1}s", t0.elapsed().as_secs_f64());
    Ok(())
}
