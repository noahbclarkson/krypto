//! Decompose worst capped-book drawdowns for the current benchmark leaders.
//!
//! Purpose:
//! - extend the visual audit with actual drawdown attribution, not just endpoint/path charts
//! - stress the current leaders as yardsticks under the realistic top-3 capped-book lens
//! - show which symbols are doing the damage inside the worst peak-to-trough window

use anyhow::Result;
use krypto::{data::loader::DataLoader, features::indicators::FeatureEngine};
use plotters::prelude::*;
use polars::prelude::*;
use std::collections::HashMap;
use std::fs;

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

const CHART_PATH: &str = "charts/capped_book_drawdown_attribution.png";
const SNAPSHOT_LATEST_MD: &str = "snapshots/capped_book_drawdown_attribution_latest.md";
const SNAPSHOT_LATEST_CSV: &str = "snapshots/capped_book_drawdown_attribution_latest.csv";

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
    symbol: String,
    entry_idx: usize,
    exit_idx: usize,
    strength: f64,
    gross_return: f64,
}

#[derive(Clone, Debug)]
struct SymbolPlan {
    trades: Vec<TradeWindow>,
}

#[derive(Clone, Debug)]
struct DailyContribution {
    symbol: String,
    strength: f64,
    ret: f64,
}

#[derive(Clone, Debug)]
struct PortfolioTrace {
    equity_curve: Vec<f64>,
    drawdown_curve: Vec<f64>,
    daily_selected: Vec<Vec<DailyContribution>>,
}

#[derive(Clone, Debug)]
struct ContributionRow {
    symbol: String,
    contribution_pct: f64,
}

#[derive(Clone, Debug)]
struct DrawdownSummary {
    universe: String,
    strategy: String,
    peak_idx: usize,
    trough_idx: usize,
    trough_dd_pct: f64,
    window_return_pct: f64,
    contributions: Vec<ContributionRow>,
}

struct UniverseData {
    data: Vec<(String, DataFrame)>,
    steps: usize,
}

fn main() -> Result<()> {
    println!("=== CAPPED-BOOK DRAWDOWN ATTRIBUTION ===\n");
    println!(
        "Goal: identify which symbols drive the worst capped-book drawdown for the benchmark leaders"
    );
    println!(
        "Lens: top-{} capped book, next-open entry, {}-bar hold, {:.1}% taker/side\n",
        POSITION_CAP,
        HOLD_BARS,
        TAKER_FEE * 100.0
    );

    fs::create_dir_all("charts")?;
    fs::create_dir_all("snapshots")?;

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
    let mut summaries = Vec::new();

    for (label, symbols) in cases {
        println!("\n=== {} ===", label);
        let universe = aligned_universe(&raw_cache, symbols)?;
        for &strategy in StrategyKind::all() {
            let plans = build_symbol_plans(&universe, strategy)?;
            let trace = simulate_portfolio_trace(&plans, universe.steps, POSITION_CAP);
            let summary = summarize_drawdown(label, strategy.name(), &trace);
            println!(
                "- {:<20} worst DD {:>6.1}% | peak {} -> trough {} | window return {:+.1}%",
                strategy.name(),
                summary.trough_dd_pct,
                summary.peak_idx,
                summary.trough_idx,
                summary.window_return_pct,
            );
            for row in summary.contributions.iter().take(4) {
                println!("    {:<10} {:+7.1}%", row.symbol, row.contribution_pct);
            }
            summaries.push(summary);
        }
    }

    draw_attribution_chart(&summaries, CHART_PATH)?;
    write_markdown(&summaries, SNAPSHOT_LATEST_MD)?;
    write_csv(&summaries, SNAPSHOT_LATEST_CSV)?;

    println!("\nWrote:");
    println!("- {}", CHART_PATH);
    println!("- {}", SNAPSHOT_LATEST_MD);
    println!("- {}", SNAPSHOT_LATEST_CSV);

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
        .map(|(symbol, df)| build_symbol_plan(symbol, df, strategy))
        .collect()
}

fn build_symbol_plan(symbol: &str, df: &DataFrame, strategy: StrategyKind) -> Result<SymbolPlan> {
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
        trades.push(TradeWindow {
            symbol: symbol.to_string(),
            entry_idx,
            exit_idx,
            strength: strengths.get(i).copied().unwrap_or(0.0).abs(),
            gross_return,
        });
        i = exit_idx;
    }

    Ok(SymbolPlan { trades })
}

