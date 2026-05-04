# Hyperparameter Optimization: USDT Hedge Percentile Threshold
**Date:** 2026-05-04 08:47 UTC

## Mission
Audit the hardcoded `0.50` (50th percentile) activation threshold for the high-volatility USDT hedge overlay. The logic reduces position size by 30% (`size_mult = 0.70`) when BTC's current 21-day ATR exceeds this threshold of its 252-bar ATR history. This magic number had never been systematically optimized.

## Execution
- **Parameter Isolated:** `hedge_pct_threshold`
- **Range Tested:** `0` (disabled) to `95` (95th percentile) in steps of `5`. Total of 20 values.
- **Validation:** Walk-forward across 9 universes × 7 windows (1,260 total windows evaluated).
- **Harness:** `examples/hedge_threshold_hyperopt.rs` 

## Results
- **T=50 (50th percentile)** emerged as the robustness winner globally:
  - Pass Rate: **88.9%** (56/63 windows passed)
  - Avg Sharpe: **5.171**
  - Avg Return: **112.4%**
  - Coverage: **9/9** universes positive.
- Disabling the overlay (T=0) resulted in lower robustness: 54/63 pass rate (85.7%), lower Sharpe (4.455), and significantly worse maximum drawdown metrics.
- Other thresholds like T=10 and T=15 tied in pass rate but had lower Sharpe ratios (4.919 and 5.028 respectively).
- The overlay acts effectively to preserve capital when volatility is elevated above the historical median.

## Conclusion
**Confirmed Default.** The previously hardcoded `0.50` threshold is empirically optimal across 9 universes of out-of-sample data. It provides the highest pass rate and Sharpe ratio of all values tested. 

No code changes are required as the existing production code already implements this optimum.

## Artifacts
- **Harness:** `examples/hedge_threshold_hyperopt.rs`
- **Chart:** `charts/comparison_chart.png` (Pass rate vs Sharpe across threshold spectrum & Base5 equity curves)
- **Raw Data:** `snapshots/hedge_threshold_sweep.csv`, `snapshots/hedge_threshold_summary.csv`, `snapshots/hedge_threshold_equity.csv`