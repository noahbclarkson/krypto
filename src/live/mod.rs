//! Live trading infrastructure.
//!
//! This module provides real-time trading capabilities:
//! - WebSocket data feeds for live market data
//! - Order execution via Binance Futures API
//! - Live bot that processes signals and manages positions
//!
//! # Architecture
//!
//! ```text
//! ┌─────────────┐     ┌──────────────┐     ┌─────────────┐
//! │  WebSocket  │────▶│   LiveBot    │────▶│  Executor   │
//! │  Data Feed  │     │  (strategy)  │     │  (orders)   │
//! └─────────────┘     └──────────────┘     └─────────────┘
//! ```
//!
//! # Safety
//!
//! - All orders use test endpoint by default
//! - Position size limits enforced
//! - Graceful shutdown on disconnect

pub mod bot;
pub mod config;
pub mod executor;
pub mod feed;
pub mod mock_exchange;

pub use bot::{BotState, LiveBot};
pub use config::LiveConfig;
pub use executor::{Executor, OrderResult, PositionInfo};
pub use feed::{KlineEvent, LiveFeed};
pub use mock_exchange::{Bar, MockExchange, MockExchangeConfig, MockFill, MockOrder, MockOrderStatus, MockOrderType, MockPosition, MockSide};
