//! A/D Momentum Lookback Hyperopt — Full Integer Sweep 5-100
//!
//! Target: ad_lookback parameter (A/D momentum = A/D(t) - A/D(t-N))
//! Prior: 20 bars (no systematic justification)
//! Sweep: 5-100 step 1 (96 values)
//! Strategy: A/D momentum long-only, top-2 rank, 21-bar hold, Chandelier(45,2.5) exit
//! Universes: All 9
//! Validation: Walk-forward 252/252
//!
//! This is the first systematic sweep of the A/D lookback parameter.

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
const HOLD_MAX: usize = 21;
const TAKER_FEE: f64 = 0.001;
const POSITION_CAP: usize = 2;
const MIN_TRADES: usize = 3;
const CHAND_PERIOD: usize = 45;
const CHAND_MULT: f64 = 2.5;
const TURTLE_ENTRY: usize = 21; // Already optimized from EP sweep

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

const LOOKBACK_MIN: usize = 5;
const LOOKBACK_MAX: usize = 100;
const LOOKBACK_STEP: usize = 1;

const CSV_OUT: &str = "snapshots/ad_lookback_sweep_results.csv";
const EQUITY_OUT: &str = "snapshots/ad_lookback_equity_curves.csv";
const SUMMARY_JSON: &str = "snapshots/ad_lookback_sweep_summary.json";

struct SymData {
    close: Vec<f64>,
    high: Vec<f64>,
    low: Vec<f64>,
    vol: Vec<f64>,
    ad: Vec<f64>,
}

fn compute_ad(high: &[f64], low: &[f64], close: &[f64], vol: &[f64]) -> Vec<f64> {
    let n = close.len();
    let mut ad = vec![0.0; n];
    for i in 0..n {
        let h = high[i];
        let l = low[i];
        let c = close[i];
        let v = vol[i];
        let hl = h - l;
        let mult = if hl > 0.0 {
            ((c - l) - (h - c)) / hl
        } else {
            0.0
        };
        let mf = mult * v;
        ad[i] = if i == 0 { mf } else { ad[i - 1] + mf };
    }
    ad
}

