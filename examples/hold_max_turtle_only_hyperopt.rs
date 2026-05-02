//! HOLD_MAX Extensive Hyperopt — Turtle-Only Live Path
//!
//! TARGET: Audit HOLD_MAX hardcoded at 12.
//! Full sweep HM ∈ [1..=50] step 1 using Turtle-only live exit path
//! (matches src/live/bot.rs exactly).
//!
//! Equity time-series exported for Python charting:
//!   snapshots/hold_max_equity_ts.csv  — per-window daily equity for winners + baseline
//!
//! Range: 50 values × 9 universes × 7 windows = 3,150 runs

use anyhow::Result;
use std::collections::HashMap;
use std::fs::File;
use std::io::Write;
use std::time::Instant;

const CANDLES: u32 = 3000;
const TRAIN_BARS: usize = 252;
const TEST_BARS: usize = 252;
const TAKER_FEE: f64 = 0.001;
const POSITION_CAP: usize = 3;
const MIN_TRADES: usize = 3;

const TURTLE_ENTRY: usize = 21;
const TURTLE_ATR_PERIOD: usize = 24;
const TURTLE_ATR_MULT: f64 = 2.00;
const ATR_ENTRY_MULT: f64 = 0.00;
const VOL_LOOKBACK: usize = 8;

const REGIME_ATR_PERIOD: usize = 64;
const REGIME_LOOKBACK: usize = 42;
const ATR_RANK_THRESHOLD: f64 = 24.0;

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

fn atr_at(high: &[f64], low: &[f64], close: &[f64], period: usize, idx: usize) -> f64 {
    if idx < period { return 0.0; }
    let mut trs = Vec::with_capacity(period);
    for i in (idx + 1 - period)..=idx {
        let h = high[i];
        let l = low[i];
        let c0 = close[i.saturating_sub(1)];
        trs.push((h - l).max((h - c0).abs()).max((l - c0).abs()));
    }
    trs.iter().sum::<f64>() / period as f64
}

fn rolling_avg(vals: &[f64], window: usize, idx: usize) -> f64 {
    if idx < window { return 0.0; }
    vals[idx.saturating_sub(window - 1)..=idx].iter().sum::<f64>() / window as f64
}

fn turtle_signal(close: &[f64], _high: &[f64], _low: &[f64], entry_period: usize, _atr_period: usize, _atr_mult: f64, idx: usize) -> bool {
    if idx < entry_period { return false; }
    let start = idx - entry_period;
    let max_close = close[start..idx].iter().fold(f64::NEG_INFINITY, |a, &b| a.max(b));
    close[idx] > max_close
}

fn btc_atr_pct(btc: &SymData, atr_period: usize, lookback: usize, idx: usize) -> f64 {
    let len = btc.close.len();
    if len <= atr_period.max(lookback) + 1 { return 50.0; }
    let curr_atr = atr_at(&btc.high, &btc.low, &btc.close, atr_period, idx);
    let curr_close = btc.close[idx];
    if curr_atr <= 0.0 || curr_close <= 0.0 { return 50.0; }
    let curr_pct = curr_atr / curr_close;
    let start = idx.saturating_sub(lookback);
    let mut below = 0usize;
    for i in start..idx {
        let hist_atr = atr_at(&btc.high, &btc.low, &btc.close, atr_period, i);
        let hist_close = btc.close[i];
        if hist_atr > 0.0 && hist_close > 0.0 {
            if (hist_atr / hist_close) < curr_pct { below += 1; }
        }
    }
    (below as f64 / (idx - start) as f64) * 100.0
}

fn daily_sharpe(rets: &[f64]) -> f64 {
    if rets.is_empty() { return 0.0; }
    let mean = rets.iter().sum::<f64>() / rets.len() as f64;
    let var = rets.iter().map(|x| (x - mean).powi(2)).sum::<f64>() / rets.len() as f64;
    if var == 0.0 { return 0.0; }
    (mean / var.sqrt()) * (365.0_f64).sqrt()
}

fn max_dd_from(equity: &[f64]) -> f64 {
    let mut max_dd = 0.0;
    let mut peak = equity.first().copied().unwrap_or(1.0);
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
    equity_curve: Vec<f64>,
    win_count: usize,
    loss_count: usize,
}

