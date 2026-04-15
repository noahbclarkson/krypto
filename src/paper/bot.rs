//! Paper trading bot implementation.
//!
//! Provides bar-by-bar processing for strategy validation without
//! any exchange connection.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;

/// A single OHLCV bar.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Bar {
    pub time: DateTime<Utc>,
    pub open: f64,
    pub high: f64,
    pub low: f64,
    pub close: f64,
    pub volume: f64,
}

impl Bar {
    /// Create a new bar.
    pub fn new(
        time: DateTime<Utc>,
        open: f64,
        high: f64,
        low: f64,
        close: f64,
        volume: f64,
    ) -> Self {
        Self {
            time,
            open,
            high,
            low,
            close,
            volume,
        }
    }

    /// Calculate the bar's range as a percentage.
    pub fn range_pct(&self) -> f64 {
        if self.close > 0.0 {
            (self.high - self.low) / self.close * 100.0
        } else {
            0.0
        }
    }

    /// Returns true if this is a bullish bar (close > open).
    pub fn is_bullish(&self) -> bool {
        self.close > self.open
    }

    /// Returns the body size as a percentage.
    pub fn body_pct(&self) -> f64 {
        if self.close > 0.0 {
            (self.close - self.open).abs() / self.close * 100.0
        } else {
            0.0
        }
    }
}

/// Trade signal returned by a strategy.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Trade {
    /// Open a long position with the given size fraction (0.0 to 1.0).
    Long { size: f64 },
    /// Open a short position with the given size fraction (0.0 to 1.0).
    Short { size: f64 },
    /// Close the current position.
    Close,
    /// Reverse from long to short or vice versa.
    Reverse,
}

/// Current position state.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub enum Position {
    #[default]
    Flat,
    Long {
        entry_price: f64,
        size: f64,
        highest: f64,
    },
    Short {
        entry_price: f64,
        size: f64,
        lowest: f64,
    },
}

impl Position {
    /// Returns the signed position size (-1.0 to 1.0).
    pub fn signed_size(&self) -> f64 {
        match self {
            Position::Flat => 0.0,
            Position::Long { size, .. } => *size,
            Position::Short { size, .. } => -*size,
        }
    }

    /// Returns the entry price if in a position, otherwise 0.0.
    pub fn entry_price(&self) -> f64 {
        match self {
            Position::Flat => 0.0,
            Position::Long { entry_price, .. } => *entry_price,
            Position::Short { entry_price, .. } => *entry_price,
        }
    }

    /// Returns true if the position is flat (no open trade).
    pub fn is_flat(&self) -> bool {
        matches!(self, Position::Flat)
    }

    /// Returns true if the position is long.
    pub fn is_long(&self) -> bool {
        matches!(self, Position::Long { .. })
    }

    /// Returns true if the position is short.
    pub fn is_short(&self) -> bool {
        matches!(self, Position::Short { .. })
    }
}

/// A completed trade record.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompletedTrade {
    pub entry_time: DateTime<Utc>,
    pub exit_time: DateTime<Utc>,
    pub entry_price: f64,
    pub exit_price: f64,
    pub size: f64,
    pub is_long: bool,
    pub pnl: f64,
    pub pnl_pct: f64,
}

impl CompletedTrade {
    /// Returns true if the trade was profitable.
    pub fn is_winner(&self) -> bool {
        self.pnl > 0.0
    }
}

/// Strategy trait for bar-by-bar processing.
///
/// Implement this trait to create a trading strategy that can be
/// validated with the paper trading bot.
pub trait Strategy: Send + Sync {
    /// Returns the strategy name for logging.
    fn name(&self) -> &str;

    /// Process a bar and optionally return a trade signal.
    ///
    /// # Arguments
    /// * `bar` - The current bar
    /// * `position` - Current signed position size (-1.0 to 1.0)
    /// * `history` - Recent bars for indicator calculation (up to last 50)
    ///
    /// # Returns
    /// * `Some(Trade)` if a trade should be executed
    /// * `None` if no action needed
    fn on_bar(&mut self, bar: &Bar, position: f64, history: &[Bar]) -> Option<Trade>;

    /// Optional: Called when the bot is reset.
    fn reset(&mut self) {}
}

