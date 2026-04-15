//! Bear market regime filter testing with Sharpe ratio

use anyhow::Result;
use krypto::data::loader::DataLoader;
use polars::prelude::*;

const SYMBOLS: &[&str] = &[
    "BTCUSDT", "ETHUSDT", "SOLUSDT", "DOGEUSDT", "XRPUSDT", "ADAUSDT",
];
const TAKER_FEE: f64 = 0.001;
const SLIPPAGE_BPS: f64 = 5.0;
const HOLD_BARS: usize = 21;
const PERIOD: usize = 20;

#[tokio::main]
async fn main() -> Result<()> {
    println!("=== Bear Market Regime Filter Testing ===\n");

    let loader = DataLoader::new(None, None);
    let mut data: Vec<(&str, DataFrame)> = Vec::new();

    for symbol in SYMBOLS {
        println!("Loading {}...", symbol);
        let df = loader.fetch_data(symbol, "1d", 3000).await?;
        data.push((*symbol, df));
    }

    println!("\n=== All Regime Configs (with MACD) ===\n");
    println!(
        "{:<25} {:>10} {:>7} {:>8} {:>8} {:>10} {:>6}",
        "Config", "Return", "Trades", "WinRate", "MaxDD", "Sharpe", "Days"
    );
    println!("{}", "-".repeat(78));

    let configs = vec![
        ("SMA 200", RegimeConfig::SMA200),
        ("SMA 50", RegimeConfig::SMA50),
        ("SMA 200 Rising", RegimeConfig::SMA200Rising),
        ("SMA 50 Rising", RegimeConfig::SMA50Rising),
        ("Combined 200", RegimeConfig::Combined200),
        ("Combined 50", RegimeConfig::Combined50),
        ("ADX > 25", RegimeConfig::ADX),
        ("Low Vol", RegimeConfig::LowVol),
        ("No Regime", RegimeConfig::None),
    ];

    let mut best_sharpe: f64 = f64::NEG_INFINITY;
    let mut best_name = "";
    let mut results_vec = Vec::new();

    for (name, config) in &configs {
        let result = test_config(&data, config.clone(), true)?;
        let annual_sharpe = result.sharpe * (365.0_f64).sqrt();
        println!(
            "{:<25} {:>9.0}% {:>7} {:>7.1}% {:>7.1}% {:>10.2} {:>6.0}",
            name,
            result.avg_return,
            result.total_trades,
            result.win_rate * 100.0,
            result.max_dd * 100.0,
            annual_sharpe,
            result.days
        );
        results_vec.push((*name, result.clone()));

        if annual_sharpe > best_sharpe {
            best_sharpe = annual_sharpe;
            best_name = name;
        }
    }

    println!(
        "\n=== Best Config by Sharpe: {} (Sharpe: {:.2}) ===",
        best_name, best_sharpe
    );

    // Per-symbol analysis with best config
    let best_config = configs
        .iter()
        .find(|(n, _)| n == &best_name)
        .unwrap()
        .1
        .clone();

    println!("\n=== Per-Symbol Analysis ({}) ===\n", best_name);
    println!(
        "{:<10} {:>10} {:>7} {:>8} {:>8} {:>10} {:>6}",
        "Symbol", "Return", "Trades", "WinRate", "MaxDD", "Sharpe", "Days"
    );
    println!("{}", "-".repeat(63));

    let mut total_return = 0.0;
    let mut total_trades = 0;
    let mut total_wins = 0;

    for (symbol, df) in &data {
        let result = test_single_symbol(df, best_config.clone(), true)?;
        let annual_sharpe = result.sharpe * (365.0_f64).sqrt();
        println!(
            "{:<10} {:>9.0}% {:>7} {:>7.1}% {:>7.1}% {:>10.2} {:>6.0}",
            symbol,
            result.return_pct,
            result.trades,
            result.win_rate * 100.0,
            result.max_dd * 100.0,
            annual_sharpe,
            result.days
        );
        total_return += result.return_pct;
        total_trades += result.trades;
        total_wins += (result.trades as f64 * result.win_rate) as usize;
    }

    println!("{}", "-".repeat(63));
    println!(
        "{:<10} {:>9.0}% {:>7} {:>7.1}%",
        "AVG",
        total_return / SYMBOLS.len() as f64,
        total_trades,
        total_wins as f64 / total_trades as f64 * 100.0
    );

    // Compare specific configs
    println!("\n=== Key Comparisons ===\n");

    let sma200 = results_vec
        .iter()
        .find(|(n, _)| *n == "SMA 200")
        .unwrap()
        .1
        .clone();
    let combined = results_vec
        .iter()
        .find(|(n, _)| *n == "Combined 200")
        .unwrap()
        .1
        .clone();
    let none = results_vec
        .iter()
        .find(|(n, _)| *n == "No Regime")
        .unwrap()
        .1
        .clone();

    println!("SMA 200 vs No Regime:");
    println!(
        "  SMA 200:  {:.0}% return, {} trades, {:.1}% win rate, {:.2} Sharpe",
        sma200.avg_return,
        sma200.total_trades,
        sma200.win_rate * 100.0,
        sma200.sharpe * (365.0_f64).sqrt()
    );
    println!(
        "  No Regime: {:.0}% return, {} trades, {:.1}% win rate, {:.2} Sharpe",
        none.avg_return,
        none.total_trades,
        none.win_rate * 100.0,
        none.sharpe * (365.0_f64).sqrt()
    );

    println!("\nSMA 200 vs Combined (SMA 200 + Rising):");
    println!(
        "  SMA 200:     {:.0}% return, {} trades, {:.1}% win rate, {:.2} Sharpe",
        sma200.avg_return,
        sma200.total_trades,
        sma200.win_rate * 100.0,
        sma200.sharpe * (365.0_f64).sqrt()
    );
    println!(
        "  Combined:    {:.0}% return, {} trades, {:.1}% win rate, {:.2} Sharpe",
        combined.avg_return,
        combined.total_trades,
        combined.win_rate * 100.0,
        combined.sharpe * (365.0_f64).sqrt()
    );

    // Conclusion
    println!("\n=== CONCLUSION ===\n");
    println!("Best regime filter: {}", best_name);
    println!("Reasoning:");
    if best_name == "SMA 200" {
        println!("- Simple price > SMA 200 filter provides best balance");
        println!("- Filters out bear market periods effectively");
        println!("- Adding 'SMA rising' constraint is TOO restrictive");
    } else if best_name == "Combined 200" {
        println!("- Combined filter provides best risk-adjusted returns");
        println!("- Fewer trades but higher quality");
    } else if best_name == "No Regime" {
        println!("- No regime filter works best");
        println!("- Regime filters are too restrictive for this strategy");
    }

    Ok(())
}

