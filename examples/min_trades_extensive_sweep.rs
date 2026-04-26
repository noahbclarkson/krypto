//! MIN_TRADES Hyperopt — Extensive Range
//!
//! HYPOTHESIS: MIN_TRADES=3 was validated on range 1-6 only.
//! The range {7, 8, 10, 12, 15, 20} was never tested.
//! At higher thresholds, the strategy's statistical reliability changes:
//! - Lower MIN_TRADES: more windows qualify but with fewer trades (noisier)
//! - Higher MIN_TRADES: fewer windows qualify but each has more statistical weight
//!
//! SWEEP: {1, 2, 3, 4, 5, 6, 7, 8, 10, 12, 15, 20} × 9 universes × ~6 windows
//! Current production default: MIN_TRADES=3
//!
//! If winner ≠ 3 → update config.rs and document rationale.

use anyhow::Result;
use krypto::data::loader::DataLoader;
use polars::prelude::DataFrame;
use std::collections::HashMap;
use std::fs::File;
use std::io::Write;
use std::time::Instant;

const CANDLES: u32 = 3000;
const TRAIN_BARS: usize = 252;
const TEST_BARS: usize = 252;

// Production params (frozen)
const EP: usize = 21;
const CHAND_P: usize = 7;
const CHAND_M: f64 = 2.30;
const ATR_P: usize = 24;
const ATR_M: f64 = 2.0;
const HOLD_MAX: usize = 12;
const POS_CAP: usize = 3;
const TAKER_FEE: f64 = 0.001;

// SWEEP: extensive range including values > 6 (never tested)
const MIN_TRADE_VALS: &[usize] = &[1, 2, 3, 4, 5, 6, 7, 8, 10, 12, 15, 20];

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

const CSV_OUT: &str = "snapshots/min_trades_sweep.csv";
const EQUITY_CSV: &str = "snapshots/min_trades_equity.csv";
const SUMMARY_MD: &str = "snapshots/min_trades_summary.md";

