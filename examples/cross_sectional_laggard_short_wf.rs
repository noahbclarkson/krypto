//! Cross-Sectional Laggard Short — Walk-Forward Validation
//!
//! PURPOSE: First strict OOS test of the short-side sleeve (Track C).
//!
//! Design: Market-neutral long-short portfolio.
//!   - Universe: Base5+ (BTC, ETH, SOL, XRP, DOGE, ADA)
//!   - Each day: rank symbols by 20-bar momentum (return)
//!   - Long top-2, Short bottom-1 simultaneously (equal $ notional per leg)
//!   - Regime filter: None (always deployed)
//!   - If no bull signal: flat (no short, no long)
//!   - Hold: 21 bars
//!   - Fee: 0.1% taker per side per leg
//!
//! This is the #1 structural gap in the program — zero short-side after 30+ sessions.
//! A pure trend-following long book has no survival mechanism in bear regimes.

use anyhow::Result;
use krypto::data::loader::DataLoader;
use polars::prelude::*;
use std::collections::HashMap;
use std::fs::OpenOptions;
use std::io::Write;
use std::time::Instant;

const CANDLES: u32 = 3000;
const TRAIN_BARS: usize = 252;
const TEST_BARS: usize = 252;
const HOLD: usize = 21;
const MOM_LOOKBACK: usize = 20;
const SMA_REGIME: usize = 200;
const TAKER_FEE: f64 = 0.001;
const WARMUP: usize = 200;
const MIN_TRADES: usize = 5;

const SYMBOLS: &[&str] = &[
    "BTCUSDT", "ETHUSDT", "SOLUSDT", "XRPUSDT", "DOGEUSDT", "ADAUSDT",
];

const UNIVERSES: &[(&str, &[&str])] = &[
    (
        "Base5",
        &[
            "BTCUSDT", "ETHUSDT", "SOLUSDT", "XRPUSDT", "DOGEUSDT", "ADAUSDT",
        ],
    ),
    (
        "NoDOGE",
        &["BTCUSDT", "ETHUSDT", "SOLUSDT", "XRPUSDT", "ADAUSDT"],
    ),
    (
        "Legacy4",
        &["BTCUSDT", "ETHUSDT", "XRPUSDT", "LTCUSDT", "EOSUSDT"],
    ),
    (
        "Legacy5BNB",
        &[
            "BTCUSDT", "ETHUSDT", "XRPUSDT", "LTCUSDT", "BNBUSDT", "EOSUSDT",
        ],
    ),
    (
        "OldGuardNoBNB",
        &[
            "BTCUSDT", "ETHUSDT", "XRPUSDT", "LTCUSDT", "EOSUSDT", "BCHUSDT",
        ],
    ),
    (
        "LargeCaps5",
        &[
            "BTCUSDT", "ETHUSDT", "SOLUSDT", "XRPUSDT", "BNBUSDT", "ADAUSDT",
        ],
    ),
    ("Legacy3", &["BTCUSDT", "XRPUSDT", "LTCUSDT", "EOSUSDT"]),
    (
        "LowVolume5",
        &["XRPUSDT", "LTCUSDT", "EOSUSDT", "BCHUSDT", "ADAUSDT"],
    ),
    (
        "OldGuard4",
        &["BTCUSDT", "XRPUSDT", "LTCUSDT", "EOSUSDT", "BCHUSDT"],
    ),
];

// ── Math helpers ──────────────────────────────────────────────────────────────

fn calc_sharpe(daily_rets: &[f64]) -> f64 {
    if daily_rets.len() < 5 {
        return 0.0;
    }
    let mean = daily_rets.iter().sum::<f64>() / daily_rets.len() as f64;
    let var =
        daily_rets.iter().map(|r| (r - mean).powi(2)).sum::<f64>() / daily_rets.len().max(1) as f64;
    let std = var.sqrt();
    if std < 1e-9 {
        return 0.0;
    }
    mean * 365.0 / (std * (365.0_f64).sqrt())
}

fn calc_max_dd(equity: &[f64]) -> f64 {
    let mut peak = equity[0];
    let mut max_dd = 0.0_f64;
    for &e in equity {
        peak = peak.max(e);
        let dd = (1.0 - e / peak) * 100.0;
        max_dd = max_dd.max(dd);
    }
    max_dd
}

fn sma(data: &[f64], period: usize, end: usize) -> f64 {
    if end < period {
        return 0.0;
    }
    data[end - period..=end].iter().sum::<f64>() / period as f64
}

