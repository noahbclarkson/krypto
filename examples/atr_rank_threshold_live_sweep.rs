//! ATR_RANK_THRESHOLD Sweep — Turtle-Only Live Path
//!
//! PURPOSE: Extensively sweep ATR_RANK_THRESHOLD using the Turtle-only live exit
//! (matching src/live/bot.rs exactly) to find the most robust threshold.
//!
//! PRIOR: atr_rank_filter_prod_sweep.rs used CHAND_P=7 dual Chandelier+Turtle exit.
//!         regime_atr_hyperopt.rs used AP/LB/T joint sweep but only on BTC alone.
//!         This harness tests T under the ACTUAL live bot path (Turtle-only).
//!
//! HYPOTHESIS: T=5 was selected from a coarse 21-value grid (0,5,10,...,100).
//!             An extensive sweep (0..=100 step 1) may find a better robustness point.
//!
//! RANGE: T ∈ [0..=100 step 1] — 101 values × 9 universes × 7 WF windows = 6,363 runs
//! PARAMS: EP=21, ATR(24,2.0), HM=12, CAP=3, ATR_RANK(AP=12,LB=42,T=<swept>)
//! EXIT:   Turtle-only (highest_high - ATR_MULT*ATR) — matches live bot exactly

use anyhow::Result;
use krypto::data::loader::DataLoader;
use std::collections::HashMap;
use std::fs::File;
use std::io::Write;

const CANDLES: u32 = 3000;
const TRAIN_BARS: usize = 252;
const TEST_BARS: usize = 252;

const TURTLE_ENTRY: usize = 21;
const TURTLE_ATR_PERIOD: usize = 24;
const TURTLE_ATR_MULT: f64 = 2.00;
const HOLD_MAX: usize = 12;
const POSITION_CAP: usize = 3;
const MIN_TRADES: usize = 3;
const TAKER_FEE: f64 = 0.001;

const REGIME_ATR_PERIOD: usize = 12;
const REGIME_LOOKBACK: usize = 42;

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

struct SymData {
    close: Vec<f64>,
    high: Vec<f64>,
    low: Vec<f64>,
    vol: Vec<f64>,
}

fn atr_at(h: &[f64], l: &[f64], c: &[f64], p: usize, idx: usize) -> f64 {
    if idx < p { return 0.0; }
    let mut trs = Vec::with_capacity(p);
    for i in (idx + 1 - p)..=idx {
        let hi = h[i];
        let lo = l[i];
        let c0 = c[i.saturating_sub(1)];
        trs.push((hi - lo).max((hi - c0).abs()).max((lo - c0).abs()));
    }
    trs.iter().sum::<f64>() / p as f64
}

fn max_close(start: usize, end: usize, close: &[f64]) -> f64 {
    close[start..end].iter().fold(f64::NEG_INFINITY, |a, &b| a.max(b))
}

fn btc_atr_pct(btc_data: &SymData, period: usize, lookback: usize, idx: usize) -> f64 {
    if idx < period.max(lookback) { return 50.0; }
    let curr_atr = atr_at(&btc_data.high, &btc_data.low, &btc_data.close, period, idx);
    let mut hist = Vec::with_capacity(lookback);
    for j in (idx + 1 - lookback)..=idx {
        if j >= period {
            hist.push(atr_at(&btc_data.high, &btc_data.low, &btc_data.close, period, j));
        }
    }
    if hist.is_empty() { return 50.0; }
    let count = hist.iter().filter(|&&x| x < curr_atr).count();
    (count as f64 / hist.len() as f64) * 100.0
}

fn annualised_sharpe(daily_rets: &[f64]) -> f64 {
    if daily_rets.is_empty() { return 0.0; }
    let mean = daily_rets.iter().sum::<f64>() / daily_rets.len() as f64;
    let var = daily_rets.iter().map(|x| (x - mean).powi(2)).sum::<f64>() / daily_rets.len() as f64;
    if var == 0.0 { return 0.0; }
    (mean / var.sqrt()) * (365.0_f64).sqrt()
}

fn max_dd_from(equity: &[f64]) -> f64 {
    let mut max_dd = 0.0;
    let mut peak = 1.0;
    for &val in equity {
        if val > peak { peak = val; }
        let dd = 1.0 - val / peak;
        if dd > max_dd { max_dd = dd; }
    }
    max_dd * 100.0
}

struct SimResult {
    equity: f64,
    sharpe: f64,
    dd: f64,
    trades: usize,
    daily_rets: Vec<f64>,
}

