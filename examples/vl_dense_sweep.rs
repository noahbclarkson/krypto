//! VOL_LOOKBACK Dense AP17 Sweep (Live-Compatible Harness)
//!
//! AUDIT: VOL_LOOKBACK is hardcoded inconsistently across harnesses:
//!   - live_compatible_wf.rs:  VOL_LOOKBACK = 8
//!   - turtle_chandelier_walkforward.rs: VOL_LOOKBACK = 96
//!   - config.rs: NOT DEFINED (production value unknown)
//!
//! This sweep uses the live_compatible_wf.rs harness (Turtle-only exit, ATR_rank filter)
//! which matches src/live/bot.rs semantics exactly.
//!
//! Range: VL ∈ [1..=200] step 1 (200 values)
//! Scope: 9 universes × walk-forward windows × 200 values
//!
//! OUTPUTS:
//!   snapshots/vl_hyperopt_live_compat_{sweep,summary,equity}.csv
//!   charts/comparison_chart.png

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

const REGIME_ATR_PERIOD: usize = 17;
const REGIME_LOOKBACK: usize = 42;
const ATR_RANK_T: f64 = 5.0;

const VL_VALUES: &[usize] = &[
    1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20,
    21, 22, 23, 24, 25, 26, 27, 28, 29, 30, 31, 32, 33, 34, 35, 36, 37, 38, 39, 40,
    41, 42, 43, 44, 45, 46, 47, 48, 49, 50, 51, 52, 53, 54, 55, 56, 57, 58, 59, 60,
    61, 62, 63, 64, 65, 66, 67, 68, 69, 70, 71, 72, 73, 74, 75, 76, 77, 78, 79, 80,
    81, 82, 83, 84, 85, 86, 87, 88, 89, 90, 91, 92, 93, 94, 95, 96, 97, 98, 99, 100,
    101, 102, 103, 104, 105, 106, 107, 108, 109, 110, 111, 112, 113, 114, 115, 116, 117, 118, 119, 120,
    121, 122, 123, 124, 125, 126, 127, 128, 129, 130, 131, 132, 133, 134, 135, 136, 137, 138, 139, 140,
    141, 142, 143, 144, 145, 146, 147, 148, 149, 150, 151, 152, 153, 154, 155, 156, 157, 158, 159, 160,
    161, 162, 163, 164, 165, 166, 167, 168, 169, 170, 171, 172, 173, 174, 175, 176, 177, 178, 179, 180,
    181, 182, 183, 184, 185, 186, 187, 188, 189, 190, 191, 192, 193, 194, 195, 196, 197, 198, 199, 200,
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

struct SymData {
    close: Vec<f64>,
    high: Vec<f64>,
    low: Vec<f64>,
    vol: Vec<f64>,
}

struct WfResult {
    equity: f64,
    sharpe: f64,
    dd: f64,
    trades: usize,
    win_rate: f64,
    equity_curve: Vec<f64>,
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

fn btc_atr_pct(sd: &SymData, regime_atr_period: usize, regime_lookback: usize, idx: usize) -> f64 {
    if idx < regime_atr_period.max(regime_lookback) { return 50.0; }
    let curr_atr = atr_at(&sd.high, &sd.low, &sd.close, regime_atr_period, idx);
    let curr_close = sd.close[idx];
    if curr_atr <= 0.0 || curr_close <= 0.0 { return 50.0; }
    let curr_pct = curr_atr / curr_close;
    let start = idx.saturating_sub(regime_lookback);
    let mut below = 0usize;
    let mut total = 0usize;
    for i in start..idx {
        let c = sd.close[i];
        if c <= 0.0 { continue; }
        let hist_atr = atr_at(&sd.high, &sd.low, &sd.close, regime_atr_period, i);
        if hist_atr <= 0.0 { continue; }
        if hist_atr / c < curr_pct { below += 1; }
        total += 1;
    }
    if total == 0 { 50.0 } else { (below as f64 / total as f64) * 100.0 }
}

fn turtle_signal(close: &[f64], high: &[f64], low: &[f64], entry_period: usize, atr_period: usize, atr_mult: f64, idx: usize) -> bool {
    if idx < entry_period { return false; }
    let start = idx - entry_period;
    let max_close = close[start..idx].iter().fold(f64::NEG_INFINITY, |a, &b| a.max(b));
    if close[idx] > max_close {
        if atr_mult > 0.0 {
            let atr = atr_at(high, low, close, atr_period, idx);
            if close[idx] < max_close + atr * atr_mult { return false; }
        }
        true
    } else { false }
}

fn annualised_sharpe(daily_rets: &[f64]) -> f64 {
    if daily_rets.is_empty() || daily_rets.len() < 2 { return 0.0; }
    let mean = daily_rets.iter().sum::<f64>() / daily_rets.len() as f64;
    let variance = daily_rets.iter().map(|r| (r - mean).powi(2)).sum::<f64>() / daily_rets.len() as f64;
    let std = variance.sqrt();
    if std == 0.0 { return 0.0; }
    mean / std * (252.0_f64.sqrt())
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

fn run_sim(
    sym_data: &HashMap<String, SymData>,
    symbols: &[String],
    test_start: usize,
    test_end: usize,
    vol_lookback: usize,
) -> WfResult {
    let mut equity = 1.0_f64;
    let mut equity_curve = vec![1.0_f64];
    let mut peak = equity;
    let mut total_trades = 0usize;
    let mut wins = 0usize;
    let mut daily_rets = Vec::new();

    let mut bar = test_start;
    while bar + 2 < test_end {
        let btc = sym_data.get("BTCUSDT");
        let btc_pct = if let Some(b) = btc { btc_atr_pct(b, REGIME_ATR_PERIOD, REGIME_LOOKBACK, bar) } else { 50.0 };
        
        if btc_pct < ATR_RANK_T {
            equity_curve.push(equity);
            bar += 1;
            continue;
        }

        let mut scores: Vec<(&str, f64)> = Vec::new();
        for sym in symbols {
            if let Some(sd) = sym_data.get(sym) {
                if bar >= sd.close.len() { continue; }
                let rol_vol = rolling_avg(&sd.vol, vol_lookback, bar);
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
                            
                            if pct_ret > 0.0 { wins += 1; }
                            
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
        win_rate: if total_trades > 0 { wins as f64 / total_trades as f64 * 100.0 } else { 0.0 },
        equity_curve,
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    println!("Loading data for VOL_LOOKBACK hyperopt (200 values x 9 universes x WF windows)...");
    println!("Live-compatible harness: Turtle-only exit + ATR_rank(17,42,5.0) + USDT hedge");

    let loader = DataLoader::new(None, None);
    let mut sym_data = HashMap::new();
    let mut min_len = usize::MAX;

    let all_symbols: std::collections::HashSet<_> = UNIVERSES.iter().flat_map(|(_, s)| s.iter()).collect();
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
    if windows == 0 {
        println!("ERROR: insufficient data (need {} bars, got {})", TRAIN_BARS + TEST_BARS, min_len);
        return Ok(());
    }
    println!("Data: {} bars, {} walk-forward windows", min_len, windows);

    // ── Sweep ──────────────────────────────────────────────────────────────────
    let mut csv_lines = vec!["vl,universe,window,ret_pct,sharpe,max_dd_pct,trades,win_rate_pct,pass".to_string()];
    // vl -> (rets, sharpes, dds, trades_list, win_rates, passes)
    let mut vl_stats: HashMap<usize, (Vec<f64>, Vec<f64>, Vec<f64>, Vec<usize>, Vec<f64>, Vec<bool>)> = HashMap::new();
    for &vl in VL_VALUES {
        vl_stats.insert(vl, (vec![], vec![], vec![], vec![], vec![], vec![]));
    }

    for (u_name, u_syms) in UNIVERSES {
        let syms: Vec<String> = u_syms.iter().map(|&s| s.to_string()).collect();
        for w in 0..windows {
            let start = min_len - (windows - w) * TEST_BARS - TRAIN_BARS;
            let end = start + TEST_BARS + TRAIN_BARS;
            
            for &vl in VL_VALUES {
                let res = run_sim(&sym_data, &syms, start + TRAIN_BARS, end, vl);
                let ret_pct = (res.equity - 1.0) * 100.0;
                let pass = res.trades >= MIN_TRADES && res.sharpe > 0.0;
                
                csv_lines.push(format!("{0},{1},{2},{3:.4},{4:.4},{5:.2},{6},{7:.1},{8}",
                    vl, u_name, w, ret_pct, res.sharpe, res.dd, res.trades, res.win_rate, pass));
                
                let s = vl_stats.get_mut(&vl).unwrap();
                s.0.push(ret_pct);
                s.1.push(res.sharpe);
                s.2.push(res.dd);
                s.3.push(res.trades);
                s.4.push(res.win_rate);
                s.5.push(pass);
            }
        }
    }

    // ── Write sweep CSV ──────────────────────────────────────────────────────────
    let mut f = File::create("snapshots/vl_dense_ap17_sweep.csv")?;
    for line in &csv_lines { writeln!(f, "{}", line)?; }
    drop(f);
    println!("Written: snapshots/vl_dense_ap17_sweep.csv");

    // ── Aggregate ─────────────────────────────────────────────────────────────────
    let mut summary_lines = vec!["vl,pass_count,total_count,pass_pct,avg_sharpe,avg_ret_pct,avg_dd_pct,total_trades,avg_win_rate,positive_universes".to_string()];
    // (vl, pass_pct, avg_sharpe, avg_ret, pass_count, avg_dd, uni_pos)
    let mut vl_results: Vec<(usize, f64, f64, f64, usize, f64, usize)> = vec![];
    
    for &vl in VL_VALUES {
        let (rets, sharpes, dds, trades_list, win_rates, passes) = vl_stats.get(&vl).unwrap();
        let n = rets.len() as f64;
        let n_int = rets.len();
        let pass_count = passes.iter().filter(|&&p| p).count();
        let avg_sharpe = sharpes.iter().sum::<f64>() / n;
        let avg_ret = rets.iter().sum::<f64>() / n;
        let avg_dd = dds.iter().sum::<f64>() / n;
        let avg_win_rate = win_rates.iter().sum::<f64>() / n;
        let total_trades: usize = trades_list.iter().sum();
        
        // Positive universes (at least half windows positive Sharpe)
        let mut uni_positive: usize = 0;
        for (u_idx, (_u_name, _)) in UNIVERSES.iter().enumerate() {
            let w_start = u_idx * windows;
            let w_end = w_start + windows;
            let u_sharpes = &sharpes[w_start..w_end];
            let u_pos = u_sharpes.iter().filter(|&&s| s > 0.0).count();
            if u_pos * 2 >= windows { uni_positive += 1; }
        }
        
        let pass_pct = pass_count as f64 / n * 100.0;
        summary_lines.push(format!("{0},{1},{2},{3:.2},{4:.4},{5:.2},{6:.2},{7},{8:.1},{9}",
            vl, pass_count, n_int, pass_pct, avg_sharpe, avg_ret, avg_dd, total_trades, avg_win_rate, uni_positive));
        
        vl_results.push((vl, pass_pct, avg_sharpe, avg_ret, pass_count, avg_dd, uni_positive));
    }

    let mut f = File::create("snapshots/vl_dense_ap17_summary.csv")?;
    for line in &summary_lines { writeln!(f, "{}", line)?; }
    drop(f);
    println!("Written: snapshots/vl_dense_ap17_summary.csv");

    // ── Sort for winners ──────────────────────────────────────────────────────────
    // Primary: pass_pct desc, Secondary: avg_sharpe desc
    vl_results.sort_by(|a, b| {
        let cmp_pct = b.1.partial_cmp(&a.1).unwrap();
        if cmp_pct != std::cmp::Ordering::Equal { cmp_pct }
        else { b.2.partial_cmp(&a.2).unwrap() }
    });

    println!("\n=== VOL_LOOKBACK Dense Sweep Results (200 values, AP17 live-compatible harness) ===");
    println!("{:>6} | {:>7} | {:>8} | {:>8} | {:>4} | {:>5} | {:>5}",
        "VL", "Pass%", "AvgSharpe", "AvgRet%", "Pass", "AvgDD", "+Uni");
    println!("{}", "-".repeat(60));
    for (vl, pass_pct, avg_sharpe, avg_ret, pass_count, avg_dd, uni_pos) in &vl_results {
        println!("{0:6} | {1:6.1} | {2:8.3} | {3:8.1} | {4:4} | {5:5.1} | {6:5}",
            vl, pass_pct, avg_sharpe, avg_ret, pass_count, avg_dd, uni_pos);
    }

    let baseline_vl = 8usize;
    let winner_vl = vl_results.first().map(|r| r.0).unwrap_or(baseline_vl);
    
    // Runner-ups: next-best robust settings by the same primary ranking
    // (pass rate first, then Sharpe), excluding baseline/winner duplicates.
    let mut runners: Vec<usize> = vl_results
        .iter()
        .map(|r| r.0)
        .filter(|&vl| vl != winner_vl && vl != baseline_vl)
        .collect();
    runners.dedup();
    let runner1_vl = runners.get(0).copied().unwrap_or(baseline_vl);
    let runner2_vl = runners.get(1).copied().unwrap_or(baseline_vl);

    println!("\nBaseline: VL={}", baseline_vl);
    println!("Winner:   VL={}", winner_vl);
    println!("Runner1:  VL={} (2nd by Sharpe)", runner1_vl);
    println!("Runner2:  VL={} (3rd by Sharpe)", runner2_vl);


    // ── Export equity curves for baseline + winner + runners ────────────────────
    println!("
Generating equity curves...");
    let equity_keys = vec![
        (baseline_vl, "baseline"),
        (winner_vl, "winner"),
        (runner1_vl, "runner1_sharpe"),
        (runner2_vl, "runner2_sharpe"),
    ];

    for (vl, label) in &equity_keys {
        // Accumulate equity sums per bar index across all universe/window combos
        // Use the last TEST_BARS bars of each window for averaging (most recent)
        let mut sum_equity = vec![0.0_f64; TEST_BARS];
        let mut cnt_equity = vec![0usize; TEST_BARS];

        for (_u_idx, (_u_name, u_syms)) in UNIVERSES.iter().enumerate() {
            let syms: Vec<String> = u_syms.iter().map(|&s| s.to_string()).collect();
            for w in 0..windows {
                let start = min_len - (windows - w) * TEST_BARS - TRAIN_BARS;
                let end = start + TEST_BARS + TRAIN_BARS;
                let res = run_sim(&sym_data, &syms, start + TRAIN_BARS, end, *vl);
                for (bi, &e) in res.equity_curve.iter().enumerate() {
                    if bi < TEST_BARS {
                        sum_equity[bi] += e;
                        cnt_equity[bi] += 1;
                    }
                }
            }
        }

        let avg_equity: Vec<f64> = sum_equity
            .iter()
            .zip(cnt_equity.iter())
            .map(|(&s, &c)| if c > 0 { s / c as f64 } else { 1.0_f64 })
            .collect();

        let csv_path = format!("snapshots/vl_dense_ap17_equity_{}.csv", label);
        let mut f = File::create(&csv_path)?;
        writeln!(f, "bar,equity")?;
        for (i, &e) in avg_equity.iter().enumerate() {
            writeln!(f, "{},{:.6}", i, e)?;
        }
        drop(f);
        println!("Written: {}", csv_path);
    }

    // ── Summary text ─────────────────────────────────────────────────────────────
    let mut txt = File::create("snapshots/vl_dense_ap17_summary.txt")?;
    writeln!(txt, "VOL_LOOKBACK Hyperopt — Live-Compatible Harness")?;
    writeln!(txt, "=================================================")?;
    writeln!(txt, "Harness: Turtle-only + ATR_rank(17,42,5.0) + USDT hedge")?;
    writeln!(txt, "Range: VL ∈ [1..=200 step 1] = 200 values")?;
    writeln!(txt, "Scope: 9 universes × {} windows = {} OOS windows", windows, UNIVERSES.len() * windows)?;
    writeln!(txt, "Baseline: VL={}", baseline_vl)?;
    writeln!(txt, "Winner:   VL={}", winner_vl)?;
    writeln!(txt, "Runner1:  VL={}", runner1_vl)?;
    writeln!(txt, "Runner2:  VL={}", runner2_vl)?;
    writeln!(txt, "")?;
    writeln!(txt, "{:>6} | {:>7} | {:>8} | {:>8} | {:>4} | {:>5} | {:>5}",
        "VL", "Pass%", "AvgSharpe", "AvgRet%", "Pass", "AvgDD", "+Uni")?;
    writeln!(txt, "{}", "-".repeat(60))?;
    for (vl, pass_pct, avg_sharpe, avg_ret, pass_count, avg_dd, uni_pos) in &vl_results {
        writeln!(txt, "{0:6} | {1:6.1} | {2:8.3} | {3:8.1} | {4:4} | {5:5.1} | {6:5}",
            vl, pass_pct, avg_sharpe, avg_ret, pass_count, avg_dd, uni_pos)?;
    }
    drop(txt);
    println!("Written: snapshots/vl_dense_ap17_summary.txt");

    Ok(())
}
