//! Equity/drawdown chart export from the audited trend-portfolio harness.
//!
//! Runs the same simulate_portfolio logic as trend_portfolio_stress_audit,
//! captures the full equity/drawdown series, exports to CSV, and renders
//! equity.png + drawdown.png via plotters.
//!
//! This is the REAL harness — not an ad-hoc script. Numbers match the
//! benchmark tables exactly.

use anyhow::Result;
use krypto::{data::loader::DataLoader, features::indicators::FeatureEngine};
use plotters::prelude::*;
use polars::prelude::*;
use std::{collections::HashMap, fs};

const CANDLES: u32 = 3000;
const HOLD_BARS: usize = 21;
const TAKER_FEE: f64 = 0.001;
const WARMUP_BARS: usize = 200;
const PERIOD: usize = 20;
const POSITION_CAP: usize = 3;
const CHART_DIR: &str = "charts";

const UNIVERSE: &[&str] = &[
    "BTCUSDT", "ETHUSDT", "SOLUSDT", "XRPUSDT", "DOGEUSDT", "ADAUSDT",
];

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum StrategyKind {
    Macd,
    MacdRegime,
    TurtleRegime,
    TurtleRegimeMacd,
}

impl StrategyKind {
    fn name(&self) -> &'static str {
        match self {
            Self::Macd => "MACD",
            Self::MacdRegime => "MACD+Regime",
            Self::TurtleRegime => "Turtle+Regime",
            Self::TurtleRegimeMacd => "Turtle+Regime+MACD",
        }
    }

    fn all() -> &'static [StrategyKind] {
        &[
            Self::Macd,
            Self::MacdRegime,
            Self::TurtleRegime,
            Self::TurtleRegimeMacd,
        ]
    }

    fn slug(&self) -> &'static str {
        match self {
            Self::Macd => "macd",
            Self::MacdRegime => "macd_regime",
            Self::TurtleRegime => "turtle_regime",
            Self::TurtleRegimeMacd => "turtle_regime_macd",
        }
    }
}

#[derive(Clone, Debug)]
struct TradeWindow {
    entry_idx: usize,
    exit_idx: usize,
    strength: f64,
    #[allow(dead_code)]
    gross_return: f64,
    net_return: f64,
}

#[derive(Clone, Debug)]
struct SymbolPlan {
    trades: Vec<TradeWindow>,
}

/// Extended portfolio result with full equity series
#[derive(Clone, Debug)]
struct PortfolioResult {
    equity_curve: Vec<f64>,
    drawdown_curve: Vec<f64>,
    active_positions: Vec<usize>,
    aligned_return_pct: f64,
    sharpe: f64,
    max_dd_pct: f64,
    trades: usize,
    win_rate_pct: f64,
}

struct UniverseData {
    data: Vec<(String, DataFrame)>,
    steps: usize,
}

