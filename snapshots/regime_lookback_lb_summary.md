# REGIME_LOOKBACK Extensive Hyperopt — Live Turtle-Only Path

**Date:** 2026-05-02
**Strategy:** Turtle-only exit (matches `src/live/bot.rs` after 2026-05-01 fix)
**Sweep:** LB ∈ [5..=200] step 1 (196 values) × 9 universes × 7 windows
**Fixed params:** EP=21, ATR(64,2.0), T=24, HOLD_MAX=12, CAP=3, VL=8, USDT hedge, fee=0.10%

## Robustness Winner: LB=45

| Metric | LB=45 (Winner) | LB=42 (Baseline) | Delta |
|--------|------------|-----------------|-------|
| Global Pass | 55/63 (87.3%) | 55/63 (87.3%) | +0 |
| Avg Sharpe | 6.188 | 6.188 | +0.0% |
| Avg Return | 73.8% | 73.8% | +0.0pp |
| Avg DD | 19.7% | 19.7% | +0.0pp |
| Total Trades | 464 | 464 | 0 |
| Positive Universes | 9/9 | 9/9 | +0 |

## Top 10 LB Values (Robustness-First)

| LB | Pass | Pct% | Sharpe | Ret% | DD% | PosUni |
|----|------|------|--------|------|-----|--------|
| 45 | 55/63 | 87.3% | 6.188 | 73.8% | 19.7% | 9/9 |
| 44 | 55/63 | 87.3% | 6.188 | 73.8% | 19.7% | 9/9 |
| 43 | 55/63 | 87.3% | 6.188 | 73.8% | 19.7% | 9/9 |
| 42 | 55/63 | 87.3% | 6.188 | 73.8% | 19.7% | 9/9 |
| 46 | 54/63 | 85.7% | 4.462 | 69.2% | 20.2% | 9/9 |
| 50 | 54/63 | 85.7% | 4.364 | 73.5% | 20.7% | 9/9 |
| 49 | 54/63 | 85.7% | 4.354 | 72.9% | 20.7% | 9/9 |
| 48 | 54/63 | 85.7% | 4.323 | 67.3% | 20.6% | 9/9 |
| 47 | 54/63 | 85.7% | 4.323 | 67.3% | 20.6% | 9/9 |
| 41 | 53/63 | 84.1% | 5.731 | 71.1% | 19.9% | 9/9 |

*Pass = windows with ≥3 trades AND Sharpe>0 / total windows.*

## Conclusion
LB=45 is the winner over the baseline LB=42. 
Update `src/live/config.rs`: `REGIME_LOOKBACK = 45`
And update `examples/live_compatible_wf.rs`: `REGIME_LOOKBACK = 45`
See `charts/comparison_chart.png` for equity curve comparison.
