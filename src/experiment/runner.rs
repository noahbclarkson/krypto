//! Experiment runner - orchestrates the backtest loop.
//!
//! The runner coordinates:
//! 1. Loading configuration
//! 2. Fetching/preparing data
//! 3. Running validation splits
//! 4. Optimizing strategy parameters (if configured)
//! 5. Evaluating and comparing results
//! 6. Persisting outputs and manifest

use anyhow::{Result, Context, bail};
use std::path::PathBuf;
use tracing::{info, warn, debug};

use crate::config::{ExperimentConfig, RuntimeConfig, DataSplit};
use crate::backtest::{Backtester, BacktestResult};
use crate::experiment::{RunManifest, RunStatus, ResultsSummary, BacktestMetrics, OutputFile, OutputFileType};

/// Experiment runner orchestrates the full backtest workflow.
pub struct ExperimentRunner {
    config: ExperimentConfig,
    runtime: Option<RuntimeConfig>,
    manifest: RunManifest,
    output_dir: PathBuf,
}

impl ExperimentRunner {
    /// Create a new experiment runner from configuration.
    pub fn new(config: ExperimentConfig) -> Result<Self> {
        config.validate()?;
        
        let run_id = config.run_id();
        let output_dir = config.output.base_dir.join(&run_id);
        
        // Create output directory
        std::fs::create_dir_all(&output_dir)
            .with_context(|| format!("Failed to create output directory: {:?}", output_dir))?;
        
        // Serialize config for manifest
        let config_json = serde_json::to_value(&config)?;
        
        let manifest = RunManifest::new(run_id, config_json);
        
        Ok(Self {
            config,
            runtime: None,
            manifest,
            output_dir,
        })
    }
    
    /// Run the full experiment pipeline.
    pub fn run(&mut self) -> Result<ResultsSummary> {
        info!("Starting experiment: {}", self.config.name);
        info!("Output directory: {:?}", self.output_dir);
        
        // Phase 1: Load and prepare data
        let data = self.load_data()
            .context("Failed to load data")?;
        
        // Phase 2: Compute runtime config (splits)
        let total_candles = data.len();
        self.runtime = Some(RuntimeConfig::from_experiment(self.config.clone(), total_candles)?);
        let runtime = self.runtime.as_ref().unwrap();
        
        info!("Loaded {} candles, {} validation splits", 
              total_candles, runtime.splits.len());
        
        // Phase 3: Run backtests across all splits
        let mut all_train_results = Vec::new();
        let mut all_test_results = Vec::new();
        
        for split in &runtime.splits {
            let (train_result, test_result) = self.run_split(&data, split)?;
            all_train_results.push(train_result);
            all_test_results.push(test_result);
        }
        
        // Phase 4: Aggregate and evaluate results
        let summary = self.aggregate_results(&all_train_results, &all_test_results)?;
        
        // Phase 5: Save outputs
        self.save_outputs(&summary)?;
        
        // Phase 6: Complete manifest
        self.manifest.complete(summary.clone());
        self.save_manifest()?;
        
        info!("Experiment completed successfully");
        Ok(summary)
    }
    
    /// Load data for the experiment.
    fn load_data(&self) -> Result<Vec<f64>> {
        // TODO: Integrate with actual DataLoader
        // For now, return placeholder
        info!("Loading data from {} for symbols: {:?}", 
              self.config.data.source, self.config.data.symbols);
        
        // Placeholder: would use crate::data::DataLoader here
        // For minimal viable path, we expect data to be pre-loaded
        
        Ok(vec![0.0; 1000]) // Placeholder
    }
    