#[tokio::main]
async fn main() -> Result<()> {
    println!("=== HARNESS CHART EXPORT ===\n");
    println!("Same logic as trend_portfolio_stress_audit, with chart output.");
    println!("Universe: {}", UNIVERSE.join(", "));
    println!(
        "Execution: signal at close, entry next open, exit after {} bars at open",
        HOLD_BARS
    );
    println!(
        "Fees: {:.1}% taker each side | capped book: top-{}\n",
        TAKER_FEE * 100.0,
        POSITION_CAP
    );

    let loader = DataLoader::new(None, None);
    let mut raw_cache = HashMap::<String, DataFrame>::new();

    for &symbol in UNIVERSE {
        print!("Loading {}... ", symbol);
        let raw = loader.fetch_with_cache(symbol, "1d", CANDLES).await?;
        let df = FeatureEngine::add_technicals(&raw, None)?;
        println!("{} bars", df.height());
        raw_cache.insert(symbol.to_string(), df);
    }

    let universe = aligned_universe(&raw_cache, UNIVERSE)?;
    println!("Aligned universe: {} steps\n", universe.steps);

    // Run all strategies, collect full equity series
    let mut results: Vec<(StrategyKind, PortfolioResult)> = Vec::new();

    for &strategy in StrategyKind::all() {
        let plans = build_symbol_plans(&universe, strategy)?;
        let result = simulate_portfolio_full(&plans, universe.steps, Some(POSITION_CAP));
        println!(
            "{:<22} Return: {:>12.1}% | Sharpe: {:>6.2} | MaxDD: {:>6.1}% | Trades: {:>4} | Win: {:>5.1}%",
            strategy.name(),
            result.aligned_return_pct,
            result.sharpe,
            result.max_dd_pct,
            result.trades,
            result.win_rate_pct,
        );
        results.push((strategy, result));
    }

    // Sort by Sharpe descending
    results.sort_by(|a, b| b.1.sharpe.partial_cmp(&a.1.sharpe).unwrap());

    fs::create_dir_all(CHART_DIR)?;

    // Export CSV for each strategy
    println!("\n--- CSV Export ---");
    for (strategy, result) in &results {
        let csv_path = format!("{}/{}_equity.csv", CHART_DIR, strategy.slug());
        export_csv(&csv_path, result)?;
        println!("Wrote {}", csv_path);
    }

    // Render combined equity chart
    let equity_path = format!("{}/equity.png", CHART_DIR);
    render_equity_chart(&equity_path, &results)?;
    println!("\nRendered {}", equity_path);

    // Render combined drawdown chart
    let dd_path = format!("{}/drawdown.png", CHART_DIR);
    render_drawdown_chart(&dd_path, &results)?;
    println!("Rendered {}", dd_path);

    // Also render individual strategy charts
    for (strategy, result) in &results {
        let path = format!("{}/{}_equity.png", CHART_DIR, strategy.slug());
        render_single_equity(&path, strategy.name(), result)?;
        println!("Rendered {}", path);
    }

    println!("\n=== DONE ===");
    Ok(())
}

fn export_csv(path: &str, result: &PortfolioResult) -> Result<()> {
    let mut out = String::from("day,equity,drawdown_pct,active_positions\n");
    for (i, ((eq, dd), pos)) in result
        .equity_curve
        .iter()
        .zip(result.drawdown_curve.iter())
        .zip(result.active_positions.iter())
        .enumerate()
    {
        out.push_str(&format!("{},{:.6},{:.4},{}\n", i, eq, dd, pos));
    }
    fs::write(path, out)?;
    Ok(())
}

fn aligned_universe(
    raw_cache: &HashMap<String, DataFrame>,
    symbols: &[&str],
) -> Result<UniverseData> {
    let min_len = symbols
        .iter()
        .filter_map(|symbol| raw_cache.get(*symbol).map(|df| df.height()))
        .min()
        .ok_or_else(|| anyhow::anyhow!("empty universe"))?;
    let steps = min_len.saturating_sub(1);
    if steps == 0 {
        anyhow::bail!("not enough data");
    }

    let mut data = Vec::new();
    for &symbol in symbols {
        let df = raw_cache
            .get(symbol)
            .ok_or_else(|| anyhow::anyhow!("missing symbol {symbol}"))?
            .slice(0, min_len);
        data.push((symbol.to_string(), df));
    }

    Ok(UniverseData { data, steps })
}

fn build_symbol_plans(universe: &UniverseData, strategy: StrategyKind) -> Result<Vec<SymbolPlan>> {
    universe
        .data
        .iter()
        .map(|(_, df)| build_symbol_plan(df, strategy))
        .collect()
}

fn build_symbol_plan(df: &DataFrame, strategy: StrategyKind) -> Result<SymbolPlan> {
    let signals = generate_signals(df, strategy)?;
    let strengths = generate_strengths(df, strategy)?;
    let open = df.column("open")?.f64()?;
    let n = open.len();
    let mut trades = Vec::new();
    let mut i = WARMUP_BARS;

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

        let gross_return = if signal > 0 {
            exit / entry - 1.0
        } else {
            entry / exit - 1.0
        };
        let net_return = gross_return - 2.0 * TAKER_FEE;
        trades.push(TradeWindow {
            entry_idx,
            exit_idx,
            strength: strengths.get(i).copied().unwrap_or(0.0).abs(),
            gross_return,
            net_return,
        });
        i = exit_idx;
    }

    Ok(SymbolPlan { trades })
}

