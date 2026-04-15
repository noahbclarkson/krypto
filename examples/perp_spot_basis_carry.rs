//! Test a genuinely different structural carry family: perp-vs-spot basis mean reversion.
//!
//! Construction:
//! - use Binance spot OHLCV for spot execution prices
//! - use Binance perp funding history mark prices as a perp proxy
//! - compute rolling z-scores of the perp-vs-spot basis = mark/spot - 1
//! - when basis is extremely positive, short perp / long spot at next spot open
//! - when basis is extremely negative, long perp / short spot at next spot open
//! - hold a short fixed window and include realized funding cashflows on the perp leg
//!
//! This is intentionally different from the earlier cross-perp funding-dispersion family:
//! it asks whether direct perp-vs-spot dislocations plus funding carry are any cleaner.

use anyhow::{anyhow, Result};
use krypto::data::funding_rate::FundingRateLoader;
use krypto::data::DataLoader;
use polars::prelude::*;
use std::collections::HashMap;

const SYMBOLS: &[(&str, &str)] = &[
    ("BTCUSDT", "BTCFDUSD"),
    ("ETHUSDT", "ETHFDUSD"),
    ("SOLUSDT", "SOLFDUSD"),
];
const INTERVAL: &str = "4h";
const CANDLES: u32 = 3000;
const FEE_PER_SIDE: f64 = 0.001;
const BASIS_WINDOWS: &[usize] = &[30, 60, 90];
const ENTRY_ZS: &[f64] = &[1.0, 1.5, 2.0];
const HOLDS: &[usize] = &[1, 3, 6];
const MIN_TRADES_PER_WINDOW: usize = 8;
const WARMUP_BARS: usize = 120;
const RESAMPLE_BLOCKS: usize = 6;

#[derive(Clone)]
struct AssetSeries {
    times: Vec<i64>,
    spot_open: Vec<f64>,
    basis: Vec<f64>,
    basis_z: HashMap<usize, Vec<f64>>,
    funding_events: Vec<(i64, f64)>,
}

#[derive(Clone, Copy, Debug)]
struct Config {
    basis_window: usize,
    entry_z: f64,
    hold_bars: usize,
}

#[derive(Default, Clone, Debug)]
struct EvalResult {
    total_return_pct: f64,
    price_return_pct: f64,
    funding_return_pct: f64,
    trades: usize,
    wins: usize,
    avg_trade_pct: f64,
}

impl EvalResult {
    fn win_rate(&self) -> f64 {
        if self.trades == 0 {
            0.0
        } else {
            self.wins as f64 / self.trades as f64
        }
    }
}

fn configs() -> Vec<Config> {
    let mut out = Vec::new();
    for &basis_window in BASIS_WINDOWS {
        for &entry_z in ENTRY_ZS {
            for &hold_bars in HOLDS {
                out.push(Config {
                    basis_window,
                    entry_z,
                    hold_bars,
                });
            }
        }
    }
    out
}

