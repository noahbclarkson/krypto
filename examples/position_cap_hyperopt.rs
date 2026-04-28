//! POSITION_CAP hyperopt — current live Turtle-only strategy.
//!
//! Assumption under audit:
//!   POSITION_CAP=3 is hardcoded in live config, but the original sweep was
//!   done before the current Turtle-only exit became production.
//!
//! Goal:
//!   Sweep POSITION_CAP across the full logical range for the current 9-universe
//!   walk-forward harness, export actual equity curves, and rank candidates by
//!   robustness (pass rate first, then cross-universe agreement, then Sharpe).

use anyhow::Result;
use krypto::data::loader::DataLoader;
use polars::prelude::*;
use std::collections::{HashMap, HashSet};
use std::fs::File;
use std::io::Write;
use std::time::Instant;

const CANDLES: u32 = 3000;
const TRAIN_BARS: usize = 252;
const TEST_BARS: usize = 252;
const HOLD_MAX: usize = 12;
const TAKER_FEE: f64 = 0.001;
const MIN_TRADES: usize = 3;
const TURTLE_ENTRY: usize = 21;
const TURTLE_ATR_PERIOD: usize = 24;
const TURTLE_ATR_MULT: f64 = 2.00;
const ATR_ENTRY_MULT: f64 = 0.00;
const VOL_LOOKBACK: usize = 8;
const BASELINE_CAP: usize = 3;
const POSITION_CAPS: [usize; 10] = [1, 2, 3, 4, 5, 6, 7, 8, 9, 10];

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

const SUMMARY_CSV: &str = "snapshots/position_cap_sweep_summary.csv";
const DETAIL_CSV: &str = "snapshots/position_cap_sweep_detail.csv";
const ALL_EQUITY_CSV: &str = "snapshots/position_cap_all_equity.csv";
const SELECTED_EQUITY_CSV: &str = "snapshots/position_cap_selected_equity.csv";
const SUMMARY_JSON: &str = "snapshots/position_cap_sweep_summary.json";
const REPORT_MD: &str = "snapshots/position_cap_sweep_report.md";

#[derive(Clone)]
struct SymData {
    close: Vec<f64>,
    high: Vec<f64>,
    low: Vec<f64>,
    vol: Vec<f64>,
}

#[derive(Clone, Debug)]
struct WfResult {
    ret: f64,
    sharpe: f64,
    max_dd: f64,
    trades: usize,
    win_rate: f64,
    pass: bool,
}

#[derive(Clone, Debug)]
struct DetailRow {
    cap: usize,
    universe: String,
    window: usize,
    ret: f64,
    sharpe: f64,
    max_dd: f64,
    trades: usize,
    win_rate: f64,
    pass: bool,
}

#[derive(Clone, Debug)]
struct CapSummary {
    cap: usize,
    avg_sharpe: f64,
    median_sharpe: f64,
    avg_return: f64,
    avg_max_dd: f64,
    avg_win_rate: f64,
    pass_rate: f64,
    total_trades: usize,
    positive_universes: usize,
    base5_pass_rate: f64,
    windows: usize,
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
    trs.iter().sum::<f64>() / period as f64
}

fn rolling_avg(vals: &[f64], window: usize, idx: usize) -> f64 {
    if idx >= vals.len() {
        return 0.0;
    }
    if idx < window {
        vals[idx]
    } else {
        vals[idx + 1 - window..=idx].iter().sum::<f64>() / window as f64
    }
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
    let mean = daily_rets.iter().sum::<f64>() / daily_rets.len() as f64;
    let sd = (daily_rets
        .iter()
        .map(|x| (x - mean).powi(2))
        .sum::<f64>()
        / daily_rets.len() as f64)
        .sqrt();
    if sd == 0.0 {
        return 0.0;
    }
    mean * 365.0_f64.sqrt() / sd
}

