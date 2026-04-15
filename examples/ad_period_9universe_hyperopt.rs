//! A/D Period Fine-Grained Hyperopt — 9 Universes × Chandelier(15, 2.0)
//!
//! Full sweep: AD_PERIOD ∈ [1, 100] step 1 = 100 values
//! Baseline: p=47 (current default)
//!
//! Validated Chandelier exit: P=15, M=2.0 (from ad_chandelier_hyperopt.rs)
//! Walk-forward: 252-bar train / 252-bar test
//! Fees: 0.1% taker each side

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
const TAKER_FEE: f64 = 0.001;
const CHAND_PERIOD: usize = 15;
const CHAND_MULT: f64 = 2.00;
const MIN_TRADES: usize = 3;
const POSITION_CAP: usize = 3;
const HOLD_MAX: usize = 45;
const TOP_K: usize = 2;

const UNIVERSES: &[(&str, &[&str])] = &[
    ("Base5",        &["BTCUSDT","ETHUSDT","SOLUSDT","XRPUSDT","DOGEUSDT","ADAUSDT"]),
    ("NoDOGE",       &["BTCUSDT","ETHUSDT","SOLUSDT","XRPUSDT","ADAUSDT"]),
    ("Legacy4",       &["BTCUSDT","ETHUSDT","XRPUSDT","LTCUSDT","EOSUSDT"]),
    ("Legacy5BNB",   &["BTCUSDT","ETHUSDT","XRPUSDT","LTCUSDT","BNBUSDT","EOSUSDT"]),
    ("OldGuardNoBNB",&["BTCUSDT","ETHUSDT","XRPUSDT","LTCUSDT","EOSUSDT","BCHUSDT"]),
    ("LargeCaps5",   &["BTCUSDT","ETHUSDT","SOLUSDT","XRPUSDT","BNBUSDT","ADAUSDT"]),
    ("Legacy3",       &["BTCUSDT","XRPUSDT","LTCUSDT","EOSUSDT"]),
    ("LowVolume5",   &["XRPUSDT","LTCUSDT","EOSUSDT","BCHUSDT","ADAUSDT"]),
    ("OldGuard4",    &["BTCUSDT","XRPUSDT","LTCUSDT","EOSUSDT","BCHUSDT"]),
];

const CSV_OUT: &str = "snapshots/ad_period_9universe_hyperopt.csv";
const EQUITY_OUT_DIR: &str = "snapshots/ad_period_equity_curves";

struct SymData {
    close: Vec<f64>,
    high: Vec<f64>,
    low: Vec<f64>,
    #[allow(dead_code)]
    vol: Vec<f64>,
    ad_line: Vec<f64>,
}

fn compute_ad(high: &[f64], low: &[f64], close: &[f64], vol: &[f64]) -> Vec<f64> {
    let n = close.len();
    let mut ad = vec![0.0; n];
    for i in 0..n {
        let h = high[i]; let l = low[i]; let c = close[i]; let v = vol[i];
        let hl = h - l;
        let mult = if hl > 1e-9 { ((c - l) - (h - c)) / hl } else { 0.0 };
        ad[i] = if i == 0 { mult * v } else { ad[i - 1] + mult * v };
    }
    ad
}

fn ad_momentum_at(ad_line: &[f64], period: usize, idx: usize) -> f64 {
    if idx < period { return 0.0; }
    ad_line[idx] - ad_line[idx - period]
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
    #[allow(dead_code)]
    max_dd: f64,
    trades: usize,
    #[allow(dead_code)]
    win_rate: f64,
    pass: bool,
}

