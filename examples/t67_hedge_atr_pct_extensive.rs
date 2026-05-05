//! T67: HEDGE_ATR_PCT Extensive Hyperopt (0-100 step 1)
//!
//! Mission: Find the optimal BTC-ATR-based hedge trigger threshold.
//! The USDT hedge overlay reduces position size when BTC volatility is elevated.
//! HEDGE_ATR_PCT is the percentile of 252-bar history that the 21-bar ATR must exceed.
//!
//! Prior: T66 sweep HEDGE_PCT∈[0..=100] step 1 × 9 universes × 7 WF windows
//! found PCT=45 as robustness winner (58/63 pass, Sharpe 7.079, DD 21.2%).
//! But this used coarse parameter selection. Test EXTENSIVE range
//! to find if there's a better peak or plateau.
//!
//! Output:
//!   snapshots/t67_hedge_atr_pct_sweep.csv       — all metrics per PCT value
//!   snapshots/t67_chart_equity.csv             — window equity for chart configs
//!   charts/t67_comparison_chart.png             — Baseline vs Winner vs Runner-ups

use anyhow::Result;
use krypto::data::loader::DataLoader;
use std::collections::HashMap;
use std::fs::File;
use std::io::Write;

const CANDLES: u32 = 3000;
const TRAIN_BARS: usize = 252;
const TEST_BARS: usize = 252;
const HOLD_MAX: usize = 12;
const TAKER_FEE: f64 = 0.001;
const POSITION_CAP: usize = 3;
const MIN_TRADES: usize = 3;

const TURTLE_ENTRY: usize = 21;
const TURTLE_ATR_PERIOD: usize = 24;
const TURTLE_ATR_MULT: f64 = 2.00;
const ATR_ENTRY_MULT: f64 = 0.00;
const VOL_LOOKBACK: usize = 92;

const REGIME_ATR_PERIOD: usize = 17;
const REGIME_LOOKBACK: usize = 41;
const ATR_RANK_T: f64 = 5.0;
const HEDGE_SIZE_MULT: f64 = 0.40; // T66 winner — not being tuned here

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

