use anyhow::Result;
use chrono::{DateTime, Utc};
use krypto::{data::loader::DataLoader, features::indicators::FeatureEngine};
use polars::prelude::*;
use std::collections::HashMap;

const SYMBOLS: &[&str] = &[
    "BTCUSDT", "ETHUSDT", "SOLUSDT", "XRPUSDT", "DOGEUSDT", "ADAUSDT",
];
const CANDLES: u32 = 3000;
const HOLD_BARS: usize = 21;
const TAKER_FEE: f64 = 0.001;
const RESAMPLE_BLOCKS: usize = 6;
const FAILURE_BLOCKS: [usize; 2] = [2, 3];

#[derive(Clone, Debug)]
struct TradeRecord {
    symbol: String,
    direction: i32,
    signal_time: String,
    entry_time: String,
    exit_time: String,
    net_return_pct: f64,
}

#[derive(Default, Clone, Debug)]
struct SideStats {
    trades: usize,
    return_pct: f64,
    wins: usize,
}

impl SideStats {
    fn add(&mut self, ret: f64) {
        self.trades += 1;
        self.return_pct += ret;
        if ret > 0.0 {
            self.wins += 1;
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
    let close = df.column("close")?.f64()?;
    let n = close.len();
    let mut signals = vec![0i32; n];

    let ema_12 = calculate_ema(&close, 12)?;
    let ema_26 = calculate_ema(&close, 26)?;
    let mut macd_line = vec![None; n];
    for i in 0..n {
        if let (Some(fast), Some(slow)) = (ema_12[i], ema_26[i]) {
            macd_line[i] = Some(fast - slow);
        }
    }

    let macd_values: Vec<Option<f64>> = macd_line.clone();
    let mut signal_line = vec![None; n];
    let period = 9usize;
    let multiplier = 2.0 / (period as f64 + 1.0);
    let first_valid = macd_values.iter().position(|v| v.is_some()).unwrap_or(n);
    if n > first_valid + period {
        let seed: f64 = macd_values[first_valid..first_valid + period]
            .iter()
            .filter_map(|v| *v)
            .sum();
        signal_line[first_valid + period - 1] = Some(seed / period as f64);
        for i in first_valid + period..n {
            if let (Some(prev), Some(curr)) = (signal_line[i - 1], macd_values[i]) {
                signal_line[i] = Some((curr - prev) * multiplier + prev);
            }
        }
        for i in 26..n {
            if let (Some(macd_curr), Some(sig_curr)) = (macd_line[i], signal_line[i]) {
                if macd_curr > sig_curr {
                    signals[i] = 1;
                } else if macd_curr < sig_curr {
                    signals[i] = -1;
                }
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

fn backtest_trades(df: &DataFrame, symbol: &str, signals: &[i32]) -> Result<Vec<TradeRecord>> {
    let open = df.column("open")?.f64()?;
    let times = df.column("time")?.datetime()?;
    let n = open.len();
    let mut trades = Vec::new();
    let mut i = 200usize;

    while i + HOLD_BARS + 1 < n {
        let signal = signals.get(i).copied().unwrap_or(0);
        if signal == 0 {
            i += 1;
            continue;
        }
        let entry_idx = i + 1;
        let exit_idx = i + 1 + HOLD_BARS;
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
        let signal_ts = times
            .as_datetime_iter()
            .nth(i)
            .flatten()
            .map(|dt| dt.and_utc().timestamp_millis())
            .unwrap_or_default();
        let entry_ts = times
            .as_datetime_iter()
            .nth(entry_idx)
            .flatten()
            .map(|dt| dt.and_utc().timestamp_millis())
            .unwrap_or_default();
        let exit_ts = times
            .as_datetime_iter()
            .nth(exit_idx)
            .flatten()
            .map(|dt| dt.and_utc().timestamp_millis())
            .unwrap_or_default();
        trades.push(TradeRecord {
            symbol: symbol.to_string(),
            direction: signal,
            signal_time: ts_to_string(signal_ts),
            entry_time: ts_to_string(entry_ts),
            exit_time: ts_to_string(exit_ts),
            net_return_pct: net,
        });
        i = exit_idx;
    }
    Ok(trades)
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
    println!("MACD+Regime failing pair = blocks {:?}", FAILURE_BLOCKS);
    for &idx in &FAILURE_BLOCKS {
        let (start, end) = blocks[idx];
        let df = data_cache.get("BTCUSDT").unwrap();
        let times = df.column("time")?.datetime()?;
        let start_ts = times
            .as_datetime_iter()
            .nth(start)
            .flatten()
            .map(|dt| dt.and_utc().timestamp_millis())
            .unwrap_or_default();
        let end_ts = times
            .as_datetime_iter()
            .nth(end.saturating_sub(1))
            .flatten()
            .map(|dt| dt.and_utc().timestamp_millis())
            .unwrap_or_default();
        println!(
            "Block {} => {} .. {} (rows {}..{})",
            idx,
            ts_to_string(start_ts),
            ts_to_string(end_ts),
            start,
            end.saturating_sub(1)
        );
    }

    println!("\nPer-symbol decomposition on failing pair:");
    println!(
        "{:<10} {:>9} {:>9} {:>9} {:>9} {:>10}",
        "Symbol", "LongRet%", "LongTrd", "ShortRet%", "ShortTrd", "TotalRet%"
    );

    let mut all_worst: Vec<TradeRecord> = Vec::new();
    for symbol in SYMBOLS {
        let df = data_cache.get(&symbol.to_string()).unwrap();
        let mut long = SideStats::default();
        let mut short = SideStats::default();
        let mut symbol_worst: Vec<TradeRecord> = Vec::new();

        for &block_idx in &FAILURE_BLOCKS {
            let (start, end) = blocks[block_idx];
            let times = df.column("time")?.datetime()?;
            let start_ts = times
                .as_datetime_iter()
                .nth(start)
                .flatten()
                .map(|dt| dt.and_utc().timestamp_millis())
                .unwrap_or_default();
            let end_ts = times
                .as_datetime_iter()
                .nth(end.saturating_sub(1))
                .flatten()
                .map(|dt| dt.and_utc().timestamp_millis())
                .unwrap_or_default();
            println!(
                "  {} block {} => {} .. {}",
                symbol,
                block_idx,
                ts_to_string(start_ts),
                ts_to_string(end_ts)
            );
            let slice = df.slice(start as i64, end - start);
            let signals = generate_macd_regime_signals(&slice)?;
            let trades = backtest_trades(&slice, symbol, &signals)?;
            for tr in &trades {
                if tr.direction > 0 {
                    long.add(tr.net_return_pct);
                } else {
                    short.add(tr.net_return_pct);
                }
            }
            symbol_worst.extend(trades);
        }

        println!(
            "{:<10} {:+9.1} {:>9} {:+9.1} {:>9} {:+10.1}",
            symbol,
            long.return_pct,
            long.trades,
            short.return_pct,
            short.trades,
            long.return_pct + short.return_pct
        );
        symbol_worst.sort_by(|a, b| a.net_return_pct.partial_cmp(&b.net_return_pct).unwrap());
        all_worst.extend(symbol_worst.into_iter().take(2));
    }

    all_worst.sort_by(|a, b| a.net_return_pct.partial_cmp(&b.net_return_pct).unwrap());
    println!("\nWorst trades across the failing pair:");
    for tr in all_worst.iter().take(12) {
        let side = if tr.direction > 0 { "LONG" } else { "SHORT" };
        println!(
            "{:<8} {:<5} signal {} entry {} exit {} ret {:+.2}%",
            tr.symbol, side, tr.signal_time, tr.entry_time, tr.exit_time, tr.net_return_pct
        );
    }

    Ok(())
}