/// Full portfolio simulation returning equity + drawdown curves.
/// Identical logic to trend_portfolio_stress_audit::simulate_portfolio.
fn simulate_portfolio_full(
    plans: &[SymbolPlan],
    steps: usize,
    cap: Option<usize>,
) -> PortfolioResult {
    let mut equity_curve = vec![1.0; steps + 1];
    let mut daily_returns = vec![0.0; steps];
    let mut active_counts = vec![0usize; steps + 1];
    let mut trades = 0usize;
    let mut wins = 0usize;

    for plan in plans {
        for trade in &plan.trades {
            trades += 1;
            if trade.net_return > 0.0 {
                wins += 1;
            }
        }
    }

    for day in 0..steps {
        let mut active = Vec::<(f64, f64)>::new();
        for plan in plans {
            for trade in &plan.trades {
                if day == trade.entry_idx {
                    active.push((trade.strength, -TAKER_FEE));
                }
                if day >= trade.entry_idx && day < trade.exit_idx {
                    let span = (trade.exit_idx - trade.entry_idx) as f64;
                    if span > 0.0 {
                        active.push((trade.strength, trade.gross_return / span));
                    }
                }
                if day == trade.exit_idx {
                    active.push((trade.strength, -TAKER_FEE));
                }
            }
        }

        active_counts[day] = active.len();
        if active.is_empty() {
            daily_returns[day] = 0.0;
            equity_curve[day + 1] = equity_curve[day];
            continue;
        }

        active.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap());
        let selected_len = cap.unwrap_or(active.len()).min(active.len());
        let selected = &active[..selected_len];
        let avg_ret = selected.iter().map(|(_, r)| *r).sum::<f64>() / selected.len() as f64;
        daily_returns[day] = avg_ret;
        equity_curve[day + 1] = equity_curve[day] * (1.0 + avg_ret);
    }
    active_counts[steps] = active_counts[steps.saturating_sub(1)];

    let aligned_return_pct = (equity_curve.last().copied().unwrap_or(1.0) - 1.0) * 100.0;
    let sharpe = calc_sharpe_from_returns(&daily_returns);
    let max_dd_pct = calc_max_drawdown_pct(&equity_curve);
    let win_rate_pct = if trades == 0 {
        0.0
    } else {
        wins as f64 / trades as f64 * 100.0
    };

    // Build drawdown curve
    let mut drawdown_curve = vec![0.0; equity_curve.len()];
    let mut peak = equity_curve[0];
    for i in 0..equity_curve.len() {
        if equity_curve[i] > peak {
            peak = equity_curve[i];
        }
        drawdown_curve[i] = (equity_curve[i] / peak - 1.0) * 100.0;
    }

    PortfolioResult {
        equity_curve,
        drawdown_curve,
        active_positions: active_counts,
        aligned_return_pct,
        sharpe,
        max_dd_pct,
        trades,
        win_rate_pct,
    }
}

// ─── Signal generation (copied verbatim from trend_portfolio_stress_audit) ───

fn generate_signals(df: &DataFrame, strategy: StrategyKind) -> Result<Vec<i32>> {
    match strategy {
        StrategyKind::Macd => generate_macd_signals(df),
        StrategyKind::MacdRegime => generate_macd_regime_signals(df),
        StrategyKind::TurtleRegime => generate_turtle_regime_signals(df, PERIOD),
        StrategyKind::TurtleRegimeMacd => generate_turtle_regime_macd_signals(df, PERIOD),
    }
}

fn generate_strengths(df: &DataFrame, strategy: StrategyKind) -> Result<Vec<f64>> {
    match strategy {
        StrategyKind::Macd | StrategyKind::MacdRegime => generate_macd_strengths(df),
        StrategyKind::TurtleRegime | StrategyKind::TurtleRegimeMacd => {
            generate_turtle_strengths(df, PERIOD)
        }
    }
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
    }
    Ok(signals)
}

fn generate_macd_strengths(df: &DataFrame) -> Result<Vec<f64>> {
    let macd = df.column("macd")?.f64()?;
    let signal = df.column("macd_signal")?.f64()?;
    let mut out = vec![0.0; macd.len()];
    for i in 0..macd.len() {
        out[i] = (macd.get(i).unwrap_or(0.0) - signal.get(i).unwrap_or(0.0)).abs();
    }
    Ok(out)
}

