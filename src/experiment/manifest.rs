//! Run manifest for experiment reproducibility and auditability.
//!
//! Every experiment run produces a `RunManifest` that captures:
//! - Exact configuration used
//! - Git commit hash
//! - Environment details
//! - Results summary
//! - Output file locations
//!
//! This enables comparison across runs and reproducibility.

use anyhow::Result;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Manifest for a single experiment run.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunManifest {
    /// Unique run identifier
    pub run_id: String,

    /// Timestamp when run started
    pub started_at: DateTime<Utc>,

    /// Timestamp when run completed
    pub completed_at: Option<DateTime<Utc>>,

    /// Run status
    pub status: RunStatus,

    /// Configuration used (full experiment config)
    pub config: serde_json::Value,

    /// Git information
    pub git: GitInfo,

    /// Environment information
    pub environment: EnvironmentInfo,

    /// Data checksums for reproducibility
    pub data: DataInfo,

    /// Results summary
    pub results: Option<ResultsSummary>,

    /// Output files generated
    pub outputs: Vec<OutputFile>,

    /// Error message if failed
    pub error: Option<String>,
}

impl RunManifest {
    /// Create a new manifest for a run.
    pub fn new(run_id: String, config_json: serde_json::Value) -> Self {
        Self {
            run_id,
            started_at: Utc::now(),
            completed_at: None,
            status: RunStatus::Running,
            config: config_json,
            git: GitInfo::capture(),
            environment: EnvironmentInfo::capture(),
            data: DataInfo::default(),
            results: None,
            outputs: Vec::new(),
            error: None,
        }
    }

    /// Mark run as completed successfully.
    pub fn complete(&mut self, results: ResultsSummary) {
        self.status = RunStatus::Completed;
        self.completed_at = Some(Utc::now());
        self.results = Some(results);
    }

    /// Mark run as failed.
    pub fn fail(&mut self, error: String) {
        self.status = RunStatus::Failed;
        self.completed_at = Some(Utc::now());
        self.error = Some(error);
    }

    /// Add an output file to the manifest.
    pub fn add_output(&mut self, file: OutputFile) {
        self.outputs.push(file);
    }

    /// Save manifest to JSON file.
    pub fn save(&self, path: &PathBuf) -> Result<()> {
        let content = serde_json::to_string_pretty(self)?;
        std::fs::write(path, content)?;
        Ok(())
    }

    /// Load manifest from JSON file.
    pub fn load(path: &PathBuf) -> Result<Self> {
        let content = std::fs::read_to_string(path)?;
        let manifest: Self = serde_json::from_str(&content)?;
        Ok(manifest)
    }

    /// Get duration of the run.
    pub fn duration(&self) -> Option<chrono::Duration> {
        self.completed_at.map(|end| end - self.started_at)
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Supporting Types
// ─────────────────────────────────────────────────────────────────────────────

/// Run status.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum RunStatus {
    Running,
    Completed,
    Failed,
    Cancelled,
}

impl RunStatus {
    /// Get human-readable status text.
    pub fn text(&self) -> &'static str {
        match self {
            RunStatus::Running => "running",
            RunStatus::Completed => "completed",
            RunStatus::Failed => "failed",
            RunStatus::Cancelled => "cancelled",
        }
    }
}

impl RunManifest {
    /// Get human-readable status text.
    pub fn status_text(&self) -> &'static str {
        self.status.text()
    }
}

/// Git repository information.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GitInfo {
    /// Current commit hash
    pub commit_hash: String,

    /// Current branch name
    pub branch: String,

    /// Whether there are uncommitted changes
    pub dirty: bool,

    /// Remote URL (if available)
    pub remote: Option<String>,
}

