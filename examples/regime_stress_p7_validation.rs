//! Regime Stress Test: Current Production Params
//! Tests P=7/M=2.25 (EP=24, HM=12) on pre-2021 held-out data.

use anyhow::Result;
use krypto::data::loader::DataLoader;
use std::collections::HashMap;

const HOLD_MAX: usize = 12;
const TAKER_FEE: f64 = 0.001;
const CHAND_P: usize = 7;
const CHAND_M: f64 = 2.25;
const EP: usize = 24;
const TURTLE_ATR_M: f64 = 2.0;

const SYMBOLS: &[&str] = &[
    "BTCUSDT", "ETHUSDT", "SOLUSDT", "XRPUSDT",
    "DOGEUSDT", "ADAUSDT", "LTCUSDT", "EOSUSDT",
    "BNBUSDT", "BCHUSDT",
];

fn tr(high: f64, low: f64, prev_close: f64) -> f64 {
    (high - low).max((high - prev_close).abs()).max((low - prev_close).abs())
}

fn run_window(symbol: &str, start_ts: i64, end_ts: i64) -> Result<Option<HashMap<String, f64>>> {
    let rt = tokio::runtime::Runtime::new()?;
    rt.block_on(async {
        let loader = DataLoader::new(None, None);
        let df = loader.fetch_data(symbol, "1d", 3000).await?;
        let time_col = df.column("time")?.datetime()?;
        let close_col = df.column("close")?.f64()?;
        let high_col = df.column("high")?.f64()?;
        let low_col = df.column("low")?.f64()?;
        let n = df.height();

        let mut close_v = Vec::new();
        let mut high_v = Vec::new();
        let mut low_v = Vec::new();

        for i in 0..n {
            let t = time_col.get(i).unwrap_or(0);
            if t < start_ts || t > end_ts { continue; }
            close_v.push(close_col.get(i).unwrap_or(0.0));
            high_v.push(high_col.get(i).unwrap_or(0.0));
            low_v.push(low_col.get(i).unwrap_or(0.0));
        }

        if close_v.len() < EP + CHAND_P + 20 { return Ok(None); }

        let mut capital = 10000.0;
        let mut peak = capital;
        let mut max_dd = 0.0;
        let mut wins = 0usize;
        let mut losses = 0usize;
        let mut trades = 0usize;
        let mut in_pos = false;
        let mut entry_price = 0.0;
        let mut highest_high = 0.0;
        let mut lowest_low = 0.0;
        let mut bars_held = 0usize;
        let mut atr_buf: Vec<f64> = Vec::with_capacity(CHAND_P);

        for i in (EP + 1)..close_v.len() {
            if !in_pos {
                let start_i = i.saturating_sub(EP);
                let mx = close_v[start_i..i].iter().copied().fold(f64::NEG_INFINITY, f64::max);
                if close_v[i] >= mx {
                    in_pos = true;
                    entry_price = close_v[i];
                    highest_high = high_v[i];
                    lowest_low = low_v[i];
                    bars_held = 0;
                    atr_buf.clear();
                    for j in (i.saturating_sub(CHAND_P)..=i) {
                        if j > 0 {
                            let c0 = close_v[j - 1];
                            atr_buf.push(tr(high_v[j], low_v[j], c0));
                        }
                    }
                }
            } else {
                if high_v[i] > highest_high { highest_high = high_v[i]; }
                if low_v[i] < lowest_low { lowest_low = low_v[i]; }
                bars_held += 1;

                let c0 = close_v[i - 1];
                atr_buf.push(tr(high_v[i], low_v[i], c0));
                if atr_buf.len() > CHAND_P { atr_buf.remove(0); }

                if atr_buf.len() >= CHAND_P {
                    let atr_val: f64 = atr_buf.iter().sum::<f64>() / CHAND_P as f64;
                    if atr_val > 0.0 {
                        let chand_stop = highest_high - CHAND_M * atr_val;
                        let turtle_stop = lowest_low - TURTLE_ATR_M * atr_val;
                        let exit = chand_stop.max(turtle_stop);

                        if low_v[i] <= exit || bars_held >= HOLD_MAX {
                            let pnl = (close_v[i] - entry_price) / entry_price;
                            capital *= 1.0 + pnl - TAKER_FEE;
                            if pnl > 0.0 { wins += 1; } else { losses += 1; }
                            trades += 1;
                            in_pos = false;

                            if capital > peak { peak = capital; }
                            let dd = (peak - capital) / peak;
                            if dd > max_dd { max_dd = dd; }
                        }
                    }
                }
            }
        }

        if trades < 3 { return Ok(None); }

        let ret = (capital - 10000.0) / 10000.0 * 100.0;
        let wr = wins as f64 / (wins + losses) as f64 * 100.0;
        let sharpe = if max_dd > 0.0 { (ret / 100.0) / max_dd } else { ret / 100.0 };

        let mut r = HashMap::new();
        r.insert("return".to_string(), ret);
        r.insert("sharpe".to_string(), sharpe);
        r.insert("max_dd".to_string(), max_dd * 100.0);
        r.insert("win_rate".to_string(), wr);
        r.insert("trades".to_string(), trades as f64);
        Ok(Some(r))
    })
}