fn generate_macd_regime_signals(df: &DataFrame) -> Result<Vec<i32>> {
    let macd = generate_macd_signals(df)?;
    let close = df.column("close")?.f64()?;
    let sma_200 = calculate_sma(&close, 200);
    let mut out = vec![0i32; macd.len()];
    for i in 0..macd.len() {
        let sig = macd[i];
        let price = close.get(i).unwrap_or(0.0);
        let sma = sma_200.get(i).copied().unwrap_or(0.0);
        if sig > 0 && price > sma {
            out[i] = 1;
        } else if sig < 0 && price < sma {
            out[i] = -1;
        }
    }
    Ok(out)
}

fn generate_turtle_signals(df: &DataFrame, period: usize) -> Result<Vec<i32>> {
    let close = df.column("close")?.f64()?;
    let high = df.column("high")?.f64()?;
    let low = df.column("low")?.f64()?;
    let n = close.len();
    let mut signals = vec![0i32; n];
    for i in period..n {
        let period_high = (i - period..i)
            .filter_map(|j| high.get(j))
            .fold(f64::NEG_INFINITY, f64::max);
        let period_low = (i - period..i)
            .filter_map(|j| low.get(j))
            .fold(f64::INFINITY, f64::min);
        let current_close = close.get(i).unwrap_or(0.0);
        if current_close > period_high {
            signals[i] = 1;
        } else if current_close < period_low {
            signals[i] = -1;
        }
    }
    Ok(signals)
}

fn generate_turtle_strengths(df: &DataFrame, period: usize) -> Result<Vec<f64>> {
    let close = df.column("close")?.f64()?;
    let high = df.column("high")?.f64()?;
    let low = df.column("low")?.f64()?;
    let n = close.len();
    let mut out = vec![0.0; n];
    for i in period..n {
        let period_high = (i - period..i)
            .filter_map(|j| high.get(j))
            .fold(f64::NEG_INFINITY, f64::max);
        let period_low = (i - period..i)
            .filter_map(|j| low.get(j))
            .fold(f64::INFINITY, f64::min);
        let current_close = close.get(i).unwrap_or(0.0);
        let range = (period_high - period_low).abs().max(1e-9);
        if current_close > period_high {
            out[i] = (current_close - period_high) / range;
        } else if current_close < period_low {
            out[i] = (period_low - current_close) / range;
        }
    }
    Ok(out)
}

fn generate_turtle_regime_signals(df: &DataFrame, period: usize) -> Result<Vec<i32>> {
    let turtle = generate_turtle_signals(df, period)?;
    let close = df.column("close")?.f64()?;
    let sma_200 = calculate_sma(&close, 200);
    let mut out = vec![0i32; turtle.len()];
    for i in 0..turtle.len() {
        let sig = turtle[i];
        let price = close.get(i).unwrap_or(0.0);
        let sma = sma_200.get(i).copied().unwrap_or(0.0);
        if sig > 0 && price > sma {
            out[i] = 1;
        } else if sig < 0 {
            out[i] = -1;
        }
    }
    Ok(out)
}

fn generate_turtle_regime_macd_signals(df: &DataFrame, period: usize) -> Result<Vec<i32>> {
    let turtle = generate_turtle_regime_signals(df, period)?;
    let macd = generate_macd_signals(df)?;
    let mut out = vec![0i32; turtle.len()];
    for i in 0..turtle.len() {
        let t = turtle[i];
        let m = macd[i];
        if t > 0 && m > 0 {
            out[i] = 1;
        } else if t < 0 && m < 0 {
            out[i] = -1;
        }
    }
    Ok(out)
}

fn calculate_sma(values: &Float64Chunked, period: usize) -> Vec<f64> {
    let mut out = vec![0.0; values.len()];
    let mut sum = 0.0;
    for i in 0..values.len() {
        sum += values.get(i).unwrap_or(0.0);
        if i >= period {
            sum -= values.get(i - period).unwrap_or(0.0);
        }
        if i + 1 >= period {
            out[i] = sum / period as f64;
        }
    }
    out
}