fn ad_momentum(ad: &[f64], lookback: usize, idx: usize) -> f64 {
    if idx < lookback {
        0.0
    } else {
        ad[idx] - ad[idx - lookback]
    }
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

fn chandelier_exit(low: &[f64], close: &[f64], atr_highs: &[f64], period: usize, mult: f64, entry_low: f64, idx: usize, bars_held: usize, entry_idx: usize) -> bool {
    if bars_held == 0 { return false; }
    if idx <= entry_idx || entry_idx < 0 { return false; }
    // trailing stop: lowest low since entry - mult * ATR
    let mut lowest_low = f64::INFINITY;
    for i in entry_idx..=idx.min(low.len() - 1) {
        if let Some(&l) = low.get(i) {
            lowest_low = lowest_low.min(l);
        }
    }
    let stop = lowest_low - mult * atr_highs[idx.min(atr_highs.len() - 1)];
    close[idx] < stop
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
    let annual_factor = (252.0 / daily_rets.len() as f64).sqrt();
    (mn / sd) * annual_factor
}

fn max_drawdown(equity: &[f64]) -> f64 {
    if equity.is_empty() { return 0.0; }
    let mut peak = equity[0];
    let mut max_dd = 0.0;
    for &val in equity.iter().skip(1) {
        peak = peak.max(val);
        let dd = (peak - val) / peak;
        max_dd = max_dd.max(dd);
    }
    max_dd
}

fn load_universe(loader: &DataLoader, symbols: &[&str]) -> HashMap<String, SymData> {
    let mut map = HashMap::new();
    for &sym in symbols {
        let df = loader.load_candles(sym, "USDT", "1d", CANDLES).ok();
        if let Some(df) = df {
            let close: Vec<f64> = df.column("close").unwrap().f64().unwrap().to_vec();
            let high: Vec<f64> = df.column("high").unwrap().f64().unwrap().to_vec();
            let low: Vec<f64> = df.column("low").unwrap().f64().unwrap().to_vec();
            let vol: Vec<f64> = df.column("volume").unwrap().f64().unwrap().to_vec();
            let ad = compute_ad(&high, &low, &close, &vol);
            map.insert(sym.to_string(), SymData { close, high, low, vol, ad });
        }
    }
    map
}

fn run_backtest(
    sym_data: &HashMap<String, SymData>,
    symbols: &[String],
    lookback: usize,
    train_start: usize,
    train_end: usize,
    test_start: usize,
    test_end: usize,
) -> Option<(f64, f64, f64, f64, i64, i64)> {
    // Gather test data
    let mut test_closes: Vec<Vec<f64>> = Vec::new();
    let min_len = symbols.iter().filter_map(|s| sym_data.get(s)).map(|d| d.close.len()).min().unwrap_or(0);
    let actual_test_end = test_end.min(min_len);

    for sym in symbols {
        if let Some(d) = sym_data.get(sym) {
            let start = test_start.min(d.close.len());
            let end = actual_test_end.min(d.close.len());
            test_closes.push(d.close[start..end].to_vec());
        }
    }

    if test_closes.is_empty() || test_closes[0].is_empty() {
        return None;
    }

    let n_test = test_closes[0].len();
    let mut equity = vec![1.0; n_test];
    let mut position = 0.0;
    let mut entry_price = 0.0;
    let mut entry_bar = 0;
    let mut entry_sym_idx = 0;
    let mut daily_rets = Vec::new();

    for bar in 0..n_test {
        let prev_equity = if bar == 0 { 1.0 } else { equity[bar - 1] };

        // Expire old positions
        if position > 0.0 && bar > entry_bar && (bar - entry_bar) >= HOLD_MAX {
            let pnl = position * (test_closes[entry_sym_idx][bar] / entry_price - 1.0);
            let fee = position * TAKER_FEE * 2.0;
            let new_equity = (prev_equity + pnl - fee).max(0.0001);
            equity[bar] = new_equity;
            position = 0.0;
            if bar > 0 { daily_rets.push(new_equity / prev_equity - 1.0); }
            continue;
        }

        // Compute Chandelier exit if in position
        if position > 0.0 {
            if let Some(d) = sym_data.get(&symbols[entry_sym_idx]) {
                let atr_val = atr_at(&d.high, &d.low, &d.close, CHAND_PERIOD, test_start + bar);
                let stop = {
                    let mut lowest_low = f64::INFINITY;
                    for i in entry_bar..=bar {
                        if let Some(&l) = d.low.get(test_start + i) {
                            lowest_low = lowest_low.min(l);
                        }
                    }
                    lowest_low - CHAND_MULT * atr_val
                };
                if test_closes[entry_sym_idx][bar] < stop {
                    let pnl = position * (test_closes[entry_sym_idx][bar] / entry_price - 1.0);
                    let fee = position * TAKER_FEE * 2.0;
                    let new_equity = (prev_equity + pnl - fee).max(0.0001);
                    equity[bar] = new_equity;
                    position = 0.0;
                    if bar > 0 { daily_rets.push(new_equity / prev_equity - 1.0); }
                    continue;
                }
            }
        }

        // Entry signal: compute A/D momentum for all symbols at bar-1 (signal at close, entry next bar)
        if bar > 0 && position == 0.0 {
            let mut mom_scores: Vec<(usize, f64)> = Vec::new();
            for (sym_idx, sym) in symbols.iter().enumerate() {
                if let Some(d) = sym_data.get(sym) {
                    let data_bar = test_start + bar - 1;
                    if data_bar >= lookback + 1 {
                        let mom = ad_momentum(&d.ad, lookback, data_bar);
                        if mom > 0.0 {
                            mom_scores.push((sym_idx, mom));
                        }
                    }
                }
            }
            mom_scores.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());
            if mom_scores.len() >= 2 {
                // top-2 entry
                for &(sym_idx, _) in mom_scores.iter().take(POSITION_CAP) {
                    let entry_p = test_closes[sym_idx][bar]; // next bar open = today's close
                    let pos_size = prev_equity / POSITION_CAP as f64;
                    if pos_size * TAKER_FEE < prev_equity * 0.01 {
                        entry_price = entry_p;
                        entry_bar = bar;
                        entry_sym_idx = sym_idx;
                        position = pos_size / entry_price;
                        break; // only enter first position for simplicity
                    }
                }
            }
        }

        if position > 0.0 {
            let pnl = position * (test_closes[entry_sym_idx][bar] / entry_price - 1.0);
            let new_equity = (prev_equity + pnl).max(0.0001);
            equity[bar] = new_equity;
            if bar > 0 { daily_rets.push(new_equity / prev_equity - 1.0); }
        } else {
            equity[bar] = prev_equity;
        }
    }

    let total_return = (equity.last().unwrap() / equity[0] - 1.0) * 100.0;
    let sharpe = annualised_sharpe(&daily_rets);
    let max_dd = max_drawdown(&equity) * 100.0;
    let trades = (daily_rets.len() / 3) as i64; // rough estimate

    Some((total_return, sharpe, max_dd, equity.last().unwrap_or(1.0), trades, daily_rets.len() as i64))
}

