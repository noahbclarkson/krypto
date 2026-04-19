//! Chandelier ATR Multiplier Fine-Grain Hyperopt
//!
//! Target: CHAND_MULT — ATR multiplier for Chandelier trailing stop
//! Current default: 2.5 (from coarse 8-value sweep: 1.5-5.0 step 0.5)
//! New sweep: 1.0 to 5.0 step 0.05 (81 values)
//!
//! Strategy: Turtle(EP=21) entry + Chandelier(P=45, M) exit
//! Universes: All 9 | Walk-forward 252/252 | 0.1% taker each side

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
const HOLD_MAX: usize = 60;
const TAKER_FEE: f64 = 0.001;
const POSITION_CAP: usize = 2;
const CHAND_PERIOD: usize = 45;
const TURTLE_ENTRY: usize = 21;

const UNIVERSES: &[(&str, &[&str])] = &[
    ("Base5",        &["BTCUSDT","ETHUSDT","SOLUSDT","XRPUSDT","DOGEUSDT","ADAUSDT"]),
    ("NoDOGE",       &["BTCUSDT","ETHUSDT","SOLUSDT","XRPUSDT","ADAUSDT"]),
    ("Legacy4",      &["BTCUSDT","ETHUSDT","XRPUSDT","LTCUSDT","EOSUSDT"]),
    ("Legacy5BNB",   &["BTCUSDT","ETHUSDT","XRPUSDT","LTCUSDT","BNBUSDT","EOSUSDT"]),
    ("OldGuardNoBNB",&["BTCUSDT","ETHUSDT","XRPUSDT","LTCUSDT","EOSUSDT","BCHUSDT"]),
    ("LargeCaps5",   &["BTCUSDT","ETHUSDT","SOLUSDT","XRPUSDT","BNBUSDT","ADAUSDT"]),
    ("Legacy3",      &["BTCUSDT","XRPUSDT","LTCUSDT","EOSUSDT"]),
    ("LowVolume5",   &["XRPUSDT","LTCUSDT","EOSUSDT","BCHUSDT","ADAUSDT"]),
    ("OldGuard4",   &["BTCUSDT","XRPUSDT","LTCUSDT","EOSUSDT","BCHUSDT"]),
];

const MULT_MIN: f64 = 1.0;
const MULT_MAX: f64 = 5.0;
const MULT_STEP: f64 = 0.05;

const CSV_OUT: &str = "snapshots/chandelier_mult_fine_sweep_results.csv";
const UNIV_CSV_OUT: &str = "snapshots/chandelier_mult_universe_results.csv";
const EQUITY_OUT: &str = "snapshots/chandelier_mult_equity_curves.csv";
const SUMMARY_JSON: &str = "snapshots/chandelier_mult_sweep_summary.json";

struct SymData {
    close: Vec<f64>,
    high: Vec<f64>,
    low: Vec<f64>,
    vol: Vec<f64>,
}

fn atr_at(high: &[f64], low: &[f64], close: &[f64], period: usize, idx: usize) -> f64 {
    if idx < period { return 0.0; }
    let mut sum = 0.0_f64;
    for i in (idx + 1 - period)..=idx {
        let h = high.get(i).copied().unwrap_or(0.0);
        let l = low.get(i).copied().unwrap_or(0.0);
        let c0 = close.get(i.saturating_sub(1)).copied().unwrap_or(0.0);
        sum += (h - l).max((h - c0).abs()).max((l - c0).abs());
    }
    sum / period as f64
}

fn annualised_sharpe(daily_rets: &[f64]) -> f64 {
    if daily_rets.len() < 2 { return 0.0; }
    let mn: f64 = daily_rets.iter().sum::<f64>() / daily_rets.len() as f64;
    let sd = (daily_rets.iter().map(|x| (x - mn).powi(2)).sum::<f64>() / daily_rets.len() as f64).sqrt();
    if sd == 0.0 { return 0.0; }
    mn * 365.0_f64.sqrt() / sd
}

fn max_dd_pct(equity: &[f64]) -> f64 {
    let mut peak = f64::NEG_INFINITY;
    let mut max_dd = 0.0_f64;
    for &e in equity {
        if e > peak { peak = e; }
        let dd = (peak - e) / peak;
        if dd > max_dd { max_dd = dd; }
    }
    max_dd * 100.0
}

