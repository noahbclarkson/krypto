//! Test stop loss during hold for Turtle + Regime + MACD strategy

use anyhow::Result;
use krypto::{data::loader::DataLoader, features::indicators::FeatureEngine};
use polars::prelude::*;

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
    println!("=== STOP LOSS TEST: Turtle + Regime + MACD ===\n");

    let loader = DataLoader::new(None, None);

    let mut data_cache: std::collections::HashMap<String, DataFrame> =
        std::collections::HashMap::new();

    for symbol in SYMBOLS {
        print!("Loading {}... ", symbol);
        match loader.fetch_data(symbol, "1d", CANDLES).await {
            Ok(raw) => {
                let df = FeatureEngine::add_technicals(&raw, None)?;
                println!("{} bars", df.height());
                data_cache.insert(symbol.to_string(), df);
            }
            Err(e) => {
                println!("SKIP ({})", e);
            }
        }
    }

    let stop_configs = vec![
        ("No Stop (baseline)", None, None),
        ("5% Fixed Stop", Some(0.05), None),
        ("10% Fixed Stop", Some(0.10), None),
        ("15% Fixed Stop", Some(0.15), None),
        ("ATR×1.5 Stop", None, Some(1.5)),
        ("ATR×2.0 Stop", None, Some(2.0)),
        ("ATR×3.0 Stop", None, Some(3.0)),
        ("ATR×4.0 Stop", None, Some(4.0)),
    ];

    println!(
        "\nTesting {} stop loss configurations...\n",
        stop_configs.len()
    );

    for (name, fixed_pct, atr_mult) in &stop_configs {
        println!("{}", "=".repeat(80));
        println!("CONFIG: {}", name);
        println!("{}", "=".repeat(80));

        let mut portfolio_return = 0.0;
        let mut total_trades = 0;
        let mut total_wins = 0;
        let mut total_stopped_out = 0;
        let mut total_max_dd = 0.0;

        for symbol in SYMBOLS {
            if let Some(df) = data_cache.get(&symbol.to_string()) {
                let result = run_backtest_with_stop(df, *fixed_pct, *atr_mult)?;

                portfolio_return += result.total_return_pct;
                total_trades += result.trades;
                total_wins += result.wins;
                total_stopped_out += result.stopped_out;
                total_max_dd += result.max_drawdown;

                println!(
                    "{:12} | Return: {:>8.1}% | Trades: {:>3} | WinRate: {:>5.1}% | Stopped: {:>3} ({:>4.1}%) | MaxDD: {:>6.1}%",
                    symbol,
                    result.total_return_pct,
                    result.trades,
                    if result.trades > 0 { result.wins as f64 / result.trades as f64 * 100.0 } else { 0.0 },
                    result.stopped_out,
                    if result.trades > 0 { result.stopped_out as f64 / result.trades as f64 * 100.0 } else { 0.0 },
                    result.max_drawdown
                );
            }
        }

        let avg_dd = total_max_dd / data_cache.len() as f64;
        let stop_rate = if total_trades > 0 {
            total_stopped_out as f64 / total_trades as f64 * 100.0
        } else {
            0.0
        };
        let overall_win_rate = if total_trades > 0 {
            total_wins as f64 / total_trades as f64 * 100.0
        } else {
            0.0
        };

        println!("{}", "-".repeat(80));
        println!(
            "{:12} | Return: {:>8.1}% | Trades: {:>3} | WinRate: {:>5.1}% | Stopped: {:>3} ({:>4.1}%) | AvgDD: {:>6.1}%",
            "PORTFOLIO",
            portfolio_return,
            total_trades,
            overall_win_rate,
            total_stopped_out,
            stop_rate,
            avg_dd
        );
        println!();
    }

    println!("\n{}", "=".repeat(80));
    println!("SUMMARY");
    println!("{}", "=".repeat(80));
    println!("\nStop loss testing complete.");
    println!("Key metrics:");
    println!("  - Return: Portfolio total return");
    println!("  - Stopped: Trades exited early by stop loss");
    println!("  - MaxDD: Maximum drawdown per symbol");

    Ok(())
}

struct StopTestResult {
    total_return_pct: f64,
    trades: usize,
    wins: usize,
    stopped_out: usize,
    max_drawdown: f64,
}

fn run_backtest_with_stop(
    df: &DataFrame,
    fixed_stop_pct: Option<f64>,
    atr_stop_mult: Option<f64>,
) -> Result<StopTestResult> {
    let close = df.column("close")?.f64()?;
    let high = df.column("high")?.f64()?;
    let low = df.column("low")?.f64()?;
    let n = close.len();

    let sma_200 = calculate_sma(&close, 200)?;
    let atr_14 = calculate_atr(df, 14)?;
    let macd_signals = generate_macd_signals(df)?;
    let turtle_signals = generate_turtle_signals(df, PERIOD)?;

    let mut trade_returns: Vec<f64> = Vec::new();
    let mut stopped_out_count = 0;
    let mut equity: f64 = 100.0;
    let mut peak: f64 = 100.0;
    let mut max_dd: f64 = 0.0;

    let mut i = 200;

    while i < n.saturating_sub(1) {
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

                let stop_price = if let Some(pct) = fixed_stop_pct {
                    Some(entry_price * (1.0 - pct))
                } else if let Some(mult) = atr_stop_mult {
                    let atr_val = atr_14[entry_idx].unwrap_or(0.0);
                    if atr_val > 0.0 {
                        Some(entry_price - mult * atr_val)
                    } else {
                        None
                    }
                } else {
                    None
                };

                let mut exited_early = false;
                let mut exit_price = 0.0;
                let mut exit_idx = entry_idx + HOLD_BARS;

                for day in 1..=HOLD_BARS {
                    let bar_idx = entry_idx + day;
                    if bar_idx >= n {
                        exit_idx = bar_idx - 1;
                        break;
                    }

                    let day_low = low.get(bar_idx).unwrap_or(0.0);

                    if let Some(stop) = stop_price {
                        if day_low < stop {
                            exit_price = stop;
                            exit_idx = bar_idx;
                            exited_early = true;
                            stopped_out_count += 1;
                            break;
                        }
                    }
                }

                if !exited_early {
                    exit_idx = (entry_idx + HOLD_BARS).min(n - 1);
                    exit_price = close.get(exit_idx).unwrap_or(entry_price);
                }

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

    Ok(StopTestResult {
        total_return_pct,
        trades,
        wins,
        stopped_out: stopped_out_count,
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

fn calculate_atr(df: &DataFrame, period: usize) -> Result<Vec<Option<f64>>> {
    let high = df.column("high")?.f64()?;
    let low = df.column("low")?.f64()?;
    let close = df.column("close")?.f64()?;
    let n = high.len();

    let mut tr: Vec<f64> = vec![0.0; n];

    for i in 1..n {
        let h = high.get(i).unwrap_or(0.0);
        let l = low.get(i).unwrap_or(0.0);
        let c_prev = close.get(i - 1).unwrap_or(0.0);

        let hl = h - l;
        let hc = (h - c_prev).abs();
        let lc = (l - c_prev).abs();

        tr[i] = hl.max(hc).max(lc);
    }

    let mut atr: Vec<Option<f64>> = vec![None; n];

    for i in period..n {
        let sum: f64 = (0..period).map(|j| tr[i - j]).sum();
        atr[i] = Some(sum / period as f64);
    }

    Ok(atr)
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
