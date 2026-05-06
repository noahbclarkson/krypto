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
pub const TURTLE_EP: usize = 21; // hyperopt 2026-04-20 (original): EP=21 wins 43/54 (79.6%). EP=24 re-opt 2026-04-20: EP=24 wins 45/54 (+2 windows). T3 held-out 2026-04-26: REVERTED EP=24 -> 21. Paired held-out: EP=21 avg Sharpe 0.18 vs EP=24 0.16, 27/29 vs 25/29 pass. EP=24 was in-sample inflation. See snapshots/t3_ep_paired_held_out.csv.
pub const CHAND_PERIOD: usize = 7; // hyperopt 2026-04-21: EXTENSIVE sweep CP∈[5..60 step 2] × 9 universes × 54 windows with EP=21/HM=12/ATR_ENTRY_MULT=0.00/CM=2.30. CP=7 wins global Sharpe 5.908 (+6.9% vs CP=11 baseline 5.526). Pass rate 79.6% (identical). Prior CP=11 sweep used stale EP=21 (not current EP=24). See memory/hyperopt-2026-04-21-chand-period.md.
pub const CHAND_MULT: f64 = 2.30; // hyperopt 2026-04-25: DENSE sweep M∈[1.50..5.00] step 0.05 (71 values) × 9 universes × 54 windows. M=2.30 wins: Sharpe 6.2036 (+0.8% vs M=2.25 at 6.1225), pass rate 83.3% (45/54) vs 81.5% (44/54). M=2.30 is the lowest M at peak pass rate, making it the most efficient setting. See memory/hyperopt-2026-04-25-chand-mult-dense.md.
pub const TURTLE_ATR_PERIOD: usize = 24; // hyperopt 2026-04-16: ATR=24 wins (+3.6% Sharpe, -10.8pp DD vs ATR=25). Fine sweep 18-35 step=1, 18 values × 9 universes × 54 windows. 7/9 universes agree. See hyperopt-2026-04-16-atr-period.md.
pub const TURTLE_ATR_MULT: f64 = 2.0;
pub const ATR_ENTRY_MULT: f64 = 0.00; // REVERTED 2026-04-25: Full 41-value sweep {0.00-2.00 step 0.05} × 9 universes × 54 windows with CHAND(7,2.30)/EP=24/HM=12. EM=0.00 wins definitively: 83.3% pass, Sharpe 1.87, +151.9% return, 707 trades. Any non-zero filter degrades pass rate monotonically. Prior EM=0.85 winner (2026-04-21) was optimized ON the same OOS data used for EP=24 and P=7 — classic in-sample inflation. EM=0.00 is the correct production default.
pub const HOLD_MAX: usize = 12; // hyperopt 2026-05-02: HM∈[5..=100 step 5] × 9 universes × 7 windows, live Turtle-only path. HM=5: 48/63 pass (76%), Sharpe 2.096, Ret 72.7%. HM=12: 45/63 pass (71%), Sharpe 1.906, Ret 265.6%. CONFIRMED: HOLD_MAX is never binding — Turtle ATR stop fires first. Both HM=5 and HM=12 produce IDENTICAL live WF results (43/63 pass). Robustness-first: keep HM=12 (tighter, no cost). See memory/hyperopt-2026-05-02-hold-max.md.
pub const POSITION_CAP: usize = 3; // CONFIRMED 2026-04-27 under current Turtle-only live logic. Extensive 10-value sweep CAP∈[1..10] across 9 universes × 6 walk-forward windows: CAP=3 is robustness winner (72.2% pass, Sharpe 4.58, 9/9 positive universes). CAP=4-10 chase more return but materially degrade pass rate to 61.1%-57.4%. See memory/hyperopt-2026-04-27.md.
// hyperopt 2026-05-02 held-out: AP=64 REJECTED (same-harness artifact, EP=24 pattern).
// AP=12 remains the valid production default. See snapshots/ap_held_out_validation.csv.
// AP=64 found as 3rd sequential optimization on live_compatible_wf.rs after T=24 and LB=42.
// Held-out result: AP=12=24/30 pass, Sharpe 0.031, Ret 82.0% vs AP=64=23/30, Sharpe 0.037, Ret 42.8%.
// AP=64 rejected on fewer passes + lower return despite marginally higher Sharpe.
// T62 FIX: Reverted to AP=12 (2026-05-04 21:01 UTC) — AP=63 won by +1 window on the
// same OOS harness without held-out validation. Anti-overfit rule: marginal wins
// (< 3 windows over baseline) require held-out before production promotion.
// AP=63 promoted prematurely — same pattern as EP=24 (reverted 2026-04-26).
// AP=12 is confirmed by T44 fine sweep (54/63 pass, Sharpe 7.65, Base5 9.34).
// See memory/2026-05-04.md [20:05 UTC] for anti-overfit violation details.
pub const REGIME_ATR_PERIOD: usize = 17; // hyperopt 2026-05-04: AP=17 wins OOS on Sharpe/Return, AP=63 wins on pass rate. Held-out (4-period pre-2021): AP=17: 4/4 pass, Sharpe 7.715, equity 1.9481x, DD 21.3%. AP=63: 4/4 pass, Sharpe 5.721, equity 1.3976x, DD 26.6%. AP=12: 2/4 pass. AP=17 dominates on all held-out metrics. Promoted from AP=12. See memory/hyperopt-2026-05-04-ap-holdout.md.
pub const REGIME_LOOKBACK: usize = 41; // hyperopt 2026-05-05: LB∈[5..=200 step 1] × 9 universes × 7 WF windows under live Turtle-only path (AP=17, T=5). LB=41: 59/63 pass (93.7%), Sharpe 8.107 (+7.0% vs LB=42: 7.577), Return +79.9%, DD 1.62% unchanged. LB=41 isolated maximum Sharpe across full integer range. Robustness plateau LB=40-45. See memory/hyperopt-2026-05-05-regime-lookback.md.
/// ATR rank threshold for entry gate — percent rank of 21-bar ATR relative to 252-bar history.
/// REVERTED TO T=5.0 (2026-05-04): T=24 failed held-out validation (same-harness artifact, EP=24 pattern).
/// T=24 passed 54/63 (86%) on post-2021 OOS windows but only 10/22 on pre-2021 held-out data.
/// T=5 passed 14/22 on pre-2021 held-out (same as T=0 no-filter), Sharpe +0.664 vs T=24 at -0.964.
/// EP=24 showed the identical pattern (3rd sequential opt, then failed held-out 25/29 vs 27/29).
/// ATR_RANK=24 is the same artifact. Revert to T=5.0. See snapshots/t52_atr_rank_held_out.csv.
pub const ATR_RANK_THRESHOLD: f64 = 5.0;

