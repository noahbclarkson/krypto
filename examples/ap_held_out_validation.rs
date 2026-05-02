//! AP=64 vs AP=12 Held-Out Validation on Pre-2021 Data
//!
//! Purpose: REGIME_ATR_PERIOD=64 was found as the 3rd sequential optimization
//! on `live_compatible_wf.rs` (ATR_RANK=24 → LB=42 → AP=64).
//! EP=24 failed held-out validation (same pattern: sequential optimization on same OOS grid).
//! This is the identical check for AP=64.
//!
//! Test: AP=64 (current production) vs AP=12 (prior validated baseline)
//! Tested against pre-2021 data — data the OOS hyperopt NEVER touched.
//!
//! Decision rule:
//!   If AP=12 ≥ AP=64 on held-out → AP=64 is a same-harness artifact → revert AP=64 → 12
//!   If AP=64 > AP=12 on held-out → AP=64 is validated → keep AP=64

use anyhow::Result;
use krypto::data::loader::DataLoader;
use std::collections::HashMap;
use std::fs::File;
use std::io::Write;
use std::time::Instant;

const HOLD_MAX: usize = 12;
const TAKER_FEE: f64 = 0.001;
const MIN_TRADES: usize = 3;
const EP: usize = 21;
const CHAND_PERIOD: usize = 7;
const CHAND_MULT: f64 = 2.30;
const TURTLE_ATR_PERIOD: usize = 24;
const TURTLE_ATR_MULT: f64 = 2.0;
const ATR_ENTRY_MULT: f64 = 0.00;
const POSITION_CAP: usize = 3;
const ATR_RANK_THRESHOLD: f64 = 24.0;
const REGIME_LOOKBACK: usize = 42;
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

fn regime_atr_percentile_btc(
    btc_close: &[f64], btc_high: &[f64], btc_low: &[f64],
    regime_atr_period: usize, regime_lookback: usize,
    idx: usize,
) -> f64 {
    let cur_atr = atr_at(btc_high, btc_low, btc_close, regime_atr_period, idx);
    if cur_atr <= 0.0 { return 50.0; }
    let start = idx.saturating_sub(regime_lookback);
    let mut hist: Vec<f64> = Vec::with_capacity(idx - start);
    for i in start..=idx {
        let a = atr_at(btc_high, btc_low, btc_close, regime_atr_period, i);
        if a > 0.0 { hist.push(a); }
    }
    if hist.len() < 10 { return 50.0; }
    let below = hist.iter().filter(|&&x| x < cur_atr).count();
    (below as f64 / hist.len() as f64) * 100.0
}

fn turtle_signal(
    close: &[f64], entry_period: usize, idx: usize,
) -> bool {
    if idx < entry_period + 1 { return false; }
    let start = idx + 1 - entry_period;
    let mut max_close = f64::NEG_INFINITY;
    for i in start..idx {
        if let Some(&c) = close.get(i) { max_close = max_close.max(c); }
    }
    close.get(idx).map_or(false, |&c| c > max_close)
}

fn max_dd(equity: &[f64]) -> f64 {
    let mut peak = f64::NEG_INFINITY;
    let mut max_dd = 0.0;
    for &e in equity {
        if e > peak { peak = e; }
        let dd = (peak - e) / peak;
        if dd > max_dd { max_dd = dd; }
    }
    max_dd
}

fn annualised_sharpe(daily_rets: &[f64]) -> f64 {
    if daily_rets.len() < 2 { return 0.0; }
    let mn: f64 = daily_rets.iter().sum::<f64>() / daily_rets.len() as f64;
    let sd = (daily_rets.iter().map(|x| (x - mn).powi(2)).sum::<f64>() / daily_rets.len() as f64).sqrt();
    if sd == 0.0 { return 0.0; }
    let ann_factor = (365.25 / daily_rets.len() as f64).sqrt();
    mn / sd * ann_factor
}

fn ms_to_year(ms: i64) -> i32 {
    chrono::DateTime::from_timestamp(ms / 1000, 0)
        .map(|dt| chrono::Datelike::year(&dt))
        .unwrap_or(1970)
}