#[derive(Clone, Debug)]
enum RegimeConfig {
    SMA200,
    SMA50,
    SMA200Rising,
    SMA50Rising,
    Combined200,
    Combined50,
    ADX,
    LowVol,
    None,
}

#[derive(Clone)]
struct BacktestResult {
    avg_return: f64,
    total_trades: usize,
    win_rate: f64,
    max_dd: f64,
    sharpe: f64,
    days: f64,
}

struct SingleResult {
    return_pct: f64,
    trades: usize,
    win_rate: f64,
    max_dd: f64,
    sharpe: f64,
    days: f64,
}

fn test_config(
    data: &[(&str, DataFrame)],
    config: RegimeConfig,
    use_macd: bool,
) -> Result<BacktestResult> {
    let mut returns = Vec::new();
    let mut all_trades = 0;
    let mut all_wins = 0;
    let mut max_dd: f64 = 0.0;
    let mut sharpes = Vec::new();
    let mut days: f64 = 0.0;

    for (_, df) in data {
        let result = test_single_symbol(df, config.clone(), use_macd)?;
        returns.push(result.return_pct);
        all_trades += result.trades;
        all_wins += (result.trades as f64 * result.win_rate) as usize;
        max_dd = max_dd.max(result.max_dd);
        sharpes.push(result.sharpe);
        days = days.max(result.days);
    }

    let avg_return = returns.iter().sum::<f64>() / returns.len() as f64;
    let avg_sharpe = sharpes.iter().sum::<f64>() / sharpes.len() as f64;

    Ok(BacktestResult {
        avg_return,
        total_trades: all_trades,
        win_rate: if all_trades > 0 {
            all_wins as f64 / all_trades as f64
        } else {
            0.0
        },
        max_dd,
        sharpe: avg_sharpe,
        days,
    })
}

