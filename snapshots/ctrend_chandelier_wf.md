# CTREND + Chandelier Walk-Forward: 9 Universes

**Entry:** CTREND multi-horizon price+volume momentum
**Exit:** Chandelier(11, 2.25) OR Turtle_ATR(24, 2) — first fires wins
**252/252 train/test walk-forward, 0.1% taker each side**

| Universe | Pass | Avg Ret | Avg Sharpe | Worst DD | Trades |
|---|---|---|---|---|---|
| Base5 | 5/6 | +167.4% | 2.78 | 67.8% | 101 |
| NoDOGE | 4/6 | +67.9% | 2.39 | 67.8% | 89 |
| Legacy4 | 3/6 | +42.1% | 0.92 | 76.8% | 95 |
| Legacy5BNB | 3/6 | +61.7% | 2.47 | 62.5% | 90 |
| OldGuardNoBNB | 3/6 | +37.1% | 1.57 | 68.9% | 95 |
| LargeCaps5 | 4/6 | +71.7% | 2.46 | 53.5% | 88 |
| Legacy3 | 3/6 | +89.3% | 3.39 | 61.0% | 93 |
| LowVolume5 | 2/6 | +38.8% | 1.17 | 84.0% | 111 |
| OldGuard4 | 3/6 | +62.9% | 3.41 | 64.6% | 96 |

**GLOBAL: 30/54 pass (44% fail), avg Sharpe 2.2833, 858 trades**

## Comparison to Turtle+Chandelier baseline

| Metric | Turtle+Chandelier | CTREND+Chandelier |
|---|---|---|
| Global pass rate | ~80% (43/54) | see above |
| Avg Sharpe | ~4.78 | see above |
| Entry signal | Turtle breakout (EP=21) | CTREND multi-horizon |

If CTREND+Chandelier pass rate ≥ Turtle+Chandelier → CTREND promoted to second signal family candidate.
