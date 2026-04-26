//! T3: EP=24 vs EP=21 Paired Held-Out Validation on Current Production Params
//!
//! Purpose: EP=24 won OOS by only +2 windows over EP=21 (+4% Sharpe).
//! At 30% false-positive rate per window: expected ~3 false winners per 96-value sweep.
//! A 2-window delta is noise-level without held-out confirmation.
//!
//! This harness compares EP=21 vs EP=24 using CURRENT production params:
//!   CHAND_PERIOD=7, CHAND_MULT=2.30, HOLD_MAX=12, ATR_ENTRY_MULT=0.00
//! Tested against pre-2021 held-out data — data the OOS hyperopt never touched.
//!
//! Decision rule:
//!   If EP=24 < EP=21 on held-out → revert EP=24 → 21
//!   If EP=24 ≥ EP=21 on held-out → keep EP=24 (marginal but validated)

use anyhow::Result;
use krypto::data::loader::DataLoader;
use std::collections::HashMap;
use std::fs::File;
use std::io::Write;
use std::time::Instant;

const HOLD_MAX: usize = 12;
const TAKER_FEE: f64 = 0.001;
const MIN_TRADES: usize = 3;
const CHAND_PERIOD: usize = 7;
const CHAND_MULT: f64 = 2.30;
const TURTLE_ATR_PERIOD: usize = 24;
const TURTLE_ATR_MULT: f64 = 2.0;
const ATR_ENTRY_MULT: f64 = 0.00;
const CANDLES: u32 = 3000;

const SYMBOLS: &[&str] = &[
    "BTCUSDT", "ETHUSDT", "SOLUSDT", "XRPUSDT",
    "DOGEUSDT", "ADAUSDT", "LTCUSDT", "EOSUSDT",
    "BNBUSDT", "BCHUSDT",
];

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

fn turtle_signal(
    close: &[f64], high: &[f64], low: &[f64],
    entry_period: usize, atr_period: usize, atr_mult: f64, idx: usize,
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
    let ann_factor = (365.25 / daily_rets.len() as f64).sqrt();
    mn / sd * ann_factor
}

fn max_dd(equity: &[f64]) -> f64 {
    let mut peak = f64::NEG_INFINITY;
    let mut max_dd = 0.0;
    for &val in equity.iter() {
        if val > peak { peak = val; }
        let dd = (peak - val) / peak;
        if dd > max_dd { max_dd = dd; }
    }
    max_dd * 100.0
}

fn run_backtest(
    close: &[f64], high: &[f64], low: &[f64],
    test_start: usize, test_end: usize,
    entry_period: usize,
) -> (bool, f64, f64, f64, i64) {
    let mut equity = 1.0;
    let mut equity_curve = vec![equity];
    let mut active = false;
    let mut entry_price = 0.0;
    let mut bars_in_pos = 0;
    let mut trade_count = 0;

    for idx in test_start..=test_end {
        if !active {
            if turtle_signal(close, high, low, entry_period, TURTLE_ATR_PERIOD, ATR_ENTRY_MULT, idx) {
                active = true;
                entry_price = close[idx];
                equity *= 1.0 - TAKER_FEE;
                bars_in_pos = 0;
                equity_curve.push(equity);
            } else {
                equity_curve.push(equity);
            }
        } else {
            bars_in_pos += 1;
            let ret = (close[idx] - entry_price) / entry_price;
            equity *= (1.0 + ret) * (1.0 - TAKER_FEE);

            let should_exit = if bars_in_pos >= HOLD_MAX {
                true
            } else {
                let entry_bar = idx - bars_in_pos;
                let highest_high_chand = high[entry_bar..=idx].iter().fold(f64::NEG_INFINITY, |m, &v| m.max(v));
                let atr_chand = atr_at(high, low, close, CHAND_PERIOD, idx);
                let atr_turtle = atr_at(high, low, close, TURTLE_ATR_PERIOD, idx);
                let trail_chand = highest_high_chand - CHAND_MULT * atr_chand;
                let trail_turtle = highest_high_chand - TURTLE_ATR_MULT * atr_turtle;
                close[idx] < trail_chand || close[idx] < trail_turtle
            };

            if should_exit {
                active = false;
                entry_price = 0.0;
                trade_count += 1;
            }
            equity_curve.push(equity);
        }
    }

    if equity_curve.len() < 2 { return (false, -100.0, 0.0, 0.0, 0); }

    let rets: Vec<f64> = equity_curve.windows(2)
        .map(|w| (w[1] - w[0]) / w[0])
        .collect();

    let ret_pct = (equity - 1.0) * 100.0;
    let sh = annualised_sharpe(&rets);
    let dd = max_dd(&equity_curve);
    let pass = sh > 0.0 && trade_count >= MIN_TRADES;

    (pass, ret_pct, sh, dd, trade_count as i64)
}

fn ms_to_year(ms: i64) -> i32 {
    chrono::DateTime::from_timestamp(ms / 1000, 0)
        .map(|dt| chrono::Datelike::year(&dt))
        .unwrap_or(1970)
}

