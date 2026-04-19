//! Configuration for live trading.

use serde::{Deserialize, Serialize};

/// Trading mode — safety interlock for live execution.
///
/// ```text
/// DryRun     → simulated orders only, no exchange connection
/// Testnet    → real testnet orders (testnet.binancefuture.com)
/// Production → REAL mainnet orders — MUST be explicitly opted in
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TradingMode {
    DryRun,
    Testnet,
    Production,
}

impl Default for TradingMode {
    fn default() -> Self { TradingMode::DryRun }
}

/// Turtle+Chandelier strategy params (validated walk-forward, frozen 2026-04-16).
pub const TURTLE_EP: usize = 21;
pub const CHAND_PERIOD: usize = 15; // hyperopt 2026-04-19: 2D joint sweep P×M, 2D sweep winner P=15/M=1.50. Joint sweep over P∈[15,30]×M∈[1.50,2.10]. 7/9 universes validate P15/M150 wins over P20/M215. See hyperopt-2026-04-19-chand-pm-2d.md.
pub const CHAND_MULT: f64 = 1.50;
pub const TURTLE_ATR_PERIOD: usize = 24; // hyperopt 2026-04-16: ATR=24 wins (+3.6% Sharpe, -10.8pp DD vs ATR=25). Fine sweep 18-35 step=1, 18 values × 9 universes × 54 windows. 7/9 universes agree. See hyperopt-2026-04-16-atr-period.md.
pub const TURTLE_ATR_MULT: f64 = 2.0;
pub const HOLD_MAX: usize = 45; // Walk-forward harness uses HM=45 (validated). Live production may prefer HM=15 (higher Sharpe with P=15/M=1.50 — see hyperopt-2026-04-19-holdmax.md). HM=15 sweep on Base5: Sharpe 5.95 vs HM=45: 5.40 (+10%). Chandelier(P=15/M=1.50) fires ~bar 14-15; HM=15 is near the exit boundary.
pub const POSITION_CAP: usize = 3;

/// Configuration for live trading bot.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LiveConfig {
    /// API key for Binance (set via environment variable in production)
    pub api_key: Option<String>,
    /// API secret for Binance (set via environment variable in production)
    pub api_secret: Option<String>,
    /// Trading symbols (e.g., ["BTCUSDT", "ETHUSDT"])
    pub symbols: Vec<String>,
    /// Kline interval (e.g., "1d", "4h", "1h")
    pub interval: String,
    /// Initial capital for position sizing
    pub initial_capital: f64,
    /// Maximum position size as fraction (0.0 to 1.0)
    pub max_position_size: f64,
    /// Fee percentage (0.001 = 0.1%)
    pub fee_pct: f64,
    /// Use testnet for orders (safety flag)
    pub use_testnet: bool,
    /// Dry run mode - no real orders placed
    pub dry_run: bool,
    // --- Safety: trading mode (prevents accidental mainnet orders) ---
    /// Trading mode: DryRun (simulated), Testnet (real testnet), Production (real mainnet)
    #[serde(default = "TradingMode::default")]
    pub mode: TradingMode,
    // --- Turtle+Chandelier strategy params (frozen) ---
    /// Turtle entry lookback (default: 21)
    pub ep: usize,
    /// Chandelier ATR period (default: 20)
    pub chand_period: usize,
    /// Chandelier ATR multiplier (default: 2.15)
    pub chand_mult: f64,
    /// Turtle ATR stop period (default: 24)
    pub atr_period: usize,
    /// Turtle ATR stop multiplier (default: 2.0)
    pub atr_mult: f64,
    /// Max hold bars (default: 45)
    pub hold_max: usize,
    /// Max concurrent positions (default: 3)
    pub position_cap: usize,
    // --- Legacy fields (kept for backward compat, unused by signal logic) ---
    #[serde(default = "default_bb_period")]
    pub bb_period: usize,
    #[serde(default = "default_bb_std")]
    pub bb_std: f64,
    #[serde(default = "default_atr_stop_mult")]
    pub atr_stop_mult: f64,
}

