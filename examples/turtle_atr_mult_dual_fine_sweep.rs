//! TURTLE_ATR_MULT Fine Hyperopt: All 9 Universes
//!
//! EP=21 (from 2026-04-10 full grid search)
//! Dense sweep of Turtle ATR exit multiplier M=0.5..5.0 step 0.1 under current production params
//! Walk-forward: 252-bar train / 252-bar test
//! Fees: 0.1% taker each side, min 3 trades per window

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
const HOLD_MAX: usize = 12; // hyperopt 2026-04-21: HM=12 wins +71.4% Sharpe vs HM=45 baseline (2.72 vs 1.59 avg Sharpe, 9-universe × 54 windows). Full sweep 19 values [5-180] with production params CHAND(11,2.25)/EP=24. HM=45 plateau: all HM≥35 produce IDENTICAL results (Chandelier fires first ~bar 12-15). HM=12 is tighter, exits before Chandelier in edge cases, better Sharpe. Pass rate: 96.3% vs 92.6% (+3.7pp). Update: 2026-04-21. See memory/hyperopt-2026-04-21-hold-max.md.
const TAKER_FEE: f64 = 0.001;
const POSITION_CAP: usize = 3; // hyperopt 2026-04-11: CAP=3 wins over CAP=2 (+1.4% Sharpe, +13pp pass rate, 91% vs 78%)
// MIN_TRADES=3: hyperopt 2026-04-16 sweep across 14 values × 9 universes. MT=1-6 produce IDENTICAL
// results (81.5% pass, Sharpe 4.455). MT=3 sits safely in the middle of the plateau.
// Degradation begins at MT=7 (−1.9pp), MT=10 (−11.1pp), MT=15 (−53.7pp).
// Only window sensitive to MT=3 vs MT=7 is Legacy5BNB W04 (6 trades → PASS at MT=3, FAIL at MT=7).
// MIN_TRADES does NOT affect strategy returns — only pass/fail label and statistical reliability.
const MIN_TRADES: usize = 3;
const CHAND_PERIOD: usize = 7; // hyperopt 2026-04-21: EXTENSIVE sweep CP∈[5..60 step 2] × 9 universes × 54 windows with current production params (HM=12, ATR_EM=0.90, EP=24, CM=2.25). CP=7 wins global Sharpe 5.908 (+6.9% vs CP=11 baseline 5.526). Pass rate 79.6% (identical). Prior CP=11 sweep used stale EP=21 (not current EP=24). See memory/hyperopt-2026-04-21-chand-period.md.
// Extended fine-sweep 2026-04-16: CP=15-50 step=1 on Base5. CP=17 emerged with best Sharpe (5.83 vs CP=28=5.76 on Base5).
// However, 9-way robustness check (all 9 universes): CP=20 slightly edges CP=17 (45/54=83% vs 44/54=81% global pass, Sharpe 4.51 vs 4.38).
// CP=17 and CP=28 are nearly identical globally (44/54=81% both, Sharpe 4.38 vs 4.46).
// Production keeps P=28 (already frozen, stable). CP=17 is a viable alternative if a future re-tune is desired.
const CHAND_MULT: f64 = 2.30; // hyperopt 2026-04-25: DENSE sweep M∈[1.50..5.00] step 0.05 (71 values) × 9 universes × 54 windows. M=2.30 wins: Sharpe 6.2036 (+0.8% vs M=2.25 at 6.1225), 83.3% pass (45/54) vs 81.5% (44/54). Lowest M at peak pass rate — most efficient setting. See memory/hyperopt-2026-04-25-chand-mult-dense.md.
const TURTLE_ENTRY: usize = 21; // REVERTED 2026-04-26: EP=24 was in-sample inflation. Paired held-out: EP=21 27/29 pass / Sharpe 0.18 vs EP=24 25/29 pass / 0.16. See snapshots/t3_ep_paired_held_out.csv. Prior EP sweep (2026-04-20) used stale CHAND(11,2.25) and found EP=24 winner. Revert to EP=21 as documented.
const TURTLE_ATR_PERIOD: usize = 24; // hyperopt 2026-04-16: ATR=24 wins (+3.6% Sharpe, -10.8pp DD vs ATR=25). Fine sweep 18-35 step=1, 18 values × 9 universes × 54 windows. 7/9 universes agree. Dual exit: Chandelier OR Turtle ATR fires first. See hyperopt-2026-04-16-atr-period.md.
const ATR_ENTRY_MULT: f64 = 0.00; // REVERTED 2026-04-25: 41-value sweep {0.00-2.00 step 0.05} × 9 universes × 54 windows with CHAND(7,2.30)/EP=24/HM=12. EM=0.00 wins: 83.3% pass, Sharpe 1.87, +151.9% return, 707 trades. Prior EM=0.85 (2026-04-21) was in-sample inflation — both EP=24 and EM=0.85 were optimized on the same OOS validation data. EM=0.00 is the production default.
// hyperopt 2026-04-28: DENSE re-sweep VL∈[1..100 step 1] × 9 universes × 6 windows on CURRENT production
// params (CHAND_P=7, CHAND_M=2.30, EP=21, HM=12, ATR=24). VL=8 is the robustness winner:
// 40/54 pass, 9/9 positive universes, avg Sharpe 3.147, Base5 6/6. Prior VL=9 remains very close
// (40/54, Sharpe 3.112, Base5 6/6), so the real finding is a stable plateau around 7-9 with 8 as the
// best default. See snapshots/vl_extensive_current_params_summary.csv and charts/comparison_chart.png.
const VOL_LOOKBACK: usize = 8; // dollar-volume smoothing window (rolling SMA of vol*price)
// ATR_EMA_PERIOD: EMA smoothing of Chandelier ATR values. ATR_EMA=1 = raw SMA ATR (baseline).
// Extensively swept 2026-04-29: ATR_EMA ∈ [1..200] step 1 × 9 universes × 54 windows.
// Result: NULL. ATR_EMA=4 wins pass rate (+1 window) but ATR_EMA=1 wins Sharpe (4.12 vs 3.76).
// ATR_EMA=1 (raw ATR) confirmed as production default — simplest mechanism, best Sharpe.
// See memory/hyperopt-2026-04-29-atr-ema.md, charts/atr_ema_comparison.png.
const ATR_EMA_PERIOD: usize = 1;
const BASELINE_TURTLE_ATR_MULT: f64 = 2.00; // baseline from coarse 2026-04-12 sweep; this fine sweep tests M=0.5..5.0 step 0.1 under current production params.

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