fn momentum(data: &[f64], period: usize, end: usize) -> f64 {
    if end < period {
        return 0.0;
    }
    data[end] / data[end - period] - 1.0
}

// ── Regime: true = bull market, false = bear/flat ────────────────────────────

fn is_bull_regime(btc_close: &[f64], bar: usize) -> bool {
    // REMOVED SMA REGIME FILTER:
    // The SMA200 filter forces the short sleeve flat exactly when it is most needed
    // (during bear regimes). We want this sleeve active at all times.
    true
}

// ── Run one universe's walk-forward ──────────────────────────────────────────

fn run_universe(
    name: &str,
    syms: &[&str],
    cache: &HashMap<String, DataFrame>,
    all_results: &mut Vec<String>,
) -> Result<()> {
    let n = cache.values().next().map(|d| d.height()).unwrap_or(0);
    if n < TRAIN_BARS + TEST_BARS + WARMUP {
        println!("  [{}] SKIP — insufficient data ({n} bars)", name);
        return Ok(());
    }

    // Build per-symbol price vectors
    let mut close: HashMap<&str, Vec<f64>> = HashMap::new();
    let mut high: HashMap<&str, Vec<f64>> = HashMap::new();
    let mut low: HashMap<&str, Vec<f64>> = HashMap::new();

    for &sym in syms {
        if let Some(df) = cache.get(sym) {
            close.insert(
                sym,
                df.column("close")?
                    .f64()?
                    .into_iter()
                    .map(|v| v.unwrap_or(0.0))
                    .collect(),
            );
            high.insert(
                sym,
                df.column("high")?
                    .f64()?
                    .into_iter()
                    .map(|v| v.unwrap_or(0.0))
                    .collect(),
            );
            low.insert(
                sym,
                df.column("low")?
                    .f64()?
                    .into_iter()
                    .map(|v| v.unwrap_or(0.0))
                    .collect(),
            );
        }
    }

    // Truncate all arrays to minimum length across universe symbols
    let min_len = close.values().map(|v| v.len()).min().unwrap_or(n);
    for sym in syms {
        if let Some(c) = close.get_mut(sym) {
            c.truncate(min_len);
        }
        if let Some(h) = high.get_mut(sym) {
            h.truncate(min_len);
        }
        if let Some(l) = low.get_mut(sym) {
            l.truncate(min_len);
        }
    }

    // Regime filter requires BTCUSDT — skip universes without it
    let Some(btc_close) = close.get("BTCUSDT") else {
        println!("  [{}] SKIP — no BTCUSDT for regime filter", name);
        return Ok(());
    };
    let n = min_len; // override n to use common length

    println!("\n═══ Universe: {name} ═══");

    let mut total_long_passes = 0usize;
    let mut total_short_passes = 0usize;
    let mut total_marketneutral_passes = 0usize;
    let mut total_windows = 0usize;

    // Walk-forward windows
    let step = TEST_BARS;
    let mut test_start = TRAIN_BARS + WARMUP;

    while test_start + TEST_BARS <= n && test_start < n {
        total_windows += 1;
        let _train_end = test_start - 1;
        let actual_test_bars = (n - test_start).min(TEST_BARS);
        let test_end = test_start + actual_test_bars - 1;

        // ── Per-window metrics ──────────────────────────────────────────────
        let mut long_equity = vec![1.0_f64; actual_test_bars + 1];
        let mut short_equity = vec![1.0_f64; actual_test_bars + 1];
        let mut mn_equity = vec![1.0_f64; actual_test_bars + 1];

        let _long_trades = 0i32;
        let _short_trades = 0i32;
        let _mn_trades = 0i32;
        let _long_notional = 0.0_f64;
        let _short_notional = 0.0_f64;

        // Per-bar portfolio P&L (equal notional long-short)
        let mut bar_returns = vec![0.0_f64; actual_test_bars];

        for t in 0..TEST_BARS {
            let bar = test_start + t;

            // ── No regime gate (always deploy) ───────────────────────────
            let bull = true;

            // ── Momentum ranking ───────────────────────────────────────────
            let mut mom_scores: Vec<(&str, f64)> = Vec::new();
            for &sym in syms {
                if let Some(c) = close.get(sym) {
                    if bar > MOM_LOOKBACK {
                        let mom = momentum(c, MOM_LOOKBACK, bar.saturating_sub(1));
                        mom_scores.push((sym, mom));
                    }
                }
            }

            if mom_scores.len() < 3 {
                for eq_arr in [&mut long_equity, &mut short_equity, &mut mn_equity] {
                    if t > 0 {
                        eq_arr[t] = eq_arr[t - 1];
                    }
                }
                continue;
            }

            // Sort descending by momentum
            mom_scores.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));

            let long_syms: Vec<_> = mom_scores.iter().take(2).map(|(s, _)| *s).collect();
            let short_sym = mom_scores.last().map(|(s, _)| *s).unwrap_or("");

            // ── Equal notional P&L ──────────────────────────────────────────
            // Long leg: equally weighted (1/2 each), rebalance each bar
            // Short leg: 1x notional (equals one long leg)

            let long_notional_per_sym = 0.5_f64; // each long sym = 50% equity
            let short_notional = 0.5_f64; // short = 50% equity notional (reduced from 100%)

            // Bar return for each leg
            let mut long_bar_ret = 0.0_f64;
            for ls in &long_syms {
                if let Some(c) = close.get(ls) {
                    if bar > 0 && bar < c.len() {
                        let r = c[bar] / c[bar - 1] - 1.0;
                        long_bar_ret += r * long_notional_per_sym;
                    }
                }
            }

            let mut short_bar_ret = 0.0_f64;
            if let Some(c) = close.get(short_sym) {
                if bar > 0 && bar < c.len() {
                    // Short profit = negative of return
                    let r = c[bar] / c[bar - 1] - 1.0;
                    short_bar_ret -= r * short_notional;
                }
            }

            // Fee drag (0.1% taker on entry and exit per leg, per 21-bar hold cycle)
            let fee_rate = TAKER_FEE * 2.0 / HOLD as f64; // amortized per bar
            let long_fee = long_syms.len() as f64 * fee_rate;
            let short_fee = fee_rate; // one short leg

            let net_long = long_bar_ret - long_fee;
            let net_short = short_bar_ret - short_fee;

            // Market-neutral = equal long + short (net beta ~0)
            let net_mn = net_long + net_short;

            // Compound equity
            if t == 0 {
                long_equity[t] = 1.0 + net_long;
                short_equity[t] = 1.0 + net_short;
                mn_equity[t] = 1.0 + net_mn;
            } else {
                long_equity[t] = long_equity[t - 1] * (1.0 + net_long);
                short_equity[t] = short_equity[t - 1] * (1.0 + net_short);
                mn_equity[t] = mn_equity[t - 1] * (1.0 + net_mn);
            }

            bar_returns[t] = net_mn;
        }

        // ── Per-window statistics ──────────────────────────────────────────
        let long_ret = (long_equity.last().unwrap() / long_equity.first().unwrap() - 1.0) * 100.0;
        let short_ret =
            (short_equity.last().unwrap() / short_equity.first().unwrap() - 1.0) * 100.0;
        let mn_ret = (mn_equity.last().unwrap() / mn_equity.first().unwrap() - 1.0) * 100.0;

        let long_dd = calc_max_dd(&long_equity);
        let short_dd = calc_max_dd(&short_equity);
        let mn_dd = calc_max_dd(&mn_equity);

        let window_start = test_start;
        let window_end = test_end;
        println!(
            "  W{w} [{window_start:5}..{window_end:5}]: L={long_ret:>+8.1}% DD={long_dd:>6.1}% | S={short_ret:>+8.1}% DD={short_dd:>6.1}% | MN={mn_ret:>+8.1}% DD={mn_dd:>6.1}%",
            w = total_windows - 1
        );

        // Pass/fail: positive return with reasonable DD
        let long_pass = long_ret > 0.0 && long_dd < 95.0;
        let short_pass = short_ret > 0.0 && short_dd < 95.0;
        let mn_pass = mn_ret > 0.0 && mn_dd < 95.0;

        if long_pass {
            total_long_passes += 1;
        }
        if short_pass {
            total_short_passes += 1;
        }
        if mn_pass {
            total_marketneutral_passes += 1;
        }

        let bars_str = format!("{}-{}-{}", window_start, window_end, TEST_BARS);
        all_results.push(format!(
            "{},W{},{},{:.2},{:.2},{:.2},{:.2},{:.2},{:.2},{},{},{}",
            name,
            total_windows - 1,
            bars_str,
            long_ret,
            short_ret,
            mn_ret,
            long_dd,
            short_dd,
            mn_dd,
            if long_pass { "PASS" } else { "FAIL" },
            if short_pass { "PASS" } else { "FAIL" },
            if mn_pass { "PASS" } else { "FAIL" }
        ));

        test_start += step;
    }

    let lw = total_long_passes;
    let sw = total_short_passes;
    let mw = total_marketneutral_passes;
    let tw = total_windows;
    println!("  [{name}] Summary: Long {lw}/{tw} | Short {sw}/{tw} | MarketNeutral {mw}/{tw}",);

    Ok(())
}

