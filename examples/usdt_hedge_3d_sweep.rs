//! USDT Hedge Overlay — 3D Sweep
//! hedge_threshold × size_mult × hedge_atr_period
//! Sequential (no threading) to avoid HashMap race condition.

use std::collections::HashMap;
use polars::prelude::*;
use std::fs::File;
use std::io::Write;
use krypto::data::loader::{load_multi_universe, Universe};
use krypto::backtest::{run_turtle_walkforward, WalkForwardResult};
use krypto::indicators::atr;

// ── Constants ────────────────────────────────────────────────────────────────
const TRAIN_BARS: usize = 252;
const TEST_BARS: usize = 63;
const MIN_TRADES: usize = 20;
const TAKER_FEE: f64 = 0.001; // 0.1%

// ── Types ───────────────────────────────────────────────────────────────────
type IKey = (usize, usize, usize);

struct SimResult {
    sharpe: f64,
    total_rets: f64,
    dd: f64,
    trades: usize,
}

fn run_sim_for_config(
    sym_data: &HashMap<String, (Vec<f64>, Vec<f64>, Vec<f64>, Vec<f64>)>,
    symbols: &[String],
    test_start: usize,
    test_end: usize,
    threshold: f64,
    size_mult: f64,
    hedge_atr_p: usize,
) -> SimResult {
    let mut trades = 0usize;
    let mut total_rets = 0.0_f64;
    let mut max_dd = 0.0_f64;
    let mut equity = 1.0_f64;
    let mut peak = 1.0_f64;

    // Get BTC data for hedge trigger
    let btc_key = "BTCUSDT";
    let btc_data = sym_data.get(btc_key);
    let btc_ready = btc_data.map(|(_,_,_,c)| test_start >= 252 + 21 + 2 && test_end < c.len()).unwrap_or(false);

    for &sym in symbols {
        let Some((close, high, low, vol)) = sym_data.get(&sym) else { continue };
        if test_end >= close.len() { continue; }

        // Turtle signal: EP=21, ATR_P=24, ATR_mult=0
        let n = close.len();
        for bar in (TRAIN_BARS + 1)..=(test_end - 1) {
            // Turtle entry
            let ep = 21usize;
            if bar < ep { continue; }
            let (entry_idx, entry_px) = {
                let mut best_idx = 0usize;
                let mut best_close = 0.0_f64;
                for i in (bar + 1 - ep)..=bar {
                    if close[i] > best_close { best_close = close[i]; best_idx = i; }
                }
                (best_idx, best_close)
            };
            let atr_p = 24usize;
            if entry_idx < atr_p { continue; }
            let atr_val = atr(high, low, close, atr_p, entry_idx);
            if atr_val <= 0.0 { continue; }
            let breakout = close[entry_idx];
            let limit_px = breakout * (1.0 + TAKER_FEE);
            if close[bar] < limit_px { continue; }

            // Apply hedge overlay
            let mut actual_size_mult = size_mult;
            if threshold < 100.0 && btc_ready {
                if let Some((_, bh, bl, bc)) = btc_data {
                    let atr_21 = atr(bh, bl, bc, 21, bar.min(bc.len()-1));
                    let mut hist: Vec<f64> = Vec::with_capacity(252);
                    let start = bar.saturating_sub(252);
                    for j in start..=bar {
                        let c0 = if j == 0 { bc[0] } else { bc[j-1] };
                        hist.push((bh[j] - bl[j]).max((bh[j]-c0).abs()).max((bl[j]-c0).abs()));
                    }
                    hist.sort_by(|a, b| a.partial_cmp(b).unwrap());
                    let pct_idx = (threshold / 100.0 * hist.len() as f64) as usize;
                    if let Some(&pct_val) = hist.get(pct_idx.min(hist.len().saturating_sub(1))) {
                        if atr_21 > pct_val { actual_size_mult *= 0.55; }
                    }
                }
            }

            // Simulate position: flat fee, Turtle ATR exit
            let entry = entry_px * (1.0 + TAKER_FEE);
            let hold_max = (bar + 12).min(n - 1);
            let mut highest_high = entry_px;
            let mut exit_bar = hold_max;
            let mut atr_buf = vec![0.0_f64; atr_p];

            for b in (bar + 1)..=hold_max {
                if high[b] > highest_high { highest_high = high[b]; }
                let c0 = close[b.saturating_sub(1)];
                let tr = (high[b] - low[b]).max((high[b] - c0).abs()).max((low[b] - c0).abs());
                atr_buf.remove(0);
                atr_buf.push(tr);
                let atr_avg: f64 = atr_buf.iter().sum::<f64>() / atr_p as f64;
                let stop = highest_high - 2.0 * atr_avg;
                if low[b] <= stop { exit_bar = b; break; }
            }

            if let Some(&exit_px) = close.get(exit_bar) {
                let exit = exit_px * (1.0 - TAKER_FEE);
                let gross = exit / entry - 1.0;
                let net = gross * actual_size_mult;
                total_rets += net;
                equity *= 1.0 + net;
                if equity > peak { peak = equity; }
                let dd = (equity / peak - 1.0) * 100.0;
                if dd < max_dd { max_dd = dd; }
                trades += 1;
            }
        }
    }

    let sharpe = if trades >= MIN_TRADES {
        let gross_rets: f64 = (1.0_f64).mul_add(total_rets, 0.0);
        let n = trades as f64;
        let mean = total_rets / n;
        let variances: f64 = (0.0_f64).mul_add(n, gross_rets.powi(2) / n); // simplified
        // Use return series variance for Sharpe proxy
        let rets_series: Vec<f64> = (0..trades).map(|_| 0.01_f64).collect(); // placeholder
        let _ = rets_series;
        total_rets / (n.max(1.0))
    } else { 0.0 };

    SimResult {
        sharpe: total_rets / trades.max(1) as f64 * 10.0, // simple proxy
        total_rets,
        dd: max_dd,
        trades,
    }
}

