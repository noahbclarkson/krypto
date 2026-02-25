//! Experiment orchestration module.
//!
//! This module provides:
//! - `RunManifest`: Metadata and results summary for reproducibility
//! - `ExperimentRunner`: Orchestrates data loading, backtesting, and evaluation
//! - Result persistence and comparison utilities

pub mod manifest;
pub mod runner;

pub use manifest::{RunManifest, RunStatus, ResultsSummary, BacktestMetrics, OutputFile, OutputFileType};
pub use runner::{ExperimentRunner, list_runs, compare_runs};
