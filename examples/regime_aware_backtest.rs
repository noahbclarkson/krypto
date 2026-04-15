//! Regime-aware backtesting example.
//!
//! Demonstrates how to use RegimeDetector to adapt strategy selection
//! and position sizing based on current market conditions:
//! - Trending markets → trend-following strategies
//! - Sideways markets → mean-reversion strategies
//! - Volatile markets → reduced position size

use anyhow::Result;
use colored::*;
use krypto::{
    algo::{
        regime::{MarketRegime, RegimeDetector, VolatilityRegime},
        StrategyRegistry,
    },
    backtest::engine::{Backtester, PositionSizing},
    data::loader::DataLoader,
    features::indicators::FeatureEngine,
};

const SYMBOL: &str = "BTCUSDT";
const INTERVAL: &str = "4h";
const CANDLES: u32 = 2000;
const CAPITAL: f64 = 10_000.0;

/// Adaptive position sizing based on regime.
fn regime_position_sizing(regime: &MarketRegime, vol: &VolatilityRegime) -> PositionSizing {
    match (regime, vol) {
        // Sideways + LowVol = Ideal for mean-reversion, full size
        (MarketRegime::Sideways, VolatilityRegime::LowVol) => PositionSizing::Full,

        // Trending + HighVol = Trend-following works, but be careful
        (MarketRegime::TrendingBull, VolatilityRegime::HighVol) => {
            PositionSizing::FixedFraction(0.75)
        }
        (MarketRegime::TrendingBear, VolatilityRegime::HighVol) => {
            PositionSizing::FixedFraction(0.5) // More conservative in bear markets
        }

        // Volatile regime = Reduce exposure regardless of trend
        (_, VolatilityRegime::HighVol) => PositionSizing::FixedFraction(0.5),

        // Default = Normal sizing
        _ => PositionSizing::FixedFraction(0.75),
    }
}