#[tokio::main]
async fn main() -> Result<()> {
    println!("=== PERP-VS-SPOT BASIS CARRY TEST ===\n");
    println!(
        "Universe: {:?}",
        SYMBOLS.iter().map(|(perp, _)| *perp).collect::<Vec<_>>()
    );
    println!("Interval: {}", INTERVAL);
    println!("Execution: signal at close, enter synthetic perp-vs-spot spread at next spot open");
    println!("Signal: basis z-score on perp mark vs spot open");
    println!(
        "PnL model: basis convergence + realized funding on perp leg - {:.1}% taker each side",
        FEE_PER_SIDE * 100.0
    );
    println!(
        "Grid: basis_window in {:?}, entry_z in {:?}, hold in {:?}\n",
        BASIS_WINDOWS, ENTRY_ZS, HOLDS
    );

    let loader = DataLoader::new(None, None);
    let funding_loader = FundingRateLoader::with_cache_dir("examples/funding_cache");

    let mut data = HashMap::new();
    for &(perp_symbol, spot_symbol) in SYMBOLS {
        print!("Loading {} vs {}... ", perp_symbol, spot_symbol);
        let spot_df = loader
            .fetch_with_cache(spot_symbol, INTERVAL, CANDLES)
            .await?;
        let funding_df = funding_loader.fetch(perp_symbol, None, None).await?;
        let asset = build_asset_series(&spot_df, &funding_df)?;
        println!(
            "{} aligned bars, {} funding rows",
            asset.times.len(),
            asset.funding_events.len()
        );
        data.insert(perp_symbol.to_string(), asset);
    }

    let bars = data
        .values()
        .next()
        .ok_or_else(|| anyhow!("no data loaded"))?
        .times
        .len();
    let quarter_windows = quarter_windows(bars);
    let resample_sets = cpcv_style_windows(bars);

    let mut rows = Vec::new();
    for cfg in configs() {
        let full = evaluate(&data, cfg, &[(WARMUP_BARS, bars)])?;

        let mut wf_passed = 0usize;
        for &(start, end) in &quarter_windows {
            let eval = evaluate(&data, cfg, &[(start, end)])?;
            if eval.total_return_pct > 0.0 && eval.trades >= MIN_TRADES_PER_WINDOW {
                wf_passed += 1;
            }
        }

        let mut rs_passed = 0usize;
        for windows in &resample_sets {
            let eval = evaluate(&data, cfg, windows)?;
            if eval.total_return_pct > 0.0 && eval.trades >= MIN_TRADES_PER_WINDOW {
                rs_passed += 1;
            }
        }

        rows.push((cfg, full, wf_passed, rs_passed));
    }

    rows.sort_by(|a, b| {
        b.3.cmp(&a.3).then_with(|| b.2.cmp(&a.2)).then_with(|| {
            b.1.total_return_pct
                .partial_cmp(&a.1.total_return_pct)
                .unwrap()
        })
    });

    println!(
        "{:<20} {:<6} {:>10} {:>10} {:>10} {:>8} {:>8} {:>12}",
        "Config", "Hold", "Total%", "Price%", "Funding%", "Trades", "Win%", "Resamples"
    );
    for (cfg, full, _, rs_passed) in &rows {
        println!(
            "w{:>3}/z>{:.1} {:>4} {:>10.1} {:>10.1} {:>10.1} {:>8} {:>7.1}% {:>6}/{}",
            cfg.basis_window,
            cfg.entry_z,
            cfg.hold_bars,
            full.total_return_pct,
            full.price_return_pct,
            full.funding_return_pct,
            full.trades,
            full.win_rate() * 100.0,
            rs_passed,
            resample_sets.len(),
        );
    }

    if let Some((best_cfg, best_full, best_wf, best_rs)) = rows.first() {
        println!("\nBest by chronology-first ranking:");
        println!(
            "- basis_window {}, entry_z {:.1}, hold {} -> total {:.1}%, price {:.1}%, funding {:.1}%, trades {}, win rate {:.1}%, avg trade {:.3}%, WF {}/{}, resamples {}/{}",
            best_cfg.basis_window,
            best_cfg.entry_z,
            best_cfg.hold_bars,
            best_full.total_return_pct,
            best_full.price_return_pct,
            best_full.funding_return_pct,
            best_full.trades,
            best_full.win_rate() * 100.0,
            best_full.avg_trade_pct,
            best_wf,
            quarter_windows.len(),
            best_rs,
            resample_sets.len(),
        );
    }

    println!("\nInterpretation:");
    println!("- If funding contribution is real but total PnL is weak, basis convergence timing is still poor.");
    println!("- If both price and funding stay weak, direct perp-vs-spot basis is not yet a trustworthy daily/4h carry family here.");
    println!("- Even a positive row would still need more realism before promotion (borrow costs, spot/perp execution differences, capacity).\n");

    Ok(())
}

fn build_asset_series(spot_df: &DataFrame, funding_df: &DataFrame) -> Result<AssetSeries> {
    let spot_times_s = spot_df.column("time")?.cast(&DataType::Int64)?;
    let spot_opens = spot_df.column("open")?.f64()?;

    let mut spot_map = HashMap::new();
    for i in 0..spot_df.height() {
        let t = spot_times_s.i64()?.get(i).unwrap_or(0);
        let open = spot_opens.get(i).unwrap_or(0.0);
        spot_map.insert(t, open);
    }

    let fund_times_s = funding_df.column("time")?.cast(&DataType::Int64)?;
    let mark_prices = funding_df.column("mark_price")?.f64()?;
    let fund_rates = funding_df.column("funding_rate")?.f64()?;

    let mut times = Vec::new();
    let mut spot_open = Vec::new();
    let mut basis = Vec::new();
    let mut funding_events = Vec::with_capacity(funding_df.height());

    for i in 0..funding_df.height() {
        let t = fund_times_s.i64()?.get(i).unwrap_or(0);
        let mark = mark_prices.get(i).unwrap_or(0.0);
        let rate = fund_rates.get(i).unwrap_or(0.0);
        funding_events.push((t, rate));
        if let Some(&spot) = spot_map.get(&t) {
            if spot > 0.0 && mark > 0.0 {
                times.push(t);
                spot_open.push(spot);
                basis.push(mark / spot - 1.0);
            }
        }
    }

    if times.len() < WARMUP_BARS + 200 {
        return Err(anyhow!("not enough aligned basis bars: {}", times.len()));
    }

    let mut basis_z = HashMap::new();
    for &window in BASIS_WINDOWS {
        basis_z.insert(window, rolling_z(&basis, window));
    }

    Ok(AssetSeries {
        times,
        spot_open,
        basis,
        basis_z,
        funding_events,
    })
}