    /// Run backtest for a single train/test split.
    fn run_split(&self, _data: &[f64], split: &DataSplit) -> Result<(BacktestResult, BacktestResult)> {
        info!("Running split {}: train [{}, {}), test [{}, {})",
              split.index,
              split.train_range.0, split.train_range.1,
              split.test_range.0, split.test_range.1);
        
        // Create backtester with config
        let backtester = Backtester::new(
            self.config.sizing.initial_capital,
            self.config.costs.fee_pct,
            self.config.costs.slippage_bps,
        );
        
        // TODO: Get actual DataFrame and signals from strategy
        // For now, return placeholder results
        
        let train_result = BacktestResult {
            total_trades: 0,
            win_rate: 0.0,
            profit_factor: 0.0,
            final_equity: self.config.sizing.initial_capital,
            total_return_pct: 0.0,
            max_drawdown_pct: 0.0,
            sharpe_ratio: 0.0,
            kelly_fraction: 0.0,
            equity_curve: vec![],
            total_fees_paid: 0.0,
        };
        
        let test_result = train_result.clone();
        
        Ok((train_result, test_result))
    }
    
    /// Aggregate results across all splits.
    fn aggregate_results(
        &self,
        train_results: &[BacktestResult],
        test_results: &[BacktestResult],
    ) -> Result<ResultsSummary> {
        if train_results.is_empty() {
            bail!("No results to aggregate");
        }
        
        // Find best test result
        let best_idx = test_results
            .iter()
            .enumerate()
            .max_by(|(_, a), (_, b)| {
                a.sharpe_ratio.partial_cmp(&b.sharpe_ratio).unwrap()
            })
            .map(|(i, _)| i)
            .unwrap_or(0);
        
        let best = BacktestMetrics::from(test_results[best_idx].clone());
        
        // Compute averages
        let avg_trades: f64 = test_results.iter().map(|r| r.total_trades as f64).sum::<f64>() 
                              / test_results.len() as f64;
        let avg_win_rate: f64 = test_results.iter().map(|r| r.win_rate).sum::<f64>() 
                                / test_results.len() as f64;
        let avg_sharpe: f64 = test_results.iter().map(|r| r.sharpe_ratio).sum::<f64>() 
                              / test_results.len() as f64;
        let avg_return: f64 = test_results.iter().map(|r| r.total_return_pct).sum::<f64>() 
                              / test_results.len() as f64;
        let avg_dd: f64 = test_results.iter().map(|r| r.max_drawdown_pct).sum::<f64>() 
                          / test_results.len() as f64;
        
        let average = BacktestMetrics {
            total_trades: avg_trades as usize,
            win_rate: avg_win_rate,
            profit_factor: 0.0,
            total_return_pct: avg_return,
            max_drawdown_pct: avg_dd,
            sharpe_ratio: avg_sharpe,
            kelly_fraction: 0.0,
            total_fees_paid: 0.0,
        };
        
        // Find worst (by drawdown)
        let worst_idx = test_results
            .iter()
            .enumerate()
            .max_by(|(_, a), (_, b)| {
                a.max_drawdown_pct.partial_cmp(&b.max_drawdown_pct).unwrap()
            })
            .map(|(i, _)| i)
            .unwrap_or(0);
        
        let worst = BacktestMetrics::from(test_results[worst_idx].clone());
        
        // Compute robustness
        let train_sharpe = train_results.get(best_idx).map(|r| r.sharpe_ratio).unwrap_or(0.0);
        let test_sharpe = test_results.get(best_idx).map(|r| r.sharpe_ratio).unwrap_or(0.0);
        let robustness = if train_sharpe > 0.0 {
            Some(test_sharpe / train_sharpe)
        } else {
            None
        };
        
        Ok(ResultsSummary {
            best,
            average,
            worst,
            combinations_tested: train_results.len(),
            combinations_passed: train_results.iter().filter(|r| r.sharpe_ratio > 0.0).count(),
            train_metrics: train_results.get(best_idx).map(|r| BacktestMetrics::from(r.clone())),
            test_metrics: test_results.get(best_idx).map(|r| BacktestMetrics::from(r.clone())),
            robustness,
        })
    }
    
