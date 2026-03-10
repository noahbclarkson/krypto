//! Simple strategy grid search with directional accuracy validation.
//!
//! Tests very simple strategies with minimal parameters to find ones
//! that achieve >52% directional accuracy (minimum viable for live trading).
//!
//! Usage:
//!   cargo run --example simple_strategy_grid -- [symbol] [interval] [candles]
//!   cargo run --example simple_strategy_grid -- BTCUSDT 1h 1000

use chrono::{TimeZone, Utc};
use krypto::{
    data::loader::DataLoader,
    paper::{Bar, BotSummary, PaperBot, Strategy, Trade},
};

// -----------------------------------------------------------------------------
// Simple SMA Crossover Strategy
// -----------------------------------------------------------------------------

struct SmaCrossover {
    fast: usize,
    slow: usize,
    name: String,
}

impl SmaCrossover {
    fn new(fast: usize, slow: usize) -> Self {
        Self {
            fast,
            slow,
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
        if history.len() < self.slow + 1 {
            return None;
        }

        let closes: Vec<f64> = history.iter().map(|b| b.close).collect();
        let fast_sma = Self::calc_sma(&closes, self.fast)?;
        let slow_sma = Self::calc_sma(&closes, self.slow)?;

        let prev_fast = Self::calc_sma(&closes[..closes.len() - 1], self.fast)?;
        let prev_slow = Self::calc_sma(&closes[..closes.len() - 1], self.slow)?;

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
// Simple EMA Crossover Strategy
// -----------------------------------------------------------------------------

struct EmaCrossover {
    fast: usize,
    slow: usize,
    name: String,
    fast_ema: Option<f64>,
    slow_ema: Option<f64>,
}

impl EmaCrossover {
    fn new(fast: usize, slow: usize) -> Self {
        Self {
            fast,
            slow,
            name: format!("EMA_{}_{}", fast, slow),
            fast_ema: None,
            slow_ema: None,
        }
    }

    fn calc_ema(prev_ema: Option<f64>, price: f64, period: usize) -> f64 {
        let multiplier = 2.0 / (period as f64 + 1.0);
        match prev_ema {
            Some(ema) => (price - ema) * multiplier + ema,
            None => price,
        }
    }
}

impl Strategy for EmaCrossover {
    fn name(&self) -> &str {
        &self.name
    }

    fn on_bar(&mut self, bar: &Bar, position: f64, _history: &[Bar]) -> Option<Trade> {
        // Update EMAs
        let new_fast = Self::calc_ema(self.fast_ema, bar.close, self.fast);
        let new_slow = Self::calc_ema(self.slow_ema, bar.close, self.slow);

        let prev_fast = self.fast_ema;
        let prev_slow = self.slow_ema;

        self.fast_ema = Some(new_fast);
        self.slow_ema = Some(new_slow);

        // Need previous values for crossover detection
        let (Some(prev_fast), Some(prev_slow)) = (prev_fast, prev_slow) else {
            return None;
        };

        let bullish_cross = prev_fast <= prev_slow && new_fast > new_slow;
        let bearish_cross = prev_fast >= prev_slow && new_fast < new_slow;

        if bullish_cross && position == 0.0 {
            Some(Trade::Long { size: 1.0 })
        } else if bearish_cross && position > 0.0 {
            Some(Trade::Close)
        } else {
            None
        }
    }

    fn reset(&mut self) {
        self.fast_ema = None;
        self.slow_ema = None;
    }
}

// -----------------------------------------------------------------------------
// Price Momentum Strategy (simplest possible)
// -----------------------------------------------------------------------------

struct PriceMomentum {
    lookback: usize,
    threshold_pct: f64,
    name: String,
}

impl PriceMomentum {
    fn new(lookback: usize, threshold_pct: f64) -> Self {
        Self {
            lookback,
            threshold_pct,
            name: format!("Mom_{}_{:.1}%", lookback, threshold_pct),
        }
    }
}

impl Strategy for PriceMomentum {
    fn name(&self) -> &str {
        &self.name
    }

    fn on_bar(&mut self, _bar: &Bar, position: f64, history: &[Bar]) -> Option<Trade> {
        if history.len() < self.lookback + 1 {
            return None;
        }

        let current = history.last()?.close;
        let past = history[history.len() - self.lookback - 1].close;

        let change_pct = (current - past) / past * 100.0;

        if change_pct > self.threshold_pct && position == 0.0 {
            Some(Trade::Long { size: 1.0 })
        } else if change_pct < -self.threshold_pct && position > 0.0 {
            Some(Trade::Close)
        } else {
            None
        }
    }
}

// -----------------------------------------------------------------------------
// Main
// -----------------------------------------------------------------------------

fn print_summary(summary: &BotSummary, passed: bool) {
    let marker = if passed { "✓" } else { " " };
    println!(
        "{} {:<20} | Dir.Acc: {:>5.1}% | Win: {:>5.1}% | Trades: {:>3} | Return: {:>7.2}% | PF: {:>5.2}",
        marker,
        summary.strategy,
        summary.directional_accuracy,
        summary.win_rate,
        summary.total_trades,
        summary.total_return_pct,
        summary.profit_factor,
    );
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().collect();
    let symbol = args.get(1).map(|s| s.as_str()).unwrap_or("BTCUSDT");
    let interval = args.get(2).map(|s| s.as_str()).unwrap_or("1h");
    let candles: i32 = args.get(3).and_then(|s| s.parse().ok()).unwrap_or(1000);

    println!("=== Simple Strategy Grid Search ===");
    println!("Symbol: {}, Interval: {}, Candles: {}\n", symbol, interval, candles);

    // Load data
    println!("Fetching data from Binance...");
    let loader = DataLoader::new(None, None);
    let df = loader.fetch_data(symbol, interval, candles as u32).await?;
    
    let n_rows = df.height();
    println!("Loaded {} bars\n", n_rows);

    // Convert to bars
    let time_col = df.column("time")?.datetime()?;
    let open_col = df.column("open")?.f64()?;
    let high_col = df.column("high")?.f64()?;
    let low_col = df.column("low")?.f64()?;
    let close_col = df.column("close")?.f64()?;
    let volume_col = df.column("volume")?.f64()?;

    let bars: Vec<Bar> = (0..n_rows)
        .map(|i| {
            let time_ms = time_col.get(i).unwrap_or(0);
            let secs = time_ms / 1000;
            let nsecs = ((time_ms % 1000) * 1_000_000) as u32;
            let time = Utc.timestamp_opt(secs, nsecs).single().unwrap_or_else(Utc::now);

            Bar::new(
                time,
                open_col.get(i).unwrap_or(0.0),
                high_col.get(i).unwrap_or(0.0),
                low_col.get(i).unwrap_or(0.0),
                close_col.get(i).unwrap_or(0.0),
                volume_col.get(i).unwrap_or(0.0),
            )
        })
        .collect();

    // Define strategy grid
    let sma_params: Vec<(usize, usize)> = vec![
        (5, 10), (5, 20), (10, 20), (10, 30), (20, 50), (20, 100),
    ];

    let ema_params: Vec<(usize, usize)> = vec![
        (5, 10), (5, 20), (10, 20), (10, 30), (20, 50),
    ];

    let momentum_params: Vec<(usize, f64)> = vec![
        (5, 1.0), (5, 2.0), (10, 2.0), (10, 3.0), (20, 3.0), (20, 5.0),
    ];

    let initial_capital = 10_000.0;
    let min_directional_accuracy = 52.0;
    let min_trades = 10;

    let mut all_summaries: Vec<BotSummary> = Vec::new();
    let mut passed_summaries: Vec<BotSummary> = Vec::new();

    println!("Testing SMA Crossovers...");
    for (fast, slow) in &sma_params {
        let strategy = Box::new(SmaCrossover::new(*fast, *slow));
        let mut bot = PaperBot::new(strategy, initial_capital).with_fee(0.001);
        
        for bar in &bars {
            bot.on_bar(bar);
        }
        
        let summary = bot.summary();
        all_summaries.push(summary.clone());
        
        if summary.directional_accuracy >= min_directional_accuracy && summary.total_trades >= min_trades {
            passed_summaries.push(summary);
        }
    }

    println!("Testing EMA Crossovers...");
    for (fast, slow) in &ema_params {
        let strategy = Box::new(EmaCrossover::new(*fast, *slow));
        let mut bot = PaperBot::new(strategy, initial_capital).with_fee(0.001);
        
        for bar in &bars {
            bot.on_bar(bar);
        }
        
        let summary = bot.summary();
        all_summaries.push(summary.clone());
        
        if summary.directional_accuracy >= min_directional_accuracy && summary.total_trades >= min_trades {
            passed_summaries.push(summary);
        }
    }

    println!("Testing Momentum Strategies...\n");
    for (lookback, threshold) in &momentum_params {
        let strategy = Box::new(PriceMomentum::new(*lookback, *threshold));
        let mut bot = PaperBot::new(strategy, initial_capital).with_fee(0.001);
        
        for bar in &bars {
            bot.on_bar(bar);
        }
        
        let summary = bot.summary();
        all_summaries.push(summary.clone());
        
        if summary.directional_accuracy >= min_directional_accuracy && summary.total_trades >= min_trades {
            passed_summaries.push(summary);
        }
    }

    // Print results
    println!("{}", "=".repeat(90));
    println!("ALL RESULTS (sorted by directional accuracy)");
    println!("{}", "=".repeat(90));
    
    all_summaries.sort_by(|a, b| b.directional_accuracy.partial_cmp(&a.directional_accuracy).unwrap());
    
    for summary in &all_summaries {
        let passed = summary.directional_accuracy >= min_directional_accuracy && summary.total_trades >= min_trades;
        print_summary(summary, passed);
    }

    // Print passed strategies
    if !passed_summaries.is_empty() {
        println!("\n{}", "=".repeat(90));
        println!("STRATEGIES PASSING >52% DIRECTIONAL ACCURACY (min {} trades)", min_trades);
        println!("{}", "=".repeat(90));
        
        passed_summaries.sort_by(|a, b| b.directional_accuracy.partial_cmp(&a.directional_accuracy).unwrap());
        
        for summary in &passed_summaries {
            print_summary(summary, true);
        }
    } else {
        println!("\n{}", "=".repeat(90));
        println!("NO STRATEGIES PASSED THE >52% DIRECTIONAL ACCURACY GATE");
        println!("{}", "=".repeat(90));
    }

    // Summary stats
    let total_tested = all_summaries.len();
    let total_passed = passed_summaries.len();
    let avg_dir_acc = if total_tested > 0 {
        all_summaries.iter().map(|s| s.directional_accuracy).sum::<f64>() / total_tested as f64
    } else {
        0.0
    };

    println!("\n--- Summary ---");
    println!("Total tested: {}", total_tested);
    println!("Passed gate: {} ({:.1}%)", total_passed, total_passed as f64 / total_tested as f64 * 100.0);
    println!("Avg directional accuracy: {:.1}%", avg_dir_acc);

    Ok(())
}
