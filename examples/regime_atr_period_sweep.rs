//! REGIME_ATR Period × Lookback — Extensive 2D Hyperopt
//!
//! HYPOTHESIS: The regime ATR period (AP) and lookback (LB) used for the BTC ATR-rank
//! entry gate are hardcoded at AP=21/LB=252 in the existing harness, but production
//! config uses AP=12/LB=42. These interact with the entry gate threshold T=5.0.
//! An extensive sweep of AP ∈ [5..60] step 1 (56 values) × LB ∈ {21,42,63,126,252}
//! × 9 universes × 6 WF windows tests whether production AP=12/LB=42 is truly optimal
//! or if a different (AP, LB) pair improves the robustness of the ATR-rank filter.
//!
//! Note: ATR_RANK_THRESHOLD=5.0 is fixed (production default). We are only optimizing
//! the ATR calculation used for the rank filter, not the threshold itself.

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

// Production params (frozen)
const TURTLE_ENTRY: usize = 21;
const CHAND_PERIOD: usize = 7;
const CHAND_MULT: f64 = 2.30;
const TURTLE_ATR_PERIOD: usize = 24;
const TURTLE_ATR_MULT: f64 = 2.00;
const HOLD_MAX: usize = 12;
const ATR_ENTRY_MULT: f64 = 0.00;
const POSITION_CAP: usize = 3;
const VOL_LOOKBACK: usize = 96;
const TAKER_FEE: f64 = 0.001;
const MIN_TRADES: usize = 3;
const ATR_RANK_THRESHOLD: f64 = 5.0;

// 9 standard universes
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

// AP ∈ [5..60] step 1
const AP_VALUES: &[usize] = &[
     5,  6,  7,  8,  9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20,
    21, 22, 23, 24, 25, 26, 27, 28, 29, 30, 31, 32, 33, 34, 35, 36,
    37, 38, 39, 40, 41, 42, 43, 44, 45, 46, 47, 48, 49, 50, 51, 52,
    53, 54, 55, 56, 57, 58, 59, 60,
];
const LB_VALUES: &[usize] = &[21, 42, 63, 126, 252];

const CSV_OUT: &str = "snapshots/regime_atr_period_sweep.csv";

#[derive(Clone)]
struct SymData {
    close: Vec<f64>,
    high: Vec<f64>,
    low: Vec<f64>,
    vol: Vec<f64>,
}

fn atr_at(h: &[f64], l: &[f64], c: &[f64], p: usize, idx: usize) -> f64 {
    if idx < p { return 0.0; }
    let mut trs = Vec::with_capacity(p);
    for i in (idx + 1 - p)..=idx {
        let hi = *h.get(i).unwrap_or(&0.0);
        let lo = *l.get(i).unwrap_or(&0.0);
        let c0 = *c.get(i.saturating_sub(1)).unwrap_or(&0.0);
        trs.push((hi - lo).max((hi - c0).abs()).max((lo - c0).abs()));
    }
    if trs.is_empty() { return 0.0; }
    trs.iter().sum::<f64>() / p as f64
}

fn rolling_avg(vals: &[f64], window: usize, idx: usize) -> f64 {
    if idx < window { return *vals.get(idx).unwrap_or(&0.0); }
    vals[idx + 1 - window..=idx].iter().sum::<f64>() / window as f64
}

fn turtle_signal(c: &[f64], h: &[f64], l: &[f64], ep: usize, ap: usize, am: f64, idx: usize) -> bool {
    if idx < ep + 1 { return false; }
    let start = idx + 1 - ep;
    let mut mx = f64::NEG_INFINITY;
    for i in start..idx {
        if let Some(&cv) = c.get(i) { mx = mx.max(cv); }
    }
    if let Some(&curr) = c.get(idx) {
        let breakout = curr > mx;
        if breakout && am > 0.0 {
            let atr_val = atr_at(h, l, c, ap, idx);
            return curr >= mx + am * atr_val;
        }
        breakout
    } else {
        false
    }
}