fn test_single_symbol(
    df: &DataFrame,
    config: RegimeConfig,
    use_macd: bool,
) -> Result<SingleResult> {
    let close = df.column("close")?.f64()?;
    let high = df.column("high")?.f64()?;
    let low = df.column("low")?.f64()?;
    let n = close.len();

    // Generate Turtle breakout signals
    let mut turtle_signals = vec![false; n];
    for i in (PERIOD + 1)..n {
        let mut highest = f64::NEG_INFINITY;
        for j in (i - PERIOD - 1)..(i - 1) {
            if let Some(h) = high.get(j) {
                highest = highest.max(h);
            }
        }
        if let Some(prev_close) = close.get(i - 1) {
            if prev_close > highest {
                turtle_signals[i] = true;
            }
        }
    }

    let macd_signals = generate_macd_signals(&close)?;
    let regime_filter = generate_regime_filter(&close, &high, &low, &config)?;

    let mut final_signals = vec![false; n];
    for i in 0..n {
        let turtle = turtle_signals[i];
        let macd = if use_macd { macd_signals[i] } else { true };
        let regime = regime_filter[i];
        final_signals[i] = turtle && macd && regime;
    }

    let slippage = SLIPPAGE_BPS / 10000.0;
    let mut equity: f64 = 1.0;
    let mut max_equity: f64 = 1.0;
    let mut max_dd: f64 = 0.0;
    let mut trades: usize = 0;
    let mut wins: usize = 0;
    let mut in_trade = false;
    let mut entry_idx: usize = 0;
    let mut entry_price: f64 = 0.0;
    let mut returns: Vec<f64> = Vec::new();

    for i in 0..n {
        let price = close.get(i).unwrap_or(0.0);

        if !in_trade && final_signals[i] {
            in_trade = true;
            entry_idx = i;
            entry_price = price * (1.0 + slippage);
        } else if in_trade && (i - entry_idx) >= HOLD_BARS {
            let exit_price = price * (1.0 - slippage);
            let gross_return = (exit_price - entry_price) / entry_price;
            let net_return = gross_return - TAKER_FEE * 2.0;

            equity *= 1.0 + net_return;
            max_equity = max_equity.max(equity);
            max_dd = max_dd.max((max_equity - equity) / max_equity);

            trades += 1;
            returns.push(net_return);
            if net_return > 0.0 {
                wins += 1;
            }

            in_trade = false;
        }
    }

    let avg_return = if !returns.is_empty() {
        returns.iter().sum::<f64>() / returns.len() as f64
    } else {
        0.0
    };
    let std_return = if returns.len() > 1 {
        let variance = returns
            .iter()
            .map(|r| (r - avg_return).powi(2))
            .sum::<f64>()
            / (returns.len() - 1) as f64;
        variance.sqrt()
    } else {
        1.0
    };

    let daily_sharpe = if std_return > 0.0 {
        avg_return / std_return
    } else {
        0.0
    };

    Ok(SingleResult {
        return_pct: (equity - 1.0) * 100.0,
        trades,
        win_rate: if trades > 0 {
            wins as f64 / trades as f64
        } else {
            0.0
        },
        max_dd,
        sharpe: daily_sharpe,
        days: n as f64,
    })
}

fn generate_macd_signals(close: &ChunkedArray<Float64Type>) -> Result<Vec<bool>> {
    let n = close.len();
    let mut signals = vec![false; n];

    let fast_ema = calculate_ema(close, 12)?;
    let slow_ema = calculate_ema(close, 26)?;
    let macd_line: Vec<f64> = (0..n).map(|i| fast_ema[i] - slow_ema[i]).collect();
    let signal_line = calculate_ema_series(&macd_line, 9);

    for i in 1..n {
        signals[i] = macd_line[i] > signal_line[i];
    }

    Ok(signals)
}

fn calculate_ema(close: &ChunkedArray<Float64Type>, period: usize) -> Result<Vec<f64>> {
    let n = close.len();
    let mut ema = vec![0.0; n];
    let multiplier = 2.0 / (period as f64 + 1.0);

    let mut sum = 0.0;
    for i in 0..period.min(n) {
        sum += close.get(i).unwrap_or(0.0);
        ema[i] = sum / (i + 1) as f64;
    }
    if period <= n {
        ema[period - 1] = sum / period as f64;
    }

    for i in period..n {
        let price = close.get(i).unwrap_or(0.0);
        ema[i] = (price - ema[i - 1]) * multiplier + ema[i - 1];
    }

    Ok(ema)
}

fn calculate_ema_series(data: &[f64], period: usize) -> Vec<f64> {
    let n = data.len();
    let mut ema = vec![0.0; n];
    let multiplier = 2.0 / (period as f64 + 1.0);

    let mut sum = 0.0;
    for i in 0..period.min(n) {
        sum += data[i];
        ema[i] = sum / (i + 1) as f64;
    }
    if period <= n {
        ema[period - 1] = sum / period as f64;
    }

    for i in period..n {
        ema[i] = (data[i] - ema[i - 1]) * multiplier + ema[i - 1];
    }

    ema
}