fn default_bb_period() -> usize { TURTLE_EP }
fn default_bb_std() -> f64 { CHAND_MULT }
fn default_atr_stop_mult() -> f64 { TURTLE_ATR_MULT }

impl Default for LiveConfig {
    fn default() -> Self {
        Self {
            api_key: None,
            api_secret: None,
            symbols: vec!["BTCUSDT".to_string()],
            interval: "1d".to_string(),
            initial_capital: 10_000.0,
            max_position_size: 1.0 / POSITION_CAP as f64,
            fee_pct: 0.0004, // ~0.04% RT (conservative taker)
            use_testnet: true,
            dry_run: true,
            mode: TradingMode::default(),
            // Turtle+Chandelier (frozen 2026-04-16)
            ep: TURTLE_EP,
            chand_period: CHAND_PERIOD,
            chand_mult: CHAND_MULT,
            atr_period: TURTLE_ATR_PERIOD,
            atr_mult: TURTLE_ATR_MULT,
            hold_max: HOLD_MAX,
            position_cap: POSITION_CAP,
            // Legacy
            bb_period: TURTLE_EP,
            bb_std: CHAND_MULT,
            atr_stop_mult: TURTLE_ATR_MULT,
        }
    }
}

impl LiveConfig {
    /// Create config from environment variables.
    ///
    /// Reads BINANCE_API_KEY and BINANCE_API_SECRET from environment.
    pub fn from_env() -> Self {
        Self {
            api_key: std::env::var("BINANCE_API_KEY").ok(),
            api_secret: std::env::var("BINANCE_API_SECRET").ok(),
            ..Self::default()
        }
    }

    /// Create config for production (mainnet, real orders).
    pub fn production(symbols: Vec<String>, initial_capital: f64) -> Self {
        Self {
            symbols,
            initial_capital,
            mode: TradingMode::Production,
            use_testnet: false,
            dry_run: false,
            ..Self::from_env()
        }
    }

    /// Create config for paper trading (mainnet data, no real orders).
    pub fn paper(symbols: Vec<String>, initial_capital: f64) -> Self {
        Self {
            symbols,
            initial_capital,
            mode: TradingMode::DryRun,
            use_testnet: false,
            dry_run: true,
            ..Self::from_env()
        }
    }

    /// Create config for testnet (test data, real testnet orders).
    pub fn testnet(symbols: Vec<String>, initial_capital: f64) -> Self {
        Self {
            symbols,
            initial_capital,
            mode: TradingMode::Testnet,
            use_testnet: true,
            dry_run: false,
            ..Self::from_env()
        }
    }

    /// Validate configuration is safe to start.
    pub fn validate(&self) -> anyhow::Result<()> {
        if self.symbols.is_empty() {
            anyhow::bail!("At least one symbol required");
        }
        if self.initial_capital <= 0.0 {
            anyhow::bail!("Initial capital must be positive");
        }
        if self.max_position_size <= 0.0 || self.max_position_size > 1.0 {
            anyhow::bail!("Max position size must be between 0 and 1");
        }
        if !self.dry_run && (self.api_key.is_none() || self.api_secret.is_none()) {
            anyhow::bail!("API credentials required for live trading");
        }

        // Safety interlock: Production mode requires explicit opt-in
        if self.mode == TradingMode::Production && !self.dry_run {
            if self.use_testnet {
                anyhow::bail!(
                    "CONFLICT: mode=Production but use_testnet=true. \
                    Production mode requires use_testnet=false. \
                    Edit live_turtle_chandelier.rs to set use_testnet: false before proceeding."
                );
            }
            tracing::error!(
                "🔴 PRODUCTION MODE ARMED — real mainnet orders will be placed. \
                This is irreversible. Ensure you understand the risk."
            );
        }

        // Warn when placing real testnet orders
        if self.mode == TradingMode::Testnet && !self.dry_run {
            tracing::warn!("⚠️  TESTNET MODE — real orders will be placed on testnet.binancefuture.com");
        }

        Ok(())
    }
}