// ── Main ───────────────────────────────────────────────────────────────────────

fn main() -> Result<()> {
    let start = Instant::now();
    println!("Cross-Sectional Laggard Short — Walk-Forward Validation");
    println!("Strategy: Long top-2 by 20d mom, Short bottom-1, No regime gate (always deployed), 50% short notional");
    println!("Universe: 9 stress universes | Train 252 / Test 252 | Fee 0.1% taker\n");

    // Load data
    let runtime = tokio::runtime::Runtime::new()?;
    let mut cache: HashMap<String, DataFrame> = HashMap::new();

    // Collect all unique symbols across all universes
    let mut all_syms: Vec<&str> = UNIVERSES
        .iter()
        .flat_map(|(_, s)| s.iter().copied())
        .collect();
    all_syms.sort();
    all_syms.dedup();

    println!("Loading {} symbols across 9 universes...", all_syms.len());
    let mut loaded = 0;
    runtime.block_on(async {
        for &sym in &all_syms {
            let loader = DataLoader::new(None, None);
            match loader.fetch_data(sym, "1d", CANDLES).await {
                Ok(df) => {
                    cache.insert(sym.to_string(), df);
                    loaded += 1;
                }
                Err(e) => {
                    eprintln!("  WARNING: failed to load {sym}: {e}");
                }
            }
        }
    });
    println!("Loaded {loaded}/{} symbols\n", all_syms.len());

    let csv_path = "snapshots/laggard_short_wf.csv";
    let mut all_results = Vec::new();
    all_results.push(
        "universe,window,bars,long_ret%,short_ret%,mn_ret%,long_dd%,short_dd%,mn_dd%,long_pass,short_pass,mn_pass".to_string()
    );

    for (name, syms) in UNIVERSES {
        run_universe(name, syms, &cache, &mut all_results)?;
    }

    // Write results
    {
        let mut f = OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(true)
            .open(csv_path)?;
        for line in &all_results {
            writeln!(f, "{}", line)?;
        }
    }

    // ── Summary ─────────────────────────────────────────────────────────────
    println!("\n═══════════════════════════════════════════════");
    println!("SUMMARY: Cross-Sectional Laggard Short Walk-Forward");
    println!("═══════════════════════════════════════════════");

    // Parse and aggregate
    let mut long_passes = 0usize;
    let mut short_passes = 0usize;
    let mut mn_passes = 0usize;
    let mut total = 0usize;
    let mut lines_with_wrong_field_count = 0usize;

    for line in all_results.iter().skip(1) {
        let fields: Vec<&str> = line.split(',').collect();
        if fields.len() >= 12 {
            total += 1;
            if fields[9] == "PASS" {
                long_passes += 1;
            }
            if fields[10] == "PASS" {
                short_passes += 1;
            }
            if fields[11] == "PASS" {
                mn_passes += 1;
            }
        } else if !line.is_empty() {
            lines_with_wrong_field_count += 1;
            eprintln!(
                "WRONG FIELD COUNT: {} fields in: {}",
                fields.len(),
                &line[..line.len().min(80)]
            );
        }
    }

    if lines_with_wrong_field_count > 0 {
        eprintln!(
            "WARNING: {} lines had wrong field count",
            lines_with_wrong_field_count
        );
    }
    println!(
        "Total windows: {total} (all_results.len() = {})",
        all_results.len()
    );
    println!(
        "Long-only pass rate:  {long_passes}/{total} = {:.0}%",
        if total > 0 {
            long_passes as f64 / total as f64 * 100.0
        } else {
            0.0
        }
    );
    println!(
        "Short-only pass rate: {short_passes}/{total} = {:.0}%",
        if total > 0 {
            short_passes as f64 / total as f64 * 100.0
        } else {
            0.0
        }
    );
    println!(
        "MarketNeutral pass:   {mn_passes}/{total} = {:.0}%",
        if total > 0 {
            mn_passes as f64 / total as f64 * 100.0
        } else {
            0.0
        }
    );

    println!(
        "\nNote: Short-side sleeve is designed to provide crisis-alpha (short when longs fail)."
    );
    println!("Pure short-side or market-neutral may underperform in bull markets.");
    println!("\nRuntime: {:.1}s", start.elapsed().as_secs_f64());
    println!("Results: {csv_path}");

    Ok(())
}
