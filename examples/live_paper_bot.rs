//! Live paper trading bot example.
//!
//! Fetches live BTCUSDT data from Binance and runs paper trading strategies.
//!
//! Usage:
//!   cargo run --example live_paper_bot
//!
//! Options:
//!   --candles <num>    Number of candles to fetch (default: 1000)
//!   --interval <str>   Candle interval (default: 1h)
//!   --capital <num>    Initial capital (default: 10000)

use chrono::{DateTime, Utc};
use colored::*;
use krypto::{
    data::loader::DataLoader,
    paper::{Bar, BotSummary, PaperBot, Strategy, Trade},
};
use std::collections::VecDeque;

// -----------------------------------------------------------------------------
// Strategy 1: Simple Moving Average Crossover
// -----------------------------------------------------------------------------

struct SmaCrossover {
    fast_period: usize,
    slow_period: usize,
    name: String,
}

impl SmaCrossover {
    fn new(fast: usize, slow: usize) -> Self {
        Self {
            fast_period: fast,
            slow_period: slow,
            name: format!("SMA_{}_{}", fast, slow),
        }
    }

    fn calc_sma(prices: &[f64], period: usize) -> Option<f64> {
        if prices.len() < period {
            return None;
        }
        let sum: f64 = prices.iter().rev().take(period).sum();
        Some(sum / period as f64)
    }
}

impl Strategy for SmaCrossover {
    fn name(&self) -> &str {
        &self.name
    }

    fn on_bar(&mut self, _bar: &Bar, position: f64, history: &[Bar]) -> Option<Trade> {
        if history.len() < self.slow_period + 1 {
            return None;
        }

        let closes: Vec<f64> = history.iter().map(|b| b.close).collect();
        let fast_sma = Self::calc_sma(&closes, self.fast_period)?;
        let slow_sma = Self::calc_sma(&closes, self.slow_period)?;

        let prev_fast = Self::calc_sma(&closes[..closes.len() - 1], self.fast_period)?;
        let prev_slow = Self::calc_sma(&closes[..closes.len() - 1], self.slow_period)?;

        // Crossover detection
        let bullish_cross = prev_fast <= prev_slow && fast_sma > slow_sma;
        let bearish_cross = prev_fast >= prev_slow && fast_sma < slow_sma;

        if bullish_cross && position == 0.0 {
            Some(Trade::Long { size: 1.0 })
        } else if bearish_cross && position > 0.0 {
            Some(Trade::Close)
        } else {
            None
        }
    }
}

// -----------------------------------------------------------------------------
// Strategy 2: RSI Mean Reversion
// -----------------------------------------------------------------------------

struct RsiStrategy {
    period: usize,
    oversold: f64,
    overbought: f64,
    prices: VecDeque<f64>,
}

impl RsiStrategy {
    fn new(period: usize, oversold: f64, overbought: f64) -> Self {
        Self {
            period,
            oversold,
            overbought,
            prices: VecDeque::with_capacity(period + 1),
        }
    }

    fn calc_rsi(&self) -> Option<f64> {
        if self.prices.len() < self.period + 1 {
            return None;
        }

        let mut gains = 0.0;
        let mut losses = 0.0;
        let prices: Vec<f64> = self.prices.iter().copied().collect();

        for i in 1..=self.period {
            let change = prices[i] - prices[i - 1];
            if change > 0.0 {
                gains += change;
            } else {
                losses += change.abs();
            }
        }

        let avg_gain = gains / self.period as f64;
        let avg_loss = losses / self.period as f64;

        if avg_loss == 0.0 {
            return Some(100.0);
        }

        let rs = avg_gain / avg_loss;
        Some(100.0 - (100.0 / (1.0 + rs)))
    }
}

impl Strategy for RsiStrategy {
    fn name(&self) -> &str {
        "RSI_Reversion"
    }

    fn on_bar(&mut self, bar: &Bar, position: f64, _history: &[Bar]) -> Option<Trade> {
        self.prices.push_back(bar.close);
        if self.prices.len() > self.period + 1 {
            self.prices.pop_front();
        }

        let rsi = self.calc_rsi()?;

        if rsi < self.oversold && position == 0.0 {
            Some(Trade::Long { size: 1.0 })
        } else if rsi > self.overbought && position > 0.0 {
            Some(Trade::Close)
        } else {
            None
        }
    }

    fn reset(&mut self) {
        self.prices.clear();
    }
}

// -----------------------------------------------------------------------------
// Helper Functions
// -----------------------------------------------------------------------------

