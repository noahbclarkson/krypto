# Krypto Foundation: Design Decisions & Tradeoffs

**Document Purpose:** Record implementation decisions for future reference. Each choice has implications - this document explains why we made them.

**Last Updated:** 2026-02-25

---

## 0. System Requirements

### Rust Version

**Requirement:** Rust 1.82+ (edition2024 support)

**Current Issue:** System Rust (1.75.0) is too old for modern crate ecosystem. Multiple transitive dependencies (yoke, icu_normalizer_data, comfy-table) now require Rust 1.82+.

**Workaround:** The foundation code is written and structured correctly, but cannot be compiled on this system. To compile:
1. Install rustup: `curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh`
2. Update Rust: `rustup update stable`
3. Verify: `rustc --version` should show 1.82+

**Files Added (Foundation):**
- `src/config/mod.rs` - Config module root
- `src/config/experiment.rs` - Experiment config schema (JSON-based)
- `src/config/runtime.rs` - Runtime config with computed splits
- `src/experiment/mod.rs` - Experiment module root
- `src/experiment/manifest.rs` - Run manifest for reproducibility
- `src/experiment/runner.rs` - Experiment orchestrator
- `src/bin/krypto.rs` - CLI entrypoint
- `docs/TRADEOFFS.md` - This document
- `configs/example.json` - Example experiment config

---

## 1. Validation Methods

### Decision: Support Three Validation Methods

| Method | Use Case | Pros | Cons |
|--------|----------|------|------|
| `simple_split` | Quick prototyping | Fast, simple | Single test set may be unrepresentative |
| `walk_forward` | Time series validation | Expanding window simulates real deployment | May miss early regime changes |
| `cpcv` | Regime-robust validation | Tests across multiple regimes | Requires more data, slower |

**Recommendation:** Use `walk_forward` for most cases. Use `cpcv` when you have 3+ years of data spanning multiple market regimes.

### Purge Gap & Embargo

**Decision:** Default `purge_gap: 0`, strongly recommend setting it.

**Rationale:** Without a purge gap, the test set may contain information leakage from the train set (e.g., if your strategy uses a 50-period moving average, the first 50 bars of the test set are influenced by training data).

**Recommendation:** Set `purge_gap` to at least 2x your longest lookback period.

---

## 2. Transaction Costs

### Decision: Conservative Defaults (0.1% fee + 5 bps slippage)

**Rationale:** Optimistic costs are the #1 source of false-positive backtests. Real execution includes:
- Exchange fees (0.1% for Binance spot, less with BNB)
- Slippage (varies by liquidity, 5-10 bps typical for majors)
- Spread cost (implicit)
- Market impact (larger positions)

**Tradeoff:** Conservative costs will reduce reported returns but improve realism.

### Fee Calculation

**Current Approach:** Apply `fee_pct * 2.0` to round-trip trades.

**Known Issue:** Trailing stops currently apply the same `* 2.0` even though entry fee was already paid. This slightly overstates fees for stop-outs. Acceptable for now - better to overestimate costs than underestimate.

---

## 3. Position Sizing

### Decision: Fixed Fractional by Default, Kelly Optional

**Rationale:** 
- Kelly criterion is optimal in theory but requires accurate win rate and payoff ratio estimates
- In practice, these are uncertain in crypto markets
- Half-Kelly or quarter-Kelly is safer

**Implementation:**
- `kelly_fraction: 0` → Use `position_fraction` (fixed sizing)
- `kelly_fraction: 0.5` → Use 50% of Kelly-optimal position
- `kelly_fraction: 1.0` → Full Kelly (not recommended)

**Guardrail:** Config validation rejects `kelly_fraction > 0.25`.

---

## 4. Bias Control

### Multiple Comparison Problem

**Issue:** Running 160 parameter combinations × 11 strategies × 15 symbols = 26,400 tests. By chance alone, ~5% will show significant results.

**Mitigation Strategies:**
1. **Threshold filtering:** Require minimum Sharpe, profit factor, and trades
2. **Robustness check:** Test/train Sharpe ratio must exceed threshold
3. **Out-of-sample holdout:** Keep a final "embargo" period never used in training
4. **Bonferroni correction:** (Future) Adjust p-values for multiple comparisons

### Look-Ahead Bias