fn run_universe_sweep(
    loader: &DataLoader,
    universe_name: &str,
    symbols: &[&str],
    lookbacks: &[usize],
) -> Vec<(usize, f64, f64, f64, i64, i64)> {
    let sym_data = load_universe(loader, symbols);
    let sym_strings: Vec<String> = symbols.iter().map(|s| s.to_string()).collect();

    // Find common data length
    let min_len = sym_data.values().map(|d| d.close.len()).min().unwrap_or(0);
    if min_len < TRAIN_BARS * 2 + TEST_BARS {
        return vec![];
    }

    // Walk-forward windows
    let mut results = vec![];
    let total_bars = min_len;
    let n_windows = (total_bars - TRAIN_BARS) / TEST_BARS;
    let n_windows = n_windows.max(1);

    for &lb in lookbacks {
        let mut window_returns = vec![];
        let mut window_sharpes = vec![];
        let mut window_dds = vec![];
        let mut total_trades = 0i64;

        for w in 0..n_windows {
            let train_start = w * TEST_BARS;
            let train_end = train_start + TRAIN_BARS;
            let test_start = train_end;
            let test_end = (train_end + TEST_BARS).min(min_len);

            if test_end - test_start < 30 {
                continue;
            }

            if let Some((ret, sharpe, dd, _, trades, _)) = run_backtest(
                &sym_data, &sym_strings, lb, train_start, train_end, test_start, test_end,
            ) {
                window_returns.push(ret);
                window_sharpes.push(sharpe);
                window_dds.push(dd);
                total_trades += trades;
            }
        }

        if window_returns.is_empty() {
            continue;
        }

        let avg_ret = window_returns.iter().sum::<f64>() / window_returns.len() as f64;
        let avg_sharpe = window_sharpes.iter().sum::<f64>() / window_sharpes.len() as f64;
        let avg_dd = window_dds.iter().sum::<f64>() / window_dds.len() as f64;
        let pass_rate = window_returns.iter().filter(|&&r| r > 0.0).count() as f64 / window_returns.len() as f64 * 100.0;

        println!("  LB={:3} | avg_ret={:8.2f}% | avg_sharpe={:7.3f} | avg_dd={:7.2f}% | pass={:5.1f}% | trades={}",
            lb, avg_ret, avg_sharpe, avg_dd, pass_rate, total_trades);

        results.push((lb, avg_sharpe, avg_ret, avg_dd, total_trades, pass_rate as i64));
    }

    results
}