fn dataframe_to_bars(df: &polars::prelude::DataFrame) -> anyhow::Result<Vec<Bar>> {
    #[allow(unused_imports)]
    use polars::prelude::*;

    let time_col = df.column("time")?.datetime()?;
    let open_col = df.column("open")?.f64()?;
    let high_col = df.column("high")?.f64()?;
    let low_col = df.column("low")?.f64()?;
    let close_col = df.column("close")?.f64()?;
    let volume_col = df.column("volume")?.f64()?;

    let mut bars = Vec::with_capacity(df.height());

    for i in 0..df.height() {
        let time_ms = time_col.get(i).unwrap_or(0);
        let secs = time_ms / 1000;
        let nsecs = ((time_ms % 1000) * 1_000_000) as u32;

        let time = DateTime::from_timestamp(secs, nsecs).unwrap_or_else(Utc::now);

        bars.push(Bar::new(
            time,
            open_col.get(i).unwrap_or(0.0),
            high_col.get(i).unwrap_or(0.0),
            low_col.get(i).unwrap_or(0.0),
            close_col.get(i).unwrap_or(0.0),
            volume_col.get(i).unwrap_or(0.0),
        ));
    }

    Ok(bars)
}

fn print_summary(summary: &BotSummary) {
    let status = if summary.total_return_pct >= 0.0 {
        "✓".green()
    } else {
        "✗".red()
    };

    println!(
        "{} {:<20} | Win Rate: {:>6.2}% | Trades: {:>4} | Return: {:>8.2}% | DD: {:>6.2}% | PF: {:>6.2}",
        status,
        summary.strategy,
        summary.win_rate,
        summary.total_trades,
        summary.total_return_pct,
        summary.max_drawdown_pct,
        summary.profit_factor,
    );
    println!(
        "  {:<20} | Wins: {:>4} | Losses: {:>4} | Avg Win: {:>6.2}% | Avg Loss: {:>6.2}%",
        "", summary.wins, summary.losses, summary.avg_win_pct, summary.avg_loss_pct,
    );
    println!(
        "  {:<20} | Dir Accuracy: {:>6.2}% | Total PnL: ${:.2} | Fees: ${:.2}",
        "", summary.directional_accuracy, summary.total_pnl, summary.total_fees,
    );
}

fn parse_args() -> (u16, String, f64) {
    let args: Vec<String> = std::env::args().collect();

    let mut candles: u32 = 1000;
    let mut interval = "1h".to_string();
    let mut capital = 10_000.0;

    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "--candles" => {
                if i + 1 < args.len() {
                    candles = args[i + 1].parse().unwrap_or(1000);
                    i += 1;
                }
            }
            "--interval" => {
                if i + 1 < args.len() {
                    interval = args[i + 1].clone();
                    i += 1;
                }
            }
            "--capital" => {
                if i + 1 < args.len() {
                    capital = args[i + 1].parse().unwrap_or(10_000.0);
                    i += 1;
                }
            }
            _ => {}
        }
        i += 1;
    }

    (candles as u16, interval, capital)
}

