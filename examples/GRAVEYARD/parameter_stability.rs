//! Parameter Stability Analysis for BollingerReversion 1d
//!
//! Tests whether the ATR×multiplier parameter is robust or just a lucky needle.
//! A real edge should have a smooth peak in the Sharpe curve, not a single spike.
//!
//! Outputs:
//! - Console table of Sharpe vs ATR mult for each symbol
//! - Charts saved to krypto/charts/parameter_stability.png
//!
//! Usage:
//!   cargo run --profile sweep --example parameter_stability

use anyhow::Result;
use colored::*;
use krypto::{
    algo::StrategyRegistry, backtest::engine::Backtester, data::loader::DataLoader,
    features::indicators::FeatureEngine,
};
use plotters::prelude::*;
use plotters::style::Color;
use polars::prelude::*;

const CANDLES: u32 = 2000;
const CAPITAL: f64 = 10_000.0;
const TAKER_FEE: f64 = 0.001;

const SYMBOLS: &[&str] = &["BTCFDUSD", "ETHFDUSD", "SOLFDUSD", "XRPFDUSD", "DOGEFDUSD"];

// Fine-grained ATR multipliers
const ATR_MULTS: &[f64] = &[
    0.20, 0.25, 0.30, 0.35, 0.40, 0.45, 0.50, 0.60, 0.70, 0.80, 0.90, 1.0, 1.25, 1.5, 2.0,
];

#[derive(Debug, Clone)]
struct ParamResult {
    atr_mult: f64,
    return_pct: f64,
    sharpe: f64,
    max_dd: f64,
    trades: usize,
    win_rate: f64,
}

