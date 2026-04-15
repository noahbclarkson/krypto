//! Perp-Perp Funding Rate Cross-Sectional Carry
//!
//! Rank all USDT perpetuals by funding rate z-score each day.
//! Go LONG the cheapest-funded (underpriced shorts) and SHORT the most expensive-funded (overpriced longs).
//! Funding rates gravitate to 0.01%/8h anchor — extreme readings mean-revert.
//!
//! This is the perp-perp carry lane: no spot leg, no funding of funding.
//!
//! - Universe: BTC, ETH, SOL, ADA, DOGE, XRP USDT perpetuals
//! - Resolution: DAILY (aligned with the rest of the system)
//! - Signal: cross-sectional z-score of rolling 20d funding percentile
//! - Entry: next open, both legs
//! - Hold: fixed 21 bars
//! - Fees: 0.1% taker each side per leg

use colored::*;
use krypto::data::funding_rate::FundingRateLoader;
use krypto::data::DataLoader;
use polars::prelude::*;
use std::collections::HashMap;

const SYMBOLS: &[&str] = &[
    "BTCUSDT", "ETHUSDT", "SOLUSDT", "ADAUSDT", "DOGEUSDT", "XRPUSDT",
];
const INTERVAL: &str = "1d";
const CANDLES: u32 = 2000;
const FUNDING_Z_WINDOW: usize = 20;
const FEE_PER_SIDE: f64 = 0.001;
const HOLD_BARS: usize = 21;
const ENTRY_Z: f64 = 1.0;
const TOP_N: usize = 2;
const WARMUP_BARS: usize = 60;

#[derive(Clone, Copy, Debug, Default)]
struct TradeResult {
    n: usize,
    wins: usize,
    total_return: f64,
    avg_return: f64,
    max_dd: f64,
    peak_equity: f64,
    largest_win: f64,
    largest_loss: f64,
}

impl TradeResult {
    fn add(&mut self, ret: f64) {
        self.n += 1;
        if ret > 0.0 {
            self.wins += 1;
        }
        if ret > self.largest_win {
            self.largest_win = ret;
        }
        if ret < self.largest_loss {
            self.largest_loss = ret;
        }
        let prev_equity = 1.0 + self.avg_return;
        let new_equity = prev_equity * (1.0 + ret);
        self.avg_return = new_equity - 1.0;
        if new_equity > self.peak_equity {
            self.peak_equity = new_equity;
        }
        let dd = (new_equity - self.peak_equity) / self.peak_equity;
        if dd < self.max_dd {
            self.max_dd = dd;
        }
        self.total_return += ret;
    }
    fn win_rate(&self) -> f64 {
        if self.n == 0 {
            0.0
        } else {
            self.wins as f64 / self.n as f64
        }
    }
    fn avg_trade(&self) -> f64 {
        if self.n == 0 {
            0.0
        } else {
            self.total_return / self.n as f64
        }
    }
}

#[derive(Clone, Copy, Debug)]
struct Config {
    funding_z_window: usize,
    entry_z: f64,
    hold_bars: usize,
    top_n: usize,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            funding_z_window: 20,
            entry_z: 1.0,
            hold_bars: 21,
            top_n: 2,
        }
    }
}

fn find_bar_idx(times: &[i64], target: i64) -> Option<usize> {
    times.iter().position(|&t| t >= target).or_else(|| {
        if times.is_empty() {
            None
        } else {
            Some(times.len() - 1)
        }
    })
}

