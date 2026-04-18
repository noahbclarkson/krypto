//! ============================================================
//! FULL-RANGE TURTLE_ATR_PERIOD HYPEROPTIMIZATION
//! ============================================================
//!
//! Background:
//!   - Prior FINE sweep (2026-04-16): ATR ∈ {18..=35} step 1 (18 values)
//!     Winner: ATR=24 (+3.6% Sharpe vs coarse ATR=25)
//!     BUT: Fine sweep only covered 18-35. Full logical range (5-100) never tested.
//!   - Prior COARSE sweep (2026-04-12): ATR ∈ {10,15,20,25,28,30,35,40,50,60}
//!     Winner: ATR=25 with Sharpe 6.287
//!
//! Target: TURTLE_ATR_PERIOD — ATR lookback for Turtle ATR dual-exit stop.
//!
//! Design:
//!   - Full range sweep: ATR ∈ {5,10,15,20,25,30,35,40,45,50,55,60,65,70,75,80,85,90,95,100} (20 values)
//!   - Production params: EP=21, CHAND_PERIOD=20, CHAND_MULT=2.15, CAP=3, HM=45, FEE=0.1%
//!   - Phase 1: Base5 all 20 ATR values (6 windows) — fast candidate identification
//!   - Phase 2: Top 5 candidates × 9 universes — full robustness validation
//!   - Exports: per-bar equity curves for Baseline(ATR=25), Winner, and top runners

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
const CHAND_PERIOD: usize = 20;
const CHAND_MULT: f64 = 2.15;

const ATR_VALUES: &[usize] = &[
    5, 10, 15, 20, 25, 30, 35,
    40, 45, 50, 55, 60, 65, 70,
    75, 80, 85, 90, 95, 100,
];

const UNIVERSES: &[(&str, &[&str])] = &[
    ("Base5",        &["BTCUSDT","ETHUSDT","SOLUSDT","XRPUSDT","DOGEUSDT","ADAUSDT"]),
    ("NoDOGE",       &["BTCUSDT","ETHUSDT","SOLUSDT","XRPUSDT","ADAUSDT"]),
    ("Legacy4",      &["BTCUSDT","ETHUSDT","XRPUSDT","LTCUSDT","EOSUSDT"]),
    ("Legacy5BNB",   &["BTCUSDT","ETHUSDT","XRPUSDT","LTCUSDT","BNBUSDT","EOSUSDT"]),
    ("OldGuardNoBNB",&["BTCUSDT","ETHUSDT","XRPUSDT","LTCUSDT","EOSUSDT","BCHUSDT"]),
    ("LargeCaps5",   &["BTCUSDT","ETHUSDT","SOLUSDT","XRPUSDT","BNBUSDT","ADAUSDT"]),
    ("Legacy3",      &["BTCUSDT","XRPUSDT","LTCUSDT","EOSUSDT"]),
    ("LowVolume5",  &["XRPUSDT","LTCUSDT","EOSUSDT","BCHUSDT","ADAUSDT"]),
    ("OldGuard4",    &["BTCUSDT","XRPUSDT","LTCUSDT","EOSUSDT","BCHUSDT"]),
];

const CSV_OUT: &str = "snapshots/turtle_atr_full_sweep.csv";
const EQUITY_CSV_OUT: &str = "snapshots/turtle_atr_phase1_equity.csv";
const SUMMARY_MD: &str = "snapshots/turtle_atr_full_sweep.md";

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

fn turtle_signal(close: &[f64], entry_period: usize, idx: usize) -> bool {
    if idx < entry_period { return false; }
    let entry_high = close[(idx + 1 - entry_period)..=idx].iter().fold(f64::NEG_INFINITY, |m, &v| m.max(v));
    close.get(idx).map(|&c| c >= entry_high).unwrap_or(false)
}

