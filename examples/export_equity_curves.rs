use anyhow::Result;
use krypto::{data::loader::DataLoader, features::indicators::FeatureEngine};
use polars::prelude::*;
use std::fs;

const SYMBOLS: &[&str] = &[
    "BTCUSDT", "ETHUSDT", "SOLUSDT", "DOGEUSDT", "XRPUSDT", "ADAUSDT",
];
const CANDLES: u32 = 3000;
const TAKER_FEE: f64 = 0.001;
const SLIPPAGE_BPS: f64 = 5.0;
const HOLD_BARS: usize = 21;
const PERIOD: usize = 20;

#[tokio::main]
async fn main() -> Result<()> {
    let loader = DataLoader::new(None, None);
    let mut data_cache: std::collections::HashMap<String, DataFrame> =
        std::collections::HashMap::new();

    for symbol in SYMBOLS {
        let raw = loader.fetch_data(symbol, "1d", CANDLES).await?;
        let df = FeatureEngine::add_technicals(&raw, None)?;
        data_cache.insert(symbol.to_string(), df);
    }

    let mut curves: Vec<(String, Vec<f64>)> = Vec::new();
    let mut max_len = 0usize;
    for symbol in SYMBOLS {
        if let Some(df) = data_cache.get(&symbol.to_string()) {
            let curve = run_backtest_with_equity(df)?;
            max_len = max_len.max(curve.len());
            curves.push((symbol.to_string(), curve));
        }
    }

    let mut portfolio_curve: Vec<f64> = vec![100.0];
    for i in 1..max_len {
        let mut sum = 0.0;
        let mut count = 0usize;
        for (_, curve) in &curves {
            if i < curve.len() {
                sum += curve[i] / curve[0];
                count += 1;
            }
        }
        if count > 0 {
            portfolio_curve.push(100.0 * (sum / count as f64));
        }
    }

    let mut out = String::new();
    out.push_str("{\n  \"curves\": [\n");
    for (idx, (name, curve)) in curves.iter().enumerate() {
        out.push_str(&format!("    {{\"name\":\"{}\",\"values\":[", name));
        for (i, v) in curve.iter().enumerate() {
            if i > 0 {
                out.push(',');
            }
            out.push_str(&format!("{:.6}", v));
        }
        out.push_str("]}");
        if idx + 1 != curves.len() {
            out.push(',');
        }
        out.push('\n');
    }
    out.push_str("  ],\n  \"portfolio\": [");
    for (i, v) in portfolio_curve.iter().enumerate() {
        if i > 0 {
            out.push(',');
        }
        out.push_str(&format!("{:.6}", v));
    }
    out.push_str("]\n}\n");

    fs::create_dir_all("charts")?;
    fs::write("charts/equity_curves.json", out)?;
    println!("wrote charts/equity_curves.json");
    Ok(())
}

fn run_backtest_with_equity(df: &DataFrame) -> Result<Vec<f64>> {
    let close = df.column("close")?.f64()?;
    let n = close.len();
    let sma_200 = calculate_sma(&close, 200)?;
    let macd_signals = generate_macd_signals(df)?;
    let turtle_signals = generate_turtle_signals(df, PERIOD)?;
    let mut equity_curve: Vec<f64> = vec![100.0];
    let mut equity: f64 = 100.0;
    let mut i = 200;
    while i < n.saturating_sub(HOLD_BARS + 1) {
        let turtle_signal = turtle_signals[i];
        if turtle_signal > 0 && i + 1 < n {
            let current_price = close.get(i).unwrap_or(0.0);
            let current_sma = sma_200[i].unwrap_or(0.0);
            let regime_ok = current_price > current_sma;
            let macd_ok = macd_signals[i] > 0;
            if regime_ok && macd_ok {
                let entry_idx = i + 1;
                let entry_price = match close.get(entry_idx) {
                    Some(p) if p > 0.0 => p,
                    _ => {
                        i += 1;
                        continue;
                    }
                };
                let exit_idx = (entry_idx + HOLD_BARS).min(n - 1);
                let exit_price = close.get(exit_idx).unwrap_or(entry_price);
                let gross_return = (exit_price / entry_price - 1.0) * 100.0;
                let slippage_cost = SLIPPAGE_BPS / 100.0 * 2.0;
                let net_return = gross_return - 2.0 * TAKER_FEE * 100.0 - slippage_cost;
                equity *= 1.0 + net_return / 100.0;
                for _ in 0..HOLD_BARS {
                    equity_curve.push(equity);
                }
                i = exit_idx + 1;
                continue;
            }
        }
        equity_curve.push(equity);
        i += 1;
    }
    Ok(equity_curve)
}

