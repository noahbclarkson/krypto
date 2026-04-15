use anyhow::Result;
use chrono::{DateTime, Utc};
use krypto::{data::loader::DataLoader, features::indicators::FeatureEngine};
use polars::prelude::*;
use std::collections::HashMap;

const SYMBOLS: &[&str] = &[
    "BTCUSDT", "ETHUSDT", "XRPUSDT", "LTCUSDT", "BNBUSDT", "EOSUSDT",
];
const CANDLES: u32 = 3000;
const HOLD_BARS: usize = 21;
const TAKER_FEE: f64 = 0.001;
const RESAMPLE_BLOCKS: usize = 6;

#[derive(Default, Clone, Debug)]
struct EvalResult {
    total_return_pct: f64,
    trades: usize,
    wins: usize,
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

fn ts_to_string(ts: i64) -> String {
    DateTime::<Utc>::from_timestamp_millis(ts)
        .map(|dt| dt.format("%Y-%m-%d").to_string())
        .unwrap_or_else(|| "invalid-ts".to_string())
}

fn calculate_sma(series: &ChunkedArray<Float64Type>, period: usize) -> Result<Vec<Option<f64>>> {
    let n = series.len();
    let mut sma = vec![None; n];
    for i in period..n {
        let sum: f64 = (0..period).filter_map(|j| series.get(i - j)).sum();
        sma[i] = Some(sum / period as f64);
    }
    Ok(sma)
}

fn calculate_ema(series: &ChunkedArray<Float64Type>, period: usize) -> Result<Vec<Option<f64>>> {
    let n = series.len();
    let mut ema = vec![None; n];
    let multiplier = 2.0 / (period as f64 + 1.0);
    if n >= period {
        let sum: f64 = (0..period).filter_map(|j| series.get(j)).sum();
        ema[period - 1] = Some(sum / period as f64);
        for i in period..n {
            if let (Some(prev_ema), Some(curr_price)) = (ema[i - 1], series.get(i)) {
                ema[i] = Some((curr_price - prev_ema) * multiplier + prev_ema);
            }
        }
    }
    Ok(ema)
}

fn generate_macd_signals(df: &DataFrame) -> Result<Vec<i32>> {
    let macd = df.column("macd").ok().and_then(|s| s.f64().ok());
    let signal = df.column("macd_signal").ok().and_then(|s| s.f64().ok());
    let close = df.column("close")?.f64()?;
    let n = close.len();
    let mut signals = vec![0i32; n];

    if let (Some(macd_series), Some(signal_series)) = (macd, signal) {
        for i in 1..n {
            let macd_curr = macd_series.get(i).unwrap_or(0.0);
            let sig_curr = signal_series.get(i).unwrap_or(0.0);
            if macd_curr > sig_curr {
                signals[i] = 1;
            } else if macd_curr < sig_curr {
                signals[i] = -1;
            }
        }
    } else {
        let ema_12 = calculate_ema(&close, 12)?;
        let ema_26 = calculate_ema(&close, 26)?;
        for i in 26..n {
            let fast = ema_12[i].unwrap_or(0.0);
            let slow = ema_26[i].unwrap_or(0.0);
            if fast > slow {
                signals[i] = 1;
            } else if fast < slow {
                signals[i] = -1;
            }
        }
    }
    Ok(signals)
}

fn generate_macd_regime_signals(df: &DataFrame) -> Result<Vec<i32>> {
    let macd = generate_macd_signals(df)?;
    let close = df.column("close")?.f64()?;
    let sma_200 = calculate_sma(&close, 200)?;
    let mut out = vec![0i32; macd.len()];
    for i in 0..macd.len() {
        let sig = macd[i];
        let price = close.get(i).unwrap_or(0.0);
        let sma = sma_200[i].unwrap_or(0.0);
        if sig > 0 && price > sma {
            out[i] = 1;
        } else if sig < 0 && price < sma {
            out[i] = -1;
        }
    }
    Ok(out)
}

fn min_rows(data_cache: &HashMap<String, DataFrame>) -> usize {
    data_cache.values().map(|df| df.height()).min().unwrap_or(0)
}

fn cpcv_base_blocks(data_cache: &HashMap<String, DataFrame>) -> Vec<(usize, usize)> {
    let n = min_rows(data_cache);
    let block = n / RESAMPLE_BLOCKS;
    let mut base = Vec::new();
    for block_idx in 0..RESAMPLE_BLOCKS {
        let start = block_idx * block;
        let end = if block_idx == RESAMPLE_BLOCKS - 1 {
            n
        } else {
            (block_idx + 1) * block
        };
        base.push((start, end));
    }
    base
}

fn backtest_fixed_hold_next_open_window(
    df: &DataFrame,
    signals: &[i32],
    start: usize,
    end: usize,
) -> Result<EvalResult> {
    let open = df.column("open")?.f64()?;
    let n = open.len();
    let mut result = EvalResult::default();
    let mut i = 200usize.max(start);
    let end = end.min(n);

    while i + HOLD_BARS + 1 < end {
        let signal = signals.get(i).copied().unwrap_or(0);
        if signal == 0 {
            i += 1;
            continue;
        }
        let entry_idx = i + 1;
        let exit_idx = i + 1 + HOLD_BARS;
        if exit_idx >= end {
            break;
        }
        let entry = match open.get(entry_idx) {
            Some(v) if v > 0.0 => v,
            _ => {
                i += 1;
                continue;
            }
        };
        let exit = match open.get(exit_idx) {
            Some(v) if v > 0.0 => v,
            _ => {
                i += 1;
                continue;
            }
        };

        let gross = if signal > 0 {
            (exit / entry - 1.0) * 100.0
        } else {
            (entry / exit - 1.0) * 100.0
        };
        let net = gross - 2.0 * TAKER_FEE * 100.0;
        result.total_return_pct += net;
        result.trades += 1;
        if net > 0.0 {
            result.wins += 1;
        }
        i = exit_idx;
    }

    Ok(result)
}

#[tokio::main]
async fn main() -> Result<()> {
    let loader = DataLoader::new(None, None);
    let mut data_cache = HashMap::<String, DataFrame>::new();
    for symbol in SYMBOLS {
        let raw = loader.fetch_with_cache(symbol, "1d", CANDLES).await?;
        let df = FeatureEngine::add_technicals(&raw, None)?;
        data_cache.insert(symbol.to_string(), df);
    }

    let blocks = cpcv_base_blocks(&data_cache);
    println!("=== OLDGUARD6 MACD vs MACD+Regime DIAGNOSIS ===");
    println!("Universe: {}\n", SYMBOLS.join(", "));

    println!("Chronology blocks:");
    let ref_df = data_cache.get("BTCUSDT").unwrap();
    let ref_times = ref_df.column("time")?.datetime()?;
    for (idx, (start, end)) in blocks.iter().enumerate() {
        let start_ts = ref_times
            .as_datetime_iter()
            .nth(*start)
            .flatten()
            .map(|dt| dt.and_utc().timestamp_millis())
            .unwrap_or_default();
        let end_ts = ref_times
            .as_datetime_iter()
            .nth(end.saturating_sub(1))
            .flatten()
            .map(|dt| dt.and_utc().timestamp_millis())
            .unwrap_or_default();
        println!(
            "- block {} => {} .. {} (rows {}..{})",
            idx,
            ts_to_string(start_ts),
            ts_to_string(end_ts),
            start,
            end.saturating_sub(1)
        );
    }

    println!("\nPer-symbol full-sample attribution:");
    println!(
        "{:<8} {:>11} {:>8} {:>9} {:>13} {:>8} {:>9} {:>10}",
        "Symbol", "MACD Ret%", "Trades", "Win%", "MACD+Reg Ret%", "Trades", "Win%", "Delta"
    );

    let mut macd_total = 0.0;
    let mut macd_reg_total = 0.0;
    let mut macd_wins = 0usize;
    let mut macd_reg_wins = 0usize;

    for symbol in SYMBOLS {
        let df = data_cache.get(&symbol.to_string()).unwrap();
        let macd =
            backtest_fixed_hold_next_open_window(df, &generate_macd_signals(df)?, 0, df.height())?;
        let macd_reg = backtest_fixed_hold_next_open_window(
            df,
            &generate_macd_regime_signals(df)?,
            0,
            df.height(),
        )?;
        macd_total += macd.total_return_pct;
        macd_reg_total += macd_reg.total_return_pct;
        if macd.total_return_pct > macd_reg.total_return_pct {
            macd_wins += 1;
        }
        if macd_reg.total_return_pct > macd.total_return_pct {
            macd_reg_wins += 1;
        }
        println!(
            "{:<8} {:+11.1} {:>8} {:>8.1}% {:+13.1} {:>8} {:>8.1}% {:+10.1}",
            symbol,
            macd.total_return_pct,
            macd.trades,
            macd.win_rate() * 100.0,
            macd_reg.total_return_pct,
            macd_reg.trades,
            macd_reg.win_rate() * 100.0,
            macd.total_return_pct - macd_reg.total_return_pct,
        );
    }

    println!("\nPortfolio totals on OldGuard6:");
    println!("- MACD:        {:+.1}%", macd_total);
    println!("- MACD+Regime: {:+.1}%", macd_reg_total);
    println!(
        "- Delta:       {:+.1}% (MACD better in {}/{}, MACD+Regime better in {}/{})",
        macd_total - macd_reg_total,
        macd_wins,
        SYMBOLS.len(),
        macd_reg_wins,
        SYMBOLS.len()
    );

    println!("\nPer-block attribution by symbol:");
    println!(
        "{:<8} {:>7} {:>12} {:>7} {:>12} {:>8}",
        "Symbol", "Block", "MACD Ret%", "Trades", "MACD+Reg Ret%", "Delta"
    );
    let mut block_macd_better = vec![0usize; RESAMPLE_BLOCKS];
    let mut block_reg_better = vec![0usize; RESAMPLE_BLOCKS];
    let mut block_delta_sum = vec![0.0f64; RESAMPLE_BLOCKS];
    for symbol in SYMBOLS {
        let df = data_cache.get(&symbol.to_string()).unwrap();
        let macd_signals = generate_macd_signals(df)?;
        let macd_reg_signals = generate_macd_regime_signals(df)?;
        for (block_idx, (start, end)) in blocks.iter().enumerate() {
            let macd = backtest_fixed_hold_next_open_window(df, &macd_signals, *start, *end)?;
            let macd_reg =
                backtest_fixed_hold_next_open_window(df, &macd_reg_signals, *start, *end)?;
            let delta = macd.total_return_pct - macd_reg.total_return_pct;
            if delta > 0.0 {
                block_macd_better[block_idx] += 1;
            }
            if delta < 0.0 {
                block_reg_better[block_idx] += 1;
            }
            block_delta_sum[block_idx] += delta;
            println!(
                "{:<8} {:>7} {:+12.1} {:>7} {:+12.1} {:+8.1}",
                symbol,
                block_idx,
                macd.total_return_pct,
                macd.trades,
                macd_reg.total_return_pct,
                delta,
            );
        }
    }

    println!("\nBlock-level summary:");
    for (idx, (start, end)) in blocks.iter().enumerate() {
        let start_ts = ref_times
            .as_datetime_iter()
            .nth(*start)
            .flatten()
            .map(|dt| dt.and_utc().timestamp_millis())
            .unwrap_or_default();
        let end_ts = ref_times
            .as_datetime_iter()
            .nth(end.saturating_sub(1))
            .flatten()
            .map(|dt| dt.and_utc().timestamp_millis())
            .unwrap_or_default();
        println!(
            "- block {} ({} .. {}): delta {:+.1}%, MACD better on {}/{}, MACD+Regime better on {}/{}",
            idx,
            ts_to_string(start_ts),
            ts_to_string(end_ts),
            block_delta_sum[idx],
            block_macd_better[idx],
            SYMBOLS.len(),
            block_reg_better[idx],
            SYMBOLS.len(),
        );
    }

    Ok(())
}
