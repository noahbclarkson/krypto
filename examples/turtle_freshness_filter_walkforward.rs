//! Turtle+Chandelier + Signal Freshness Filter Walk-Forward
//!
//! Idea #21: Only enter when last exit (stop or HOLD_MAX) was >= COOLDOWN bars ago.
//! Mechanistically different from ATR entry filter (market conditions vs trade history).
//!
//! Baseline (no cooldown): Turtle+Chandelier, all params frozen.
//! COOLDOWN ∈ {0, 3, 5, 10, 15, 20} bars after exit before re-entering same symbol.

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

// Freshness filter cooldown values to test
const COOLDOWNS: &[usize] = &[0, 3, 5, 10, 15, 20];

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

const CSV_OUT: &str = "snapshots/turtle_freshness_filter_wf.csv";
const MD_OUT: &str = "snapshots/turtle_freshness_filter_wf.md";

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

struct WfResult {
    ret: f64,
    sharpe: f64,
    trades: usize,
    max_dd: f64,
    pass: bool,
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
    // last_exit_bar[i] = bar index when symbol i last exited (-1 if never)
    let mut last_exit_bar = vec![0_isize; symbols.len()];
    // Simple equity tracking for Sharpe
    let mut daily_equity = vec![1.0_f64];

    for bar_idx in 0..test_bars {
        let bar = start_bar + bar_idx;

        // Compute vol ranking for this bar
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

        // Entry logic with freshness filter
        for &sym_i in ranked.iter().take(POSITION_CAP) {
            if active[sym_i] { continue; }
            if in_pos >= POSITION_CAP { break; }
            if cooldown > 0 && last_exit_bar[sym_i] >= 0 {
                let bars_since_exit = (bar_idx as isize) - last_exit_bar[sym_i];
                if bars_since_exit < cooldown as isize {
                    continue; // freshness filter: skip if exited within cooldown bars
                }
            }
            if let Some(d) = data.get(symbols[sym_i]) {
                if turtle_signal(&d.close, &d.high, TURTLE_ENTRY, bar) {
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
    // Max DD from equity progression
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

    WfResult { ret, sharpe, trades, max_dd: max_dd_pct, pass: trades >= MIN_TRADES && sharpe > 0.0 }
}

#[tokio::main]
async fn main() -> Result<()> {
    let start = Instant::now();
    let loader = DataLoader::new(None, None);
    let mut data: HashMap<String, SymData> = HashMap::new();

    // Collect all unique symbols across universes
    let mut all_syms = std::collections::HashSet::new();
    for (_, syms) in UNIVERSES {
        for s in *syms {
            all_syms.insert(s.to_string());
        }
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
    println!("Running {} windows for {} universes", n_windows, UNIVERSES.len());

    let mut csv = File::create(CSV_OUT)?;
    writeln!(csv, "universe,cooldown,window,ret,sharpe,trades,max_dd,pass")?;

    let mut md = File::create(MD_OUT)?;
    writeln!(md, "# Turtle Freshness Filter Walk-Forward\n")?;
    writeln!(md, "| Universe | Cooldown | Pass/Total | Avg Ret% | Avg Sharpe | Trades |")?;
    writeln!(md, "|----------|----------|------------|----------|-------------|--------|")?;

    let mut overall_by_cooldown: HashMap<usize, (usize, usize, f64, f64, f64)> = HashMap::new();

    for (uname, symbols) in UNIVERSES {
        for &cooldown in COOLDOWNS {
            let mut total_pass = 0;
            let mut total_ret = 0.0;
            let mut total_sharpe = 0.0;
            let mut total_trades = 0_usize;
            let mut n = 0;

            for w in 0..n_windows {
                let train_end = TRAIN_BARS + w * TEST_BARS;
                let test_end = (train_end + TEST_BARS).min(min_len);
                if test_end - train_end < 30 { continue; }

                let result = run_window(&data, symbols, train_end, test_end, cooldown);
                total_pass += result.pass as usize;
                total_ret += result.ret;
                total_sharpe += result.sharpe;
                total_trades += result.trades;
                n += 1;
                writeln!(csv, "{},{},{},{:.2},{:.3},{},{:.2},{}",
                    uname, cooldown, w, result.ret, result.sharpe, result.trades, result.max_dd, result.pass)?;
            }

            if n == 0 { continue; }
            let avg_ret = total_ret / n as f64;
            let avg_sharpe = total_sharpe / n as f64;
            let pass_rate = total_pass as f64 / n as f64 * 100.0;

            writeln!(md, "| {} | {} | {}/{} ({:.0}%) | {:.1}% | {:.3} | {} |",
                uname, cooldown, total_pass, n, pass_rate, avg_ret, avg_sharpe, total_trades)?;

            overall_by_cooldown.entry(cooldown).or_insert_with(|| (0, 0, 0.0, 0.0, 0.0));
            if let Some(e) = overall_by_cooldown.get_mut(&cooldown) {
                e.0 += total_pass;
                e.1 += n;
                e.2 += avg_ret;
                e.3 += avg_sharpe;
                e.4 += total_trades as f64;
            }
        }
    }

    // Summary by cooldown
    writeln!(md, "\n## Summary by Cooldown\n")?;
    writeln!(md, "| Cooldown | Pass Rate | Avg Sharpe | Avg Return | Total Trades | Verdict |")?;
    writeln!(md, "|----------|-----------|------------|------------|--------------|---------|")?;

    let mut cooldown_results: Vec<_> = overall_by_cooldown.iter().collect();
    cooldown_results.sort_by_key(|(&c, _)| c);

    let baseline_sharpe = cooldown_results.iter().find(|(&c, _)| c == 0)
        .map(|(_, &(pass, total, _, sharpe_sum, _))| {
            if total > 0 { sharpe_sum / total as f64 } else { 0.0 }
        }).unwrap_or(0.0);

    for (&cooldown, &(pass, total, ret_sum, sharpe_sum, trades)) in cooldown_results {
        let n_universe = UNIVERSES.len();
        let avg_sharpe = sharpe_sum / n_universe as f64;
        let avg_ret = ret_sum / n_universe as f64;
        let pass_rate = pass as f64 / total as f64 * 100.0;
        let verdict = if cooldown == 0 {
            "BASELINE".to_string()
        } else if avg_sharpe >= baseline_sharpe * 0.98 {
            "✅ KEEPS".to_string()
        } else {
            "🪦 REJECT".to_string()
        };
        writeln!(md, "| {} | {:.1}% | {:.3} | {:.1}% | {:.0} | {} |",
            cooldown, pass_rate, avg_sharpe, avg_ret, trades, verdict)?;
    }

    println!("\nResults written to {} and {}", CSV_OUT, MD_OUT);
    println!("Total runtime: {:.1}s", start.elapsed().as_secs_f64());

    Ok(())
}
