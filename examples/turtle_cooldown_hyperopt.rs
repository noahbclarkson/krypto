//! Turtle+Chandelier — Extensive Freshness Cooldown Hyperparameter Sweep
//!
//! Parameter: FRESHNESS_COOLDOWN (bars after exit before re-entering same symbol)
//! Range: 0..=30 (step 1) — 31 values tested
//! Baseline: cd=0 (no cooldown)
//!
//! Step 1: Coarse scan all 31 values on Base5 (6 windows)
//! Step 2: Fine validation top 3 on all 9 universes
//! Step 3: Export equity curves for winner + baseline + runner-ups

use anyhow::Result;
use krypto::data::loader::DataLoader;

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
const CHAND_PERIOD: usize = 20;
const CHAND_MULT: f64 = 2.15;
const TURTLE_ENTRY: usize = 21;
const TURTLE_ATR_PERIOD: usize = 24;
const VOL_LOOKBACK: usize = 2;
const TURTLE_ATR_MULT: f64 = 2.00;

// Extended cooldown sweep: 0..=30 (step 1)
const COOLDOWNS: &[usize] = &[
    0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10,
    11, 12, 13, 14, 15, 16, 17, 18, 19, 20,
    21, 22, 23, 24, 25, 26, 27, 28, 29, 30,
];

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

const CSV_OUT: &str = "snapshots/turtle_cooldown_sweep.csv";
const MD_OUT: &str = "snapshots/turtle_cooldown_sweep.md";
const EQUITY_CSV: &str = "snapshots/turtle_cooldown_equity.csv";

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

struct WfResult {
    ret: f64,
    sharpe: f64,
    trades: usize,
    max_dd: f64,
    pass: bool,
    daily_equity: Vec<f64>,
}

fn run_window(
    data: &HashMap<String, SymData>,
    symbols: &[&str],
    start_bar: usize,
    end_bar: usize,
    cooldown: usize,
) -> WfResult {
    let test_bars = end_bar - start_bar;
    let mut equity = 1.0_f64;
    let mut peak = 1.0_f64;
    let mut trades = 0;
    let mut active = vec![false; symbols.len()];
    let mut pos_prices = vec![0.0_f64; symbols.len()];
    let mut holds = vec![0_usize; symbols.len()];
    let mut stops = vec![0.0_f64; symbols.len()];
    let mut last_exit_bar = vec![0_isize; symbols.len()];
    let mut daily_equity = vec![1.0_f64];

    for bar_idx in 0..test_bars {
        let bar = start_bar + bar_idx;

        // Vol ranking
        let mut vol_scores = Vec::new();
        for sym in symbols {
            if let Some(d) = data.get(*sym) {
                let vol_now = d.vol.get(bar).copied().unwrap_or(0.0) * d.close.get(bar).copied().unwrap_or(0.0);
                let vol_hist: Vec<f64> = (bar.saturating_sub(VOL_LOOKBACK)..=bar)
                    .filter_map(|i| Some(d.vol.get(i)? * d.close.get(i)?))
                    .collect();
                let avg_vol = if vol_hist.is_empty() { 0.0 } else { vol_hist.iter().sum::<f64>() / vol_hist.len() as f64 };
                vol_scores.push(if avg_vol > 0.0 { vol_now / avg_vol } else { 0.0 });
            } else {
                vol_scores.push(0.0);
            }
        }
        let mut ranked: Vec<usize> = (0..symbols.len()).collect();
        ranked.sort_by(|&a, &b| vol_scores[b].partial_cmp(&vol_scores[a]).unwrap());
        let in_pos = active.iter().filter(|&&x| x).count();

        // Entry with freshness cooldown
        for &sym_i in ranked.iter().take(POSITION_CAP) {
            if active[sym_i] { continue; }
            if in_pos >= POSITION_CAP { break; }
            if cooldown > 0 && last_exit_bar[sym_i] >= 0 {
                let bars_since_exit = (bar_idx as isize) - last_exit_bar[sym_i];
                if bars_since_exit < cooldown as isize {
                    continue;
                }
            }
            if let Some(d) = data.get(symbols[sym_i]) {
                if turtle_signal(&d.close, TURTLE_ENTRY, bar) {
                    let px = d.close.get(bar).copied().unwrap_or(0.0);
                    if px <= 0.0 { continue; }
                    active[sym_i] = true;
                    pos_prices[sym_i] = px;
                    holds[sym_i] = 0;
                    stops[sym_i] = px - atr_at(&d.high, &d.low, &d.close, TURTLE_ATR_PERIOD, bar) * TURTLE_ATR_MULT;
                }
            }
        }

        // Position management
        for (sym_i, sym) in symbols.iter().enumerate() {
            if !active[sym_i] { continue; }
            if let Some(d) = data.get(*sym) {
                let close = d.close.get(bar).copied().unwrap_or(0.0);
                let low = d.low.get(bar).copied().unwrap_or(0.0);
                let atr_val = atr_at(&d.high, &d.low, &d.close, TURTLE_ATR_PERIOD, bar);
                let chand_stop = close - atr_val * CHAND_MULT;
                let turtle_stop = close - atr_val * TURTLE_ATR_MULT;
                let best_stop = chand_stop.max(turtle_stop);

                holds[sym_i] += 1;
                let exit = if holds[sym_i] >= HOLD_MAX {
                    true
                } else if low <= stops[sym_i] {
                    true
                } else {
                    stops[sym_i] = stops[sym_i].max(best_stop);
                    false
                };

                if exit {
                    active[sym_i] = false;
                    let sell_px = if low <= stops[sym_i] { stops[sym_i] } else { close };
                    let ret = (sell_px - pos_prices[sym_i]) / pos_prices[sym_i] - TAKER_FEE;
                    equity *= 1.0 + ret;
                    trades += 1;
                    last_exit_bar[sym_i] = bar_idx as isize;
                    pos_prices[sym_i] = 0.0;
                }
            }
        }

        if equity > peak { peak = equity; }
        daily_equity.push(equity);
    }

    let ret = (equity - 1.0) * 100.0;
    let mut peak_e = 1.0_f64;
    let mut max_dd = 0.0_f64;
    for &e in &daily_equity {
        if e > peak_e { peak_e = e; }
        let dd = (peak_e - e) / peak_e;
        if dd > max_dd { max_dd = dd; }
    }
    let max_dd_pct = max_dd * 100.0;

    // Compute Sharpe from daily equity returns
    let daily_rets: Vec<f64> = daily_equity.windows(2)
        .map(|w| (w[1] - w[0]) / w[0])
        .collect();
    let sharpe = if daily_rets.len() >= 2 {
        let mn: f64 = daily_rets.iter().sum::<f64>() / daily_rets.len() as f64;
        let sd = (daily_rets.iter().map(|x| (x - mn).powi(2)).sum::<f64>() / daily_rets.len() as f64).sqrt();
        if sd > 0.0 { mn * 365.0_f64.sqrt() / sd } else { 0.0 }
    } else { 0.0 };

    WfResult { ret, sharpe, trades, max_dd: max_dd_pct, pass: trades >= MIN_TRADES && sharpe > 0.0, daily_equity }
}

