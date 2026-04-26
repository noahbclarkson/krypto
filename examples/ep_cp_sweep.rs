// EP × CP 2D Surface Sweep
// Based on turtle_chandelier_walkforward.rs (working harness)
// Tests EP ∈ [15..30 step 1] × CP ∈ [5..20 step 1]
// Walk-forward: Base5 universe, 504-bar train / 504-bar test
//
// Key test: CP=7 was found while EP=24 was active. EP=24 is REVERTED.
// The EP×CP pairing at current defaults (EP=21, CP=7) has never been validated.

use anyhow::Result;
use krypto::data::loader::DataLoader;
use polars::prelude::*;
use std::collections::HashMap;
use std::fs::File;
use std::io::Write;
use std::time::Instant;

const CANDLES: u32 = 5000;
const TRAIN_BARS: usize = 504;
const TEST_BARS: usize = 504;
const HOLD_MAX: usize = 12;
const TAKER_FEE: f64 = 0.001;
const POSITION_CAP: usize = 3;
const MIN_TRADES: usize = 3;
const CHAND_MULT: f64 = 2.30;
const TURTLE_ATR_PERIOD: usize = 24;
const ATR_ENTRY_MULT: f64 = 0.00;
const VOL_LOOKBACK: usize = 1;
const TURTLE_ATR_MULT: f64 = 2.00;

const OUT_DIR: &str = "snapshots/ep_cp_sweep";
const SYMBOLS: [&str; 5] = ["BTCUSDT", "ETHUSDT", "SOLUSDT", "XRPUSDT", "ADAUSDT"];

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
        let h = *high.get(i).unwrap_or(&0.0);
        let l = *low.get(i).unwrap_or(&0.0);
        let c0 = *close.get(i.saturating_sub(1)).unwrap_or(&0.0);
        trs.push((h - l).max((h - c0).abs()).max((l - c0).abs()));
    }
    if trs.is_empty() { return 0.0; }
    trs.iter().sum::<f64>() / period as f64
}

fn rolling_avg(vals: &[f64], window: usize, idx: usize) -> f64 {
    if idx < window { return *vals.get(idx).unwrap_or(&0.0); }
    let start = idx + 1 - window;
    vals[start..=idx].iter().sum::<f64>() / window as f64
}

