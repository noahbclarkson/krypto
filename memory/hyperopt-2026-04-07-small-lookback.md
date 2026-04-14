# Hyperparameter Optimization Report — 2026-04-07

## Session Focus
Systematic optimization of the `FactorSmallByDollarVol` lookback period (previously hardcoded to 63).

## 1. Audit: Hardcoded Parameter Found
**Parameter:** Inverse dollar-volume lookback period.
**Current Value:** 63 days (hardcoded magic number).
**Location:** `src/features/indicators.rs` (used in factor computation) and local overrides in experiments.
**Evidence:** The lookback length defines the "smallness" factor but was never systematically tested across a wide range.

## 2. Systematic Optimization Sweep
**Harness:** Created `examples/small_lookback_sweep.rs` based on the factor sleeve overlay benchmark.
**Range Tested:** 5 to 150 days (step=5).
**Conditions:** Signal at close, next-open execution, 21-bar hold, 0.1% taker fee, capped top-3 book, 9 harsh universes.
**Export:** Added time-series equity curve export for graphing.

## 3. Results & Findings
The sweep revealed that while 63 was an okay value, it is not the global optimum.
The relationship between lookback and return is somewhat flat in the middle but peaks earlier than 63 on many universes.

**Top Lookback (Winner): 15 days**
- **Base5 Sharpe:** 7.50 (vs baseline 65 at 7.21)
- **Base5 Return:** +42,925% (vs baseline 65 at +36,929%)
- **Base5 Max DD:** 16.5% (vs baseline 65 at 16.2%)
- The peak represents a faster adaptation to volume/liquidity regime changes. The previous default (63, approx 2-3 months) lagged behind recent volume shifts.

## 4. Update Applied
The stable default for `FactorSmallByDollarVol` lookback is updated to **15** from **63**. This aligns the factor better with the faster 3-day A/D momentum signal found earlier today, making the overall portfolio more responsive.

## 5. Artifacts
- **Chart:** `/home/ubuntu/.openclaw/workspace-krypto/comparison_chart.png` (Log-scale equity curves comparing lb=15 winner against runner-ups and the lb=65 baseline proxy).
- **Data:** `/home/ubuntu/.openclaw/workspace-krypto/krypto/snapshots/small_lookback_sweep_latest.csv` and `small_lookback_sweep_equity.csv`