struct Position {
    sym: String,
    entry: f64,
    atr_buf: Vec<f64>,
    highest_high: f64,
    bars_held: usize,
}

fn run_sim(
    sym_data: &HashMap<String, SymData>,
    symbols: &[String],
    test_start: usize,
    test_end: usize,
    threshold: f64,
) -> SimResult {
    let mut equity = 1.0_f64;
    let mut equity_curve = vec![1.0_f64];
    let mut daily_rets = Vec::new();
    let mut total_trades = 0usize;
    let mut positions: Vec<Position> = Vec::new();

    let mut bar = test_start;
    while bar + 2 < test_end {
        // --- Regime gate: block entry if BTC ATR rank below threshold ---
        let btc = sym_data.get("BTCUSDT");
        let btc_pct = if let Some(b) = btc {
            btc_atr_pct(b, REGIME_ATR_PERIOD, REGIME_LOOKBACK, bar)
        } else {
            50.0
        };

        if btc_pct >= threshold {
            // --- Entry: Turtle breakout on ALL symbols (live bot matches this)
            // Live bot checks all symbols; this sweep iterates over all configured symbols
            for sym in symbols {
                if positions.len() >= POSITION_CAP { break; }
                if positions.iter().any(|p| &p.sym == sym) { continue; }
                let d = match sym_data.get(sym) {
                    Some(d) => d,
                    None => continue,
                };
                if bar < TURTLE_ENTRY { continue; }
                let max_close_val = max_close(bar - TURTLE_ENTRY, bar, &d.close);
                if d.close[bar] > max_close_val {
                    let atr = atr_at(&d.high, &d.low, &d.close, TURTLE_ATR_PERIOD, bar);
                    positions.push(Position {
                        sym: sym.clone(),
                        entry: d.close[bar],
                        atr_buf: vec![atr; TURTLE_ATR_PERIOD.min(bar + 1)],
                        highest_high: d.high[bar],
                        bars_held: 0,
                    });
                }
            }
        }

        // --- Exits ---
        let mut new_positions = Vec::new();
        for mut p in positions {
            let d = match sym_data.get(&p.sym) {
                Some(d) => d,
                None => {
                    new_positions.push(p);
                    continue;
                }
            };
            p.bars_held += 1;

            // Update ATR buffer and highest high
            let atr = atr_at(&d.high, &d.low, &d.close, TURTLE_ATR_PERIOD, bar);
            p.atr_buf.push(atr);
            if p.atr_buf.len() > TURTLE_ATR_PERIOD * 2 {
                p.atr_buf.remove(0);
            }
            if d.high[bar] > p.highest_high {
                p.highest_high = d.high[bar];
            }

            // Turtle ATR trailing stop
            let avg_atr = p.atr_buf.iter().sum::<f64>() / p.atr_buf.len() as f64;
            let turtle_stop = p.highest_high - TURTLE_ATR_MULT * avg_atr;

            let mut exited = false;

            // Long exit: price drops to or below turtle stop
            if d.low[bar] <= turtle_stop {
                let exit_px = turtle_stop.min(d.close[bar]);
                let gross = (exit_px / p.entry - 1.0) * 100.0;
                let fee_gross = gross - TAKER_FEE * 200.0;
                equity *= 1.0 + fee_gross / 100.0;
                total_trades += 1;
                exited = true;
            }

            // HOLD_MAX enforced independent of ATR warmup
            if !exited && p.bars_held >= HOLD_MAX {
                let exit_px = d.close[bar] * (1.0 - TAKER_FEE);
                let gross = (exit_px / p.entry - 1.0) * 100.0;
                let fee_gross = gross - TAKER_FEE * 200.0;
                equity *= 1.0 + fee_gross / 100.0;
                total_trades += 1;
                exited = true;
            }

            if !exited {
                new_positions.push(p);
            }
        }
        positions = new_positions;

        equity_curve.push(equity);
        bar += 1;
    }

    // Daily returns
    for i in 1..equity_curve.len() {
        let r = (equity_curve[i] / equity_curve[i - 1]) - 1.0;
        daily_rets.push(r);
    }

    let sharpe = annualised_sharpe(&daily_rets);
    let dd = max_dd_from(&equity_curve);
    SimResult { equity, sharpe, dd, trades: total_trades, daily_rets }
}

