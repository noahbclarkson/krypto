//! Rigorous Validation: Turtle + Regime + MACD with FIXED Monte Carlo
//!
//! Fixes from audit:
//! 1. Signal rate matching (random = same count as Turtle)
//! 2. Slippage model (5 bps per trade)
//! 3. More symbols (LTC, BNB, ADA)
//! 4. Equity curve generation

use anyhow::Result;
use krypto::{data::loader::DataLoader, features::indicators::FeatureEngine};
use plotters::prelude::*;
use polars::prelude::*;
use std::collections::hash_map::DefaultHasher;
use std::hash::Hasher;

const SYMBOLS: &[&str] = &[
    "SOLUSDT", "DOGEUSDT", "BTCUSDT", "ETHUSDT", "XRPUSDT", "LTCUSDT", "BNBUSDT", "ADAUSDT",
];
const CANDLES: u32 = 3000;
const TAKER_FEE: f64 = 0.001;
const SLIPPAGE_BPS: f64 = 5.0; // 5 basis points per trade
const HOLD_BARS: usize = 21;
const PERIOD: usize = 20;
const N_MONTE_CARLO: usize = 100;

#[tokio::main]
async fn main() -> Result<()> {
    println!("=== RIGOROUS VALIDATION: TURTLE + REGIME + MACD (FIXED) ===\n");
    println!("Fixes applied:");
    println!("  - Signal rate matching (random = same count as Turtle)");
    println!("  - Slippage: {:.0} bps per trade", SLIPPAGE_BPS);
    println!("  - More symbols: {}\n", SYMBOLS.join(", "));

    let loader = DataLoader::new(None, None);

    // Load data
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

    // 1. Full history with slippage
    println!("\n{}", "=".repeat(80));
    println!("1. FULL HISTORY RESULTS (with slippage)");
    println!("{}", "=".repeat(80));

    let mut total_return = 0.0;
    let mut total_trades = 0;
    let mut total_wins = 0;
    let mut all_equity_curves: Vec<(String, Vec<f64>)> = Vec::new();

    for symbol in SYMBOLS {
        if let Some(df) = data_cache.get(&symbol.to_string()) {
            let (result, equity_curve) = run_combined_backtest_with_equity(df, true, true)?;
            total_return += result.total_return_pct;
            total_trades += result.trades;
            total_wins += (result.win_rate * result.trades as f64) as usize;
            all_equity_curves.push((symbol.to_string(), equity_curve));

            println!(
                "{:12} | Return: {:>8.1}% | Trades: {:>3} | WinRate: {:>5.1}%",
                symbol,
                result.total_return_pct,
                result.trades,
                result.win_rate * 100.0
            );
        }
    }

    println!("{}", "-".repeat(80));
    println!(
        "{:12} | Return: {:>8.1}% | Trades: {:>3} | WinRate: {:>5.1}%",
        "PORTFOLIO",
        total_return,
        total_trades,
        if total_trades > 0 {
            total_wins as f64 / total_trades as f64 * 100.0
        } else {
            0.0
        }
    );

    // 2. Monte Carlo with FIXED signal rate matching
    println!("\n\n{}", "=".repeat(80));
    println!("2. MONTE CARLO (FIXED: Signal rate matched)");
    println!("{}", "=".repeat(80));
    println!("Generate random signals with SAME count as Turtle.\n");

    for symbol in SYMBOLS {
        if let Some(df) = data_cache.get(&symbol.to_string()) {
            let turtle_signals = generate_turtle_signals(df, PERIOD)?;
            let turtle_count = turtle_signals.iter().filter(|&&s| s != 0).count();

            let real_result = run_combined_backtest(df, true, true)?;
            let mut random_returns: Vec<f64> = Vec::with_capacity(N_MONTE_CARLO);

            for seed in 0..N_MONTE_CARLO {
                let random_signals = generate_random_signals_matched(df, turtle_count, seed as u64);
                let result = run_backtest_with_signals(df, &random_signals, true)?;
                random_returns.push(result.total_return_pct);
            }

            random_returns.sort_by(|a, b| a.partial_cmp(b).unwrap());

            let real_return = real_result.total_return_pct;
            let percentile = random_returns
                .iter()
                .position(|&r| r > real_return)
                .unwrap_or(N_MONTE_CARLO);
            let p_value = percentile as f64 / N_MONTE_CARLO as f64;

            let mean_random = random_returns.iter().sum::<f64>() / N_MONTE_CARLO as f64;
            let median_random = random_returns[N_MONTE_CARLO / 2];

            println!(
                "{:12} | Real: {:>7.1}% | Random mean: {:>7.1}% | Median: {:>7.1}% | Pctl: {:>3}% | Signal count: {}",
                symbol,
                real_return,
                mean_random,
                median_random,
                (1.0 - p_value) * 100.0,
                turtle_count
            );
        }
    }

    // 3. Walk-forward validation
    println!("\n\n{}", "=".repeat(80));
    println!("3. WALK-FORWARD VALIDATION");
    println!("{}", "=".repeat(80));

    for symbol in SYMBOLS.iter().take(5) {
        if let Some(df) = data_cache.get(&symbol.to_string()) {
            let n = df.height();
            let quarter = n / 4;

            print!("{:12} |", symbol);

            for i in 0..4 {
                let start = i * quarter;
                let end = if i == 3 { n } else { (i + 1) * quarter };
                let period_df = df.slice(start as i64, end - start);

                let result = run_combined_backtest(&period_df, true, true)?;

                let indicator = if result.total_return_pct > 0.0 {
                    "✓"
                } else {
                    "✗"
                };
                print!(
                    " P{}: {:>6.1}% {} |",
                    i + 1,
                    result.total_return_pct,
                    indicator
                );
            }
            println!();
        }
    }

    // 4. Generate equity curve chart
    println!("\n\n{}", "=".repeat(80));
    println!("4. GENERATING EQUITY CURVE CHART");
    println!("{}", "=".repeat(80));

    let chart_path = "charts/rigorous_validation_equity.png";
    if let Err(e) = generate_equity_curve_chart(&all_equity_curves, chart_path) {
        println!("Chart generation failed: {}", e);
    } else {
        println!("Chart saved to: {}", chart_path);
    }

    // 5. Summary
    println!("\n\n{}", "=".repeat(80));
    println!("SUMMARY");
    println!("{}", "=".repeat(80));

    println!("\nConfiguration: Turtle + Regime Filter + MACD Confirmation");
    println!("Slippage: {:.0} bps per trade", SLIPPAGE_BPS);
    println!("Portfolio return: {:.1}% over ~8 years", total_return);
    println!(
        "Total trades: {} (avg {:.1} per symbol)",
        total_trades,
        total_trades as f64 / data_cache.len() as f64
    );
    println!(
        "Overall win rate: {:.1}%",
        if total_trades > 0 {
            total_wins as f64 / total_trades as f64 * 100.0
        } else {
            0.0
        }
    );

    println!("\n⚠️  REMAINING CONCERNS:");
    println!("  - Survivorship bias: Only symbols that survived");
    println!("  - Time-based exit: No risk management during 21-day hold");
    println!("  - Regime dependency: Strategy may underperform in bear markets");

    Ok(())
}