fn generate_regime_filter(
    close: &ChunkedArray<Float64Type>,
    high: &ChunkedArray<Float64Type>,
    low: &ChunkedArray<Float64Type>,
    config: &RegimeConfig,
) -> Result<Vec<bool>> {
    let n = close.len();
    let mut filter = vec![true; n];

    match config {
        RegimeConfig::SMA200 => {
            let period = 200;
            for i in period..n {
                let sum: f64 = (i - period..i).map(|j| close.get(j).unwrap_or(0.0)).sum();
                let sma = sum / period as f64;
                filter[i] = close.get(i).unwrap_or(0.0) > sma;
            }
        }
        RegimeConfig::SMA50 => {
            let period = 50;
            for i in period..n {
                let sum: f64 = (i - period..i).map(|j| close.get(j).unwrap_or(0.0)).sum();
                let sma = sum / period as f64;
                filter[i] = close.get(i).unwrap_or(0.0) > sma;
            }
        }
        RegimeConfig::SMA200Rising => {
            let period = 200;
            for i in (period + 5)..n {
                let sum_now: f64 = (i - period..i).map(|j| close.get(j).unwrap_or(0.0)).sum();
                let sum_prev: f64 = (i - period - 5..i - 5)
                    .map(|j| close.get(j).unwrap_or(0.0))
                    .sum();
                filter[i] = sum_now > sum_prev;
            }
        }
        RegimeConfig::SMA50Rising => {
            let period = 50;
            for i in (period + 5)..n {
                let sum_now: f64 = (i - period..i).map(|j| close.get(j).unwrap_or(0.0)).sum();
                let sum_prev: f64 = (i - period - 5..i - 5)
                    .map(|j| close.get(j).unwrap_or(0.0))
                    .sum();
                filter[i] = sum_now > sum_prev;
            }
        }
        RegimeConfig::Combined200 => {
            let period = 200;
            for i in (period + 5)..n {
                let sum_now: f64 = (i - period..i).map(|j| close.get(j).unwrap_or(0.0)).sum();
                let sum_prev: f64 = (i - period - 5..i - 5)
                    .map(|j| close.get(j).unwrap_or(0.0))
                    .sum();
                let sma_now = sum_now / period as f64;
                let price_above = close.get(i).unwrap_or(0.0) > sma_now;
                let sma_rising = sum_now > sum_prev;
                filter[i] = price_above && sma_rising;
            }
        }
        RegimeConfig::Combined50 => {
            let period = 50;
            for i in (period + 5)..n {
                let sum_now: f64 = (i - period..i).map(|j| close.get(j).unwrap_or(0.0)).sum();
                let sum_prev: f64 = (i - period - 5..i - 5)
                    .map(|j| close.get(j).unwrap_or(0.0))
                    .sum();
                let sma_now = sum_now / period as f64;
                let price_above = close.get(i).unwrap_or(0.0) > sma_now;
                let sma_rising = sum_now > sum_prev;
                filter[i] = price_above && sma_rising;
            }
        }
        RegimeConfig::ADX => {
            let period = 14;
            for i in (period * 2)..n {
                let mut plus_dm = 0.0;
                let mut minus_dm = 0.0;
                let mut tr_sum = 0.0;

                for j in (i - period)..i {
                    let h_curr = high.get(j).unwrap_or(0.0);
                    let l_curr = low.get(j).unwrap_or(0.0);
                    let c_prev = close.get(j - 1).unwrap_or(0.0);
                    let h_prev = high.get(j - 1).unwrap_or(0.0);
                    let l_prev = low.get(j - 1).unwrap_or(0.0);

                    let up_move = h_curr - h_prev;
                    let down_move = l_prev - l_curr;

                    plus_dm += if up_move > down_move && up_move > 0.0 {
                        up_move
                    } else {
                        0.0
                    };
                    minus_dm += if down_move > up_move && down_move > 0.0 {
                        down_move
                    } else {
                        0.0
                    };

                    let tr = (h_curr - l_curr)
                        .max((h_curr - c_prev).abs())
                        .max((l_curr - c_prev).abs());
                    tr_sum += tr;
                }

                if tr_sum > 0.0 {
                    let plus_di = 100.0 * plus_dm / tr_sum;
                    let minus_di = 100.0 * minus_dm / tr_sum;
                    let dx = 100.0 * (plus_di - minus_di).abs() / (plus_di + minus_di + 0.0001);
                    filter[i] = dx > 25.0;
                }
            }
        }
        RegimeConfig::LowVol => {
            let period = 20;
            for i in (period * 2)..n {
                let mut atr_recent = 0.0;
                let mut atr_historical = 0.0;

                for j in (i - period)..i {
                    let h = high.get(j).unwrap_or(0.0);
                    let l = low.get(j).unwrap_or(0.0);
                    let c_prev = close.get(j - 1).unwrap_or(0.0);
                    let tr = (h - l).max((h - c_prev).abs()).max((l - c_prev).abs());
                    atr_recent += tr;
                }

                for j in (i - period * 2)..(i - period) {
                    let h = high.get(j).unwrap_or(0.0);
                    let l = low.get(j).unwrap_or(0.0);
                    let c_prev = close.get(j - 1).unwrap_or(0.0);
                    let tr = (h - l).max((h - c_prev).abs()).max((l - c_prev).abs());
                    atr_historical += tr;
                }

                filter[i] = atr_recent < atr_historical;
            }
        }
        RegimeConfig::None => {}
    }

    Ok(filter)
}
