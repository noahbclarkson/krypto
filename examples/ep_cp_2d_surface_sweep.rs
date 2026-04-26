// EP x CP 2D Surface Sweep - Validate EP=21/CP=7 Pairing
//
// Tests EP in [15..30 step 1] x CP in [5..20 step 1]
// Walk-forward: Base5 (BTC,ETH,SOL,XRP,ADA), 54 windows, 504-bar train / 504-bar test
//
// Key gap: CP=7 was found while EP=24 was active. EP=24 is REVERTED.
// The current production pairing EP=21/CP=7 has never been validated.

use std::collections::BTreeMap;
use std::fs::File;
use std::io::Write;
use std::time::Instant;
use std::path::Path;
use polars::prelude::*;

const TRAIN_BARS: usize = 504;
const TEST_BARS: usize = 504;
const OUT_DIR: &str = "snapshots/ep_cp_surface";

const SYMBOLS: [&str; 5] = ["BTCUSDT", "ETHUSDT", "SOLUSDT", "XRPUSDT", "ADAUSDT"];

// Strategy

struct TradeState {
    highest_high: f64,
    lowest_low: f64,
    bars_held: usize,
    atr_buf: Vec<f64>,
}

fn tr(high: f64, low: f64, close: f64, prev_close: f64) -> f64 {
    (high - low).max((high - prev_close).abs()).max((low - prev_close).abs())
}

fn run_turtle(
    close: &[f64],
    high: &[f64],
    low: &[f64],
    test_start: usize,
    test_end: usize,
    ep: usize,
    cp: usize,
    cm: f64,
    am: f64,
    hold_max: usize,
) -> (bool, f64, f64, f64, usize, f64, Vec<f64>) {
    let mut state: Option<TradeState> = None;
    let mut eq: f64 = 10000.0;
    let mut trades: usize = 0;
    let mut wins: usize = 0;
    let mut equity_curve: Vec<f64> = Vec::with_capacity(test_end - test_start);

    for i in test_start..test_end {
        if state.is_none() {
            if i >= ep {
                let mut mx: f64 = close[i - ep];
                for j in (i - ep)..i {
                    if close[j] > mx { mx = close[j]; }
                }
                if close[i] >= mx {
                    let avail: usize = ep.min(cp);
                    let start: usize = if i > avail { i - avail } else { 0 };
                    let mut atr_buf: Vec<f64> = Vec::with_capacity(cp);
                    for j in start..i {
                        let prev_c: f64 = if j > 0 { close[j - 1] } else { close[j] };
                        atr_buf.push(tr(high[j], low[j], close[j], prev_c));
                    }
                    state = Some(TradeState {
                        highest_high: high[i],
                        lowest_low: low[i],
                        bars_held: 0,
                        atr_buf,
                    });
                }
            }
            equity_curve.push(eq);
        } else {
            let s = state.as_mut().unwrap();

            if high[i] > s.highest_high { s.highest_high = high[i]; }
            if low[i] < s.lowest_low { s.lowest_low = low[i]; }
            s.bars_held += 1;

            if i > 0 {
                s.atr_buf.push(tr(high[i], low[i], close[i], close[i - 1]));
            }
            if s.atr_buf.len() > cp { s.atr_buf.remove(0); }

            let atr: f64 = if s.atr_buf.len() >= cp {
                s.atr_buf.iter().sum::<f64>() / cp as f64
            } else {
                equity_curve.push(eq);
                continue;
            };
            if atr <= 0.0 {
                equity_curve.push(eq);
                continue;
            }

            let chand_stop: f64 = s.highest_high - cm * atr;
            let turtle_stop: f64 = s.lowest_low - am * atr;
            let stop: f64 = chand_stop.max(turtle_stop);

            if low[i] <= stop || s.bars_held >= hold_max {
                let ret: f64 = (stop - close[i]) / close[i];
                eq = eq * (1.0 + ret);
                trades += 1;
                if ret > 0.0 { wins += 1; }
                state = None;
            }
            equity_curve.push(eq);
        }
    }

    let ret_pct: f64 = (eq / 10000.0 - 1.0) * 100.0;
    let pass: bool = eq > 10000.0 && trades >= 3;

    let mut daily_rets: Vec<f64> = Vec::new();
    for i in 1..equity_curve.len() {
        if equity_curve[i - 1] > 0.0 {
            daily_rets.push((equity_curve[i] - equity_curve[i - 1]) / equity_curve[i - 1]);
        }
    }
    let mean_r: f64 = if daily_rets.is_empty() {
        0.0
    } else {
        daily_rets.iter().sum::<f64>() / daily_rets.len() as f64
    };
    let var: f64 = if daily_rets.is_empty() {
        0.0
    } else {
        daily_rets.iter().map(|r| (r - mean_r).powi(2)).sum::<f64>() / daily_rets.len() as f64
    };
    let std_r: f64 = var.sqrt();
    let sharpe: f64 = if std_r > 0.0 { mean_r / std_r * 15.874507866 } else { 0.0 };

    let peak: f64 = equity_curve.iter().fold(0.0, |a, &b| a.max(b));
    let max_dd: f64 = if peak > 0.0 {
        equity_curve.iter().map(|&e| (peak - e) / peak * 100.0).fold(0.0, |a, b| a.max(b))
    } else {
        0.0
    };

    let win_rate: f64 = if trades > 0 { wins as f64 / trades as f64 } else { 0.0 };

    (pass, ret_pct, sharpe, max_dd, trades, win_rate, equity_curve)
}