**Guardrails:**
- All indicators use only data available at time T
- No future leak in feature engineering
- Purge gap between train/test

### Survivorship Bias

**Known Gap:** We only test currently-listed symbols. Delisted/failed coins are not in the dataset.

**Mitigation:** None currently. This overstates returns for strategies that would have held failing assets.

---

## 5. Regime Handling

### Current Approach: Single Regime (No Detection)

**Rationale:** Regime detection (bull/bear/sideways) adds complexity and can be wrong. Starting simple.

**Future Enhancement:** Add `RegimeDetector` integration:
- Detect regime from EMA50/200 relationship + Bollinger width
- Use different strategy parameters per regime
- Report performance broken down by regime

### Non-Stationarity

**Issue:** Market characteristics change over time (volatility, correlation, trendiness).

**Current Mitigation:** Walk-forward validation implicitly tests strategy on different time periods.

**Future:** Consider rolling parameter updates or adaptive strategies.

---

## 6. Backtest Engine

### Long-Only by Default

**Decision:** Backtester supports shorts in code, but most testing uses long-only.

**Rationale:**
- Shorting crypto has unlimited downside and liquidation risk
- Funding rates can be expensive
- Most retail strategies are long-only

### Trailing Stop Implementation

**Current:** Fixed percentage trailing stop, evaluated on bar's `low` (for longs) or `high` (for shorts).

**Limitation:** Uses the bar's extreme price, which may not reflect actual fill. In reality:
- Stop would trigger somewhere between open and low
- Fill price depends on order type (market vs limit)
- Slippage applies to stop fills too

**Acceptable For:** Initial research. Add more sophisticated stop modeling for production.

### Kelly Fraction Computation

**Current:** Simplified Kelly = `win_rate - (1 - win_rate) / payoff_ratio`

**Known Limitations:**
- Assumes normal distribution of returns (crypto is fat-tailed)
- Doesn't account for correlation between trades
- Uses historical win rate which may not persist

---

## 7. Experiment Output

### Run Manifest

**Decision:** Every run produces a `manifest.json` capturing:
- Full configuration
- Git commit hash
- Environment details
- Results summary
- Output file list

**Rationale:** Enables reproducibility and comparison. If a strategy looked good in run A but failed in run B, we can diff the manifests to understand why.

### Output Structure

```
experiments/
├── btc_trend_following__20260225_130000/
│   ├── manifest.json        # Run metadata
│   ├── config.json          # Config snapshot
│   ├── metrics.json         # Aggregated metrics
│   ├── equity_curve.csv     # Equity over time
│   └── trades.csv           # Trade log
└── ...
```

---

## 8. Future Enhancements

### Not Yet Implemented

| Feature | Priority | Notes |
|---------|----------|-------|
| Live paper trading | High | Alpha gate before real money |
| Multi-asset portfolio | Medium | Correlation-aware position sizing |
| Regime-weighted ensemble | Medium | Already implemented, not integrated |
| Triple barrier labeling | Low | For ML-based strategies |
| Fractional differentiation | Low | For stationary features |
| Combinatorial purged CV | Medium | More robust than walk-forward |

### Defer Decision Points

- **Database for results:** Currently file-based. Consider SQLite or PostgreSQL if runs exceed thousands.
- **Parallel execution:** Currently sequential. Rayon integration for multi-symbol backtests.
- **Cloud execution:** Not needed for research phase.

---

## 9. Lessons from v1/v2

### What Failed

1. **PLS regression** - Wrong model for non-stationary, non-linear crypto data
2. **GA optimization** - Overfit to backtest period, no out-of-sample validation
3. **Complex config** - Too many moving parts before validating core hypothesis
4. **No baseline** - Never compared to buy-and-hold

### What Worked

1. **Polars for data** - Fast, ergonomic DataFrame library
2. **Config-driven approach** - YAML config is the right pattern
3. **SignalGenerator trait** - Clean abstraction for strategies
4. **FeatureEngine** - Solid indicator library

### Core Lesson

> **Validate the hypothesis before building complexity.**
> 
> The fundamental question (do technical indicators predict price direction?) must be answered before adding ML, ensembles, or execution engines.

---

## Changelog

| Date | Change |
|------|--------|
| 2026-02-25 | Initial document created during foundation build |