#[tokio::main]
async fn main() -> Result<()> {
    let now = std::time::Instant::now();
    let thresholds: Vec<f64> = (0..=100).map(|t| t as f64).collect();
    let n_thresholds = thresholds.len();

    println!("=== ATR_RANK_THRESHOLD Extensive Sweep (Turtle-Only Live Path) ===");
    println!("Values: 0..=100 step 1 ({} thresholds)", n_thresholds);
    println!("Exit: Turtle-only (highest_high - ATR_MULT*ATR) — matches src/live/bot.rs");
    println!("");

    let mut summary_file = File::create("snapshots/atr_rank_threshold_live_sweep.csv")?;
    writeln!(summary_file, "universe,window,threshold,return_pct,sharpe,max_dd_pct,trades,pass")?;

    // Per-threshold aggregate accumulators: (threshold, pass_count, total_trades, sum_return, sum_sharpe, sum_dd)
    let mut agg: Vec<(f64, usize, usize, f64, f64, f64)> =
        thresholds.iter().map(|&t| (t, 0, 0, 0.0, 0.0, 0.0)).collect();

    // Load all data for all symbols across all universes
    println!("Loading data...");
    let loader = DataLoader::new(None, None);
    let all_symbols: std::collections::HashSet<_> = UNIVERSES
        .iter()
        .flat_map(|(_, s)| s.iter())
        .copied()
        .collect();

    let mut sym_data_map: HashMap<String, SymData> = HashMap::new();
    let mut min_len = usize::MAX;

    for &sym in &all_symbols {
        let df = loader.fetch_data(sym, "1d", CANDLES).await?;
        let close: Vec<f64> = df.column("close")?.f64()?.into_no_null_iter().collect();
        let high: Vec<f64> = df.column("high")?.f64()?.into_no_null_iter().collect();
        let low: Vec<f64> = df.column("low")?.f64()?.into_no_null_iter().collect();
        let vol: Vec<f64> = df.column("volume")?.f64()?.into_no_null_iter().collect();
        if close.len() < min_len {
            min_len = close.len();
        }
        sym_data_map.insert(sym.to_string(), SymData { close, high, low, vol });
    }

    let n_windows = (min_len.saturating_sub(TRAIN_BARS)) / TEST_BARS;
    let total_runs = n_thresholds * UNIVERSES.len() * n_windows;
    println!(
        "Loaded {} symbols. Min length: {} bars. {} windows. {} total runs.",
        sym_data_map.len(),
        min_len,
        n_windows,
        total_runs
    );

    let mut run_count = 0usize;
    for &(uname, symbols) in UNIVERSES {
        let sym_strs: Vec<String> = symbols.iter().map(|&s| s.to_string()).collect();

        print!("  {uname}... ");
        let start = std::time::Instant::now();

        for win in 0..n_windows {
            let test_start = TRAIN_BARS + win * TEST_BARS;
            let test_end = (test_start + TEST_BARS).min(min_len);
            if test_end <= test_start + MIN_TRADES {
                break;
            }

            for (ti, &t) in thresholds.iter().enumerate() {
                let r = run_sim(&sym_data_map, &sym_strs, test_start, test_end, t);
                let pass = r.sharpe > 0.0 && r.equity > 1.0;

                writeln!(summary_file, "{},{},{},{:.4},{:.6},{:.4},{},{}",
                    uname, win, t, r.equity * 100.0 - 100.0, r.sharpe, r.dd, r.trades, if pass {1} else {0})?;

                // Accumulate
                agg[ti].1 += if pass { 1 } else { 0 };
                agg[ti].2 += r.trades;
                agg[ti].3 += r.equity * 100.0 - 100.0;
                agg[ti].4 += r.sharpe;
                agg[ti].5 += r.dd;

                run_count += 1;
            }
        }
        println!(
            "{} windows in {:.1}s",
            n_windows,
            start.elapsed().as_secs_f64()
        );
    }

    // Write aggregate summary
    let mut agg_file = File::create("snapshots/atr_rank_threshold_live_summary.csv")?;
    writeln!(agg_file, "threshold,pass_count,total_trades,avg_return_pct,avg_sharpe,avg_dd_pct")?;
    for &(t, pass_cnt, total_tr, sum_ret, sum_sharpe, sum_dd) in &agg {
        let n = UNIVERSES.len() * n_windows;
        let avg_ret = sum_ret / n as f64;
        let avg_sharpe = sum_sharpe / n as f64;
        let avg_dd = sum_dd / n as f64;
        writeln!(agg_file, "{},{},{},{:.4},{:.6},{:.4}", t, pass_cnt, total_tr, avg_ret, avg_sharpe, avg_dd)?;
    }

    // Sort by pass_count desc, then avg_sharpe desc
    let mut sorted: Vec<(usize, (f64, usize, usize, f64, f64, f64))> =
        agg.into_iter().enumerate().map(|(i, a)| (i, a)).collect();
    sorted.sort_by(|a, b| {
        let na = UNIVERSES.len() * n_windows;
        let nb = UNIVERSES.len() * n_windows;
        let sa = a.1.4 / na as f64;
        let sb = b.1.4 / nb as f64;
        b.1.1.cmp(&a.1.1)
            .then_with(|| sb.partial_cmp(&sa).unwrap())
            .then_with(|| b.1.3.partial_cmp(&a.1.3).unwrap())
    });

    println!("\n=== TOP 20 THRESHOLDS (by pass count, then Sharpe) ===");
    println!("{:<6} {:>6} {:>8} {:>12} {:>10} {:>10}",
        "Rank", "T", "PASS", "AVG_RET%", "AVG_SHARPE", "AVG_DD%");
    for (rank, (ti, stats)) in sorted.iter().take(20).enumerate() {
        let t = *ti as f64;
        let n = UNIVERSES.len() * n_windows;
        let avg_sharpe = stats.4 / n as f64;
        let avg_ret = stats.3 / n as f64;
        let avg_dd = stats.5 / n as f64;
        println!(
            "  [{:>2}] T={:>3.0}: pass={:>3}/{}, sharpe={:>8.4}, ret={:>10.2}%, dd={:>7.2}%",
            rank + 1,
            t,
            stats.1,
            n,
            avg_sharpe,
            avg_ret,
            avg_dd
        );
    }

    // Identify winner
    let (best_t_idx, best_stats) = &sorted[0];
    let best_t = *best_t_idx as f64;
    let n = UNIVERSES.len() * n_windows;
    println!(
        "\n>>> WINNER: T={} — {} pass, Sharpe {:.4}, Ret {:.2}%, DD {:.2}%",
        best_t,
        best_stats.1,
        best_stats.4 / n as f64,
        best_stats.3 / n as f64,
        best_stats.5 / n as f64
    );

    // Top-5 thresholds for equity export
    let top5_ts: Vec<f64> = sorted.iter().take(5).map(|(ti, _)| *ti as f64).collect();
    let baseline_t = 0.0_f64;

    // Export equity for baseline + top 5 across all universes/windows
    let mut eq_file = File::create("snapshots/atr_rank_threshold_live_selected_equity.csv")?;
    writeln!(eq_file, "universe,window,bar_idx,equity_T_{}", baseline_t)?;
    for t in &top5_ts {
        write!(eq_file, ",equity_T_{}", t)?;
    }
    writeln!(eq_file)?;

    for &(uname, symbols) in UNIVERSES {
        let sym_strs: Vec<String> = symbols.iter().map(|&s| s.to_string()).collect();

        for win in 0..n_windows {
            let test_start = TRAIN_BARS + win * TEST_BARS;
            let test_end = (test_start + TEST_BARS).min(min_len);
            if test_end <= test_start + MIN_TRADES {
                break;
            }

            // Run all needed thresholds for this window
            let mut eqs: Vec<Vec<f64>> = Vec::new();
            // Baseline (T=0)
            let r_base = run_sim(&sym_data_map, &sym_strs, test_start, test_end, baseline_t);
            eqs.push(r_base.daily_rets);

            for &t in &top5_ts {
                let r = run_sim(&sym_data_map, &sym_strs, test_start, test_end, t);
                eqs.push(r.daily_rets);
            }

            // Compute cumulative equity from daily returns
            let n_bars = eqs[0].len();
            let mut equity_vals = vec![1.0_f64; eqs.len()];
            for i in 0..n_bars {
                for (ci, eq) in eqs.iter_mut().enumerate() {
                    if i < eq.len() {
                        equity_vals[ci] *= 1.0 + eq[i];
                    }
                }
                write!(eq_file, "{},{},{},{:.6}", uname, win, i, equity_vals[0])?;
                for (ci, _) in top5_ts.iter().enumerate() {
                    write!(eq_file, ",{:.6}", equity_vals[ci + 1])?;
                }
                writeln!(eq_file)?;
            }
        }
    }

    let elapsed = now.elapsed().as_secs_f64();
    println!("\n=== DONE in {:.1}s ({} runs/sec) ===", elapsed, run_count as f64 / elapsed);
    println!("Files written:");
    println!("  snapshots/atr_rank_threshold_live_sweep.csv       — per-window detail ({} rows)", run_count);
    println!("  snapshots/atr_rank_threshold_live_summary.csv     — aggregated by threshold ({} rows)", n_thresholds);
    println!("  snapshots/atr_rank_threshold_live_selected_equity.csv — time-series equity for baseline+top5");

    Ok(())
}
