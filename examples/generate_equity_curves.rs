//! Generate equity curve chart for validated strategy

use anyhow::Result;
use krypto::{data::loader::DataLoader, features::indicators::FeatureEngine};
use plotters::prelude::*;
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
    println!("=== EQUITY CURVE GENERATION ===\n");

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

    let mut all_curves: Vec<(String, Vec<f64>)> = Vec::new();
    let mut portfolio_curve: Vec<f64> = vec![100.0];
    let mut max_len = 0;

    for symbol in SYMBOLS {
        if let Some(df) = data_cache.get(&symbol.to_string()) {
            let (result, curve) = run_backtest_with_equity(df)?;
            println!(
                "{:12} | Return: {:>8.1}% | Trades: {:>3} | WinRate: {:>5.1}%",
                symbol,
                result.total_return_pct,
                result.trades,
                if result.trades > 0 {
                    result.wins as f64 / result.trades as f64 * 100.0
                } else {
                    0.0
                }
            );
            all_curves.push((symbol.to_string(), curve.clone()));
            max_len = max_len.max(curve.len());
        }
    }

    for i in 1..max_len {
        let mut sum = 0.0;
        let mut count = 0;
        for (_, curve) in &all_curves {
            if i < curve.len() {
                sum += curve[i] / curve[0];
                count += 1;
            }
        }
        if count > 0 {
            let avg = sum / count as f64;
            portfolio_curve.push(100.0 * avg);
        }
    }

    let chart_path = "charts/equity_curves.png";
    if let Err(e) = generate_chart(&all_curves, &portfolio_curve, chart_path) {
        println!("Chart generation failed: {}", e);
    } else {
        println!("\nChart saved to: {}", chart_path);
    }

    let dd_path = "charts/drawdown_chart.png";
    if let Err(e) = generate_drawdown_chart(&portfolio_curve, dd_path) {
        println!("Drawdown chart failed: {}", e);
    } else {
        println!("Drawdown chart saved to: {}", dd_path);
    }

    Ok(())
}

struct BacktestResult {
    total_return_pct: f64,
    trades: usize,
    wins: usize,
}

fn run_backtest_with_equity(df: &DataFrame) -> Result<(BacktestResult, Vec<f64>)> {
    let close = df.column("close")?.f64()?;
    let n = close.len();

    let sma_200 = calculate_sma(&close, 200)?;
    let macd_signals = generate_macd_signals(df)?;
    let turtle_signals = generate_turtle_signals(df, PERIOD)?;

    let mut trade_returns: Vec<f64> = Vec::new();
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

                trade_returns.push(net_return);
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

    let trades = trade_returns.len();
    let total_return_pct = trade_returns.iter().sum();
    let wins = trade_returns.iter().filter(|&&r| r > 0.0).count();

    Ok((
        BacktestResult {
            total_return_pct,
            trades,
            wins,
        },
        equity_curve,
    ))
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

fn generate_chart(
    curves: &[(String, Vec<f64>)],
    portfolio: &[f64],
    path: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    std::fs::create_dir_all("charts")?;

    let max_len = curves.iter().map(|(_, c)| c.len()).max().unwrap_or(0);
    let max_val = curves
        .iter()
        .flat_map(|(_, c)| c.iter())
        .cloned()
        .fold(0.0, f64::max)
        .max(portfolio.iter().cloned().fold(0.0, f64::max));

    let root = BitMapBackend::new(path, (1200, 800)).into_drawing_area();
    root.fill(&WHITE)?;

    let mut chart = ChartBuilder::on(&root)
        .caption("Equity Curves: Turtle + Regime + MACD", ("sans-serif", 24))
        .margin(10)
        .x_label_area_size(40)
        .y_label_area_size(60)
        .build_cartesian_2d(0..max_len, 0.0..max_val)?;

    chart
        .configure_mesh()
        .x_desc("Trading Days")
        .y_desc("Equity (%)")
        .draw()?;

    let colors = vec![RED, BLUE, GREEN, MAGENTA, CYAN, YELLOW];

    for (i, (name, curve)) in curves.iter().enumerate() {
        let color = colors[i % colors.len()];
        let points: Vec<(usize, f64)> = curve.iter().enumerate().map(|(i, &v)| (i, v)).collect();
        chart
            .draw_series(LineSeries::new(points, color.stroke_width(2)))?
            .label(name.clone())
            .legend(move |(x, y)| {
                PathElement::new(vec![(x, y), (x + 20, y)], color.stroke_width(2))
            });
    }

    let portfolio_points: Vec<(usize, f64)> =
        portfolio.iter().enumerate().map(|(i, &v)| (i, v)).collect();
    chart
        .draw_series(LineSeries::new(portfolio_points, BLACK.stroke_width(3)))?
        .label("Portfolio (avg)".to_string())
        .legend(move |(x, y)| PathElement::new(vec![(x, y), (x + 20, y)], BLACK.stroke_width(3)));

    chart
        .configure_series_labels()
        .background_style(WHITE.mix(0.8))
        .border_style(BLACK)
        .draw()?;

    root.present()?;
    Ok(())
}

fn generate_drawdown_chart(
    portfolio: &[f64],
    path: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    std::fs::create_dir_all("charts")?;

    let mut peak = portfolio[0];
    let mut drawdown: Vec<f64> = Vec::new();

    for &equity in portfolio {
        peak = peak.max(equity);
        let dd = (peak - equity) / peak * 100.0;
        drawdown.push(dd);
    }

    let max_dd = drawdown.iter().cloned().fold(0.0, f64::max);
    let n = drawdown.len();

    let root = BitMapBackend::new(path, (1200, 400)).into_drawing_area();
    root.fill(&WHITE)?;

    let mut chart = ChartBuilder::on(&root)
        .caption("Portfolio Drawdown", ("sans-serif", 20))
        .margin(10)
        .x_label_area_size(40)
        .y_label_area_size(60)
        .build_cartesian_2d(0..n, 0.0..max_dd)?;

    chart
        .configure_mesh()
        .x_desc("Trading Days")
        .y_desc("Drawdown (%)")
        .draw()?;

    let points: Vec<(usize, f64)> = drawdown.iter().enumerate().map(|(i, &v)| (i, v)).collect();
    chart
        .draw_series(LineSeries::new(points, RED.stroke_width(2)))?
        .label("Drawdown")
        .legend(move |(x, y)| PathElement::new(vec![(x, y), (x + 20, y)], RED.stroke_width(2)));

    chart
        .configure_series_labels()
        .background_style(WHITE.mix(0.8))
        .border_style(BLACK)
        .draw()?;

    root.present()?;
    Ok(())
}
