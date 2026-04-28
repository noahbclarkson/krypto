//! Extensive VOL_LOOKBACK hyperopt on CURRENT production Turtle params.
//!
//! Purpose: audit the hardcoded dollar-volume smoothing window used by the
//! validated walk-forward harness. Prior checks used either stale strategy
//! params or sparse VL ranges. This harness runs the full logical integer
//! range VL=1..100 on the current production config.
//!
//! Fixed params:
//! - CHAND(7, 2.30)
//! - EP=21
//! - ATR(24, 2.0)
//! - ATR_ENTRY_MULT=0.00
//! - HOLD_MAX=12
//! - POSITION_CAP=3
//! - MIN_TRADES=3
//!
//! Validation: 9 universes × 6 walk-forward windows = 54 OOS windows / VL.
//! Selection rule: robustness-first = pass_count > positive_universes >
//! avg_sharpe > lower avg_dd.
//!
//! Outputs:
//! - snapshots/vl_extensive_current_params_sweep.csv
//! - snapshots/vl_extensive_current_params_summary.csv
//! - snapshots/vl_extensive_selected_equity.csv
//! - snapshots/vl_extensive_aggregate_equity.csv

use anyhow::Result;
use krypto::data::loader::DataLoader;
use polars::prelude::*;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::fs::File;
use std::io::Write;
use std::time::Instant;

const CANDLES: u32 = 3000;
const TRAIN_BARS: usize = 252;
const TEST_BARS: usize = 252;
const HOLD_MAX: usize = 12;
const TAKER_FEE: f64 = 0.001;
const POSITION_CAP: usize = 3;
const MIN_TRADES: usize = 3;
const CHAND_PERIOD: usize = 7;
const CHAND_MULT: f64 = 2.30;
const EP: usize = 21;
const ATR_PERIOD: usize = 24;
const ATR_MULT: f64 = 2.0;
const ATR_ENTRY_MULT: f64 = 0.00;
const BASELINE_VL: usize = 9;
const VL_MIN: usize = 1;
const VL_MAX: usize = 100;

const UNIVERSES: &[(&str, &[&str])] = &[
    ("Base5",         &["BTCUSDT", "ETHUSDT", "SOLUSDT", "XRPUSDT", "DOGEUSDT", "ADAUSDT"]),
    ("NoDOGE",        &["BTCUSDT", "ETHUSDT", "SOLUSDT", "XRPUSDT", "ADAUSDT"]),
    ("Legacy4",       &["BTCUSDT", "ETHUSDT", "XRPUSDT", "LTCUSDT", "EOSUSDT"]),
    ("Legacy5BNB",    &["BTCUSDT", "ETHUSDT", "XRPUSDT", "LTCUSDT", "BNBUSDT", "EOSUSDT"]),
    ("OldGuardNoBNB", &["BTCUSDT", "ETHUSDT", "XRPUSDT", "LTCUSDT", "EOSUSDT", "BCHUSDT"]),
    ("LargeCaps5",    &["BTCUSDT", "ETHUSDT", "SOLUSDT", "XRPUSDT", "BNBUSDT", "ADAUSDT"]),
    ("Legacy3",       &["BTCUSDT", "XRPUSDT", "LTCUSDT", "EOSUSDT"]),
    ("LowVolume5",    &["XRPUSDT", "LTCUSDT", "EOSUSDT", "BCHUSDT", "ADAUSDT"]),
    ("OldGuard4",     &["BTCUSDT", "XRPUSDT", "LTCUSDT", "EOSUSDT", "BCHUSDT"]),
];