// Data Loading

fn load_symbol(sym: &str) -> Option<(Vec<f64>, Vec<f64>, Vec<f64>)> {
    let path = Path::new("data/cache").join(format!("{}_1d.parquet", sym.to_lowercase()));
    let file = std::fs::File::open(&path).ok()?;
    let df = ParquetReader::new(file).finish().ok()?;
    let close: Vec<f64> = df.column("close").ok()?.f64().ok()?.into_no_null_iter().collect();
    let high: Vec<f64> = df.column("high").ok()?.f64().ok()?.into_no_null_iter().collect();
    let low: Vec<f64> = df.column("low").ok()?.f64().ok()?.into_no_null_iter().collect();
    Some((close, high, low))
}

// CSV helpers - use format! then write! for decimal places

fn write_csv_det(
    f: &mut File,
    ep: usize,
    cp: usize,
    wi: usize,
    avg_ret: f64,
    avg_sh: f64,
    avg_dd: f64,
    sym_trades: usize,
    pass_i: usize,
) -> std::io::Result<()> {
    write!(f, "{}", ep)?;
    write!(f, ",{}", cp)?;
    write!(f, ",{}", wi)?;
    write!(f, ",{}", format!("{:.4}", avg_ret))?;
    write!(f, ",{}", format!("{:.4}", avg_sh))?;
    write!(f, ",{}", format!("{:.2}", avg_dd))?;
    write!(f, ",{}", sym_trades)?;
    write!(f, ",{}", format!("{:.4}", 0.0_f64))?;
    write!(f, ",{}", pass_i)?;
    write!(f, "\n")?;
    Ok(())
}

fn write_csv_sum(
    f: &mut File,
    ep: usize,
    cp: usize,
    avg_s: f64,
    avg_r: f64,
    avg_d: f64,
    ep_trades: usize,
    ep_pass: usize,
    total_windows: usize,
) -> std::io::Result<()> {
    write!(f, "{}", ep)?;
    write!(f, ",{}", cp)?;
    write!(f, ",{}", format!("{:.4}", avg_s))?;
    write!(f, ",{}", format!("{:.4}", avg_r))?;
    write!(f, ",{}", format!("{:.2}", avg_d))?;
    write!(f, ",{}", ep_trades)?;
    write!(f, ",{}", ep_pass)?;
    write!(f, ",{}", total_windows)?;
    write!(f, "\n")?;
    Ok(())
}

