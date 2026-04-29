//! ATR_ENTRY_MULT Fine-Grained Hyperopt — Current Production Params
//!
//! Mission: Comprehensive fine-grained sweep of ATR_ENTRY_MULT with CURRENT
//! production params (CHAND_P=7, CHAND_M=2.30, EP=21, HM=12, CAP=3).
//!
//! PRIOR WORK:
//! - atr_entry_mult_prod_sweep.rs: 11 values (coarse 0.05 step), EP=21, CHAND_P=11 (STALE).
//!   → EM=0.00 won decisively (but universe count unclear).
//! - atr_entry_mult_full_sweep.rs: 41 values (0.05 step), 9 universes, EP=24 (STALE),
//!   CHAND_P=11/M=2.25 (STALE) → EM=0.00 confirmed at 83.3% pass.
//! - atr_entry_mult_fine_sweep.rs: 201 values (0.01 step), Base5 only (6 windows), EP=24 (STALE).
//!   → EM=0.00 confirmed (61/63 pass).
//!
//! THIS SWEEP: 201 values × 9 universes × 6 windows (1,134 window-runs per value)
//! with EP=21, CHAND_P=7, CHAND_M=2.30, HM=12, CAP=3, ATR_P=24, ATR_M=2.0.
//! Full 9-universe coverage at 0.01 granularity — the definitive test.
//!
//! SWEEP: em ∈ [0.00..2.00] step 0.01 → 201 values

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
const HOLD_MAX: usize = 12;
const TAKER_FEE: f64 = 0.001;
const POSITION_CAP: usize = 3;
const MIN_TRADES: usize = 3;

// Current production params
const CHAND_PERIOD: usize = 7;
const CHAND_MULT: f64 = 2.30;
const TURTLE_ENTRY: usize = 21;
const TURTLE_ATR_PERIOD: usize = 24;
const TURTLE_ATR_MULT: f64 = 2.00;
const VOL_LOOKBACK: usize = 8;

const EM_START: f64 = 0.00;
const EM_END: f64 = 2.00;
const EM_STEP: f64 = 0.01;
const N_VALUES: usize = 201;

const UNIVERSES: &[(&str, &[&str])] = &[
    (
        "Base5",
        &[
            "BTCUSDT", "ETHUSDT", "SOLUSDT", "XRPUSDT", "DOGEUSDT", "ADAUSDT",
        ],
    ),
    (
        "NoDOGE",
        &["BTCUSDT", "ETHUSDT", "SOLUSDT", "XRPUSDT", "ADAUSDT"],
    ),
    (
        "Legacy4",
        &["BTCUSDT", "ETHUSDT", "XRPUSDT", "LTCUSDT", "EOSUSDT"],
    ),
    (
        "Legacy5BNB",
        &[
            "BTCUSDT", "ETHUSDT", "XRPUSDT", "LTCUSDT", "BNBUSDT", "EOSUSDT",
        ],
    ),
    (
        "OldGuardNoBNB",
        &[
            "BTCUSDT", "ETHUSDT", "XRPUSDT", "LTCUSDT", "EOSUSDT", "BCHUSDT",
        ],
    ),
    (
        "LargeCaps5",
        &[
            "BTCUSDT", "ETHUSDT", "SOLUSDT", "XRPUSDT", "BNBUSDT", "ADAUSDT",
        ],
    ),
    ("Legacy3", &["BTCUSDT", "XRPUSDT", "LTCUSDT", "EOSUSDT"]),
    (
        "LowVolume5",
        &["XRPUSDT", "LTCUSDT", "EOSUSDT", "BCHUSDT", "ADAUSDT"],
    ),
    (
        "OldGuard4",
        &["BTCUSDT", "XRPUSDT", "LTCUSDT", "EOSUSDT", "BCHUSDT"],
    ),
];

const OUT_CSV: &str = "snapshots/atr_entry_mult_current_sweep.csv";
const EQUITY_CSV: &str = "snapshots/atr_entry_mult_current_equity.csv";
const SUMMARY_CSV: &str = "snapshots/atr_entry_mult_current_summary.csv";

struct SymData {
    close: Vec<f64>,
    high: Vec<f64>,
    low: Vec<f64>,
    vol: Vec<f64>,
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
    vals[idx + 1 - window..=idx].iter().sum::<f64>() / window as f64
}

