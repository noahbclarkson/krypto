//! Generate real capped-book equity and drawdown charts for the current benchmark leaders.
//!
//! Purpose:
//! - answer Noah's request for actual performance-path visuals, not meta charts
//! - show how the benchmark leaders behave under the realistic top-3 capped-book lens
//! - compare a broader modern basket vs a harsher old-guard basket
//!
//! Execution assumptions match the fair daily harness:
//! - signal at close using only current/past data
//! - entry at next open
//! - exit at open after fixed 21-bar hold
//! - 0.1% taker fee on entry and exit
//! - top-3 strength-capped portfolio book

use anyhow::Result;
use krypto::{data::loader::DataLoader, features::indicators::FeatureEngine};
use plotters::prelude::*;
use polars::prelude::*;
use std::collections::HashMap;

const CANDLES: u32 = 3000;
const HOLD_BARS: usize = 21;
const TAKER_FEE: f64 = 0.001;
const WARMUP_BARS: usize = 200;
const PERIOD: usize = 20;
const POSITION_CAP: usize = 3;

const BASE6: &[&str] = &[
    "BTCUSDT", "ETHUSDT", "SOLUSDT", "XRPUSDT", "DOGEUSDT", "ADAUSDT",
];
const OLD_GUARD_NO_BNB: &[&str] = &[
    "BTCUSDT", "ETHUSDT", "XRPUSDT", "LTCUSDT", "EOSUSDT", "BCHUSDT",
];

const EQUITY_PATH: &str = "charts/capped_book_equity_audit.png";
const DRAWDOWN_PATH: &str = "charts/capped_book_drawdown_audit.png";

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum StrategyKind {
    MacdRegime,
    TurtleRegimeMacd,
}

impl StrategyKind {
    fn name(&self) -> &'static str {
        match self {
            Self::MacdRegime => "MACD+Regime",
            Self::TurtleRegimeMacd => "Turtle+Regime+MACD",
        }
    }

    fn all() -> &'static [StrategyKind] {
        &[Self::MacdRegime, Self::TurtleRegimeMacd]
    }
}

#[derive(Clone, Debug)]
struct TradeWindow {
    entry_idx: usize,
    exit_idx: usize,
    strength: f64,
    gross_return: f64,
    net_return: f64,
}

#[derive(Clone, Debug)]
struct SymbolPlan {
    trades: Vec<TradeWindow>,
}

#[derive(Clone, Debug)]
struct PortfolioPath {
    equity_curve: Vec<f64>,
    drawdown_curve: Vec<f64>,
    total_return_pct: f64,
    max_dd_pct: f64,
}

struct UniverseData {
    data: Vec<(String, DataFrame)>,
    steps: usize,
}

