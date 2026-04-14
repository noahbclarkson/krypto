# Hyperopt Session: Dynamic Trend Parameters (2026-04-09)

## Audit: Hardcoded Parameters
Target: `DynamicTrend` strategy parameters in `src/algo/strategies.rs`
- `ema_fast`: 50
- `ema_slow`: 200
- `rsi_filter`: 50.0
These values were hardcoded default "magic numbers" typically used in standard trend-following without extensive empirical testing on the exact universe.

## Optimization Execution
Method: Systematic parameter sweep across the logical integer range.
Tested ranges:
- `ema_fast`: [10, 20, 30, 40, 50]
- `ema_slow`: [50, 100, 150, 200, 250]
- Exported resulting equity curves mapping portfolio growth over test duration.

## Key Insights
- The baseline (50/200) serves as a relatively slow trend follower.
- Finding optimal configurations (e.g. 10/50) captures momentum shifts faster while preserving drawdowns at acceptable limits.
- Resulting equity curves emphasize that tighter, more responsive trend parameters heavily outperform the sluggish default 50/200 on crypto assets due to their highly volatile regimes.

## Updates
- Successfully updated `comparison_chart.png` visualizing the trajectory differences between Baseline, Winner, and Runner-Up.
- Changes to source code should prioritize tightening `ema_fast` and `ema_slow` to the winning configuration `(10, 50)`.