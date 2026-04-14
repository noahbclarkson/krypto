# Hyperopt Session: Chandelier Exit Multiplier and Period (2026-04-09 09:17 UTC)

## Audit: Hardcoded Parameters
Target: `Chandelier Exit` parameters: `period` and `multiplier`.
Status: Previously, we systematically optimized `period` but left `multiplier` hardcoded to `3.0` based on standard defaults.

## Optimization Execution
Method: Full grid search across 2 dimensions across 9 universes.
Sweep:
- `period`: 5, 10, 15, 20, 25, 30, 35, 40, 45, 50, 55, 60 (12 values)
- `mult`: 1.5, 2.0, 2.5, 3.0, 3.5, 4.0, 4.5, 5.0 (8 values)
Total combinations: 96 per universe × 9 universes = 864 runs across ~5 windows each (~4,320 backtests).

### Key Insights:
- **Winner: period=45, mult=2.5**
  - Found the most robust combination across all 9 universes with the highest average Out-of-Sample (OOS) Sharpe ratio.
- **Observation**: The optimal `multiplier` (2.5) is tighter than the assumed 3.0, while the `period` (45) is longer. This suggests a longer lookback for ATR volatility assessment combined with a slightly tighter trailing stop yields the best risk-adjusted performance across varied crypto market conditions.
- **Drawdown Control**: The Chandelier Exit variants continue to show higher Max Drawdowns compared to the `Fixed21` baseline on the easiest universes (like Base5), but they successfully manage trade durations dynamically.

## Updates
- Validated the comprehensive parameter grid.
- Generated comparative equity curves.

## Chart Export
- Saved `chandelier_mult_comparison.png` plotting the Baseline, Winner, and Runner-ups to `charts/chandelier_mult_comparison.png`.
- Output directory: `snapshots/` contains full CSVs including `chandelier_mult_ranked.csv`.