#[tokio::main]
async fn main() -> Result<()> {
    let start = Instant::now();
    let loader = DataLoader::new(None, None);
    let mut data: HashMap<String, SymData> = HashMap::new();

    // Collect all unique symbols across universes
    let mut all_syms = std::collections::HashSet::new();
    for (_, syms) in UNIVERSES {
        for s in *syms { all_syms.insert(s.to_string()); }
    }

    for sym in all_syms {
        match loader.fetch_with_cache(&sym, "1d", CANDLES).await {
            Ok(df) => {
                let close: Vec<f64> = df.column("close")?.f64()?.into_no_null_iter().collect();
                let high: Vec<f64> = df.column("high")?.f64()?.into_no_null_iter().collect();
                let low: Vec<f64> = df.column("low")?.f64()?.into_no_null_iter().collect();
                let vol: Vec<f64> = df.column("volume")?.f64()?.into_no_null_iter().collect();
                data.insert(sym, SymData { close, high, low, vol });
            }
            Err(e) => { eprintln!("WARNING: {} load failed: {}", sym, e); }
        }
    }

    let min_len = UNIVERSES.iter().filter_map(|(_, s)| {
        data.get(s[0]).map(|d| d.close.len())
    }).min().unwrap_or(0);

    let n_windows = (min_len.saturating_sub(TRAIN_BARS)) / TEST_BARS;
    println!("Bars: {}, Windows: {}, Cooldowns: {}", min_len, n_windows, COOLDOWNS.len());
    println!("Running {} cooldown values × {} universes × {} windows = {} runs",
        COOLDOWNS.len(), UNIVERSES.len(), n_windows,
        COOLDOWNS.len() * UNIVERSES.len() * n_windows);

    // ========== PHASE 1: Scan all cooldowns on Base5 ==========
    println!("\n=== PHASE 1: Base5 coarse scan (all {} cooldowns) ===", COOLDOWNS.len());
    let base5_syms = &["BTCUSDT","ETHUSDT","SOLUSDT","XRPUSDT","DOGEUSDT","ADAUSDT"][..];

    let mut base5_results: HashMap<usize, Vec<WfResult>> = HashMap::new();
    for &cooldown in COOLDOWNS {
        let mut results = Vec::new();
        for w in 0..n_windows {
            let train_end = TRAIN_BARS + w * TEST_BARS;
            let test_end = (train_end + TEST_BARS).min(min_len);
            if test_end - train_end < 30 { continue; }
            let r = run_window(&data, base5_syms, train_end, test_end, cooldown);
            results.push(r);
        }
        base5_results.insert(cooldown, results);
    }

    // Compute aggregate Sharpe per cooldown on Base5
    let mut base5_sharpe: Vec<(usize, f64, f64, usize)> = Vec::new();
    for (&cd, results) in &base5_results {
        let n = results.len();
        if n == 0 { continue; }
        let avg_sharpe: f64 = results.iter().map(|r| r.sharpe).sum::<f64>() / n as f64;
        let avg_ret: f64 = results.iter().map(|r| r.ret).sum::<f64>() / n as f64;
        let total_trades: usize = results.iter().map(|r| r.trades).sum();
        base5_sharpe.push((cd, avg_sharpe, avg_ret, total_trades));
    }
    base5_sharpe.sort_by(|a, b| b.1.partial_cmp(&b.1).unwrap());

    println!("\nBase5 top 10 cooldowns by Sharpe:");
    println!("{:>4} {:>10} {:>10} {:>8}", "CD", "Sharpe", "Ret%", "Trades");
    for (cd, sharpe, ret, trades) in base5_sharpe.iter().take(10) {
        println!("{:4} {:10.4} {:10.1} {:8}", cd, sharpe, ret, trades);
    }

    // Top 3 candidates
    let top3: Vec<usize> = base5_sharpe.iter().take(3).map(|(cd,_,_,_)| *cd).collect();
    let baseline_cd = 0;
    println!("\nPhase 2 candidates: {:?}", top3);
    println!("Baseline: cd=0");

    // ========== PHASE 2: Full 9-universe validation of top3 + baseline ==========
    println!("\n=== PHASE 2: Full 9-universe validation ===");
    let validation_cds: Vec<usize> = {
        let mut v = vec![baseline_cd];
        for &c in &top3 { if c != baseline_cd { v.push(c); } }
        v
    };

    let mut csv = File::create(CSV_OUT)?;
    writeln!(csv, "universe,cooldown,window,ret,sharpe,trades,max_dd,pass")?;

    let mut md = File::create(MD_OUT)?;
    writeln!(md, "# Turtle Freshness Cooldown Hyperopt — 2026-04-18\n")?;
    writeln!(md, "## Phase 1: Base5 Scan (0..=30 step 1)\n")?;
    writeln!(md, "| CD | Base5 Sharpe | Base5 Ret% | Trades |")?;
    writeln!(md, "|----|-------------|-----------|--------|")?;
    for (cd, sharpe, ret, trades) in &base5_sharpe {
        let marker = if *cd == baseline_cd { " [BASE]" } else if top3.contains(cd) { " ★" } else { "" };
        writeln!(md, "| {} | {:.4} | {:.1}% | {} |{}|", cd, sharpe, ret, trades, marker)?;
    }

    writeln!(md, "\n## Phase 2: Full 9-Universe Walk-Forward (Top 3 + Baseline)\n")?;
    writeln!(md, "| Universe | CD | Pass/Total | Avg Ret% | Avg Sharpe | Trades |")?;
    writeln!(md, "|----------|----|------------|----------|-------------|--------|")?;

    let mut overall: HashMap<usize, (usize, usize, f64, f64, f64)> = HashMap::new();
    // Equity curves: aggregated across windows per universe
    let mut equity_curves: HashMap<usize, HashMap<String, Vec<f64>>> = HashMap::new();

    for (uname, symbols) in UNIVERSES {
        for &cd in &validation_cds {
            let mut total_pass = 0;
            let mut total_ret = 0.0;
            let mut total_sharpe = 0.0;
            let mut total_trades = 0_usize;
            let mut n = 0;
            // Accumulate equity across windows
            let mut agg_equity: Vec<f64> = vec![1.0_f64];

            for w in 0..n_windows {
                let train_end = TRAIN_BARS + w * TEST_BARS;
                let test_end = (train_end + TEST_BARS).min(min_len);
                if test_end - train_end < 30 { continue; }

                let result = run_window(&data, symbols, train_end, test_end, cd);
                total_pass += result.pass as usize;
                total_ret += result.ret;
                total_sharpe += result.sharpe;
                total_trades += result.trades;
                n += 1;

                // Compound the equity curve across windows
                let start_equity = *agg_equity.last().unwrap();
                for &e in &result.daily_equity {
                    agg_equity.push(start_equity * e);
                }

                writeln!(csv, "{},{},{},{:.2},{:.3},{},{:.2},{}",
                    uname, cd, w, result.ret, result.sharpe, result.trades, result.max_dd, result.pass)?;
            }

            if n == 0 { continue; }
            let avg_ret = total_ret / n as f64;
            let avg_sharpe = total_sharpe / n as f64;
            let pass_rate = total_pass as f64 / n as f64 * 100.0;

            writeln!(md, "| {} | {} | {}/{} ({:.0}%) | {:.1}% | {:.3} | {} |",
                uname, cd, total_pass, n, pass_rate, avg_ret, avg_sharpe, total_trades)?;

            overall.entry(cd).or_insert_with(|| (0, 0, 0.0, 0.0, 0.0));
            if let Some(e) = overall.get_mut(&cd) {
                e.0 += total_pass;
                e.1 += n;
                e.2 += avg_ret;
                e.3 += avg_sharpe;
                e.4 += total_trades as f64;
            }

            equity_curves.entry(cd).or_insert_with(|| HashMap::new());
            if let Some(uni_map) = equity_curves.get_mut(&cd) {
                uni_map.insert(uname.to_string(), agg_equity);
            }
        }
    }

    // ========== PHASE 3: Equity curves ==========
    // Write aggregated equity curves to CSV for Python
    let mut eq_csv = File::create(EQUITY_CSV)?;
    writeln!(eq_csv, "day,cd,universe,equity")?;

    for (&cd, uni_map) in &equity_curves {
        for (uname, equity) in uni_map {
            for (day_idx, &eq) in equity.iter().enumerate() {
                writeln!(eq_csv, "{},{},{},{:.6}", day_idx, cd, uname, eq)?;
            }
        }
    }

    // Summary
    writeln!(md, "\n## Summary\n")?;
    writeln!(md, "| CD | Pass Rate | Avg Sharpe | Avg Ret% | Total Trades | vs Baseline |")?;
    writeln!(md, "|----|-----------|------------|----------|--------------|-------------|")?;

    let mut summary: Vec<(usize, (usize, usize, f64, f64, f64))> = overall.into_iter().collect();
    summary.sort_by_key(|(cd, _)| *cd);

    let base_sharpe = {
        let mut bs = 0.0_f64;
        for (cd, vals) in summary.iter() {
            if *cd == baseline_cd {
                let &(_, _, _, s, _) = vals;
                bs = s / UNIVERSES.len() as f64;
                break;
            }
        }
        bs
    };

    let mut winner_cd = baseline_cd;
    let mut winner_sharpe = base_sharpe;

    for (cd, (pass, total, ret_sum, sharpe_sum, trades)) in &summary {
        let n_uni = UNIVERSES.len();
        let avg_sharpe = sharpe_sum / n_uni as f64;
        let avg_ret = ret_sum / n_uni as f64;
        let pass_rate = *pass as f64 / *total as f64 * 100.0;
        let delta = (avg_sharpe - base_sharpe) / base_sharpe * 100.0;
        let delta_str = if *cd == baseline_cd {
            "baseline".to_string()
        } else {
            format!("{:+.1}%", delta)
        };
        let verdict = if *cd == baseline_cd {
            "BASELINE".to_string()
        } else if avg_sharpe >= base_sharpe * 0.98 {
            format!("✅ {:.3}", avg_sharpe)
        } else {
            format!("🪦 {:.3}", avg_sharpe)
        };
        writeln!(md, "| {} | {:.1}% | {:.3} | {:.1}% | {:.0} | {} | {} |",
            cd, pass_rate, avg_sharpe, avg_ret, trades, delta_str, verdict)?;

        if avg_sharpe > winner_sharpe {
            winner_sharpe = avg_sharpe;
            winner_cd = *cd;
        }
    }

    writeln!(md, "\n**WINNER: cd={} (Sharpe {:.3}, vs baseline {:.3})**", winner_cd, winner_sharpe, base_sharpe)?;

    println!("\nResults: {} and {}", CSV_OUT, MD_OUT);
    println!("Equity curves: {}", EQUITY_CSV);
    println!("WINNER: cd={} (Sharpe {:.3} vs baseline {:.3})", winner_cd, winner_sharpe, base_sharpe);
    println!("Total runtime: {:.1}s", start.elapsed().as_secs_f64());

    Ok(())
}
