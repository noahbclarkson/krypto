//! ATR_ENTRY_MULT vs Turtle ATR Exit Bug Fix — Isolation Test
//!
//! Compare 3 configurations to separate the effects of:
//! 1. ATR_ENTRY_MULT (EM) — momentum filter on entry
//! 2. Turtle ATR exit anchor — highest_high (WRONG) vs lowest_low (CORRECT)
//!
//! Configs tested:
//!   A: EM=0.0, highest_high anchor  (old buggy)
//!   B: EM=0.0, lowest_low anchor    (Turtle ATR exit fix only)
//!   C: EM=0.90, lowest_low anchor   (EM + fix — current production)

use std::collections::HashMap;
use polars::prelude::*;
use krypto::data::loader::DataLoader;

const TRAIN_BARS: usize = 252;
const TEST_BARS: usize = 252;
const HOLD_MAX: usize = 12;
const TAKER_FEE: f64 = 0.001;
const POSITION_CAP: usize = 3;
const MIN_TRADES: usize = 3;
const VOL_LOOKBACK: usize = 2;
const CHAND_PERIOD: usize = 11;
const CHAND_MULT: f64 = 2.25;
const TURTLE_ENTRY: usize = 24;
const TURTLE_ATR_PERIOD: usize = 24;
const TURTLE_ATR_MULT: f64 = 2.00;

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

struct WfResult {
    ret: f64, sharpe: f64, max_dd: f64, trades: usize, pass: bool,
}

struct SymData {
    close: Vec<f64>,
    high: Vec<f64>,
    low: Vec<f64>,
    vol: Vec<f64>,
}

fn load_sym(sym: &str, n: usize) -> Option<SymData> {
    let cache = std::env::var("KRYPTO_CACHE").unwrap_or_else(|_| "data/cache".to_string());
    let path = format!("{}/{}_{}.parquet", cache, sym.to_lowercase(), "1d");
    let df = DataLoader::load_parquet(std::path::Path::new(&path)).ok()?;
    let n = df.height().min(n);
    macro_rules! col_vec {
        ($name:expr) => {{
            let chunked = df.column($name).ok()?.f64().ok()?;
            chunked.into_iter().filter_map(|x| x).take(n).collect::<Vec<_>>()
        }};
    }
    Some(SymData {
        close: col_vec!("close"),
        high: col_vec!("high"),
        low: col_vec!("low"),
        vol: col_vec!("volume"),
    })
}

fn rolling_avg(vol: &[f64], window: usize, idx: usize) -> f64 {
    let start = idx.saturating_sub(window);
    let slice = &vol[start..idx];
    if slice.is_empty() { return 0.0; }
    slice.iter().sum::<f64>() / slice.len() as f64
}

fn atr_at(high: &[f64], low: &[f64], close: &[f64], period: usize, idx: usize) -> f64 {
    if idx < period { return 0.0; }
    let mut tr_sum = 0.0_f64;
    for i in (idx + 1 - period)..=idx {
        let pc = if i == 0 { close[0] } else { close[i - 1] };
        let tr = (high[i] - low[i]).max((high[i] - pc).abs()).max((low[i] - pc).abs());
        tr_sum += tr;
    }
    tr_sum / period as f64
}

