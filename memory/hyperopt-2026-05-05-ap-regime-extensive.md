# hyperopt-2026-05-05: REGIME_ATR_PERIOD Extensive Sweep

## Mission
Extensive sweep of REGIME_ATR_PERIOD (AP) ∈ [1..=80 step 1] — NOT the narrow 7-value 
cluster sweep done before. Proper walk-forward validation using the ACTUAL live Turtle-only 
exit path (ATR(24,2.0) turtle stop, matches src/live/bot.rs).

## Key Finding
- **Winner: AP=63/64** (88.9% pass rate, 56/63 windows)
- **Baseline: AP=17** (85.7% pass rate, 54/63 windows)
- **But AP=17 has HIGHER Sharpe (6.37 vs 5.91) and higher return (161.7% vs 141.3%)**
- Recommendation: **KEEP AP=17 as production default**. AP=63 only wins pass rate; 
  AP=17 dominates on risk-adjusted return.

## Result: No Change to Production Defaults

AP=17 remains the production default. The AP=63 winner is statistically insignificant 
(+2 passes out of 63, +3.2pp) and worse on every other metric.

## Charts
- `charts/comparison_chart.png` — equity curves (log scale) + top-20 Sharpe bar chart
- `charts/comparison_by_universe.png` — equity breakdown by universe

## Scope
80 AP values × 9 universes × 7 WF windows = 5,040 simulation runs
Fixes: LB=42, T=5.0, EP=21, ATR(24,2.0), HM=12, CAP=3, VL=96