/// Run backtest on a single-symbol time slice.
/// Returns (passed, sharpe_pct, total_return_pct, max_dd, trade_count)
fn run_backtest(
    close: &[f64], high: &[f64], low: &[f64], times: &[i64],
    btc_close: &[f64], btc_high: &[f64], btc_low: &[f64],
    regime_atr_period: usize,
    start_idx: usize, end_idx: usize,
) -> (bool, f64, f64, f64, i64) {
    let mut equity = 1.0f64;
    let mut equity_curve = vec![1.0];
    let mut trade_count = 0i64;
    let mut active = false;
    let mut entry_price = 0.0f64;
    let mut bars_in_pos = 0usize;
    let mut entry_bar = 0usize;

    for idx in start_idx..end_idx {
        let ts = times[idx];
        let year = ms_to_year(ts);

        // Regime filter on BTC
        let btc_rank = regime_atr_percentile_btc(
            btc_close, btc_high, btc_low,
            regime_atr_period, REGIME_LOOKBACK, idx,
        );
        let regime_pass = btc_rank >= ATR_RANK_THRESHOLD;

        if active {
            bars_in_pos += 1;
            // Exit checks
            let exit = {
                let entry_bar_idx = entry_bar;
                let highest_high_chand = high[entry_bar_idx..=idx].iter().fold(f64::NEG_INFINITY, |m, &v| m.max(v));
                let atr_chand = atr_at(high, low, close, CHAND_PERIOD, idx);
                let atr_turtle = atr_at(high, low, close, TURTLE_ATR_PERIOD, idx);
                let trail_chand = highest_high_chand - CHAND_MULT * atr_chand;
                let trail_turtle = highest_high_chand - TURTLE_ATR_MULT * atr_turtle;
                bars_in_pos >= HOLD_MAX || close[idx] < trail_chand || close[idx] < trail_turtle
            };

            if exit {
                let gross = (close[idx] / entry_price - 1.0) * (1.0 - TAKER_FEE);
                equity *= 1.0 + gross;
                active = false;
                entry_price = 0.0;
                trade_count += 1;
            }
            equity_curve.push(equity);
        } else {
            // Entry check
            if regime_pass && turtle_signal(close, EP, idx) {
                active = true;
                entry_price = close[idx];
                entry_bar = idx;
                bars_in_pos = 0;
                equity *= 1.0 - TAKER_FEE; // entry fee
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
    let pass = sh > 0.0 && trade_count >= MIN_TRADES as i64;

    (pass, sh, ret_pct, dd, trade_count)
}

#[tokio::main]
async fn main() -> Result<()> {
    let start = Instant::now();
    println!("=== AP=64 vs AP=12 Held-Out Validation ===");
    println!("Pre-2021 held-out data — data the OOS hyperopt NEVER touched");
    println!("Current prod: EP={}, CHAND({},{}/{}), HM={}, ATR_RANK={}, LB={}",
        EP, CHAND_PERIOD, CHAND_MULT, TURTLE_ATR_PERIOD, HOLD_MAX, ATR_RANK_THRESHOLD as usize, REGIME_LOOKBACK);

    let loader = DataLoader::new(None, None);
    let all_syms: std::collections::HashSet<String> = SYMBOLS.iter().map(|s| s.to_string()).collect();

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

    // Get BTC regime data once
    let btc_data = sym_data_map.get("BTCUSDT").expect("BTCUSDT must load");
    let (btc_close, btc_high, btc_low, _) = ( &btc_data.0, &btc_data.1, &btc_data.2, &btc_data.3);

    println!("\nLoaded {} symbols\n", sym_data_map.len());

    // Pre-2021 held-out periods
    #[derive(Debug, Clone, Copy)]
    struct Period {
        name: &'static str,
        year_start: i32,
        year_end: i32,
    }
    let periods = vec![
        Period { name: "P1-2019", year_start: 2019, year_end: 2019 },
        Period { name: "P2-2020", year_start: 2020, year_end: 2020 },
        Period { name: "P3-2020", year_start: 2020, year_end: 2020 },
    ];

    let mut results: Vec<String> = vec![
        "Period,AP,Passed,Sharpe,TotalRet%,MaxDD%,Trades,W/L".to_string(),
    ];

    // Aggregate counters
    let mut ap12_pass = 0u32;
    let mut ap64_pass = 0u32;
    let mut ap12_sharpe_sum = 0.0f64;
    let mut ap64_sharpe_sum = 0.0f64;
    let mut ap12_ret_sum = 0.0f64;
    let mut ap64_ret_sum = 0.0f64;
    let mut ap12_dd_max = 0.0f64;
    let mut ap64_dd_max = 0.0f64;
    let mut ap12_trades = 0u32;
    let mut ap64_trades = 0u32;

    for (sym, (close, high, low, times)) in sym_data_map.iter() {
        for period in &periods {
            // Find time indices for this period
            let start_idx = times.iter().position(|&t| ms_to_year(t) == period.year_start).unwrap_or(0);
            let end_idx = times.iter().rposition(|&t| ms_to_year(t) <= period.year_end).map(|p| p + 1).unwrap_or(times.len());

            if end_idx - start_idx < 50 { continue; }

            // Run with AP=12
            let (pass12, sh12, ret12, dd12, trades12) = run_backtest(
                close, high, low, times,
                btc_close, btc_high, btc_low,
                12, start_idx, end_idx,
            );

            // Run with AP=64
            let (pass64, sh64, ret64, dd64, trades64) = run_backtest(
                close, high, low, times,
                btc_close, btc_high, btc_low,
                64, start_idx, end_idx,
            );

            results.push(format!("{}-{},AP=12,{},{:0.3},{:0.1}%%,{:0.1}%%,{}", 
                period.name, sym, if pass12 { 1 } else { 0 }, sh12, ret12, dd12*100.0, trades12));
            results.push(format!("{}-{},AP=64,{},{:0.3},{:0.1}%%,{:0.1}%%,{}",
                period.name, sym, if pass64 { 1 } else { 0 }, sh64, ret64, dd64*100.0, trades64));

            if pass12 { ap12_pass += 1; }
            if pass64 { ap64_pass += 1; }
            ap12_sharpe_sum += sh12;
            ap64_sharpe_sum += sh64;
            ap12_ret_sum += ret12;
            ap64_ret_sum += ret64;
            ap12_dd_max = ap12_dd_max.max(dd12);
            ap64_dd_max = ap64_dd_max.max(dd64);
            ap12_trades += trades12 as u32;
            ap64_trades += trades64 as u32;
        }
    }

    // Summary
    let n = SYMBOLS.len() as f64 * 3.0; // 3 periods × n symbols
    results.push(format!("---"));
    results.push(format!("AP=12: {}/{} pass, avg Sharpe {:0.3}, avg Ret {:0.1}%%, MaxDD {:0.1}%%, {} trades",
        ap12_pass, n as u32, ap12_sharpe_sum/n, ap12_ret_sum/n, ap12_dd_max*100.0, ap12_trades));
    results.push(format!("AP=64: {}/{} pass, avg Sharpe {:0.3}, avg Ret {:0.1}%%, MaxDD {:0.1}%%, {} trades",
        ap64_pass, n as u32, ap64_sharpe_sum/n, ap64_ret_sum/n, ap64_dd_max*100.0, ap64_trades));
    results.push(format!("---"));

    let verdict = if ap64_pass >= ap12_pass && ap64_sharpe_sum >= ap12_sharpe_sum {
        "AP=64 VALIDATED: ≥ AP=12 on held-out → keep AP=64 in config.rs"
    } else {
        "AP=64 REJECTED: < AP=12 on held-out → REVERT AP=64 → AP=12"
    };
    results.push(verdict.to_string());
    results.push(format!("Runtime: {:.1}s", start.elapsed().as_secs_f64()));

    let output = results.join("\n");
    println!("\n{}", output);

    let mut f = File::create("snapshots/ap_held_out_validation.csv")?;
    f.write_all(output.as_bytes())?;

    Ok(())
}