impl GitInfo {
    /// Capture current git information.
    pub fn capture() -> Self {
        let commit_hash = std::process::Command::new("git")
            .args(["rev-parse", "HEAD"])
            .output()
            .ok()
            .and_then(|o| String::from_utf8(o.stdout).ok())
            .map(|s| s.trim().to_string())
            .unwrap_or_else(|| "unknown".to_string());

        let branch = std::process::Command::new("git")
            .args(["rev-parse", "--abbrev-ref", "HEAD"])
            .output()
            .ok()
            .and_then(|o| String::from_utf8(o.stdout).ok())
            .map(|s| s.trim().to_string())
            .unwrap_or_else(|| "unknown".to_string());

        let dirty = std::process::Command::new("git")
            .args(["status", "--porcelain"])
            .output()
            .ok()
            .and_then(|o| String::from_utf8(o.stdout).ok())
            .map(|s| !s.trim().is_empty())
            .unwrap_or(false);

        let remote = std::process::Command::new("git")
            .args(["remote", "get-url", "origin"])
            .output()
            .ok()
            .and_then(|o| String::from_utf8(o.stdout).ok())
            .map(|s| s.trim().to_string());

        Self {
            commit_hash,
            branch,
            dirty,
            remote,
        }
    }
}

impl Default for GitInfo {
    fn default() -> Self {
        Self::capture()
    }
}

/// Environment information.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EnvironmentInfo {
    /// Operating system
    pub os: String,

    /// Rust version
    pub rust_version: String,

    /// Hostname
    pub hostname: String,

    /// Timestamp
    pub captured_at: DateTime<Utc>,
}

impl EnvironmentInfo {
    /// Capture current environment information.
    pub fn capture() -> Self {
        let os = std::env::consts::OS.to_string();

        let rust_version = std::process::Command::new("rustc")
            .args(["--version"])
            .output()
            .ok()
            .and_then(|o| String::from_utf8(o.stdout).ok())
            .map(|s| s.trim().to_string())
            .unwrap_or_else(|| "unknown".to_string());

        let hostname = get_hostname();

        Self {
            os,
            rust_version,
            hostname,
            captured_at: Utc::now(),
        }
    }
}

impl Default for EnvironmentInfo {
    fn default() -> Self {
        Self::capture()
    }
}

/// Data source information for reproducibility.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct DataInfo {
    /// Symbols used
    pub symbols: Vec<String>,

    /// Total candles per symbol
    pub candles_per_symbol: std::collections::HashMap<String, usize>,

    /// Date range
    pub date_range: Option<(String, String)>,

    /// Data checksums (for cache validation)
    pub checksums: std::collections::HashMap<String, String>,
}

/// Summary of experiment results.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResultsSummary {
    /// Best result across all splits/optimizations
    pub best: BacktestMetrics,

    /// Average result across splits
    pub average: BacktestMetrics,

    /// Worst result (for risk assessment)
    pub worst: BacktestMetrics,

    /// Number of parameter combinations tested
    pub combinations_tested: usize,

    /// Number of combinations passing thresholds
    pub combinations_passed: usize,

    /// Train metrics (for overfitting detection)
    pub train_metrics: Option<BacktestMetrics>,

    /// Test metrics
    pub test_metrics: Option<BacktestMetrics>,

    /// Robustness score (test/train ratio for primary metric)
    pub robustness: Option<f64>,

    /// Individual split results (train/test pairs)
    pub split_results: Vec<SplitResult>,
}

/// Result from a single train/test split.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SplitResult {
    /// Split index
    pub split_index: usize,

    /// Symbol this split was run on
    pub symbol: String,

    /// Train period metrics
    pub train_metrics: BacktestMetrics,

    /// Test period metrics
    pub test_metrics: BacktestMetrics,

    /// Train period range (start, end) as bar indices
    pub train_range: (usize, usize),

    /// Test period range (start, end) as bar indices
    pub test_range: (usize, usize),

    /// Test period equity curve (mark-to-market equity at each bar)
    pub equity_curve: Option<Vec<f64>>,
}

/// Backtest metrics snapshot.
///
/// Captures both core metrics and V3 extended metrics from the backtester.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct BacktestMetrics {
    // Core metrics
    pub total_trades: usize,
    pub win_rate: f64,
    pub profit_factor: f64,
    pub total_return_pct: f64,
    pub max_drawdown_pct: f64,
    pub sharpe_ratio: f64,
    pub kelly_fraction: f64,
    pub total_fees_paid: f64,

    // V3 extended metrics
    pub sortino_ratio: f64,
    pub calmar_ratio: f64,
    pub avg_trade_duration_bars: f64,
    pub max_consecutive_wins: usize,
    pub max_consecutive_losses: usize,
    pub avg_win_pct: f64,
    pub avg_loss_pct: f64,
    pub largest_win_pct: f64,
    pub largest_loss_pct: f64,
}