/// Simulate: Turtle entry + Chandelier(M) exit, walk-forward window.
/// Uses same loop structure as turtle_chandelier_walkforward.rs:
/// variable bar advancement (jumps to exit_bar after each trade).
fn run_sim(
    sym_data: &HashMap<String, SymData>,
    symbols: &[String],
    chand_mult: f64,
    test_start: usize,
    test_end: usize,
) -> Option<(f64, f64, f64, usize, Vec<f64>)> {
    if test_end - test_start < HOLD_MAX + 5 { return None; }

    let mut equity_curve = vec![1.0_f64];
    let mut equity = 1.0_f64;
    let mut total_trades = 0_usize;
    let mut wins = 0_usize;
    let mut daily_rets = Vec::new();

    let mut bar = test_start;
    while bar + 2 < test_end {
        // Rank symbols by dollar volume (top POSITION_CAP)
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
        let top_syms: Vec<&str> = scores.iter().take(POSITION_CAP).map(|(s, _)| *s).collect();

        if top_syms.is_empty() {
            equity_curve.push(equity);
            bar += 1;
            continue;
        }

        // Turtle breakout entry
        let mut entered = false;
        for sym in &top_syms {
            if let Some(sd) = sym_data.get(*sym) {
                if bar >= TURTLE_ENTRY + 1 && bar < sd.close.len() {
                    // close > max_close of previous TURTLE_ENTRY bars (exclude current)
                    let start = bar + 1 - TURTLE_ENTRY;
                    let mut max_close = f64::NEG_INFINITY;
                    for i in start..bar {
                        if let Some(&c) = sd.close.get(i) { max_close = max_close.max(c); }
                    }
                    if let Some(&curr_close) = sd.close.get(bar) {
                        if curr_close > max_close {
                            let entry_px = sd.close[bar];
                            let entry = entry_px * (1.0 - TAKER_FEE);
                            let entry_bar_next = bar + 1;
                            let n = sd.close.len();

                            // Chandelier trailing stop
                            let mut highest_high = sd.high[entry_bar_next];
                            let mut exit_bar = (entry_bar_next + HOLD_MAX).min(n.saturating_sub(1));
                            for b in entry_bar_next..exit_bar.min(n.saturating_sub(1)) {
                                highest_high = highest_high.max(sd.high[b]);
                                let atr_val = atr_at(&sd.high, &sd.low, &sd.close, CHAND_PERIOD, b);
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
                                break;
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

    if equity_curve.is_empty() { return None; }
    let ret = (equity - 1.0) * 100.0;
    let sh = annualised_sharpe(&daily_rets);
    let dd = max_dd_pct(&equity_curve);
    Some((ret, sh, dd, total_trades, equity_curve))
}

#[tokio::main]
async fn main() -> Result<()> {
    let t0 = Instant::now();
    eprintln!("╔══════════════════════════════════════════════════════╗");
    eprintln!("║  Chandelier ATR Multiplier Fine-Grain Hyperopt     ║");
    eprintln!("║  Sweep: {:.2} to {:.2} step {:.3} (81 values)      ║", MULT_MIN, MULT_MAX, MULT_STEP);
    eprintln!("╚══════════════════════════════════════════════════════╝\n");

    let loader = DataLoader::new(None, None);

    let mults: Vec<f64> = {
        let mut v = vec![];
        let mut x = MULT_MIN;
        while x <= MULT_MAX + MULT_STEP / 2.0 {
            v.push((x * 100.0).round() / 100.0);
            x += MULT_STEP;
        }
        v
    };
    eprintln!("{} values: {:.2} → {:.2}\n", mults.len(), mults.first().unwrap(), mults.last().unwrap());

    // Load all data
    let mut all_syms: std::collections::HashSet<String> = std::collections::HashSet::new();
    for (_, syms) in UNIVERSES {
        for &s in *syms { all_syms.insert(s.to_string()); }
    }

    let mut raw_cache: HashMap<String, DataFrame> = HashMap::new();
    let mut min_len = usize::MAX;
    for sym in all_syms.iter() {
        match loader.fetch_with_cache(sym, "1d", CANDLES).await {
            Ok(df) => {
                min_len = min_len.min(df.height());
                raw_cache.insert(sym.clone(), df);
            }
            Err(e) => { eprintln!("  WARNING: {} load failed: {}", sym, e); }
        }
    }

    let n = min_len.saturating_sub(CHAND_PERIOD + 10).min(2800);
    eprintln!("Common bars: {}\n", n);

    // Build SymData
    let mut sym_data_map: HashMap<String, SymData> = HashMap::new();
    for sym in all_syms.iter() {
        if let Some(df) = raw_cache.get(sym) {
            macro_rules! col_vec {
                ($name:expr) => {{
                    let chunked = df.column($name)?.f64()?;
                    chunked.into_iter().filter_map(|x| x).take(n).collect::<Vec<_>>()
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

    let mut all_results: Vec<(String, f64, f64, f64, f64, usize, bool)> = vec![];
    let mut all_equity_curves: Vec<(String, f64, Vec<f64>)> = vec![];

    for &(univ_name, symbols) in UNIVERSES {
        let sym_strings: Vec<String> = symbols.iter().map(|s| s.to_string()).collect();
        let all_loaded = sym_strings.iter().all(|s| sym_data_map.contains_key(s));
        if !all_loaded {
            eprintln!("{:>20} SKIPPED", univ_name);
            continue;
        }

        let total_windows = n.saturating_sub(TRAIN_BARS + TEST_BARS) / TEST_BARS;
        if total_windows == 0 {
            eprintln!("{:>20} SKIPPED", univ_name);
            continue;
        }
        eprintln!("{:>20} ({} syms, {} windows)", univ_name, sym_strings.len(), total_windows);

        for mult in &mults {
            let mut window_rets = vec![];
            let mut window_sharpes = vec![];
            let mut window_dds = vec![];
            let mut total_trades = 0_usize;
            let mut pos_windows = 0_usize;
            let mut chart_equity: Option<Vec<f64>> = None;

            for wi in 0..total_windows {
                let train_end = TRAIN_BARS + wi * TEST_BARS;
                let test_start = train_end;
                let test_end = (test_start + TEST_BARS).min(n);

                if test_end.saturating_sub(test_start) < 5 { continue; }

                if let Some((ret, sh, dd, trades, equity)) = run_sim(
                    &sym_data_map, &sym_strings, *mult, test_start, test_end,
                ) {
                    window_rets.push(ret);
                    window_sharpes.push(sh);
                    window_dds.push(dd);
                    total_trades += trades;
                    if ret > 0.0 { pos_windows += 1; }
                    if chart_equity.is_none() { chart_equity = Some(equity); }
                }
            }

            if window_rets.is_empty() { continue; }

            let avg_ret = window_rets.iter().sum::<f64>() / window_rets.len() as f64;
            let avg_sh = window_sharpes.iter().sum::<f64>() / window_sharpes.len() as f64;
            let avg_dd = window_dds.iter().sum::<f64>() / window_dds.len() as f64;
            let pass_rate = pos_windows as f64 / total_windows as f64 * 100.0;
            let pass = pass_rate >= 50.0;

            eprintln!("  M={:.2} | ret={:+8.2}% sh={:7.3} dd={:7.2}% | {}/{}w {}t{}",
                mult, avg_ret, avg_sh, avg_dd, pos_windows, total_windows, total_trades,
                if pass { " ✓" } else { "" });

            all_results.push((univ_name.to_string(), *mult, avg_sh, avg_ret, avg_dd, total_trades, pass));
            if let Some(eq) = chart_equity {
                all_equity_curves.push((univ_name.to_string(), *mult, eq));
            }
        }
    }

    // Aggregate across universes
    let mut mult_agg: HashMap<i64, (f64, f64, f64, i64, i64, usize)> = HashMap::new();
    for (_univ, mult, sharpe, ret, dd, trades, pass) in &all_results {
        let key = (*mult * 100.0).round() as i64;
        let e = mult_agg.entry(key).or_insert((0.0_f64, 0.0_f64, 0.0_f64, 0_i64, 0_i64, 0_usize));
        e.0 += sharpe;
        e.1 += ret;
        e.2 += dd;
        e.3 += *trades as i64;
        e.4 += if *pass { 1 } else { 0 };
        e.5 += 1;
    }

    let mut ranked: Vec<(f64, f64, f64, f64, i64, i64, usize)> = mult_agg.iter()
        .map(|(&key, &(s, r, d, t, p, c))| {
            let cnt = c as f64;
            (key as f64 / 100.0, s / cnt, r / cnt, d / cnt, t, p, c)
        }).collect();
    ranked.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());

    // Write per-universe CSV
    {
        let mut f = File::create(CSV_OUT)?;
        writeln!(f, "universe,mult,avg_sharpe,avg_return_pct,avg_max_dd_pct,total_trades,pass")?;
        for (_univ, mult, sharpe, ret, dd, trades, pass) in &all_results {
            writeln!(f, "{},{:.2},{:.4},{:.4},{:.4},{},{}", _univ, mult, sharpe, ret, dd, trades, pass)?;
        }
    }

    // Write aggregated CSV
    {
        let mut f = File::create(UNIV_CSV_OUT)?;
        writeln!(f, "mult,avg_sharpe,avg_return_pct,avg_max_dd_pct,total_trades,pass_count,n_universes")?;
        for (mult, sharpe, ret, dd, trades, passes, count) in &ranked {
            writeln!(f, "{:.2},{:.4},{:.4},{:.4},{},{},{}", mult, sharpe, ret, dd, trades, passes, count)?;
        }
    }

    // Write equity curves
    {
        let mut f = File::create(EQUITY_OUT)?;
        writeln!(f, "universe,mult,step,equity")?;
        for (univ, mult, equity) in &all_equity_curves {
            for (step, eq) in equity.iter().enumerate() {
                writeln!(f, "{},{:.2},{},{:.6}", univ, mult, step, eq)?;
            }
        }
    }

    // Write JSON summary
    {
        let mut f = File::create(SUMMARY_JSON)?;
        writeln!(f, "{{")?;
        writeln!(f, "  \"parameter\": \"chandelier_mult\",")?;
        writeln!(f, "  \"sweep_min\": {:.2},", MULT_MIN)?;
        writeln!(f, "  \"sweep_max\": {:.2},", MULT_MAX)?;
        writeln!(f, "  \"sweep_step\": {:.3},", MULT_STEP)?;
        writeln!(f, "  \"n_values\": {},", ranked.len())?;
        writeln!(f, "  \"ranked\": [")?;
        for (i, (mult, sharpe, ret, dd, trades, passes, count)) in ranked.iter().take(30).enumerate() {
            writeln!(f, "    {{\"rank\":{},\"mult\":{:.2},\"avg_sharpe\":{:.4},\"avg_return_pct\":{:.4},\"avg_dd_pct\":{:.4},\"total_trades\":{},\"pass_count\":{},\"n_universes\":{}}}{}",
                i + 1, mult, sharpe, ret, dd, trades, passes, count,
                if i < 29 { "," } else { "" })?;
        }
        writeln!(f, "  ],")?;
        if let Some((wm, ws, _wr, wd, wt, wp, _wc)) = ranked.first() {
            writeln!(f, "  \"winner\": {{\"mult\":{:.2},\"avg_sharpe\":{:.4},\"avg_dd_pct\":{:.4},\"total_trades\":{},\"pass_count\":{}}},",
                wm, ws, wd, wt, wp)?;
        }
        let base_sharpe = mult_agg.get(&25).map(|v| v.0 / v.5 as f64).unwrap_or(0.0);
        writeln!(f, "  \"baseline_mult\": 2.5,")?;
        writeln!(f, "  \"baseline_sharpe\": {:.4},", base_sharpe)?;
        writeln!(f, "  \"chart_mults\": [")?;
        for (i, (m, _, _, _, _, _, _)) in ranked.iter().take(5).enumerate() {
            writeln!(f, "    {:.2}{}", m, if i < 4 { "," } else { "" })?;
        }
        writeln!(f, "  ],")?;
        writeln!(f, "  \"elapsed_seconds\": {},", t0.elapsed().as_secs())?;
        writeln!(f, "  \"universes\": {},", UNIVERSES.len())?;
        writeln!(f, "}}")?;
    }

    // Print top 20
    println!("\n╔══════════════════════════════════════════════════════════════╗");
    println!("║  TOP 20 by Avg OOS Sharpe (across {} universes)          ║", UNIVERSES.len());
    println!("╠══════════════════════════════════════════════════════════════╣");
    println!("{:>4} {:>6} {:>10} {:>10} {:>10} {:>8} {:>8}", "Rank", "Mult", "AvgSharpe", "AvgRet%", "AvgDD%", "Trades", "Pass#");
    println!("╠══════════════════════════════════════════════════════════════╣");
    for (i, (mult, sharpe, ret, dd, trades, passes, count)) in ranked.iter().take(20).enumerate() {
        let marker = if (*mult - 2.5).abs() < 0.001 {
            " ←BASE".to_string()
        } else if i == 0 {
            " ★WIN".to_string()
        } else {
            String::new()
        };
        println!("{:>4} {:>6.2} {:>10.4} {:>10.2} {:>10.2} {:>8} {:>5}/{}{}",
            i + 1, mult, sharpe, ret, dd, trades, passes, count, marker);
    }
    println!("╚══════════════════════════════════════════════════════════════╝");

    let elapsed = t0.elapsed();
    eprintln!("\nDone: {:.1}s", elapsed.as_secs_f64());
    eprintln!("Results: {}", CSV_OUT);
    eprintln!("Aggregated: {}", UNIV_CSV_OUT);
    eprintln!("Equity:  {}", EQUITY_OUT);
    eprintln!("JSON:    {}", SUMMARY_JSON);

    Ok(())
}