fn turtle_signal(
    close: &[f64], high: &[f64], low: &[f64],
    entry_period: usize, atr_period: usize, atr_mult: f64,
    idx: usize,
) -> bool {
    if idx < entry_period + 1 { return false; }
    let start = idx + 1 - entry_period;
    let mut max_close = f64::NEG_INFINITY;
    for i in start..idx {
        if let Some(&c) = close.get(i) { max_close = max_close.max(c); }
    }
    if let Some(&curr_close) = close.get(idx) {
        let breakout = curr_close > max_close;
        if breakout && atr_mult > 0.0 {
            let atr_val = atr_at(high, low, close, atr_period, idx);
            return curr_close >= max_close + atr_mult * atr_val;
        }
        breakout
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
    equity: Vec<f64>,
}

fn run_sim(
    sym_data: &HashMap<String, SymData>,
    symbols: &[String],
    test_start: usize,
    test_end: usize,
    ep: usize,
    cp: usize,
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
                let rol_vol = rolling_avg(&sd.vol, VOL_LOOKBACK, bar);
                let price = *sd.close.get(bar).unwrap_or(&0.0);
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
                if bar >= ep + 1 && bar < sd.close.len() {
                    if turtle_signal(&sd.close, &sd.high, &sd.low, ep, TURTLE_ATR_PERIOD, ATR_ENTRY_MULT, bar) {
                        let entry_px = sd.close[bar];
                        let entry = entry_px * (1.0 - TAKER_FEE);
                        let entry_bar_next = bar + 1;
                        let n = sd.close.len();

                        let mut highest_high_chand = sd.high[entry_bar_next];
                        let mut lowest_low_turtle = sd.low[entry_bar_next];
                        let max_bar = (entry_bar_next + HOLD_MAX).min(n.saturating_sub(1));
                        let mut exit_bar = max_bar;

                        for b in entry_bar_next..=max_bar.min(n.saturating_sub(1)) {
                            highest_high_chand = highest_high_chand.max(sd.high[b]);
                            let atr_chand = atr_at(&sd.high, &sd.low, &sd.close, cp, b);
                            let trail_chand = highest_high_chand - CHAND_MULT * atr_chand;

                            lowest_low_turtle = lowest_low_turtle.min(sd.low[b]);
                            let atr_turtle = atr_at(&sd.high, &sd.low, &sd.close, TURTLE_ATR_PERIOD, b);
                            let trail_turtle = lowest_low_turtle - TURTLE_ATR_MULT * atr_turtle;

                            if sd.close[b] < trail_chand || sd.close[b] < trail_turtle {
                                exit_bar = b;
                                break;
                            }
                        }

                        if let Some(&exit_px) = sd.close.get(exit_bar) {
                            let exit = exit_px * (1.0 - TAKER_FEE);
                            let gross_ret = exit / entry - 1.0;
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

    WfResult { ret, sharpe, max_dd, trades: total_trades, win_rate, pass, equity: equity_curve }
}

#[tokio::main]
async fn main() -> Result<()> {
    let t0 = Instant::now();

    std::fs::create_dir_all(OUT_DIR)?;

    // Open CSV files
    let mut sum_file = File::create(format!("{}/summary.csv", OUT_DIR))?;
    writeln!(sum_file, "ep,cp,avg_sharpe,avg_ret_pct,avg_max_dd_pct,total_trades,pass_count,total_windows")?;

    let mut det_file = File::create(format!("{}/detail.csv", OUT_DIR))?;
    writeln!(det_file, "ep,cp,window,ret_pct,sharpe,max_dd_pct,trades,win_rate_pct,pass")?;

    // Load data
    eprintln!("Loading data...");
    let loader = DataLoader::new(None, None);
    let mut raw_cache: HashMap<String, DataFrame> = HashMap::new();
    let mut min_len = usize::MAX;

    for sym in SYMBOLS {
        match loader.fetch_with_cache(sym, "1d", CANDLES).await {
            Ok(df) => {
                let n = df.height();
                eprintln!("  {}: {} rows", sym, n);
                min_len = min_len.min(n);
                raw_cache.insert(sym.to_string(), df);
            }
            Err(e) => { eprintln!("  {}: MISSING ({})", sym, e); }
        }
    }

    if raw_cache.len() != SYMBOLS.len() {
        eprintln!("Not all symbols loaded. Aborting.");
        return Ok(());
    }

    let n = min_len;
    eprintln!("Common bars: {}", n);

    let mut sym_data_map: HashMap<String, SymData> = HashMap::new();
    for sym in SYMBOLS {
        if let Some(df) = raw_cache.get(sym) {
            macro_rules! col_vec {
                ($name:expr) => {{
                    let chunked = df.column($name)?.f64()?;
                    chunked.into_iter().filter_map(|x| x).take(n).collect::<Vec<_>>()
                }};
            }
            sym_data_map.insert(sym.to_string(), SymData {
                close: col_vec!("close"),
                high:  col_vec!("high"),
                low:   col_vec!("low"),
                vol:   col_vec!("volume"),
            });
        }
    }

    let total_windows = n.saturating_sub(TRAIN_BARS + TEST_BARS) / TEST_BARS;
    eprintln!("Walk-forward windows: {}", total_windows);

    // Parameter grid
    let ep_range: Vec<usize> = (15..=30).collect();
    let cp_range: Vec<usize> = (5..=20).collect();
    let total_combos = ep_range.len() * cp_range.len();
    eprintln!("Sweeping EP{}..{} ({}) x CP{}..{} ({}) = {} combos",
        ep_range[0], ep_range.last().unwrap(), ep_range.len(),
        cp_range[0], cp_range.last().unwrap(), cp_range.len(),
        total_combos);

    let symbols: Vec<String> = SYMBOLS.iter().map(|s| s.to_string()).collect();
    let mut all_results: Vec<(usize, usize, f64, f64, f64, usize, usize, usize)> = Vec::new();

    for ep in &ep_range {
        for cp in &cp_range {
            print!("\r  EP={:2} CP={:2}", ep, cp);

            let mut ep_sum_s = 0.0_f64;
            let mut ep_sum_r = 0.0_f64;
            let mut ep_sum_d = 0.0_f64;
            let mut ep_trades = 0usize;
            let mut ep_pass = 0usize;

            // Open equity curve file
            let eq_path = format!("{}/EP{:02}_CP{:02}.eq.csv", OUT_DIR, ep, cp);
            let mut eq_file = File::create(&eq_path)?;

            for wi in 0..total_windows {
                let train_end = TRAIN_BARS + wi * TEST_BARS;
                let test_start = train_end;
                let test_end = (test_start + TEST_BARS).min(n);

                if test_end.saturating_sub(test_start) < 5 { continue; }

                let r = run_sim(&sym_data_map, &symbols, test_start, test_end, *ep, *cp);

                let pass_i: usize = if r.pass { 1 } else { 0 };
                ep_sum_s += r.sharpe;
                ep_sum_r += r.ret;
                ep_sum_d += r.max_dd;
                ep_trades += r.trades;
                ep_pass += pass_i;

                writeln!(det_file, "{},{},{},{:.4},{:.4},{:.2},{},{:.2},{}",
                    ep, cp, wi, r.ret, r.sharpe, r.max_dd, r.trades, r.win_rate, pass_i)?;

                // Write equity curve: bar_idx,equity
                for (bar_idx, &eq_val) in r.equity.iter().enumerate() {
                    writeln!(eq_file, "{},{}", wi, eq_val)?;
                }
            }

            let denom = total_windows as f64;
            let avg_s = ep_sum_s / denom;
            let avg_r = ep_sum_r / denom;
            let avg_d = ep_sum_d / denom;

            writeln!(sum_file, "{},{},{:.4},{:.4},{:.2},{},{},{}",
                ep, cp, avg_s, avg_r, avg_d, ep_trades, ep_pass, total_windows)?;

            all_results.push((*ep, *cp, avg_s, avg_r, avg_d, ep_trades, ep_pass, total_windows));
        }
    }
    println!();

    // Sort by Sharpe
    all_results.sort_by(|a, b| b.2.partial_cmp(&a.2).unwrap());

    let baseline_sharpe = all_results
        .iter()
        .find(|(e, c, _, _, _, _, _, _)| *e == 21 && *c == 11)
        .map(|r| r.2)
        .unwrap_or(0.0_f64);

    eprintln!("\n=== TOP 10 (by avg Sharpe across {} windows) ===", total_windows);
    for (rank, r) in all_results.iter().enumerate().take(10) {
        let (ep, cp, sh, ret, dd, _, passes, total) = *r;
        let pr = passes as f64 / total as f64 * 100.0_f64;
        let delta = if baseline_sharpe > 0.0_f64 { (sh - baseline_sharpe) / baseline_sharpe * 100.0_f64 } else { 0.0_f64 };
        eprintln!("  #{:2}: EP={:2} CP={:2}  Sharpe={:.4} ({:+.1}%)  Ret={:+.1}%  DD={:.1}%  Pass={}/{} ({:.0}%)",
            rank + 1, ep, cp, sh, delta, ret, dd, passes, total, pr);
    }

    eprintln!("\n  Baseline EP=21 CP=11: Sharpe={:.4}", baseline_sharpe);
    if let Some(w) = all_results.first() {
        let delta = if baseline_sharpe > 0.0_f64 { (w.2 - baseline_sharpe) / baseline_sharpe * 100.0_f64 } else { 0.0_f64 };
        eprintln!("  Winner  EP={:2} CP={:2}: Sharpe={:.4} ({:+.1}%)", w.0, w.1, w.2, delta);
    }

    // Write Python chart script (uses existing charts/plot_ep_cp_surface.py)
    let top5: Vec<(usize, usize)> = all_results.iter().take(5).map(|r| (r.0, r.1)).collect();
    let top5_json = format!("{:?}", top5);
    let py_path = "charts/plot_ep_cp_surface.py";
    eprintln!("Chart script: {}", py_path);

    // Write results markdown
    let md_path = format!("{}/results.md", OUT_DIR);
    let mut md = File::create(&md_path)?;
    writeln!(md, "# EP x CP 2D Surface Sweep Results")?;
    writeln!(md, "")?;
    writeln!(md, "Tested: EP in [15..30 step 1] x CP in [5..20 step 1] = {} combos", total_combos)?;
    writeln!(md, "Walk-forward: Base5 (BTC,ETH,SOL,XRP,ADA), {} windows", total_windows)?;
    writeln!(md, "")?;
    writeln!(md, "## Top 10 (by avg Sharpe)")?;
    writeln!(md, "| Rank | EP | CP | Sharpe | vs Baseline | Ret% | DD% | Pass |")?;
    writeln!(md, "|------|----|----|--------|-------------|------|-----|------|")?;
    for (rank, r) in all_results.iter().enumerate().take(10) {
        let (ep, cp, sh, ret, dd, _, passes, total) = *r;
        let delta = if baseline_sharpe > 0.0_f64 { (sh - baseline_sharpe) / baseline_sharpe * 100.0_f64 } else { 0.0_f64 };
        let pr = passes as f64 / total as f64 * 100.0_f64;
        writeln!(md, "| {} | {} | {} | {:.4} | {:+.1}% | {:+.1}% | {:.1}% | {}/{} ({:.0}%) |",
            rank + 1, ep, cp, sh, delta, ret, dd, passes, total, pr)?;
    }
    writeln!(md, "")?;
    writeln!(md, "Baseline (EP=21, CP=11): Sharpe={:.4}", baseline_sharpe)?;
    writeln!(md, "")?;
    writeln!(md, "Charts: {}/comparison_chart.png", OUT_DIR)?;
    writeln!(md, "Charts: {}/heatmap_sharpe.png", OUT_DIR)?;
    drop(md);

    eprintln!("\nTotal time: {:?}", t0.elapsed());

    // Run chart script
    eprintln!("\nGenerating charts...");
    let result = std::process::Command::new("python3")
        .arg(&py_path)
        .arg(&top5_json)
        .current_dir(".")
        .output();

    if let Ok(out) = result {
        if !out.stdout.is_empty() { println!("{}", String::from_utf8_lossy(&out.stdout)); }
        if !out.stderr.is_empty() { eprintln!("Py stderr: {}", String::from_utf8_lossy(&out.stderr)); }
    }

    Ok(())
}
