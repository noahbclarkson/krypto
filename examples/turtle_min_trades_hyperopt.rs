//! =========================================================
//! HYPERPARAMETER OPTIMIZATION: MIN_TRADES
//! =========================================================
//!
//! Background:
//!   MIN_TRADES is a hardcoded threshold: a walk-forward window must have
//!   >= MIN_TRADES trades to be considered valid (pass/fail filter).
//!   Currently set to 3 with NO documented justification.
//!
//! Target sweep: MIN_TRADES ∈ {1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 12, 15, 20, 30}
//!   Step 1 for 1-10, step 2 for 12-30
//!
//! Strategy: Turtle+Chandelier DUAL_EXIT
//!   EP=21, Chandelier(28,2.15), Turtle_ATR(24,2.0), CAP=3, HM=45
//!   Walk-forward: 252 train / 252 test, all 9 universes

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
const HOLD_MAX: usize = 45;
const TAKER_FEE: f64 = 0.001;
const POSITION_CAP: usize = 3;
const EP: usize = 21;
const CHAND_PERIOD: usize = 28;
const CHAND_MULT: f64 = 2.15;
const TURTLE_ATR_PERIOD: usize = 24;
const TURTLE_ATR_MULT: f64 = 2.00;

const MIN_TRADES_VALUES: &[usize] = &[
    1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 12, 15, 20, 30
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

const CSV_OUT: &str = "snapshots/turtle_min_trades_sweep.csv";
const AGG_CSV: &str = "snapshots/turtle_min_trades_agg.csv";
const MD_OUT: &str = "snapshots/turtle_min_trades_sweep.md";

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
    trs.iter().sum::<f64>() / period as f64
}