fn epoch_ms_to_date(ms: i64) -> String {
    use chrono::{DateTime, Utc};
    if ms == i64::MAX || ms == i64::MIN {
        return "N/A".to_string();
    }
    let secs = ms / 1000;
    let dt = DateTime::from_timestamp(secs, 0)
        .unwrap_or_else(|| DateTime::from_timestamp(0, 0).unwrap());
    dt.format("%Y-%m-%d").to_string()
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    println!("{}", "═══ PERP-PERP FUNDING CARRY ═══".cyan().bold());
    println!("Universe: {:?}", SYMBOLS);
    println!(
        "Entry z: {:.1}, Hold: {} bars, TopN: {}",
        ENTRY_Z, HOLD_BARS, TOP_N
    );
    println!();

    let funding_loader = FundingRateLoader::with_cache_dir("examples/funding_cache");
    let ohlcv_loader = DataLoader::new(None, None);

    // Load all funding histories
    let mut all_funding: HashMap<&str, Vec<(i64, f64)>> = HashMap::new();
    let mut min_time = i64::MAX;
    let mut max_time = i64::MIN;

    for &sym in SYMBOLS {
        let df = funding_loader.fetch(sym, None, None).await?;
        let times_raw = df.column("time")?.cast(&DataType::Int64)?.i64()?.to_vec();
        let rates_raw = df.column("funding_rate")?.f64()?.to_vec();
        let mut pairs: Vec<(i64, f64)> = Vec::new();
        for i in 0..times_raw.len() {
            if let (Some(t), Some(r)) = (times_raw[i], rates_raw[i]) {
                if !r.is_nan() && r.is_finite() && r.abs() < 1.0 {
                    pairs.push((t, r));
                }
            }
        }
        pairs.sort_by_key(|(t, _)| *t);
        let first = *pairs.first().map(|(t, _)| t).unwrap_or(&i64::MAX);
        let last = *pairs.last().map(|(t, _)| t).unwrap_or(&i64::MIN);
        if first < min_time {
            min_time = first;
        }
        if last > max_time {
            max_time = last;
        }
        let count = pairs.len();
        all_funding.insert(sym, pairs);
        println!(
            "  {}: {} records ({} → {})",
            sym,
            count,
            epoch_ms_to_date(first),
            epoch_ms_to_date(last)
        );
    }

    println!(
        "\n  Common range: {} → {}",
        epoch_ms_to_date(min_time),
        epoch_ms_to_date(max_time)
    );

    // Load OHLCV for each symbol
    let mut ohlcvs: HashMap<&str, DataFrame> = HashMap::new();
    for &sym in SYMBOLS {
        let df = ohlcv_loader.fetch_data(sym, INTERVAL, CANDLES).await?;
        ohlcvs.insert(sym, df);
        println!(
            "  {}: {} daily bars",
            sym,
            ohlcvs.get(sym).unwrap().height()
        );
    }

    // BTC as time anchor — flatten Option types immediately
    let btc_df = ohlcvs.get("BTCUSDT").expect("BTC must load");
    let btc_times_raw = btc_df
        .column("time")?
        .cast(&DataType::Int64)?
        .i64()?
        .to_vec();
    let btc_times: Vec<i64> = btc_times_raw.into_iter().filter_map(|x| x).collect();
    let btc_start = btc_times.first().copied().unwrap_or(0);
    let btc_end = btc_times.last().copied().unwrap_or(0);
    println!(
        "\n  BTC anchor: {} bars ({} → {})",
        btc_times.len(),
        epoch_ms_to_date(btc_start),
        epoch_ms_to_date(btc_end)
    );

    // Build daily funding snapshots
    let mut daily_funding: Vec<HashMap<&str, f64>> = Vec::new();
    let mut daily_times: Vec<i64> = Vec::new();

    for btc_t in &btc_times {
        let mut day_funding: HashMap<&str, f64> = HashMap::new();
        for &sym in SYMBOLS {
            if let Some(funding_vec) = all_funding.get(sym) {
                let applicable: Vec<_> =
                    funding_vec.iter().filter(|(ft, _)| *ft <= *btc_t).collect();
                if let Some((_, fr)) = applicable.last() {
                    day_funding.insert(sym, *fr);
                }
            }
        }
        if day_funding.len() == SYMBOLS.len() {
            daily_times.push(*btc_t);
            daily_funding.push(day_funding);
        }
    }

    println!(
        "\n  {} days with complete funding data",
        daily_funding.len()
    );

    let warmup = WARMUP_BARS.max(FUNDING_Z_WINDOW) + HOLD_BARS;
    if daily_funding.len() < warmup {
        println!(
            "ERROR: Not enough data ({} < {})",
            daily_funding.len(),
            warmup
        );
        return Ok(());
    }

    // Pre-load OHLCV data into flattened arrays
    struct SymbolData {
        times: Vec<i64>,
        opens: Vec<f64>,
    }
    let mut symbol_data: HashMap<&str, SymbolData> = HashMap::new();
    for &sym in SYMBOLS {
        if let Some(df) = ohlcvs.get(sym) {
            let times_raw = df.column("time")?.cast(&DataType::Int64)?.i64()?.to_vec();
            let opens_raw = df.column("open")?.f64()?.to_vec();
            let times: Vec<i64> = times_raw.into_iter().filter_map(|x| x).collect();
            let opens: Vec<f64> = opens_raw.into_iter().filter_map(|x| x).collect();
            symbol_data.insert(sym, SymbolData { times, opens });
        }
    }

    // Configs to test
    let configs: Vec<Config> = vec![
        Config {
            funding_z_window: 20,
            entry_z: 0.5,
            hold_bars: 21,
            top_n: 2,
        },
        Config {
            funding_z_window: 20,
            entry_z: 1.0,
            hold_bars: 21,
            top_n: 2,
        },
        Config {
            funding_z_window: 20,
            entry_z: 1.5,
            hold_bars: 21,
            top_n: 2,
        },
        Config {
            funding_z_window: 10,
            entry_z: 1.0,
            hold_bars: 21,
            top_n: 2,
        },
        Config {
            funding_z_window: 10,
            entry_z: 0.5,
            hold_bars: 10,
            top_n: 2,
        },
        Config {
            funding_z_window: 30,
            entry_z: 1.0,
            hold_bars: 21,
            top_n: 2,
        },
        Config {
            funding_z_window: 20,
            entry_z: 1.0,
            hold_bars: 10,
            top_n: 2,
        },
    ];

    let mut all_results: Vec<(Config, TradeResult)> = Vec::new();

    for cfg in &configs {
        let mut tr_all: TradeResult = Default::default();
        let mut tr_long: TradeResult = Default::default();
        let mut tr_short: TradeResult = Default::default();

        let start = cfg.funding_z_window;
        for i in start..daily_funding.len().saturating_sub(cfg.hold_bars) {
            // Compute z-score for each symbol over its own rolling funding window
            let mut symbol_z: Vec<(&str, f64)> = Vec::new();
            for &sym in SYMBOLS {
                let window_start = i.saturating_sub(cfg.funding_z_window);
                let mut window: Vec<f64> = Vec::new();
                for di in window_start..i {
                    if let Some(d) = daily_funding.get(di) {
                        if let Some(&fr) = d.get(sym) {
                            window.push(fr);
                        }
                    }
                }
                if window.len() < cfg.funding_z_window / 2 {
                    continue;
                }
                let mean = window.iter().sum::<f64>() / window.len() as f64;
                let variance =
                    window.iter().map(|x| (x - mean).powi(2)).sum::<f64>() / window.len() as f64;
                let std = variance.sqrt();
                if std < 1e-9 {
                    continue;
                }
                let current = *daily_funding
                    .get(i)
                    .and_then(|d| d.get(sym))
                    .unwrap_or(&0.0);
                let z = (current - mean) / std;
                symbol_z.push((sym, z));
            }

            if symbol_z.len() < SYMBOLS.len() {
                continue;
            }

            // Sort: most negative z = cheapest to hold longs = LONG
            //       most positive z = expensive = SHORT
            symbol_z.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap());

            let top_short: Vec<_> = symbol_z.iter().rev().take(cfg.top_n).collect();
            let bottom_long: Vec<_> = symbol_z.iter().take(cfg.top_n).collect();

            if top_short.is_empty() || bottom_long.is_empty() {
                continue;
            }
            let z_spread = top_short[0].1 - bottom_long[0].1;
            if z_spread < cfg.entry_z * 2.0 {
                continue;
            }

            let entry_time = *daily_times.get(i).unwrap_or(&0);
            let exit_idx = (i + cfg.hold_bars).min(daily_times.len().saturating_sub(1));
            let exit_time = *daily_times.get(exit_idx).unwrap_or(&0);
            if entry_time == 0 || exit_time == 0 {
                continue;
            }

            // Long leg: cheapest-funded perps (shorts are underpriced = long them)
            let mut long_ret = 0.0;
            let mut long_n = 0;
            for &(sym, _) in bottom_long.iter() {
                if let Some(sd) = symbol_data.get(sym) {
                    if let (Some(ei), Some(xi)) = (
                        find_bar_idx(&sd.times, entry_time),
                        find_bar_idx(&sd.times, exit_time),
                    ) {
                        if ei < sd.opens.len() && xi < sd.opens.len() && xi > ei {
                            let entry = sd.opens[ei];
                            let exit = sd.opens[xi];
                            let gross = (exit - entry) / entry;
                            long_ret += gross - FEE_PER_SIDE;
                            long_n += 1;
                        }
                    }
                }
            }
            if long_n > 0 {
                long_ret /= long_n as f64;
            }

            // Short leg: most expensive-funded perps (longs are overpriced = short them)
            let mut short_ret = 0.0;
            let mut short_n = 0;
            for &(sym, _) in top_short.iter() {
                if let Some(sd) = symbol_data.get(sym) {
                    if let (Some(ei), Some(xi)) = (
                        find_bar_idx(&sd.times, entry_time),
                        find_bar_idx(&sd.times, exit_time),
                    ) {
                        if ei < sd.opens.len() && xi < sd.opens.len() && xi > ei {
                            let entry = sd.opens[ei];
                            let exit = sd.opens[xi];
                            let gross = -(exit - entry) / entry; // gain when price falls
                            short_ret += gross - FEE_PER_SIDE;
                            short_n += 1;
                        }
                    }
                }
            }
            if short_n > 0 {
                short_ret /= short_n as f64;
            }

            let combined_ret = (long_ret + short_ret) / 2.0;
            tr_all.add(combined_ret);
            tr_long.add(long_ret);
            tr_short.add(short_ret);
        }

        let total_ret = (1.0 + tr_all.avg_trade()).powi(tr_all.n as i32) - 1.0;

        println!(
            "\n{}",
            format!(
                "z_win={}, entry_z={:.1}, hold={}, top_n={}",
                cfg.funding_z_window, cfg.entry_z, cfg.hold_bars, cfg.top_n
            )
            .yellow()
        );
        println!(
            "  ALL:   {:>4} trades | tot={:>7.1}% | avg={:>7.3}% | DD={:>6.1}% | WR={:>5.1}%",
            tr_all.n,
            total_ret * 100.0,
            tr_all.avg_trade() * 100.0,
            tr_all.max_dd * 100.0,
            tr_all.win_rate() * 100.0
        );
        println!(
            "  LONG:  {:>4} trades | avg={:>7.3}% | WR={:>5.1}%",
            tr_long.n,
            tr_long.avg_trade() * 100.0,
            tr_long.win_rate() * 100.0
        );
        println!(
            "  SHORT: {:>4} trades | avg={:>7.3}% | WR={:>5.1}%",
            tr_short.n,
            tr_short.avg_trade() * 100.0,
            tr_short.win_rate() * 100.0
        );

        all_results.push((*cfg, tr_all));
    }

    // Best config
    all_results.sort_by(|a, b| b.1.avg_trade().partial_cmp(&a.1.avg_trade()).unwrap());
    if let Some((best_cfg, best_tr)) = all_results.first() {
        let total_ret = (1.0 + best_tr.avg_trade()).powi(best_tr.n as i32) - 1.0;
        println!("\n{}", "═══ BEST ═══".green().bold());
        println!(
            "  z_win={}, entry_z={:.1}, hold={}, top_n={}",
            best_cfg.funding_z_window, best_cfg.entry_z, best_cfg.hold_bars, best_cfg.top_n
        );
        println!(
            "  {:>4} trades | tot={:>7.1}% | avg={:>7.3}% | DD={:>6.1}% | WR={:>5.1}%",
            best_tr.n,
            total_ret * 100.0,
            best_tr.avg_trade() * 100.0,
            best_tr.max_dd * 100.0,
            best_tr.win_rate() * 100.0
        );
    }

    // BTC buy-hold sanity over same period
    if daily_times.len() >= 2 {
        let start_t = *daily_times.first().unwrap_or(&0);
        let end_t = *daily_times.last().unwrap_or(&0);
        if let Some(sd) = symbol_data.get("BTCUSDT") {
            if let (Some(si), Some(ei)) = (
                find_bar_idx(&sd.times, start_t),
                find_bar_idx(&sd.times, end_t),
            ) {
                if si < sd.opens.len() && ei < sd.opens.len() {
                    let btc_ret = (sd.opens[ei] - sd.opens[si]) / sd.opens[si];
                    println!("\n{}", "═══ SANITY ═══".cyan());
                    println!("  BTC buy-hold (same period): {:>7.1}%", btc_ret * 100.0);
                }
            }
        }
    }

    Ok(())
}
