//! Experiment runner - orchestrates the backtest loop.
//!
//! The runner coordinates:
//! 1. Loading configuration
//! 2. Fetching/preparing data
//! 3. Running validation splits
//! 4. Optimizing strategy parameters (if configured)
//! 5. Evaluating and comparing results
//! 6. Persisting outputs and manifest

use anyhow::{bail, Context, Result};
use chrono::{DateTime, NaiveDate, Utc};
use std::path::PathBuf;
use tracing::{info, warn};

use crate::backtest::engine::{BacktestResult, Backtester};
use crate::config::{DataSplit, ExperimentConfig, RuntimeConfig};
use crate::data::loader::DataLoader;
use crate::experiment::manifest::{OutputFile, OutputFileType};
use crate::experiment::{BacktestMetrics, ResultsSummary, RunManifest};

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

        let symbols = self.config.data.symbols.clone();
        info!(
            "Running experiment for {} symbol(s): {:?}",
            symbols.len(),
            symbols
        );

        // Collect results across all symbols
        let mut all_train_results = Vec::new();
        let mut all_test_results = Vec::new();

        // Iterate over ALL configured symbols
        for symbol in &symbols {
            info!("Processing symbol: {}", symbol);

            // Phase 1: Load and prepare data for this symbol
            let data = self
                .load_data_for_symbol(symbol)
                .with_context(|| format!("Failed to load data for symbol {}", symbol))?;

            // Phase 2: Compute runtime config (splits) based on this symbol's data
            let total_candles = data.len();
            let runtime = RuntimeConfig::from_experiment(self.config.clone(), total_candles)?;

            info!(
                "Loaded {} candles for {}, {} validation splits",
                total_candles,
                symbol,
                runtime.splits.len()
            );

            // Phase 3: Run backtests across all splits for this symbol
            for split in &runtime.splits {
                let (train_result, test_result) = self.run_split(&data, split)?;
                all_train_results.push(train_result);
                all_test_results.push(test_result);
            }
        }

        // Store runtime for later use (use last symbol's runtime for reference)
        self.runtime = Some(RuntimeConfig::from_experiment(
            self.config.clone(),
            all_train_results.len(),
        )?);

        // Phase 4: Aggregate and evaluate results across all symbols
        let summary = self.aggregate_results(&all_train_results, &all_test_results)?;

        // Phase 5: Save outputs
        self.save_outputs(&summary)?;

        // Phase 6: Complete manifest
        self.manifest.complete(summary.clone());
        self.save_manifest()?;

        info!(
            "Experiment completed successfully across {} symbol(s)",
            symbols.len()
        );
        Ok(summary)
    }

    /// Load data for a specific symbol.
    fn load_data_for_symbol(&self, symbol: &str) -> Result<Vec<f64>> {
        info!(
            "Loading data from {} for symbol: {}",
            self.config.data.source, symbol
        );

        let source = self.config.data.source.to_lowercase();
        if source != "binance" {
            bail!(
                "Unsupported data source '{}'. Currently supported: binance",
                self.config.data.source
            );
        }

        let total_candles = self.resolve_lookback_candles()?;
        info!(
            "Fetching {} {} candles for {} from Binance",
            total_candles, self.config.data.interval, symbol
        );

        let closes =
            self.fetch_binance_closes(symbol, &self.config.data.interval, total_candles)?;

        if closes.is_empty() {
            bail!("Data loader returned no close prices for {}", symbol);
        }

        Ok(closes)
    }

    fn resolve_lookback_candles(&self) -> Result<u16> {
        if let Some(lookback) = self.config.data.lookback_candles {
            let bounded = lookback.min(u16::MAX as usize) as u16;
            if lookback > u16::MAX as usize {
                warn!(
                    "lookback_candles={} exceeds u16::MAX; clamped to {}",
                    lookback,
                    u16::MAX
                );
            }
            return Ok(bounded.max(1));
        }

        let start = DateTime::parse_from_rfc3339(&self.config.data.start_date)
            .map(|dt| dt.with_timezone(&Utc))
            .or_else(|_| {
                NaiveDate::parse_from_str(&self.config.data.start_date, "%Y-%m-%d")
                    .map(|d| d.and_hms_opt(0, 0, 0).unwrap().and_utc())
            })
            .with_context(|| {
                format!(
                    "Invalid start_date '{}'. Use RFC3339 or YYYY-MM-DD",
                    self.config.data.start_date
                )
            })?;

        let end = match &self.config.data.end_date {
            Some(end_str) => DateTime::parse_from_rfc3339(end_str)
                .map(|dt| dt.with_timezone(&Utc))
                .or_else(|_| {
                    NaiveDate::parse_from_str(end_str, "%Y-%m-%d")
                        .map(|d| d.and_hms_opt(0, 0, 0).unwrap().and_utc())
                })
                .with_context(|| {
                    format!("Invalid end_date '{}'. Use RFC3339 or YYYY-MM-DD", end_str)
                })?,
            None => Utc::now(),
        };

        if end <= start {
            bail!("end_date must be after start_date");
        }

        let interval_secs = interval_to_seconds(&self.config.data.interval).ok_or_else(|| {
            anyhow::anyhow!("Unsupported interval '{}'.", self.config.data.interval)
        })?;

        let secs = (end - start).num_seconds().max(interval_secs as i64);
        let estimated = ((secs as f64 / interval_secs as f64).ceil() as usize).max(1);
        let bounded = estimated.min(u16::MAX as usize) as u16;

        if estimated > u16::MAX as usize {
            warn!(
                "Resolved candle count {} exceeds u16::MAX; clamped to {}",
                estimated,
                u16::MAX
            );
        }

        Ok(bounded)
    }

    fn fetch_binance_closes(
        &self,
        symbol: &str,
        interval: &str,
        total_candles: u16,
    ) -> Result<Vec<f64>> {
        let loader = DataLoader::new(None, None);

        let fut = async move {
            let df = loader.fetch_data(symbol, interval, total_candles).await?;
            let closes = df
                .column("close")?
                .f64()?
                .into_iter()
                .flatten()
                .collect::<Vec<f64>>();
            Ok::<Vec<f64>, anyhow::Error>(closes)
        };

        match tokio::runtime::Handle::try_current() {
            Ok(handle) => tokio::task::block_in_place(|| handle.block_on(fut)),
            Err(_) => {
                let rt = tokio::runtime::Runtime::new()?;
                rt.block_on(fut)
            }
        }
    }

    /// Run backtest for a single train/test split.
    fn run_split(
        &self,
        _data: &[f64],
        split: &DataSplit,
    ) -> Result<(BacktestResult, BacktestResult)> {
        info!(
            "Running split {}: train [{}, {}), test [{}, {})",
            split.index,
            split.train_range.0,
            split.train_range.1,
            split.test_range.0,
            split.test_range.1
        );

        // Create backtester with config
        let _backtester = Backtester::new(
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
            .max_by(|(_, a), (_, b)| a.sharpe_ratio.partial_cmp(&b.sharpe_ratio).unwrap())
            .map(|(i, _)| i)
            .unwrap_or(0);

        let best = BacktestMetrics::from(test_results[best_idx].clone());

        // Compute averages
        let avg_trades: f64 = test_results
            .iter()
            .map(|r| r.total_trades as f64)
            .sum::<f64>()
            / test_results.len() as f64;
        let avg_win_rate: f64 =
            test_results.iter().map(|r| r.win_rate).sum::<f64>() / test_results.len() as f64;
        let avg_sharpe: f64 =
            test_results.iter().map(|r| r.sharpe_ratio).sum::<f64>() / test_results.len() as f64;
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
            .max_by(|(_, a), (_, b)| a.max_drawdown_pct.partial_cmp(&b.max_drawdown_pct).unwrap())
            .map(|(i, _)| i)
            .unwrap_or(0);

        let worst = BacktestMetrics::from(test_results[worst_idx].clone());

        // Compute robustness
        let train_sharpe = train_results
            .get(best_idx)
            .map(|r: &BacktestResult| r.sharpe_ratio)
            .unwrap_or(0.0);
        let test_sharpe = test_results
            .get(best_idx)
            .map(|r: &BacktestResult| r.sharpe_ratio)
            .unwrap_or(0.0);
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
            combinations_passed: train_results
                .iter()
                .filter(|r| r.sharpe_ratio > 0.0)
                .count(),
            train_metrics: train_results
                .get(best_idx)
                .map(|r: &BacktestResult| BacktestMetrics::from(r.clone())),
            test_metrics: test_results
                .get(best_idx)
                .map(|r: &BacktestResult| BacktestMetrics::from(r.clone())),
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

fn interval_to_seconds(interval: &str) -> Option<u64> {
    match interval {
        "1m" => Some(60),
        "3m" => Some(3 * 60),
        "5m" => Some(5 * 60),
        "15m" => Some(15 * 60),
        "30m" => Some(30 * 60),
        "1h" => Some(60 * 60),
        "2h" => Some(2 * 60 * 60),
        "4h" => Some(4 * 60 * 60),
        "6h" => Some(6 * 60 * 60),
        "8h" => Some(8 * 60 * 60),
        "12h" => Some(12 * 60 * 60),
        "1d" => Some(24 * 60 * 60),
        "3d" => Some(3 * 24 * 60 * 60),
        "1w" => Some(7 * 24 * 60 * 60),
        _ => None,
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// CLI Entry Point Support
// ─────────────────────────────────────────────────────────────────────────────

/// Run an experiment from a config file.
pub fn run_from_config(path: &PathBuf) -> Result<ResultsSummary> {
    // Note: YAML support not yet implemented - use JSON
    let config = ExperimentConfig::from_json(path)?;

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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::ExperimentConfig;

    #[test]
    fn interval_mapping_supports_common_binance_intervals() {
        assert_eq!(interval_to_seconds("1h"), Some(3600));
        assert_eq!(interval_to_seconds("4h"), Some(14_400));
        assert_eq!(interval_to_seconds("1d"), Some(86_400));
        assert_eq!(interval_to_seconds("bogus"), None);
    }

    #[test]
    fn resolves_lookback_from_config_or_dates() {
        let mut config = ExperimentConfig::example();
        config.data.lookback_candles = Some(1234);
        let runner = ExperimentRunner::new(config).expect("runner should build");
        assert_eq!(runner.resolve_lookback_candles().unwrap(), 1234);

        let mut config2 = ExperimentConfig::example();
        config2.data.lookback_candles = None;
        config2.data.interval = "1h".to_string();
        config2.data.start_date = "2024-01-01".to_string();
        config2.data.end_date = Some("2024-01-02".to_string());
        let runner2 = ExperimentRunner::new(config2).expect("runner should build");
        assert_eq!(runner2.resolve_lookback_candles().unwrap(), 24);
    }
}
