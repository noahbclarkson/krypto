//! T11: Failure Mode Diagnostic
//!
//! Question: 13/54 global windows fail. HOW MANY are purely asset-specific (LTC/EOS/BCH)
//! vs regime-wide (could affect BTC/ETH/SOL in the production universe)?
//!
//! Production params: EP=21, CP=7, CM=2.30, HM=12, ATR=24
//! Run with: cargo run --example t11_failure_mode_diagnostic --profile sweep

use anyhow::Result;
use krypto::data::loader::DataLoader;
use polars::prelude::*;
use std::collections::HashMap;
use std::time::Instant;

const CANDLES: u32 = 3000;
const TURTLE_ENTRY: usize = 21;
const CHAND_PERIOD: usize = 7;
const CHAND_MULT: f64 = 2.30;
const HOLD_MAX: usize = 12;
const TURTLE_ATR_PERIOD: usize = 24;
const TURTLE_ATR_MULT: f64 = 2.0;
const ATR_ENTRY_MULT: f64 = 0.0;
const TAKER_FEE: f64 = 0.0004;
const SLIPPAGE_BPS: f64 = 10.0;
const FRESHNESS_COOLDOWN: usize = 0;
const TRAIN_BARS: usize = 252;
const TEST_BARS: usize = 252;

const UNIVERSES: &[(&str, &[&str])] = &[
    ("Base5",       &["BTCUSDT","ETHUSDT","SOLUSDT","XRPUSDT","DOGEUSDT","ADAUSDT"]),
    ("NoDOGE",      &["BTCUSDT","ETHUSDT","SOLUSDT","XRPUSDT","ADAUSDT"]),
    ("LargeCaps5",  &["BTCUSDT","ETHUSDT","SOLUSDT","XRPUSDT","BNBUSDT","ADAUSDT"]),
    ("LowVolume5",  &["XRPUSDT","LTCUSDT","EOSUSDT","BCHUSDT","ADAUSDT"]),
    ("Legacy4",     &["BTCUSDT","ETHUSDT","XRPUSDT","LTCUSDT"]),
    ("Legacy3",     &["BTCUSDT","XRPUSDT","LTCUSDT"]),
    ("OldGuard",    &["BTCUSDT","ETHUSDT","XRPUSDT","LTCUSDT","EOSUSDT"]),
    ("Legacy5BNB",  &["BTCUSDT","ETHUSDT","XRPUSDT","LTCUSDT","BNBUSDT","EOSUSDT"]),
    ("DOGEOnly",    &["BTCUSDT","ETHUSDT","SOLUSDT","XRPUSDT","DOGEUSDT"]),
];

struct SymData { close: Vec<f64>, high: Vec<f64>, low: Vec<f64>, vol: Vec<f64> }

fn atr_at(high: &[f64], low: &[f64], close: &[f64], period: usize, idx: usize) -> f64 {
    if idx < period { return 0.0; }
    let mut trs = Vec::with_capacity(period);
    for i in (idx + 1 - period)..=idx {
        let h = *high.get(i).unwrap_or(&0.0);
        let l = *low.get(i).unwrap_or(&0.0);
        let c0 = *close.get(i.saturating_sub(1)).unwrap_or(&0.0);
        trs.push((h - l).max((h - c0).abs()).max((l - c0).abs()));
    }
    if trs.is_empty() { return 0.0; }
    trs.iter().sum::<f64>() / period as f64
}

fn turtle_signal_raw(close: &[f64], idx: usize) -> bool {
    if idx < TURTLE_ENTRY + 1 { return false; }
    let start = idx + 1 - TURTLE_ENTRY;
    let mut max_close = f64::NEG_INFINITY;
    for i in start..idx {
        if let Some(&c) = close.get(i) { max_close = max_close.max(c); }
    }
    close.get(idx).map_or(false, |&c| c > max_close)
}

fn annualised_sharpe(daily_rets: &[f64]) -> f64 {
    if daily_rets.len() < 2 { return 0.0; }
    let mn: f64 = daily_rets.iter().sum::<f64>() / daily_rets.len() as f64;
    let sd = (daily_rets.iter().map(|x| (x - mn).powi(2)).sum::<f64>() / daily_rets.len() as f64).sqrt();
    if sd == 0.0 { return 0.0; }
    mn * 365.0_f64.sqrt() / sd
}

