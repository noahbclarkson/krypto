//! Data loading and management module.
//!
//! Provides tools for fetching market data from exchanges and caching locally.

pub mod exchange_balance;
pub mod funding_rate;
pub mod loader;
pub mod open_interest;
pub mod universe;

// Re-export main types for convenience
pub use exchange_balance::{ExchangeBalanceLoader, ExchangeBalancePoint};
pub use funding_rate::{
    align_to_ohlcv as align_funding_to_ohlcv, compute_funding_features, FundingRateLoader,
};
pub use loader::{CacheConfig, DataLoader};
pub use open_interest::{
    align_to_ohlcv as align_open_interest_to_ohlcv, compute_open_interest_features,
    OpenInterestLoader, OpenInterestPeriod,
};
