# Hyperopt Session — 2026-04-08 09:25 UTC

## Audit: Hardcoded Parameters
Target: `TURTLE_ENTRY` (the Donchian Channel breakout lookback period)
Found hardcoded as 20 bars across multiple strategies (ad_accumulation_walk_forward.rs, vol_regime_position_sizing_benchmark.rs, cross_family_drawdown_attribution.rs, etc.).
Original rationale: 20-day breakout is the classic Turtle strategy parameter from the 1980s commodities markets. Never rigorously optimized for modern 24/7 crypto markets.

## Optimization Execution
Method: Full integer sweep from 5 to 100 bars across 9 universes.
Criteria: "Score" ranking (Pass count across 9 universes + Average Sharpe). 
Goal: Find the most globally robust lookback period, not just an overfit peak on one universe.

Results:
- **Baseline (20 bars):** Score 9.16, Sharpe 0.16, Passed 9/9 universes.
- **Winner (65 bars):** Score 9.21, Sharpe 0.21, Passed 9/9 universes.
- **Runner Up 1 (67 bars):** Score 9.21 (ties 65, slightly lower raw return).
- **Runner Up 2 (63 bars):** Score 9.20.

**Insight:** The classic 20-day Turtle breakout is too fast/noisy for crypto daily bars. A slower ~65-day (roughly two-month) lookback filters out fake-outs and captures major sustained regime shifts much better.

## Updates
Generated time-series equity curves comparing the 20-bar baseline vs the 65-bar winner and runner-ups. 
Saved chart to `krypto/charts/comparison_chart.png`.
Documented the hyperparameter finding. This implies future examples should use 65 for TURTLE_ENTRY rather than 20 to achieve superior risk-adjusted returns across broad asset baskets.