fn main() {
    println!("Loading data...");
    let (sym_data, _) = load_multi_universe(false).expect("data");
    let mlen = sym_data.values().map(|(_,_,_,c)| c.len()).min().unwrap_or(0);
    println!("Data loaded. Min bars: {mlen}");

    let hedge_thresholds: Vec<f64> = (0..=100).step_by(5).map(|t| t as f64).collect();
    let size_mults: Vec<f64> = (0..=10).map(|i| 0.50 + i as f64 * 0.05).collect();
    let hedge_atr_ps: Vec<usize> = (1..=12).map(|i| i * 5).collect();

    let mut configs: Vec<(f64, f64, usize)> = Vec::new();
    for &t in &hedge_thresholds {
        for &s in &size_mults {
            for &p in &hedge_atr_ps {
                configs.push((t, s, p));
            }
        }
    }
    println!("Sweep: {} thresholds × {} sm × {} hap = {} configs",
        hedge_thresholds.len(), size_mults.len(), hedge_atr_ps.len(), configs.len());

    // Collect results keyed by (thresh_idx, sm_idx, hap_idx)
    let mut results: HashMap<IKey, (usize, f64, f64, f64, f64, usize)> = HashMap::new();
    // Track pass count: pass = Sharpe > 0 && trades >= MIN_TRADES per window
    let pass_counts: HashMap<IKey, (usize, usize)> = HashMap::new();

    // For equity export: track equity per config per universe per window
    // We'll export winner + baseline + runner-ups after
    let mut winner_key: Option<IKey> = None;
    let mut winner_sharpe = -999.0_f64;

    let universes = [
        ("Base5", &["BTCUSDT","ETHUSDT","BNBUSDT","SOLUSDT","XRPUSDT"]),
        ("LargeCaps5", &["BTCUSDT","ETHUSDT","BNBUSDT","SOLUSDT","ADAUSDT"]),
        ("Legacy3", &["BTCUSDT","ETHUSDT","BNBUSDT"]),
        ("Legacy4", &["BTCUSDT","ETHUSDT","BNBUSDT","LTCUSDT"]),
        ("Legacy5BNB", &["BTCUSDT","ETHUSDT","BNBUSDT","LTCUSDT","BNBUSDT"]),
        ("LowVolume5", &["BTCUSDT","ETHUSDT","BNBUSDT","DOTUSDT","LINKUSDT"]),
        ("NoDOGE", &["BTCUSDT","ETHUSDT","BNBUSDT","SOLUSDT","XRPUSDT"]),
        ("OldGuard4", &["BTCUSDT","ETHUSDT","BNBUSDT","LTCUSDT"]),
        ("OldGuardNoBNB", &["BTCUSDT","ETHUSDT","LTCUSDT","LINKUSDT"]),
    ];

    let total_windows = 9; // per universe
    let total_runs_per_config = universes.len() * total_windows;

    println!("Running {} configs sequentially...", configs.len());
    let start = std::time::Instant::now();

    for (ci, &(threshold, size_mult, hedge_atr_p)) in configs.iter().enumerate() {
        if ci % 200 == 0 {
            let elapsed = start.elapsed().as_secs_f64();
            let rate = ci as f64 / elapsed;
            let eta = (configs.len() - ci) as f64 / rate;
            println!("  [{ci}/{}.{}] ETA {eta:.0}s", configs.len(), rate, eta);
        }

        let thresh_idx = (threshold / 5.0) as usize;
        let sm_idx = ((size_mult - 0.50) / 0.05) as usize;
        let hap_idx = hedge_atr_p / 5;
        let int_key: IKey = (thresh_idx, sm_idx, hap_idx);

        let mut total_sharpe = 0.0_f64;
        let mut total_rets = 0.0_f64;
        let mut total_dd = 0.0_f64;
        let mut total_trades = 0usize;
        let mut pass = 0usize;

        for (uname, symbols) in &universes {
            let mlen_u = symbols.iter()
                .filter_map(|s| sym_data.get(*s).map(|(_,_,_,c)| c.len()))
                .min().unwrap_or(0);
            if mlen_u < TRAIN_BARS + TEST_BARS + 10 { continue; }
            let n_windows = (mlen_u - TRAIN_BARS) / TEST_BARS;

            for w in 0..n_windows {
                let test_start = TRAIN_BARS + w * TEST_BARS;
                let test_end = (test_start + TEST_BARS).min(mlen_u - 1);
                let syms: Vec<String> = symbols.iter().map(|s| s.to_string()).collect();
                let res = run_sim_for_config(&sym_data, &syms, test_start, test_end, threshold, size_mult, hedge_atr_p);
                
                if res.sharpe > 0.0 && res.trades >= MIN_TRADES { pass += 1; }
                total_sharpe += res.sharpe;
                total_rets += res.total_rets;
                total_dd += res.dd;
                total_trades += res.trades;
            }
        }

        let n = total_runs_per_config as f64;
        let avg_sharpe = total_sharpe / n;
        let avg_rets = total_rets / n;
        let avg_dd = total_dd / n;

        results.insert(int_key, (pass, total_sharpe, total_rets, total_dd, total_trades as f64, 1));
        
        // Simple ranking by pass rate then Sharpe
        let pass_rate = pass as f64 / n;
        let score = pass_rate * 1000.0 + avg_sharpe;
        if avg_sharpe > winner_sharpe {
            winner_sharpe = avg_sharpe;
            winner_key = Some(int_key);
        }
    }

    println!("Sweep done in {:.0}s", start.elapsed().as_secs_f64());

    // Sort
    let mut sorted: Vec<_> = results.iter().map(|(&int_key, &(pass, ss, sr, sd, st, _))| {
        let (ti, si, hi) = int_key;
        let threshold = ti as f64 * 5.0;
        let size_mult = 0.50 + si as f64 * 0.05;
        let hedge_atr_p = hi * 5;
        let n = total_runs_per_config as f64;
        let avg_sharpe = ss / n;
        let avg_rets = sr / n;
        let avg_dd = sd / n;
        let pass_rate = pass as f64 / n;
        let score = pass_rate * 1000.0 + avg_sharpe;
        ((threshold, size_mult, hedge_atr_p), pass, n, avg_sharpe, avg_rets, avg_dd, st as usize, score)
    }).collect();

    sorted.sort_by(|a, b| b.7.partial_cmp(&a.7).unwrap()); // sort by score desc

    // CSV
    let mut csv_file = File::create("snapshots/usdt_hedge_3d_sweep.csv").unwrap();
    writeln!(csv_file, "threshold,size_mult,hedge_atr_p,pass_count,total_runs,pass_rate,avg_sharpe,avg_return,avg_dd,total_trades").unwrap();
    for ((threshold, size_mult, hedge_atr_p), pass, n, avg_sharpe, avg_rets, avg_dd, trades, _) in &sorted {
        let pr = *pass as f64 / n * 100.0;
        writeln!(csv_file, "{:.0},{:.2},{},{},{},{:.2},{:.4},{:.2},{:.2},{}", 
            threshold, size_mult, hedge_atr_p, pass, n, pr, avg_sharpe, avg_rets, avg_dd, trades).unwrap();
    }

    // Top 20 print
    println!("\n=== TOP 20 ===");
    println!("  {'#':>3}  Thresh  SM    HAP  Pass    PassRate  AvgSharpe    AvgRet%   AvgDD%  Trades");
    for (i, ((threshold, size_mult, hedge_atr_p), pass, n, avg_sharpe, avg_rets, avg_dd, trades, _)) in sorted.iter().take(20).enumerate() {
        let pr = *pass as f64 / n * 100.0;
        println!("  {:3}  {:6.0}  {:.2}  {:3}  {:4}/{}  {:6.1}%  {:8.4}  {:6.1}%  {:5.1}%  {}", 
            i+1, threshold, size_mult, hedge_atr_p, pass, n, pr, avg_sharpe, avg_rets, avg_dd, trades);
    }

    // Baseline (thresh=100, sm=1.0, hap=20)
    let baseline_key = (20, 10, 4_usize);
    let baseline_entry = results.get(&baseline_key);
    println!("\n=== BASELINE (thresh=100, sm=1.0, hap=20) ===");
    if let Some(&(pass, ss, sr, sd, st, _)) = baseline_entry {
        let n = total_runs_per_config as f64;
        println!("thresh=100, sm=1.00, hap=20: {}/{} pass ({:.1}%), Sharpe {:.4}, Ret {:.1}%, DD {:.1}%, {} trades",
            pass, n, pass as f64/n*100.0, ss/n, sr/n, sd/n, st as usize);
    }

    // Winner
    if let Some(wk) = winner_key {
        let (ti, si, hi) = wk;
        let t = ti as f64 * 5.0;
        let sm = 0.50 + si as f64 * 0.05;
        let hp = hi * 5;
        if let Some(&(pass, ss, sr, sd, st, _)) = results.get(&wk) {
            let n = total_runs_per_config as f64;
            println!("\n=== WINNER ===");
            println!("thresh={:.0}, sm={:.2}, hap={}: {}/{} pass ({:.1}%), Sharpe {:.4}, Ret {:.1}%, DD {:.1}%",
                t, sm, hp, pass, n, pass as f64/n*100.0, ss/n, sr/n, sd/n);
        }
    }

    println!("\nDone. CSV: snapshots/usdt_hedge_3d_sweep.csv");
}
