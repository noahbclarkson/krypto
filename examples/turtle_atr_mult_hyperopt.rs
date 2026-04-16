//! Turtle ATR Multiplier Hyperopt
//!
//! HYPOTHESIS: The Turtle ATR exit (N=25) and Chandelier ATR exit (N=28)
//! use the SAME multiplier (2.0). But these are different mechanisms operating
//! on different lookback periods. The Turtle ATR might have a different optimal
//! multiplier than Chandelier.
//!
//! DESIGN:
//! - Sweep TURTLE_ATR_MULT across {1.0, 1.5, 2.0, 2.5, 3.0, 3.5, 4.0, 4.5, 5.0}
//! - Chandelier multiplier fixed at 2.0 (validated optimal)
//! - 9 universes × ~6 windows each (54 total)
//! - Walk-forward: 252-bar train / 252-bar test
//! - DUAL_EXIT: Chandelier(28, 2.0) OR Turtle_ATR(25, M) fires first
//!
//! BASELINE: TURTLE_ATR_MULT=2.0 (same as CHAND_MULT=2.0) → same as current code

use anyhow::Result;
use krypto::data::loader::DataLoader;
use polars::frame::DataFrame;
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

// THE SWEEP PARAMETER
const TURTLE_ATR_MULT_VALS: &[f64] = &[1.0, 1.5, 2.0, 2.5, 3.0, 3.5, 4.0, 4.5, 5.0];
const BASELINE_MULT: f64 = 2.0;

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

