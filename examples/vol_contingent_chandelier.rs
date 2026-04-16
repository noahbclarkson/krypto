//! Vol-Contingent Chandelier Multiplier Walk-Forward
//!
//! Tests: Chandelier's fixed ATR multiplier (2.0) is wrong in high-vol regimes.
//! Mechanism: vol_rank > 75th pct → CHAND_MULT × vol_high_mult (tighter stop);
//!            vol_rank < 25th pct → CHAND_MULT × 0.80 (looser stop);
//!            neutral → CHAND_MULT = 2.0 (baseline).
//!
//! Sweep: 5 configs × 9 universes × 6 windows = 270 runs
//! Baseline: Turtle+Chandelier(28, 2.0) dual-exit

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
const CHAND_PERIOD: usize = 28;
const CHAND_MULT: f64 = 2.00;
const TURTLE_ENTRY: usize = 21;
const TURTLE_ATR_PERIOD: usize = 24;
const TURTLE_ATR_MULT: f64 = 2.00;
const VOL_HISTORY: usize = 252;

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

const CSV_OUT: &str = "snapshots/vol_contingent_chandelier.csv";
const MD_OUT: &str = "snapshots/vol_contingent_chandelier.md";

// Configs: vol_high_mult applied when vol_rank > 75th pct
const CONFIGS: &[(&str, f64)] = &[
    ("BASE",  1.00),  // no vol contingency
    ("VH_110", 1.10), // +10% tighter in high vol
    ("VH_120", 1.20), // +20% tighter in high vol
    ("VH_130", 1.30), // +30% tighter in high vol
    ("VH_150", 1.50), // +50% tighter in high vol
];

macro_rules! col_vec {
    ($df:expr, $name:expr, $n:expr) => {{
        let chunked = $df.column($name).unwrap().f64().unwrap();
        chunked.into_iter().filter_map(|x| x).take($n).collect::<Vec<_>>()
    }};
}

struct SymData {
    close: Vec<f64>,
    high: Vec<f64>,
    low: Vec<f64>,
    vol: Vec<f64>,
    // 21-bar realized vol annualised
    realized_vol: Vec<f64>,
    // vol percentile rank vs 252-bar history
    vol_pct_rank: Vec<f64>,
}