/// Summary statistics for the paper trading bot.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BotSummary {
    /// Strategy name
    pub strategy: String,
    /// Initial capital
    pub initial_capital: f64,
    /// Final equity
    pub final_equity: f64,
    /// Total PnL in currency units
    pub total_pnl: f64,
    /// Total return as percentage
    pub total_return_pct: f64,
    /// Number of completed trades
    pub total_trades: usize,
    /// Number of winning trades
    pub wins: usize,
    /// Number of losing trades
    pub losses: usize,
    /// Win rate as percentage
    pub win_rate: f64,
    /// Profit factor (gross profit / gross loss)
    pub profit_factor: f64,
    /// Maximum drawdown as percentage
    pub max_drawdown_pct: f64,
    /// Average win percentage
    pub avg_win_pct: f64,
    /// Average loss percentage
    pub avg_loss_pct: f64,
    /// Largest win percentage
    pub largest_win_pct: f64,
    /// Largest loss percentage
    pub largest_loss_pct: f64,
    /// Total fees paid
    pub total_fees: f64,
    /// Directional accuracy (correct predictions / total trades)
    pub directional_accuracy: f64,
}

impl BotSummary {
    /// Returns true if the strategy meets the minimum win rate target.
    pub fn meets_target(&self, min_win_rate: f64) -> bool {
        self.win_rate >= min_win_rate
    }
}

/// Paper trading bot for strategy validation.
pub struct PaperBot {
    strategy: Box<dyn Strategy>,
    initial_capital: f64,
    equity: f64,
    position: Position,
    history: VecDeque<Bar>,
    trades: Vec<CompletedTrade>,
    peak_equity: f64,
    max_drawdown: f64,
    fee_pct: f64,
    trailing_stop_pct: f64,
    // Trade tracking
    gross_profit: f64,
    gross_loss: f64,
    largest_win_pct: f64,
    largest_loss_pct: f64,
}

impl PaperBot {
    /// Create a new paper trading bot.
    ///
    /// # Arguments
    /// * `strategy` - The trading strategy to use
    /// * `initial_capital` - Starting capital
    pub fn new(strategy: Box<dyn Strategy>, initial_capital: f64) -> Self {
        Self {
            strategy,
            initial_capital,
            equity: initial_capital,
            position: Position::Flat,
            history: VecDeque::with_capacity(100),
            trades: Vec::new(),
            peak_equity: initial_capital,
            max_drawdown: 0.0,
            fee_pct: 0.001,         // 0.1% default fee
            trailing_stop_pct: 0.0, // disabled by default
            gross_profit: 0.0,
            gross_loss: 0.0,
            largest_win_pct: 0.0,
            largest_loss_pct: 0.0,
        }
    }

    /// Set the fee percentage per trade (default: 0.001 = 0.1%).
    pub fn with_fee(mut self, fee_pct: f64) -> Self {
        self.fee_pct = fee_pct;
        self
    }

    /// Set trailing stop percentage (default: 0.0 = disabled).
    pub fn with_trailing_stop(mut self, pct: f64) -> Self {
        self.trailing_stop_pct = pct;
        self
    }

    /// Process a bar and execute any trade signal.
    ///
    /// Returns the trade that was executed, if any.
    pub fn on_bar(&mut self, bar: &Bar) -> Option<CompletedTrade> {
        // Update history
        self.history.push_back(bar.clone());
        if self.history.len() > 100 {
            self.history.pop_front();
        }

        // Check trailing stop first
        if let Some(trade) = self.check_trailing_stop(bar) {
            return Some(trade);
        }

        // Get strategy signal
        let position_size = self.position.signed_size();
        let history: Vec<Bar> = self.history.iter().cloned().collect();
        let signal = self.strategy.on_bar(bar, position_size, &history);

        // Execute signal
        signal.and_then(|trade| self.execute_trade(bar, trade))
    }