fn turtle_signal(close: &[f64], high: &[f64], low: &[f64], entry_period: usize, atr_period: usize, atr_mult: f64, idx: usize) -> bool {
    if idx < entry_period { return false; }
    let start = idx - entry_period;
    let max_close = close[start..idx].iter().fold(f64::NEG_INFINITY, |a, &b| a.max(b));
    if close[idx] > max_close {
        if atr_mult > 0.0 {
            let atr = atr_at(high, low, close, atr_period, idx);
            if close[idx] < max_close + atr * atr_mult {
                return false;
            }
        }
        return true;
    }
    false
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

struct WfResult {
    equity: f64,
    sharpe: f64,
    dd: f64,
    trades: usize,
}

fn run_sim(
    sym_data: &HashMap<String, SymData>,
    symbols: &[String],
    test_start: usize,
    test_end: usize,
    hedge_atr_pct: f64,
) -> WfResult {
    let mut equity = 1.0_f64;
    let mut peak = 1.0_f64;
    let mut total_trades = 0usize;
    let mut daily_rets = Vec::new();

    let mut bar = test_start;
    while bar + 2 < test_end {
        let btc = sym_data.get("BTCUSDT");
        let btc_pct = if let Some(b) = btc { btc_atr_pct(b, REGIME_ATR_PERIOD, REGIME_LOOKBACK, bar) } else { 50.0 };
        
        if btc_pct < ATR_RANK_T {
            bar += 1;
            continue;
        }

        let mut scores: Vec<(&str, f64)> = Vec::new();
        for sym in symbols {
            if let Some(sd) = sym_data.get(sym) {
                if bar >= sd.close.len() { continue; }
                let rol_vol = rolling_avg(&sd.vol, VOL_LOOKBACK, bar);
                let price = sd.close.get(bar).copied().unwrap_or(0.0);
                let dv = rol_vol * price;
                scores.push((sym.as_str(), if dv.is_finite() && dv > 0.0 { dv } else { 0.0 }));
            }
        }
        scores.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());
        let top_syms: Vec<String> = scores.into_iter().take(POSITION_CAP).map(|(s, _)| s.to_string()).collect();

        if top_syms.is_empty() {
            bar += 1;
            continue;
        }

        let mut entered = false;
        for sym in &top_syms {
            if let Some(sd) = sym_data.get(sym) {
                if bar >= TURTLE_ENTRY + 1 && bar < sd.close.len() {
                    if turtle_signal(&sd.close, &sd.high, &sd.low, TURTLE_ENTRY, TURTLE_ATR_PERIOD, ATR_ENTRY_MULT, bar) {
                        let entry_px = sd.close[bar];
                        let mut size_mult = 1.0;
                        
                        if let Some(b) = btc {
                            if bar >= 252 + 21 {
                                let atr_21 = atr_at(&b.high, &b.low, &b.close, 21, bar);
                                let mut hist = Vec::with_capacity(252);
                                for j in (bar + 1 - 252)..=bar {
                                    let h = b.high[j];
                                    let l = b.low[j];
                                    let c0 = b.close[j.saturating_sub(1)];
                                    hist.push((h - l).max((h - c0).abs()).max((l - c0).abs()));
                                }
                                hist.sort_by(|a, b| a.partial_cmp(b).unwrap());
                                let pct_idx = (hedge_atr_pct * hist.len() as f64) as usize;
                                let pct_threshold = hist[pct_idx.min(hist.len().saturating_sub(1))];
                                if atr_21 > pct_threshold {
                                    size_mult = HEDGE_SIZE_MULT;
                                }
                            }
                        }

                        let entry = entry_px * (1.0 + TAKER_FEE);
                        let entry_bar_next = bar + 1;
                        let n = sd.close.len();

                        let mut highest_high = sd.high[entry_bar_next];
                        let max_bar = (entry_bar_next + HOLD_MAX).min(n.saturating_sub(1));
                        let mut exit_bar = max_bar;
                        
                        let mut atr_buf: std::collections::VecDeque<f64> = std::collections::VecDeque::new();
                        
                        for b in entry_bar_next..=max_bar {
                            if sd.high[b] > highest_high { highest_high = sd.high[b]; }
                            let c0 = sd.close[b.saturating_sub(1)];
                            let tr = (sd.high[b] - sd.low[b]).max((sd.high[b] - c0).abs()).max((sd.low[b] - c0).abs());
                            atr_buf.push_back(tr);
                            if atr_buf.len() > TURTLE_ATR_PERIOD { atr_buf.pop_front(); }
                            
                            if atr_buf.len() == TURTLE_ATR_PERIOD {
                                let atr = atr_buf.iter().sum::<f64>() / TURTLE_ATR_PERIOD as f64;
                                let turtle_stop = highest_high - TURTLE_ATR_MULT * atr;
                                if sd.low[b] <= turtle_stop {
                                    exit_bar = b;
                                    break;
                                }
                            }
                        }

                        if let Some(&exit_px) = sd.close.get(exit_bar) {
                            let exit = exit_px * (1.0 - TAKER_FEE);
                            let pct_ret = exit / entry - 1.0;
                            let gross_ret = pct_ret * size_mult;
                            let bars_held = (exit_bar as i64 - entry_bar_next as i64).max(1) as usize;

                            total_trades += 1;
                            equity *= 1.0 + gross_ret;
                            if equity > peak { peak = equity; }

                            let avg_daily = gross_ret / bars_held as f64;
                            for _ in 0..bars_held {
                                daily_rets.push(avg_daily);
                            }

                            bar = exit_bar + 1;
                            entered = true;
                            break;
                        }
                    }
                }
            }
        }
        if !entered {
            bar += 1;
        }
    }
    
    WfResult {
        equity,
        sharpe: annualised_sharpe(&daily_rets),
        dd: max_dd_from(&[1.0, equity]),
        trades: total_trades,
    }
}