fn annualised_sharpe(daily_rets: &[f64]) -> f64 {
    if daily_rets.len() < 2 {
        return 0.0;
    }
    let mn: f64 = daily_rets.iter().sum::<f64>() / daily_rets.len() as f64;
    let sd =
        (daily_rets.iter().map(|x| (x - mn).powi(2)).sum::<f64>() / daily_rets.len() as f64).sqrt();
    if sd == 0.0 {
        return 0.0;
    }
    mn * 365.0_f64.sqrt() / sd
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
    em: f64,
) -> WfResult {
    let mut equity = 1.0_f64;
    let mut equity_curve = vec![1.0_f64];
    let mut peak = equity;
    let mut wins = 0usize;
    let mut total_trades = 0usize;
    let mut daily_rets = Vec::new();

    let mut bar = test_start;
    while bar + 2 < test_end {
        // Dollar-volume ranking
        let mut scores: Vec<(&str, f64)> = Vec::new();
        for sym in symbols {
            if let Some(sd) = sym_data.get(sym) {
                if bar >= sd.close.len() {
                    continue;
                }
                let rol_vol = rolling_avg(&sd.vol, VOL_LOOKBACK, bar);
                let price = sd.close.get(bar).copied().unwrap_or(0.0);
                let dv = rol_vol * price;
                scores.push((
                    sym.as_str(),
                    if dv.is_finite() && dv > 0.0 { dv } else { 0.0 },
                ));
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

        // Turtle breakout entry with ATR filter
        let mut entered = false;
        'syms: for sym in &top_syms {
            if let Some(sd) = sym_data.get(sym) {
                if bar >= TURTLE_ENTRY + 1 && bar < sd.close.len() {
                    let start = bar + 1 - TURTLE_ENTRY;
                    let mut max_close = f64::NEG_INFINITY;
                    for i in start..bar {
                        if let Some(&c) = sd.close.get(i) {
                            max_close = max_close.max(c);
                        }
                    }
                    if let Some(&curr_close) = sd.close.get(bar) {
                        let breakout = curr_close > max_close;
                        let passes_filter = if em > 0.0 && breakout {
                            let atr_val =
                                atr_at(&sd.high, &sd.low, &sd.close, TURTLE_ATR_PERIOD, bar);
                            curr_close >= max_close + em * atr_val
                        } else {
                            breakout
                        };
                        if passes_filter {
                            let entry_px = curr_close * (1.0 - TAKER_FEE);
                            let entry_bar_next = bar + 1;
                            let n = sd.close.len();

                            // Dual exit: Chandelier ATR OR Turtle ATR
                            let mut highest_high_chand = sd.high[entry_bar_next.min(n - 1)];
                            let mut lowest_low_turtle = sd.low[entry_bar_next.min(n - 1)];
                            let max_bar = (entry_bar_next + HOLD_MAX).min(n.saturating_sub(1));
                            let mut exit_bar = max_bar;

                            for b in entry_bar_next..=max_bar {
                                if b >= n {
                                    break;
                                }
                                highest_high_chand = highest_high_chand.max(sd.high[b]);
                                let atr_chand =
                                    atr_at(&sd.high, &sd.low, &sd.close, CHAND_PERIOD, b);
                                let trail_chand = highest_high_chand - CHAND_MULT * atr_chand;
                                lowest_low_turtle = lowest_low_turtle.min(sd.low[b]);
                                let atr_turtle =
                                    atr_at(&sd.high, &sd.low, &sd.close, TURTLE_ATR_PERIOD, b);
                                let trail_turtle = lowest_low_turtle - TURTLE_ATR_MULT * atr_turtle;

                                if sd.close[b] < trail_chand || sd.close[b] < trail_turtle {
                                    exit_bar = b;
                                    break;
                                }
                            }

                            if let Some(&exit_px) = sd.close.get(exit_bar) {
                                let exit = exit_px * (1.0 - TAKER_FEE);
                                let gross_ret = exit / entry_px - 1.0;
                                let bars_held =
                                    (exit_bar as i64 - entry_bar_next as i64).max(1) as usize;
                                wins += if gross_ret > 0.0 { 1 } else { 0 };
                                total_trades += 1;
                                equity *= 1.0 + gross_ret;
                                let avg_daily = gross_ret / bars_held as f64;
                                for _ in 0..bars_held {
                                    daily_rets.push(avg_daily);
                                }
                                if equity > peak {
                                    peak = equity;
                                }
                                equity_curve.push(equity);
                                bar = exit_bar + 1;
                                entered = true;
                                break 'syms;
                            }
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
    eprintln!("==== ATR_ENTRY_MULT Fine-Grained Sweep — Current Production Params ====");
    eprintln!("CHAND(7,2.30)/EP=21/HM=12/CAP=3/ATR(24,2.0)");
    eprintln!(
        "Sweep: EM ∈ [{:.2}..{:.2}] step {:.2} → {} values × 9 universes × 6 windows",
        EM_START, EM_END, EM_STEP, N_VALUES
    );

    let loader = DataLoader::new(None, None);
    let mut all_syms: std::collections::HashSet<String> = std::collections::HashSet::new();
    for (_, syms) in UNIVERSES {
        for &s in *syms {
            all_syms.insert(s.to_string());
        }
    }

    let mut raw_cache: HashMap<String, DataFrame> = HashMap::new();
    let mut min_len = usize::MAX;
    for sym in all_syms.iter() {
        match loader.fetch_with_cache(sym.as_str(), "1d", CANDLES).await {
            Ok(df) => {
                min_len = min_len.min(df.height());
                raw_cache.insert(sym.clone(), df);
            }
            Err(e) => {
                eprintln!("  WARNING: {} load failed: {}", sym, e);
            }
        }
    }

    let n = min_len.min(2800);
    let mut sym_data_map: HashMap<String, SymData> = HashMap::new();
    for sym in &all_syms {
        if let Some(df) = raw_cache.get(sym) {
            let n_min = df.height().min(n);
            let col_vec = |name: &str| -> Vec<f64> {
                let chunked = df.column(name).unwrap().f64().unwrap();
                chunked
                    .into_iter()
                    .filter_map(|x| x)
                    .take(n_min)
                    .collect::<Vec<_>>()
            };
            sym_data_map.insert(
                sym.clone(),
                SymData {
                    close: col_vec("close"),
                    high: col_vec("high"),
                    low: col_vec("low"),
                    vol: col_vec("volume"),
                },
            );
        }
    }
    eprintln!("Loaded {} symbols, {} bars\n", sym_data_map.len(), n);

    // Pre-compute window indices
    let mut window_ranges: Vec<(usize, usize, usize)> = Vec::new();
    for &(uname, symbols) in UNIVERSES {
        let symbols: Vec<String> = symbols.iter().map(|s| s.to_string()).collect();
        if !symbols.iter().all(|s| sym_data_map.contains_key(s)) {
            continue;
        }
        let total_windows = n.saturating_sub(TRAIN_BARS + TEST_BARS) / TEST_BARS;
        for wi in 0..total_windows {
            let train_end = TRAIN_BARS + wi * TEST_BARS;
            let test_start = train_end;
            let test_end = (test_start + TEST_BARS).min(n);
            if test_end.saturating_sub(test_start) < 5 {
                continue;
            }
            window_ranges.push((test_start, test_end, wi));
        }
    }
    let n_windows = window_ranges.len();
    eprintln!("Total window-runs: {} across all universes\n", n_windows);

    let mut out_csv = File::create(OUT_CSV)?;
    writeln!(
        out_csv,
        "em,universe,window,return_pct,sharpe,max_dd_pct,trades,win_rate_pct,pass"
    )?;

    let mut equity_csv = File::create(EQUITY_CSV)?;
    writeln!(equity_csv, "em,universe,window,bar_idx,equity")?;

    // Results storage: indexed by em value
    let mut summary: Vec<(f64, usize, usize, f64, f64, f64, usize)> = Vec::new();

    let em_values: Vec<f64> = (0..N_VALUES)
        .map(|i| EM_START + i as f64 * EM_STEP)
        .collect();

    for &em in &em_values {
        let mut total_pass = 0usize;
        let mut all_sharpes = vec![];
        let mut all_returns = vec![];
        let mut all_dds = vec![];
        let mut all_trades = 0usize;

        for &(uname, symbols) in UNIVERSES {
            let symbols: Vec<String> = symbols.iter().map(|s| s.to_string()).collect();
            if !symbols.iter().all(|s| sym_data_map.contains_key(s)) {
                continue;
            }

            let total_windows = n.saturating_sub(TRAIN_BARS + TEST_BARS) / TEST_BARS;
            for wi in 0..total_windows {
                let train_end = TRAIN_BARS + wi * TEST_BARS;
                let test_start = train_end;
                let test_end = (test_start + TEST_BARS).min(n);
                if test_end.saturating_sub(test_start) < 5 {
                    continue;
                }

                let r = run_sim(&sym_data_map, &symbols, test_start, test_end, em);

                if r.pass {
                    total_pass += 1;
                }
                all_sharpes.push(r.sharpe);
                all_returns.push(r.ret);
                all_dds.push(r.max_dd);
                all_trades += r.trades;

                writeln!(
                    out_csv,
                    "{:.2},{},{},{:.4},{:.4},{:.2},{},{:.2},{}",
                    em, uname, wi, r.ret, r.sharpe, r.max_dd, r.trades, r.win_rate, r.pass
                )?;

                // Downsample equity curve: every 10th bar
                for (bi, &eq) in r.equity_curve.iter().enumerate() {
                    if bi % 10 == 0 {
                        writeln!(equity_csv, "{:.2},{},{},{},{:.8}", em, uname, wi, bi, eq)?;
                    }
                }
            }
        }

        let avg_sharpe = all_sharpes.iter().sum::<f64>() / all_sharpes.len().max(1) as f64;
        let avg_return = all_returns.iter().sum::<f64>() / all_returns.len().max(1) as f64;
        let avg_dd = all_dds.iter().sum::<f64>() / all_dds.len().max(1) as f64;
        summary.push((
            em, total_pass, n_windows, avg_sharpe, avg_return, avg_dd, all_trades,
        ));
    }

    // Sort by pass count desc, then Sharpe desc
    summary.sort_by(|a, b| {
        let cmp_pass = b.1.cmp(&a.1);
        if cmp_pass != std::cmp::Ordering::Equal {
            return cmp_pass;
        }
        b.3.partial_cmp(&a.3).unwrap_or(std::cmp::Ordering::Equal)
    });

    let mut sum_csv = File::create(SUMMARY_CSV)?;
    writeln!(
        sum_csv,
        "em,runs_pass,runs_total,pass_pct,avg_sharpe,avg_ret_pct,avg_dd,total_trades"
    )?;
    for (em, passes, total, avg_sh, avg_ret, avg_dd, trades) in &summary {
        let pass_pct = *passes as f64 / (*total as f64) * 100.0;
        writeln!(
            sum_csv,
            "{:.2},{},{},{:.2},{:.4},{:.4},{:.2},{}",
            em, passes, total, pass_pct, avg_sh, avg_ret, avg_dd, trades
        )?;
    }

    eprintln!("\n========== TOP 15 BY PASS RATE THEN SHARPE ==========");
    eprintln!(
        "{:<8} {:>6} {:>6} {:>8} {:>10} {:>12} {:>8} {:>10}",
        "EM", "PASS", "TOTAL", "PASS%", "AVG_SHARPE", "AVG_RET%", "AVG_DD", "TRADES"
    );
    eprintln!("{}", "-".repeat(76));
    for (em, passes, total, avg_sh, avg_ret, avg_dd, trades) in summary.iter().take(15) {
        let pass_pct = *passes as f64 / (*total as f64) * 100.0;
        eprintln!(
            "{:.2}     {:>4} {:>6} {:>7.2}% {:>10.4} {:>12.4} {:>8.2} {:>10}",
            em, passes, total, pass_pct, avg_sh, avg_ret, avg_dd, trades
        );
    }

    eprintln!("\nRuntime: {:.1}s", t0.elapsed().as_secs_f64());
    eprintln!("Output: {}", OUT_CSV);
    eprintln!("Equity: {}", EQUITY_CSV);
    eprintln!("Summary: {}", SUMMARY_CSV);

    Ok(())
}