struct TrendResult {
    total_return_pct: f64,
    trades: usize,
    win_rate: f64,
}

fn run_combined_backtest(
    df: &DataFrame,
    use_regime_filter: bool,
    use_macd_confirm: bool,
) -> Result<TrendResult> {
    let (result, _) = run_combined_backtest_with_equity(df, use_regime_filter, use_macd_confirm)?;
    Ok(result)
}

fn run_combined_backtest_with_equity(
    df: &DataFrame,
    use_regime_filter: bool,
    use_macd_confirm: bool,
) -> Result<(TrendResult, Vec<f64>)> {
    let close = df.column("close")?.f64()?;
    let n = close.len();

    let sma_200 = calculate_sma(&close, 200)?;
    let turtle_signals = generate_turtle_signals(df, PERIOD)?;
    let macd_signals = generate_macd_signals(df)?;

    let mut trade_returns: Vec<f64> = Vec::new();
    let mut equity_curve: Vec<f64> = Vec::new();
    let mut equity = 100.0; // Start at 100%
    let mut i = 200;

    while i < n.saturating_sub(HOLD_BARS + 1) {
        let turtle_signal = turtle_signals[i];

        if turtle_signal != 0 && i + 1 + HOLD_BARS < n {
            let mut should_trade = true;

            if use_regime_filter && turtle_signal > 0 {
                let current_price = close.get(i).unwrap_or(0.0);
                let current_sma = sma_200[i].unwrap_or(0.0);
                if current_price <= current_sma {
                    should_trade = false;
                }
            }

            if use_macd_confirm && should_trade {
                let macd_signal = macd_signals[i];
                if turtle_signal > 0 && macd_signal <= 0 {
                    should_trade = false;
                } else if turtle_signal < 0 && macd_signal >= 0 {
                    should_trade = false;
                }
            }

            if should_trade {
                let entry_price = match close.get(i + 1) {
                    Some(p) if p > 0.0 => p,
                    _ => {
                        i += 1;
                        continue;
                    }
                };
                let exit_idx = i + 1 + HOLD_BARS;
                let exit_price = match close.get(exit_idx) {
                    Some(p) if p > 0.0 => p,
                    _ => {
                        i += 1;
                        continue;
                    }
                };

                let gross_return = if turtle_signal > 0 {
                    (exit_price / entry_price - 1.0) * 100.0
                } else {
                    (entry_price / exit_price - 1.0) * 100.0
                };

                // Apply slippage (reduces return)
                let slippage_cost = SLIPPAGE_BPS / 100.0 * 2.0; // 2× per trade
                let net_return = gross_return - 2.0 * TAKER_FEE * 100.0 - slippage_cost;

                trade_returns.push(net_return);
                equity *= 1.0 + net_return / 100.0;

                // Fill equity curve for hold period
                for _ in 0..HOLD_BARS {
                    equity_curve.push(equity);
                }

                i += HOLD_BARS + 1;
                continue;
            }
        }

        // No trade, equity stays same
        equity_curve.push(equity);
        i += 1;
    }

    let trades = trade_returns.len();
    let total_return_pct = trade_returns.iter().sum();
    let win_rate = if trades > 0 {
        trade_returns.iter().filter(|&&r| r > 0.0).count() as f64 / trades as f64
    } else {
        0.0
    };

    Ok((
        TrendResult {
            total_return_pct,
            trades,
            win_rate,
        },
        equity_curve,
    ))
}

