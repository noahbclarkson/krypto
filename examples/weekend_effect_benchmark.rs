//! Weekend Effect Benchmark
//!
//! Sep 2025 literature: BTC weekend momentum stronger than weekdays.
//! Institutional flow closure drives weekend retail/whale dominance.
//!
//! Tests whether day-of-week conditional tilt on the frozen three-sleeve book
//! is a genuine orthogonal lane or just another exposure-shape trick.
//!
//! Execution assumptions:
//! - signal at close using only current/past data
//! - entry next open
//! - exit after fixed 21-bar hold at open
//! - 0.1% taker each side
//! - three-sleeve consensus: A/D + MACD+Regime + Small proxy

use anyhow::Result;
use chrono::{Datelike, TimeZone, Utc, Weekday};
use krypto::{
    data::{loader::DataLoader, universe::compute_cross_sectional_features},
    features::indicators::FeatureEngine,
};
use polars::prelude::*;
use std::collections::HashMap;

const BENCHMARK: &str = "BTCUSDT";
const CANDLES: u32 = 3000;
const HOLD_BARS: usize = 21;
const TAKER_FEE: f64 = 0.001;
const CS_LOOKBACK: usize = 63;
const WARMUP_BARS: usize = 200;
const AD_PERIOD: usize = 5; // hyperopt winner 2026-04-13 (was 47)
const MIN_TRADES_PER_WINDOW: usize = 30;

const UNIVERSES: &[(&str, &[&str])] = &[
    (
        "Base5",
        &["ETHUSDT", "SOLUSDT", "XRPUSDT", "DOGEUSDT", "ADAUSDT"],
    ),
    ("NoDOGE", &["ETHUSDT", "SOLUSDT", "XRPUSDT", "ADAUSDT"]),
    ("Legacy4", &["ETHUSDT", "XRPUSDT", "LTCUSDT", "EOSUSDT"]),
    (
        "Legacy5BNB",
        &["ETHUSDT", "XRPUSDT", "LTCUSDT", "BNBUSDT", "EOSUSDT"],
    ),
    (
        "OldGuardNoBNB",
        &["ETHUSDT", "XRPUSDT", "LTCUSDT", "EOSUSDT", "BCHUSDT"],
    ),
    (
        "LargeCaps5",
        &["ETHUSDT", "SOLUSDT", "XRPUSDT", "BNBUSDT", "ADAUSDT"],
    ),
    ("Legacy3", &["XRPUSDT", "LTCUSDT", "EOSUSDT"]),
    (
        "LowVolume5",
        &["XRPUSDT", "LTCUSDT", "EOSUSDT", "BCHUSDT", "ADAUSDT"],
    ),
    ("OldGuard4", &["XRPUSDT", "LTCUSDT", "EOSUSDT", "BCHUSDT"]),
];

// Chrono helper — weekend detection from Unix-ms timestamp
fn is_friday(ts: i64) -> bool {
    if let Some(t) = Utc.timestamp_millis_opt(ts).single() {
        t.weekday() == Weekday::Fri
    } else {
        false
    }
}
fn is_weekend(ts: i64) -> bool {
    if let Some(t) = Utc.timestamp_millis_opt(ts).single() {
        matches!(t.weekday(), Weekday::Sat | Weekday::Sun)
    } else {
        false
    }
}

fn col_f64(df: &DataFrame, name: &str) -> Vec<f64> {
    let ca = df
        .column(name)
        .ok()
        .and_then(|c| c.f64().ok())
        .unwrap_or_else(|| panic!("column {} not found", name));
    (0..ca.len()).map(|i| ca.get(i).unwrap_or(0.0)).collect()
}

fn col_i64(df: &DataFrame, name: &str) -> Vec<i64> {
    if let Ok(ca) = df.column(name).and_then(|c| c.i64()) {
        return (0..ca.len()).map(|i| ca.get(i).unwrap_or(0_i64)).collect();
    }
    if let Ok(ca) = df.column(name).and_then(|c| c.datetime()) {
        return (0..ca.len()).map(|i| ca.get(i).unwrap_or(0_i64)).collect();
    }
    panic!("column {} not found as i64 or datetime", name)
}

