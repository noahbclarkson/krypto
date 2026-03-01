//! Paper trading bot example.
//!
//! Demonstrates running multiple strategies on BTCUSDT sample data
//! and comparing their win rates.
//!
//! Usage:
//!   cargo run --example paper_bot

use chrono::{TimeZone, Utc};
use krypto::paper::{Bar, BotSummary, PaperBot, Strategy, Trade};
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

    fn on_bar(&mut self, bar: &Bar, position: f64, history: &[Bar]) -> Option<Trade> {
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
        } else if bearish_cross && position == 0.0 {
            // Could also short, but keeping it simple
            None
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

        let rsi = match self.calc_rsi() {
            Some(r) => r,
            None => return None,
        };

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
// Strategy 3: Momentum Breakout
// -----------------------------------------------------------------------------

struct MomentumBreakout {
    lookback: usize,
    threshold_pct: f64,
    name: String,
}

impl MomentumBreakout {
    fn new(lookback: usize, threshold_pct: f64) -> Self {
        Self {
            lookback,
            threshold_pct,
            name: format!("Momentum_{}_{:.0}pct", lookback, threshold_pct * 100.0),
        }
    }
}

impl Strategy for MomentumBreakout {
    fn name(&self) -> &str {
        &self.name
    }

    fn on_bar(&mut self, bar: &Bar, position: f64, history: &[Bar]) -> Option<Trade> {
        if history.len() < self.lookback {
            return None;
        }

        // Get the highest high and lowest low over lookback period
        let lookback_bars: Vec<&Bar> = history.iter().rev().take(self.lookback).collect();
        let highest = lookback_bars.iter().map(|b| b.high).fold(f64::MIN, f64::max);
        let lowest = lookback_bars.iter().map(|b| b.low).fold(f64::MAX, f64::min);

        let range = highest - lowest;
        let breakout_level = highest - range * self.threshold_pct;
        let breakdown_level = lowest + range * self.threshold_pct;

        if bar.close > breakout_level && position == 0.0 {
            Some(Trade::Long { size: 1.0 })
        } else if bar.close < breakdown_level && position > 0.0 {
            Some(Trade::Close)
        } else {
            None
        }
    }
}

// -----------------------------------------------------------------------------
// Data Loading
// -----------------------------------------------------------------------------

#[derive(Debug, serde::Deserialize)]
struct JsonBar {
    open_time: i64,
    open: f64,
    high: f64,
    low: f64,
    close: f64,
    volume: f64,
}

fn load_sample_data() -> Vec<Bar> {
    let path = "data/samples/btcusdt_1h_500candles.json";
    let content = std::fs::read_to_string(path)
        .expect("Failed to read sample data file");

    let json_bars: Vec<JsonBar> = serde_json::from_str(&content)
        .expect("Failed to parse JSON");

    json_bars.into_iter().map(|jb| {
        let secs = jb.open_time / 1000;
        let nsecs = ((jb.open_time % 1000) * 1_000_000) as u32;
        let time = Utc.timestamp_opt(secs, nsecs).single().unwrap_or_else(Utc::now);

        Bar::new(time, jb.open, jb.high, jb.low, jb.close, jb.volume)
    }).collect()
}

// -----------------------------------------------------------------------------
// Main
// -----------------------------------------------------------------------------

fn print_summary(summary: &BotSummary) {
    println!(
        "  {:<20} | Win Rate: {:>6.2}% | Trades: {:>4} | Return: {:>8.2}% | DD: {:>6.2}% | PF: {:>6.2}",
        summary.strategy,
        summary.win_rate,
        summary.total_trades,
        summary.total_return_pct,
        summary.max_drawdown_pct,
        summary.profit_factor,
    );
    println!(
        "  {:<20} | Wins: {:>4} | Losses: {:>4} | Avg Win: {:>6.2}% | Avg Loss: {:>6.2}%",
        "",
        summary.wins,
        summary.losses,
        summary.avg_win_pct,
        summary.avg_loss_pct,
    );
    println!(
        "  {:<20} | Dir Accuracy: {:>6.2}% | Fees: ${:.2}",
        "",
        summary.directional_accuracy,
        summary.total_fees,
    );
}

fn main() {
    println!("=== Paper Trading Bot - Strategy Validation ===\n");

    // Load data
    println!("Loading BTCUSDT sample data...");
    let bars = load_sample_data();
    println!("Loaded {} bars\n", bars.len());

    let initial_capital = 10_000.0;

    // Create strategies
    let strategies: Vec<(&str, Box<dyn Strategy>)> = vec![
        ("SMA Crossover", Box::new(SmaCrossover::new(20, 50))),
        ("RSI Reversion", Box::new(RsiStrategy::new(14, 30.0, 70.0))),
        ("Momentum", Box::new(MomentumBreakout::new(20, 0.3))),
    ];

    println!("Running {} strategies...\n", strategies.len());
    println!("{}", "=".repeat(100));

    let mut summaries: Vec<BotSummary> = Vec::new();

    for (label, strategy) in strategies {
        println!("\nStrategy: {}", label);
        println!("{}", "-".repeat(60));

        let mut bot = PaperBot::new(strategy, initial_capital)
            .with_fee(0.001)  // 0.1% fee
            .with_trailing_stop(0.05); // 5% trailing stop

        for bar in &bars {
            bot.on_bar(bar);
        }

        let summary = bot.summary();
        summaries.push(summary.clone());
        print_summary(&summary);
    }

    // Final comparison
    println!("\n{}", "=".repeat(100));
    println!("\n=== STRATEGY COMPARISON ===\n");
    println!(
        "{:<20} | {:>8} | {:>8} | {:>8} | {:>8} | {:>8}",
        "Strategy", "Win Rate", "Trades", "Return%", "Max DD%", "Dir Acc%"
    );
    println!("{}", "-".repeat(80));

    for s in &summaries {
        println!(
            "{:<20} | {:>7.2}% | {:>7} | {:>7.2}% | {:>7.2}% | {:>7.2}%",
            s.strategy,
            s.win_rate,
            s.total_trades,
            s.total_return_pct,
            s.max_drawdown_pct,
            s.directional_accuracy,
        );
    }

    // Check target
    println!("\n=== TARGET VALIDATION (>52% directional accuracy) ===\n");
    let target = 52.0;
    let passing: Vec<_> = summaries.iter().filter(|s| s.directional_accuracy >= target).collect();

    if passing.is_empty() {
        println!("❌ No strategies meet the {:.0}% directional accuracy target.", target);
    } else {
        println!("✅ {} strateg(y/ies) meet the {:.0}% target:", passing.len(), target);
        for s in passing {
            println!("   - {} ({:.2}%)", s.strategy, s.directional_accuracy);
        }
    }

    // Best strategy
    let best = summaries.iter()
        .max_by(|a, b| a.directional_accuracy.partial_cmp(&b.directional_accuracy).unwrap())
        .unwrap();

    println!("\n=== BEST STRATEGY ===");
    println!("{} with {:.2}% directional accuracy", best.strategy, best.directional_accuracy);
}