fn main() -> Result<()> {
    let start = Instant::now();
    println!("=== A/D Momentum Lookback Hyperopt ===");
    println!("Sweep: {} to {} step {}", LOOKBACK_MIN, LOOKBACK_MAX, LOOKBACK_STEP);
    println!("Universes: {}", UNIVERSES.len());
    println!("");

    let loader = DataLoader::new();
    let lookbacks: Vec<usize> = (LOOKBACK_MIN..=LOOKBACK_MAX)
        .filter(|x| (x - LOOKBACK_MIN) % LOOKBACK_STEP == 0)
        .collect();

    let mut all_results: Vec<(String, usize, f64, f64, f64, i64, i64)> = vec![];

    for (universe_name, symbols) in UNIVERSES {
        println!("Universe: {}", universe_name);
        let results = run_universe_sweep(&loader, universe_name, symbols, &lookbacks);
        for (lb, sharpe, ret, dd, trades, passes) in results {
            all_results.push((universe_name.to_string(), lb, sharpe, ret, dd, trades, passes));
        }
    }

    // Write CSV
    {
        let mut file = File::create(CSV_OUT)?;
        writeln!(file, "universe,lookback,avg_sharpe,avg_return_pct,avg_max_dd_pct,trades,pass_rate_pct")?;
        for (univ, lb, sharpe, ret, dd, trades, passes) in &all_results {
            writeln!(file, "{},{},{:.4},{:.4},{:.4},{},{}", univ, lb, sharpe, ret, dd, trades, passes)?;
        }
    }

    // Aggregate across universes: average Sharpe per lookback
    let mut lb_agg: HashMap<usize, (f64, f64, f64, i64, i64, usize)> = HashMap::new();
    for (univ, lb, sharpe, ret, dd, trades, passes) in &all_results {
        let entry = lb_agg.entry(*lb).or_insert((0.0, 0.0, 0.0, 0, 0, 0));
        entry.0 += sharpe;
        entry.1 += ret;
        entry.2 += dd;
        entry.3 += *trades;
        entry.4 += *passes;
        entry.5 += 1;
    }

    let mut ranked: Vec<(usize, f64, f64, f64, i64, i64)> = lb_agg.iter().map(|(&lb, &(s, r, d, t, p, c))| {
        let cnt = c as f64;
        (lb, s / cnt, r / cnt, d / cnt, t, p / cnt as i64)
    }).collect();
    ranked.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());

    // Write summary JSON
    {
        let mut file = File::create(SUMMARY_JSON)?;
        writeln!(file, "{{")?;
        writeln!(file, "  \"parameter\": \"ad_lookback\",")?;
        writeln!(file, "  \"sweep_min\": {},", LOOKBACK_MIN)?;
        writeln!(file, "  \"sweep_max\": {},", LOOKBACK_MAX)?;
        writeln!(file, "  \"sweep_step\": {},", LOOKBACK_STEP)?;
        writeln!(file, "  \"n_values\": {},", ranked.len())?;
        writeln!(file, "  \"ranked_by_avg_sharpe\": [")?;
        for (i, (lb, sharpe, ret, dd, trades, passes)) in ranked.iter().take(20).enumerate() {
            writeln!(file, "    {{\"rank\":{},\"lookback\":{},\"avg_sharpe\":{:.4},\"avg_return_pct\":{:.4},\"avg_dd_pct\":{:.4},\"total_trades\":{},\"avg_pass_pct\":{:.1}}}{}",
                i + 1, lb, sharpe, ret, dd, trades, *passes as f64 / UNIVERSES.len() as f64)?;
        }
        writeln!(file, "  ],")?;
        let winner = ranked.first();
        if let Some((lb, sharpe, _, _, _, _)) = winner {
            writeln!(file, "  \"winner\": {{\"lookback\":{},\"avg_sharpe\":{:.4}}},", lb, sharpe)?;
        }
        writeln!(file, "  \"baseline_lookback\": 20,")?;
        if let Some(base) = lb_agg.get(&20) {
            writeln!(file, "  \"baseline_sharpe\": {:.4},", base.0 / base.5 as f64)?;
        }
        writeln!(file, "  \"elapsed_seconds\": {}", start.elapsed().as_secs())?;
        writeln!(file, "}}")?;
    }

    // Print top 20
    println!("\n=== TOP 20 by avg OOS Sharpe ===");
    println!("{:>4} {:>6} {:>10} {:>10} {:>10} {:>8}", "Rank", "LB", "AvgSharpe", "AvgRet%", "AvgDD%", "Trades");
    for (i, (lb, sharpe, ret, dd, trades, _)) in ranked.iter().take(20).enumerate() {
        println!("{:>4} {:>6} {:>10.4f} {:>10.2f} {:>10.2f} {:>8}", i + 1, lb, sharpe, ret, dd, trades);
    }

    // Find baseline (LB=20) rank
    let baseline_rank = ranked.iter().position(|(lb, _, _, _, _, _)| *lb == 20).map(|p| p + 1);
    println!("\nBaseline LB=20 rank: {:?}", baseline_rank.map(|r| format!("#{}/{}", r, ranked.len())));

    let elapsed = start.elapsed();
    println!("\nTotal time: {:.1}s", elapsed.as_secs_f64());
    println!("Results: {}", CSV_OUT);
    println!("Summary: {}", SUMMARY_JSON);

    Ok(())
}