fn main() -> Result<()> {
    println!("=== CAPPED-BOOK VISUAL AUDIT ===\n");
    println!("Goal: generate real equity + drawdown charts for the top benchmark leaders");
    println!(
        "Lens: top-{} capped book, next-open entry, {}-bar hold, {:.1}% taker/side\n",
        POSITION_CAP,
        HOLD_BARS,
        TAKER_FEE * 100.0
    );

    let loader = DataLoader::new(None, None);
    let needed = [
        "BTCUSDT", "ETHUSDT", "SOLUSDT", "XRPUSDT", "DOGEUSDT", "ADAUSDT", "LTCUSDT", "EOSUSDT",
        "BCHUSDT",
    ];

    let mut raw_cache = HashMap::<String, DataFrame>::new();
    for symbol in needed {
        print!("Loading {}... ", symbol);
        let raw = tokio::runtime::Runtime::new()?
            .block_on(loader.fetch_with_cache(symbol, "1d", CANDLES))?;
        let df = FeatureEngine::add_technicals(&raw, None)?;
        println!("{} bars", df.height());
        raw_cache.insert(symbol.to_string(), df);
    }

    let cases = vec![("Base6", BASE6), ("OldGuardNoBNB", OLD_GUARD_NO_BNB)];

    let mut rendered = Vec::<(String, Vec<(String, PortfolioPath)>)>::new();
    for (label, symbols) in cases {
        println!("\n=== {} ===", label);
        let universe = aligned_universe(&raw_cache, symbols)?;
        let mut paths = Vec::new();
        for &strategy in StrategyKind::all() {
            let plans = build_symbol_plans(&universe, strategy)?;
            let path = simulate_portfolio_path(&plans, universe.steps, POSITION_CAP);
            println!(
                "- {:<20} Return {:>10.1}% | MaxDD {:>6.1}%",
                strategy.name(),
                path.total_return_pct,
                path.max_dd_pct,
            );
            paths.push((strategy.name().to_string(), path));
        }
        rendered.push((label.to_string(), paths));
    }

    draw_equity_chart(&rendered, EQUITY_PATH)?;
    draw_drawdown_chart(&rendered, DRAWDOWN_PATH)?;

    println!("\nWrote:");
    println!("- {}", EQUITY_PATH);
    println!("- {}", DRAWDOWN_PATH);

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

fn simulate_portfolio_path(plans: &[SymbolPlan], steps: usize, cap: usize) -> PortfolioPath {
    let mut equity_curve = vec![1.0; steps + 1];

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

        if active.is_empty() {
            equity_curve[day + 1] = equity_curve[day];
            continue;
        }

        active.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap());
        let selected = &active[..cap.min(active.len())];
        let avg_ret = selected.iter().map(|(_, r)| *r).sum::<f64>() / selected.len() as f64;
        equity_curve[day + 1] = equity_curve[day] * (1.0 + avg_ret);
    }

    let mut peak = equity_curve[0];
    let mut drawdown_curve = Vec::with_capacity(equity_curve.len());
    let mut max_dd: f64 = 0.0;
    for &eq in &equity_curve {
        peak = peak.max(eq);
        let dd = if peak > 0.0 { 1.0 - eq / peak } else { 0.0 };
        max_dd = max_dd.max(dd);
        drawdown_curve.push(dd * 100.0);
    }

    let total_return_pct = (equity_curve.last().copied().unwrap_or(1.0) - 1.0) * 100.0;

    PortfolioPath {
        equity_curve,
        drawdown_curve,
        total_return_pct,
        max_dd_pct: max_dd * 100.0,
    }
}

fn generate_signals(df: &DataFrame, strategy: StrategyKind) -> Result<Vec<i32>> {
    match strategy {
        StrategyKind::MacdRegime => generate_macd_regime_signals(df),
        StrategyKind::TurtleRegimeMacd => generate_turtle_regime_macd_signals(df, PERIOD),
    }
}

fn generate_strengths(df: &DataFrame, strategy: StrategyKind) -> Result<Vec<f64>> {
    match strategy {
        StrategyKind::MacdRegime => generate_macd_strengths(df),
        StrategyKind::TurtleRegimeMacd => generate_turtle_strengths(df, PERIOD),
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
        if turtle[i] > 0 && macd[i] > 0 {
            out[i] = 1;
        } else if turtle[i] < 0 && macd[i] < 0 {
            out[i] = -1;
        }
    }
    Ok(out)
}

fn calculate_sma(series: &Float64Chunked, period: usize) -> Vec<f64> {
    let n = series.len();
    let mut out = vec![0.0; n];
    let mut sum = 0.0;
    for i in 0..n {
        sum += series.get(i).unwrap_or(0.0);
        if i >= period {
            sum -= series.get(i - period).unwrap_or(0.0);
        }
        if i + 1 >= period {
            out[i] = sum / period as f64;
        }
    }
    out
}