fn run_backtest_with_signals(
    df: &DataFrame,
    signals: &[i32],
    use_regime_filter: bool,
) -> Result<TrendResult> {
    let close = df.column("close")?.f64()?;
    let n = close.len();

    let sma_200 = calculate_sma(&close, 200)?;

    let mut trade_returns: Vec<f64> = Vec::new();
    let mut i = 200;

    while i < n.saturating_sub(HOLD_BARS + 1) {
        let signal = signals[i];

        if signal != 0 && i + 1 + HOLD_BARS < n {
            let mut should_trade = true;

            if use_regime_filter && signal > 0 {
                let current_price = close.get(i).unwrap_or(0.0);
                let current_sma = sma_200[i].unwrap_or(0.0);
                if current_price <= current_sma {
                    should_trade = false;
                }
            }

            if should_trade {
                let entry_price = match close.get(i + 1) {
                    Some(p) if p > 0.0 => p,
                    _ => {
                        i += 1;
                        continue;
                    }
                };
                let exit_idx = i + 1 + HOLD_BARS;
                let exit_price = match close.get(exit_idx) {
                    Some(p) if p > 0.0 => p,
                    _ => {
                        i += 1;
                        continue;
                    }
                };

                let gross_return = if signal > 0 {
                    (exit_price / entry_price - 1.0) * 100.0
                } else {
                    (entry_price / exit_price - 1.0) * 100.0
                };

                let slippage_cost = SLIPPAGE_BPS / 100.0 * 2.0;
                let net_return = gross_return - 2.0 * TAKER_FEE * 100.0 - slippage_cost;

                trade_returns.push(net_return);
                i += HOLD_BARS + 1;
                continue;
            }
        }
        i += 1;
    }

    let trades = trade_returns.len();
    let total_return_pct = trade_returns.iter().sum();
    let win_rate = if trades > 0 {
        trade_returns.iter().filter(|&&r| r > 0.0).count() as f64 / trades as f64
    } else {
        0.0
    };

    Ok(TrendResult {
        total_return_pct,
        trades,
        win_rate,
    })
}