    /// Check if trailing stop was hit.
    fn check_trailing_stop(&mut self, bar: &Bar) -> Option<CompletedTrade> {
        if self.trailing_stop_pct <= 0.0 {
            return None;
        }

        match &mut self.position {
            Position::Long { highest, .. } => {
                if bar.high > *highest {
                    *highest = bar.high;
                }
                let stop_price = *highest * (1.0 - self.trailing_stop_pct);
                if bar.low <= stop_price {
                    let exit_price = stop_price;
                    return Some(self.close_position(exit_price, bar.time));
                }
            }
            Position::Short { lowest, .. } => {
                if bar.low < *lowest {
                    *lowest = bar.low;
                }
                let stop_price = *lowest * (1.0 + self.trailing_stop_pct);
                if bar.high >= stop_price {
                    let exit_price = stop_price;
                    return Some(self.close_position(exit_price, bar.time));
                }
            }
            Position::Flat => {}
        }

        None
    }

    /// Execute a trade signal.
    fn execute_trade(&mut self, bar: &Bar, trade: Trade) -> Option<CompletedTrade> {
        match trade {
            Trade::Long { size } => {
                match self.position {
                    Position::Flat => {
                        self.open_long(bar.close, size.clamp(0.0, 1.0));
                        None
                    }
                    Position::Short { .. } => {
                        let closed = self.close_position(bar.close, bar.time);
                        self.open_long(bar.close, size.clamp(0.0, 1.0));
                        Some(closed)
                    }
                    Position::Long { .. } => None, // Already long
                }
            }
            Trade::Short { size } => {
                match self.position {
                    Position::Flat => {
                        self.open_short(bar.close, size.clamp(0.0, 1.0));
                        None
                    }
                    Position::Long { .. } => {
                        let closed = self.close_position(bar.close, bar.time);
                        self.open_short(bar.close, size.clamp(0.0, 1.0));
                        Some(closed)
                    }
                    Position::Short { .. } => None, // Already short
                }
            }
            Trade::Close => match self.position {
                Position::Flat => None,
                _ => Some(self.close_position(bar.close, bar.time)),
            },
            Trade::Reverse => match self.position {
                Position::Flat => None,
                Position::Long { size, .. } => {
                    let closed = self.close_position(bar.close, bar.time);
                    self.open_short(bar.close, size);
                    Some(closed)
                }
                Position::Short { size, .. } => {
                    let closed = self.close_position(bar.close, bar.time);
                    self.open_long(bar.close, size);
                    Some(closed)
                }
            },
        }
    }

    /// Open a long position.
    fn open_long(&mut self, price: f64, size: f64) {
        // Deduct entry fee
        let notional = self.equity * size;
        let fee = notional * self.fee_pct;
        self.equity -= fee;

        self.position = Position::Long {
            entry_price: price,
            size,
            highest: price,
        };
    }

    /// Open a short position.
    fn open_short(&mut self, price: f64, size: f64) {
        // Deduct entry fee
        let notional = self.equity * size;
        let fee = notional * self.fee_pct;
        self.equity -= fee;

        self.position = Position::Short {
            entry_price: price,
            size,
            lowest: price,
        };
    }

    /// Close the current position and record the trade.
    fn close_position(&mut self, exit_price: f64, exit_time: DateTime<Utc>) -> CompletedTrade {
        let (entry_price, size, is_long, entry_time) = match &self.position {
            // All call sites guard with `Position::Flat => None` before calling this
            Position::Flat => unreachable!("close_position called on flat position"),
            Position::Long {
                entry_price, size, ..
            } => (
                *entry_price,
                *size,
                true,
                self.history.back().map(|b| b.time).unwrap_or_default(),
            ),
            Position::Short {
                entry_price, size, ..
            } => (
                *entry_price,
                *size,
                false,
                self.history.back().map(|b| b.time).unwrap_or_default(),
            ),
        };

        // Calculate PnL
        let pnl_pct = if is_long {
            (exit_price - entry_price) / entry_price
        } else {
            (entry_price - exit_price) / entry_price
        };

        let notional = self.equity * size;
        let pnl = notional * pnl_pct;

        // Deduct exit fee
        let fee = notional * self.fee_pct;
        let net_pnl = pnl - fee;

        self.equity += net_pnl;

        // Track peak and drawdown
        if self.equity > self.peak_equity {
            self.peak_equity = self.equity;
        }
        let dd = (self.peak_equity - self.equity) / self.peak_equity;
        if dd > self.max_drawdown {
            self.max_drawdown = dd;
        }

        // Track profit/loss stats
        if net_pnl > 0.0 {
            self.gross_profit += net_pnl;
            if pnl_pct > self.largest_win_pct {
                self.largest_win_pct = pnl_pct;
            }
        } else {
            self.gross_loss += net_pnl.abs();
            if pnl_pct.abs() > self.largest_loss_pct {
                self.largest_loss_pct = pnl_pct.abs();
            }
        }

        // Record trade
        let trade = CompletedTrade {
            entry_time,
            exit_time,
            entry_price,
            exit_price,
            size,
            is_long,
            pnl: net_pnl,
            pnl_pct: pnl_pct * 100.0,
        };

        self.trades.push(trade.clone());
        self.position = Position::Flat;

        trade
    }