fn compute_ad_line(high: &[f64], low: &[f64], close: &[f64], volume: &[f64]) -> Vec<f64> {
    let n = high.len();
    let mut ad = vec![0.0; n];
    for i in 0..n {
        let range = high[i] - low[i];
        let mf = if range > 1e-9 {
            ((close[i] - low[i]) - (high[i] - close[i])) / range
        } else {
            0.0
        };
        ad[i] = if i == 0 {
            mf * volume[i]
        } else {
            ad[i - 1] + mf * volume[i]
        };
    }
    ad
}

fn ad_momentum_signal(high: &[f64], low: &[f64], close: &[f64], volume: &[f64]) -> Vec<i32> {
    let ad = compute_ad_line(high, low, close, volume);
    let mut sig = vec![0; ad.len()];
    for i in AD_PERIOD..ad.len() {
        let mom = ad[i] - ad[i - AD_PERIOD];
        sig[i] = if mom > 0.0 {
            1
        } else if mom < 0.0 {
            -1
        } else {
            0
        };
    }
    sig
}

fn macd_regime_signal(
    close: &[f64],
    macd: &[f64],
    macd_signal: &[f64],
    sma200: &[f64],
) -> Vec<i32> {
    let n = close.len();
    let mut sig = vec![0; n];
    // Simple SMA200
    let mut sma = vec![0.0; n];
    for i in 199..n {
        let sum: f64 = close[(i.saturating_sub(199))..=i].iter().sum();
        sma[i] = sum / 200.0;
    }
    for i in 200..n {
        let c = close[i];
        let m = macd[i];
        let s = macd_signal[i];
        let ma = sma[i];
        if c > ma && m > s {
            sig[i] = 1;
        } else if c < ma && m < s {
            sig[i] = -1;
        }
    }
    sig
}

fn small_proxy_signal(volume: &[f64], period: usize) -> Vec<i32> {
    let n = volume.len();
    let mut sig = vec![0; n];
    for i in period..n {
        let window = &volume[(i.saturating_sub(period))..i];
        let sum: f64 = window.iter().sum();
        let avg = sum / period as f64;
        let cur = volume[i];
        sig[i] = if cur < avg * 0.8 {
            1
        } else if cur > avg * 1.2 {
            -1
        } else {
            0
        };
    }
    sig
}

fn three_sleeve_consensus(ad: &[i32], macd: &[i32], small: &[i32]) -> Vec<i32> {
    let n = ad.len().min(macd.len()).min(small.len());
    let mut out = vec![0; n];
    for i in 0..n {
        let sum = ad[i] + macd[i] + small[i];
        out[i] = if sum >= 2 {
            1
        } else if sum <= -2 {
            -1
        } else {
            0
        };
    }
    out
}

fn max_dd(rets: &[f64]) -> f64 {
    let mut peak = 0.0;
    let mut max_dd = 0.0;
    let mut eq = 0.0;
    for &r in rets {
        eq += r;
        if eq > peak {
            peak = eq;
        }
        let dd = peak - eq;
        if dd > max_dd {
            max_dd = dd;
        }
    }
    max_dd
}

struct SliceResult {
    total_ret: f64, // compounded equity multiplier - 1.0
    trades: usize,
    wins: usize,
    dd: f64,            // max drawdown of compounded equity
    fri_rets: Vec<f64>, // net decimal returns
    wd_rets: Vec<f64>,
}