const CSV_METRICS: &str = "snapshots/turtle_atr_mult_dual_fine_sweep.csv";
const CSV_SUMMARY: &str = "snapshots/turtle_atr_mult_dual_fine_summary.csv";
const CSV_EQUITY: &str = "snapshots/turtle_atr_mult_dual_fine_equity.csv";
const MD_OUT: &str = "snapshots/turtle_atr_mult_dual_fine_report.md";

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
        let h = high.get(i).copied().unwrap_or(0.0);
        let l = low.get(i).copied().unwrap_or(0.0);
        let c0 = close.get(i.saturating_sub(1)).copied().unwrap_or(0.0);
        trs.push((h - l).max((h - c0).abs()).max((l - c0).abs()));
    }
    if trs.is_empty() { return 0.0; }
    trs.iter().sum::<f64>() / period as f64
}

fn rolling_avg(vals: &[f64], window: usize, idx: usize) -> f64 {
    if idx < window { return *vals.get(idx).unwrap_or(&0.0); }
    let start = idx + 1 - window;
    vals[start..=idx].iter().sum::<f64>() / window as f64
}

fn turtle_signal(close: &[f64], high: &[f64], low: &[f64], entry_period: usize, atr_period: usize, atr_mult: f64, idx: usize) -> bool {
    if idx < entry_period + 1 { return false; }
    let start = idx + 1 - entry_period;
    let mut max_close = f64::NEG_INFINITY;
    for i in start..idx {
        if let Some(&c) = close.get(i) { max_close = max_close.max(c); }
    }
    if let Some(&curr_close) = close.get(idx) {
        let breakout = curr_close > max_close;
        if breakout && atr_mult > 0.0 {
            // ATR momentum filter: require close >= max_close + ATR(atr_period) * atr_mult
            let atr_val = atr_at(high, low, close, atr_period, idx);
            return curr_close >= max_close + atr_mult * atr_val;
        }
        breakout
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

#[derive(Clone)]
struct WfResult {
    ret: f64,
    sharpe: f64,
    max_dd: f64,
    trades: usize,
    win_rate: f64,
    pass: bool,
    equity_curve: Vec<f64>,
}

fn run_sim(
    sym_data: &HashMap<String, SymData>,
    symbols: &[String],
    test_start: usize,
    test_end: usize,
    turtle_atr_mult: f64,
) -> WfResult {
    let mut equity = 1.0_f64;
    let mut equity_curve = vec![1.0_f64];
    let mut peak = equity;
    let mut wins = 0usize;
    let mut total_trades = 0usize;
    let mut daily_rets = Vec::new();

    let mut bar = test_start;
    while bar + 2 < test_end {
        // Rank symbols by dollar volume
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

        // Turtle breakout entry (no regime filter)
        let mut entered = false;
        for sym in &top_syms {
            if let Some(sd) = sym_data.get(sym) {
                if bar >= TURTLE_ENTRY + 1 && bar < sd.close.len() {
                    if turtle_signal(&sd.close, &sd.high, &sd.low, TURTLE_ENTRY, TURTLE_ATR_PERIOD, ATR_ENTRY_MULT, bar) {
                        let entry_px = sd.close[bar];
                        let entry = entry_px * (1.0 - TAKER_FEE);
                        let entry_bar_next = bar + 1;
                        let n = sd.close.len();

                        // DUAL_EXIT: Chandelier ATR OR Turtle ATR — whichever fires first
                        // Chandelier: trailing max_high - M * ATR(chand_period)
                        // Turtle ATR: trailing min_low - M * ATR(turtle_atr_period)
                        let mut highest_high_chand = sd.high[entry_bar_next];
                        let mut lowest_low_turtle = sd.low[entry_bar_next];
                        let max_bar = (entry_bar_next + HOLD_MAX).min(n.saturating_sub(1));
                        let mut exit_bar = max_bar;
                        for b in entry_bar_next..=max_bar.min(n.saturating_sub(1)) {
                            // Chandelier ATR
                            highest_high_chand = highest_high_chand.max(sd.high[b]);
                            let atr_chand = atr_at(&sd.high, &sd.low, &sd.close, CHAND_PERIOD, b);
                            let trail_chand = highest_high_chand - CHAND_MULT * atr_chand;
                            // Turtle ATR trailing stop: lowest_low - TURTLE_ATR_MULT * ATR(turtle_period)
                            lowest_low_turtle = lowest_low_turtle.min(sd.low[b]);
                            let atr_turtle = atr_at(&sd.high, &sd.low, &sd.close, TURTLE_ATR_PERIOD, b);
                            let trail_turtle = lowest_low_turtle - turtle_atr_mult * atr_turtle;
                            // Exit on EITHER stop
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
    eprintln!("==== TURTLE_ATR_MULT Fine Sweep: 9 Universes ====");
    eprintln!("Range: M=0.5..5.0 step=0.1 (46 values); baseline M={:.2}", BASELINE_TURTLE_ATR_MULT);
    eprintln!("Current params: EP={}, CHAND({}, {:.2}), ATR_P={}, HM={}, VL={}\n", TURTLE_ENTRY, CHAND_PERIOD, CHAND_MULT, TURTLE_ATR_PERIOD, HOLD_MAX, VOL_LOOKBACK);

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

    let atr_mults: Vec<f64> = (5..=50).map(|i| i as f64 / 10.0).collect();
    let total_windows = n.saturating_sub(TRAIN_BARS + TEST_BARS) / TEST_BARS;

    let mut metrics_lines = vec!["atr_mult,universe,window,return_pct,sharpe,max_dd_pct,trades,win_rate_pct,pass".to_string()];
    let mut summary_lines = vec!["atr_mult,pass_count,total_count,pass_rate_pct,positive_universes,avg_sharpe,avg_ret_pct,worst_dd_pct,total_trades,avg_win_rate_pct".to_string()];
    let mut equity_lines = vec!["atr_mult,bar,equity".to_string()];

    for &mult in &atr_mults {
        eprintln!("-- M={:.1} --", mult);
        let mut records: Vec<(String, usize, WfResult)> = Vec::new();
        let mut pass_count = 0usize;
        let mut total_count = 0usize;
        let mut total_trades = 0usize;
        let mut universe_avg_rets: HashMap<String, f64> = HashMap::new();
        let mut base5_bar = 0usize;
        let mut base5_global_equity = 1.0_f64;

        for &(label, symbols) in UNIVERSES {
            let symbols: Vec<String> = symbols.iter().map(|s| s.to_string()).collect();
            if !symbols.iter().all(|s| sym_data_map.contains_key(s)) { continue; }
            let mut uni_ret_sum = 0.0_f64;
            let mut uni_windows = 0usize;

            for wi in 0..total_windows {
                let train_end = TRAIN_BARS + wi * TEST_BARS;
                let test_start = train_end;
                let test_end = (test_start + TEST_BARS).min(n);
                if test_end.saturating_sub(test_start) < 5 { continue; }

                let r = run_sim(&sym_data_map, &symbols, test_start, test_end, mult);
                metrics_lines.push(format!(
                    "{:.1},{},{},{:.4},{:.6},{:.4},{},{:.4},{}",
                    mult, label, wi, r.ret, r.sharpe, r.max_dd, r.trades, r.win_rate, r.pass
                ));
                if r.pass { pass_count += 1; }
                total_count += 1;
                total_trades += r.trades;
                uni_ret_sum += r.ret;
                uni_windows += 1;

                // Export a correctly compounded Base5 time-series equity curve for every tested multiplier.
                // Each WF window starts at 1.0 internally; compound by anchoring the whole window
                // to the prior global equity, then update the anchor only after the window ends.
                if label == "Base5" {
                    let window_anchor = base5_global_equity;
                    for &rel_eq in r.equity_curve.iter().skip(1) {
                        let eq = window_anchor * rel_eq;
                        equity_lines.push(format!("{:.1},{},{:.10}", mult, base5_bar, eq));
                        base5_bar += 1;
                    }
                    base5_global_equity = window_anchor * r.equity_curve.last().copied().unwrap_or(1.0);
                }

                records.push((label.to_string(), wi, r));
            }
            if uni_windows > 0 { universe_avg_rets.insert(label.to_string(), uni_ret_sum / uni_windows as f64); }
        }

        let avg_sharpe = records.iter().map(|(_,_,r)| r.sharpe).sum::<f64>() / records.len().max(1) as f64;
        let avg_ret = records.iter().map(|(_,_,r)| r.ret).sum::<f64>() / records.len().max(1) as f64;
        let worst_dd = records.iter().map(|(_,_,r)| r.max_dd).fold(0.0_f64, |a,b| a.max(b));
        let avg_wr = records.iter().map(|(_,_,r)| r.win_rate).sum::<f64>() / records.len().max(1) as f64;
        let positive_universes = universe_avg_rets.values().filter(|&&x| x > 0.0).count();
        let pass_rate = pass_count as f64 / total_count.max(1) as f64 * 100.0;
        summary_lines.push(format!(
            "{:.1},{},{},{:.4},{},{:.6},{:.4},{:.4},{},{:.4}",
            mult, pass_count, total_count, pass_rate, positive_universes, avg_sharpe, avg_ret, worst_dd, total_trades, avg_wr
        ));
        eprintln!("   pass={}/{} ({:.1}%), avg_sh={:.3}, avg_ret={:+.1}%, worstDD={:.1}%", pass_count, total_count, pass_rate, avg_sharpe, avg_ret, worst_dd);
    }

    let mut f = File::create(CSV_METRICS)?;
    for line in &metrics_lines { writeln!(f, "{}", line)?; }
    let mut f = File::create(CSV_SUMMARY)?;
    for line in &summary_lines { writeln!(f, "{}", line)?; }
    let mut f = File::create(CSV_EQUITY)?;
    for line in &equity_lines { writeln!(f, "{}", line)?; }

    let mut md = File::create(MD_OUT)?;
    writeln!(md, "# TURTLE_ATR_MULT Fine Sweep")?;
    writeln!(md, "")?;
    writeln!(md, "Range: M=0.5..5.0 step=0.1 (46 values), 9 universes × {} walk-forward windows.", total_windows)?;
    writeln!(md, "Current production baseline: M={:.2}.", BASELINE_TURTLE_ATR_MULT)?;
    writeln!(md, "Outputs: `{}`, `{}`, `{}`.", CSV_METRICS, CSV_SUMMARY, CSV_EQUITY)?;

    eprintln!("\nCSV metrics: {}", CSV_METRICS);
    eprintln!("CSV summary: {}", CSV_SUMMARY);
    eprintln!("CSV equity: {}", CSV_EQUITY);
    eprintln!("MD: {}", MD_OUT);
    eprintln!("Runtime: {:?}", t0.elapsed());
    Ok(())
}