fn calc_sharpe_from_returns(returns: &[f64]) -> f64 {
    if returns.is_empty() {
        return 0.0;
    }
    let mean = returns.iter().sum::<f64>() / returns.len() as f64;
    let var = returns
        .iter()
        .map(|r| {
            let d = r - mean;
            d * d
        })
        .sum::<f64>()
        / returns.len() as f64;
    let std = var.sqrt();
    if std <= 1e-12 {
        0.0
    } else {
        mean / std * 365.0f64.sqrt()
    }
}

fn calc_max_drawdown_pct(equity_curve: &[f64]) -> f64 {
    let mut peak = equity_curve.first().copied().unwrap_or(1.0);
    let mut max_dd = 0.0;
    for &value in equity_curve {
        if value > peak {
            peak = value;
        }
        let dd = (value / peak - 1.0) * 100.0;
        if dd < max_dd {
            max_dd = dd;
        }
    }
    max_dd.abs()
}

// ─── Chart rendering ───

const COLORS: &[RGBColor] = &[
    RGBColor(31, 119, 180), // blue
    RGBColor(255, 127, 14), // orange
    RGBColor(44, 160, 44),  // green
    RGBColor(214, 39, 40),  // red
];

fn render_equity_chart(path: &str, results: &[(StrategyKind, PortfolioResult)]) -> Result<()> {
    let root = BitMapBackend::new(path, (1600, 900)).into_drawing_area();
    root.fill(&WHITE)?;

    // Use log scale for equity (values span orders of magnitude)
    let max_eq = results
        .iter()
        .flat_map(|(_, r)| r.equity_curve.iter())
        .cloned()
        .fold(1.0f64, f64::max);
    let max_days = results
        .iter()
        .map(|(_, r)| r.equity_curve.len())
        .max()
        .unwrap_or(1);

    // Convert to log10 for plotting
    let log_min = 0.0f64; // log10(1.0) = 0
    let log_max = max_eq.log10() * 1.05;

    let mut chart = ChartBuilder::on(&root)
        .caption(
            format!(
                "Portfolio Equity (log scale) — Base6 Capped Top-{} | {} bars | {:.1}% fee",
                POSITION_CAP,
                CANDLES,
                TAKER_FEE * 100.0
            ),
            ("sans-serif", 28),
        )
        .margin(20)
        .x_label_area_size(50)
        .y_label_area_size(80)
        .build_cartesian_2d(0..max_days, log_min..log_max)?;

    chart
        .configure_mesh()
        .x_desc("Day")
        .y_desc("log₁₀(Equity)")
        .y_label_formatter(&|y| {
            let val = 10.0f64.powf(*y);
            if val >= 1000.0 {
                format!("{:.0}x", val)
            } else {
                format!("{:.1}x", val)
            }
        })
        .draw()?;

    for (i, (strategy, result)) in results.iter().enumerate() {
        let color = COLORS[i % COLORS.len()];
        let series: Vec<(usize, f64)> = result
            .equity_curve
            .iter()
            .enumerate()
            .map(|(j, v)| (j, v.max(1e-10).log10()))
            .collect();

        chart
            .draw_series(LineSeries::new(series, color.stroke_width(2)))?
            .label(format!(
                "{} (Ret:{:.0}% Shp:{:.2} DD:{:.1}%)",
                strategy.name(),
                result.aligned_return_pct,
                result.sharpe,
                result.max_dd_pct,
            ))
            .legend(move |(x, y)| {
                PathElement::new(vec![(x, y), (x + 20, y)], color.stroke_width(2))
            });
    }

    chart
        .configure_series_labels()
        .background_style(WHITE.mix(0.8))
        .border_style(BLACK)
        .position(SeriesLabelPosition::UpperLeft)
        .draw()?;

    root.present()?;
    Ok(())
}