fn run_symbol(
    open: &[f64],
    close_time: &[i64],
    ad_sig: &[i32],
    macd_sig: &[i32],
    small_sig: &[i32],
    start: usize,
    end: usize,
) -> SliceResult {
    let signals = three_sleeve_consensus(ad_sig, macd_sig, small_sig);
    let mut equity = 10000.0;
    let mut peak = equity;
    let mut all_rets = Vec::new();
    let mut fri_rets = Vec::new();
    let mut wd_rets = Vec::new();
    let mut wins = 0;
    let mut i = start
        .max(WARMUP_BARS)
        .min(end.saturating_sub(HOLD_BARS + 2));

    while i + HOLD_BARS + 1 < end {
        let sig = *signals.get(i).unwrap_or(&0);
        if sig == 0 {
            i += 1;
            continue;
        }
        let entry_p = *open.get(i + 1).unwrap_or(&0.0);
        let exit_p = *open.get(i + 1 + HOLD_BARS).unwrap_or(&0.0);
        if entry_p <= 0.0 || exit_p <= 0.0 {
            i += 1;
            continue;
        }
        let gross = if sig > 0 {
            exit_p / entry_p - 1.0
        } else {
            entry_p / exit_p - 1.0
        };
        let net = gross - 2.0 * TAKER_FEE;
        all_rets.push(net);
        if net > 0.0 {
            wins += 1;
        }
        equity *= 1.0 + net;
        if equity > peak {
            peak = equity;
        }
        let entry_time = *close_time.get(i + 1).unwrap_or(&0);
        if is_friday(entry_time) {
            fri_rets.push(net);
        } else if !is_weekend(entry_time) {
            wd_rets.push(net);
        }
        i = i + 1 + HOLD_BARS;
    }

    let total_ret = (equity - 10000.0) / 10000.0; // multiplier - 1
    let dd = (peak - equity) / peak;
    SliceResult {
        total_ret,
        trades: all_rets.len(),
        wins,
        dd,
        fri_rets,
        wd_rets,
    }
}

fn compound_max_dd(rets: &[f64]) -> f64 {
    if rets.is_empty() {
        return 0.0;
    }
    let mut peak = 1.0;
    let mut equity = 1.0;
    let mut max_dd = 0.0;
    for &r in rets {
        equity *= 1.0 + r;
        if equity > peak {
            peak = equity;
        }
        let dd = (peak - equity) / peak;
        if dd > max_dd {
            max_dd = dd;
        }
    }
    max_dd
}