struct SymResult { pass: bool, sharpe: f64, trades: usize }

fn run_symbol(sd: &SymData, test_start: usize, test_end: usize) -> SymResult {
    let mut equity = 1.0_f64;
    let mut daily_rets = Vec::new();
    let mut wins = 0usize;
    let mut total_trades = 0usize;
    let mut last_exit_bar = -999i64;
    let mut position = 0.0_f64;
    let mut entry_bar = 0usize;
    let mut entry_px = 0.0_f64;

    let mut bar = test_start;
    while bar + 2 < test_end {
        if position <= 0.0 {
            if turtle_signal_raw(&sd.close, bar) {
                if (bar as i64 - last_exit_bar) as usize > FRESHNESS_COOLDOWN {
                    position = 1.0;
                    entry_bar = bar;
                    entry_px = sd.close[bar] * (1.0 + TAKER_FEE + SLIPPAGE_BPS / 10000.0);
                }
            }
            bar += 1;
            continue;
        }

        // Exit logic
        let atr_chand = atr_at(&sd.high, &sd.low, &sd.close, CHAND_PERIOD, bar);
        let atr_turtle = atr_at(&sd.high, &sd.low, &sd.close, TURTLE_ATR_PERIOD, bar);
        let mut highest_high = sd.high[entry_bar..=bar].iter().copied().fold(0.0_f64, f64::max);
        let mut lowest_low = sd.low[entry_bar..=bar].iter().copied().fold(f64::MAX, f64::min);
        let trail_chand = highest_high - CHAND_MULT * atr_chand;
        let trail_turtle = lowest_low - TURTLE_ATR_MULT * atr_turtle;
        let stop = trail_chand.max(trail_turtle);
        let bars_held = bar.saturating_sub(entry_bar);

        if sd.close[bar] < stop || bars_held >= HOLD_MAX {
            let exit_px = sd.close[bar] * (1.0 - TAKER_FEE - SLIPPAGE_BPS / 10000.0);
            let gross_ret = exit_px / entry_px - 1.0;
            wins += if gross_ret > 0.0 { 1 } else { 0 };
            total_trades += 1;
            equity *= 1.0 + gross_ret;
            let avg_daily = if bars_held > 0 { gross_ret / bars_held as f64 } else { 0.0 };
            for _ in 0..bars_held { daily_rets.push(avg_daily); }
            last_exit_bar = bar as i64;
            position = 0.0;
        }
        bar += 1;
    }

    let sharpe = annualised_sharpe(&daily_rets);
    let pass = sharpe > 0.0 && total_trades >= 5;
    SymResult { pass, sharpe, trades: total_trades }
}