fn compute_stop_pct(df: &DataFrame, atr_mult: f64) -> f64 {
    let atr = df
        .column("atr")
        .ok()
        .and_then(|s| {
            s.f64()
                .ok()
                .and_then(|ca| ca.get(ca.len().saturating_sub(1)))
        })
        .unwrap_or(0.0);
    let close = df
        .column("close")
        .ok()
        .and_then(|s| {
            s.f64()
                .ok()
                .and_then(|ca| ca.get(ca.len().saturating_sub(1)))
        })
        .unwrap_or(1.0);
    if close > 0.0 {
        (atr * atr_mult / close).clamp(0.005, 0.30)
    } else {
        0.05
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    println!("\n{}", "━".repeat(80).bright_cyan());
    println!(
        "{}",
        "  PARAMETER STABILITY ANALYSIS — BollingerReversion 1d"
            .bright_cyan()
            .bold()
    );
    println!("{}", "━".repeat(80).bright_cyan());

    let loader = DataLoader::new(None, None);
    let registry = StrategyRegistry::new();

    // Data structure: (symbol, Vec<(atr_mult, sharpe, return, dd, trades)>)
    let mut all_data: Vec<(String, Vec<ParamResult>)> = Vec::new();

    for symbol in SYMBOLS {
        print!("▶ {:<12} ", symbol);

        let raw = match loader.fetch_data(symbol, "1d", CANDLES).await {
            Ok(d) => d,
            Err(e) => {
                println!("{} ({})", "SKIP".red(), e);
                continue;
            }
        };
        let df = match FeatureEngine::add_technicals(&raw, None) {
            Ok(d) => d,
            Err(e) => {
                println!("{} ({})", "FEATURES FAIL".red(), e);
                continue;
            }
        };
        print!("({} bars) ", df.height());

        let strategy = registry.create("bollinger_reversion").unwrap();
        let signals = match strategy.predict(&df) {
            Ok(s) => s,
            Err(e) => {
                println!("{} predict: {}", "✗".red(), e);
                continue;
            }
        };

        let mut results: Vec<ParamResult> = Vec::new();

        for &atr_mult in ATR_MULTS {
            let stop_pct = compute_stop_pct(&df, atr_mult);
            let bt = Backtester::new(CAPITAL, TAKER_FEE, 0.0);

            if let Ok(result) = bt.run(&df, &signals, stop_pct, 0.0) {
                if result.total_trades >= 10 {
                    results.push(ParamResult {
                        atr_mult,
                        return_pct: result.total_return_pct,
                        sharpe: result.sharpe_ratio,
                        max_dd: result.max_drawdown_pct,
                        trades: result.total_trades,
                        win_rate: result.win_rate,
                    });
                }
            }
        }

        // Find peak
        if let Some(best) = results.iter().max_by(|a, b| {
            a.sharpe
                .partial_cmp(&b.sharpe)
                .unwrap_or(std::cmp::Ordering::Equal)
        }) {
            println!(
                "Peak: ATR×{:.2} → Sharpe {:.2}, Return {:.1}%",
                best.atr_mult, best.sharpe, best.return_pct
            );
        }

        all_data.push((symbol.to_string(), results));
    }

    // Print detailed table
    println!("\n{}", "━".repeat(80).bright_cyan());
    println!("{}", "  SHARPE BY ATR MULTIPLIER".bright_cyan().bold());
    println!("{}", "━".repeat(80).bright_cyan());

    print!("{:<12}", "ATR×");
    for (symbol, _) in &all_data {
        print!("{:>10}", symbol.replace("FDUSD", ""));
    }
    println!();
    println!("{}", "-".repeat(70));

    for &atr in ATR_MULTS {
        print!("{:<12.2}", atr);
        for (_, results) in &all_data {
            if let Some(r) = results.iter().find(|r| (r.atr_mult - atr).abs() < 0.001) {
                if r.sharpe > 20.0 {
                    print!("{}{:>9.1}{}", "🏆".green(), r.sharpe, "");
                } else if r.sharpe > 5.0 {
                    print!("{}{:>9.1}{}", "✅".green(), r.sharpe, "");
                } else if r.sharpe > 0.0 {
                    print!("{:>10.1}", r.sharpe);
                } else {
                    print!("{}{:>9.1}{}", "", r.sharpe, "");
                }
            } else {
                print!("{:>10}", "-");
            }
        }
        println!();
    }

    // Find optimal range (where Sharpe is within 10% of peak)
    println!("\n{}", "━".repeat(80).bright_cyan());
    println!(
        "{}",
        "  OPTIMAL RANGE (Sharpe within 90% of peak)"
            .bright_cyan()
            .bold()
    );
    println!("{}", "━".repeat(80).bright_cyan());

    let mut wide_peaks = 0;
    let mut needle_peaks = 0;

    for (symbol, results) in &all_data {
        if results.is_empty() {
            continue;
        }

        let best = results
            .iter()
            .max_by(|a, b| {
                a.sharpe
                    .partial_cmp(&b.sharpe)
                    .unwrap_or(std::cmp::Ordering::Equal)
            })
            .unwrap();
        let threshold = best.sharpe * 0.9;

        let good_range: Vec<&ParamResult> =
            results.iter().filter(|r| r.sharpe >= threshold).collect();

        if good_range.len() >= 3 {
            wide_peaks += 1;
            println!(
                "{} {} — ATR×{:.2} to ×{:.2} ({} configs within 90%)",
                "✅".green(),
                symbol,
                good_range.first().unwrap().atr_mult,
                good_range.last().unwrap().atr_mult,
                good_range.len()
            );
        } else {
            needle_peaks += 1;
            println!(
                "{} {} — NEEDLE at ATR×{:.2} (only {} config within 90%)",
                "⚠️".yellow(),
                symbol,
                best.atr_mult,
                good_range.len()
            );
        }
    }

    // Generate chart
    println!("\n{}", "━".repeat(80).bright_cyan());
    println!("  Generating chart...");

    let chart_path =
        "/home/ubuntu/.openclaw/workspace-krypto/krypto/charts/parameter_stability.png";
    std::fs::create_dir_all("/home/ubuntu/.openclaw/workspace-krypto/krypto/charts").ok();

    let root = BitMapBackend::new(chart_path, (1200, 600)).into_drawing_area();
    root.fill(&WHITE)?;

    let mut chart = ChartBuilder::on(&root)
        .caption(
            "Parameter Stability: Sharpe vs ATR Multiplier",
            ("sans-serif", 20).into_font(),
        )
        .margin(20)
        .x_label_area_size(40)
        .y_label_area_size(60)
        .build_cartesian_2d(0.15..2.1, -20.0..550.0)?;

    chart
        .configure_mesh()
        .x_desc("ATR Multiplier")
        .y_desc("Sharpe Ratio")
        .x_labels(15)
        .y_labels(10)
        .draw()?;

    let colors = [RED, BLUE, GREEN, MAGENTA, CYAN];

    for (i, (symbol, results)) in all_data.iter().enumerate() {
        let color = colors[i % colors.len()];
        let points: Vec<(f64, f64)> = results.iter().map(|r| (r.atr_mult, r.sharpe)).collect();

        chart
            .draw_series(LineSeries::new(points, color.stroke_width(2)))?
            .label(symbol.clone())
            .legend(move |(x, y)| PathElement::new(vec![(x, y), (x + 20, y)], color));
    }

    chart
        .configure_series_labels()
        .background_style(WHITE.mix(0.8))
        .border_style(&BLACK)
        .draw()?;

    root.present()?;
    println!("  Chart saved to: {}", chart_path);

    // Verdict
    println!("\n{}", "━".repeat(80).bright_cyan());
    if wide_peaks == SYMBOLS.len() {
        println!("  ✅ ALL symbols have WIDE parameter stability — edge is robust");
    } else if wide_peaks >= SYMBOLS.len() / 2 {
        println!(
            "  ⚠️ {}/{} symbols have wide peaks, {} are needles",
            wide_peaks,
            SYMBOLS.len(),
            needle_peaks
        );
    } else {
        println!("  ❌ MOST symbols have needle peaks — likely overfitting");
    }
    println!("{}", "━".repeat(80).bright_cyan());

    Ok(())
}