// Main

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let t0 = Instant::now();

    std::fs::create_dir_all(OUT_DIR)?;

    let det_path = format!("{}/detail.csv", OUT_DIR);
    let mut det_file = File::create(&det_path)?;
    write!(det_file, "ep,cp,window,ret_pct,sharpe,max_dd_pct,trades,win_rate,pass\n")?;

    let sum_path = format!("{}/summary.csv", OUT_DIR);
    let mut sum_file = File::create(&sum_path)?;
    write!(sum_file, "ep,cp,avg_sharpe,avg_ret_pct,avg_max_dd_pct,total_trades,pass_count,total_windows\n")?;

    println!("Loading data...");
    let mut all_data: BTreeMap<String, (Vec<f64>, Vec<f64>, Vec<f64>)> = BTreeMap::new();
    let mut min_len: usize = usize::MAX;

    for &sym in &SYMBOLS {
        if let Some((close, high, low)) = load_symbol(sym) {
            let n: usize = close.len().min(high.len()).min(low.len());
            println!("  {}: {} bars", sym, n);
            all_data.insert(sym.to_string(), (close, high, low));
            if n < min_len { min_len = n; }
        } else {
            println!("  {}: MISSING", sym);
        }
    }

    if all_data.is_empty() {
        eprintln!("No data. Aborting.");
        return Ok(());
    }
    println!("Common window: {} bars", min_len);

    let total_windows: usize = (min_len.saturating_sub(TRAIN_BARS) / TEST_BARS).min(54);
    println!("Walk-forward windows: {}", total_windows);

    let ep_range: Vec<usize> = (15..=30).collect();
    let cp_range: Vec<usize> = (5..=20).collect();
    let total_combos: usize = ep_range.len() * cp_range.len();
    println!("Sweeping EP{}..{} x CP{}..{} = {} combos",
        ep_range[0], ep_range.last().unwrap(),
        cp_range[0], cp_range.last().unwrap(),
        total_combos);

    let cm: f64 = 2.30;
    let am: f64 = 2.0;
    let hold_max: usize = 12;

    let mut all_results: Vec<(usize, usize, f64, f64, f64, usize, usize, usize)> = Vec::new();

    for ep in &ep_range {
        for cp in &cp_range {
            print!("\r  EP={:2} CP={:2}", ep, cp);
            std::io::stdout().flush().ok();

            let mut ep_sum_s: f64 = 0.0;
            let mut ep_sum_r: f64 = 0.0;
            let mut ep_sum_d: f64 = 0.0;
            let mut ep_trades: usize = 0;
            let mut ep_pass: usize = 0;

            for wi in 0..total_windows {
                let train_end: usize = TRAIN_BARS + wi * TEST_BARS;
                let test_start: usize = train_end;
                let test_end: usize = (test_start + TEST_BARS).min(min_len);

                if test_end.saturating_sub(test_start) < 20 { continue; }

                let mut n_sym: usize = 0;
                let mut sym_ret: f64 = 0.0;
                let mut sym_sharpe: f64 = 0.0;
                let mut sym_dd: f64 = 0.0;
                let mut sym_trades: usize = 0;
                let mut sym_pass: usize = 0;

                for &sym in &SYMBOLS {
                    if let Some((ref close, ref high, ref low)) = all_data.get(sym) {
                        if close.len() >= test_end {
                            let (pass, ret, sh, dd, trades, _, _) = run_turtle(
                                close, high, low,
                                test_start, test_end,
                                *ep, *cp, cm, am, hold_max,
                            );
                            sym_ret += ret;
                            sym_sharpe += sh;
                            sym_dd += dd;
                            sym_trades += trades;
                            if pass { sym_pass += 1; }
                            n_sym += 1;
                        }
                    }
                }

                if n_sym == 0 { continue; }

                let avg_ret: f64 = sym_ret / n_sym as f64;
                let avg_sh: f64 = sym_sharpe / n_sym as f64;
                let avg_dd: f64 = sym_dd / n_sym as f64;
                let pass_i: usize = if sym_pass >= 3 { 1 } else { 0 };

                ep_sum_s += avg_sh;
                ep_sum_r += avg_ret;
                ep_sum_d += avg_dd;
                ep_trades += sym_trades;
                ep_pass += pass_i;

                write_csv_det(&mut det_file, *ep, *cp, wi, avg_ret, avg_sh, avg_dd, sym_trades, pass_i)?;
            }

            let denom: f64 = total_windows as f64;
            let avg_s: f64 = ep_sum_s / denom;
            let avg_r: f64 = ep_sum_r / denom;
            let avg_d: f64 = ep_sum_d / denom;

            write_csv_sum(&mut sum_file, *ep, *cp, avg_s, avg_r, avg_d, ep_trades, ep_pass, total_windows)?;
            all_results.push((*ep, *cp, avg_s, avg_r, avg_d, ep_trades, ep_pass, total_windows));
        }
    }
    println!();

    all_results.sort_by(|a, b| b.2.partial_cmp(&a.2).unwrap());

    let baseline_sharpe: f64 = all_results
        .iter()
        .find(|(e, c, _, _, _, _, _, _)| *e == 21 && *c == 11)
        .map(|r| r.2)
        .unwrap_or(0.0);

    println!("\n=== TOP 10 (by avg Sharpe across {} windows) ===", total_windows);
    for (rank, r) in all_results.iter().enumerate().take(10) {
        let (ep, cp, sh, ret, dd, _, passes, total) = *r;
        let pr: f64 = passes as f64 / total as f64 * 100.0;
        let delta_pct: f64 = if baseline_sharpe > 0.0 { (sh - baseline_sharpe) / baseline_sharpe * 100.0 } else { 0.0 };
        println!("  #{:2}: EP={:2} CP={:2}  Sharpe={} ({})  Ret={}  DD={}  Pass={}/{} ({:.0}%)",
            rank + 1, ep, cp,
            format!("{:.4}", sh),
            format!("{:+.1}", delta_pct),
            format!("{:+.1}", ret),
            format!("{:.1}", dd),
            passes, total, pr);
    }

    println!("\n  Baseline EP=21 CP=11: Sharpe={}", format!("{:.4}", baseline_sharpe));
    if let Some(w) = all_results.first() {
        let delta_pct: f64 = if baseline_sharpe > 0.0 { (w.2 - baseline_sharpe) / baseline_sharpe * 100.0 } else { 0.0 };
        println!("  Winner  EP={:2} CP={:2}: Sharpe={} ({})", w.0, w.1, format!("{:.4}", w.2), format!("{:+.1}", delta_pct));
    }

    println!("\nTotal time: {:.1}s", (t0.elapsed().as_secs_f32()));

    Ok(())
}