fn annualised_sharpe(rets: &[f64]) -> f64 {
    if rets.len() < 2 { return 0.0; }
    let mn: f64 = rets.iter().sum::<f64>() / rets.len() as f64;
    let sd = (rets.iter().map(|x| (x - mn).powi(2)).sum::<f64>() / rets.len() as f64).sqrt();
    if sd <= 1e-12 { return 0.0; }
    mn * 365.0_f64.sqrt() / sd
}

fn max_dd(equity: &[f64]) -> f64 {
    let mut peak = f64::NEG_INFINITY;
    let mut max_dd = 0.0_f64;
    for &e in equity {
        if e > peak { peak = e; }
        let dd = (peak - e) / peak;
        if dd > max_dd { max_dd = dd; }
    }
    max_dd * 100.0
}

/// BTC ATR percentile at index idx (0..=100).
fn btc_atr_pct(btc: &SymData, atr_p: usize, lookback: usize, idx: usize) -> f64 {
    if idx < atr_p.max(lookback) + 1 { return 50.0; }
    let curr_atr = atr_at(&btc.high, &btc.low, &btc.close, atr_p, idx);
    let curr_close = *btc.close.get(idx).unwrap_or(&1.0);
    if curr_close <= 0.0 || curr_atr <= 0.0 { return 50.0; }
    let curr_pct = curr_atr / curr_close;
    let start = idx.saturating_sub(lookback);
    let mut below = 0usize;
    let mut total = 0usize;
    for i in start..idx {
        if let Some(&c) = btc.close.get(i) {
            if c > 0.0 {
                let hist_atr = atr_at(&btc.high, &btc.low, &btc.close, atr_p, i);
                if hist_atr / c < curr_pct { below += 1; }
                total += 1;
            }
        }
    }
    if total == 0 { return 50.0; }
    (below as f64 / total as f64) * 100.0
}

struct SimResult {
    ret: f64,
    sharpe: f64,
    max_dd: f64,
    trades: usize,
    win_rate: f64,
    pass: bool,
    equity_final: f64,
}