fn max_dd_from(equity: &[f64]) -> f64 {
    let mut peak = f64::NEG_INFINITY;
    let mut max_dd = 0.0_f64;
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

fn median(mut values: Vec<f64>) -> f64 {
    if values.is_empty() {
        return 0.0;
    }
    values.sort_by(|a, b| a.total_cmp(b));
    let mid = values.len() / 2;
    if values.len() % 2 == 0 {
        (values[mid - 1] + values[mid]) / 2.0
    } else {
        values[mid]
    }
}

fn is_better(a: &CapSummary, b: &CapSummary) -> bool {
    a.pass_rate > b.pass_rate
        || (a.pass_rate == b.pass_rate && a.positive_universes > b.positive_universes)
        || (a.pass_rate == b.pass_rate
            && a.positive_universes == b.positive_universes
            && a.avg_sharpe > b.avg_sharpe)
        || (a.pass_rate == b.pass_rate
            && a.positive_universes == b.positive_universes
            && a.avg_sharpe == b.avg_sharpe
            && a.avg_max_dd < b.avg_max_dd)
        || (a.pass_rate == b.pass_rate
            && a.positive_universes == b.positive_universes
            && a.avg_sharpe == b.avg_sharpe
            && a.avg_max_dd == b.avg_max_dd
            && a.cap.abs_diff(BASELINE_CAP) < b.cap.abs_diff(BASELINE_CAP))
}

fn run_sim_for_cap(
    sym_data: &HashMap<String, SymData>,
    symbols: &[String],
    test_start: usize,
    test_end: usize,
    position_cap: usize,
) -> (WfResult, Vec<f64>) {
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
                let rol_vol = rolling_avg(&sd.vol, VOL_LOOKBACK, bar);
                let price = sd.close[bar];
                let dv = rol_vol * price;
                scores.push((sym.as_str(), if dv.is_finite() && dv > 0.0 { dv } else { 0.0 }));
            }
        }
        scores.sort_by(|a, b| b.1.total_cmp(&a.1));
        let top_syms: Vec<String> = scores
            .into_iter()
            .take(position_cap)
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
                if bar >= TURTLE_ENTRY + 1
                    && bar < sd.close.len()
                    && turtle_signal(
                        &sd.close,
                        &sd.high,
                        &sd.low,
                        TURTLE_ENTRY,
                        TURTLE_ATR_PERIOD,
                        ATR_ENTRY_MULT,
                        bar,
                    )
                {
                    let entry_px = sd.close[bar];
                    let entry = entry_px * (1.0 - TAKER_FEE);
                    let entry_bar_next = bar + 1;
                    let n = sd.close.len();

                    let mut lowest_low_turtle = sd.low[entry_bar_next];
                    let max_bar = (entry_bar_next + HOLD_MAX).min(n.saturating_sub(1));
                    let mut exit_bar = max_bar;
                    for b in entry_bar_next..=max_bar {
                        lowest_low_turtle = lowest_low_turtle.min(sd.low[b]);
                        let atr_turtle = atr_at(&sd.high, &sd.low, &sd.close, TURTLE_ATR_PERIOD, b);
                        let trail_turtle = lowest_low_turtle - TURTLE_ATR_MULT * atr_turtle;
                        if sd.close[b] < trail_turtle {
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

    (
        WfResult {
            ret,
            sharpe,
            max_dd,
            trades: total_trades,
            win_rate,
            pass,
        },
        equity_curve,
    )
}

fn write_json_summary(path: &str, summaries: &[CapSummary], winner: &CapSummary) -> Result<()> {
    let mut f = File::create(path)?;
    writeln!(f, "{{")?;
    writeln!(f, "  \"parameter\": \"POSITION_CAP\",")?;
    writeln!(f, "  \"baseline\": {},", BASELINE_CAP)?;
    writeln!(f, "  \"winner\": {},", winner.cap)?;
    writeln!(f, "  \"selection_rule\": \"pass_rate > positive_universes > avg_sharpe > lower_avg_max_dd\",")?;
    writeln!(f, "  \"values\": [")?;
    for (i, s) in summaries.iter().enumerate() {
        writeln!(
            f,
            "    {{\"cap\":{},\"avg_sharpe\":{:.6},\"median_sharpe\":{:.6},\"avg_return\":{:.6},\"avg_max_dd\":{:.6},\"avg_win_rate\":{:.6},\"pass_rate\":{:.6},\"total_trades\":{},\"positive_universes\":{},\"base5_pass_rate\":{:.6},\"windows\":{}}}{}",
            s.cap,
            s.avg_sharpe,
            s.median_sharpe,
            s.avg_return,
            s.avg_max_dd,
            s.avg_win_rate,
            s.pass_rate,
            s.total_trades,
            s.positive_universes,
            s.base5_pass_rate,
            s.windows,
            if i + 1 == summaries.len() { "" } else { "," }
        )?;
    }
    writeln!(f, "  ]")?;
    writeln!(f, "}}")?;
    Ok(())
}

#[tokio::main]
async fn main() -> Result<()> {
    let t0 = Instant::now();
    eprintln!("==== POSITION_CAP hyperopt: current Turtle-only strategy ====");
    eprintln!(
        "Sweep: {:?} | EP={} ATR({}, {}) HM={} VOL_LOOKBACK={} | baseline cap={}\n",
        POSITION_CAPS,
        TURTLE_ENTRY,
        TURTLE_ATR_PERIOD,
        TURTLE_ATR_MULT,
        HOLD_MAX,
        VOL_LOOKBACK,
        BASELINE_CAP,
    );

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
                    chunked.into_iter().filter_map(|x| x).take(n_min).collect::<Vec<_>>()
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
    eprintln!("Loaded {} symbols, {} bars\n", sym_data_map.len(), n);

    let mut detail_rows: Vec<DetailRow> = Vec::new();
    let mut results_by_cap: HashMap<usize, Vec<DetailRow>> = HashMap::new();
    let mut equity_segments_by_cap: HashMap<usize, Vec<Vec<f64>>> = HashMap::new();
    for &cap in &POSITION_CAPS {
        results_by_cap.insert(cap, Vec::new());
        equity_segments_by_cap.insert(cap, Vec::new());
    }

    for &(label, symbols) in UNIVERSES {
        let symbols: Vec<String> = symbols.iter().map(|s| s.to_string()).collect();
        if !symbols.iter().all(|s| sym_data_map.contains_key(s)) {
            eprintln!("{:>20} SKIPPED (missing data)", label);
            continue;
        }

        let total_windows = n.saturating_sub(TRAIN_BARS + TEST_BARS) / TEST_BARS;
        if total_windows == 0 {
            eprintln!("{:>20} SKIPPED (not enough data)", label);
            continue;
        }

        eprintln!("==== {:<18} ==== {} syms, {} windows", label, symbols.len(), total_windows);
        for wi in 0..total_windows {
            let train_end = TRAIN_BARS + wi * TEST_BARS;
            let test_start = train_end;
            let test_end = (test_start + TEST_BARS).min(n);
            if test_end.saturating_sub(test_start) < 5 {
                continue;
            }

            for &cap in &POSITION_CAPS {
                let (wf_result, eq_curve) =
                    run_sim_for_cap(&sym_data_map, &symbols, test_start, test_end, cap);
                let row = DetailRow {
                    cap,
                    universe: label.to_string(),
                    window: wi,
                    ret: wf_result.ret,
                    sharpe: wf_result.sharpe,
                    max_dd: wf_result.max_dd,
                    trades: wf_result.trades,
                    win_rate: wf_result.win_rate,
                    pass: wf_result.pass,
                };
                detail_rows.push(row.clone());
                results_by_cap.get_mut(&cap).unwrap().push(row);
                equity_segments_by_cap.get_mut(&cap).unwrap().push(eq_curve);
            }
        }
    }

    let mut summaries: Vec<CapSummary> = Vec::new();
    for &cap in &POSITION_CAPS {
        let rows = results_by_cap.get(&cap).unwrap();
        let sharpes: Vec<f64> = rows.iter().map(|r| r.sharpe).collect();
        let avg_sharpe = sharpes.iter().sum::<f64>() / sharpes.len().max(1) as f64;
        let median_sharpe = median(sharpes.clone());
        let avg_return = rows.iter().map(|r| r.ret).sum::<f64>() / rows.len().max(1) as f64;
        let avg_max_dd = rows.iter().map(|r| r.max_dd).sum::<f64>() / rows.len().max(1) as f64;
        let avg_win_rate = rows.iter().map(|r| r.win_rate).sum::<f64>() / rows.len().max(1) as f64;
        let total_trades = rows.iter().map(|r| r.trades).sum::<usize>();
        let pass_count = rows.iter().filter(|r| r.pass).count();
        let pass_rate = pass_count as f64 / rows.len().max(1) as f64 * 100.0;

        let mut universe_avg: HashMap<&str, f64> = HashMap::new();
        let mut universe_ct: HashMap<&str, usize> = HashMap::new();
        for row in rows {
            *universe_avg.entry(&row.universe).or_insert(0.0) += row.sharpe;
            *universe_ct.entry(&row.universe).or_insert(0) += 1;
        }
        let positive_universes = universe_avg
            .iter()
            .filter(|(uni, total)| **total / *universe_ct.get(*uni).unwrap_or(&1) as f64 > 0.0)
            .count();

        let base5_rows: Vec<&DetailRow> = rows.iter().filter(|r| r.universe == "Base5").collect();
        let base5_pass_rate = if base5_rows.is_empty() {
            0.0
        } else {
            base5_rows.iter().filter(|r| r.pass).count() as f64 / base5_rows.len() as f64 * 100.0
        };

        summaries.push(CapSummary {
            cap,
            avg_sharpe,
            median_sharpe,
            avg_return,
            avg_max_dd,
            avg_win_rate,
            pass_rate,
            total_trades,
            positive_universes,
            base5_pass_rate,
            windows: rows.len(),
        });
    }

    summaries.sort_by(|a, b| {
        if is_better(a, b) {
            std::cmp::Ordering::Less
        } else if is_better(b, a) {
            std::cmp::Ordering::Greater
        } else {
            a.cap.cmp(&b.cap)
        }
    });
    let winner = summaries.first().cloned().expect("non-empty summaries");

    let mut selected_caps = vec![BASELINE_CAP];
    for summary in &summaries {
        if !selected_caps.contains(&summary.cap) {
            selected_caps.push(summary.cap);
        }
        if selected_caps.len() == 4 {
            break;
        }
    }

    eprintln!("\n==== ROBUSTNESS RANKING ====");
    for s in &summaries {
        eprintln!(
            "CAP={:>2} | pass={:>5.1}% | pos_unis={}/9 | sharpe={:>6.3} | median={:>6.3} | ret={:>7.1}% | dd={:>5.1}% | Base5={:>5.1}% | trades={}",
            s.cap,
            s.pass_rate,
            s.positive_universes,
            s.avg_sharpe,
            s.median_sharpe,
            s.avg_return,
            s.avg_max_dd,
            s.base5_pass_rate,
            s.total_trades,
        );
    }

    let mut summary_file = File::create(SUMMARY_CSV)?;
    writeln!(
        summary_file,
        "position_cap,avg_sharpe,median_sharpe,avg_return,avg_max_dd,avg_win_rate,pass_rate,total_trades,positive_universes,base5_pass_rate,windows"
    )?;
    for s in &summaries {
        writeln!(
            summary_file,
            "{},{:.6},{:.6},{:.6},{:.6},{:.6},{:.6},{},{},{:.6},{}",
            s.cap,
            s.avg_sharpe,
            s.median_sharpe,
            s.avg_return,
            s.avg_max_dd,
            s.avg_win_rate,
            s.pass_rate,
            s.total_trades,
            s.positive_universes,
            s.base5_pass_rate,
            s.windows,
        )?;
    }

    let mut detail_file = File::create(DETAIL_CSV)?;
    writeln!(
        detail_file,
        "position_cap,universe,window,return_pct,sharpe,max_dd_pct,trades,win_rate_pct,pass"
    )?;
    for row in &detail_rows {
        writeln!(
            detail_file,
            "{},{},{},{:.6},{:.6},{:.6},{},{:.6},{}",
            row.cap,
            row.universe,
            row.window,
            row.ret,
            row.sharpe,
            row.max_dd,
            row.trades,
            row.win_rate,
            row.pass,
        )?;
    }

    let mut compounded_by_cap: HashMap<usize, Vec<f64>> = HashMap::new();
    let mut max_len = 0usize;
    for &cap in &POSITION_CAPS {
        let mut merged = vec![1.0_f64];
        if let Some(segments) = equity_segments_by_cap.get(&cap) {
            for eq in segments {
                let anchor = *merged.last().unwrap_or(&1.0);
                for &v in eq.iter().skip(1) {
                    merged.push(anchor * v);
                }
            }
        }
        max_len = max_len.max(merged.len());
        compounded_by_cap.insert(cap, merged);
    }

    let mut eq_file = File::create(ALL_EQUITY_CSV)?;
    let headers = POSITION_CAPS
        .iter()
        .map(|cap| format!("cap_{}", cap))
        .collect::<Vec<_>>()
        .join(",");
    writeln!(eq_file, "step,{}", headers)?;
    for step in 0..max_len {
        let row = POSITION_CAPS
            .iter()
            .map(|cap| {
                compounded_by_cap
                    .get(cap)
                    .and_then(|v| v.get(step))
                    .map(|v| format!("{:.6}", v))
                    .unwrap_or_default()
            })
            .collect::<Vec<_>>()
            .join(",");
        writeln!(eq_file, "{},{}", step, row)?;
    }

    let mut selected_file = File::create(SELECTED_EQUITY_CSV)?;
    let selected_headers = selected_caps
        .iter()
        .map(|cap| format!("cap_{}", cap))
        .collect::<Vec<_>>()
        .join(",");
    writeln!(selected_file, "step,{}", selected_headers)?;
    for step in 0..max_len {
        let row = selected_caps
            .iter()
            .map(|cap| {
                compounded_by_cap
                    .get(cap)
                    .and_then(|v| v.get(step))
                    .map(|v| format!("{:.6}", v))
                    .unwrap_or_default()
            })
            .collect::<Vec<_>>()
            .join(",");
        writeln!(selected_file, "{},{}", step, row)?;
    }

    write_json_summary(SUMMARY_JSON, &summaries, &winner)?;

    let baseline = summaries
        .iter()
        .find(|s| s.cap == BASELINE_CAP)
        .cloned()
        .expect("baseline present");
    let mut report = File::create(REPORT_MD)?;
    writeln!(report, "# POSITION_CAP Hyperopt — 2026-04-27")?;
    writeln!(report)?;
    writeln!(report, "- Strategy: current Turtle-only production logic")?;
    writeln!(report, "- Fixed params: EP={}, ATR({}, {}), ATR_ENTRY_MULT={}, HM={}, VOL_LOOKBACK={}", TURTLE_ENTRY, TURTLE_ATR_PERIOD, TURTLE_ATR_MULT, ATR_ENTRY_MULT, HOLD_MAX, VOL_LOOKBACK)?;
    writeln!(report, "- Sweep: POSITION_CAP ∈ {:?}", POSITION_CAPS)?;
    writeln!(report, "- Selection rule: pass rate > positive universes > avg Sharpe > lower avg max DD")?;
    writeln!(report)?;
    writeln!(report, "## Winner")?;
    writeln!(report, "- Winner: CAP={} | pass={:.1}% | avg Sharpe={:.3} | avg return={:.1}% | avg DD={:.1}% | positive universes={}/9", winner.cap, winner.pass_rate, winner.avg_sharpe, winner.avg_return, winner.avg_max_dd, winner.positive_universes)?;
    writeln!(report, "- Baseline CAP={} | pass={:.1}% | avg Sharpe={:.3} | avg return={:.1}% | avg DD={:.1}%", baseline.cap, baseline.pass_rate, baseline.avg_sharpe, baseline.avg_return, baseline.avg_max_dd)?;
    writeln!(report)?;
    writeln!(report, "## Selected equity exports")?;
    writeln!(report, "- {}", ALL_EQUITY_CSV)?;
    writeln!(report, "- {}", SELECTED_EQUITY_CSV)?;
    writeln!(report, "- {}", SUMMARY_CSV)?;
    writeln!(report, "- {}", DETAIL_CSV)?;
    writeln!(report, "- {}", SUMMARY_JSON)?;

    eprintln!("\nWinner: CAP={} | baseline CAP={} | selected caps {:?}", winner.cap, BASELINE_CAP, selected_caps);
    eprintln!("Wrote: {}, {}, {}, {}, {}, {}", SUMMARY_CSV, DETAIL_CSV, ALL_EQUITY_CSV, SELECTED_EQUITY_CSV, SUMMARY_JSON, REPORT_MD);
    eprintln!("Runtime: {:.2}s", t0.elapsed().as_secs_f64());
    Ok(())
}