fn generate_random_signals_matched(df: &DataFrame, target_count: usize, seed: u64) -> Vec<i32> {
    let n = df.height();

    let mut hasher = DefaultHasher::new();
    hasher.write_u64(seed);
    let mut state = hasher.finish();

    let mut signals = vec![0i32; n];

    // Generate target_count random positions (after warmup)
    let available_positions: Vec<usize> = (200..n.saturating_sub(HOLD_BARS + 1)).collect();
    let mut positions = available_positions.clone();

    // Fisher-Yates shuffle to pick random positions
    for i in (1..positions.len()).rev() {
        state = state.wrapping_mul(6364136223846793005).wrapping_add(1);
        let j = (state % (i as u64 + 1)) as usize;
        positions.swap(i, j);
    }

    // Assign signals to first target_count positions
    for &pos in positions.iter().take(target_count) {
        state = state.wrapping_mul(6364136223846793005).wrapping_add(1);
        signals[pos] = if state % 2 == 0 { 1 } else { -1 };
    }

    signals
}

fn generate_turtle_signals(df: &DataFrame, period: usize) -> Result<Vec<i32>> {
    let close = df.column("close")?.f64()?;
    let high = df.column("high")?.f64()?;
    let low = df.column("low")?.f64()?;
    let n = close.len();

    let mut signals = vec![0i32; n];

    for i in period..n {
        let mut period_high = f64::NEG_INFINITY;
        let mut period_low = f64::INFINITY;

        for j in (i - period)..i {
            if let Some(h) = high.get(j) {
                period_high = period_high.max(h);
            }
            if let Some(l) = low.get(j) {
                period_low = period_low.min(l);
            }
        }

        let current_close = close.get(i).unwrap_or(0.0);

        if current_close > period_high {
            signals[i] = 1;
        } else if current_close < period_low {
            signals[i] = -1;
        }
    }

    Ok(signals)
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

fn generate_equity_curve_chart(curves: &[(String, Vec<f64>)], path: &str) -> Result<()> {
    std::fs::create_dir_all("charts")?;

    let root = BitMapBackend::new(path, (1200, 800)).into_drawing_area();
    root.fill(&WHITE)?;

    // Find max Y value for scaling
    let max_y = curves
        .iter()
        .flat_map(|(_, c)| c.iter())
        .cloned()
        .fold(100.0, f64::max)
        .min(5000.0);

    let mut chart = ChartBuilder::on(&root)
        .caption(
            "Turtle + Regime + MACD: Equity Curves",
            ("sans-serif", 24).into_font(),
        )
        .margin(10)
        .x_label_area_size(40)
        .y_label_area_size(60)
        .build_cartesian_2d(0..curves[0].1.len(), 0.0..max_y)?;

    chart.configure_mesh().draw()?;

    let colors = [
        RED,
        BLUE,
        GREEN,
        MAGENTA,
        CYAN,
        YELLOW,
        BLACK,
        RGBColor(128, 0, 128),
    ];

    for (i, (name, curve)) in curves.iter().enumerate() {
        let color = colors[i % colors.len()];
        let max_val = curve.iter().cloned().fold(f64::NEG_INFINITY, f64::max);

        chart
            .draw_series(LineSeries::new(
                curve.iter().enumerate().map(|(x, y)| (x, *y)),
                color.stroke_width(2),
            ))?
            .label(format!("{} (max: {:.0}%)", name, max_val))
            .legend(move |(x, y)| PathElement::new(vec![(x, y), (x + 20, y)], color));
    }

    chart
        .configure_series_labels()
        .background_style(WHITE.mix(0.8))
        .border_style(BLACK)
        .draw()?;

    root.present()?;
    Ok(())
}
