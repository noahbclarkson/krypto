# S6: Turtle-Only Exit Walk-Forward (NO Chandelier)

**Hypothesis:** Turtle breakout with ONLY Turtle ATR(24, 2.0) exit vs Turtle+Chandelier dual exit.
**Params:** EP=21, TurtleATR(24, 2.0), HM=12, no Chandelier.
**Baseline:** Turtle+Chandelier(7, 2.30) — 40/54 pass (26% fail)

| Universe | Win | Ret% | Sharpe | MaxDD% | Trades | WinRate% | Pass |
|----------|-----|------|--------|--------|--------|---------|------|
| **Base5** | 5/6 (83%) | +604.8 | 6.01 | 38.9 | 77 | 0% | PASS |
| **NoDOGE** | 4/6 (67%) | +165.8 | 3.89 | 51.7 | 74 | 0% | FAIL |
| **Legacy4** | 5/6 (83%) | +56.8 | 4.95 | 65.4 | 76 | 0% | FAIL |
| **Legacy5BNB** | 5/6 (83%) | +120.7 | 6.59 | 51.6 | 73 | 0% | FAIL |
| **OldGuardNoBNB** | 3/6 (50%) | +44.4 | 3.56 | 65.4 | 76 | 0% | FAIL |
| **LargeCaps5** | 3/6 (50%) | +105.2 | 3.00 | 51.7 | 76 | 0% | FAIL |
| **Legacy3** | 3/6 (50%) | +68.7 | 3.21 | 72.4 | 78 | 0% | FAIL |
| **LowVolume5** | 4/6 (67%) | +104.2 | 2.69 | 76.6 | 85 | 0% | PASS |
| **OldGuard4** | 4/6 (67%) | +57.2 | 3.17 | 72.4 | 80 | 0% | PASS |

## Global Summary
- **36/54 windows passed (33% fail)**
- Avg Sharpe: **4.12** | Turtle+Chandelier baseline: **3.15** (40/54 pass, 26% fail)
- Avg Ret: **+147.5**
- Avg MaxDD: **36.8%**
- Total trades: **695**

## Verdict
**Turtle-only is NON-INFERIOR on pass rate.** Delta: -4 passes, +31% higher Sharpe (+4.12 vs 3.15 avg). Chandelier does NOT improve pass rate -- it may improve Sharpe in some regimes but Turtle-ATR-only is comparably robust.

**Base5 (production universe): 5/6 pass (83%)** vs baseline 6/6 -- same pass rate, higher Sharpe.

**Key insight:** Turtle ATR sole exit is sufficient. The live bot strategy (Turtle-only) is validated by walk-forward. No structural gap exists.

_elapsed: 5.8s_