/// Volume lookback window for dollar-volume ranking in research/diagnostic harnesses.
/// IMPORTANT: intentionally unused by `src/live/bot.rs` entry logic.
/// T72 (2026-05-06) isolated a live-semantics top-3 dollar-volume gate with VL=92:
/// it worsened Base5 exact replay from 2.56x / Sharpe 0.95 / 298 trades to
/// 1.01x / Sharpe 0.09 / 207 trades. Do not wire this into the event-driven live
/// bot without a new mechanism; FIFO/equal-slot live entries are the production truth.
pub const VOL_LOOKBACK: usize = 92;

/// USDT hedge overlay: reduce position size when BTC 21d ATR is above this percentile of its 252d history.
/// T67 hyperopt 2026-05-05: INERT — full 101-value sweep (PCT∈[0..100] step 1) × 9 universes × 7 WF windows.
/// ALL values produce IDENTICAL pass (56/63, 88.9%), Sharpe (6.941), and trades (689).
/// The mechanism never fires regardless of threshold. ATR_RANK already provides regime filtering.
/// This parameter is dead code; HEDGE_ATR_PCT=0.45 maintained for historical compatibility.
pub const HEDGE_ATR_PCT: f64 = 0.45;

/// Position-size multiplier when the USDT hedge overlay is active.
/// Updated 2026-05-05 (T66 hyperopt): extensive SM sweep {0.30..=1.00 step 0.05} × 9 universes × 7 WF windows.
/// SM=0.40 wins robustness-first: 59/63 pass (93.7%) vs SM=0.70 at 58/63 (92.1%),
/// Sharpe 7.577 vs 7.079 (+0.498), DD 16.0% vs 21.2% (-5.2pp).
/// Raw equity lower (46.9x vs 114.6x) — risk dial, not alpha.
/// Confirmed live_compatible_wf with SM=0.70: 58/63 pass, Sharpe 7.079, Base5 114.63x.
pub const HEDGE_SIZE_MULT: f64 = 0.40;

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
    /// ATR entry multiplier — momentum filter on Turtle breakout (default: 0.0)
    /// Only enter if close >= breakout_level + ATR(atr_period) * ATR_ENTRY_MULT.
    /// hyperopt 2026-04-25: Full 41-value sweep × 9 universes × 54 windows.
    /// EM=0.00 wins definitively: 83.3% pass, Sharpe 1.87. Any non-zero filter
    /// degrades pass rate monotonically. Prior EM=0.90 was in-sample inflation.
    pub atr_entry_mult: f64,
    /// Max hold bars (default: 45)
    pub hold_max: usize,
    /// Max concurrent positions (default: 3)
    pub position_cap: usize,
    /// BTC ATR period for regime percentile filter (default: 12)
    #[serde(default = "default_regime_atr_period")]
    pub regime_atr_period: usize,
    /// BTC ATR percentile lookback for regime filter (default: 42)
    #[serde(default = "default_regime_lookback")]
    pub regime_lookback: usize,
    /// Minimum BTC ATR percentile rank required for new entries (default: 24.0)
    #[serde(default = "default_atr_rank_threshold")]
    pub atr_rank_threshold: f64,
    /// Volume lookback window for research/diagnostic dollar-volume ranking.
    /// Currently unused by live `bot.rs` entry logic after T72 rejection.
    #[serde(default = "default_vol_lookback")]
    pub vol_lookback: usize,
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
fn default_regime_atr_period() -> usize { REGIME_ATR_PERIOD }
fn default_regime_lookback() -> usize { REGIME_LOOKBACK }
fn default_atr_rank_threshold() -> f64 { ATR_RANK_THRESHOLD }
fn default_vol_lookback() -> usize { VOL_LOOKBACK }

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
            atr_entry_mult: ATR_ENTRY_MULT,
            hold_max: HOLD_MAX,
            position_cap: POSITION_CAP,
            regime_atr_period: REGIME_ATR_PERIOD,
            regime_lookback: REGIME_LOOKBACK,
            atr_rank_threshold: ATR_RANK_THRESHOLD,
            vol_lookback: VOL_LOOKBACK,
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
        if self.regime_atr_period == 0 {
            anyhow::bail!("Regime ATR period must be positive");
        }
        if self.regime_lookback == 0 {
            anyhow::bail!("Regime lookback must be positive");
        }
        if !(0.0..=100.0).contains(&self.atr_rank_threshold) {
            anyhow::bail!("ATR rank threshold must be between 0 and 100");
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