#[derive(Clone)]
struct SymData {
    close: Vec<f64>,
    high: Vec<f64>,
    low: Vec<f64>,
    vol: Vec<f64>,
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

#[derive(Clone, Debug)]
struct SummaryRow {
    vl: usize,
    pass_count: usize,
    total: usize,
    positive_universes: usize,
    avg_sharpe: f64,
    avg_ret: f64,
    avg_dd: f64,
    total_trades: usize,
    base5_pass: usize,
    base5_total: usize,
    base5_avg_sharpe: f64,
    base5_avg_ret: f64,
}

fn atr_at(high: &[f64], low: &[f64], close: &[f64], period: usize, idx: usize) -> f64 {
    if idx < period {
        return 0.0;
    }
    let mut trs = Vec::with_capacity(period);
    for i in (idx + 1 - period)..=idx {
        let h = high.get(i).copied().unwrap_or(0.0);
        let l = low.get(i).copied().unwrap_or(0.0);
        let c0 = close.get(i.saturating_sub(1)).copied().unwrap_or(0.0);
        trs.push((h - l).max((h - c0).abs()).max((l - c0).abs()));
    }
    if trs.is_empty() {
        return 0.0;
    }
    trs.iter().sum::<f64>() / period as f64
}

fn rolling_avg(vals: &[f64], window: usize, idx: usize) -> f64 {
    if idx < window {
        return *vals.get(idx).unwrap_or(&0.0);
    }
    let start = idx + 1 - window;
    vals[start..=idx].iter().sum::<f64>() / window as f64
}

fn turtle_signal(
    close: &[f64],
    high: &[f64],
    low: &[f64],
    entry_period: usize,
    atr_period: usize,
    atr_mult: f64,
    idx: usize,
) -> bool {
    if idx < entry_period + 1 {
        return false;
    }
    let start = idx + 1 - entry_period;
    let mut max_close = f64::NEG_INFINITY;
    for i in start..idx {
        if let Some(&c) = close.get(i) {
            max_close = max_close.max(c);
        }
    }
    if let Some(&curr_close) = close.get(idx) {
        let breakout = curr_close > max_close;
        if breakout && atr_mult > 0.0 {
            let atr_val = atr_at(high, low, close, atr_period, idx);
            return curr_close >= max_close + atr_mult * atr_val;
        }
        breakout
    } else {
        false
    }
}

fn annualised_sharpe(daily_rets: &[f64]) -> f64 {
    if daily_rets.len() < 2 {
        return 0.0;
    }
    let mn = daily_rets.iter().sum::<f64>() / daily_rets.len() as f64;
    let sd = (daily_rets
        .iter()
        .map(|x| (x - mn).powi(2))
        .sum::<f64>()
        / daily_rets.len() as f64)
        .sqrt();
    if sd == 0.0 {
        return 0.0;
    }
    mn * 365.0_f64.sqrt() / sd
}

fn max_dd_from(equity: &[f64]) -> f64 {
    let mut peak = f64::NEG_INFINITY;
    let mut max_dd = 0.0;
    for &e in equity {
        if e > peak {
            peak = e;
        }
        let dd = (peak - e) / peak;
        if dd > max_dd {
            max_dd = dd;
        }
    }
    max_dd * 100.0
}

fn run_sim(
    sym_data: &HashMap<String, SymData>,
    symbols: &[String],
    vol_lookback: usize,
    test_start: usize,
    test_end: usize,
) -> WfResult {
    let mut equity = 1.0_f64;
    let mut equity_curve = vec![1.0_f64];
    let mut wins = 0usize;
    let mut total_trades = 0usize;
    let mut daily_rets = Vec::new();

    let mut bar = test_start;
    while bar + 2 < test_end {
        let mut scores: Vec<(&str, f64)> = Vec::new();
        for sym in symbols {
            if let Some(sd) = sym_data.get(sym) {
                if bar >= sd.close.len() {
                    continue;
                }
                let rol_vol = rolling_avg(&sd.vol, vol_lookback, bar);
                let price = sd.close.get(bar).copied().unwrap_or(0.0);
                let dv = rol_vol * price;
                scores.push((sym.as_str(), if dv.is_finite() && dv > 0.0 { dv } else { 0.0 }));
            }
        }
        scores.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());
        let top_syms: Vec<String> = scores
            .into_iter()
            .take(POSITION_CAP)
            .map(|(s, _)| s.to_string())
            .collect();

        if top_syms.is_empty() {
            equity_curve.push(equity);
            bar += 1;
            continue;
        }

        let mut entered = false;
        for sym in &top_syms {
            if let Some(sd) = sym_data.get(sym) {
                if bar >= EP + 1 && bar < sd.close.len() {
                    if turtle_signal(&sd.close, &sd.high, &sd.low, EP, ATR_PERIOD, ATR_ENTRY_MULT, bar) {
                        let entry_px = sd.close[bar];
                        let entry = entry_px * (1.0 - TAKER_FEE);
                        let entry_bar_next = bar + 1;
                        let n = sd.close.len();

                        let mut highest_high_chand = sd.high[entry_bar_next];
                        let mut lowest_low_turtle = sd.low[entry_bar_next];
                        let max_bar = (entry_bar_next + HOLD_MAX).min(n.saturating_sub(1));
                        let mut exit_bar = max_bar;

                        for b in entry_bar_next..=max_bar.min(n.saturating_sub(1)) {
                            highest_high_chand = highest_high_chand.max(sd.high[b]);
                            let atr_chand = atr_at(&sd.high, &sd.low, &sd.close, CHAND_PERIOD, b);
                            let trail_chand = highest_high_chand - CHAND_MULT * atr_chand;

                            lowest_low_turtle = lowest_low_turtle.min(sd.low[b]);
                            let atr_turtle = atr_at(&sd.high, &sd.low, &sd.close, ATR_PERIOD, b);
                            let trail_turtle = lowest_low_turtle - ATR_MULT * atr_turtle;

                            if sd.close[b] < trail_chand || sd.close[b] < trail_turtle {
                                exit_bar = b;
                                break;
                            }
                        }

                        if let Some(&exit_px) = sd.close.get(exit_bar) {
                            let exit = exit_px * (1.0 - TAKER_FEE);
                            let gross_ret = exit / entry - 1.0;
                            let bars_held = (exit_bar as i64 - entry_bar_next as i64).max(1) as usize;
                            wins += usize::from(gross_ret > 0.0);
                            total_trades += 1;
                            equity *= 1.0 + gross_ret;
                            let avg_daily = gross_ret / bars_held as f64;
                            for _ in 0..bars_held {
                                daily_rets.push(avg_daily);
                            }
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
    let win_rate = if total_trades > 0 {
        wins as f64 / total_trades as f64 * 100.0
    } else {
        0.0
    };
    let pass = total_trades >= MIN_TRADES && ret > 0.0;

    WfResult {
        ret,
        sharpe,
        max_dd,
        trades: total_trades,
        win_rate,
        pass,
        equity_curve,
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    let t0 = Instant::now();
    eprintln!("==== Extensive VOL_LOOKBACK hyperopt (current production params) ====");
    eprintln!("CHAND(7, 2.30), EP=21, HM=12, ATR(24, 2.0), ATR_ENTRY_MULT=0.00");
    eprintln!("VL sweep: {}..={} ({} values)", VL_MIN, VL_MAX, VL_MAX - VL_MIN + 1);

    let loader = DataLoader::new(None, None);
    let mut all_syms: HashSet<String> = HashSet::new();
    for (_, syms) in UNIVERSES {
        for &s in *syms {
            all_syms.insert(s.to_string());
        }
    }

    let mut raw_cache: HashMap<String, DataFrame> = HashMap::new();
    let mut min_len = usize::MAX;
    for sym in &all_syms {
        match loader.fetch_with_cache(sym.as_str(), "1d", CANDLES).await {
            Ok(df) => {
                min_len = min_len.min(df.height());
                raw_cache.insert(sym.clone(), df);
            }
            Err(e) => eprintln!("WARNING: {} load failed: {}", sym, e),
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
                    chunked
                        .into_iter()
                        .filter_map(|x| x)
                        .take(n_min)
                        .collect::<Vec<_>>()
                }};
            }
            sym_data_map.insert(
                sym.clone(),
                SymData {
                    close: col_vec!("close"),
                    high: col_vec!("high"),
                    low: col_vec!("low"),
                    vol: col_vec!("volume"),
                },
            );
        }
    }
    eprintln!("Loaded {} symbols, {} bars", sym_data_map.len(), n);

    let total_windows = n.saturating_sub(TRAIN_BARS + TEST_BARS) / TEST_BARS;
    eprintln!("Windows per universe: {}", total_windows);

    let mut results: BTreeMap<usize, HashMap<String, Vec<WfResult>>> = BTreeMap::new();
    for vl in VL_MIN..=VL_MAX {
        results.insert(vl, HashMap::new());
    }

    for &(label, symbols) in UNIVERSES {
        let symbols: Vec<String> = symbols.iter().map(|s| s.to_string()).collect();
        if !symbols.iter().all(|s| sym_data_map.contains_key(s)) {
            continue;
        }
        for vl in VL_MIN..=VL_MAX {
            for wi in 0..total_windows {
                let test_start = TRAIN_BARS + wi * TEST_BARS;
                let test_end = (test_start + TEST_BARS).min(n);
                if test_end.saturating_sub(test_start) < 5 {
                    continue;
                }
                let r = run_sim(&sym_data_map, &symbols, vl, test_start, test_end);
                results
                    .get_mut(&vl)
                    .unwrap()
                    .entry(label.to_string())
                    .or_default()
                    .push(r);
            }
        }
    }

    let mut sweep_lines = vec![
        "vl,universe,window,return_pct,sharpe,max_dd_pct,trades,win_rate_pct,pass".to_string(),
    ];
    let mut summary_lines = vec![
        "vl,pass_count,total,positive_universes,avg_sharpe,avg_ret,avg_dd,total_trades,base5_pass,base5_total,base5_avg_sharpe,base5_avg_ret".to_string(),
    ];
    let mut summary_rows: Vec<SummaryRow> = Vec::new();

    for vl in VL_MIN..=VL_MAX {
        let mut pass_count = 0usize;
        let mut total = 0usize;
        let mut sum_sharpe = 0.0_f64;
        let mut sum_ret = 0.0_f64;
        let mut sum_dd = 0.0_f64;
        let mut total_trades = 0usize;
        let mut positive_universes = 0usize;
        let mut base5_pass = 0usize;
        let mut base5_total = 0usize;
        let mut base5_sum_sharpe = 0.0_f64;
        let mut base5_sum_ret = 0.0_f64;

        if let Some(universe_map) = results.get(&vl) {
            for (universe, windows) in universe_map {
                let mut universe_ret_sum = 0.0_f64;
                for (wi, r) in windows.iter().enumerate() {
                    sweep_lines.push(format!(
                        "{},{},{},{:.2},{:.4},{:.2},{},{:.2},{}",
                        vl, universe, wi, r.ret, r.sharpe, r.max_dd, r.trades, r.win_rate, r.pass
                    ));
                    pass_count += usize::from(r.pass);
                    total += 1;
                    sum_sharpe += r.sharpe;
                    sum_ret += r.ret;
                    sum_dd += r.max_dd;
                    total_trades += r.trades;
                    universe_ret_sum += r.ret;

                    if universe == "Base5" {
                        base5_pass += usize::from(r.pass);
                        base5_total += 1;
                        base5_sum_sharpe += r.sharpe;
                        base5_sum_ret += r.ret;
                    }
                }
                if !windows.is_empty() && universe_ret_sum / windows.len() as f64 > 0.0 {
                    positive_universes += 1;
                }
            }
        }

        let denom = total.max(1) as f64;
        let base5_denom = base5_total.max(1) as f64;
        let row = SummaryRow {
            vl,
            pass_count,
            total,
            positive_universes,
            avg_sharpe: sum_sharpe / denom,
            avg_ret: sum_ret / denom,
            avg_dd: sum_dd / denom,
            total_trades,
            base5_pass,
            base5_total,
            base5_avg_sharpe: base5_sum_sharpe / base5_denom,
            base5_avg_ret: base5_sum_ret / base5_denom,
        };
        summary_lines.push(format!(
            "{},{},{},{},{:.4},{:.2},{:.2},{},{},{},{:.4},{:.2}",
            row.vl,
            row.pass_count,
            row.total,
            row.positive_universes,
            row.avg_sharpe,
            row.avg_ret,
            row.avg_dd,
            row.total_trades,
            row.base5_pass,
            row.base5_total,
            row.base5_avg_sharpe,
            row.base5_avg_ret
        ));
        summary_rows.push(row);
    }

    summary_rows.sort_by(|a, b| {
        b.pass_count
            .cmp(&a.pass_count)
            .then(b.positive_universes.cmp(&a.positive_universes))
            .then_with(|| b.avg_sharpe.partial_cmp(&a.avg_sharpe).unwrap())
            .then_with(|| a.avg_dd.partial_cmp(&b.avg_dd).unwrap())
    });

    let winner = summary_rows.first().cloned().unwrap();
    let baseline = summary_rows
        .iter()
        .find(|r| r.vl == BASELINE_VL)
        .cloned()
        .unwrap();

    let mut selected_vls = vec![BASELINE_VL];
    if !selected_vls.contains(&winner.vl) {
        selected_vls.push(winner.vl);
    }
    for row in &summary_rows {
        if !selected_vls.contains(&row.vl) {
            selected_vls.push(row.vl);
        }
        if selected_vls.len() >= 4 {
            break;
        }
    }
    selected_vls.sort_unstable();

    let mut selected_equity_lines = vec!["vl,universe,window,step,equity".to_string()];
    let mut aggregate_equity_lines = vec!["step".to_string()];
    for vl in &selected_vls {
        aggregate_equity_lines[0].push_str(&format!(",vl_{}", vl));
    }

    let mut aggregate_map: BTreeMap<usize, Vec<f64>> = BTreeMap::new();
    for &vl in &selected_vls {
        let mut agg = vec![1.0_f64];
        if let Some(universe_map) = results.get(&vl) {
            let mut universe_names: Vec<_> = universe_map.keys().cloned().collect();
            universe_names.sort();
            for universe in universe_names {
                if let Some(windows) = universe_map.get(&universe) {
                    for (wi, r) in windows.iter().enumerate() {
                        for (step, &eq) in r.equity_curve.iter().enumerate() {
                            selected_equity_lines.push(format!(
                                "{},{},{},{},{}",
                                vl, universe, wi, step, eq
                            ));
                        }
                        let anchor = *agg.last().unwrap_or(&1.0);
                        for &eq in r.equity_curve.iter().skip(1) {
                            agg.push(anchor * eq);
                        }
                    }
                }
            }
        }
        aggregate_map.insert(vl, agg);
    }

    let max_steps = aggregate_map.values().map(|v| v.len()).max().unwrap_or(0);
    for step in 0..max_steps {
        let mut row = vec![step.to_string()];
        for &vl in &selected_vls {
            let agg = aggregate_map.get(&vl).unwrap();
            let eq = if step < agg.len() {
                agg[step]
            } else {
                *agg.last().unwrap()
            };
            row.push(format!("{:.8}", eq));
        }
        aggregate_equity_lines.push(row.join(","));
    }

    let mut f = File::create("snapshots/vl_extensive_current_params_sweep.csv")?;
    for line in &sweep_lines {
        writeln!(f, "{}", line)?;
    }

    let mut g = File::create("snapshots/vl_extensive_current_params_summary.csv")?;
    for line in &summary_lines {
        writeln!(g, "{}", line)?;
    }

    let mut h = File::create("snapshots/vl_extensive_selected_equity.csv")?;
    for line in &selected_equity_lines {
        writeln!(h, "{}", line)?;
    }

    let mut i = File::create("snapshots/vl_extensive_aggregate_equity.csv")?;
    for line in &aggregate_equity_lines {
        writeln!(i, "{}", line)?;
    }

    eprintln!("\nTop 10 robustness ranking:");
    for row in summary_rows.iter().take(10) {
        eprintln!(
            "VL={:>3} | pass={:>2}/{} | pos_univ={}/9 | sh={:>6.3} | ret={:>7.2}% | dd={:>6.2}% | Base5={}/{}",
            row.vl,
            row.pass_count,
            row.total,
            row.positive_universes,
            row.avg_sharpe,
            row.avg_ret,
            row.avg_dd,
            row.base5_pass,
            row.base5_total,
        );
    }

    eprintln!("\nBaseline VL={}: pass {}/{} | sh {:.4} | pos_univ {}/9 | Base5 {}/{}",
        baseline.vl,
        baseline.pass_count,
        baseline.total,
        baseline.avg_sharpe,
        baseline.positive_universes,
        baseline.base5_pass,
        baseline.base5_total,
    );
    eprintln!("Winner   VL={}: pass {}/{} | sh {:.4} | pos_univ {}/9 | Base5 {}/{}",
        winner.vl,
        winner.pass_count,
        winner.total,
        winner.avg_sharpe,
        winner.positive_universes,
        winner.base5_pass,
        winner.base5_total,
    );
    eprintln!("Selected curves: {:?}", selected_vls);
    eprintln!("Runtime: {:?}", t0.elapsed());

    Ok(())
}