// -----------------------------------------------------------------------------
// Main
// -----------------------------------------------------------------------------

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Initialize logging
    tracing_subscriber::fmt::init();

    let (candles, interval, initial_capital) = parse_args();

    println!("{}", "=".repeat(100).bold());
    println!("{}", "  LIVE PAPER TRADING BOT - BTCUSDT".bold().cyan());
    println!("{}", "=".repeat(100).bold());
    println!();

    println!("Configuration:");
    println!("  Symbol:   {}", "BTCUSDT".yellow());
    println!("  Interval: {}", interval.yellow());
    println!("  Candles:  {}", candles);
    println!("  Capital:  ${:.2}", initial_capital);
    println!();

    // Fetch live data
    println!("{}", "Fetching live data from Binance...".cyan());
    let loader = DataLoader::new(None, None);

    let df = loader
        .fetch_data("BTCUSDT", &interval, candles as u32)
        .await?;
    println!("{} Fetched {} candles", "✓".green(), df.height());
    println!();

    // Convert to bars
    println!("{}", "Converting data to bars...".cyan());
    let bars = dataframe_to_bars(&df)?;
    println!("{} Converted {} bars", "✓".green(), bars.len());

    if !bars.is_empty() {
        let first = &bars[0];
        let last = &bars[bars.len() - 1];
        println!(
            "  Time range: {} to {}",
            first.time.format("%Y-%m-%d %H:%M"),
            last.time.format("%Y-%m-%d %H:%M")
        );
        println!(
            "  Price range: ${:.2} - ${:.2}",
            bars.iter().map(|b| b.low).fold(f64::MAX, f64::min),
            bars.iter().map(|b| b.high).fold(f64::MIN, f64::max)
        );
    }
    println!();

    // Create strategies
    let strategies: Vec<(&str, Box<dyn Strategy>)> = vec![
        ("SMA Crossover (10/30)", Box::new(SmaCrossover::new(10, 30))),
        ("SMA Crossover (20/50)", Box::new(SmaCrossover::new(20, 50))),
        (
            "RSI Mean Reversion",
            Box::new(RsiStrategy::new(14, 30.0, 70.0)),
        ),
    ];

    println!(
        "{}",
        format!("Running {} strategies...", strategies.len()).cyan()
    );
    println!("{}", "=".repeat(100));
    println!();

    let mut summaries: Vec<BotSummary> = Vec::new();

    for (label, strategy) in strategies {
        println!("{}", format!("Strategy: {}", label).bold());
        println!("{}", "-".repeat(80));

        let mut bot = PaperBot::new(strategy, initial_capital)
            .with_fee(0.001) // 0.1% fee
            .with_trailing_stop(0.15); // 15% trailing stop

        // Process all bars
        for bar in &bars {
            bot.on_bar(bar);
        }

        let summary = bot.summary();
        summaries.push(summary.clone());
        print_summary(&summary);
        println!();
    }

    // Final comparison
    println!("{}", "=".repeat(100).bold());
    println!("{}", "  STRATEGY COMPARISON".bold().cyan());
    println!("{}", "=".repeat(100).bold());
    println!();

    println!(
        "{:<25} | {:>8} | {:>7} | {:>9} | {:>9} | {:>8}",
        "Strategy", "Win Rate", "Trades", "Return%", "Max DD%", "Dir Acc%"
    );
    println!("{}", "-".repeat(90));

    for s in &summaries {
        let status = if s.total_return_pct >= 0.0 {
            s.strategy.green()
        } else {
            s.strategy.red()
        };

        println!(
            "{:<25} | {:>7.2}% | {:>6} | {:>8.2}% | {:>8.2}% | {:>7.2}%",
            status,
            s.win_rate,
            s.total_trades,
            s.total_return_pct,
            s.max_drawdown_pct,
            s.directional_accuracy,
        );
    }
    println!();

    // Performance stats
    println!("{}", "=".repeat(100).bold());
    println!("{}", "  PERFORMANCE SUMMARY".bold().cyan());
    println!("{}", "=".repeat(100).bold());
    println!();

    // Best strategy
    let best = summaries
        .iter()
        .max_by(|a, b| a.total_return_pct.partial_cmp(&b.total_return_pct).unwrap())
        .unwrap();

    println!(
        "{} Best Return: {} ({:.2}%)",
        "🏆".yellow(),
        best.strategy.bold(),
        best.total_return_pct
    );

    let best_accuracy = summaries
        .iter()
        .max_by(|a, b| {
            a.directional_accuracy
                .partial_cmp(&b.directional_accuracy)
                .unwrap()
        })
        .unwrap();

    println!(
        "{} Best Accuracy: {} ({:.2}%)",
        "🎯".yellow(),
        best_accuracy.strategy.bold(),
        best_accuracy.directional_accuracy
    );

    let lowest_dd = summaries
        .iter()
        .min_by(|a, b| a.max_drawdown_pct.partial_cmp(&b.max_drawdown_pct).unwrap())
        .unwrap();

    println!(
        "{} Lowest Drawdown: {} ({:.2}%)",
        "🛡️".yellow(),
        lowest_dd.strategy.bold(),
        lowest_dd.max_drawdown_pct
    );
    println!();

    // Check target
    let target = 52.0;
    let passing: Vec<_> = summaries
        .iter()
        .filter(|s| s.directional_accuracy >= target)
        .collect();

    println!("{}", "=".repeat(100).bold());
    if passing.is_empty() {
        println!(
            "❌ No strategies meet the {:.0}% directional accuracy target.",
            target
        );
    } else {
        println!(
            "✅ {} strateg(y/ies) meet the {:.0}% accuracy target:",
            passing.len(),
            target
        );
        for s in passing {
            println!("   • {} ({:.2}%)", s.strategy, s.directional_accuracy);
        }
    }
    println!("{}", "=".repeat(100).bold());

    Ok(())
}