    /// Get the current position.
    pub fn position(&self) -> &Position {
        &self.position
    }

    /// Get current equity (mark-to-market).
    pub fn equity(&self) -> f64 {
        self.equity
    }

    /// Get the list of completed trades.
    pub fn trades(&self) -> &[CompletedTrade] {
        &self.trades
    }

    /// Generate a summary of the bot's performance.
    pub fn summary(&self) -> BotSummary {
        let wins = self.trades.iter().filter(|t| t.is_winner()).count();
        let losses = self.trades.len() - wins;
        let total_trades = self.trades.len();

        let win_rate = if total_trades > 0 {
            wins as f64 / total_trades as f64 * 100.0
        } else {
            0.0
        };

        let profit_factor = if self.gross_loss > 0.0 {
            self.gross_profit / self.gross_loss
        } else if self.gross_profit > 0.0 {
            f64::INFINITY
        } else {
            0.0
        };

        let avg_win_pct = if wins > 0 {
            self.trades
                .iter()
                .filter(|t| t.is_winner())
                .map(|t| t.pnl_pct)
                .sum::<f64>()
                / wins as f64
        } else {
            0.0
        };

        let avg_loss_pct = if losses > 0 {
            self.trades
                .iter()
                .filter(|t| !t.is_winner())
                .map(|t| t.pnl_pct.abs())
                .sum::<f64>()
                / losses as f64
        } else {
            0.0
        };

        let total_pnl = self.equity - self.initial_capital;
        let total_return_pct = (self.equity / self.initial_capital - 1.0) * 100.0;

        // Calculate directional accuracy
        // A trade is directionally correct if:
        // - Long trade and price went up (pnl_pct > 0)
        // - Short trade and price went down (pnl_pct > 0)
        let directionally_correct = self.trades.iter().filter(|t| t.pnl > 0.0).count();
        let directional_accuracy = if total_trades > 0 {
            directionally_correct as f64 / total_trades as f64 * 100.0
        } else {
            0.0
        };

        let total_fees = self.trades.len() as f64 * 2.0 * self.fee_pct * self.initial_capital;

        BotSummary {
            strategy: self.strategy.name().to_string(),
            initial_capital: self.initial_capital,
            final_equity: self.equity,
            total_pnl,
            total_return_pct,
            total_trades,
            wins,
            losses,
            win_rate,
            profit_factor,
            max_drawdown_pct: self.max_drawdown * 100.0,
            avg_win_pct,
            avg_loss_pct,
            largest_win_pct: self.largest_win_pct * 100.0,
            largest_loss_pct: self.largest_loss_pct * 100.0,
            total_fees,
            directional_accuracy,
        }
    }

