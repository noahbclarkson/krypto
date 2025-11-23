pub mod ensemble;
pub mod optimization;
pub mod regime;
pub mod strategies;

use anyhow::Result;
use polars::prelude::*;

/// The interface all sub-models must implement
pub trait SignalGenerator: Send + Sync {
    /// Name for logging
    fn name(&self) -> &str;

    /// Train the model (optional, some are heuristic)
    fn train(&mut self, features: &DataFrame, labels: &Series) -> Result<()>;

    /// Returns signal: -1.0 (Short) to 1.0 (Long)
    fn predict(&self, features: &DataFrame) -> Result<Series>;

    /// Returns a text explanation for the signal at every step.
    /// Useful for UI transparency ("Why did we buy?").
    fn explain(&self, features: &DataFrame) -> Result<Vec<String>>;
}
