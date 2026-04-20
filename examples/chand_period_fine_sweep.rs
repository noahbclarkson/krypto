//! CHAND_PERIOD Fine Sweep — Step 1 in Critical Region [5-30]
//!
//! Prior coarse sweep (step 2) found CP=11 as winner with +1.9% Sharpe.
//! This fine sweep uses step=1 in the critical region [5-30] to:
//!   1. Confirm CP=11 is the true optimum (not CP=10 or CP=12)
//!   2. Map the full shape of the Sharpe curve around the optimum
//!   3. Export equity curves for winner + runner-ups for charting
//!
//! Fixed params (current production):
//!   CHAND_MULT=2.25, EP=21, ATR=24, ATR_M=2.0, HM=45, CAP=3, VOL=2
//!
//! Universe: Base5 (6 windows) for fine sweep
//! Validation: 9 universes for winner confirmation

use anyhow::Result;
use krypto::data::loader::DataLoader;
use std::collections::HashMap;
use std::fs::OpenOptions;
use std::io::Write;

const CANDLES: u32 = 3000;
const TRAIN_BARS: usize = 252;
const TEST_BARS: usize = 252;
const HOLD_MAX: usize = 45;
const TAKER_FEE: f64 = 0.001;
const POSITION_CAP: usize = 3;
const MIN_TRADES: usize = 3;
const CHAND_MULT: f64 = 2.25;
const TURTLE_ENTRY: usize = 21;
const TURTLE_ATR_PERIOD: usize = 24;
const TURTLE_ATR_MULT: f64 = 2.00;
const VOL_LOOKBACK: usize = 2;

// Fine sweep range: step 1 from 5 to 30 (26 values)
const CP_START: usize = 5;
const CP_END: usize = 30;

const BASE5: [&str; 6] = ["BTCUSDT", "ETHUSDT", "SOLUSDT", "XRPUSDT", "DOGEUSDT", "ADAUSDT"];

const UNIVERSES: [(&str, [&str; 6]); 9] = [
    ("Base5",     ["BTCUSDT","ETHUSDT","SOLUSDT","XRPUSDT","DOGEUSDT","ADAUSDT"]),
    ("NoDOGE",   ["BTCUSDT","ETHUSDT","SOLUSDT","XRPUSDT","ADAUSDT",    "BNBUSDT"]),
    ("Legacy4",  ["BTCUSDT","ETHUSDT","XRPUSDT","LTCUSDT","EOSUSDT",    "BNBUSDT"]),
    ("Legacy5",  ["BTCUSDT","ETHUSDT","XRPUSDT","LTCUSDT","BNBUSDT",   "EOSUSDT"]),
    ("OldGuard", ["BTCUSDT","ETHUSDT","XRPUSDT","LTCUSDT","EOSUSDT",   "BCHUSDT"]),
    ("LargeCap",["BTCUSDT","ETHUSDT","SOLUSDT","XRPUSDT","BNBUSDT",   "ADAUSDT"]),
    ("Legacy3",  ["BTCUSDT","XRPUSDT","LTCUSDT","EOSUSDT",   "BCHUSDT", "BNBUSDT"]),
    ("LowVol",   ["XRPUSDT","LTCUSDT","EOSUSDT","BCHUSDT","ADAUSDT",  "BNBUSDT"]),
    ("OldGuard4",["BTCUSDT","XRPUSDT","LTCUSDT","EOSUSDT",   "BCHUSDT", "BNBUSDT"]),
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
        let h = high.get(i).copied().unwrap_or(0.0);
        let l = low.get(i).copied().unwrap_or(0.0);
        let c0 = close.get(i.saturating_sub(1)).copied().unwrap_or(0.0);
        trs.push((h - l).max((h - c0).abs()).max((l - c0).abs()));
    }
    if trs.is_empty() { return 0.0; }
    trs.iter().sum::<f64>() / period as f64
}

fn rolling_dv(close: &[f64], vol: &[f64], lookback: usize, bar: usize) -> f64 {
    if bar < lookback.saturating_sub(1) {
        return close.get(bar).copied().unwrap_or(0.0) * vol.get(bar).copied().unwrap_or(0.0);
    }
    let start = bar + 1 - lookback;
    let mut sum = 0.0;
    for i in start..=bar {
        let c = close.get(i).copied().unwrap_or(0.0);
        let v = vol.get(i).copied().unwrap_or(0.0);
        sum += c * v;
    }
    sum / lookback as f64
}