impl From<crate::backtest::engine::BacktestResult> for BacktestMetrics {
    fn from(result: crate::backtest::engine::BacktestResult) -> Self {
        Self {
            // Core metrics
            total_trades: result.total_trades,
            win_rate: result.win_rate,
            profit_factor: result.profit_factor,
            total_return_pct: result.total_return_pct,
            max_drawdown_pct: result.max_drawdown_pct,
            sharpe_ratio: result.sharpe_ratio,
            kelly_fraction: result.kelly_fraction,
            total_fees_paid: result.total_fees_paid,

            // V3 extended metrics
            sortino_ratio: result.sortino_ratio,
            calmar_ratio: result.calmar_ratio,
            avg_trade_duration_bars: result.avg_trade_duration_bars,
            max_consecutive_wins: result.max_consecutive_wins,
            max_consecutive_losses: result.max_consecutive_losses,
            avg_win_pct: result.avg_win_pct,
            avg_loss_pct: result.avg_loss_pct,
            largest_win_pct: result.largest_win_pct,
            largest_loss_pct: result.largest_loss_pct,
        }
    }
}

/// Output file generated by the experiment.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OutputFile {
    /// File type
    pub file_type: OutputFileType,

    /// Relative path from experiment output directory
    pub path: PathBuf,

    /// File size in bytes
    pub size_bytes: u64,

    /// Description
    pub description: String,
}

/// Types of output files.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum OutputFileType {
    Manifest,
    EquityCurve,
    TradeLog,
    Metrics,
    Plot,
    ModelSnapshot,
    Custom,
}

// ─────────────────────────────────────────────────────────────────────────────
// Comparison Utilities
// ─────────────────────────────────────────────────────────────────────────────

impl RunManifest {
    /// Compare this run with another run.
    pub fn compare(&self, other: &RunManifest) -> RunComparison {
        let self_results = self.results.as_ref();
        let other_results = other.results.as_ref();

        RunComparison {
            run_a: self.run_id.clone(),
            run_b: other.run_id.clone(),
            sharpe_diff: self_results
                .zip(other_results)
                .map(|(a, b)| a.best.sharpe_ratio - b.best.sharpe_ratio),
            return_diff: self_results
                .zip(other_results)
                .map(|(a, b)| a.best.total_return_pct - b.best.total_return_pct),
            drawdown_diff: self_results
                .zip(other_results)
                .map(|(a, b)| a.best.max_drawdown_pct - b.best.max_drawdown_pct),
            config_diff: diff_configs(&self.config, &other.config),
        }
    }
}

/// Comparison between two runs.
#[derive(Debug, Clone)]
pub struct RunComparison {
    pub run_a: String,
    pub run_b: String,
    pub sharpe_diff: Option<f64>,
    pub return_diff: Option<f64>,
    pub drawdown_diff: Option<f64>,
    pub config_diff: Vec<String>,
}

/// Find differences between two configs (simplified).
fn diff_configs(a: &serde_json::Value, b: &serde_json::Value) -> Vec<String> {
    let mut diffs = Vec::new();

    if let (Some(a_obj), Some(b_obj)) = (a.as_object(), b.as_object()) {
        for key in a_obj.keys() {
            if a_obj.get(key) != b_obj.get(key) {
                diffs.push(format!(
                    "{}: {:?} -> {:?}",
                    key,
                    a_obj.get(key),
                    b_obj.get(key)
                ));
            }
        }
    }

    diffs
}

/// Get the system hostname without external dependencies.
fn get_hostname() -> String {
    #[cfg(unix)]
    {
        use std::ffi::CStr;
        use std::os::raw::c_char;

        let mut buf = [0u8; 256];
        unsafe {
            let result = libc::gethostname(buf.as_mut_ptr() as *mut c_char, buf.len());
            if result == 0 {
                let cstr = CStr::from_ptr(buf.as_ptr() as *const c_char);
                return cstr.to_string_lossy().to_string();
            }
        }
    }

    // Fallback for non-Unix or if the above fails
    "unknown".to_string()
}
