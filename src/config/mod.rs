//! Configuration module for experiment-driven backtesting.
//!
//! This module defines the schema for reproducible, config-driven backtests.
//! Every experiment is defined by an `ExperimentConfig` that specifies:
//! - Data source and timeframe
//! - Transaction costs (fees + slippage)
//! - Position sizing rules
//! - Walk-forward validation windows
//! - Evaluation metrics
//! - Strategy parameters

pub mod experiment;
pub mod runtime;

pub use experiment::*;
pub use runtime::*;