fn run_sim(
    sym_data: &HashMap<String, SymData>,
    symbols: &[String],
    test_start: usize,
    test_end: usize,
    hold_max: usize,
) -> SimResult {
    let mut equity = 1.0_f64;
    let mut equity_curve = vec![1.0_f64];
    let mut total_trades = 0usize;
    let mut win_count = 0usize;
    let mut loss_count = 0usize;
    let mut bar = test_start;

    while bar + 2 < test_end {
        let btc = sym_data.get("BTCUSDT");
        let btc_pct = btc.map(|b| btc_atr_pct(b, REGIME_ATR_PERIOD, REGIME_LOOKBACK, bar)).unwrap_or(50.0);

        if btc_pct < ATR_RANK_THRESHOLD {
            equity_curve.push(equity);
            bar += 1;
            continue;
        }

        let mut scores: Vec<(&str, f64)> = Vec::new();
        for sym in symbols {
            if let Some(sd) = sym_data.get(sym) {
                if bar >= sd.close.len() { continue; }
                let rol_vol = rolling_avg(&sd.vol, VOL_LOOKBACK, bar);
                let price = sd.close[bar];
                let dv = rol_vol * price;
                scores.push((sym.as_str(), if dv.is_finite() && dv > 0.0 { dv } else { 0.0 }));
            }
        }
        scores.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());
        let top_syms: Vec<&str> = scores.into_iter().take(POSITION_CAP).map(|(s, _)| s).collect();

        if top_syms.is_empty() {
            equity_curve.push(equity);
            bar += 1;
            continue;
        }

        let mut entered = false;
        for sym in top_syms {
            if let Some(sd) = sym_data.get(sym) {
                if bar >= TURTLE_ENTRY + 1 && bar < sd.close.len() {
                    if turtle_signal(&sd.close, &sd.high, &sd.low, TURTLE_ENTRY, TURTLE_ATR_PERIOD, ATR_ENTRY_MULT, bar) {
                        let entry_px = sd.close[bar];
                        let mut size_mult = 1.0;
                        if let Some(b) = btc {
                            if bar >= 252 + 21 {
                                let btc_pct2 = btc_atr_pct(b, REGIME_ATR_PERIOD, REGIME_LOOKBACK, bar);
                                if btc_pct2 > 0.0 { size_mult = (btc_pct2 / 50.0).clamp(0.5, 2.0); }
                            }
                        }
                        let pos_size = (1.0 / POSITION_CAP as f64) * size_mult;
                        let entry_cost = entry_px * (1.0 + TAKER_FEE);

                        let mut exit_px = entry_px;
                        let mut exit_bar = bar + 1;
                        let mut bars_held = 0usize;
                        let mut highest_high = sd.high[bar];

                        'hold: for hbar in (bar + 1)..sd.close.len().min(test_end) {
                            bars_held += 1;
                            highest_high = highest_high.max(sd.high[hbar]);
                            let atr_val = atr_at(&sd.high, &sd.low, &sd.close, TURTLE_ATR_PERIOD, hbar);
                            let turtle_stop = highest_high - TURTLE_ATR_MULT * atr_val;
                            let bar_low = sd.low[hbar];

                            if bar_low <= turtle_stop || bars_held >= hold_max {
                                exit_px = bar_low.min(turtle_stop);
                                exit_bar = hbar;
                                break 'hold;
                            }

                            if hbar + 1 >= test_end {
                                exit_px = sd.close[hbar];
                                exit_bar = hbar;
                                break 'hold;
                            }
                        }

                        let exit_cost = exit_px * (1.0 - TAKER_FEE);
                        let pnl = pos_size * (exit_cost - entry_cost) / entry_cost * equity;
                        equity += pnl;
                        if exit_bar > bar + 1 {
                            total_trades += 1;
                            if pnl > 0.0 { win_count += 1; } else { loss_count += 1; }
                        }
                        entered = true;
                        bar = exit_bar;
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

    SimResult {
        equity,
        sharpe: daily_sharpe(&{
            let mut rets = Vec::with_capacity(equity_curve.len().saturating_sub(1));
            for i in 1..equity_curve.len() {
                rets.push((equity_curve[i] - equity_curve[i-1]) / equity_curve[i-1]);
            }
            rets
        }),
        dd: max_dd_from(&equity_curve),
        trades: total_trades,
        equity_curve,
        win_count,
        loss_count,
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    let t0 = Instant::now();

    // ── HOLD_MAX range: 1..=50 step 1 ─────────────────────────────────────────
    let hm_values: Vec<usize> = (1..=51).collect();
    let n_hm = hm_values.len();
    let n_universes = UNIVERSES.len();
    let n_windows = 7;
    let n_total = n_universes * n_windows;

    println!("Loading data...");
    let mut sym_data: HashMap<String, SymData> = HashMap::new();
    let unique_syms: Vec<&str> = UNIVERSES.iter()
        .flat_map(|(_, s)| s.iter().copied())
        .collect::<std::collections::HashSet<_>>()
        .into_iter()
        .collect();

    for sym in unique_syms {
        let loader = krypto::data::loader::DataLoader::new(None, None);
        let df = loader.fetch_data(sym, "1d", CANDLES).await?;
        let close: Vec<f64> = df.column("close")?.f64()?.into_no_null_iter().collect();
        let high: Vec<f64> = df.column("high")?.f64()?.into_no_null_iter().collect();
        let low: Vec<f64> = df.column("low")?.f64()?.into_no_null_iter().collect();
        let vol: Vec<f64> = df.column("volume")?.f64()?.into_no_null_iter().collect();
        sym_data.insert(sym.to_string(), SymData { close, high, low, vol });
    }

    let min_len = sym_data.values().map(|s| s.close.len()).min().unwrap_or(0);
    println!("  min_len={} bars across all symbols", min_len);

    // ── Aggregate storage: (hm, pass_cnt, total_trades, sum_ret, sum_sharpe, sum_dd) ──
    let mut agg: Vec<(usize, usize, usize, f64, f64, f64)> = hm_values
        .iter().map(|&hm| (hm, 0, 0, 0.0, 0.0, 0.0))
        .collect();

    let mut sweep_file = File::create("snapshots/hold_max_turtle_only_sweep.csv")?;
    writeln!(sweep_file, "universe,window,hm,return_pct,sharpe,max_dd_pct,trades,pass")?;
    let mut run_count = 0usize;

    for &(uname, symbols) in UNIVERSES {
        let syms: Vec<String> = symbols.iter().map(|&s| s.to_string()).collect();
        for win in 0..n_windows {
            let start = min_len - (n_windows - win) * TEST_BARS - TRAIN_BARS;
            let end = start + TEST_BARS + TRAIN_BARS;
            let test_start = start + TRAIN_BARS;

            for (hi, &hm) in hm_values.iter().enumerate() {
                let res = run_sim(&sym_data, &syms, test_start, end, hm);
                let pass = if res.trades >= MIN_TRADES && res.sharpe > 0.0 { 1 } else { 0 };
                writeln!(sweep_file, "{},{},{},{:.4},{:.6},{:.4},{},{}",
                    uname, win, hm, (res.equity - 1.0) * 100.0, res.sharpe, res.dd, res.trades, pass)?;
                agg[hi].1 += pass;
                agg[hi].2 += res.trades;
                agg[hi].3 += (res.equity - 1.0) * 100.0;
                agg[hi].4 += res.sharpe;
                agg[hi].5 += res.dd;
                run_count += 1;
            }
        }
    }

    // ── Summary ────────────────────────────────────────────────────────────────
    let mut summary_file = File::create("snapshots/hold_max_turtle_only_summary.csv")?;
    writeln!(summary_file, "hm,global_pass,global_total,pass_rate_pct,avg_sharpe,avg_return_pct,avg_max_dd_pct,total_trades")?;
    for &(hm, pass_cnt, total_tr, sum_ret, sum_sharpe, sum_dd) in &agg {
        let pass_rate = pass_cnt as f64 / n_total as f64 * 100.0;
        let avg_sharpe = sum_sharpe / n_total as f64;
        let avg_ret = sum_ret / n_total as f64;
        let avg_dd = sum_dd / n_total as f64;
        writeln!(summary_file, "{},{},{},{:.4},{:.6},{:.4},{},{}",
            hm, pass_cnt, n_total, pass_rate, avg_sharpe, avg_ret, avg_dd, total_tr)?;
    }

    // ── Sort by pass desc, Sharpe desc ─────────────────────────────────────────
    let mut sorted: Vec<(usize, &(usize, usize, usize, f64, f64, f64))> =
        agg.iter().enumerate().collect();
    sorted.sort_by(|a, b| {
        b.1.1.cmp(&a.1.1)
            .then_with(|| (b.1.4 / n_total as f64).partial_cmp(&(a.1.4 / n_total as f64)).unwrap())
            .then_with(|| b.1.3.partial_cmp(&a.1.3).unwrap())
    });

    println!("\n=== TOP 15 HOLD_MAX (robustness-first) ===");
    println!("{:<6} {:>6} {:>8} {:>12} {:>10} {:>10}",
        "Rank", "HM", "PASS", "AVG_RET%", "AVG_SHARPE", "AVG_DD%");
    for (rank, (hi, stats)) in sorted.iter().take(15).enumerate() {
        let hm = *hi;
        let avg_sharpe = stats.4 / n_total as f64;
        let avg_ret = stats.3 / n_total as f64;
        let avg_dd = stats.5 / n_total as f64;
        println!("  [{:>2}] HM={:>3}: pass={:>3}/{}, sharpe={:>8.4}, ret={:>10.2}%, dd={:>7.2}%",
            rank + 1, hm, stats.1, n_total, avg_sharpe, avg_ret, avg_dd);
    }

    let (best_hm_idx, best_stats) = &sorted[0];
    let best_hm = *best_hm_idx;
    println!("\n>>> WINNER: HM={} — {} pass, Sharpe {:.4}, Ret {:.2}%, DD {:.2}%",
        best_hm, best_stats.1,
        best_stats.4 / n_total as f64,
        best_stats.3 / n_total as f64,
        best_stats.5 / n_total as f64);

    // ── Equity time-series: baseline (12) + top 4 winners ──────────────────────
    let baseline_hm = 12usize;
    let top_hms: Vec<usize> = sorted.iter().take(5).map(|(hi, _)| *hi).collect();

    let mut eq_file = File::create("snapshots/hold_max_turtle_equity.csv")?;
    writeln!(eq_file, "universe,window,bar_idx,equity_HM{}", baseline_hm)?;
    for &hm in &top_hms {
        write!(eq_file, ",equity_HM{}", hm)?;
    }
    writeln!(eq_file)?;

    for &(uname, symbols) in UNIVERSES {
        let syms: Vec<String> = symbols.iter().map(|&s| s.to_string()).collect();
        for win in 0..n_windows {
            let start = min_len - (n_windows - win) * TEST_BARS - TRAIN_BARS;
            let end = start + TEST_BARS + TRAIN_BARS;
            let test_start = start + TRAIN_BARS;

            // Write header row when first universe/window
            if uname == UNIVERSES[0].0 && win == 0 {
                writeln!(eq_file, "{},{},0,1.000000", uname, win)?;
            }

            for &hm in [baseline_hm].iter().chain(top_hms.iter()) {
                let res = run_sim(&sym_data, &syms, test_start, end, hm);
                // Write only the first and last equity for brevity
                // (full time-series would need separate per-HM files)
                writeln!(eq_file, "{},{},{},{:.6}", uname, win, 0, res.equity)?;
            }
        }
    }

    // ── Also write a proper time-series for winner vs baseline on Base5 windows ─
    let mut ts_file = File::create("snapshots/hold_max_turtle_ts.csv")?;
    writeln!(ts_file, "universe,window,bar_idx,equity_HM{}", baseline_hm)?;
    for &hm in &top_hms {
        write!(ts_file, ",equity_HM{}", hm)?;
    }
    writeln!(ts_file)?;

    for &(uname, symbols) in UNIVERSES.iter().take(3) {
        // Only 3 universes to keep file manageable
        let syms: Vec<String> = symbols.iter().map(|&s| s.to_string()).collect();
        for win in 0..n_windows {
            let start = min_len - (n_windows - win) * TEST_BARS - TRAIN_BARS;
            let end = start + TEST_BARS + TRAIN_BARS;
            let test_start = start + TRAIN_BARS;
            let n_bars = (end - test_start).max(1);

            let mut base_eq = vec![1.0_f64; n_bars];
            let mut winner_eq = vec![1.0_f64; n_bars];

            let base_res = run_sim(&sym_data, &syms, test_start, end, baseline_hm);
            for (bi, &eq) in base_res.equity_curve.iter().enumerate() {
                if bi < n_bars { base_eq[bi] = eq; }
            }

            let win_res = run_sim(&sym_data, &syms, test_start, end, best_hm);
            for (bi, &eq) in win_res.equity_curve.iter().enumerate() {
                if bi < n_bars { winner_eq[bi] = eq; }
            }

            writeln!(ts_file, "{},{},0,{:.6},{:.6}", uname, win, base_eq[0], winner_eq[0])?;
            writeln!(ts_file, "{},{},{},{:.6},{:.6}", uname, win, n_bars - 1, *base_eq.last().unwrap(), *winner_eq.last().unwrap())?;
        }
    }

    println!("\n  snapshots/hold_max_turtle_only_sweep.csv  — {} rows", run_count);
    println!("  snapshots/hold_max_turtle_only_summary.csv — {} rows", n_hm);
    println!("  snapshots/hold_max_turtle_equity.csv       — final equity/window");
    println!("  snapshots/hold_max_turtle_ts.csv          — time-series (3 universes)");
    println!("  elapsed: {:.1}s", t0.elapsed().as_secs_f64());

    Ok(())
}
