//! Data loading and management module.
//!
//! Provides tools for fetching market data from exchanges and caching locally.

pub mod funding_rate;
pub mod loader;
pub mod universe;

// Re-export main types for convenience
pub use loader::{CacheConfig, DataLoader};
pub use funding_rate::{FundingRateLoader, FundingRateRecord, FundingRateStats};