struct PctResult {
    pct: f64,
    pass: usize,
    total: usize,
    sharpe: f64,
    ret: f64,
    dd: f64,
    trades: usize,
    base5_equity: f64,
    pass_per_universe: Vec<(String, usize, usize)>,
}

#[tokio::main]
async fn main() -> Result<()> {
    println!("Loading data for HEDGE_ATR_PCT hyperopt...");
    let loader = DataLoader::new(None, None);
    let mut sym_data: HashMap<String, SymData> = HashMap::new();
    let mut min_len = usize::MAX;

    let all_symbols = UNIVERSES.iter()
        .flat_map(|(_, s)| s.iter())
        .map(|&s| s)
        .collect::<std::collections::HashSet<_>>();
    for &sym in &all_symbols {
        let df = loader.fetch_data(sym, "1d", CANDLES).await?;
        let close = df.column("close")?.f64()?.into_no_null_iter().collect::<Vec<_>>();
        let high = df.column("high")?.f64()?.into_no_null_iter().collect::<Vec<_>>();
        let low = df.column("low")?.f64()?.into_no_null_iter().collect::<Vec<_>>();
        let vol = df.column("volume")?.f64()?.into_no_null_iter().collect::<Vec<_>>();
        if close.len() < min_len { min_len = close.len(); }
        sym_data.insert(sym.to_string(), SymData { close, high, low, vol });
    }

    let windows = (min_len.saturating_sub(TRAIN_BARS)) / TEST_BARS;
    if windows == 0 { return Ok(()); }
    let n_windows = windows;
    let n_universes = UNIVERSES.len();

    println!("Loaded {} symbols, min len {}, {} windows", all_symbols.len(), min_len, windows);

    // ============================================================
    // SWEEP: HEDGE_ATR_PCT ∈ [0..=100] step 1  (101 values)
    // ============================================================
    let pct_values: Vec<f64> = (0..=100).map(|v| v as f64).collect();
    let n_pct = pct_values.len();
    println!("Sweeping HEDGE_ATR_PCT ∈ [0..100] step 1 ({} values)", n_pct);

    let mut results: Vec<PctResult> = Vec::with_capacity(n_pct);

    for (pct_idx, &hedge_atr_pct) in pct_values.iter().enumerate() {
        let mut total_passes = 0usize;
        let mut total = 0usize;
        let mut all_sharpes = Vec::with_capacity(n_universes * n_windows);
        let mut all_rets = Vec::with_capacity(n_universes * n_windows);
        let mut all_dds = Vec::with_capacity(n_universes * n_windows);
        let mut all_trades = Vec::with_capacity(n_universes * n_windows);
        let mut all_pass_per_universe = Vec::with_capacity(n_universes);
        let mut base5_agg = 1.0_f64;

        for (u_idx, (u_name, u_syms)) in UNIVERSES.iter().enumerate() {
            let syms: Vec<String> = u_syms.iter().map(|&s| s.to_string()).collect();
            let mut u_passes = 0usize;
            let mut u_sharpes = Vec::with_capacity(n_windows);

            for w in 0..n_windows {
                let start = min_len - (n_windows - w) * TEST_BARS - TRAIN_BARS;
                let end = start + TEST_BARS + TRAIN_BARS;
                let res = run_sim(&sym_data, &syms, start + TRAIN_BARS, end, hedge_atr_pct);

                let passed = if res.trades >= MIN_TRADES && res.sharpe > 0.0 { 1 } else { 0 };
                if passed == 1 { u_passes += 1; }
                total_passes += passed;
                total += 1;

                u_sharpes.push(res.sharpe);
                all_sharpes.push(res.sharpe);
                all_rets.push((res.equity - 1.0) * 100.0);
                all_dds.push(res.dd);
                all_trades.push(res.trades);

                if u_idx == 0 {
                    base5_agg *= res.equity;
                }
            }

            let nw = n_windows as f64;
            all_pass_per_universe.push((u_name.to_string(), u_passes, n_windows));
        }

        let n_total = (n_universes * n_windows) as f64;
        let avg_sharpe = all_sharpes.iter().sum::<f64>() / n_total;
        let avg_ret = all_rets.iter().sum::<f64>() / n_total;
        let avg_dd = all_dds.iter().sum::<f64>() / n_total;
        let total_trades: usize = all_trades.iter().sum();

        let res = PctResult {
            pct: hedge_atr_pct,
            pass: total_passes,
            total,
            sharpe: avg_sharpe,
            ret: avg_ret,
            dd: avg_dd,
            trades: total_trades,
            base5_equity: base5_agg,
            pass_per_universe: all_pass_per_universe,
        };

        results.push(res);

        if pct_idx % 10 == 0 || pct_idx == n_pct - 1 {
            println!("  [{}/{}] PCT={:.0}: {}/{} pass, Sharpe {:.3}, Ret {:.1}%, DD {:.1}%, {} trades",
                pct_idx + 1, n_pct, hedge_atr_pct, total_passes, total,
                avg_sharpe, avg_ret, avg_dd, total_trades);
        }
    }

    // ============================================================
    // Sort ranked results
    // ============================================================
    results.sort_by(|a, b| {
        let pass_a = a.pass as f64 / a.total as f64;
        let pass_b = b.pass as f64 / b.total as f64;
        pass_b.partial_cmp(&pass_a).unwrap()
            .then_with(|| b.sharpe.partial_cmp(&a.sharpe).unwrap())
    });

    // ============================================================
    // Write main summary CSV
    // ============================================================
    let mut summary_csv = File::create("snapshots/t67_hedge_atr_pct_sweep.csv")?;
    writeln!(summary_csv, "pct,pass,total,pass_pct,sharpe,ret_pct,dd_pct,trades,base5_equity")?;
    for r in &results {
        writeln!(summary_csv, "{},{},{},{:.2},{:.4},{:.2},{:.2},{},{:.6}",
            r.pct as i32, r.pass, r.total,
            (r.pass as f64 / r.total as f64) * 100.0,
            r.sharpe, r.ret, r.dd, r.trades, r.base5_equity)?;
    }

    // ============================================================
    // Print ranked results
    // ============================================================
    println!("\n=== RANKED BY PASS RATE (primary) then SHARPE ===");
    println!("{:>4} {:>6} {:>6} {:>8} {:>8} {:>8} {:>7} {:>6}",
        "PCT", "PASS", "TOTAL", "PASS%", "SHARPE", "RET%", "DD%", "TRADES");
    for r in results.iter().take(20) {
        let pp = (r.pass as f64 / r.total as f64) * 100.0;
        println!("{:>4} {:>6} {:>6} {:>7.1}% {:>8.3} {:>8.1}% {:>7.2}% {:>6}",
            r.pct as i32, r.pass, r.total, pp, r.sharpe, r.ret, r.dd, r.trades);
    }

    // ============================================================
    // Export equity for chart configs (Baseline + top 5 + bottom 1)
    // ============================================================
    let chart_pcts: Vec<f64> = {
        let mut pts: Vec<f64> = vec![0.0]; // baseline PCT=0
        for r in results.iter().take(5) {
            if !pts.contains(&r.pct) { pts.push(r.pct); }
        }
        pts.truncate(7);
        pts
    };

    {
        let base5_syms: Vec<String> = UNIVERSES[0].1.iter().map(|&s| s.to_string()).collect();
        let mut f = File::create("snapshots/t67_chart_equity.csv")?;
        writeln!(f, "pct,window,equity")?;
        for &pct in &chart_pcts {
            for w in 0..n_windows {
                let start = min_len - (n_windows - w) * TEST_BARS - TRAIN_BARS;
                let end = start + TEST_BARS + TRAIN_BARS;
                let res = run_sim(&sym_data, &base5_syms, start + TRAIN_BARS, end, pct);
                writeln!(f, "{},{},{:.6}", pct as i32, w, res.equity)?;
            }
        }
        println!("Exported chart equity: snapshots/t67_chart_equity.csv");
    }

    // ============================================================
    // Write markdown report
    // ============================================================
    let mut md = File::create("snapshots/t67_hedge_atr_pct_sweep.md")?;
    writeln!(md, "# T67: HEDGE_ATR_PCT Extensive Hyperopt Results")?;
    writeln!(md, "")?;
    writeln!(md, "**Range:** PCT ∈ [0..=100] step 1 (101 values) × 9 universes × {} WF windows", n_windows)?;
    writeln!(md, "**Strategy:** Turtle-only live path with USDT hedge overlay")?;
    writeln!(md, "**Other params fixed:** SM=0.40, VL=92, AP=17, LB=41, T=5")?;
    writeln!(md, "")?;
    writeln!(md, "## Top 20 by Pass Rate / Sharpe")?;
    writeln!(md, "")?;
    writeln!(md, "| PCT | Pass | Total | Pass% | Sharpe | Return% | DD% | Trades | Base5 Equity |")?;
    writeln!(md, "|-----|------|--------|--------|--------|---------|-----|--------|---------------|")?;
    for r in results.iter().take(20) {
        let pp = (r.pass as f64 / r.total as f64) * 100.0;
        writeln!(md, "| {:>3} | {} | {} | {:.1}% | {:.3} | {:.1}% | {:.1}% | {} | {:.4}x |",
            r.pct as i32, r.pass, r.total, pp, r.sharpe, r.ret, r.dd, r.trades, r.base5_equity)?;
    }
    writeln!(md, "")?;
    writeln!(md, "## Winner")?;
    if let Some(winner) = results.first() {
        let pp = (winner.pass as f64 / winner.total as f64) * 100.0;
        writeln!(md, "**PCT={:.0}** — {} pass ({:.1}%), Sharpe {:.3}, Return {:.1}%, DD {:.1}%, {} trades, Base5 equity {:.4}x",
            winner.pct, winner.pass, pp, winner.sharpe, winner.ret, winner.dd, winner.trades, winner.base5_equity)?;
    }
    writeln!(md, "")?;
    writeln!(md, "## Baseline (PCT=0, no hedge)")?;
    if let Some(baseline) = results.iter().find(|r| r.pct == 0.0) {
        let pp = (baseline.pass as f64 / baseline.total as f64) * 100.0;
        writeln!(md, "PCT=0: {} pass ({:.1}%), Sharpe {:.3}, Return {:.1}%, DD {:.1}%, {} trades, Base5 equity {:.4}x",
            baseline.pass, pp, baseline.sharpe, baseline.ret, baseline.dd, baseline.trades, baseline.base5_equity)?;
    }
    writeln!(md, "")?;
    writeln!(md, "## Per-Universe Pass Rates (Winner vs Baseline)")?;
    if let Some(winner) = results.first() {
        if let Some(baseline) = results.iter().find(|r| r.pct == 0.0) {
            writeln!(md, "| Universe | {} (Baseline) | {} (Winner) | Delta |", 0, winner.pct as i32)?;
            writeln!(md, "|----------|-------------|-----------|-------|")?;
            for (i, (name, pass, _)) in winner.pass_per_universe.iter().enumerate() {
                let base_pass = &baseline.pass_per_universe[i];
                let delta = *pass as i32 - base_pass.1 as i32;
                writeln!(md, "| {} | {}/{} | {}/{} | {} |",
                    name, base_pass.1, base_pass.2, pass, base_pass.2, delta)?;
            }
        }
    }
    writeln!(md, "")?;
    writeln!(md, "*Chart: charts/t67_comparison_chart.png*")?;

    println!("\nDone. Summary: snapshots/t67_hedge_atr_pct_sweep.csv");
    println!("Chart equity: snapshots/t67_chart_equity.csv");
    println!("Report: snapshots/t67_hedge_atr_pct_sweep.md");

    Ok(())
}
