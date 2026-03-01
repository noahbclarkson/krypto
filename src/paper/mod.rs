//! Paper trading bot for strategy validation.
//!
//! This module provides a simple, no-exchange paper trading system for
//! validating trading strategies before live deployment.
//!
//! # Example
//!
//! ```rust,no_run
//! use krypto::paper::{PaperBot, Bar, Strategy, Trade, BotSummary};
//!
//! // Implement a simple strategy
//! struct MyStrategy;
//!
//! impl Strategy for MyStrategy {
//!     fn name(&self) -> &str { "MyStrategy" }
//!     
//!     fn on_bar(&mut self, bar: &Bar, position: f64) -> Option<Trade> {
//!         if bar.close > bar.open && position == 0.0 {
//!             Some(Trade::Long { size: 1.0 })
//!         } else if bar.close < bar.open && position > 0.0 {
//!             Some(Trade::Close)
//!         } else {
//!             None
//!         }
//!     }
//! }
//!
//! // Run the bot
//! let strategy = Box::new(MyStrategy);
//! let mut bot = PaperBot::new(strategy, 10_000.0);
//!
//! // Process bars...
//! // bot.on_bar(&bar);
//!
//! // Get results
//! let summary = bot.summary();
//! println!("Win rate: {:.1}%", summary.win_rate);
//! ```

mod bot;

pub use bot::{PaperBot, BotSummary, Trade, Position};
pub use bot::{Bar, Strategy};
