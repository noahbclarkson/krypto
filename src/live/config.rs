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
pub const HOLD_MAX: usize = 15; // T88 hyperopt 2026-05-10: extensive HM∈[1..=100 step 1] × 9 universes × ~8 WF windows under exact-live Turtle-only config. ALL 100 values pass 100% (69/69) on the OOS sweep — Turtle ATR exit dominates before HOLD_MAX can bind for values ≥8. HM=1 wins on Sharpe (2.13) but fires too frequently (7,378 trades vs 2,525 for HM=15). HM=15 sits mid-plateau with Sharpe 1.544 and optimal trade quality. Exact-live verification (live_bot_exact_equity.rs): HM=15 → 2.76x / Sharpe 1.02 / MaxDD 22.3% / 286 trades. Current config confirmed optimal. See memory/hyperopt-2026-05-10.md.
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
pub const REGIME_LOOKBACK: usize = 41; // REVERTED T86: LB=140 failed exact-live verification (2.58x vs 2.76x). Prior LB=41 restored. Exact-live equity path is NOT comparable to sweep aggregator. See memory/hyperopt-2026-05-08.md.
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

/// USDT hedge overlay: reduce position size when BTC hedge ATR is above this percentile of its TR history.
/// T67 hyperopt 2026-05-05: INERT — full 101-value sweep on dual Chandelier research harness.
/// T95 bug 2026-05-11: REVERTED — T95 used walk-forward pass rate as metric (HAP=0.09 won 8/11)
/// but failed to use daily account equity. HAP=0.09 produces 1.42x exact-live equity vs 2.76x for
/// HAP=0.45 — a 49% reduction. T95 was a WALK-FORWARD research harness, not exact-live source.
/// Production default: HAP=0.45 (produces verified 2.76x / Sharpe 1.03 / MaxDD 22.3%).
/// T95 finding confirmed: HEDGE_ATR_PCT is a position SIZE dial, not alpha.
pub const HEDGE_ATR_PCT: f64 = 0.45;

/// BTC ATR period for the USDT hedge overlay.
/// T75 hyperopt 2026-05-06: full integer sweep P∈[5..=100] step 1 × 9 universes × 252d WF windows.
/// P=38 wins robustness-first: 50/60 pass (83.3%), Sharpe 1.432, avg return +22.12%, DD 11.51%
/// versus old hardcoded P=21 at 47/60 pass (78.3%), Sharpe 1.294, avg return +20.54%, DD 11.87%.
/// Robust plateau P=37..45 all achieved 50/60 pass; select P=38 as the highest-Sharpe plateau member.
pub const HEDGE_ATR_PERIOD: usize = 38;

/// BTC true-range history used as the hedge percentile reference.
/// T84 2026-05-07: LB sweep under production params found LB=147 winner in
/// walk-forward harness (9/9 pass, Sharpe 1.243 vs LB=252 at 8/9 pass, 1.135).
/// EXACT-LIVE REVERTED: live_bot_exact_equity.rs with LB=147 produced 2.10x vs
/// LB=252 at 2.77x — harness gap confirmed again. Walk-forward robustness
/// winner ≠ exact-live winner. HEDGE_LOOKBACK = 252 remains the production default.
/// T84 sweep found LB=147 more robust on research harness (100% vs 89%), but
/// exact-live verification shows 2.10x vs 2.76x for LB=147 - SAME INFLATION
/// PATTERN as EP=24. Do not promote. See memory/hyperopt-2026-05-10.md.
pub const HEDGE_LOOKBACK: usize = 252;

/// Position-size multiplier when the USDT hedge overlay is active.
/// T83 hyperopt 2026-05-07: extensive HSM∈[0.10..=1.00 step 0.05] × 9 universes / 60 WF windows
/// under current exact-live semantics after HOLD_MAX=15. HSM is a defensive risk dial, not alpha:
/// higher values raise raw return but reduce robustness and increase drawdown.
/// HSM=0.25 wins pass-rate-first: 52/60 pass (86.7%), Sharpe 1.481, DD 9.82%.
/// Old HSM=0.55: 46/60 pass (76.7%), Sharpe 1.439, DD 12.85%.
/// Exact-live after update: 2.80x / daily Sharpe 1.04 / MaxDD 22.3% / 286 trades.
pub const HEDGE_SIZE_MULT: f64 = 0.25;

/// Re-entry cooldown: bars to wait after exit before re-entry on same symbol.
/// T96 hyperopt 2026-05-11: FC=93 wins on exact-live 9-universe walk-forward path.
/// 101-value sweep FC∈[0..100]: FC=0→52% pass, Sharpe 2.11, DD 59%, 1558 trades;
/// FC=93→83% pass, Sharpe 3.88, DD 23.5%, 423 trades. Wide plateau FC∈[92..97].
pub const FRESHNESS_COOLDOWN: usize = 0; // T97: REVERTED — in-sample artifact, 0 held-out passes.

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