fn turtle_signal(close: &[f64], entry_period: usize, idx: usize) -> bool {
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

struct WinResult {
    ret: f64,
    sharpe: f64,
    max_dd: f64,
    trades: usize,
    win_rate: f64,
    equity_mult: f64,  // compounded equity multiplier for this window
}

fn run_sim(sym_data: &HashMap<String, SymData>, symbols: &[String],
           test_start: usize, test_end: usize) -> WinResult {
    let mut equity = 1.0_f64;
    let mut equity_curve = vec![1.0_f64];
    let mut wins = 0usize;
    let mut total_trades = 0usize;
    let mut daily_rets = Vec::new();

    let mut bar = test_start;
    while bar + 2 < test_end {
        let mut scores: Vec<(&str, f64)> = Vec::new();
        for sym in symbols {
            if let Some(sd) = sym_data.get(sym) {
                if bar >= sd.close.len() { continue; }
                let dv = sd.vol.get(bar).copied().unwrap_or(0.0)
                    * sd.close.get(bar).copied().unwrap_or(0.0);
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
                if bar >= EP + 1 && bar < sd.close.len() {
                    if turtle_signal(&sd.close, EP, bar) {
                        let entry_px = sd.close[bar];
                        let entry = entry_px * (1.0 - TAKER_FEE);
                        let entry_bar_next = bar + 1;
                        let n = sd.close.len();

                        let mut highest_high_chand = sd.high[entry_bar_next];
                        let mut highest_high_turtle = sd.high[entry_bar_next];
                        let max_bar = (entry_bar_next + HOLD_MAX).min(n.saturating_sub(1));
                        let mut exit_bar = max_bar;

                        for b in entry_bar_next..=max_bar.min(n.saturating_sub(1)) {
                            highest_high_chand = highest_high_chand.max(sd.high[b]);
                            let atr_chand = atr_at(&sd.high, &sd.low, &sd.close, CHAND_PERIOD, b);
                            let trail_chand = highest_high_chand - CHAND_MULT * atr_chand;
                            highest_high_turtle = highest_high_turtle.max(sd.high[b]);
                            let atr_turtle = atr_at(&sd.high, &sd.low, &sd.close, TURTLE_ATR_PERIOD, b);
                            let trail_turtle = highest_high_turtle - TURTLE_ATR_MULT * atr_turtle;
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
                            for _ in 0..bars_held { daily_rets.push(avg_daily); }

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

    WinResult {
        ret: (equity - 1.0) * 100.0,
        sharpe: annualised_sharpe(&daily_rets),
        max_dd: max_dd_from(&equity_curve),
        trades: total_trades,
        win_rate: if total_trades > 0 { wins as f64 / total_trades as f64 * 100.0 } else { 0.0 },
        equity_mult: equity,
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    let t0 = Instant::now();
    eprintln!("==== MIN_TRADES Hyperopt: 14 values × 9 universes ====");

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
            let n_min = df.height().min(n);
            macro_rules! col_vec {
                ($name:expr) => {{
                    let chunked = df.column($name)?.f64()?;
                    chunked.into_iter().filter_map(|x| x).take(n_min).collect::<Vec<_>>()
                }};
            }
            sym_data_map.insert(sym.clone(), SymData {
                close: col_vec!("close"), high: col_vec!("high"),
                low: col_vec!("low"), vol: col_vec!("volume"),
            });
        }
    }
    eprintln!("Loaded {} symbols, {} bars\n", sym_data_map.len(), n);

    // Run ALL windows for Base5 once, store results for equity reconstruction
    // We'll use the CSV to derive equity curves in Python
    let base5_syms: Vec<String> = UNIVERSES[0].1.iter().map(|s| s.to_string()).collect();
    let base5_n_windows = n.saturating_sub(TRAIN_BARS + TEST_BARS) / TEST_BARS;

    // Store base5 window results once (for all MT values)
    let mut base5_window_results: Vec<WinResult> = Vec::new();
    for wi in 0..base5_n_windows {
        let train_end = TRAIN_BARS + wi * TEST_BARS;
        let test_start = train_end;
        let test_end = (test_start + TEST_BARS).min(n);
        if test_end.saturating_sub(test_start) < 5 { continue; }
        base5_window_results.push(run_sim(&sym_data_map, &base5_syms, test_start, test_end));
    }
    eprintln!("Base5 windows: {}, cached for equity reconstruction", base5_window_results.len());

    // Main sweep: per-(MT, universe, window) results
    let mut all_rows: Vec<(usize, String, usize, bool, f64, f64, f64, usize, f64)> = Vec::new();

    for &mt in MIN_TRADES_VALUES {
        eprintln!("--- MT={} ---", mt);
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

                let r = run_sim(&sym_data_map, &symbols, test_start, test_end);
                let pass = r.trades >= mt && r.ret > 0.0;
                all_rows.push((mt, label.to_string(), wi, pass, r.ret, r.sharpe, r.max_dd, r.trades, r.win_rate));
            }
        }
    }

    // Write detailed CSV
    let mut csvf = File::create(CSV_OUT)?;
    writeln!(csvf, "min_trades,universe,window,pass,return_pct,sharpe,max_dd_pct,trades,win_rate_pct")?;
    for r in &all_rows {
        writeln!(csvf, "{},{},{},{},{:.2},{:.4},{:.2},{},{:.2}",
            r.0, r.1, r.2, r.3, r.4, r.5, r.6, r.7, r.8)?;
    }

    // Aggregate by MT
    let mut agg_map: std::collections::HashMap<usize, (usize, usize, f64, f64, f64, usize, f64)> =
        std::collections::HashMap::new();

    for &mt in MIN_TRADES_VALUES {
        let subset: Vec<_> = all_rows.iter().filter(|x| x.0 == mt).collect();
        let total = subset.len();
        let passed = subset.iter().filter(|x| x.3).count();
        let avg_sh = subset.iter().map(|x| x.5).sum::<f64>() / total.max(1) as f64;
        let avg_ret = subset.iter().map(|x| x.4).sum::<f64>() / total.max(1) as f64;
        let avg_dd = subset.iter().map(|x| x.6).sum::<f64>() / total.max(1) as f64;
        let tot_trades: usize = subset.iter().map(|x| x.7).sum();
        let avg_wr = subset.iter().map(|x| x.8).sum::<f64>() / total.max(1) as f64;
        agg_map.insert(mt, (passed, total, avg_sh, avg_ret, avg_dd, tot_trades, avg_wr));
    }

    // Write aggregate CSV
    let mut aggf = File::create(AGG_CSV)?;
    writeln!(aggf, "min_trades,global_pass,global_total,global_pass_pct,avg_sharpe,avg_return_pct,avg_max_dd_pct,total_trades,avg_win_rate")?;
    for &mt in MIN_TRADES_VALUES {
        let (passed, total, avg_sh, avg_ret, avg_dd, tot_trades, avg_wr) = agg_map[&mt];
        writeln!(aggf, "{},{},{},{:.2},{:.4},{:.2},{:.2},{},{:.2}",
            mt, passed, total, passed as f64/total as f64*100.0, avg_sh, avg_ret, avg_dd, tot_trades, avg_wr)?;
    }

    // Print table
    eprintln!("\n{:>12} | {:>5} | {:>5} | {:>7} | {:>8} | {:>+8} | {:>9}",
        "MT", "PASS", "TOT", "PASS%", "AVG_SH", "AVG_RET", "AVG_DD");
    eprintln!("{}", "-".repeat(65));
    for &mt in MIN_TRADES_VALUES {
        let (passed, total, avg_sh, avg_ret, avg_dd, _, _) = agg_map[&mt];
        eprintln!("{:>12} | {:>5} | {:>5} | {:>6.1}% | {:>8.3} | {:>+8.1} | {:>8.1}%",
            mt, passed, total, passed as f64/total as f64*100.0, avg_sh, avg_ret, avg_dd);
    }

    // Write equity reconstruction CSV from cached Base5 window results
    // For each MT: compound Base5 windows in chronological order
    let eq_csv_path = "snapshots/turtle_min_trades_base5_equity.csv";
    let mut eqf = File::create(eq_csv_path)?;
    writeln!(eqf, "window,{}", MIN_TRADES_VALUES.iter().map(|v| format!("mt_{}", v)).collect::<Vec<_>>().join(","))?;

    for (wi, base_r) in base5_window_results.iter().enumerate() {
        let mut row = vec![format!("{}", wi)];
        for &mt in MIN_TRADES_VALUES {
            // Equity for this window under this MT:
            // If trades < mt threshold, window is "skipped" (equity stays at 1.0)
            // If trades >= mt, use the actual return
            let window_equity = if base_r.trades >= mt {
                base_r.equity_mult
            } else {
                1.0_f64
            };
            row.push(format!("{:.6}", window_equity));
        }
        writeln!(eqf, "{}", row.join(","))?;
    }

    // Write markdown summary
    let mut md = File::create(MD_OUT)?;
    writeln!(md, "# MIN_TRADES Hyperopt Results")?;
    writeln!(md, "")?;
    writeln!(md, "**Sweep:** {:?}", MIN_TRADES_VALUES)?;
    writeln!(md, "**Strategy:** Turtle+Chandelier DUAL_EXIT (EP=21, CHAND(28,2.15), ATR(24,2.0), CAP=3)")?;
    writeln!(md, "**Universes:** 9, Walk-forward: 252 train / 252 test")?;
    writeln!(md, "**Baseline:** MT=3 (currently hardcoded)")?;
    writeln!(md, "")?;
    writeln!(md, "| MT | Pass | Total | Pass% | Avg Sharpe | Avg Ret% | Avg DD% | Trades |")?;
    writeln!(md, "|---|---|---|---|---|---|---|---|")?;
    for &mt in MIN_TRADES_VALUES {
        let (passed, total, avg_sh, avg_ret, avg_dd, tot_trades, _) = agg_map[&mt];
        let marker = if mt == 3 { " ←BASELINE" } else { "" };
        writeln!(md, "| **{}**{} | {} | {} | {:.1}% | {:.3} | {:+.1}% | {:.1}% | {} |",
            mt, marker, passed, total, passed as f64/total as f64*100.0, avg_sh, avg_ret, avg_dd, tot_trades)?;
    }
    writeln!(md, "")?;

    // Find winner: highest pass_pct, tie-break by highest avg_sharpe
    let mut best_mt = 3usize;
    let mut best_pct = 0.0f64;
    let mut best_sh = 0.0f64;
    for &mt in MIN_TRADES_VALUES {
        let (passed, total, avg_sh, _, _, _, _) = agg_map[&mt];
        let pct = passed as f64 / total.max(1) as f64 * 100.0;
        if pct > best_pct || (pct == best_pct && avg_sh > best_sh) {
            best_pct = pct;
            best_sh = avg_sh;
            best_mt = mt;
        }
    }

    let baseline_pct = agg_map[&3].1 as f64 / (agg_map[&3].2 as f64).max(1.0) * 100.0;
    writeln!(md, "**Winner: MT={}** (pass {:.1}%, Sharpe {:.3})", best_mt, best_pct, best_sh)?;
    if best_mt != 3 {
        writeln!(md, "**Update recommendation:** Change MIN_TRADES from 3 → {}", best_mt)?;
    } else {
        writeln!(md, "**No improvement found.** Keep MIN_TRADES=3.")?;
    }
    writeln!(md, "")?;
    writeln!(md, "**Runtime:** {:?}", t0.elapsed())?;

    eprintln!("\nFiles: {} {} {} {}", CSV_OUT, AGG_CSV, eq_csv_path, MD_OUT);
    eprintln!("Done in {:?}", t0.elapsed());
    Ok(())
}