/// Run simulation for one AD period. Returns (result, equity_curve).
fn run_sim(
    sym_data: &HashMap<String, SymData>,
    symbols: &[String],
    ad_period: usize,
    test_start: usize,
    test_end: usize,
) -> (WfResult, Vec<f64>) {
    let mut equity = 1.0_f64;
    let mut equity_curve = vec![1.0_f64];
    let mut wins = 0usize;
    let mut total_trades = 0usize;
    let mut daily_rets = Vec::new();

    let mut bar = test_start;
    while bar + 2 < test_end {
        // Rank symbols by A/D momentum at current bar
        let mut scores: Vec<(&str, f64)> = Vec::new();
        for sym in symbols {
            if let Some(sd) = sym_data.get(sym) {
                if bar >= ad_period && bar < sd.ad_line.len() {
                    let mom = ad_momentum_at(&sd.ad_line, ad_period, bar);
                    if mom.is_finite() {
                        scores.push((sym.as_str(), mom));
                    }
                }
            }
        }
        scores.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());
        let top_syms: Vec<String> = scores.into_iter().take(TOP_K).map(|(s, _)| s.to_string()).collect();

        if top_syms.is_empty() {
            equity_curve.push(equity);
            bar += 1;
            continue;
        }

        // Long top-1 symbol by A/D momentum
        let mut entered = false;
        for sym in &top_syms {
            if let Some(sd) = sym_data.get(sym) {
                if bar < sd.close.len() && bar >= ad_period {
                    let entry_px = sd.close[bar];
                    let entry = entry_px * (1.0 + TAKER_FEE);
                    let entry_bar_next = bar + 1;
                    let n = sd.close.len();

                    // Chandelier ATR(15, 2.0) trailing stop
                    let mut highest_high = sd.high[entry_bar_next.min(n.saturating_sub(1))];
                    let max_bar = (entry_bar_next + HOLD_MAX).min(n.saturating_sub(1));
                    let mut exit_bar = max_bar;
                    for b in entry_bar_next..=max_bar.min(n.saturating_sub(1)) {
                        highest_high = highest_high.max(sd.high[b]);
                        let atr = atr_at(&sd.high, &sd.low, &sd.close, CHAND_PERIOD, b);
                        let trail = highest_high - CHAND_MULT * atr;
                        if sd.close[b] < trail {
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

                        if equity > 1.0_f64 { /* ignore peak */ }
                        equity_curve.push(equity);
                        bar = exit_bar + 1;
                        entered = true;
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

    let ret = (equity - 1.0) * 100.0;
    let sharpe = annualised_sharpe(&daily_rets);
    let max_dd = max_dd_from(&equity_curve);
    let win_rate = if total_trades > 0 { wins as f64 / total_trades as f64 * 100.0 } else { 0.0 };
    let pass = total_trades >= MIN_TRADES && ret > 0.0;
    (WfResult { ret, sharpe, max_dd, trades: total_trades, win_rate, pass }, equity_curve)
}

#[tokio::main]
async fn main() -> Result<()> {
    let t0 = Instant::now();

    std::fs::create_dir_all(EQUITY_OUT_DIR)?;

    eprintln!("==== A/D Period Fine-Grained Hyperopt — 9 Universes ====");
    eprintln!("Sweep: p ∈ [1, 100] step 1 (100 values)");
    eprintln!("Exit: Chandelier({}, {}), TOP_K={}, CAP={}, HM={}", CHAND_PERIOD, CHAND_MULT, TOP_K, POSITION_CAP, HOLD_MAX);

    // ── Load data ────────────────────────────────────────────────────────────────
    let loader = DataLoader::new(None, None);
    let mut all_syms: std::collections::HashSet<String> = std::collections::HashSet::new();
    for (_, syms) in UNIVERSES { for &s in *syms { all_syms.insert(s.to_string()); } }

    let mut raw_cache: HashMap<String, DataFrame> = HashMap::new();
    let mut min_len = usize::MAX;
    for sym in all_syms.iter() {
        match loader.fetch_with_cache(sym.as_str(), "1d", CANDLES).await {
            Ok(df) => { min_len = min_len.min(df.height()); raw_cache.insert(sym.clone(), df); }
            Err(e) => { eprintln!("  WARNING: {} load failed: {}", sym, e); }
        }
    }

    let n = min_len.min(2800);
    let mut sym_data_map: HashMap<String, SymData> = HashMap::new();
    for sym in &all_syms {
        if let Some(df) = raw_cache.get(sym) {
            let n_min: usize = df.height().min(n);
            macro_rules! col_vec { ($name:expr) => {{
                let chunked = df.column($name)?.f64()?;
                chunked.into_iter().filter_map(|x| x).take(n_min).collect::<Vec<_>>()
            }};
            }
            let close = col_vec!("close"); let high = col_vec!("high");
            let low = col_vec!("low"); let vol = col_vec!("volume");
            let ad_line = compute_ad(&high, &low, &close, &vol);
            sym_data_map.insert(sym.clone(), SymData { close, high, low, vol, ad_line });
        }
    }
    eprintln!("Loaded {} symbols, {} bars\n", sym_data_map.len(), n);

    // ── Run sweep ────────────────────────────────────────────────────────────────
    let ad_periods: Vec<usize> = (1..=100).collect();
    let n_periods = ad_periods.len();
    let n_universes = UNIVERSES.len();

    // Per-period global aggregates
    let mut global_sharpe: Vec<f64> = vec![0.0; n_periods];
    let mut global_ret: Vec<f64> = vec![0.0; n_periods];
    let mut global_pass: Vec<usize> = vec![0; n_periods];
    let mut global_total: Vec<usize> = vec![0; n_periods];
    let mut global_trades: Vec<usize> = vec![0; n_periods];

    // Per-period per-universe results (for equity curves of key periods)
    let baseline_p = 47;
    let export_ps: Vec<usize> = vec![baseline_p, best_p, 2, 20];

    // ── Phase 1: Collect aggregate metrics per period (fast) ───────────────────
    eprintln!("Phase 1: Sweeping 100 AD periods × 9 universes...");
    for &(_label, symbols) in UNIVERSES {
        let symbols: Vec<String> = symbols.iter().map(|s| s.to_string()).collect();
        let all_loaded = symbols.iter().all(|s| sym_data_map.contains_key(s));
        if !all_loaded { continue; }

        let total_windows = n.saturating_sub(TRAIN_BARS + TEST_BARS) / TEST_BARS;
        if total_windows == 0 { continue; }

        // Pre-compute windows for this universe
        let windows: Vec<(usize, usize)> = (0..total_windows)
            .filter_map(|wi| {
                let train_end = TRAIN_BARS + wi * TEST_BARS;
                let test_start = train_end;
                let test_end = (test_start + TEST_BARS).min(n);
                if test_end.saturating_sub(test_start) < 5 { None }
                else { Some((test_start, test_end)) }
            })
            .collect();

        for (pi, &ad_period) in ad_periods.iter().enumerate() {
            let mut agg_sharpe = 0.0_f64;
            let mut agg_ret = 0.0_f64;
            let mut passed = 0usize;
            let mut tot_trades = 0usize;

            for &(ts, te) in &windows {
                let (r, _) = run_sim(&sym_data_map, &symbols, ad_period, ts, te);
                agg_sharpe += r.sharpe;
                agg_ret += r.ret;
                if r.pass { passed += 1; }
                tot_trades += r.trades;
            }

            let nw = windows.len();
            global_sharpe[pi] += agg_sharpe / nw as f64;
            global_ret[pi] += agg_ret / nw as f64;
            global_pass[pi] += passed;
            global_total[pi] += nw;
            global_trades[pi] += tot_trades;
        }
    }

    // Find winner
    let mut best_idx = 0usize;
    let mut best_sharpe = f64::NEG_INFINITY;
    for i in 0..n_periods {
        let avg_sh = global_sharpe[i] / n_universes as f64;
        if avg_sh > best_sharpe { best_sharpe = avg_sh; best_idx = i; }
    }
    let best_p = ad_periods[best_idx];
    eprintln!("\nWINNER: AD_PERIOD={} (avg Sharpe={:.4e})", best_p, best_sharpe);

    // Top 5
    let mut ranked: Vec<(usize, f64)> = ad_periods.iter().enumerate()
        .map(|(i, &p)| (p, global_sharpe[i] / n_universes as f64))
        .collect();
    ranked.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());
    eprintln!("\nTop 5 AD periods:");
    for (rank, (p, sh)) in ranked.iter().take(5).enumerate() {
        eprintln!("  #{:2}: p={:3}  Sharpe={:.4e}", rank+1, p, *sh);
    }

    // ── Write sweep CSV ─────────────────────────────────────────────────────────
    {
        let mut csv_f = File::create(CSV_OUT)?;
        writeln!(csv_f, "ad_period,avg_sharpe,avg_return_pct,total_pass,total_windows,total_trades")?;
        for (pi, &ad_period) in ad_periods.iter().enumerate() {
            let avg_sh_f = global_sharpe[pi] / n_universes as f64;
            let avg_ret_f = global_ret[pi] / n_universes as f64;
            writeln!(csv_f, "{},{:.6},{:.4},{},{},{}",
                ad_period, avg_sh_f, avg_ret_f,
                global_pass[pi], global_total[pi], global_trades[pi])?;
        }
    }
    eprintln!("\nSweep CSV: {}", CSV_OUT);

    // ── Phase 2: Equity curves for key periods ─────────────────────────────────
    eprintln!("\nPhase 2: Exporting equity curves for p ∈ {:?}", export_ps);

    // Compute per-universe per-window equity curves for export periods
    for &export_p in &export_ps {
        let eq_path = format!("{}/ad_p{}_equity.csv", EQUITY_OUT_DIR, export_p);
        let mut eq_file = File::create(&eq_path)?;

        // Write header
        // Format: universe,window,bar,equity
        writeln!(eq_file, "universe,window,bar,equity")?;

        for &(label, symbols) in UNIVERSES {
            let symbols: Vec<String> = symbols.iter().map(|s| s.to_string()).collect();
            let all_loaded = symbols.iter().all(|s| sym_data_map.contains_key(s));
            if !all_loaded { continue; }

            let total_windows = n.saturating_sub(TRAIN_BARS + TEST_BARS) / TEST_BARS;
            if total_windows == 0 { continue; }

            for wi in 0..total_windows {
                let train_end = TRAIN_BARS + wi * TEST_BARS;
                let test_start = train_end;
                let test_end = (test_start + TEST_BARS).min(n);
                if test_end.saturating_sub(test_start) < 5 { continue; }

                let (_, eq) = run_sim(&sym_data_map, &symbols, export_p, test_start, test_end);
                for (bi, e) in eq.iter().enumerate() {
                    writeln!(eq_file, "{}_{:02},{},{},{:.8e}", label, wi, wi, bi, *e)?
                }
            }
        }
        eprintln!("  Equity CSV: {} ({} bytes)", eq_path, std::fs::metadata(&eq_path)?.len());
    }

    eprintln!("\nTotal runtime: {:?}", t0.elapsed());
    Ok(())
}