struct SymData {
    close: Vec<f64>,
    high:  Vec<f64>,
    low:   Vec<f64>,
    vol:   Vec<f64>,
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

fn turtle_signal(close: &[f64], entry_period: usize, idx: usize) -> bool {
    if idx < entry_period + 1 { return false; }
    let start = idx + 1 - entry_period;
    let mut mx = f64::NEG_INFINITY;
    for i in start..idx {
        if let Some(&c) = close.get(i) { mx = mx.max(c); }
    }
    close.get(idx).map(|&c| c > mx).unwrap_or(false)
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
    equity_curve: Vec<f64>,
}

fn run_sim(
    sym_data: &HashMap<String, SymData>,
    symbols: &[String],
    test_start: usize,
    test_end: usize,
    min_trades: usize,
) -> WfResult {
    let mut equity = 1.0_f64;
    let mut equity_curve = vec![1.0_f64];
    let mut peak = equity;
    let mut wins = 0usize;
    let mut total_trades = 0usize;
    let mut daily_rets = Vec::new();

    let mut bar = test_start;
    while bar + 2 < test_end {
        // Dollar-volume ranking for position selection
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
        let top_syms: Vec<String> = scores.into_iter().take(POS_CAP)
            .map(|(s, _)| s.to_string()).collect();

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
                        let entry_bar_next = bar + 1;
                        let n = sd.close.len();

                        // Dual exit: Chandelier OR Turtle ATR (whichever fires first)
                        let mut highest_high_chand = sd.high[entry_bar_next];
                        let mut highest_high_turtle = sd.high[entry_bar_next];
                        let max_bar = (entry_bar_next + HOLD_MAX).min(n.saturating_sub(1));
                        let mut exit_bar = max_bar;

                        for b in entry_bar_next..=max_bar.min(n.saturating_sub(1)) {
                            highest_high_chand = highest_high_chand.max(sd.high[b]);
                            let atr_c = atr_at(&sd.high, &sd.low, &sd.close, CHAND_P, b);
                            let trail_c = highest_high_chand - CHAND_M * atr_c;

                            highest_high_turtle = highest_high_turtle.max(sd.high[b]);
                            let atr_t = atr_at(&sd.high, &sd.low, &sd.close, ATR_P, b);
                            let trail_t = highest_high_turtle - ATR_M * atr_t;

                            if sd.close[b] < trail_c || sd.close[b] < trail_t {
                                exit_bar = b;
                                break;
                            }
                        }

                        if let Some(&exit_px) = sd.close.get(exit_bar) {
                            let exit = exit_px * (1.0 - TAKER_FEE);
                            let gross_ret = exit / entry_px - 1.0;
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
    // THE CRITICAL LOGIC: window passes if it has >= min_trades AND positive return
    let pass = total_trades >= min_trades && ret > 0.0;

    WfResult { ret, sharpe, max_dd, trades: total_trades, win_rate, pass, equity_curve }
}

#[tokio::main]
async fn main() -> Result<()> {
    let t0 = Instant::now();
    println!("==== MIN_TRADES Hyperopt — Extensive Range ====");
    println!("Sweeping: {:?}", MIN_TRADE_VALS);
    println!("Production default: MIN_TRADES=3 (validated 2026-04-16 on range 1-6 only)");
    println!("Goal: test full range set to sweep [1,2,3,4,5,6,7,8,10,12,15,20]\n");

    let loader = DataLoader::new(None, None);
    let mut all_syms: std::collections::HashSet<String> = std::collections::HashSet::new();
    for (_, syms) in UNIVERSES {
        for &s in *syms { all_syms.insert(s.to_string()); }
    }

    let mut raw_cache: HashMap<String, DataFrame> = HashMap::new();
    let mut min_len = usize::MAX;
    for sym in all_syms.iter() {
        match loader.fetch_with_cache(sym, "1d", CANDLES).await {
            Ok(df) => {
                min_len = min_len.min(df.height());
                raw_cache.insert(sym.clone(), df);
            }
            Err(e) => { println!("  WARNING: {} load failed: {}", sym, e); }
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
                close: col_vec!("close"),
                high:  col_vec!("high"),
                low:   col_vec!("low"),
                vol:   col_vec!("volume"),
            });
        }
    }
    println!("Loaded {} symbols, {} bars\n", sym_data_map.len(), n);

    // Per-universe per-min_trades results
    let mut results: HashMap<String, (f64, usize, usize, f64, f64, usize, f64)> = HashMap::new();

    for &(label, symbols) in UNIVERSES {
        let symbols: Vec<String> = symbols.iter().map(|s| s.to_string()).collect();
        let all_loaded = symbols.iter().all(|s| sym_data_map.contains_key(s));
        if !all_loaded { continue; }

        let total_windows = n.saturating_sub(TRAIN_BARS + TEST_BARS) / TEST_BARS;
        if total_windows == 0 { continue; }

        println!("==== {:<18} ==== {} syms, {} windows", label, symbols.len(), total_windows);

        for &mt in MIN_TRADE_VALS {
            let key = format!("{}|{}", label, mt);

            let mut pass_count = 0usize;
            let mut sum_sharpe = 0.0_f64;
            let mut sum_ret = 0.0_f64;
            let mut sum_dd = 0.0_f64;
            let mut sum_trades = 0usize;
            let mut sum_wr = 0.0_f64;

            for wi in 0..total_windows {
                let train_end = TRAIN_BARS + wi * TEST_BARS;
                let test_start = train_end;
                let test_end = (test_start + TEST_BARS).min(n);
                if test_end.saturating_sub(test_start) < 5 { continue; }

                let r = run_sim(&sym_data_map, &symbols, test_start, test_end, mt);

                let thin = if r.trades < 3 { "THIN" } else { "OK" };
                let result = if r.pass { "PASS" } else { "FAIL" };
                println!(
                    "  mt={} W{:02} | {:+8.1}% sh={:6.2} DD={:5.1}% {:4}t {:3.0}% {} {}",
                    mt, wi, r.ret, r.sharpe, r.max_dd, r.trades, r.win_rate, thin, result
                );

                sum_sharpe += r.sharpe;
                sum_ret += r.ret;
                sum_dd += r.max_dd;
                sum_trades += r.trades;
                sum_wr += r.win_rate;
                if r.pass { pass_count += 1; }
            }

            results.insert(key, (sum_sharpe, pass_count, total_windows, sum_ret, sum_dd, sum_trades, sum_wr));
        }
    }

    // Global aggregation by MIN_TRADES
    let mut global_by_mt: HashMap<usize, (f64, usize, usize, f64, f64, usize, f64)> = HashMap::new();
    for &(label, _) in UNIVERSES {
        for &mt in MIN_TRADE_VALS {
            let key = format!("{}|{}", label, mt);
            if let Some(v) = results.get(&key) {
                global_by_mt
                    .entry(mt)
                    .and_modify(|e| {
                        e.0 += v.0; e.1 += v.1; e.2 += v.2; e.3 += v.3;
                        e.4 += v.4; e.5 += v.5; e.6 += v.6;
                    })
                    .or_insert(*v);
            }
        }
    }

    // Summary sorted by avg Sharpe
    let mut summary: Vec<(usize, f64, usize, usize, f64, f64, usize, f64)> = Vec::new();
    for &mt in MIN_TRADE_VALS {
        if let Some(&(sh, pass, total, ret, dd, trades, wr)) = global_by_mt.get(&mt) {
            let avg_sharpe = sh / total as f64;
            let avg_ret = ret / total as f64;
            let avg_dd = dd / total as f64;
            let avg_wr = wr / total as f64;
            summary.push((mt, avg_sharpe, pass, total, avg_ret, avg_dd, trades, avg_wr));
        }
    }
    summary.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());