#[tokio::main]
async fn main() -> Result<()> {
    let start = Instant::now();
    println!("=== T3: EP=24 vs EP=21 Paired Held-Out Validation ===");
    println!("Current production params: CHAND(7,2.30), HM=12, ATR_ENTRY=0.00");
    println!("Held-out data: pre-2021 (periods hyperopt never used)\n");

    let loader = DataLoader::new(None, None);
    let all_syms: std::collections::HashSet<String> = SYMBOLS.iter().map(|s| s.to_string()).collect();

    let mut raw_cache: HashMap<String, krypto::data::loader::DataLoader> = HashMap::new();
    let mut sym_data_map: HashMap<String, (Vec<f64>, Vec<f64>, Vec<f64>, Vec<i64>)> = HashMap::new();

    for sym in all_syms.iter() {
        match loader.fetch_with_cache(sym, "1d", CANDLES).await {
            Ok(df) => {
                let n = df.height().min(2800);
                macro_rules! col_vec {
                    ($name:expr) => {{
                        let chunked = df.column($name)?.f64()?;
                        chunked.into_iter().filter_map(|x| x).take(n).collect::<Vec<_>>()
                    }};
                }
                let time_col = df.column("time")?.datetime()?;
                let time: Vec<i64> = time_col.into_iter().filter_map(|x| x).take(n).collect();
                sym_data_map.insert(sym.clone(), (
                    col_vec!("close"),
                    col_vec!("high"),
                    col_vec!("low"),
                    time,
                ));
            }
            Err(e) => { eprintln!("  WARNING: {} load failed: {}", sym, e); }
        }
    }

    println!("Loaded {} symbols\n", sym_data_map.len());

    // Pre-2021 year ranges (held-out from hyperopt)
    #[derive(Debug, Clone, Copy)]
    struct Period {
        name: &'static str,
        year_start: i32,
        year_end: i32,
    }
    let periods = vec![
        Period { name: "P3-2019", year_start: 2019, year_end: 2019 },
        Period { name: "P2-2020", year_start: 2020, year_end: 2020 },
        Period { name: "P1-2020", year_start: 2020, year_end: 2020 },
    ];

    let mut results: Vec<(String, String, bool, f64, f64, f64, i64, bool, f64, f64, f64, i64)> = Vec::new();

    for (sym, (close, high, low, time)) in sym_data_map.iter() {
        let n = close.len();
        if n < 100 { continue; }

        for period in &periods {
            // Find bars in this period (by year)
            let start_bar = time.iter().position(|&t| ms_to_year(t) == period.year_start).unwrap_or(0);
            let end_bar = time.iter().rposition(|&t| ms_to_year(t) == period.year_end).unwrap_or(n - 1);

            if end_bar < start_bar + 50 { continue; }

            let (pass21, ret21, sh21, dd21, trades21) = run_backtest(close, high, low, start_bar, end_bar, 21);
            let (pass24, ret24, sh24, dd24, trades24) = run_backtest(close, high, low, start_bar, end_bar, 24);

            let delta_sh = sh24 - sh21;
            println!("{:8} {:10} EP21={:0.2}({}tr) EP24={:0.2}({}tr) Δ={:0.2}",
                sym, period.name, sh21, trades21, sh24, trades24, delta_sh);

            results.push((sym.clone(), period.name.to_string(),
                pass21, ret21, sh21, dd21, trades21,
                pass24, ret24, sh24, dd24, trades24));
        }
    }

    // Summary
    println!("\n=== SUMMARY ===");
    let ep21_pass = results.iter().filter(|r| r.2).count();
    let ep24_pass = results.iter().filter(|r| r.7).count();
    let ep21_avg_sh = results.iter().map(|r| r.4).sum::<f64>() / results.len().max(1) as f64;
    let ep24_avg_sh = results.iter().map(|r| r.9).sum::<f64>() / results.len().max(1) as f64;
    let ep21_avg_ret = results.iter().map(|r| r.3).sum::<f64>() / results.len().max(1) as f64;
    let ep24_avg_ret = results.iter().map(|r| r.8).sum::<f64>() / results.len().max(1) as f64;

    println!("EP=21: {}/{} pass, avg Sharpe {:0.2}, avg return {:+.1}%", ep21_pass, results.len(), ep21_avg_sh, ep21_avg_ret);
    println!("EP=24: {}/{} pass, avg Sharpe {:0.2}, avg return {:+.1}%", ep24_pass, results.len(), ep24_avg_sh, ep24_avg_ret);
    println!("Delta Sharpe: {:0.2}", ep24_avg_sh - ep21_avg_sh);

    let verdict = if ep24_avg_sh >= ep21_avg_sh {
        "KEEP EP=24 — confirmed on held-out"
    } else {
        "REVERT EP=24 → 21 — EP=24 was in-sample inflation"
    };
    println!("\nVERDICT: {}", verdict);

    // CSV
    let mut csv = File::create("snapshots/t3_ep_paired_held_out.csv")?;
    writeln!(csv, "symbol,period,ep21_pass,ep21_return_pct,ep21_sharpe,ep21_dd,ep21_trades,ep24_pass,ep24_return_pct,ep24_sharpe,ep24_dd,ep24_trades")?;
    for r in &results {
        writeln!(csv, "{},{},{},{:0.4},{:0.4},{:0.4},{},{},{:0.4},{:0.4},{:0.4},{}",
            r.0, r.1, r.2, r.3, r.4, r.5, r.6,
            r.7, r.8, r.9, r.10, r.11)?;
    }

    println!("\nDone in {:.1}s. CSV: snapshots/t3_ep_paired_held_out.csv", start.elapsed().as_secs_f32());
    Ok(())
}
