//! Runtime configuration derived from ExperimentConfig.
//!
//! Runtime config contains computed values and resolved parameters
//! that are used during experiment execution.

use super::{ExperimentConfig, ValidationConfig};
use anyhow::Result;
use chrono::{DateTime, Utc};

/// Runtime configuration computed from experiment config.
#[derive(Debug, Clone)]
pub struct RuntimeConfig {
    /// Original experiment config
    pub experiment: ExperimentConfig,
    
    /// Resolved start datetime
    pub start_time: DateTime<Utc>,
    
    /// Resolved end datetime
    pub end_time: DateTime<Utc>,
    
    /// Computed train/test split indices
    pub splits: Vec<DataSplit>,
    
    /// Run timestamp
    pub run_timestamp: DateTime<Utc>,
}

/// A train/test data split for validation.
#[derive(Debug, Clone)]
pub struct DataSplit {
    /// Split index (0-based)
    pub index: usize,
    
    /// Train range: (start_idx, end_idx) exclusive
    pub train_range: (usize, usize),
    
    /// Test range: (start_idx, end_idx) exclusive
    pub test_range: (usize, usize),
    
    /// Purge gap indices (excluded from both)
    pub purge_range: Option<(usize, usize)>,
}

impl RuntimeConfig {
    /// Build runtime config from experiment config.
    pub fn from_experiment(config: ExperimentConfig, total_candles: usize) -> Result<Self> {
        let splits = compute_splits(&config.validation, total_candles)?;
        
        Ok(Self {
            start_time: chrono::Utc::now(),
            end_time: chrono::Utc::now(),
            experiment: config,
            splits,
            run_timestamp: chrono::Utc::now(),
        })
    }
}

/// Compute train/test splits based on validation method.
fn compute_splits(config: &ValidationConfig, total_candles: usize) -> Result<Vec<DataSplit>> {
    match config.method.as_str() {
        "simple_split" => {
            let train_end = (total_candles as f64 * config.train_ratio) as usize;
            let test_start = train_end + config.purge_gap;
            
            Ok(vec![DataSplit {
                index: 0,
                train_range: (0, train_end),
                test_range: (test_start, total_candles),
                purge_range: if config.purge_gap > 0 {
                    Some((train_end, test_start))
                } else {
                    None
                },
            }])
        }
        
        "walk_forward" => {
            // Walk-forward: rolling windows where train set grows
            let n = config.n_windows;
            let window_size = total_candles / (n + 1);
            let mut splits = Vec::with_capacity(n);
            
            for i in 0..n {
                let train_end = window_size * (i + 1);
                let test_start = train_end + config.purge_gap;
                let test_end = std::cmp::min(test_start + window_size, total_candles);
                
                splits.push(DataSplit {
                    index: i,
                    train_range: (0, train_end),
                    test_range: (test_start, test_end),
                    purge_range: if config.purge_gap > 0 {
                        Some((train_end, test_start))
                    } else {
                        None
                    },
                });
            }
            
            Ok(splits)
        }
        
        "cpcv" => {
            // Combinatorial Purged Cross-Validation
            // For now, fall back to walk-forward (to be enhanced)
            let n = config.n_windows.max(5);
            let window_size = total_candles / n;
            let mut splits = Vec::with_capacity(n);
            
            for i in 0..n {
                // In CPCV, each split uses all data except one test window
                let test_start = i * window_size;
                let test_end = std::cmp::min((i + 1) * window_size, total_candles);
                
                splits.push(DataSplit {
                    index: i,
                    train_range: (0, test_start), // Simplified: before test
                    test_range: (test_start, test_end),
                    purge_range: if config.purge_gap > 0 {
                        Some((
                            test_start.saturating_sub(config.purge_gap),
                            test_start,
                        ))
                    } else {
                        None
                    },
                });
            }
            
            Ok(splits)
        }
        
        other => anyhow::bail!("Unknown validation method: {}", other),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_simple_split() {
        let config = ValidationConfig {
            method: "simple_split".to_string(),
            train_ratio: 0.6,
            test_ratio: 0.4,
            ..Default::default()
        };
        
        let splits = compute_splits(&config, 1000).unwrap();
        assert_eq!(splits.len(), 1);
        assert_eq!(splits[0].train_range, (0, 600));
        assert_eq!(splits[0].test_range, (600, 1000));
    }
    
    #[test]
    fn test_walk_forward() {
        let config = ValidationConfig {
            method: "walk_forward".to_string(),
            n_windows: 5,
            ..Default::default()
        };
        
        let splits = compute_splits(&config, 1000).unwrap();
        assert_eq!(splits.len(), 5);
        
        // First window: train on first ~166 candles, test on next ~166
        assert!(splits[0].train_range.1 < splits[0].test_range.0);
    }
}