impl SymData {
    fn new(df: &DataFrame, n: usize) -> Self {
        let close = col_vec!(df, "close", n);
        let high = col_vec!(df, "high", n);
        let low = col_vec!(df, "low", n);
        let vol = col_vec!(df, "volume", n);

        let mut realized_vol = vec![0.0; close.len()];
        let mut vol_pct_rank = vec![0.0; close.len()];

        for i in 21..close.len() {
            let mut sum_sq = 0.0_f64;
            for j in (i + 1 - 21)..i {
                let ret = (close[j] / close[j.saturating_sub(1)].max(0.001)).ln();
                sum_sq += ret * ret;
            }
            realized_vol[i] = (sum_sq / 21.0_f64).sqrt() * 365.0_f64.sqrt();
        }

        for i in VOL_HISTORY..close.len() {
            let cur = realized_vol[i];
            let mut count = 0usize;
            for j in (i + 1 - VOL_HISTORY)..i {
                if realized_vol[j] < cur { count += 1; }
            }
            vol_pct_rank[i] = count as f64 / VOL_HISTORY as f64;
        }

        Self { close, high, low, vol, realized_vol, vol_pct_rank }
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

fn turtle_signal(close: &[f64], entry_period: usize, idx: usize) -> bool {
    if idx < entry_period + 1 { return false; }
    let start = idx + 1 - entry_period;
    let mut max_close = f64::NEG_INFINITY;
    for i in start..idx {
        if let Some(&c) = close.get(i) { max_close = max_close.max(c); }
    }
    close.get(idx).map_or(false, |&curr| curr > max_close)
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
}

fn run_sim(
    sym_data: &HashMap<String, SymData>,
    symbols: &[String],
    test_start: usize,
    test_end: usize,
    vol_high_mult: f64,
) -> WfResult {
    let mut equity = 1.0_f64;
    let mut equity_curve = vec![1.0_f64];
    let mut wins = 0usize;
    let mut total_trades = 0usize;
    let mut daily_rets = Vec::new();

    let mut bar = test_start;
    while bar + 2 < test_end {
        // Rank by dollar volume
        let mut scores: Vec<(&str, f64)> = Vec::new();
        for sym in symbols {
            if let Some(sd) = sym_data.get(sym) {
                if bar >= sd.close.len() { continue; }
                let dv = sd.vol.get(bar).copied().unwrap_or(0.0)
                    * sd.close.get(bar).copied().unwrap_or(0.0);
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
                    if turtle_signal(&sd.close, TURTLE_ENTRY, bar) {
                        let entry_px = sd.close[bar];
                        let entry = entry_px * (1.0 - TAKER_FEE);
                        let entry_bar_next = bar + 1;
                        let n = sd.close.len();

                        // Vol-contingent Chandelier multiplier
                        let vol_rank = sd.vol_pct_rank.get(entry_bar_next).copied().unwrap_or(0.5);
                        let effective_mult = if vol_rank > 0.75 {
                            CHAND_MULT * vol_high_mult   // tighter in high vol
                        } else if vol_rank < 0.25 {
                            CHAND_MULT * 0.80           // looser in low vol
                        } else {
                            CHAND_MULT                   // baseline
                        };

                        let mut highest_high_chand = sd.high[entry_bar_next];
                        let mut highest_high_turtle = sd.high[entry_bar_next];
                        let max_bar = (entry_bar_next + HOLD_MAX).min(n.saturating_sub(1));
                        let mut exit_bar = max_bar;
                        for b in entry_bar_next..=max_bar.min(n.saturating_sub(1)) {
                            highest_high_chand = highest_high_chand.max(sd.high[b]);
                            let atr_chand = atr_at(&sd.high, &sd.low, &sd.close, CHAND_PERIOD, b);
                            let trail_chand = highest_high_chand - effective_mult * atr_chand;
                            highest_high_turtle = highest_high_turtle.max(sd.high[b]);
                            let atr_turtle = atr_at(&sd.high, &sd.low, &sd.close, TURTLE_ATR_PERIOD, b);
                            let trail_turtle = highest_high_turtle - TURTLE_ATR_MULT * atr_turtle;
                            if sd.close[b] < trail_chand || sd.close[b] < trail_turtle {
                                exit_bar = b;
                                break;
                            }
                        }

                        let exit_px = sd.close[exit_bar.min(n - 1)];
                        let exit = exit_px * (1.0 - TAKER_FEE);
                        let ret = (exit / entry) - 1.0;
                        equity *= 1.0 + ret;
                        total_trades += 1;
                        if ret > 0.0 { wins += 1; }
                        daily_rets.push(ret);

                        for _ in (entry_bar_next + 1)..=exit_bar.min(n - 1) {
                            equity_curve.push(equity);
                        }
                        entered = true;
                        break;
                    }
                }
            }
        }

        if !entered {
            equity_curve.push(equity);
        }
        bar += 1;
    }

    let win_rate = if total_trades > 0 { wins as f64 / total_trades as f64 } else { 0.0 };
    WfResult {
        ret: (equity - 1.0) * 100.0,
        sharpe: annualised_sharpe(&daily_rets),
        max_dd: max_dd_from(&equity_curve),
        trades: total_trades,
        win_rate: win_rate * 100.0,
        pass: annualised_sharpe(&daily_rets) > 0.0 && total_trades >= MIN_TRADES,
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    let t0 = Instant::now();
    eprintln!("\n=== Vol-Contingent Chandelier Multiplier Walk-Forward ===");
    eprintln!("Hypothesis: CHAND_MULT=2.0 fixed is wrong in high-vol regimes.");
    eprintln!("Mechanism: vol_rank > 75th pct -> CHAND_MULT * vol_high_mult (tighter)");
    eprintln!("           vol_rank < 25th pct -> CHAND_MULT * 0.80 (looser)");
    eprintln!("           neutral              -> CHAND_MULT = 2.00 (baseline)\n");

    let loader = DataLoader::new(None, None);

    // Pre-load all symbols
    let mut all_syms: std::collections::HashSet<String> = std::collections::HashSet::new();
    for (_, syms) in UNIVERSES {
        for &s in *syms { all_syms.insert(s.to_string()); }
    }

    let mut raw_cache: HashMap<String, DataFrame> = HashMap::new();
    for sym in all_syms.iter() {
        match loader.fetch_with_cache(sym.as_str(), "1d", CANDLES).await {
            Ok(df) => { raw_cache.insert(sym.clone(), df); }
            Err(e) => { eprintln!("  WARNING: {} load failed: {}", sym, e); }
        }
    }

    let min_len = raw_cache.values().map(|df| df.height()).min().unwrap_or(0).min(2800);
    eprintln!("Loaded {} symbols, min_len={}\n", raw_cache.len(), min_len);

    let n_windows = (min_len.saturating_sub(TRAIN_BARS)) / TEST_BARS;
    eprintln!("{} walk-forward windows\n", n_windows);

    // Global aggregates: (sharpe_sum, ret_sum, dd_sum, trades_sum, count, pass_count)
    let mut global_aggs: HashMap<&str, (f64, f64, f64, f64, f64, usize)> = HashMap::new();
    for &(name, _) in CONFIGS {
        global_aggs.insert(name, (0.0, 0.0, 0.0, 0.0, 0.0, 0));
    }

    let mut csv_rows = vec!["universe,window,config,vol_high_mult,ret_pct,sharpe,max_dd,trades,win_rate,pass".to_string()];
    let mut summary_rows: Vec<String> = Vec::new();

    for (universe_name, symbols) in UNIVERSES {
        eprintln!("--- {universe_name} ---");
        let symbols: Vec<String> = symbols.iter().map(|s| s.to_string()).collect();

        // Build SymData for universe symbols
        let mut sym_data_map: HashMap<String, SymData> = HashMap::new();
        for sym in &symbols {
            if let Some(df) = raw_cache.get(sym) {
                sym_data_map.insert(sym.clone(), SymData::new(df, min_len));
            }
        }

        // Per-universe per-config aggregates
        let mut univ_aggs: HashMap<&str, (f64, f64, f64, f64, f64, usize)> = HashMap::new();
        for &(name, _) in CONFIGS {
            univ_aggs.insert(name, (0.0, 0.0, 0.0, 0.0, 0.0, 0));
        }

        for w in 0..n_windows {
            let train_start = w * TEST_BARS;
            let test_start = train_start + TRAIN_BARS;
            let test_end = test_start + TEST_BARS;
            if test_end > min_len { break; }

            for &(config_name, vol_high_mult) in CONFIGS {
                let result = run_sim(&sym_data_map, &symbols, test_start, test_end, vol_high_mult);
                let pass_i = if result.pass { 1 } else { 0 };

                // CSV row
                csv_rows.push(format!(
                    "{},{},{},{},{:.4},{:.4},{:.4},{},{:.4},{}",
                    universe_name, w, config_name, vol_high_mult,
                    result.ret, result.sharpe, result.max_dd, result.trades,
                    result.win_rate, pass_i
                ));

                // Per-universe aggregate
                if let Some((s, r, d, t, cnt, p)) = univ_aggs.get_mut(config_name) {
                    *s += result.sharpe; *r += result.ret; *d += result.max_dd;
                    *t += result.trades as f64; *cnt += 1.0; *p += pass_i;
                }

                // Global aggregate
                if let Some((s, r, d, t, cnt, p)) = global_aggs.get_mut(config_name) {
                    *s += result.sharpe; *r += result.ret; *d += result.max_dd;
                    *t += result.trades as f64; *cnt += 1.0; *p += pass_i;
                }

                let pass_str = if result.pass { "PASS" } else { "FAIL" };
                eprintln!(
                    "  {}: ret={:+.1}, Sharpe={:.3}, DD={:.1}, trades={}, {}",
                    config_name, result.ret, result.sharpe, result.max_dd, result.trades, pass_str
                );
            }
        }

        // Per-universe summary
        eprintln!("\n  [{universe_name} Summary]");
        for &(config_name, vol_high_mult) in CONFIGS {
            if let Some((s, r, d, t, cnt, p)) = univ_aggs.get(config_name) {
                if *cnt > 0.0 {
                    let avg_s = *s / *cnt;
                    let avg_r = *r / *cnt;
                    let avg_d = *d / *cnt;
                    let avg_t = *t / *cnt;
                    let pass_r = *p as f64 / *cnt * 100.0;
                    eprintln!(
                        "  {}: Sharpe={:.3}, Ret={:+.1}%, DD={:.1}%, Trades={:.0}, PassRate={:.0}%",
                        config_name, avg_s, avg_r, avg_d, avg_t, pass_r
                    );
                    summary_rows.push(format!(
                        "| {} | {} | {} | {:.3} | {:+.1}% | {:.1}% | {:.0} | {:.0}% |",
                        universe_name, config_name, vol_high_mult, avg_s, avg_r, avg_d, avg_t, pass_r
                    ));
                }
            }
        }
        eprintln!();
    }

    // Write CSV
    let mut csv_file = File::create(CSV_OUT)?;
    for row in &csv_rows { writeln!(csv_file, "{}", row)?; }
    eprintln!("Wrote: {}\n", CSV_OUT);

    // Write MD
    let mut md_file = File::create(MD_OUT)?;
    writeln!(md_file, "# Vol-Contingent Chandelier Multiplier Walk-Forward")?;
    writeln!(md_file, "\n## Hypothesis")?;
    writeln!(md_file, "Chandelier's fixed ATR multiplier (2.0) is wrong in high-vol regimes.")?;
    writeln!(md_file, "- **High vol** (21d vol > 75th pct vs 252d history): ATR spikes -> trailing stop gets too LOOSE -> need HIGHER multiplier (tighter stop)")?;
    writeln!(md_file, "- **Low vol** (21d vol < 25th pct): ATR shrinks -> trailing stop gets too TIGHT -> need LOWER multiplier (looser stop)")?;
    writeln!(md_file, "\n## Mechanism")?;
    writeln!(md_file, "```\neffective_mult = CHAND_MULT\n  if vol_rank > 0.75: effective_mult = CHAND_MULT * vol_high_mult  // tighter\n  if vol_rank < 0.25: effective_mult = CHAND_MULT * 0.80             // looser\n  // neutral: CHAND_MULT = 2.0\n```")?;
    writeln!(md_file, "\n## Configs Swept")?;
    writeln!(md_file, "\n| Config | vol_high_mult | Description |")?;
    writeln!(md_file, "|--------|--------------|-------------|")?;
    for &(name, mult) in CONFIGS {
        let desc = if mult == 1.0 { "BASELINE (no vol contingency)" } else { "Tighter stop in high-vol" };
        writeln!(md_file, "| {} | {} | {} |", name, mult, desc)?;
    }
    writeln!(md_file, "\n## Results by Universe")?;
    writeln!(md_file, "\n| Universe | Config | vol_high_mult | Avg Sharpe | Avg Ret | Avg DD | Avg Trades | Pass Rate |")?;
    writeln!(md_file, "|----------|--------|--------------|------------|---------|--------|-----------|----------|")?;
    for row in &summary_rows { writeln!(md_file, "{}", row)?; }

    writeln!(md_file, "\n## Global Summary")?;
    writeln!(md_file, "\n| Config | vol_high_mult | Global Sharpe | Global Ret | Global DD | Total Trades | Pass Rate |")?;
    writeln!(md_file, "|--------|--------------|--------------|------------|-----------|-------------|----------|")?;
    eprintln!("\n=== Global Summary ===");
    eprintln!("| Config | vol_high_mult | Global Sharpe | Global Ret | Global DD | Total Trades | Pass Rate |");
    eprintln!("|--------|--------------|--------------|------------|-----------|-------------|----------|");
    for &(config_name, vol_high_mult) in CONFIGS {
        if let Some((s, r, d, t, cnt, p)) = global_aggs.get(config_name) {
            if *cnt > 0.0 {
                let avg_s = *s / *cnt;
                let avg_r = *r / *cnt;
                let avg_d = *d / *cnt;
                let avg_t = *t / *cnt;
                let pass_r = *p as f64 / *cnt * 100.0;
                eprintln!("| {} | {} | {:.3} | {:+.1}% | {:.1}% | {:.0} | {:.0}% |",
                    config_name, vol_high_mult, avg_s, avg_r, avg_d, avg_t, pass_r);
                writeln!(md_file, "| {} | {} | {:.3} | {:+.1}% | {:.1}% | {:.0} | {:.0}% |",
                    config_name, vol_high_mult, avg_s, avg_r, avg_d, avg_t, pass_r)?;
            }
        }
    }

    writeln!(md_file, "\n*Generated: {:?}*", std::time::SystemTime::now())?;
    drop(md_file);

    eprintln!("\nWrote: {}", MD_OUT);
    eprintln!("Total time: {:.1}s", t0.elapsed().as_secs_f64());
    Ok(())
}