fn render_drawdown_chart(path: &str, results: &[(StrategyKind, PortfolioResult)]) -> Result<()> {
    let root = BitMapBackend::new(path, (1600, 600)).into_drawing_area();
    root.fill(&WHITE)?;

    let min_dd = results
        .iter()
        .flat_map(|(_, r)| r.drawdown_curve.iter())
        .cloned()
        .fold(0.0f64, f64::min);
    let max_days = results
        .iter()
        .map(|(_, r)| r.drawdown_curve.len())
        .max()
        .unwrap_or(1);

    let mut chart = ChartBuilder::on(&root)
        .caption(
            format!(
                "Portfolio Drawdown — Base6 Capped Top-{} | {} bars | {:.1}% fee",
                POSITION_CAP,
                CANDLES,
                TAKER_FEE * 100.0
            ),
            ("sans-serif", 24),
        )
        .margin(20)
        .x_label_area_size(50)
        .y_label_area_size(80)
        .build_cartesian_2d(0..max_days, (min_dd * 1.1)..5.0)?;

    chart
        .configure_mesh()
        .x_desc("Day")
        .y_desc("Drawdown %")
        .y_label_formatter(&|y| format!("{:.0}%", y))
        .draw()?;

    for (i, (strategy, result)) in results.iter().enumerate() {
        let color = COLORS[i % COLORS.len()];
        let series: Vec<(usize, f64)> = result
            .drawdown_curve
            .iter()
            .enumerate()
            .map(|(j, v)| (j, *v))
            .collect();

        chart
            .draw_series(LineSeries::new(series, color.stroke_width(2)))?
            .label(format!(
                "{} (MaxDD:{:.1}%)",
                strategy.name(),
                result.max_dd_pct
            ))
            .legend(move |(x, y)| {
                PathElement::new(vec![(x, y), (x + 20, y)], color.stroke_width(2))
            });
    }

    chart
        .configure_series_labels()
        .background_style(WHITE.mix(0.8))
        .border_style(BLACK)
        .position(SeriesLabelPosition::LowerLeft)
        .draw()?;

    root.present()?;
    Ok(())
}

fn render_single_equity(path: &str, name: &str, result: &PortfolioResult) -> Result<()> {
    let root = BitMapBackend::new(path, (1200, 800)).into_drawing_area();
    root.fill(&WHITE)?;

    let areas = root.split_vertically(560);
    let (upper, lower) = (areas.0.clone(), areas.1.clone());

    // Equity (log)
    let max_eq = result.equity_curve.iter().cloned().fold(1.0f64, f64::max);
    let log_max = max_eq.log10() * 1.05;
    let n = result.equity_curve.len();

    let mut chart = ChartBuilder::on(&upper)
        .caption(
            format!(
                "{} — Ret:{:.0}% Shp:{:.2} DD:{:.1}%",
                name, result.aligned_return_pct, result.sharpe, result.max_dd_pct
            ),
            ("sans-serif", 24),
        )
        .margin(15)
        .x_label_area_size(40)
        .y_label_area_size(70)
        .build_cartesian_2d(0..n, 0.0..log_max)?;

    chart
        .configure_mesh()
        .y_desc("log₁₀(Equity)")
        .y_label_formatter(&|y| {
            let val = 10.0f64.powf(*y);
            if val >= 1000.0 {
                format!("{:.0}x", val)
            } else {
                format!("{:.1}x", val)
            }
        })
        .draw()?;

    chart.draw_series(LineSeries::new(
        result
            .equity_curve
            .iter()
            .enumerate()
            .map(|(i, v)| (i, v.max(1e-10).log10())),
        COLORS[0].stroke_width(2),
    ))?;

    // Drawdown
    let min_dd = result.drawdown_curve.iter().cloned().fold(0.0f64, f64::min);

    let mut dd_chart = ChartBuilder::on(&lower)
        .caption("Drawdown", ("sans-serif", 18))
        .margin(15)
        .x_label_area_size(40)
        .y_label_area_size(70)
        .build_cartesian_2d(0..n, (min_dd * 1.1)..2.0)?;

    dd_chart
        .configure_mesh()
        .x_desc("Day")
        .y_desc("DD %")
        .y_label_formatter(&|y| format!("{:.0}%", y))
        .draw()?;

    dd_chart.draw_series(LineSeries::new(
        result
            .drawdown_curve
            .iter()
            .enumerate()
            .map(|(i, v)| (i, *v)),
        RGBColor(214, 39, 40).stroke_width(1),
    ))?;

    root.present()?;
    Ok(())
}