fn run_sim(
    sym_data: &HashMap<String, SymData>,
    btc: &SymData,
    symbols: &[String],
    test_start: usize,
    test_end: usize,
    ap: usize,
    lb: usize,
) -> SimResult {
    let mut equity = 1.0_f64;
    let mut wins = 0usize;
    let mut total_trades = 0usize;
    let mut daily_rets = Vec::new();

    let mut bar = test_start;
    while bar + 2 < test_end {
        // Regime filter gate
        let regime_ok = btc_atr_pct(btc, ap, lb, bar) >= ATR_RANK_THRESHOLD;

        // Dollar-volume ranking
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
            bar += 1;
            continue;
        }

        // Entry
        let mut entered = false;
        if regime_ok && equity > 0.0 {
            for sym in &top_syms {
                if equity <= 0.0 || entered { break; }
                let sd = match sym_data.get(sym) {
                    Some(d) => d,
                    None => continue,
                };
                if bar >= TURTLE_ENTRY + 1 && bar < sd.close.len() {
                    if turtle_signal(&sd.close, &sd.high, &sd.low, TURTLE_ENTRY, TURTLE_ATR_PERIOD, ATR_ENTRY_MULT, bar) {
                        let entry_px = sd.close[bar];
                        let entry = entry_px * (1.0 + TAKER_FEE);
                        let entry_bar_next = bar + 1;
                        let n = sd.close.len();

                        // Dual exit: Chandelier OR Turtle ATR
                        let mut highest_high_chand = sd.high[entry_bar_next];
                        let mut lowest_low_turtle = sd.low[entry_bar_next];
                        let max_bar = (entry_bar_next + HOLD_MAX).min(n.saturating_sub(1));
                        let mut exit_bar = max_bar;
                        for b in entry_bar_next..=max_bar.min(n.saturating_sub(1)) {
                            highest_high_chand = highest_high_chand.max(sd.high[b]);
                            let atr_chand = atr_at(&sd.high, &sd.low, &sd.close, CHAND_PERIOD, b);
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
                            for _ in 0..bars_held { daily_rets.push(avg_daily); }
                            bar = exit_bar + 1;
                            entered = true;
                            break;
                        }
                    }
                }
            }
        }

        if !entered { bar += 1; }
    }

    let final_ret = (equity - 1.0) * 100.0;
    let sharpe = annualised_sharpe(&daily_rets);
    let max_dd_val = max_dd(&[1.0_f64, equity]);
    let win_rate = if total_trades > 0 { wins as f64 / total_trades as f64 * 100.0 } else { 0.0 };

    SimResult {
        ret: final_ret,
        sharpe,
        max_dd: max_dd_val,
        trades: total_trades,
        win_rate,
        pass: total_trades >= MIN_TRADES && equity > 1.0,
        equity_final: equity,
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    let t0 = Instant::now();

    eprintln!("=== REGIME_ATR Period × Lookback — Extensive 2D Hyperopt ===");
    eprintln!("AP: 5..60 step 1 ({}) × LB: {:?} ({} combos)", AP_VALUES.len(), LB_VALUES, AP_VALUES.len() * LB_VALUES.len());
    eprintln!("Fixed: EP={}, CHAND({},{}), HM={}, ATR({},{}), CAP={}, VL={}",
        TURTLE_ENTRY, CHAND_PERIOD, CHAND_MULT, HOLD_MAX, TURTLE_ATR_PERIOD, TURTLE_ATR_MULT, POSITION_CAP, VOL_LOOKBACK);
    eprintln!("Regime threshold: T={}", ATR_RANK_THRESHOLD);
    eprintln!();

    let loader = DataLoader::new(None, None);
    let mut all_syms: std::collections::HashSet<String> = std::collections::HashSet::new();
    for (_, syms) in UNIVERSES {
        for &s in *syms { all_syms.insert(s.to_string()); }
    }

    // Load data
    let mut raw_cache: HashMap<String, DataFrame> = HashMap::new();
    let mut min_len = usize::MAX;
    for sym in all_syms.iter() {
        match loader.fetch_with_cache(sym, "1d", CANDLES).await {
            Ok(df) => {
                let h = df.height();
                min_len = min_len.min(h);
                raw_cache.insert(sym.clone(), df);
            }
            Err(e) => { eprintln!("  WARN: {} load failed: {}", sym, e); }
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
    eprintln!("Loaded {} symbols, {} bars\n", sym_data_map.len(), n);

    let btc = sym_data_map.get("BTCUSDT").cloned()
        .expect("BTCUSDT data required");

    let mut f = File::create(CSV_OUT)?;
    writeln!(f, "universe,window,ap,lb,pass,sharpe,return_pct,max_dd_pct,trades,win_rate,equity_final")?;

    let total_windows_per_universe = n.saturating_sub(TRAIN_BARS + TEST_BARS) / TEST_BARS;
    let n_total = UNIVERSES.len() * 6;
    let grid_size = AP_VALUES.len() * LB_VALUES.len();
    eprintln!("Expected: {} universes × 6 windows × {} AP×LB combos = {} total runs",
        UNIVERSES.len(), grid_size, UNIVERSES.len() * 6 * grid_size);

    let mut combo_results: HashMap<(usize, usize), (usize, f64, f64, f64, usize, f64, f64)> = HashMap::new();

    for &(uname, symbols) in UNIVERSES {
        let symbols: Vec<String> = symbols.iter().map(|s| s.to_string()).collect();
        let all_loaded = symbols.iter().all(|s| sym_data_map.contains_key(s));
        if !all_loaded {
            eprintln!("{:>20} SKIPPED (missing)", uname);
            continue;
        }

        let total_wf = n.saturating_sub(TRAIN_BARS + TEST_BARS) / TEST_BARS;
        if total_wf == 0 {
            eprintln!("{:>20} SKIPPED (no data)", uname);
            continue;
        }

        for wi in 0..6 {
            let test_start = TRAIN_BARS + wi * TEST_BARS;
            let test_end = test_start + TEST_BARS;
            if test_end > n { break; }

            for &ap in AP_VALUES {
                for &lb in LB_VALUES {
                    let res = run_sim(&sym_data_map, &btc, &symbols, test_start, test_end, ap, lb);

                    writeln!(f, "{},{},{},{},{},{:.6},{:.4f},{:.2f},{:.2f},{},{:.4f},{:.6}",
                        uname, wi, ap, lb, res.pass as i32, res.sharpe, res.ret, res.max_dd, res.trades, res.win_rate, res.equity_final)?;

                    let entry = combo_results.entry((ap, lb)).or_insert((0, 0.0, 0.0, 0.0, 0, 0.0, 0.0));
                    entry.0 += res.pass as usize;
                    entry.1 += res.sharpe;
                    entry.2 += res.ret;
                    entry.3 += res.max_dd;
                    entry.4 += res.trades;
                    entry.5 += res.win_rate;
                    entry.6 += res.equity_final;
                }
            }

            let progress = (wi + 1) as f32 / 6.0_f32 * 100.0;
            if wi % 2 == 0 || wi == 5 {
                eprintln!("  {} W{}/5: {}/{} windows done", uname, wi, wi + 1, 6);
            }
        }
    }

    drop(f);

    let elapsed = t0.elapsed();
    eprintln!("\nDone in {:.1}s. CSV -> {}", elapsed.as_secs_f64(), CSV_OUT);

    // Print summary table sorted by pass% then Sharpe
    eprintln!("\n{:>4} {:>4} {:>6} {:>8} {:>8} {:>7} {:>7}",
        "AP", "LB", "pass%", "avg_sharpe", "avg_ret%", "avg_DD%", "avg_eq");
    eprintln!("{}", "-".repeat(55));

    let mut rows: Vec<_> = combo_results.iter().collect();
    rows.sort_by(|a, b| {
        let pa = a.1.0 as f64 / n_total as f64;
        let pb = b.1.0 as f64 / n_total as f64;
        pb.partial_cmp(&pa).unwrap()
            .then_with(|| b.1.1.partial_cmp(&a.1.1).unwrap())
    });

    for row in rows.iter().take(15) {
        let (key, vals) = row;
        let (ap, lb) = *key;
        let &(pass, sh, ret, dd, trades, wr, eq) = *vals;
        let pass_pct = pass as f64 / n_total as f64 * 100.0;
        eprintln!("{:>4} {:>4} {:>5.1f}% {:>8.3f} {:>8.1f}% {:>7.1f}% {:>8.3f}",
            ap, lb, pass_pct, sh/n_total as f64, ret/n_total as f64, dd/n_total as f64, eq/n_total as f64);
    }
    eprintln!("...");
    for row in rows.iter().rev().take(5) {
        let (key, vals) = row;
        let (ap, lb) = *key;
        let &(pass, sh, ret, dd, trades, wr, eq) = *vals;
        let pass_pct = pass as f64 / n_total as f64 * 100.0;
        eprintln!("{:>4} {:>4} {:>5.1f}% {:>8.3f} {:>8.1f}% {:>7.1f}% {:>8.3f}",
            ap, lb, pass_pct, sh/n_total as f64, ret/n_total as f64, dd/n_total as f64, eq/n_total as f64);
    }

    // Production comparison
    let prod_key = (12usize, 42usize);
    if let Some(prod_res) = combo_results.get(&prod_key) {
        let (prod_pass, prod_sh, _, _, _, _, prod_eq) = *prod_res;
        eprintln!("\n--- PRODUCTION (AP=12, LB=42): {:.1f}% pass, Sharpe {:.3f}, eq {:.3f}",
            prod_pass as f64 / n_total as f64 * 100.0,
            prod_sh / n_total as f64,
            prod_eq / n_total as f64);
    }

    eprintln!("\n=== WINNER ===");
    let (best_key, best_vals) = rows[0];
    let (best_ap, best_lb) = *best_key;
    let best_pass = best_vals.0;
    let best_sh = best_vals.1;
    let best_ret = best_vals.2;
    let best_dd = best_vals.3;
    eprintln!("AP={} LB={}: {:.1f}% pass, Sharpe {:.3f}, ret {:.1f}%, DD {:.1f}%",
        best_ap, best_lb,
        best_pass as f64 / n_total as f64 * 100.0,
        best_sh / n_total as f64,
        best_ret / n_total as f64,
        best_dd / n_total as f64);

    Ok(())
}
