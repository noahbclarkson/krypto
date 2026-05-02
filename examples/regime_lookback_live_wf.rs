//! REGIME_LOOKBACK Extensive Hyperopt — Live Turtle-Only Path
//!
//! TARGET: Audit the hardcoded REGIME_LOOKBACK=42 in live_compatible_wf.rs
//! REGIME_LOOKBACK is the historical window for BTC ATR percentile rank calculation.
//! It has NEVER been systematically tested beyond {42, 252}.
//!
//! SWEEP: LB ∈ [5..=200 step 1] (196 values) × 9 universes × 7 WF windows
//!         = ~12,348 window-runs total
//!
//! Strategy: Turtle-only exit (matches src/live/bot.rs exactly after 2026-05-01 fix)
//! - Entry: Turtle breakout (EP=21) + ATR_RANK(AP=64, LB={sweep}, T=24) gate
//! - Exit: Turtle ATR trailing stop (AP=24, M=2.0) + HOLD_MAX=12
//! - Risk overlay: USDT 30% size when BTC 21d ATR > 75th pct of 252d history
//! - Fee: 0.10% taker (both sides)
//!
//! Robustness-first selection: pass rate > positive universes > avg Sharpe > DD
//!
//! Usage:
//!   cargo run --release --example regime_lookback_live_wf --profile sweep

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
const HOLD_MAX: usize = 12;
const TAKER_FEE: f64 = 0.001;
const POSITION_CAP: usize = 3;
const MIN_TRADES: usize = 3;

const TURTLE_ENTRY: usize = 21;
const TURTLE_ATR_PERIOD: usize = 24;
const TURTLE_ATR_MULT: f64 = 2.00;
const ATR_ENTRY_MULT: f64 = 0.00;
const VOL_LOOKBACK: usize = 8;

// ATR_RANK threshold = 24.0 (production default, from hyperopt 2026-05-01)
// REGIME_ATR_PERIOD = 64 (production default, from hyperopt 2026-05-02)
const REGIME_ATR_PERIOD: usize = 64;
const ATR_RANK_T: f64 = 24.0;

// LB sweep: [5..=200 step 1] = 196 values
const LB_MIN: usize = 5;
const LB_MAX: usize = 200;
// Step 1 for dense coverage

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