    /// Reset the bot to initial state.
    pub fn reset(&mut self) {
        self.equity = self.initial_capital;
        self.position = Position::Flat;
        self.history.clear();
        self.trades.clear();
        self.peak_equity = self.initial_capital;
        self.max_drawdown = 0.0;
        self.gross_profit = 0.0;
        self.gross_loss = 0.0;
        self.largest_win_pct = 0.0;
        self.largest_loss_pct = 0.0;
        self.strategy.reset();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct TestStrategy;

    impl Strategy for TestStrategy {
        fn name(&self) -> &str {
            "Test"
        }

        fn on_bar(&mut self, bar: &Bar, position: f64, _history: &[Bar]) -> Option<Trade> {
            if bar.is_bullish() && position == 0.0 {
                Some(Trade::Long { size: 1.0 })
            } else if !bar.is_bullish() && position > 0.0 {
                Some(Trade::Close)
            } else {
                None
            }
        }
    }

    fn make_bar(close: f64, open: f64) -> Bar {
        Bar::new(
            Utc::now(),
            open,
            close.max(open),
            close.min(open),
            close,
            1000.0,
        )
    }

    #[test]
    fn test_bot_basic_flow() {
        let strategy = Box::new(TestStrategy);
        let mut bot = PaperBot::new(strategy, 10_000.0);

        // Bullish bar - should enter long
        let bar1 = make_bar(100.0, 99.0);
        bot.on_bar(&bar1);
        assert!(matches!(bot.position(), Position::Long { .. }));

        // Another bullish bar - stay long
        let bar2 = make_bar(101.0, 100.0);
        bot.on_bar(&bar2);
        assert!(matches!(bot.position(), Position::Long { .. }));

        // Bearish bar - should close
        let bar3 = make_bar(100.0, 101.0);
        bot.on_bar(&bar3);
        assert!(matches!(bot.position(), Position::Flat));

        let summary = bot.summary();
        assert_eq!(summary.total_trades, 1);
    }

    #[test]
    fn test_summary_calculations() {
        let strategy = Box::new(TestStrategy);
        let mut bot = PaperBot::new(strategy, 10_000.0).with_fee(0.0);

        // Series of bars that will trigger trades
        let bars = vec![
            make_bar(100.0, 99.0), // Bullish -> enter long @ 100
            make_bar(99.0, 100.0), // Bearish -> close @ 99 (loss)
            make_bar(98.0, 97.0),  // Bullish -> enter long @ 98
            make_bar(100.0, 99.0), // Bullish -> stay long
            make_bar(99.0, 100.0), // Bearish -> close @ 99 (loss)
        ];

        for bar in &bars {
            bot.on_bar(bar);
        }

        let summary = bot.summary();
        assert_eq!(summary.total_trades, 2);
        // Trade 1: buy@100, sell@99 → loss
        // Trade 2: buy@98, sell@99 → win (98→99 is a gain)
        assert_eq!(summary.losses, 1);
        assert_eq!(summary.wins, 1);
    }

    #[test]
    fn test_directional_accuracy() {
        let strategy = Box::new(TestStrategy);
        let mut bot = PaperBot::new(strategy, 10_000.0).with_fee(0.0);

        // Create a winning trade
        let bars = vec![
            make_bar(100.0, 99.0),  // Bullish -> enter long @ 100
            make_bar(102.0, 101.0), // Bullish -> stay long
            make_bar(101.0, 102.0), // Bearish -> close @ 101 (win)
        ];

        for bar in &bars {
            bot.on_bar(bar);
        }

        let summary = bot.summary();
        assert_eq!(summary.total_trades, 1);
        assert_eq!(summary.wins, 1);
        assert!(summary.directional_accuracy > 0.0);
    }

    #[test]
    fn test_bar_range_pct() {
        let bar = Bar::new(Utc::now(), 100.0, 102.0, 98.0, 100.0, 1000.0);
        // Range = (102 - 98) / 100 * 100 = 4%
        assert!((bar.range_pct() - 4.0).abs() < 0.01);
    }

    #[test]
    fn test_bar_body_pct() {
        let bar = Bar::new(Utc::now(), 100.0, 102.0, 98.0, 105.0, 1000.0);
        // Body = (105 - 100) / 105 * 100 = 4.76%
        assert!((bar.body_pct() - 4.76).abs() < 0.1);
    }

    #[test]
    fn test_bar_is_bullish() {
        let bullish = Bar::new(Utc::now(), 100.0, 102.0, 98.0, 101.0, 1000.0);
        let bearish = Bar::new(Utc::now(), 101.0, 102.0, 98.0, 100.0, 1000.0);
        let doji = Bar::new(Utc::now(), 100.0, 102.0, 98.0, 100.0, 1000.0);

        assert!(bullish.is_bullish());
        assert!(!bearish.is_bullish());
        assert!(!doji.is_bullish()); // close == open is not bullish
    }

    #[test]
    fn test_position_signed_size() {
        let flat = Position::Flat;
        let long = Position::Long {
            entry_price: 100.0,
            size: 0.5,
            highest: 105.0,
        };
        let short = Position::Short {
            entry_price: 100.0,
            size: 0.5,
            lowest: 95.0,
        };

        assert_eq!(flat.signed_size(), 0.0);
        assert_eq!(long.signed_size(), 0.5);
        assert_eq!(short.signed_size(), -0.5);
    }

    #[test]
    fn test_position_entry_price() {
        let flat = Position::Flat;
        let long = Position::Long {
            entry_price: 100.0,
            size: 1.0,
            highest: 105.0,
        };

        assert_eq!(flat.entry_price(), 0.0);
        assert_eq!(long.entry_price(), 100.0);
    }

    #[test]
    fn test_completed_trade_is_winner() {
        let win = CompletedTrade {
            entry_time: Utc::now(),
            exit_time: Utc::now(),
            entry_price: 100.0,
            exit_price: 105.0,
            size: 1.0,
            is_long: true,
            pnl: 50.0,
            pnl_pct: 5.0,
        };

        let loss = CompletedTrade {
            pnl: -20.0,
            ..win.clone()
        };

        assert!(win.is_winner());
        assert!(!loss.is_winner());
    }

    #[test]
    fn test_bot_with_fee() {
        let strategy = Box::new(TestStrategy);
        let bot = PaperBot::new(strategy, 10_000.0).with_fee(0.002); // 0.2%

        assert_eq!(bot.fee_pct, 0.002);
    }

    #[test]
    fn test_bot_with_trailing_stop() {
        let strategy = Box::new(TestStrategy);
        let bot = PaperBot::new(strategy, 10_000.0).with_trailing_stop(0.05);

        assert_eq!(bot.trailing_stop_pct, 0.05);
    }

    #[test]
    fn test_bot_reset() {
        let strategy = Box::new(TestStrategy);
        let mut bot = PaperBot::new(strategy, 10_000.0);

        // Enter a position
        let bar = make_bar(100.0, 99.0);
        bot.on_bar(&bar);
        assert!(matches!(bot.position(), Position::Long { .. }));

        // Reset
        bot.reset();
        assert!(matches!(bot.position(), Position::Flat));
        assert_eq!(bot.equity(), 10_000.0);
        assert!(bot.trades().is_empty());
    }

    #[test]
    fn test_summary_meets_target() {
        let summary = BotSummary {
            strategy: "Test".to_string(),
            initial_capital: 10_000.0,
            final_equity: 11_000.0,
            total_pnl: 1_000.0,
            total_return_pct: 10.0,
            total_trades: 10,
            wins: 6,
            losses: 4,
            win_rate: 60.0,
            profit_factor: 1.5,
            max_drawdown_pct: 5.0,
            avg_win_pct: 3.0,
            avg_loss_pct: 2.0,
            largest_win_pct: 5.0,
            largest_loss_pct: 3.0,
            total_fees: 20.0,
            directional_accuracy: 55.0,
        };

        assert!(summary.meets_target(50.0)); // 60% > 50%
        assert!(!summary.meets_target(70.0)); // 60% < 70%
    }

    #[test]
    fn test_trade_reverse() {
        struct ReverseStrategy;

        impl Strategy for ReverseStrategy {
            fn name(&self) -> &str {
                "Reverse"
            }

            fn on_bar(&mut self, _bar: &Bar, position: f64, _history: &[Bar]) -> Option<Trade> {
                if position == 0.0 {
                    Some(Trade::Long { size: 1.0 })
                } else if position > 0.0 {
                    Some(Trade::Reverse)
                } else {
                    Some(Trade::Close)
                }
            }
        }

        let strategy = Box::new(ReverseStrategy);
        let mut bot = PaperBot::new(strategy, 10_000.0).with_fee(0.0);

        // Enter long
        let bar1 = make_bar(100.0, 99.0);
        bot.on_bar(&bar1);
        assert!(matches!(bot.position(), Position::Long { .. }));

        // Reverse to short
        let bar2 = make_bar(101.0, 100.0);
        bot.on_bar(&bar2);
        assert!(matches!(bot.position(), Position::Short { .. }));

        // Should have one completed trade
        assert_eq!(bot.trades().len(), 1);
    }
}