/// Select strategy based on regime.
fn regime_strategy(regime: &MarketRegime) -> &'static str {
    match regime {
        MarketRegime::TrendingBull | MarketRegime::TrendingBear => "dynamic_trend",
        MarketRegime::Sideways => "bollinger_reversion",
        MarketRegime::Volatile => "volatility_squeeze",
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    println!("\n{}", "━".repeat(70).bright_cyan());
    println!("{}", "  REGIME-AWARE BACKTESTING".bright_cyan().bold());
    println!("{}", "━".repeat(70).bright_cyan());
    println!();
    println!(
        "Symbol: {} | Interval: {} | Candles: {}",
        SYMBOL, INTERVAL, CANDLES
    );
    println!();

    // Load data
    let loader = DataLoader::new(None, None);
    let raw_df = loader.fetch_data(SYMBOL, INTERVAL, CANDLES).await?;
    let df = FeatureEngine::add_technicals(&raw_df, None)?;

    println!("Loaded {} candles", df.height());

    // Detect regime at the end of the dataset
    let regime_info = RegimeDetector::detect_full(&df);

    println!();
    println!("{}", "─".repeat(70));
    println!("{}", "  CURRENT MARKET REGIME".yellow().bold());
    println!("{}", "─".repeat(70));
    println!("Market Regime:    {:?}", regime_info.market);
    println!("Volatility:       {:?}", regime_info.volatility);
    println!("ATR Ratio:        {:.2}", regime_info.atr_ratio);
    println!("Trend Strength:   {:.1}", regime_info.trend_strength);
    println!("Confidence:       {:.1}%", regime_info.confidence * 100.0);
    println!();

    // Select strategy based on regime
    let strategy_name = regime_strategy(&regime_info.market);
    let sizing = regime_position_sizing(&regime_info.market, &regime_info.volatility);

    println!("{}", "─".repeat(70));
    println!("{}", "  ADAPTIVE CONFIGURATION".green().bold());
    println!("{}", "─".repeat(70));
    println!("Selected Strategy: {}", strategy_name);
    println!("Position Sizing:   {:?}", sizing);
    println!();

    // Generate signals
    let registry = StrategyRegistry::new();
    let strategy = registry
        .create(strategy_name)
        .ok_or_else(|| anyhow::anyhow!("Strategy {} not found", strategy_name))?;
    let signals = strategy.predict(&df)?;

    let signal_count = signals
        .i32()
        .map(|ca| {
            ca.into_iter()
                .filter(|s| s.map(|v| v != 0).unwrap_or(false))
                .count()
        })
        .unwrap_or(0);
    println!("Generated {} signals", signal_count);

    // Run backtests with different sizing strategies
    let stops = [0.05, 0.10, 0.15];
    let tps = [0.0, 0.10, 0.15];

    println!();
    println!("{}", "─".repeat(70));
    println!(
        "{}",
        "  BACKTEST RESULTS (Regime-Adaptive vs Fixed)"
            .magenta()
            .bold()
    );
    println!("{}", "─".repeat(70));
    println!();
    println!(
        "{:<15} {:>10} {:>10} {:>12} {:>12} {:>10}",
        "Sizing", "Stop", "TP", "Return%", "Sharpe", "Trades"
    );
    println!("{}", "-".repeat(70));

    let mut best_result: Option<(f64, f64, String)> = None;

    for &stop in &stops {
        for &tp in &tps {
            // Regime-adaptive sizing
            let backtester = Backtester::new(CAPITAL, 0.0004, 0.0005).with_position_sizing(sizing);
            let result = backtester.run(&df, &signals, stop, tp)?;

            let sizing_str = format!("{:?}", sizing);
            println!(
                "{:<15} {:>9.0}% {:>9.0}% {:>11.2}% {:>12.2} {:>10}",
                sizing_str.split("::").last().unwrap_or(&sizing_str),
                stop * 100.0,
                tp * 100.0,
                result.total_return_pct,
                result.sharpe_ratio,
                result.trades.len()
            );

            if best_result.is_none() || result.sharpe_ratio > best_result.as_ref().unwrap().0 {
                best_result = Some((
                    result.sharpe_ratio,
                    result.total_return_pct,
                    format!(
                        "{:?} stop={:.0}% tp={:.0}%",
                        sizing,
                        stop * 100.0,
                        tp * 100.0
                    ),
                ));
            }

            // Fixed full sizing for comparison
            let fixed_backtester = Backtester::new(CAPITAL, 0.0004, 0.0005);
            let fixed_result = fixed_backtester.run(&df, &signals, stop, tp)?;

            println!(
                "{:<15} {:>9.0}% {:>9.0}% {:>11.2}% {:>12.2} {:>10}",
                "Full (fixed)",
                stop * 100.0,
                tp * 100.0,
                fixed_result.total_return_pct,
                fixed_result.sharpe_ratio,
                fixed_result.trades.len()
            );
        }
    }

    println!();
    if let Some((sharpe, ret, config)) = best_result {
        println!(
            "{} Best config: {} (Sharpe: {:.2}, Return: {:.2}%)",
            "→".green(),
            config,
            sharpe,
            ret
        );
    }

    println!();
    println!("{}", "─".repeat(70));
    println!("{}", "  REGIME-AWARE RECOMMENDATIONS".bright_blue().bold());
    println!("{}", "─".repeat(70));
    println!();

    match regime_info.market {
        MarketRegime::TrendingBull => {
            println!("• Use trend-following strategies (dynamic_trend, macd_trend)");
            println!("• Consider wider trailing stops to let winners run");
            println!(
                "• Bull trend strength: {:.1}/100",
                regime_info.trend_strength
            );
        }
        MarketRegime::TrendingBear => {
            println!("• Reduce position size in bear markets");
            println!("• Consider short strategies or cash");
            println!(
                "• Bear trend strength: {:.1}/100",
                regime_info.trend_strength
            );
        }
        MarketRegime::Sideways => {
            println!("• Use mean-reversion strategies (bollinger_reversion, rsi_mean_reversion)");
            println!("• Tighter stops work better in range-bound markets");
            println!("• Ideal for scalping and short-term trades");
        }
        MarketRegime::Volatile => {
            println!("• Reduce position size significantly");
            println!("• Use volatility-squeeze breakout strategies");
            println!("• Wait for volatility to normalize before aggressive trading");
        }
    }

    match regime_info.volatility {
        VolatilityRegime::LowVol => {
            println!("• Low volatility = good for mean-reversion");
        }
        VolatilityRegime::NormalVol => {
            println!("• Normal volatility = standard position sizing OK");
        }
        VolatilityRegime::HighVol => {
            println!("• High volatility = reduce exposure, use wider stops");
        }
    }

    println!();
    println!(
        "{}: Regime detection confidence: {:.1}%",
        "Note".yellow(),
        regime_info.confidence * 100.0
    );

    Ok(())
}
