//! Test Turtle + Regime + MACD on 4h data
//!
//! Goal: More granular entry timing, more trades, better statistics

use anyhow::Result;
use krypto::{data::loader::DataLoader, features::indicators::FeatureEngine};
use polars::prelude::*;

const SYMBOLS: &[&str] = &[
    "BTCUSDT", "ETHUSDT", "SOLUSDT", "DOGEUSDT", "XRPUSDT", "ADAUSDT",
];
const CANDLES: u32 = 5000; // 4h data has more bars
const TAKER_FEE: f64 = 0.001;
const SLIPPAGE_BPS: f64 = 5.0;
const HOLD_BARS_4H: usize = 126; // 21 days * 6 bars/day = 126 bars
const PERIOD: usize = 20; // 20 bars = 80 hours ≈ 3.3 days

#[tokio::main]
async fn main() -> Result<()> {
    println!("=== 4H DATA TEST: Turtle + Regime + MACD ===\n");
    println!("Config:");
    println!("  - Timeframe: 4h (6 bars per day)");
    println!("  - Hold period: {} bars (21 days)", HOLD_BARS_4H);
    println!("  - Breakout period: {} bars (3.3 days)", PERIOD);
    println!("  - SMA regime: 1200 bars (200 days * 6)");
    println!();

    let loader = DataLoader::new(None, None);

    let mut data_cache: std::collections::HashMap<String, DataFrame> =
        std::collections::HashMap::new();

    for symbol in SYMBOLS {
        print!("Loading {} 4h... ", symbol);
        match loader.fetch_data(symbol, "4h", CANDLES).await {
            Ok(raw) => {
                let df = FeatureEngine::add_technicals(&raw, None)?;
                println!(
                    "{} bars ({:.1} years)",
                    df.height(),
                    df.height() as f64 / 6.0 / 365.0
                );
                data_cache.insert(symbol.to_string(), df);
            }
            Err(e) => {
                println!("SKIP ({})", e);
            }
        }
    }

    println!("\n{}", "=".repeat(80));
    println!("BACKTEST RESULTS");
    println!("{}", "=".repeat(80));

    let mut portfolio_return = 0.0;
    let mut total_trades = 0;
    let mut total_wins = 0;
    let mut total_max_dd = 0.0;

    for symbol in SYMBOLS {
        if let Some(df) = data_cache.get(&symbol.to_string()) {
            let result = run_backtest_4h(df)?;

            portfolio_return += result.total_return_pct;
            total_trades += result.trades;
            total_wins += result.wins;
            total_max_dd += result.max_drawdown;

            println!(
                "{:12} | Return: {:>8.1}% | Trades: {:>3} | WinRate: {:>5.1}% | MaxDD: {:>6.1}%",
                symbol,
                result.total_return_pct,
                result.trades,
                if result.trades > 0 {
                    result.wins as f64 / result.trades as f64 * 100.0
                } else {
                    0.0
                },
                result.max_drawdown
            );
        }
    }

    let avg_dd = total_max_dd / data_cache.len() as f64;
    let overall_win_rate = if total_trades > 0 {
        total_wins as f64 / total_trades as f64 * 100.0
    } else {
        0.0
    };

    println!("{}", "-".repeat(80));
    println!(
        "{:12} | Return: {:>8.1}% | Trades: {:>3} | WinRate: {:>5.1}% | AvgDD: {:>6.1}%",
        "PORTFOLIO", portfolio_return, total_trades, overall_win_rate, avg_dd
    );

    println!("\n{}", "=".repeat(80));
    println!("COMPARISON: 4H vs DAILY");
    println!("{}", "=".repeat(80));
    println!("\n4H advantages:");
    println!("  - More granular entry (6× per day vs 1×)");
    println!("  - More trades (better statistics)");
    println!("  - Earlier entry on breakouts");
    println!("\n4H disadvantages:");
    println!("  - Higher fee drag (more trades)");
    println!("  - More noise (false breakouts)");
    println!("  - Need more data for same time period");

    Ok(())
}

struct BacktestResult {
    total_return_pct: f64,
    trades: usize,
    wins: usize,
    max_drawdown: f64,
}

fn run_backtest_4h(df: &DataFrame) -> Result<BacktestResult> {
    let close = df.column("close")?.f64()?;
    let n = close.len();

    // For 4h data: SMA 200 days = 200 * 6 = 1200 bars
    let sma_period = 1200;
    let sma = calculate_sma(&close, sma_period)?;

    let macd_signals = generate_macd_signals(df)?;
    let turtle_signals = generate_turtle_signals(df, PERIOD)?;

    let mut trade_returns: Vec<f64> = Vec::new();
    let mut equity: f64 = 100.0;
    let mut peak: f64 = 100.0;
    let mut max_dd: f64 = 0.0;

    // Start after warmup (SMA 200 days)
    let mut i = sma_period;

    while i < n.saturating_sub(HOLD_BARS_4H + 1) {
        let turtle_signal = turtle_signals[i];

        if turtle_signal > 0 && i + 1 < n {
            // Regime filter
            let current_price = close.get(i).unwrap_or(0.0);
            let current_sma = sma[i].unwrap_or(0.0);
            let regime_ok = current_price > current_sma;

            // MACD confirmation
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

                let exit_idx = (entry_idx + HOLD_BARS_4H).min(n - 1);
                let exit_price = close.get(exit_idx).unwrap_or(entry_price);

                let gross_return = (exit_price / entry_price - 1.0) * 100.0;
                let slippage_cost = SLIPPAGE_BPS / 100.0 * 2.0;
                let net_return = gross_return - 2.0 * TAKER_FEE * 100.0 - slippage_cost;

                trade_returns.push(net_return);
                equity *= 1.0 + net_return / 100.0;

                peak = peak.max(equity);
                let dd = (peak - equity) / peak * 100.0;
                max_dd = max_dd.max(dd);

                i = exit_idx + 1;
                continue;
            }
        }

        i += 1;
    }

    let trades = trade_returns.len();
    let total_return_pct = trade_returns.iter().sum();
    let wins = trade_returns.iter().filter(|&&r| r > 0.0).count();

    Ok(BacktestResult {
        total_return_pct,
        trades,
        wins,
        max_drawdown: max_dd,
    })
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
        let min_val = window.min().unwrap_or(f64::NAN);
        let current = close.get(i).unwrap_or(f64::NAN);

        if current > max_val {
            signals[i] = 1;
        } else if current < min_val {
            signals[i] = -1;
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
            } else if macd_val < s {
                signals[i] = -1;
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