fn calculate_sma(close: &ChunkedArray<Float64Type>, period: usize) -> Result<Vec<Option<f64>>> {
    let n = close.len();
    let mut sma: Vec<Option<f64>> = vec![None; n];
    for i in period..n {
        let sum: f64 = (0..period).filter_map(|j| close.get(i - j)).sum();
        sma[i] = Some(sum / period as f64);
    }
    Ok(sma)
}

fn generate_turtle_signals(df: &DataFrame, period: usize) -> Result<Vec<i32>> {
    let close = df.column("close")?.f64()?;
    let n = close.len();
    let mut signals = vec![0; n];
    for i in period..n.saturating_sub(1) {
        let window = &close.slice((i - period) as i64, period);
        let max_val = window.max().unwrap_or(f64::NAN);
        let current = close.get(i).unwrap_or(f64::NAN);
        if current > max_val {
            signals[i] = 1;
        }
    }
    Ok(signals)
}

fn generate_macd_signals(df: &DataFrame) -> Result<Vec<i32>> {
    let close = df.column("close")?.f64()?;
    let n = close.len();
    let ema12 = calculate_ema(&close, 12)?;
    let ema26 = calculate_ema(&close, 26)?;
    let mut macd: Vec<f64> = vec![0.0; n];
    for i in 0..n {
        if let (Some(e12), Some(e26)) = (ema12[i], ema26[i]) {
            macd[i] = e12 - e26;
        }
    }
    let signal_line = calculate_ema_from_slice(&macd, 9)?;
    let mut signals = vec![0; n];
    for i in 0..n {
        if let Some(s) = signal_line[i] {
            let macd_val = if i < macd.len() { macd[i] } else { 0.0 };
            if macd_val > s {
                signals[i] = 1;
            }
        }
    }
    Ok(signals)
}

fn calculate_ema(close: &ChunkedArray<Float64Type>, period: usize) -> Result<Vec<Option<f64>>> {
    let n = close.len();
    let mut ema: Vec<Option<f64>> = vec![None; n];
    let mult = 2.0 / (period as f64 + 1.0);
    let mut sum = 0.0;
    for i in 0..period.min(n) {
        sum += close.get(i).unwrap_or(0.0);
    }
    if n >= period {
        ema[period - 1] = Some(sum / period as f64);
        for i in period..n {
            let current = close.get(i).unwrap_or(0.0);
            let prev_ema = ema[i - 1].unwrap_or(0.0);
            ema[i] = Some((current - prev_ema) * mult + prev_ema);
        }
    }
    Ok(ema)
}

fn calculate_ema_from_slice(data: &[f64], period: usize) -> Result<Vec<Option<f64>>> {
    let n = data.len();
    let mut ema: Vec<Option<f64>> = vec![None; n];
    let mult = 2.0 / (period as f64 + 1.0);
    let mut sum = 0.0;
    for i in 0..period.min(n) {
        sum += data[i];
    }
    if n >= period {
        ema[period - 1] = Some(sum / period as f64);
        for i in period..n {
            let current = data[i];
            let prev_ema = ema[i - 1].unwrap_or(0.0);
            ema[i] = Some((current - prev_ema) * mult + prev_ema);
        }
    }
    Ok(ema)
}