    println!("\n==== GLOBAL RANKING (by avg Sharpe) ====");
    for (i, entry) in summary.iter().enumerate() {
        let mt = entry.0;
        let sh = entry.1;
        let pass = entry.2;
        let total = entry.3;
        let ret = entry.4;
        let dd = entry.5;
        let trades = entry.6;
        let wr = entry.7;
        let pr = pass as f64 / total as f64 * 100.0;
        let marker = if mt == 3 {
            "  ←CURRENT DEFAULT".to_string()
        } else if i == 0 {
            "  ←WINNER".to_string()
        } else {
            String::new()
        };
        println!(
            "  #{:2} MT={:2}  avg_sharpe={:7.4}  pass={:3}/{} ({:4.0}%)  ret={:+8.1}%  DD={:5.1}%  trades={:5}  wr={:4.1}%{}",
            i+1, mt, sh, pass, total, pr, ret, dd, trades, wr, marker
        );
    }

    let winner = summary.first().map(|(mt,_,_,_,_,_,_,_)| *mt).unwrap_or(3);
    let winner_sharpe = summary.first().map(|(_,s,_,_,_,_,_,_)| *s).unwrap_or(0.0);
    let baseline_sharpe = summary.iter()
        .find(|(mt,_,_,_,_,_,_,_)| *mt == 3)
        .map(|(_,s,_,_,_,_,_,_)| *s).unwrap_or(0.0);
    let improvement = if baseline_sharpe != 0.0 {
        (winner_sharpe - baseline_sharpe) / baseline_sharpe * 100.0
    } else { 0.0 };

    println!("\n==== WINNER: MIN_TRADES={} ====", winner);
    println!("  Avg Sharpe: {:.4}  (baseline MT=3: {:.4}, delta={:+.2}%)", winner_sharpe, baseline_sharpe, improvement);

    // ── Write sweep CSV ────────────────────────────────────────────────────
    {
        let mut f = File::create(CSV_OUT)?;
        writeln!(f, "min_trades,avg_sharpe,pass_count,total_windows,pass_rate_pct,avg_return_pct,avg_max_dd_pct,total_trades,avg_win_rate_pct")?;
        for entry in &summary {
            let mt = entry.0;
            let sh = entry.1;
            let pass = entry.2;
            let total = entry.3;
            let ret = entry.4;
            let dd = entry.5;
            let trades = entry.6;
            let wr = entry.7;
            let pr = pass as f64 / total as f64 * 100.0;
            writeln!(f, "{},{:.4},{},{},{:.1},{:.1},{:.1},{},{:.1}", mt, sh, pass, total, pr, ret, dd, trades, wr)?;
        }
        println!("\nWrote: {}", CSV_OUT);
    }