#[tokio::main]
async fn main() -> Result<()> {
    let t0 = Instant::now();
    println!("=== T11: Failure Mode Diagnostic ===");
    println!("Params: EP={}, CP={}, CM={}, HM={}, ATR={}",
             TURTLE_ENTRY, CHAND_PERIOD, CHAND_MULT, HOLD_MAX, TURTLE_ATR_PERIOD);
    println!();

    let loader = DataLoader::new(None, None);
    let mut all_syms: std::collections::HashSet<String> = std::collections::HashSet::new();
    for (_, syms) in UNIVERSES {
        for &s in *syms { all_syms.insert(s.to_string()); }
    }

    // Load all data
    let mut raw_cache: HashMap<String, DataFrame> = HashMap::new();
    for sym in all_syms.iter() {
        match loader.fetch_with_cache(sym.as_str(), "1d", CANDLES).await {
            Ok(df) => { raw_cache.insert(sym.clone(), df); }
            Err(e) => { eprintln!("  WARNING: {} load failed: {}", sym, e); }
        }
    }

    // Build SymData
    let n = raw_cache.values().map(|df| df.height()).min().unwrap_or(0).min(2800);
    let mut sym_data_map: HashMap<String, SymData> = HashMap::new();
    for sym in &all_syms {
        if let Some(df) = raw_cache.get(sym) {
            macro_rules! col_vec {
                ($name:expr) => {{
                    let chunked = df.column($name)?.f64()?;
                    chunked.into_iter().filter_map(|x| x).take(n).collect::<Vec<_>>()
                }};
            }
            sym_data_map.insert(sym.clone(), SymData {
                close: col_vec!("close"), high: col_vec!("high"),
                low: col_vec!("low"),    vol: col_vec!("volume"),
            });
        }
    }
    eprintln!("Loaded {} symbols, {} bars\n", sym_data_map.len(), n);

    let prod_syms = ["BTCFDUSD","ETHFDUSD","SOLFDUSD","XRPFDUSD","DOGEFDUSD","ADAFDUSD"];
    let non_prod_syms = ["LTCFDUSD","EOSFDUSD","BCHFDUSD","BNBUSD","ADAUSD"];

    let mut all_results: HashMap<String, Vec<(String,bool,f64,usize)>> = HashMap::new();
    let mut global_fails = 0usize;
    let mut regime_fails = 0usize;
    let mut asset_fails = 0usize;

    for &(label, symbols) in UNIVERSES {
        let total_windows = n.saturating_sub(TRAIN_BARS + TEST_BARS) / TEST_BARS;
        let loaded = symbols.iter().filter(|s| sym_data_map.get(s.to_string().as_str()).is_some()).count();

        for window in 0..total_windows.min(6) {
            let window_key = format!("{}_W{:02}", label, window);
            let test_start = TRAIN_BARS * (window + 1);
            let test_end = (test_start + TEST_BARS).min(n);

            let mut sym_results: Vec<(String,bool,f64,usize)> = Vec::new();

            for &sym in symbols {
                if let Some(sd) = sym_data_map.get(sym) {
                    let result = run_symbol(sd, test_start, test_end);
                    sym_results.push((sym.to_string(), result.pass, result.sharpe, result.trades));
                } else {
                    sym_results.push((sym.to_string(), false, f64::NEG_INFINITY, 0));
                }
            }

            let pass_count = sym_results.iter().filter(|r| r.1).count();
            let overall_pass = pass_count > sym_results.len() / 2;

            let flag = if overall_pass { "PASS" } else { "FAIL" };
            eprintln!("{}/W{:02}: {} ({}/{} syms pass)", label, window, flag, pass_count, sym_results.len());

            if !overall_pass {
                global_fails += 1;
                let prod_fails: Vec<_> = sym_results.iter()
                    .filter(|(s,p,_,_)| prod_syms.contains(&s.as_str()) && !p).collect();
                let non_prod_fails: Vec<_> = sym_results.iter()
                    .filter(|(s,p,_,_)| non_prod_syms.contains(&s.as_str()) && !p).collect();

                if prod_fails.is_empty() {
                    asset_fails += 1;
                    println!("\n[ASSET-SPECIFIC] {}", window_key);
                    println!("  Production universe: ALL PASS");
                    println!("  Asset failures: {:?}", prod_fails.iter().map(|r| r.0.as_str()).collect::<Vec<_>>());
                } else {
                    regime_fails += 1;
                    println!("\n[REGIME-WIDE ⚠️] {}", window_key);
                    println!("  Production failures: {:?}", prod_fails.iter().map(|r| r.0.as_str()).collect::<Vec<_>>());
                }

                for (s,p,sh,_) in &sym_results {
                    let icon = if prod_syms.contains(&s.as_str()) {
                        if *p { "🟢" } else { "🔴" }
                    } else {
                        if *p { "⚪" } else { "🟠" }
                    };
                    println!("  {}{} {} Sharpe {:+.2}", icon, if *p { "PASS" } else { "FAIL" }, s, sh);
                }
            }

            all_results.insert(window_key, sym_results);
        }
    }

    println!("\n\n=== SUMMARY ===");
    println!("Total failing windows: {}", global_fails);
    println!("  Asset-specific (only LTC/EOS/BCH fail): {}", asset_fails);
    println!("  Regime-wide (production symbols fail too): {}", regime_fails);
    println!("\nRuntime: {:.1}s", t0.elapsed().as_secs_f64());

    if regime_fails == 0 {
        println!("\n✅ VERDICT: ALL failures are non-production-asset-specific.");
        println!("   Production universe is CLEAN. CTREND sleeve = optional diversification only.");
    } else {
        println!("\n⚠️  VERDICT: {} regime-wide failures detected.", regime_fails);
        println!("   CTREND sleeve is URGENT as portfolio protection.");
    }

    Ok(())
}