fn run_sim(
    sym_data_map: &HashMap<String, SymData>,
    symbols: &[String],
    test_start: usize,
    test_end: usize,
    atr_period: usize,
) -> (f64, f64, f64, usize, f64, bool, Vec<f64>) {
    let n = test_end - test_start;
    let mut equity = 1.0_f64;
    let mut equity_curve = vec![1.0_f64; n];
    let mut trades = 0usize;
    let mut wins = 0usize;
    let mut peak = 1.0_f64;
    let mut max_dd = 0.0_f64;
    let mut active_entry: Option<(String, f64)> = None;
    let mut entry_bar_next = 0usize;

    for b in 0..n {
        equity_curve[b] = equity;

        if let Some((sym, entry_px)) = active_entry.take() {
            let sd = match sym_data_map.get(&sym) {
                Some(sd) => sd,
                None => {
                    active_entry = Some((sym, entry_px));
                    continue;
                }
            };
            let bar_idx = test_start + b;

            let highest_high_chand = sd.high[(bar_idx + 1 - CHAND_PERIOD).max(test_start)..=bar_idx]
                .iter().fold(f64::NEG_INFINITY, |m, &v| m.max(v));
            let atr_chand = atr_at(&sd.high, &sd.low, &sd.close, CHAND_PERIOD, bar_idx);
            let trail_chand = highest_high_chand - CHAND_MULT * atr_chand;

            let highest_high_turtle = sd.high[(bar_idx + 1 - atr_period).max(test_start)..=bar_idx]
                .iter().fold(f64::NEG_INFINITY, |m, &v| m.max(v));
            let atr_turtle = atr_at(&sd.high, &sd.low, &sd.close, atr_period, bar_idx);
            let trail_turtle = highest_high_turtle - 2.00 * atr_turtle;

            let exit_px = sd.close.get(bar_idx).copied().unwrap_or(entry_px);
            let max_bar = (entry_bar_next + HOLD_MAX).min(test_end.saturating_sub(1));

            let should_exit = b >= max_bar || exit_px <= trail_chand || exit_px <= trail_turtle;

            if should_exit {
                let ret = (exit_px - entry_px) / entry_px - TAKER_FEE;
                equity *= 1.0 + ret;
                trades += 1;
                if ret > 0.0 { wins += 1; }
                active_entry = None;
            } else {
                active_entry = Some((sym, entry_px));
            }
        }

        if active_entry.is_none() {
            let mut candidates: Vec<(f64, &String)> = Vec::new();
            for sym in symbols {
                let sd = match sym_data_map.get(sym) {
                    Some(sd) => sd,
                    None => continue,
                };
                let bar_idx = test_start + b;
                if bar_idx < EP { continue; }
                if !turtle_signal(&sd.close, EP, bar_idx) { continue; }
                let vol_score = sd.vol.get(bar_idx).copied().unwrap_or(0.0);
                candidates.push((vol_score, sym));
            }
            candidates.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap());
            if let Some((_, sym)) = candidates.into_iter().take(POSITION_CAP).next() {
                let sd = sym_data_map.get(sym).unwrap();
                let bar_idx = test_start + b;
                let entry_px = sd.close.get(bar_idx).copied().unwrap_or(1.0);
                active_entry = Some(((*sym).clone(), entry_px));
                entry_bar_next = b;
            }
        }

        peak = peak.max(equity);
        let dd = (peak - equity) / peak;
        if dd > max_dd { max_dd = dd; }
    }

    let total_ret = (equity - 1.0) * 100.0;
    let win_rate = if trades > 0 { wins as f64 / trades as f64 * 100.0 } else { 0.0 };
    let daily_rets: Vec<f64> = equity_curve.windows(2)
        .map(|w| (w[1] - w[0]) / w[0])
        .collect();
    let mean_ret = daily_rets.iter().sum::<f64>() / daily_rets.len().max(1) as f64;
    let std_ret = (daily_rets.iter().map(|r| (r - mean_ret).powi(2)).sum::<f64>()
        / daily_rets.len().max(1) as f64).sqrt();
    let sharpe = if std_ret > 0.0 { mean_ret / std_ret * (252.0_f64).sqrt() } else { 0.0 };
    let pass = trades >= MIN_TRADES && sharpe > 0.0;

    (total_ret, sharpe, max_dd * 100.0, trades, win_rate, pass, equity_curve)
}

fn run_universe(
    sym_data_map: &HashMap<String, SymData>,
    symbols: &[String],
    atr_period: usize,
) -> (usize, usize, f64, f64, f64, usize, Vec<f64>) {
    let n = sym_data_map.get(&symbols[0])
        .map(|sd| sd.close.len()).unwrap_or(0);
    if n < TRAIN_BARS + TEST_BARS * 2 { return (0, 0, 0.0, 0.0, 0.0, 0, vec![]); }

    let total_windows = n.saturating_sub(TRAIN_BARS + TEST_BARS) / TEST_BARS;
    let mut pass_count = 0usize;
    let mut total_ret = 0.0_f64;
    let mut total_sharpe = 0.0_f64;
    let mut total_dd = 0.0_f64;
    let mut total_trades = 0usize;
    let mut agg_equity = vec![1.0_f64; 0];

    for wi in 0..total_windows {
        let test_start = TRAIN_BARS + wi * TEST_BARS;
        let test_end = (test_start + TEST_BARS).min(n);
        if test_end.saturating_sub(test_start) < 5 { continue; }

        let (ret, sh, dd, trades, _, pass, equity) =
            run_sim(sym_data_map, symbols, test_start, test_end, atr_period);

        if pass { pass_count += 1; }
        total_ret += ret;
        total_sharpe += sh;
        total_dd = total_dd.max(dd);
        total_trades += trades;

        if wi == 0 {
            agg_equity = equity;
        } else {
            for (i, &eq) in equity.iter().enumerate() {
                if i < agg_equity.len() { agg_equity[i] *= eq; }
            }
        }
    }

    let nw = total_windows.min(6);
    (pass_count, nw,
     total_sharpe / nw as f64,
     total_ret / nw as f64,
     total_dd,
     total_trades,
     agg_equity)
}