fn rolling_z(values: &[f64], window: usize) -> Vec<f64> {
    let mut out = vec![0.0; values.len()];
    if window == 0 {
        return out;
    }
    for i in 0..values.len() {
        if i + 1 < window {
            continue;
        }
        let slice = &values[i + 1 - window..=i];
        let mean = slice.iter().sum::<f64>() / slice.len() as f64;
        let var = slice.iter().map(|v| (v - mean).powi(2)).sum::<f64>() / slice.len() as f64;
        let std = var.sqrt();
        if std > 1e-12 {
            out[i] = (values[i] - mean) / std;
        }
    }
    out
}

fn evaluate(
    data: &HashMap<String, AssetSeries>,
    cfg: Config,
    windows: &[(usize, usize)],
) -> Result<EvalResult> {
    let mut combined_returns = Vec::new();
    let mut price_returns = Vec::new();
    let mut funding_returns = Vec::new();

    for asset in data.values() {
        let z = asset
            .basis_z
            .get(&cfg.basis_window)
            .ok_or_else(|| anyhow!("missing z window {}", cfg.basis_window))?;

        for &(start, end) in windows {
            let start_i = start.max(cfg.basis_window).max(1);
            let end_i = end.min(asset.times.len());
            if end_i <= start_i + cfg.hold_bars {
                continue;
            }

            for i in start_i..(end_i - cfg.hold_bars - 1) {
                let signal_z = z[i];
                if signal_z.abs() < cfg.entry_z {
                    continue;
                }

                let entry_idx = i + 1;
                let exit_idx = entry_idx + cfg.hold_bars;
                if exit_idx >= end_i {
                    continue;
                }

                let entry_basis = asset.basis[entry_idx];
                let exit_basis = asset.basis[exit_idx];
                let entry_time = asset.times[entry_idx];
                let exit_time = asset.times[exit_idx];

                let direction = if signal_z > 0.0 {
                    -1.0 // rich basis -> short perp / long spot
                } else {
                    1.0 // cheap basis -> long perp / short spot
                };

                let price_ret = direction * (exit_basis - entry_basis) - 2.0 * FEE_PER_SIDE;
                let funding_ret = direction
                    * realized_funding_return(&asset.funding_events, entry_time, exit_time);
                let total_ret = price_ret + funding_ret;

                combined_returns.push(total_ret);
                price_returns.push(price_ret);
                funding_returns.push(funding_ret);
            }
        }
    }

    summarize_returns(&combined_returns, &price_returns, &funding_returns)
}

fn realized_funding_return(events: &[(i64, f64)], entry_time: i64, exit_time: i64) -> f64 {
    events
        .iter()
        .filter(|(t, _)| *t > entry_time && *t <= exit_time)
        .map(|(_, rate)| -*rate)
        .sum::<f64>()
}

fn summarize_returns(combined: &[f64], price: &[f64], funding: &[f64]) -> Result<EvalResult> {
    if combined.len() != price.len() || combined.len() != funding.len() {
        return Err(anyhow!("return vectors misaligned"));
    }

    let mut result = EvalResult::default();
    result.trades = combined.len();
    if combined.is_empty() {
        return Ok(result);
    }

    let mut total_equity = 1.0f64;
    let mut price_equity = 1.0f64;
    let mut funding_equity = 1.0f64;
    let mut sum = 0.0;

    for i in 0..combined.len() {
        let c = combined[i];
        let p = price[i];
        let f = funding[i];
        if c > 0.0 {
            result.wins += 1;
        }
        total_equity *= 1.0 + c;
        price_equity *= 1.0 + p;
        funding_equity *= 1.0 + f;
        sum += c;
    }

    result.total_return_pct = (total_equity - 1.0) * 100.0;
    result.price_return_pct = (price_equity - 1.0) * 100.0;
    result.funding_return_pct = (funding_equity - 1.0) * 100.0;
    result.avg_trade_pct = (sum / combined.len() as f64) * 100.0;
    Ok(result)
}

fn quarter_windows(bars: usize) -> Vec<(usize, usize)> {
    let usable = bars.saturating_sub(WARMUP_BARS);
    let chunk = (usable / 4).max(1);
    let mut out = Vec::new();
    for idx in 0..4 {
        let start = WARMUP_BARS + idx * chunk;
        let end = if idx == 3 {
            bars
        } else {
            (WARMUP_BARS + (idx + 1) * chunk).min(bars)
        };
        if end > start {
            out.push((start, end));
        }
    }
    out
}

fn cpcv_style_windows(bars: usize) -> Vec<Vec<(usize, usize)>> {
    let usable = bars.saturating_sub(WARMUP_BARS);
    let block = (usable / RESAMPLE_BLOCKS).max(1);
    let mut blocks = Vec::new();
    for idx in 0..RESAMPLE_BLOCKS {
        let start = WARMUP_BARS + idx * block;
        let end = if idx == RESAMPLE_BLOCKS - 1 {
            bars
        } else {
            (WARMUP_BARS + (idx + 1) * block).min(bars)
        };
        if end > start {
            blocks.push((start, end));
        }
    }

    let mut out = Vec::new();
    for i in 0..blocks.len() {
        for j in (i + 1)..blocks.len() {
            out.push(vec![blocks[i], blocks[j]]);
        }
    }
    out
}