fn draw_equity_chart(
    rendered: &[(String, Vec<(String, PortfolioPath)>)],
    path: &str,
) -> Result<()> {
    let root = BitMapBackend::new(path, (1600, 900)).into_drawing_area();
    root.fill(&WHITE)?;
    let areas = root.split_evenly((1, 2));
    let colors = [RED, BLUE];

    for ((label, paths), area) in rendered.iter().zip(areas.into_iter()) {
        let max_len = paths
            .iter()
            .map(|(_, p)| p.equity_curve.len())
            .max()
            .unwrap_or(0);
        let max_y = paths
            .iter()
            .flat_map(|(_, p)| p.equity_curve.iter().copied())
            .fold(1.0_f64, f64::max)
            .max(1.05);

        let mut chart = ChartBuilder::on(&area)
            .margin(24)
            .caption(
                format!("{} — capped-book equity (log scale)", label),
                ("sans-serif", 30).into_font(),
            )
            .x_label_area_size(40)
            .y_label_area_size(88)
            .build_cartesian_2d(
                0usize..max_len.saturating_sub(1).max(1),
                (1.0f64..max_y).log_scale(),
            )?;

        chart
            .configure_mesh()
            .x_desc("bar index")
            .y_desc("equity (log scale, start = 1.0)")
            .light_line_style(TRANSPARENT)
            .bold_line_style(BLACK.mix(0.15))
            .axis_desc_style(("sans-serif", 18))
            .label_style(("sans-serif", 15))
            .y_label_formatter(&|v| {
                if *v >= 1_000_000.0 {
                    format!("{:.0}M", v / 1_000_000.0)
                } else if *v >= 1_000.0 {
                    format!("{:.0}k", v / 1_000.0)
                } else if *v >= 10.0 {
                    format!("{:.0}", v)
                } else {
                    format!("{:.1}", v)
                }
            })
            .draw()?;

        for (idx, (name, portfolio)) in paths.iter().enumerate() {
            let color = colors[idx % colors.len()];
            chart
                .draw_series(LineSeries::new(
                    portfolio
                        .equity_curve
                        .iter()
                        .enumerate()
                        .map(|(i, v)| (i, *v)),
                    &color,
                ))?
                .label(format!(
                    "{} ({:+.1}% / MaxDD {:.1}%)",
                    name, portfolio.total_return_pct, portfolio.max_dd_pct
                ))
                .legend(move |(x, y)| PathElement::new(vec![(x, y), (x + 20, y)], color));
        }

        chart
            .configure_series_labels()
            .background_style(WHITE.mix(0.88))
            .border_style(BLACK.mix(0.3))
            .label_font(("sans-serif", 16))
            .position(SeriesLabelPosition::UpperLeft)
            .draw()?;
    }

    root.present()?;
    Ok(())
}

fn draw_drawdown_chart(
    rendered: &[(String, Vec<(String, PortfolioPath)>)],
    path: &str,
) -> Result<()> {
    let root = BitMapBackend::new(path, (1600, 900)).into_drawing_area();
    root.fill(&WHITE)?;
    let areas = root.split_evenly((1, 2));
    let colors = [RED, BLUE];

    for ((label, paths), area) in rendered.iter().zip(areas.into_iter()) {
        let max_len = paths
            .iter()
            .map(|(_, p)| p.drawdown_curve.len())
            .max()
            .unwrap_or(0);
        let max_y = paths
            .iter()
            .flat_map(|(_, p)| p.drawdown_curve.iter().copied())
            .fold(5.0_f64, f64::max)
            .max(10.0);

        let mut chart = ChartBuilder::on(&area)
            .margin(24)
            .caption(
                format!("{} — capped-book drawdown", label),
                ("sans-serif", 30).into_font(),
            )
            .x_label_area_size(40)
            .y_label_area_size(80)
            .build_cartesian_2d(0usize..max_len.saturating_sub(1).max(1), 0f64..max_y)?;

        chart
            .configure_mesh()
            .x_desc("bar index")
            .y_desc("drawdown %")
            .light_line_style(TRANSPARENT)
            .bold_line_style(BLACK.mix(0.15))
            .axis_desc_style(("sans-serif", 18))
            .label_style(("sans-serif", 15))
            .draw()?;

        for (idx, (name, portfolio)) in paths.iter().enumerate() {
            let color = colors[idx % colors.len()];
            chart
                .draw_series(LineSeries::new(
                    portfolio
                        .drawdown_curve
                        .iter()
                        .enumerate()
                        .map(|(i, v)| (i, *v)),
                    &color,
                ))?
                .label(format!("{} (MaxDD {:.1}%)", name, portfolio.max_dd_pct))
                .legend(move |(x, y)| PathElement::new(vec![(x, y), (x + 20, y)], color));
        }

        chart
            .configure_series_labels()
            .background_style(WHITE.mix(0.88))
            .border_style(BLACK.mix(0.3))
            .label_font(("sans-serif", 16))
            .position(SeriesLabelPosition::UpperLeft)
            .draw()?;
    }

    root.present()?;
    Ok(())
}