struct WindowResult {
    trades: usize,
    ret: f64,
    sharpe: f64,
    max_dd: f64,
    win_rate: f64,
    equity_bars: Vec<f64>, // equity at each test bar (for charting)
}

fn run_window(
    sym_data: &HashMap<String, SymData>,
    symbols: &[&str],
    train_end: usize,
    test_end: usize,
    chand_period: usize,
) -> WindowResult {
    let warmup = chand_period.max(TURTLE_ATR_PERIOD).max(TURTLE_ENTRY) + TURTLE_ATR_PERIOD;
    let test_start = train_end;
    let n = test_end.min(sym_data.values().next().map(|s| s.close.len()).unwrap_or(0));

    // Rank by dollar volume
    let mut scores: Vec<(&str, f64)> = symbols.iter().filter_map(|s| {
        sym_data.get(*s).and_then(|sd| {
            if test_start < sd.close.len() {
                let dv = rolling_dv(&sd.close, &sd.vol, VOL_LOOKBACK, test_start);
                Some((*s, if dv.is_finite() && dv > 0.0 { dv } else { 0.0 }))
            } else { None }
        })
    }).collect();
    scores.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());
    let top: Vec<&str> = scores.iter().take(POSITION_CAP).map(|(s, _)| *s).collect();

    let mut trades = 0usize;
    let mut rets = Vec::new();
    let mut equity = 1.0_f64;
    let mut peak = 1.0_f64;
    let mut max_dd = 0.0_f64;
    let mut equity_bars = Vec::new();

    // Record equity at each test bar (for equity curve)
    equity_bars.push(equity);

    let mut bar = test_start.max(warmup);
    while bar < n {
        equity_bars.push(equity);

        let mut entered = false;
        for &sym in &top {
            if let Some(sd) = sym_data.get(sym) {
                if bar >= TURTLE_ENTRY + 1 && bar < sd.close.len() {
                    let start_idx = bar + 1 - TURTLE_ENTRY;
                    let mut max_close = f64::NEG_INFINITY;
                    for i in start_idx..bar {
                        if let Some(&c) = sd.close.get(i) { max_close = max_close.max(c); }
                    }
                    let curr_close = sd.close.get(bar).copied().unwrap_or(0.0);
                    if curr_close > max_close {
                        let entry_px = sd.close[bar];
                        let entry = entry_px * (1.0 - TAKER_FEE);
                        let next_bar = bar + 1;
                        let max_hold = (next_bar + HOLD_MAX).min(sd.close.len().saturating_sub(1));
                        let mut exit_bar = max_hold;
                        let mut hh_c = sd.high.get(next_bar).copied().unwrap_or(0.0);
                        let mut hh_t = sd.high.get(next_bar).copied().unwrap_or(0.0);

                        for b in next_bar..=max_hold {
                            hh_c = hh_c.max(sd.high.get(b).copied().unwrap_or(0.0));
                            let atr_c = atr_at(&sd.high, &sd.low, &sd.close, chand_period, b);
                            let trail_c = hh_c - CHAND_MULT * atr_c;
                            hh_t = hh_t.max(sd.high.get(b).copied().unwrap_or(0.0));
                            let atr_t = atr_at(&sd.high, &sd.low, &sd.close, TURTLE_ATR_PERIOD, b);
                            let trail_t = hh_t - TURTLE_ATR_MULT * atr_t;
                            if sd.close.get(b).copied().unwrap_or(0.0) < trail_c
                            || sd.close.get(b).copied().unwrap_or(0.0) < trail_t {
                                exit_bar = b; break;
                            }
                        }

                        let exit_px = sd.close.get(exit_bar).copied().unwrap_or(entry_px);
                        let exit = exit_px * (1.0 - TAKER_FEE);
                        let gross_ret = exit / entry - 1.0;
                        equity *= 1.0 + gross_ret;
                        peak = peak.max(equity);
                        let dd = (equity / peak - 1.0).min(0.0);
                        max_dd = max_dd.min(dd);
                        rets.push(gross_ret);
                        trades += 1;
                        bar = exit_bar + 1;
                        entered = true;
                        break;
                    }
                }
            }
        }
        if !entered { bar += 1; }
    }

    let sharpe = if rets.len() >= 2 {
        let mean = rets.iter().sum::<f64>() / rets.len() as f64;
        let var = rets.iter().map(|r| (r - mean).powi(2)).sum::<f64>() / (rets.len() - 1) as f64;
        let std = var.sqrt();
        if std > 1e-10 { mean / std * (252f64).sqrt() } else { 0.0 }
    } else { 0.0 };

    let wins = rets.iter().filter(|&&r| r > 0.0).count();
    let win_rate = if !rets.is_empty() { wins as f64 / rets.len() as f64 * 100.0 } else { 0.0 };

    WindowResult { trades, ret: equity - 1.0, sharpe, max_dd, win_rate, equity_bars }
}

