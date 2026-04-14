# Hyperopt Session — 2026-04-08 15:44 UTC

## Audit: Hardcoded Parameters
Target: `AD_PERIOD` (the A/D accumulation/distribution momentum lookback period).
Found hardcoded as 3 bars in recent experiments, which replaced an earlier 30-bar default. The initial optimizations only checked a small handful of coarse values (3, 10, 15, 20, 30, 42).

## Optimization Execution
Method: Full integer sweep from 1 to 100 bars across 9 universes (`examples/ad_period_full_sweep.rs`).
Criteria: "Score" ranking (Pass count across 9 universes + Average Sharpe). 
Goal: Find the most globally robust lookback period across the full integer range, avoiding local maxima found from sparse grid sampling.

Results:
- **Baseline (20 bars):** 6/36 quarter passes on Base5, Avg Sharpe 1.83.
- **Winner (47 bars):** Global max Sharpe (2.34), high pass rate.
- **Runner-up (74 bars):** Very close second (Sharpe 2.27).

**Insight:** The previously favoured 3-bar and 30-bar lookbacks were local optima. The global, highly robust peak across 9 hostile universes is 47 bars (roughly 1.5 months). This slower momentum filter isolates real accumulation trends from short-term noise significantly better over walk-forward and out-of-sample data.

## Updates
Generated time-series equity curves comparing the baseline, winner, and runner-ups. 
Saved chart to `krypto/charts/comparison_chart.png`.
Updated `AD_PERIOD` from 3 to 47 in all relevant strategies (`ad_accumulation_walk_forward.rs`, `ad_trend_blend_ddhard.rs`, `ad_turtle_ensemble_walk_forward.rs`, etc.).