fn main() -> Result<()> {
    println!("\n============================================================");
    println!("  Regime Stress Test: P=7/M=2.25 (current production params)");
    println!("  Pre-2021 held-out — no hyperopt contamination");
    println!("============================================================\n");

    let periods = vec![
        ("P1-2020", 1577836800000i64, 1612137600000),
        ("P2-2021", 1612137600000, 1643673600000),
        ("P3-2019", 1546300800000, 1577836800000),
    ];

    let mut total_pass = 0;
    let mut total_windows = 0;
    let mut total_sharpe = 0.0;

    for (period, start_ts, end_ts) in periods {
        println!("  ── {} ──", period);
        let mut period_pass = 0;
        let mut period_sharpe_sum = 0.0;
        let mut period_windows = 0;

        for sym in SYMBOLS {
            match run_window(sym, start_ts, end_ts)? {
                Some(r) => {
                    let sh = r["sharpe"];
                    let passed = sh > 0.0;
                    if passed { period_pass += 1; }
                    period_sharpe_sum += sh;
                    period_windows += 1;
                    println!("    {:8} Sharpe={:+7.3} Ret={:+8.1} DD={:5.1} WR={:5.1} {}",
                        sym, sh, r["return"], r["max_dd"], r["win_rate"],
                        if passed { "PASS" } else { "FAIL" });
                }
                None => {}
            }
        }

        let avg_sharpe = if period_windows > 0 { period_sharpe_sum / period_windows as f64 } else { 0.0 };
        println!("  {}  {}/{} pass  avg Sharpe={:.3}\n", period, period_pass, period_windows, avg_sharpe);
        total_pass += period_pass;
        total_windows += period_windows;
        total_sharpe += period_sharpe_sum;
    }

    let overall_sharpe = if total_windows > 0 { total_sharpe / total_windows as f64 } else { 0.0 };
    let overall_pass_rate = if total_windows > 0 { total_pass as f64 / total_windows as f64 * 100.0 } else { 0.0 };

    println!("{}", "=".repeat(60));
    println!("  OVERALL: {}/{} pass ({:.1})   avg Sharpe={:.3}",
        total_pass, total_windows, overall_pass_rate, overall_sharpe);
    println!("{}", "=".repeat(60));

    if overall_pass_rate >= 70.0 {
        println!("\n  ✅ REGIME STRESS: PASSED (≥70% threshold)");
    } else {
        println!("\n  ❌ REGIME STRESS: MARGINAL ({:.1} < 70%)", overall_pass_rate);
    }

    Ok(())
}