    // ── Write equity CSV (top 3 + baseline) ──────────────────────────────────
    {
        let mut f = File::create(EQUITY_CSV)?;
        writeln!(f, "universe,min_trades,window_idx,cumulative_equity")?;

        let mut mts_to_show: Vec<usize> = summary.iter().take(3).map(|(mt,_,_,_,_,_,_,_)| *mt).collect();
        if !mts_to_show.contains(&3) {
            mts_to_show.push(3);
        }

        for &(label, symbols) in UNIVERSES {
            let symbols: Vec<String> = symbols.iter().map(|s| s.to_string()).collect();
            let all_loaded = symbols.iter().all(|s| sym_data_map.contains_key(s));
            if !all_loaded { continue; }

            let total_windows = n.saturating_sub(TRAIN_BARS + TEST_BARS) / TEST_BARS;
            if total_windows == 0 { continue; }

            for &mt in &mts_to_show {
                let mut cumulative_equity = 1.0_f64;
                for wi in 0..total_windows {
                    let train_end = TRAIN_BARS + wi * TEST_BARS;
                    let test_start = train_end;
                    let test_end = (test_start + TEST_BARS).min(n);
                    if test_end.saturating_sub(test_start) < 5 { continue; }

                    let r = run_sim(&sym_data_map, &symbols, test_start, test_end, mt);
                    if let Some(&last_eq) = r.equity_curve.last() {
                        cumulative_equity *= last_eq;
                        writeln!(f, "{},{},{},{:.6}", label, mt, wi, cumulative_equity)?;
                    }
                }
            }
        }
        println!("Wrote: {}", EQUITY_CSV);
    }

    // ── Write summary MD ───────────────────────────────────────────────────
    {
        let mut f = File::create(SUMMARY_MD)?;
        writeln!(f, "# MIN_TRADES Hyperopt — Extensive Range")?;
        writeln!(f, "\nDate: 2026-04-26")?;
        writeln!(f, "\n## Hypothesis")?;
        writeln!(f, "MIN_TRADES=3 was validated on range 1-6 only.")?;
        writeln!(f, "The range {{7, 8, 10, 12, 15, 20}} was never tested.")?;
        writeln!(f, "At higher thresholds, statistical reliability changes:");
        writeln!(f, "- Lower MIN_TRADES: more windows qualify but fewer trades (noisier)")?;
        writeln!(f, "- Higher MIN_TRADES: fewer windows qualify but more statistical weight")?;
        writeln!(f, "\n## Design")?;
        writeln!(f, "- Swept: {:?}", MIN_TRADE_VALS)?;
        writeln!(f, "- Current default: MIN_TRADES=3 (2026-04-16 sweep only tested 1-6)")?;
        writeln!(f, "- 9 universes × ~{} windows each", n.saturating_sub(TRAIN_BARS + TEST_BARS) / TEST_BARS)?;
        writeln!(f, "- Pass rule: total_trades >= min_trades AND return > 0")?;
        writeln!(f, "\n## Results")?;
        writeln!(f, "\n| Rank | MT | Avg Sharpe | Pass Rate | Avg Return | Avg DD | Trades |")?;
        writeln!(f, "|------|----|------------|-----------|------------|--------|--------|")?;
        for (i, entry) in summary.iter().enumerate() {
            let mt = entry.0;
            let sh = entry.1;
            let pass = entry.2;
            let total = entry.3;
            let ret = entry.4;
            let dd = entry.5;
            let trades = entry.6;
            let pr = pass as f64 / total as f64 * 100.0;
            let marker = if mt == 3 {
                " ←CURRENT".to_string()
            } else if i == 0 {
                " ←WINNER".to_string()
            } else {
                String::new()
            };
            writeln!(f, "| {}{} | {} | {:.4} | {}/{} ({:.0}%) | {:+.1}% | {:.1}% | {} |",
                i+1, marker, mt, sh, pass, total, pr, ret, dd, trades)?;
        }
        writeln!(f, "\n## Winner")?;
        writeln!(f, "**MIN_TRADES = {}** (avg Sharpe {:.4}, baseline MT=3 {:.4}, delta={:+.2}%)",
            winner, winner_sharpe, baseline_sharpe, improvement)?;
        writeln!(f, "\n## Files")?;
        writeln!(f, "- {} — per-MT summary", CSV_OUT)?;
        writeln!(f, "- {} — equity curve data (top 3 + baseline)", EQUITY_CSV)?;
        writeln!(f, "\nElapsed: {:.1}s", t0.elapsed().as_secs_f64())?;
        println!("\nWrote: {}", SUMMARY_MD);
    }

    println!("\nTotal elapsed: {:.1}s", t0.elapsed().as_secs_f64());
    Ok(())
}