# T31 Donchian Sleeve Walk-Forward

**Hypothesis:** Donchian entry is too sparse as a replacement, but may diversify Turtle as a 25% high-conviction sleeve.

**Method:** Base5, 252/252 walk-forward, Turtle(75%) + Donchian(25%), same production dual exit and taker fees.

| Metric | Turtle | Donchian | 75/25 Sleeve | Sleeve vs Turtle |
|---|---:|---:|---:|---:|
| Pass Rate | 6/6 (100%) | 5/6 (83%) | 6/6 (100%) | +0.0 pp |
| Avg Sharpe | +3.907 | +6.465 | +4.767 | +0.859 |
| Avg Return | +3680.9% | +533.3% | +2283.8% | -1397.2% |

## Decision

**CANDIDATE.** Sleeve passes the T31 guardrail: Sharpe drop -22.0% and pass-rate delta -0.0 pp. Needs broader 9-universe validation before promotion.

## Per-Window Results

| Window | Turtle | Turtle Sh | Turtle Ret | Donchian | Don Sh | Don Ret | Sleeve | Sleeve Sh | Sleeve Ret | ΔSh |
|---|---|---:|---:|---|---:|---:|---|---:|---:|---:|
| W00 | ✅ | +6.93 | +1172.7% | ✅ | +6.06 | +571.3% | ✅ | +6.99 | +992.3% | +0.06 |
| W01 | ✅ | +4.84 | +20425.8% | ✅ | +4.68 | +1628.8% | ✅ | +5.08 | +12128.6% | +0.24 |
| W02 | ✅ | +5.76 | +333.8% | ✅ | +9.97 | +345.3% | ✅ | +7.06 | +341.4% | +1.30 |
| W03 | ✅ | +4.10 | +130.1% | ✅ | +7.15 | +415.7% | ✅ | +5.14 | +182.4% | +1.04 |
| W04 | ✅ | +1.26 | +17.1% | ✅ | +10.85 | +239.1% | ✅ | +3.88 | +53.3% | +2.62 |
| W05 | ✅ | +0.55 | +6.2% | ❌ | +0.08 | -0.3% | ✅ | +0.45 | +4.7% | -0.10 |