#[tokio::main]
async fn main() -> Result<()> {
    let t0 = Instant::now();
    eprintln!("\n=== TURTLE_ATR_PERIOD FULL-RANGE SWEEP ===");
    eprintln!("ATR range: {:?} ({} values)", ATR_VALUES, ATR_VALUES.len());
    eprintln!("Params: EP={}, CHAND({},{}), CAP={}, HM={}, FEE=0.1%",
        EP, CHAND_PERIOD, CHAND_MULT, POSITION_CAP, HOLD_MAX);

    // Load data
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
                let bars = df.height();
                min_len = min_len.min(bars);
                raw_cache.insert(sym.clone(), df);
                eprintln!("  {}: {} bars", sym, bars);
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
    eprintln!("\nLoaded {} symbols, {} bars. Running sweep...\n", sym_data_map.len(), n);

    // ── Phase 1: Base5 all ATR values ────────────────────────────────────────
    eprintln!("--- Phase 1: Base5 all {} ATR values ---", ATR_VALUES.len());
    let base5: Vec<String> = UNIVERSES[0].1.iter().map(|&s| s.to_string()).collect();
    let mut phase1: Vec<(usize, f64, f64, f64, usize, usize, usize)> = Vec::new();

    for &tatr in ATR_VALUES {
        let (passes, nw, sh, ret, dd, trades, _) = run_universe(&sym_data_map, &base5, tatr);
        phase1.push((tatr, sh, ret, dd, trades, passes, nw));
        eprintln!("  ATR={:3} | sh={:+.3} ret={:+.1}% pass={}/{} DD={:.1}% trades={}",
            tatr, sh, ret, passes, nw, dd, trades);
    }
    phase1.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());

    // ── Phase 2: Top 5 candidates × 9 universes ──────────────────────────────
    eprintln!("\n--- Phase 2: Top 5 + baseline × 9 universes ---");
    let top5: Vec<usize> = phase1.iter().take(5).map(|(t,_,_,_,_,_,_)| *t).collect();
    let baseline_atr = 25usize;

    let candidates: Vec<usize> = if top5.contains(&baseline_atr) {
        top5.clone()
    } else {
        top5.iter().take(4).chain(std::iter::once(&baseline_atr)).copied().collect()
    };

    let mut phase2: HashMap<usize, Vec<(String, usize, usize, f64, f64, f64, usize)>> = HashMap::new();
    let mut phase2_equities: HashMap<usize, Vec<f64>> = HashMap::new();

    for &tatr in &candidates {
        eprintln!("\n  === ATR={} ===", tatr);
        for (label, symbols) in UNIVERSES {
            let syms: Vec<String> = symbols.iter().map(|&s| s.to_string()).collect();
            let (passes, nw, sh, ret, dd, trades, eq) =
                run_universe(&sym_data_map, &syms, tatr);

            phase2.entry(tatr).or_default()
                .push((label.to_string(), passes, nw, sh, ret, dd, trades));
            if &**label == "Base5" {
                phase2_equities.insert(tatr, eq);
            }
            eprintln!("    {}: sh={:+.3} pass={}/{} ret={:+.1}% DD={:.1}%",
                label, sh, passes, nw, ret, dd);
        }
    }

    let mut phase2_agg: Vec<(usize, usize, usize, f64, f64, f64, usize)> = Vec::new();
    for &tatr in &candidates {
        if let Some(res) = phase2.get(&tatr) {
            let tot_pass: usize = res.iter().map(|r| r.1).sum();
            let tot_nw: usize = res.iter().map(|r| r.2).sum();
            let avg_sh = res.iter().map(|r| r.3).sum::<f64>() / res.len().max(1) as f64;
            let avg_ret = res.iter().map(|r| r.4).sum::<f64>() / res.len().max(1) as f64;
            let max_dd = res.iter().map(|r| r.5).fold(0.0f64, |a, b| a.max(b));
            let tot_tr: usize = res.iter().map(|r| r.6).sum();
            phase2_agg.push((tatr, tot_pass, tot_nw, avg_sh, avg_ret, max_dd, tot_tr));
        }
    }
    phase2_agg.sort_by(|a, b| b.3.partial_cmp(&a.3).unwrap());

    // ── Export equity curves for charting ───────────────────────────────────
    eprintln!("\n--- Exporting equity curves ---");
    let mut eq_f = File::create(EQUITY_CSV_OUT)?;
    writeln!(eq_f, "atr_period,bar,equity")?;
    for tatr in phase2_agg.iter().take(4).map(|(t,_,_,_,_,_,_)| *t) {
        if let Some(eq) = phase2_equities.get(&tatr) {
            for (bar, &e) in eq.iter().enumerate() {
                writeln!(eq_f, "{},{},{:.6}", tatr, bar, e)?;
            }
            eprintln!("  ATR={:3}: final equity = {:.4}x ({} bars)",
                tatr, eq.last().copied().unwrap_or(1.0), eq.len());
        }
    }
    eprintln!("Wrote: {}", EQUITY_CSV_OUT);

    // ── Write CSV ────────────────────────────────────────────────────────────
    {
        let mut f = File::create(CSV_OUT)?;
        writeln!(f, "atr_period,phase,universe,passes,total_w,avg_sharpe,avg_return,worst_dd,trades")?;
        for &(atr, sh, ret, dd, trades, passes, nw) in &phase1 {
            writeln!(f, "{},phase1,Base5,{},{},{:.4},{:.2},{:.2},{}", atr, passes, nw, sh, ret, dd, trades)?;
        }
        for &(atr, passes, nw, sh, ret, dd, trades) in &phase2_agg {
            writeln!(f, "{},phase2,ALL,{},{},{:.4},{:.2},{:.2},{}", atr, passes, nw, sh, ret, dd, trades)?;
        }
        eprintln!("Wrote: {}", CSV_OUT);
    }

    // ── Write markdown report ────────────────────────────────────────────────
    let mut md = File::create(SUMMARY_MD)?;
    writeln!(md, "# TURTLE_ATR_PERIOD Full-Range Hyperopt")?;
    writeln!(md, "")?;
    writeln!(md, "## Configuration")?;
    writeln!(md, "- Strategy: Turtle breakout + Chandelier({},{}) dual-exit", CHAND_PERIOD, CHAND_MULT)?;
    writeln!(md, "- ATR range: {{5,10,15,20,25,30,35,40,45,50,55,60,65,70,75,80,85,90,95,100}} (20 values)")?;
    writeln!(md, "- Fixed: EP={}, CHAND_PERIOD={}, CHAND_MULT={}, CAP={}, HM={}, FEE=0.1%", EP, CHAND_PERIOD, CHAND_MULT, POSITION_CAP, HOLD_MAX)?;
    writeln!(md, "- Walk-forward: {} train / {} test", TRAIN_BARS, TEST_BARS)?;
    writeln!(md, "")?;
    writeln!(md, "## Phase 1: Base5 Full-Range (20 values)")?;
    writeln!(md, "| Rank | ATR | Sharpe | Ret% | DD% | Trades | Pass |")?;
    writeln!(md, "|------|-----|--------|------|-----|--------|------|")?;
    for (i, &(atr, sh, ret, dd, trades, passes, nw)) in phase1.iter().enumerate() {
        let pct = passes as f64 / nw as f64 * 100.0;
        writeln!(md, "| {} | {} | {:+.4} | {:+.1}% | {:.1}% | {} | {}/{} ({:.0}%) |",
            i+1, atr, sh, ret, dd, trades, passes, nw, pct)?;
    }
    writeln!(md, "")?;
    writeln!(md, "## Phase 2: Top Candidates × 9 Universes")?;
    writeln!(md, "| Rank | ATR | Global Pass | Pass% | Sharpe | Ret% | Worst DD | Trades |")?;
    writeln!(md, "|------|-----|-------------|-------|--------|------|----------|--------|")?;
    for (i, &(atr, passes, nw, sh, ret, dd, trades)) in phase2_agg.iter().enumerate() {
        let pct = passes as f64 / nw as f64 * 100.0;
        writeln!(md, "| {} | {} | {}/{} | {:.0}% | {:+.4} | {:+.1}% | {:.1}% | {} |",
            i+1, atr, passes, nw, pct, sh, ret, dd, trades)?;
    }
    writeln!(md, "")?;
    writeln!(md, "## Per-Universe Detail (Phase 2)")?;
    for &tatr in &candidates {
        if let Some(res) = phase2.get(&tatr) {
            writeln!(md, "### ATR={}", tatr)?;
            writeln!(md, "| Universe | Pass | Sharpe | Ret% | DD% | Trades |")?;
            writeln!(md, "|----------|------|--------|------|-----|--------|")?;
            for &(ref univ, passes, nw, sh, ret, dd, trades) in res {
                writeln!(md, "| {} | {}/{} | {:+.3} | {:+.1}% | {:.1}% | {} |",
                    univ, passes, nw, sh, ret, dd, trades)?;
            }
            writeln!(md, "")?;
        }
    }
    writeln!(md, "## Winner vs Baseline (ATR=25)")?;
    if let Some(&(win_atr, _, _, win_sh, win_ret, win_dd, _)) = phase2_agg.first() {
        if let Some(&(base_atr, _, _, base_sh, base_ret, base_dd, _)) = phase2_agg.iter().find(|(a,_,_,_,_,_,_)| *a == baseline_atr) {
            writeln!(md, "| Metric | ATR={} (Baseline) | ATR={} (Winner) | Delta |", base_atr, win_atr)?;
            writeln!(md, "|--------|-----------------|----------------|-------|")?;
            writeln!(md, "| Sharpe | {:+.4} | {:+.4} | {:+.4} |", base_sh, win_sh, win_sh - base_sh)?;
            writeln!(md, "| Return | {:+.1}% | {:+.1}% | {:+.1}% |", base_ret, win_ret, win_ret - base_ret)?;
            writeln!(md, "| Worst DD | {:.1}% | {:.1}% | |", base_dd, win_dd)?;
        }
    }
    writeln!(md, "")?;
    writeln!(md, "## Key insight")?;
    // Compare extremes
    if let Some(&(atr5_sh, ..)) = phase1.iter().find(|(a,_,_,_,_,_,_)| *a == 5) {
        if let Some(&(atr100_sh, ..)) = phase1.iter().find(|(a,_,_,_,_,_,_)| *a == 100) {
            writeln!(md, "- ATR=5 (very short, high sensitivity): Sharpe {:+.4}", atr5_sh)?;
            writeln!(md, "- ATR=100 (very long, smooth): Sharpe {:+.4}", atr100_sh)?;
            writeln!(md, "- Fine-sweep optimal range was 18-35. Full-range confirms or rejects.")?;
        }
    }
    writeln!(md, "")?;
    writeln!(md, "## Output files")?;
    writeln!(md, "- Full results: {}", CSV_OUT)?;
    writeln!(md, "- Equity curves: {}", EQUITY_CSV_OUT)?;
    writeln!(md, "- Report: {}", SUMMARY_MD)?;
    let elapsed = t0.elapsed();
    writeln!(md, "")?;
    writeln!(md, "## Runtime: {:.1}s ({:.1}min)", elapsed.as_secs_f64(), elapsed.as_secs_f64() / 60.0)?;
    eprintln!("\nWrote: {}", SUMMARY_MD);

    // Final summary
    eprintln!("\n=== SUMMARY ===");
    eprintln!("Phase1 Base5 ranking:");
    for (i, &(atr, sh, ret, _, trades, passes, nw)) in phase1.iter().take(5).enumerate() {
        let pct = passes as f64 / nw as f64 * 100.0;
        eprintln!("  #{:2} ATR={:3} | sh={:+.4} ret={:+.1}% pass={:.0}% trades={}",
            i+1, atr, sh, ret, pct, trades);
    }
    eprintln!("\nPhase2 9-universe:");
    for (i, &(atr, passes, nw, sh, ret, dd, _)) in phase2_agg.iter().take(4).enumerate() {
        let pct = passes as f64 / nw as f64 * 100.0;
        eprintln!("  #{:2} ATR={:3} | sh={:+.4} pass={}/{} ({:.0}%) ret={:+.1}% DD={:.1}%",
            i+1, atr, sh, passes, nw, pct, ret, dd);
    }
    if let Some(&(win_atr, _, _, win_sh, _, _, _)) = phase2_agg.first() {
        eprintln!("\n🏆 WINNER: TURTLE_ATR_PERIOD={} (avg Sharpe {:+.4})", win_atr, win_sh);
    }
    eprintln!("Done in {:.1}s ({:.1}min)", elapsed.as_secs_f64(), elapsed.as_secs_f64() / 60.0);

    Ok(())
}