fn simulate_portfolio_trace(plans: &[SymbolPlan], steps: usize, cap: usize) -> PortfolioTrace {
    let mut equity_curve = vec![1.0; steps + 1];
    let mut daily_selected = vec![Vec::<DailyContribution>::new(); steps];

    for day in 0..steps {
        let mut active = Vec::<DailyContribution>::new();
        for plan in plans {
            for trade in &plan.trades {
                if day == trade.entry_idx {
                    active.push(DailyContribution {
                        symbol: trade.symbol.clone(),
                        strength: trade.strength,
                        ret: -TAKER_FEE,
                    });
                }
                if day >= trade.entry_idx && day < trade.exit_idx {
                    let span = (trade.exit_idx - trade.entry_idx) as f64;
                    if span > 0.0 {
                        active.push(DailyContribution {
                            symbol: trade.symbol.clone(),
                            strength: trade.strength,
                            ret: trade.gross_return / span,
                        });
                    }
                }
                if day == trade.exit_idx {
                    active.push(DailyContribution {
                        symbol: trade.symbol.clone(),
                        strength: trade.strength,
                        ret: -TAKER_FEE,
                    });
                }
            }
        }

        if active.is_empty() {
            equity_curve[day + 1] = equity_curve[day];
            continue;
        }

        active.sort_by(|a, b| b.strength.partial_cmp(&a.strength).unwrap());
        let selected = active.into_iter().take(cap).collect::<Vec<_>>();
        let avg_ret = selected.iter().map(|item| item.ret).sum::<f64>() / selected.len() as f64;
        equity_curve[day + 1] = equity_curve[day] * (1.0 + avg_ret);
        daily_selected[day] = selected;
    }

    let mut peak = equity_curve[0];
    let mut drawdown_curve = Vec::with_capacity(equity_curve.len());
    for &eq in &equity_curve {
        peak = peak.max(eq);
        let dd = if peak > 0.0 { 1.0 - eq / peak } else { 0.0 };
        drawdown_curve.push(dd * 100.0);
    }

    PortfolioTrace {
        equity_curve,
        drawdown_curve,
        daily_selected,
    }
}

fn summarize_drawdown(universe: &str, strategy: &str, trace: &PortfolioTrace) -> DrawdownSummary {
    let mut running_peak_idx = 0usize;
    let mut running_peak_value = trace.equity_curve[0];
    let mut worst_peak_idx = 0usize;
    let mut trough_idx = 0usize;
    let mut worst_dd = 0.0f64;

    for (idx, &eq) in trace.equity_curve.iter().enumerate() {
        if eq > running_peak_value {
            running_peak_value = eq;
            running_peak_idx = idx;
        }
        let dd = if running_peak_value > 0.0 {
            1.0 - eq / running_peak_value
        } else {
            0.0
        };
        if dd > worst_dd {
            worst_dd = dd;
            worst_peak_idx = running_peak_idx;
            trough_idx = idx;
        }
    }

    let peak_idx = worst_peak_idx;
    let start_day = peak_idx.min(trace.daily_selected.len());
    let end_day = trough_idx.min(trace.daily_selected.len());
    let mut contributions = HashMap::<String, f64>::new();
    for day in start_day..end_day {
        for item in &trace.daily_selected[day] {
            *contributions.entry(item.symbol.clone()).or_insert(0.0) +=
                item.ret * 100.0 / POSITION_CAP as f64;
        }
    }

    let mut rows = contributions
        .into_iter()
        .map(|(symbol, contribution_pct)| ContributionRow {
            symbol,
            contribution_pct,
        })
        .collect::<Vec<_>>();
    rows.sort_by(|a, b| a.contribution_pct.partial_cmp(&b.contribution_pct).unwrap());

    let peak_equity = trace.equity_curve.get(peak_idx).copied().unwrap_or(1.0);
    let trough_equity = trace
        .equity_curve
        .get(trough_idx)
        .copied()
        .unwrap_or(peak_equity);
    let window_return_pct = if peak_equity > 0.0 {
        (trough_equity / peak_equity - 1.0) * 100.0
    } else {
        0.0
    };

    DrawdownSummary {
        universe: universe.to_string(),
        strategy: strategy.to_string(),
        peak_idx,
        trough_idx,
        trough_dd_pct: trace
            .drawdown_curve
            .get(trough_idx)
            .copied()
            .unwrap_or(worst_dd * 100.0),
        window_return_pct,
        contributions: rows,
    }
}

