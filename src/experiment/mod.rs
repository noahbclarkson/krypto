//! Experiment orchestration module.
//!
//! This module provides:
//! - `RunManifest`: Metadata and results summary for reproducibility
//! - `ExperimentRunner`: Orchestrates data loading, backtesting, and evaluation
//! - Result persistence and comparison utilities

pub mod manifest;
pub mod runner;

pub use manifest::{
    BacktestMetrics, OutputFile, OutputFileType, ResultsSummary, RunManifest, RunStatus,
};
pub use runner::{compare_runs, list_runs, ExperimentRunner};