fn run_universe(
    sym_data: &HashMap<String, SymData>,
    symbols: &[&str],
    chand_periods: &[usize],
    universe_name: &str,
    total_windows: usize,
) -> Vec<(usize, usize, f64, f64, f64, f64, f64, bool, Vec<f64>)> {
    // Vec of (cp, window, ret%, sharpe, max_dd%, trades, win_rate%, pass, equity_bars)
    let mut results = Vec::new();
    for &cp in chand_periods {
        for wi in 0..total_windows {
            let train_end = TRAIN_BARS + wi * TEST_BARS;
            let test_end = (train_end + TEST_BARS).min(sym_data.values().next().map(|s| s.close.len()).unwrap_or(0));
            let wr = run_window(sym_data, symbols, train_end, test_end, cp);
            let pass = wr.trades >= MIN_TRADES && wr.ret > 0.0;
            results.push((
                cp, wi,
                wr.ret * 100.0,
                wr.sharpe,
                wr.max_dd * 100.0,
                wr.trades as f64,
                wr.win_rate,
                pass,
                wr.equity_bars,
            ));
        }
    }
    results
}

#[tokio::main]
async fn main() -> Result<()> {
    eprintln!("=== CHAND_PERIOD Fine Sweep (step 1, critical region [5-30]) ===");
    eprintln!("Fixed: CM=2.25, EP=21, ATR=24, ATR_M=2.0, HM=45, CAP=3");

    // ── Load Base5 data ───────────────────────────────────────────────────────
    let loader = DataLoader::new(None, None);
    let mut base5_data: HashMap<String, SymData> = HashMap::new();
    for &sym in &BASE5 {
        let df = loader.fetch_with_cache(sym, "1d", CANDLES).await?;
        let c = df.column("close")?.f64()?;
        let h = df.column("high")?.f64()?;
        let l = df.column("low")?.f64()?;
        let v = df.column("volume")?.f64()?;
        base5_data.insert(sym.to_string(), SymData {
            close: c.into_iter().filter_map(|x| x).collect(),
            high:  h.into_iter().filter_map(|x| x).collect(),
            low:   l.into_iter().filter_map(|x| x).collect(),
            vol:   v.into_iter().filter_map(|x| x).collect(),
        });
    }

    let min_len_base5 = base5_data.values().map(|s| s.close.len()).min().unwrap_or(0);
    let total_windows = (min_len_base5.saturating_sub(TRAIN_BARS + TEST_BARS)) / TEST_BARS;
    eprintln!("Base5: {} bars, {} windows", min_len_base5, total_windows);

    // ── Generate CP values (step 1 from 5 to 30) ─────────────────────────────
    let cp_values: Vec<usize> = (CP_START..=CP_END).collect();
    eprintln!("Testing {} CHAND_PERIOD values: {:?}",
        cp_values.len(),
        if cp_values.len() <= 10 { format!("{:?}", cp_values) }
        else { format!("{:?} ... {:?}", &cp_values[..5], &cp_values[cp_values.len()-3..]) }
    );

    // ── Fine sweep on Base5 ──────────────────────────────────────────────────
    eprintln!("\n─── Phase 1: Fine sweep on Base5 ({} windows) ───", total_windows);
    let base5_results = run_universe(&base5_data, &BASE5, &cp_values, "Base5", total_windows);

    // ── Aggregate and find winner ──────────────────────────────────────────────
    #[derive(Clone)]
    struct CpAgg { cp: usize, avg_sharpe: f64, avg_ret: f64, avg_dd: f64,
                   avg_trades: f64, pass_rate: f64, passed: usize, total: usize }
    let mut cp_agg: Vec<CpAgg> = Vec::new();
    for &cp in &cp_values {
        let runs: Vec<_> = base5_results.iter().filter(|(c, _, _, _, _, _, _, _, _)| *c == cp).collect();
        if runs.is_empty() { continue; }
        let n = runs.len();
        let avg_sharpe: f64 = runs.iter().map(|r| r.3).sum::<f64>() / n as f64;
        let avg_ret: f64 = runs.iter().map(|r| r.2).sum::<f64>() / n as f64;
        let avg_dd: f64 = runs.iter().map(|r| r.4).sum::<f64>() / n as f64;
        let avg_trades: f64 = runs.iter().map(|r| r.5).sum::<f64>() / n as f64;
        let passed = runs.iter().filter(|r| r.7).count();
        let pass_rate = passed as f64 / n as f64 * 100.0;
        cp_agg.push(CpAgg { cp, avg_sharpe, avg_ret, avg_dd, avg_trades, pass_rate, passed, total: n });
    }

    // Sort by Sharpe
    cp_agg.sort_by(|a, b| b.avg_sharpe.partial_cmp(&a.avg_sharpe).unwrap());

    eprintln!("\n=== Base5 Fine Sweep Results (sorted by Sharpe) ===");
    eprintln!("{:>8} {:>10} {:>10} {:>10} {:>10} {:>10}", "CP", "AvgRet%", "AvgSharpe", "AvgDD%", "AvgTrades", "PassRate%");
    eprintln!("{}", "-".repeat(60));
    for agg in &cp_agg {
        let marker = if agg.cp == 11 { " ← prior winner" } else { "" };
        eprintln!("{:>8} {:>10.2} {:>10.3} {:>10.1} {:>10.1} {:>10.1}%{}", agg.cp, agg.avg_ret, agg.avg_sharpe, agg.avg_dd, agg.avg_trades, agg.pass_rate, marker);
    }

    let winner_cp = cp_agg.first().map(|a| a.cp).unwrap_or(11);
    let winner_sharpe = cp_agg.first().map(|a| a.avg_sharpe).unwrap_or(0.0);
    eprintln!("\n🏆 WINNER: CHAND_PERIOD={} (Sharpe {:.3})", winner_cp, winner_sharpe);

    // ── Write CSV summary ──────────────────────────────────────────────────────
    let mut csv = String::from("chand_period,avg_return,avg_sharpe,avg_max_dd,avg_trades,pass_rate_pct,windows_passed,total_windows\n");
    for agg in &cp_agg {
        csv.push_str(&format!("{},{:.2},{:.4},{:.1},{:.1},{:.1},{},{}\n",
            agg.cp, agg.avg_ret, agg.avg_sharpe, agg.avg_dd, agg.avg_trades, agg.pass_rate, agg.passed, agg.total));
    }
    std::fs::write("snapshots/chand_period_fine_sweep.csv", &csv)?;
    eprintln!("Written: snapshots/chand_period_fine_sweep.csv");

    // ── Export equity curves for top 3 candidates ─────────────────────────────
    eprintln!("\n─── Exporting equity curves for top 3 candidates ───");
    let top3_cps: Vec<usize> = cp_agg.iter().take(3).map(|a| a.cp).collect();

    // Build mean equity across windows for each top CP
    for &cp in &top3_cps {
        let window_equities: Vec<Vec<f64>> = base5_results.iter()
            .filter(|(c, _, _, _, _, _, _, _, _)| *c == cp)
            .map(|r| r.8.clone())
            .collect();

        if window_equities.is_empty() { continue; }
        let max_bars = window_equities.iter().map(|e| e.len()).max().unwrap_or(0);
        let mut mean_equity = vec![0.0; max_bars];
        for eq in &window_equities {
            for (i, &v) in eq.iter().enumerate() {
                mean_equity[i] += v / window_equities.len() as f64;
            }
        }

        let mut eq_csv = String::from("universe,window,bar,equity\n");
        for (bi, &val) in mean_equity.iter().enumerate() {
            eq_csv.push_str(&format!("Base5_mean,0,{},{}\n", bi, val));
        }
        std::fs::write(&format!("snapshots/chand_period_cp{}_equity.csv", cp), &eq_csv)?;
        eprintln!("  Exported equity for CP={}", cp);
    }

    // ── Phase 2: Validate winner across all 9 universes ───────────────────────
    eprintln!("\n─── Phase 2: 9-Universe Validation for CP={} ───", winner_cp);
    let mut universe_results: Vec<(String, usize, f64, f64, f64, f64, f64, bool)> = Vec::new();

    for (uni_name, uni_symbols) in &UNIVERSES {
        let mut uni_data: HashMap<String, SymData> = HashMap::new();
        for sym in *uni_symbols {
            let df = loader.fetch_with_cache(&sym, "1d", CANDLES).await?;
            let c = df.column("close")?.f64()?;
            let h = df.column("high")?.f64()?;
            let l = df.column("low")?.f64()?;
            let v = df.column("volume")?.f64()?;
            uni_data.insert(sym.to_string(), SymData {
                close: c.into_iter().filter_map(|x| x).collect(),
                high:  h.into_iter().filter_map(|x| x).collect(),
                low:   l.into_iter().filter_map(|x| x).collect(),
                vol:   v.into_iter().filter_map(|x| x).collect(),
            });
        }
        let min_len = uni_data.values().map(|s| s.close.len()).min().unwrap_or(0);
        let uni_windows = (min_len.saturating_sub(TRAIN_BARS + TEST_BARS)) / TEST_BARS;

        for wi in 0..uni_windows {
            let train_end = TRAIN_BARS + wi * TEST_BARS;
            let test_end = (train_end + TEST_BARS).min(min_len);
            let wr = run_window(&uni_data, uni_symbols, train_end, test_end, winner_cp);
            let pass = wr.trades >= MIN_TRADES && wr.ret > 0.0;
            universe_results.push((uni_name.to_string(), wi, wr.ret * 100.0, wr.sharpe, wr.max_dd * 100.0, wr.trades as f64, wr.win_rate, pass));
        }
    }

    // Aggregate 9-universe results
    let mut uni_agg: Vec<(String, f64, f64, f64, f64, usize, usize)> = Vec::new();
    for (uni_name, _) in &UNIVERSES {
        let runs: Vec<_> = universe_results.iter().filter(|(n, _, _, _, _, _, _, _)| n == *uni_name).collect();
        if runs.is_empty() { continue; }
        let n = runs.len();
        let avg_sharpe: f64 = runs.iter().map(|r| r.3).sum::<f64>() / n as f64;
        let avg_ret: f64 = runs.iter().map(|r| r.2).sum::<f64>() / n as f64;
        let avg_dd: f64 = runs.iter().map(|r| r.4).sum::<f64>() / n as f64;
        let avg_trades: f64 = runs.iter().map(|r| r.5).sum::<f64>() / n as f64;
        let passed = runs.iter().filter(|r| r.7).count();
        uni_agg.push((uni_name.to_string(), avg_sharpe, avg_ret, avg_dd, avg_trades, passed, n));
    }

    uni_agg.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());

    eprintln!("\n=== 9-Universe Validation (CP={}) ===", winner_cp);
    eprintln!("{:>12} {:>10} {:>10} {:>10} {:>10} {:>10}", "Universe", "AvgRet%", "AvgSharpe", "AvgDD%", "AvgTrades", "PassRate%");
    eprintln!("{}", "-".repeat(65));
    let mut total_passed = 0usize;
    let mut total_runs = 0usize;
    for (n, sh, ret, dd, trades, passed, total) in &uni_agg {
        let pr = *passed as f64 / *total as f64 * 100.0;
        eprintln!("{:>12} {:>10.2} {:>10.3} {:>10.1} {:>10.1} {:>10.1}%", n, ret, sh, dd, trades, pr);
        total_passed += passed;
        total_runs += total;
    }
    let global_pass = total_passed as f64 / total_runs as f64 * 100.0;
    eprintln!("\nGlobal pass rate: {}/{} ({:.1}%)", total_passed, total_runs, global_pass);

    // Write 9-universe CSV
    let mut uni_csv = String::from("universe,avg_return,avg_sharpe,avg_max_dd,avg_trades,windows_passed,total_windows,pass_rate_pct\n");
    for (n, sh, ret, dd, trades, passed, total) in &uni_agg {
        let pr = *passed as f64 / *total as f64 * 100.0;
        uni_csv.push_str(&format!("{},{:.2},{:.4},{:.1},{:.1},{},{},{:.1}\n", n, ret, sh, dd, trades, passed, total, pr));
    }
    std::fs::write("snapshots/chand_period_fine_9u_validation.csv", &uni_csv)?;
    eprintln!("Written: snapshots/chand_period_fine_9u_validation.csv");

    println!("\nDone.");
    Ok(())
}