#[tokio::main]
async fn main() -> Result<()> {
    println!("=== Weekend Effect Benchmark ===\n");

    let loader = DataLoader::new(None, None);
    let raw_bench = loader.fetch_with_cache(BENCHMARK, "1d", CANDLES).await?;
    let bench_df = FeatureEngine::add_technicals(&raw_bench, None)?;

    // Load all symbols
    let mut load_symbols = vec![BENCHMARK];
    for &(_, symbols) in UNIVERSES {
        for &sym in symbols {
            if !load_symbols.contains(&sym) {
                load_symbols.push(sym);
            }
        }
    }

    let mut cache: HashMap<String, DataFrame> = HashMap::new();
    cache.insert(BENCHMARK.to_string(), bench_df.clone());
    for &sym in load_symbols.iter().filter(|&&s| s != BENCHMARK) {
        print!("Loading {}... ", sym);
        let raw = loader.fetch_with_cache(sym, "1d", CANDLES).await?;
        let enriched = FeatureEngine::add_technicals(&raw, Some(&bench_df))?;
        println!("{} bars", enriched.height());
        cache.insert(sym.to_string(), enriched);
    }

    // Check train window composition (last 900 bars)
    let train_n = raw_bench.height().saturating_sub(900);
    let train_times: Vec<i64> = col_i64(&raw_bench, "time")
        .into_iter()
        .skip(train_n)
        .collect();
    let total = train_times.len();
    let fri = train_times.iter().filter(|&&d| is_friday(d)).count();
    let wknd = train_times.iter().filter(|&&d| is_weekend(d)).count();
    println!(
        "\nTrain window: {} days | {} Fri | {} weekend | {} weekday\n",
        total,
        fri,
        wknd,
        total - wknd
    );

    for &(label, symbols) in UNIVERSES {
        println!("--- {} ---", label);

        // Build cross-sectional features for universe
        let bench = cache.get(BENCHMARK).unwrap();
        let mut cs_map: HashMap<String, DataFrame> = HashMap::new();
        for &sym in symbols {
            if let Some(df) = cache.get(sym) {
                cs_map.insert(sym.to_string(), df.clone());
            }
        }
        compute_cross_sectional_features(&mut cs_map, CS_LOOKBACK)?;

        // Aggregate results across universe
        let mut all_fri = Vec::new();
        let mut all_wd = Vec::new();

        for &sym in symbols {
            let df = cs_map.get(sym).unwrap();
            let open = col_f64(df, "open");
            let high = col_f64(df, "high");
            let low = col_f64(df, "low");
            let close = col_f64(df, "close");
            let volume = col_f64(df, "volume");
            let close_time = col_i64(df, "time");
            let macd = col_f64(df, "macd");
            let macd_signal = col_f64(df, "macd_signal");

            let ad_sig = ad_momentum_signal(&high, &low, &close, &volume);
            let macd_sig = macd_regime_signal(&close, &macd, &macd_signal, &[]);
            let small_sig = small_proxy_signal(&volume, 63);

            let result = run_symbol(
                &open,
                &close_time,
                &ad_sig,
                &macd_sig,
                &small_sig,
                0,
                df.height(),
            );
            all_fri.extend(result.fri_rets);
            all_wd.extend(result.wd_rets);

            print!(".");
        }
        println!();

        // Combine all fri + wd returns for baseline
        let mut baseline_all: Vec<f64> = Vec::new();
        baseline_all.extend(all_fri.iter().cloned());
        baseline_all.extend(all_wd.iter().cloned());

        // Compute aggregate compounded equity
        fn compound_eq(rets: &[f64]) -> f64 {
            rets.iter().fold(10000.0, |eq, &r| eq * (1.0 + r))
        }
        let base_eq = compound_eq(&baseline_all);
        let base_ret = (base_eq - 10000.0) / 10000.0 * 100.0; // % return
        let base_trades = baseline_all.len();
        let base_wr =
            baseline_all.iter().filter(|&&r| r > 0.0).count() as f64 / base_trades.max(1) as f64;
        let base_dd = compound_max_dd(&baseline_all) * 100.0;

        let fri_eq = compound_eq(&all_fri);
        let fri_ret = (fri_eq - 10000.0) / 10000.0 * 100.0;
        let fri_trades = all_fri.len();
        let fri_wr = all_fri.iter().filter(|&&r| r > 0.0).count() as f64 / fri_trades.max(1) as f64;
        let fri_dd = compound_max_dd(&all_fri) * 100.0;

        let wd_eq = compound_eq(&all_wd);
        let wd_ret = (wd_eq - 10000.0) / 10000.0 * 100.0;
        let wd_trades = all_wd.len();
        let wd_wr = all_wd.iter().filter(|&&r| r > 0.0).count() as f64 / wd_trades.max(1) as f64;
        let wd_dd = compound_max_dd(&all_wd) * 100.0;

        let fri_avg = if fri_trades > 0 {
            fri_ret / fri_trades as f64
        } else {
            0.0
        };
        let wd_avg = if wd_trades > 0 {
            wd_ret / wd_trades as f64
        } else {
            0.0
        };
        let prem = (fri_avg - wd_avg) * 100.0; // bp of avg trade return

        println!(
            "  Baseline: {:>10.1}% | WR {:>5.1}% | DD {:>6.1}% | n {:>5}",
            base_ret,
            base_wr * 100.0,
            base_dd,
            base_trades
        );
        println!(
            "  Friday:   {:>10.1}% | WR {:>5.1}% | DD {:>6.1}% | n {:>5}",
            fri_ret,
            fri_wr * 100.0,
            fri_dd,
            fri_trades
        );
        println!(
            "  Weekday:  {:>10.1}% | WR {:>5.1}% | DD {:>6.1}% | n {:>5}",
            wd_ret,
            wd_wr * 100.0,
            wd_dd,
            wd_trades
        );
        println!("  Weekend premium: {:>+8.1} bp (avg trade return)", prem);

        println!("  [{} | base {:>8.1}% {:>5.1}% {:>6.1}% n={} | fri {:>8.1}% {:>5.1}% {:>6.1}% n={} | wd {:>8.1}% {:>5.1}% {:>6.1}% n={} | prem {:>+7.1}bp]",
            label, base_ret, base_wr*100.0, base_dd, base_trades,
            fri_ret, fri_wr*100.0, fri_dd, fri_trades,
            wd_ret, wd_wr*100.0, wd_dd, wd_trades,
            prem);
    }

    println!("\n\n=== SUMMARY ===");
    println!("Interpretation:");
    println!("- Consistent positive Friday premium across hostile baskets = weekend conditioning earns follow-up");
    println!("- Weak/brittle premium on hostile baskets = Sep 2025 effect may be modern-cap artifact only");
    println!(
        "- Weekend effect only matters if it survives chronology stress, not just full-sample"
    );

    Ok(())
}