fn write_markdown(summaries: &[DrawdownSummary], path: &str) -> Result<()> {
    let mut out = String::new();
    out.push_str("# Capped-book Drawdown Attribution\n\n");
    out.push_str("Worst peak-to-trough drawdown under the fair top-3 capped-book lens.\n\n");

    for summary in summaries {
        out.push_str(&format!(
            "## {} — {}\n\n- Worst drawdown: **{:.1}%**\n- Peak index: **{}**\n- Trough index: **{}**\n- Peak-to-trough return: **{:+.1}%**\n\n| Symbol | Contribution during drawdown |\n|---|---:|\n",
            summary.universe,
            summary.strategy,
            summary.trough_dd_pct,
            summary.peak_idx,
            summary.trough_idx,
            summary.window_return_pct,
        ));
        for row in &summary.contributions {
            out.push_str(&format!(
                "| {} | {:+.1}% |\n",
                row.symbol, row.contribution_pct
            ));
        }
        out.push('\n');
    }

    fs::write(path, out)?;
    Ok(())
}

fn write_csv(summaries: &[DrawdownSummary], path: &str) -> Result<()> {
    let mut out = String::from("universe,strategy,peak_idx,trough_idx,worst_drawdown_pct,window_return_pct,symbol,contribution_pct\n");
    for summary in summaries {
        for row in &summary.contributions {
            out.push_str(&format!(
                "{},{},{},{},{:.4},{:.4},{},{:.4}\n",
                summary.universe,
                summary.strategy,
                summary.peak_idx,
                summary.trough_idx,
                summary.trough_dd_pct,
                summary.window_return_pct,
                row.symbol,
                row.contribution_pct,
            ));
        }
    }
    fs::write(path, out)?;
    Ok(())
}

fn draw_attribution_chart(summaries: &[DrawdownSummary], path: &str) -> Result<()> {
    let root = BitMapBackend::new(path, (1800, 1000)).into_drawing_area();
    root.fill(&WHITE)?;
    let areas = root.split_evenly((2, 2));

    for (summary, area) in summaries.iter().zip(areas.into_iter()) {
        let mut rows = summary.contributions.clone();
        rows.sort_by(|a, b| a.contribution_pct.partial_cmp(&b.contribution_pct).unwrap());
        let labels = rows.iter().map(|r| r.symbol.clone()).collect::<Vec<_>>();
        let min_x = rows
            .iter()
            .map(|r| r.contribution_pct)
            .fold(-5.0f64, f64::min)
            .min(-1.0);
        let max_x = rows
            .iter()
            .map(|r| r.contribution_pct)
            .fold(5.0f64, f64::max)
            .max(1.0);

        let mut chart = ChartBuilder::on(&area)
            .margin(20)
            .caption(
                format!(
                    "{} — {}\nWorst DD {:.1}% ({} -> {})",
                    summary.universe,
                    summary.strategy,
                    summary.trough_dd_pct,
                    summary.peak_idx,
                    summary.trough_idx
                ),
                ("sans-serif", 24).into_font(),
            )
            .x_label_area_size(60)
            .y_label_area_size(80)
            .build_cartesian_2d((min_x * 1.15)..(max_x * 1.15), 0usize..rows.len().max(1))?;

        chart
            .configure_mesh()
            .disable_mesh()
            .x_desc("contribution during worst drawdown (%)")
            .y_labels(rows.len())
            .y_label_formatter(&|y| labels.get(*y).cloned().unwrap_or_default())
            .draw()?;

        chart.draw_series(rows.iter().enumerate().map(|(idx, row)| {
            let color = if row.contribution_pct < 0.0 {
                RED.filled()
            } else {
                BLUE.filled()
            };
            Rectangle::new([(0.0, idx), (row.contribution_pct, idx + 1)], color)
        }))?;
    }

    root.present()?;
    Ok(())
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