    /// Save experiment outputs.
    fn save_outputs(&mut self, summary: &ResultsSummary) -> Result<()> {
        // Save metrics JSON
        let metrics_path = self.output_dir.join("metrics.json");
        let metrics_json = serde_json::to_string_pretty(summary)?;
        std::fs::write(&metrics_path, &metrics_json)?;
        
        self.manifest.add_output(OutputFile {
            file_type: OutputFileType::Metrics,
            path: PathBuf::from("metrics.json"),
            size_bytes: metrics_json.len() as u64,
            description: "Aggregated backtest metrics".to_string(),
        });
        
        // Save equity curve if requested
        if self.config.output.save_equity_curve {
            let curve_path = self.output_dir.join("equity_curve.csv");
            // TODO: Save actual equity curve
            std::fs::write(&curve_path, "equity\n")?;
            
            self.manifest.add_output(OutputFile {
                file_type: OutputFileType::EquityCurve,
                path: PathBuf::from("equity_curve.csv"),
                size_bytes: 7,
                description: "Equity curve over time".to_string(),
            });
        }
        
        // Save config snapshot
        let config_path = self.output_dir.join("config.json");
        let config_json = serde_json::to_string_pretty(&self.config)?;
        std::fs::write(&config_path, &config_json)?;
        
        self.manifest.add_output(OutputFile {
            file_type: OutputFileType::Custom,
            path: PathBuf::from("config.json"),
            size_bytes: config_json.len() as u64,
            description: "Configuration snapshot".to_string(),
        });
        
        Ok(())
    }
    
    /// Save the run manifest.
    fn save_manifest(&self) -> Result<()> {
        let manifest_path = self.output_dir.join("manifest.json");
        self.manifest.save(&manifest_path)?;
        Ok(())
    }
    
    /// Get the output directory for this run.
    pub fn output_dir(&self) -> &PathBuf {
        &self.output_dir
    }
    
    /// Get the run manifest.
    pub fn manifest(&self) -> &RunManifest {
        &self.manifest
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// CLI Entry Point Support
// ─────────────────────────────────────────────────────────────────────────────

/// Run an experiment from a config file.
pub fn run_from_config(path: &PathBuf) -> Result<ResultsSummary> {
    let config = if path.extension().map(|e| e == "yaml" || e == "yml").unwrap_or(false) {
        ExperimentConfig::from_yaml(path)?
    } else {
        ExperimentConfig::from_json(path)?
    };
    
    let mut runner = ExperimentRunner::new(config)?;
    runner.run()
}

/// List all experiment runs in the output directory.
pub fn list_runs(base_dir: &PathBuf) -> Result<Vec<RunManifest>> {
    let mut manifests = Vec::new();
    
    if !base_dir.exists() {
        return Ok(manifests);
    }
    
    for entry in std::fs::read_dir(base_dir)? {
        let entry = entry?;
        let manifest_path = entry.path().join("manifest.json");
        
        if manifest_path.exists() {
            match RunManifest::load(&manifest_path) {
                Ok(m) => manifests.push(m),
                Err(e) => warn!("Failed to load manifest {:?}: {}", manifest_path, e),
            }
        }
    }
    
    // Sort by start time, newest first
    manifests.sort_by(|a, b| b.started_at.cmp(&a.started_at));
    
    Ok(manifests)
}

/// Compare two experiment runs.
pub fn compare_runs(manifest_a: &RunManifest, manifest_b: &RunManifest) -> String {
    let comparison = manifest_a.compare(manifest_b);
    
    let mut report = format!("Comparison: {} vs {}\n", comparison.run_a, comparison.run_b);
    report.push_str(&"=".repeat(60));
    report.push('\n');
    
    if let Some(sharpe) = comparison.sharpe_diff {
        report.push_str(&format!("Sharpe ratio diff: {:.4}\n", sharpe));
    }
    
    if let Some(ret) = comparison.return_diff {
        report.push_str(&format!("Return diff: {:.2}%\n", ret));
    }
    
    if let Some(dd) = comparison.drawdown_diff {
        report.push_str(&format!("Drawdown diff: {:.2}%\n", dd));
    }
    
    if !comparison.config_diff.is_empty() {
        report.push_str("\nConfig differences:\n");
        for diff in comparison.config_diff {
            report.push_str(&format!("  - {}\n", diff));
        }
    }
    
    report
}