fn turtle_signal(close: &[f64], high: &[f64], low: &[f64], entry_period: usize, atr_period: usize, atr_mult: f64, idx: usize) -> bool {
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

fn run_sim(
    sym_data: &HashMap<String, SymData>,
    symbols: &[String],
    test_start: usize,
    test_end: usize,
    atr_mult: f64,
    use_lowest_low_turtle: bool,
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
                    if turtle_signal(&sd.close, &sd.high, &sd.low, TURTLE_ENTRY, TURTLE_ATR_PERIOD, atr_mult, bar) {
                        let entry_px = sd.close[bar];
                        let entry = entry_px * (1.0 - TAKER_FEE);
                        let entry_bar_next = bar + 1;
                        let n = sd.close.len();

                        let mut highest_high_chand = sd.high[entry_bar_next];
                        let mut trail_anchor_turtle = if use_lowest_low_turtle {
                            sd.low[entry_bar_next]
                        } else {
                            sd.high[entry_bar_next]
                        };
                        let max_bar = (entry_bar_next + HOLD_MAX).min(n.saturating_sub(1));
                        let mut exit_bar = max_bar;
                        for b in entry_bar_next..=max_bar.min(n.saturating_sub(1)) {
                            highest_high_chand = highest_high_chand.max(sd.high[b]);
                            let atr_chand = atr_at(&sd.high, &sd.low, &sd.close, CHAND_PERIOD, b);
                            let trail_chand = highest_high_chand - CHAND_MULT * atr_chand;

                            if use_lowest_low_turtle {
                                trail_anchor_turtle = trail_anchor_turtle.min(sd.low[b]);
                            } else {
                                trail_anchor_turtle = trail_anchor_turtle.max(sd.high[b]);
                            }
                            let atr_turtle = atr_at(&sd.high, &sd.low, &sd.close, TURTLE_ATR_PERIOD, b);
                            let trail_turtle = if use_lowest_low_turtle {
                                trail_anchor_turtle - TURTLE_ATR_MULT * atr_turtle
                            } else {
                                trail_anchor_turtle - TURTLE_ATR_MULT * atr_turtle
                            };

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
    let pass = total_trades >= MIN_TRADES && ret > 0.0;
    WfResult { ret, sharpe, max_dd, trades: total_trades, pass }
}

fn main() {
    let symbols: Vec<String> = UNIVERSES.iter()
        .flat_map(|(_, s)| s.iter().map(|s| s.to_string()))
        .collect();
    let symbols: Vec<String> = symbols.into_iter().collect::<std::collections::HashSet<_>>()
        .into_iter().collect();

    println!("Loading data...");
    let mut sym_data_map: HashMap<String, SymData> = HashMap::new();
    let mut min_len = usize::MAX;
    for sym in &symbols {
        if let Some(data) = load_sym(sym, 2800) {
            min_len = min_len.min(data.close.len());
            sym_data_map.insert(sym.clone(), data);
        } else {
            eprintln!("  SKIP {}", sym);
        }
    }
    println!("Loaded {} symbols\n", sym_data_map.len());

    // Configs: (label, atr_mult, use_lowest_low_turtle)
    let configs: &[(&str, f64, bool)] = &[
        ("A: EM=0.0, highest_high (old buggy)",   0.0, false),
        ("B: EM=0.0, lowest_low (Turtle ATR fix)", 0.0, true),
        ("C: EM=0.90, lowest_low (production)",    0.90, true),
    ];

    let mut results: Vec<(String, usize, usize, f64, usize)> = Vec::new();
    // (label, pass, total, avg_sharpe, total_trades)

    for &(label, em, use_ll) in configs {
        let mut total_pass = 0usize;
        let mut total_runs = 0usize;
        let mut all_sharpes = Vec::new();
        let mut total_trades = 0usize;

        for &(uname, symbols) in UNIVERSES {
            let symbols: Vec<String> = symbols.iter().map(|s| s.to_string()).collect();
            let all_loaded = symbols.iter().all(|s| sym_data_map.contains_key(s));
            if !all_loaded { continue; }

            let n = symbols.iter().find_map(|s| sym_data_map.get(s)).map(|s| s.close.len()).unwrap_or(0);
            let total_windows = n.saturating_sub(TRAIN_BARS + TEST_BARS) / TEST_BARS;
            if total_windows == 0 { continue; }

            for wi in 0..total_windows {
                let test_start = TRAIN_BARS + wi * TEST_BARS;
                let test_end = (test_start + TEST_BARS).min(n);
                if test_end.saturating_sub(test_start) < 5 { continue; }

                let r = run_sim(&sym_data_map, &symbols, test_start, test_end, em, use_ll);
                total_runs += 1;
                if r.pass { total_pass += 1; }
                all_sharpes.push(r.sharpe);
                total_trades += r.trades;
            }
        }

        let avg_sharpe = all_sharpes.iter().sum::<f64>() / all_sharpes.len().max(1) as f64;
        results.push((label.to_string(), total_pass, total_runs, avg_sharpe, total_trades));
        let pass_pct = total_pass as f64 / total_runs as f64 * 100.0;
        println!("{:<50}  {}/{} ({:.0}%)  Sharpe={:.2}  trades={}",
            label, total_pass, total_runs, pass_pct, avg_sharpe, total_trades);
    }

    println!("\n--- ISOLATION ---");
    println!("Turtle ATR exit fix (B vs A): +{:.0}pp pass, Sharpe {:.2}→{:.2}", 
        (results[1].1 as f64/results[1].2 as f64 - results[0].1 as f64/results[0].2 as f64) * 100.0,
        results[0].3, results[1].3);
    println!("ATR_ENTRY_MULT=0.90 (C vs B): +{:.0}pp pass, Sharpe {:.2}→{:.2}",
        (results[2].1 as f64/results[2].2 as f64 - results[1].1 as f64/results[1].2 as f64) * 100.0,
        results[1].3, results[2].3);
    println!("Combined (C vs A): +{:.0}pp pass, Sharpe {:.2}→{:.2}",
        (results[2].1 as f64/results[2].2 as f64 - results[0].1 as f64/results[0].2 as f64) * 100.0,
        results[0].3, results[2].3);
}