#[derive(Default)]
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
    regime_lookback: usize,
) -> WfResult {
    let mut equity = 1.0_f64;
    let mut equity_curve = vec![1.0_f64];
    let mut peak = equity;
    let mut total_trades = 0usize;
    let mut daily_rets = Vec::new();

    let mut bar = test_start;
    while bar + 2 < test_end {
        let btc = sym_data.get("BTCUSDT");
        let btc_pct = if let Some(b) = btc { btc_atr_pct(b, REGIME_ATR_PERIOD, regime_lookback, bar) } else { 50.0 };
        
        if btc_pct < ATR_RANK_T {
            equity_curve.push(equity);
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
            equity_curve.push(equity);
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
                                let pct_75 = hist[(0.75 * hist.len() as f64) as usize];
                                if atr_21 > pct_75 {
                                    size_mult = 0.70;
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
    
    WfResult {
        equity,
        sharpe: annualised_sharpe(&daily_rets),
        dd: max_dd_from(&equity_curve),
        trades: total_trades,
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    let start_time = Instant::now();
    println!("Loading data...");
    let loader = DataLoader::new(None, None);
    let mut sym_data = HashMap::new();
    let mut min_len = usize::MAX;

    let all_symbols = UNIVERSES.iter().flat_map(|(_, s)| s.iter()).map(|&s| s).collect::<std::collections::HashSet<_>>();
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
    println!("Data loaded. {} bars available. {} WF windows.", min_len, windows);

    // ── LB sweep ────────────────────────────────────────────────────────────────
    let lb_values: Vec<usize> = (LB_MIN..=LB_MAX).collect();
    let n_lb = lb_values.len();
    println!("Sweeping LB ∈ [{}..={}] ({} values) × 9 universes × {} windows",
        LB_MIN, LB_MAX, n_lb, windows);
    
    // ── Per-LB results ─────────────────────────────────────────────────────────
    let mut results: HashMap<usize, (usize, usize, f64, f64, f64, usize)> = HashMap::new();
    // (pass, total, avg_sharpe, avg_ret, avg_dd, total_trades)
    
    // Per-LB per-universe pass count for positive-universe counting
    let mut lb_universe_passes: HashMap<usize, usize> = HashMap::new();
    
    for lb in &lb_values {
        let mut lb_passes = 0usize;
        let mut lb_total = 0usize;
        let mut lb_sharpes = vec![];
        let mut lb_rets = vec![];
        let mut lb_dds = vec![];
        let mut lb_trades = 0usize;
        
        for (_, u_syms) in UNIVERSES {
            let syms: Vec<String> = u_syms.iter().map(|&s| s.to_string()).collect();
            let mut uni_positive = false;
            let mut uni_pass_count = 0usize;
            
            for w in 0..windows {
                let s = min_len - (windows - w) * TEST_BARS - TRAIN_BARS;
                let e = s + TEST_BARS + TRAIN_BARS;
                let res = run_sim(&sym_data, &syms, s + TRAIN_BARS, e, *lb);
                
                lb_total += 1;
                if res.trades >= MIN_TRADES && res.sharpe > 0.0 {
                    lb_passes += 1;
                    uni_pass_count += 1;
                    uni_positive = true;
                }
                lb_sharpes.push(res.sharpe);
                lb_rets.push((res.equity - 1.0) * 100.0);
                lb_dds.push(res.dd);
                lb_trades += res.trades;
            }
            
            if uni_positive {
                *lb_universe_passes.entry(*lb).or_insert(0) += 1;
            }
        }
        
        let n = lb_total as f64;
        let avg_s = lb_sharpes.iter().sum::<f64>() / n;
        let avg_r = lb_rets.iter().sum::<f64>() / n;
        let avg_d = lb_dds.iter().sum::<f64>() / n;
        results.insert(*lb, (lb_passes, lb_total, avg_s, avg_r, avg_d, lb_trades));
    }

    // ── Write sweep CSV ────────────────────────────────────────────────────────
    let sweep_path = "snapshots/regime_lookback_lb_sweep.csv";
    let mut sf = File::create(sweep_path)?;
    writeln!(sf, "lb,pass,total,pass_pct,avg_sharpe,avg_ret_pct,avg_dd_pct,trades,pos_universes")?;
    for lb in &lb_values {
        let (p, t, avg_s, avg_r, avg_d, trades) = results[lb];
        let pos_u = lb_universe_passes.get(lb).copied().unwrap_or(0);
        writeln!(sf, "{},{},{},{:.2},{:.4},{:.2},{:.2},{},{}", 
            lb, p, t, (p as f64/t as f64)*100.0, avg_s, avg_r, avg_d, trades, pos_u)?;
    }
    println!("Sweep CSV written: {}", sweep_path);

    // ── Find top-LB candidates (robustness-first) ───────────────────────────────
    let mut sorted: Vec<usize> = lb_values.clone();
    sorted.sort_by(|&a, &b| {
        let (ap, _, ash, ar, _, _) = results[&a];
        let (bp, _, bsh, br, _, _) = results[&b];
        // Sort: pass desc, pos_universes desc, sharpe desc, ret desc
        let pa = (ap as isize, *lb_universe_passes.get(&a).unwrap_or(&0) as isize, 
                  (ash * 1000.0) as isize, (ar * 10.0) as isize);
        let pb = (bp as isize, *lb_universe_passes.get(&b).unwrap_or(&0) as isize,
                  (bsh * 1000.0) as isize, (br * 10.0) as isize);
        pa.cmp(&pb)
    });
    sorted.reverse();

    println!("\nTop 10 LB values by robustness:");
    println!("{:>4} {:>4} {:>6} {:>8} {:>8} {:>8} {:>10}", "LB", "Pass", "Pct%", "Sharpe", "Ret%", "DD%", "PosUni");
    for &lb in sorted.iter().take(10) {
        let (p, t, avg_s, avg_r, avg_d, _) = results[&lb];
        let pos_u = lb_universe_passes.get(&lb).copied().unwrap_or(0);
        println!("{:>4} {:>4} {:>6.1} {:>8.3} {:>8.1} {:>8.1} {:>10}", 
            lb, p, (p as f64/t as f64)*100.0, avg_s, avg_r, avg_d, pos_u);
    }
    
    // Baseline comparison (LB=42, current hardcoded)
    let baseline = results[&42];
    let winner = sorted[0];
    let (wp, wt, wsh, wr, wd, _) = results[&winner];
    let wpu = lb_universe_passes.get(&winner).copied().unwrap_or(0);
    let bpu = lb_universe_passes.get(&42).copied().unwrap_or(0);
    println!("\nBaseline LB=42: {}/{} ({:.1}%), Sharpe {:.3}, pos_uni={}", 
        baseline.0, baseline.1, (baseline.0 as f64/baseline.1 as f64)*100.0, baseline.2, bpu);
    println!("Winner LB={}: {}/{} ({:.1}%), Sharpe {:.3}, pos_uni={}, ΔSharpe {:+.1}%, ΔPass {:+}", 
        winner, wp, wt, (wp as f64/wt as f64)*100.0, wsh, wpu, (wsh-baseline.2)/baseline.2*100.0, wp as isize - baseline.0 as isize);

    // ── Equity curve export for baseline + top 3 LBs ─────────────────────────
    // Per window per LB for Base5 (universe 0)
    let export_lbs = [42usize, winner];
    // Add 2nd and 3rd best if they're different
    if sorted.len() >= 2 && sorted[1] != winner { /* already included */ }
    if sorted.len() >= 3 && sorted[2] != winner && sorted[2] != 42 { /* include */ }
    
    let top3: Vec<usize> = sorted.iter().take(5).cloned().filter(|&l| l != winner && l != 42).take(2).collect();
    let all_chart_lbs: Vec<usize> = [42, winner].iter().cloned().chain(top3.iter().cloned()).collect();
    
    println!("\nExporting equity curves for LBs: {:?}", all_chart_lbs);
    
    for &chart_lb in &all_chart_lbs {
        let mut csv_path = File::create(format!("snapshots/regime_lookback_lb{}_base5_daily.csv", chart_lb))?;
        writeln!(csv_path, "window,equity")?;
        let base5_syms: Vec<String> = UNIVERSES[0].1.iter().map(|&s| s.to_string()).collect();
        
        for w in 0..windows {
            let s = min_len - (windows - w) * TEST_BARS - TRAIN_BARS;
            let e = s + TEST_BARS + TRAIN_BARS;
            let res = run_sim(&sym_data, &base5_syms, s + TRAIN_BARS, e, chart_lb);
            writeln!(csv_path, "{},{:.6}", w, res.equity)?;
        }
    }
    
    // Also write a combined equity for all 9 universes per LB (for the chart)
    for &chart_lb in &all_chart_lbs {
        let mut agg_equity = 1.0_f64;
        let mut csv_path = File::create(format!("snapshots/regime_lookback_lb{}_global_daily.csv", chart_lb))?;
        writeln!(csv_path, "window,equity")?;
        
        for w in 0..windows {
            let mut window_product = 1.0_f64;
            for (_, u_syms) in UNIVERSES {
                let syms: Vec<String> = u_syms.iter().map(|&s| s.to_string()).collect();
                let s = min_len - (windows - w) * TEST_BARS - TRAIN_BARS;
                let e = s + TEST_BARS + TRAIN_BARS;
                let res = run_sim(&sym_data, &syms, s + TRAIN_BARS, e, chart_lb);
                window_product *= res.equity;
            }
            agg_equity *= window_product;
            writeln!(csv_path, "{},{:.6}", w, agg_equity)?;
        }
    }
    
    println!("\nElapsed: {:.1}s", start_time.elapsed().as_secs_f64());

    // ── Write markdown summary ──────────────────────────────────────────────────
    let winner_lb = winner;
    let (wp, wt, wsh, wr, wd, wtr) = results[&winner_lb];
    let wpu = lb_universe_passes.get(&winner_lb).copied().unwrap_or(0);
    let (bp, bt, bsh, br, bd, btr) = results[&42];
    let bpu = lb_universe_passes.get(&42).copied().unwrap_or(0);
    
    let md_path = "snapshots/regime_lookback_lb_summary.md";
    let mut mf = File::create(md_path)?;
    writeln!(mf, "# REGIME_LOOKBACK Extensive Hyperopt — Live Turtle-Only Path")?;
    writeln!(mf, "")?;
    writeln!(mf, "**Date:** 2026-05-02")?;
    writeln!(mf, "**Strategy:** Turtle-only exit (matches `src/live/bot.rs` after 2026-05-01 fix)")?;
    writeln!(mf, "**Sweep:** LB ∈ [{}..={}] step 1 ({} values) × 9 universes × {} windows", LB_MIN, LB_MAX, n_lb, windows)?;
    writeln!(mf, "**Fixed params:** EP=21, ATR(64,2.0), T=24, HOLD_MAX=12, CAP=3, VL=8, USDT hedge, fee=0.10%")?;
    writeln!(mf, "")?;
    writeln!(mf, "## Robustness Winner: LB={}", winner_lb)?;
    writeln!(mf, "")?;
    writeln!(mf, "| Metric | LB={} (Winner) | LB=42 (Baseline) | Delta |", winner_lb)?;
    writeln!(mf, "|--------|------------|-----------------|-------|")?;
    writeln!(mf, "| Global Pass | {}/{} ({:.1}%) | {}/{} ({:.1}%) | {:+} |", wp, wt, (wp as f64/wt as f64)*100.0, bp, bt, (bp as f64/bt as f64)*100.0, wp as isize - bp as isize)?;
    writeln!(mf, "| Avg Sharpe | {:.3} | {:.3} | {:+.1}% |", wsh, bsh, (wsh-bsh)/bsh*100.0)?;
    writeln!(mf, "| Avg Return | {:.1}% | {:.1}% | {:+.1}pp |", wr, br, wr-br)?;
    writeln!(mf, "| Avg DD | {:.1}% | {:.1}% | {:+.1}pp |", wd, bd, wd-bd)?;
    writeln!(mf, "| Total Trades | {} | {} | {} |", wtr, btr, wtr as isize - btr as isize)?;
    writeln!(mf, "| Positive Universes | {}/9 | {}/9 | {:+} |", wpu, bpu, wpu as isize - bpu as isize)?;
    writeln!(mf, "")?;
    writeln!(mf, "## Top 10 LB Values (Robustness-First)")?;
    writeln!(mf, "")?;
    writeln!(mf, "| LB | Pass | Pct% | Sharpe | Ret% | DD% | PosUni |")?;
    writeln!(mf, "|----|------|------|--------|------|-----|--------|")?;
    for &lb in sorted.iter().take(10) {
        let (p, t, avg_s, avg_r, avg_d, _) = results[&lb];
        let pos_u = lb_universe_passes.get(&lb).copied().unwrap_or(0);
        writeln!(mf, "| {} | {}/{} | {:.1}% | {:.3} | {:.1}% | {:.1}% | {}/9 |", 
            lb, p, t, (p as f64/t as f64)*100.0, avg_s, avg_r, avg_d, pos_u)?;
    }
    writeln!(mf, "")?;
    writeln!(mf, "*Pass = windows with ≥{} trades AND Sharpe>0 / total windows.*", MIN_TRADES)?;
    writeln!(mf, "")?;
    writeln!(mf, "## Conclusion")?;
    if winner_lb != 42 {
        writeln!(mf, "LB={} is the winner over the baseline LB=42. ", winner_lb)?;
        writeln!(mf, "Update `src/live/config.rs`: `REGIME_LOOKBACK = {}`", winner_lb)?;
        writeln!(mf, "And update `examples/live_compatible_wf.rs`: `REGIME_LOOKBACK = {}`", winner_lb)?;
    } else {
        writeln!(mf, "LB=42 (baseline) is already optimal. No change needed.")?;
    }
    writeln!(mf, "See `charts/comparison_chart.png` for equity curve comparison.")?;
    println!("\nSummary written to {}", md_path);
    
    // Print all values to stdout for capture
    println!("\n=== ALL LB RESULTS ===");
    println!("lb,pass,total,pass_pct,avg_sharpe,avg_ret_pct,avg_dd_pct,trades,pos_universes");
    for lb in &lb_values {
        let (p, t, avg_s, avg_r, avg_d, trades) = results[lb];
        let pos_u = lb_universe_passes.get(lb).copied().unwrap_or(0);
        println!("{},{},{},{:.2},{:.4},{:.2},{:.2},{},{}", 
            lb, p, t, (p as f64/t as f64)*100.0, avg_s, avg_r, avg_d, trades, pos_u);
    }

    Ok(())
}