const CSV_OUT: &str = "snapshots/turtle_atr_mult_sweep.csv";
const EQUITY_CSV: &str = "snapshots/turtle_atr_mult_equity.csv";
const SUMMARY_MD: &str = "snapshots/turtle_atr_mult_summary.md";

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
    daily_rets: Vec<f64>,
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
        let top_syms: Vec<String> = scores.into_iter().take(POSITION_CAP)
            .map(|(s, _)| s.to_string()).collect();

        if top_syms.is_empty() {
            equity_curve.push(equity);
            bar += 1;
            continue;
        }

        let mut entered = false;
        for sym in &top_syms {
            if let Some(sd) = sym_data.get(sym) {
                if bar >= TURTLE_ENTRY + 1 && bar < sd.close.len() {
                    if turtle_signal(&sd.close, &sd.high, TURTLE_ENTRY, bar) {
                        let entry_px = sd.close[bar];
                        let entry_bar_next = bar + 1;
                        let n = sd.close.len();

                        // DUAL_EXIT: Chandelier(28, 2.0) OR Turtle_ATR(25, turtle_atr_mult)
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
                            let trail_turtle = highest_high_turtle - turtle_atr_mult * atr_turtle;

                            if sd.close[b] < trail_chand || sd.close[b] < trail_turtle {
                                exit_bar = b;
                                break;
                            }
                        }

                        if let Some(&exit_px) = sd.close.get(exit_bar) {
                            let exit = exit_px * (1.0 - TAKER_FEE);
                            let gross_ret = exit / entry_px - 1.0;
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

    WfResult { ret, sharpe, max_dd, trades: total_trades, win_rate, pass, equity_curve, daily_rets }
}

#[tokio::main]
async fn main() -> Result<()> {
    let t0 = Instant::now();

    let mult_strs: Vec<String> = TURTLE_ATR_MULT_VALS.iter().map(|m| format!("{:.1}", m)).collect();
    eprintln!("==== Turtle ATR Multiplier Hyperopt ====");
    eprintln!("Sweeping: {:?}", mult_strs);
    eprintln!("Baseline: {:.1} (= current CHAND_MULT)", BASELINE_MULT);
    eprintln!("DUAL_EXIT: Chandelier(28, 2.0) OR Turtle_ATR(25, M)\n");

    let loader = DataLoader::new(None, None);
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

    // Per-universe per-mult results: (universe, mult) → (sharpe_sum, pass, total, ret_sum, dd_sum, trades_sum, wr_sum)
    let mut results: HashMap<String, (f64, usize, usize, f64, f64, usize, f64)> = HashMap::new();

    for &(label, symbols) in UNIVERSES {
        let symbols: Vec<String> = symbols.iter().map(|s| s.to_string()).collect();
        let all_loaded = symbols.iter().all(|s| sym_data_map.contains_key(s));
        if !all_loaded { continue; }

        let total_windows = n.saturating_sub(TRAIN_BARS + TEST_BARS) / TEST_BARS;
        if total_windows == 0 { continue; }

        eprintln!("==== {:<18} ==== {} syms, {} windows", label, symbols.len(), total_windows);

        for &m in TURTLE_ATR_MULT_VALS {
            let key = format!("{}|{:.1}", label, m);

            let mut pass_count = 0usize;
            let mut sum_sharpe = 0.0_f64;
            let mut sum_ret = 0.0_f64;
            let mut sum_dd = 0.0_f64;
            let mut sum_trades = 0usize;
            let mut sum_wr = 0.0_f64;

            for wi in 0..total_windows {
                let train_end = TRAIN_BARS + wi * TEST_BARS;
                let test_start = train_end;
                let test_end = (test_start + TEST_BARS).min(n);
                if test_end.saturating_sub(test_start) < 5 { continue; }

                let r = run_sim(&sym_data_map, &symbols, test_start, test_end, m);

                let thin = if r.trades < MIN_TRADES { "THIN" } else { "OK" };
                let result = if r.pass { "PASS" } else { "FAIL" };
                eprintln!(
                    "  mult={:.1} W{:02} | {:+8.1}% sh={:6.2} DD={:5.1}% {:4}t {:3.0}% {} {}",
                    m, wi, r.ret, r.sharpe, r.max_dd, r.trades, r.win_rate, thin, result
                );

                sum_sharpe += r.sharpe;
                sum_ret += r.ret;
                sum_dd += r.max_dd;
                sum_trades += r.trades;
                sum_wr += r.win_rate;
                if r.pass { pass_count += 1; }
            }

            results.insert(key, (sum_sharpe, pass_count, total_windows, sum_ret, sum_dd, sum_trades, sum_wr));
        }
    }

    // Global aggregation by multiplier
    let mut global_by_mult: HashMap<String, (f64, usize, usize, f64, f64, usize, f64)> = HashMap::new();
    for &(label, _) in UNIVERSES {
        for &m in TURTLE_ATR_MULT_VALS {
            let key = format!("{:.1}", m);
            let full_key = format!("{}|{}", label, key);
            if let Some(v) = results.get(&full_key) {
                global_by_mult
                    .entry(key)
                    .and_modify(|e| {
                        e.0 += v.0; e.1 += v.1; e.2 += v.2; e.3 += v.3;
                        e.4 += v.4; e.5 += v.5; e.6 += v.6;
                    })
                    .or_insert(*v);
            }
        }
    }

    // Build summary sorted by avg Sharpe descending
    let mut summary: Vec<(String, f64, usize, usize, f64, f64, usize, f64)> = Vec::new();
    for &m in TURTLE_ATR_MULT_VALS {
        let key = format!("{:.1}", m);
        if let Some(&(sh, pass, total, ret, dd, trades, wr)) = global_by_mult.get(&key) {
            let avg_sharpe = sh / total as f64;
            let avg_ret = ret / total as f64;
            let avg_dd = dd / total as f64;
            let avg_wr = wr / total as f64;
            summary.push((key.clone(), avg_sharpe, pass, total, avg_ret, avg_dd, trades, avg_wr));
        }
    }
    summary.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());

    // Per-mult debug output
    for &m in TURTLE_ATR_MULT_VALS {
        let key = format!("{:.1}", m);
        if let Some(&(sh, pass, total, _, _, _, _)) = global_by_mult.get(&key) {
            let avg_sharpe = sh / total as f64;
            let pass_rate = pass as f64 / total as f64 * 100.0;
            let marker = if key == format!("{:.1}", BASELINE_MULT) { "  ←BASELINE" } else { "" };
            eprintln!("  mult={:.1}: avg_sharpe={:.4} pass={}/{} ({:.0}%){}", m, avg_sharpe, pass, total, pass_rate, marker);
        }
    }

    eprintln!("\n==== RANKING (by avg Sharpe) ====");
    for (i, entry) in summary.iter().enumerate() {
        let key = &entry.0;
        let sh = entry.1;
        let pass = entry.2;
        let total = entry.3;
        let ret = entry.4;
        let dd = entry.5;
        let trades = entry.6;
        let wr = entry.7;
        let pass_rate = pass as f64 / total as f64 * 100.0;
        let marker = if key == &format!("{:.1}", BASELINE_MULT) {
            "  ←BASELINE".to_string()
        } else if i == 0 {
            "  ←WINNER".to_string()
        } else {
            String::new()
        };
        eprintln!(
            "  #{:2} MULT={:4}  avg_sharpe={:7.4}  pass={:3}/{} ({:4.0}%)  ret={:+8.1}%  DD={:5.1}%  trades={:5}  wr={:4.1}%{}",
            i+1, key, sh, pass, total, pass_rate, ret, dd, trades, wr, marker
        );
    }

    let winner = summary.first().map(|(k,_,_,_,_,_,_,_)| k.clone()).unwrap_or_default();
    let winner_sharpe = summary.first().map(|(_,s,_,_,_,_,_,_)| *s).unwrap_or(0.0);
    let baseline_sharpe = summary.iter()
        .find(|(k,_,_,_,_,_,_,_)| *k == format!("{:.1}", BASELINE_MULT))
        .map(|(_,s,_,_,_,_,_,_)| *s).unwrap_or(0.0);
    let improvement = if baseline_sharpe != 0.0 {
        (winner_sharpe - baseline_sharpe) / baseline_sharpe * 100.0
    } else { 0.0 };

    eprintln!("\n==== WINNER: TURTLE_ATR_MULT={} ====", winner);
    eprintln!("  Avg Sharpe: {:.4}  (baseline {:.4}, delta={:+.2}%)", winner_sharpe, baseline_sharpe, improvement);

    // ── Write sweep CSV ──────────────────────────────────────────────────────
    {
        let mut f = File::create(CSV_OUT)?;
        writeln!(f, "mult,avg_sharpe,pass_count,total_windows,pass_rate_pct,avg_return_pct,avg_max_dd_pct,total_trades,avg_win_rate_pct")?;
        for entry in &summary {
            let key = &entry.0;
            let sh = entry.1;
            let pass = entry.2;
            let total = entry.3;
            let ret = entry.4;
            let dd = entry.5;
            let trades = entry.6;
            let wr = entry.7;
            let pr = pass as f64 / total as f64 * 100.0;
            writeln!(f, "{},{:.4},{},{},{:.1},{:.1},{:.1},{},{:.1}", key, sh, pass, total, pr, ret, dd, trades, wr)?;
        }
        eprintln!("\nWrote: {}", CSV_OUT);
    }

    // ── Write equity CSV (top 3 multipliers + baseline) ─────────────────────
    {
        let mut f = File::create(EQUITY_CSV)?;
        writeln!(f, "universe,mult,window_idx,cumulative_equity")?;

        let mut mults_to_show: Vec<String> = summary.iter().take(3).map(|(k,_,_,_,_,_,_,_)| k.clone()).collect();
        let baseline_key = format!("{:.1}", BASELINE_MULT);
        if !mults_to_show.contains(&baseline_key) {
            if let Some(idx) = summary.iter().position(|(k,_,_,_,_,_,_,_)| *k == baseline_key) {
                if idx >= 3 { mults_to_show.push(baseline_key); }
            }
        }

        for &(label, symbols) in UNIVERSES {
            let symbols: Vec<String> = symbols.iter().map(|s| s.to_string()).collect();
            let all_loaded = symbols.iter().all(|s| sym_data_map.contains_key(s));
            if !all_loaded { continue; }

            let total_windows = n.saturating_sub(TRAIN_BARS + TEST_BARS) / TEST_BARS;
            if total_windows == 0 { continue; }

            for m_str in &mults_to_show {
                let m: f64 = m_str.parse().unwrap_or(2.0);
                let mut cumulative_equity = 1.0_f64;

                for wi in 0..total_windows {
                    let train_end = TRAIN_BARS + wi * TEST_BARS;
                    let test_start = train_end;
                    let test_end = (test_start + TEST_BARS).min(n);
                    if test_end.saturating_sub(test_start) < 5 { continue; }

                    let r = run_sim(&sym_data_map, &symbols, test_start, test_end, m);
                    if let Some(&last_eq) = r.equity_curve.last() {
                        cumulative_equity *= last_eq;
                        writeln!(f, "{},{},{},{:.6}", label, m_str, wi, cumulative_equity)?;
                    }
                }
            }
        }
        eprintln!("Wrote: {}", EQUITY_CSV);
    }

    // ── Write summary MD ─────────────────────────────────────────────────────
    {
        let mut f = File::create(SUMMARY_MD)?;
        writeln!(f, "# Turtle ATR Multiplier Hyperopt")?;
        writeln!(f, "\nDate: 2026-04-12")?;
        writeln!(f, "\n## Hypothesis")?;
        writeln!(f, "The Turtle ATR exit (N=25) and Chandelier ATR exit (N=28) use the SAME multiplier (2.0).")?;
        writeln!(f, "But these are different mechanisms on different lookback periods.")?;
        writeln!(f, "The Turtle ATR exit might have a different optimal multiplier than Chandelier.")?;
        writeln!(f, "\n## Design")?;
        writeln!(f, "- Chandelier: fixed at ({}, {}) (validated optimal)", CHAND_PERIOD, CHAND_MULT)?;
        writeln!(f, "- Turtle ATR period: fixed at {} (validated optimal)", TURTLE_ATR_PERIOD)?;
        writeln!(f, "- SWEPT: Turtle ATR multiplier ∈ {{ {:?} }}", mult_strs)?;
        writeln!(f, "- Baseline (current): {:.1} (same as Chandelier multiplier)", BASELINE_MULT)?;
        writeln!(f, "- Universes: 9, Windows: ~54 total")?;
        writeln!(f, "\n## Results")?;
        writeln!(f, "\n| Rank | Mult | Avg Sharpe | Pass Rate | Avg Return | Avg DD | Trades |")?;
        writeln!(f, "|------|------|------------|-----------|------------|--------|--------|")?;
        for (i, entry) in summary.iter().enumerate() {
            let key = &entry.0;
            let sh = entry.1;
            let pass = entry.2;
            let total = entry.3;
            let ret = entry.4;
            let dd = entry.5;
            let trades = entry.6;
            let pr = pass as f64 / total as f64 * 100.0;
            let marker = if key == &format!("{:.1}", BASELINE_MULT) {
                " ←BASELINE".to_string()
            } else if i == 0 {
                " ←WINNER".to_string()
            } else {
                String::new()
            };
            writeln!(f, "| {}{} | {:.1} | {:.4} | {}/{} ({:.0}%) | {:+.1}% | {:.1}% | {} |",
                i+1, marker, key, sh, pass, total, pr, ret, dd, trades)?;
        }
        writeln!(f, "\n## Winner")?;
        writeln!(f, "**TURTLE_ATR_MULT = {}** (avg Sharpe {:.4}, baseline {:.4}, delta={:+.2}%)",
            winner, winner_sharpe, baseline_sharpe, improvement)?;
        writeln!(f, "\n## Files")?;
        writeln!(f, "- {} — per-mult summary", CSV_OUT)?;
        writeln!(f, "- {} — equity curve data (top 3 + baseline)", EQUITY_CSV)?;
        writeln!(f, "\nElapsed: {:.1}s", t0.elapsed().as_secs_f64())?;
        eprintln!("\nWrote: {}", SUMMARY_MD);
    }

    eprintln!("\nTotal elapsed: {:.1}s", t0.elapsed().as_secs_f64());
    Ok(())
}
