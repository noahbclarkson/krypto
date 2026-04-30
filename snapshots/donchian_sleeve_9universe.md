# T31 Donchian Sleeve — 9-Universe Validation

Portfolio: Turtle 75% + Donchian 25% | 252/252 WF | 6 windows

## Global Summary

| Metric | Turtle | Sleeve | Delta |
|---|---:|---:|---:|
| Global Pass | 34/54 (63%) | 34/54 (63%) | +0.0 pp |
| Avg Sharpe | +2.145 | +2.382 | +0.237 |
| Avg Return | +532.5% | +367.6% | -164.9% |

## Per-Universe Results

| Universe | Turtle Pass | Sleeve Pass | Turtle Sh | Sleeve Sh | ΔSh | Sleeve vs Turtle |
|---|---:|---:|---:|---:|---:|---|
| Base5 | 5/6 (83%) | 5/6 (83%) | +3.329 | +3.786 | +0.457 | +0.0 pp |
| NoDOGE | 4/6 (67%) | 4/6 (67%) | +3.228 | +3.336 | +0.108 | +0.0 pp |
| Legacy4 | 4/6 (67%) | 4/6 (67%) | +2.318 | +2.561 | +0.243 | +0.0 pp |
| Legacy5BNB | 4/6 (67%) | 4/6 (67%) | +2.394 | +2.906 | +0.512 | +0.0 pp |
| OldGuardNoBNB | 5/6 (83%) | 5/6 (83%) | +1.822 | +1.872 | +0.051 | +0.0 pp |
| LargeCaps5 | 4/6 (67%) | 4/6 (67%) | +3.318 | +3.543 | +0.225 | +0.0 pp |
| Legacy3 | 3/6 (50%) | 3/6 (50%) | +0.617 | +0.931 | +0.314 | +0.0 pp |
| LowVolume5 | 3/6 (50%) | 3/6 (50%) | +2.143 | +2.142 | -0.001 | +0.0 pp |
| OldGuard4 | 2/6 (33%) | 2/6 (33%) | +0.137 | +0.361 | +0.224 | +0.0 pp |

## Decision

**REJECTED.** Sleeve global pass rate 63.0% is below the T31 guardrail 69.1% (production baseline 74.1% minus 5pp). Sharpe improves +11.0% (+2.382 vs +2.145), but the absolute pass-rate failure keeps Turtle-only as production default.

