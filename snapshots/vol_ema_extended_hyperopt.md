# VOL_LOOKBACK Extended Hyperopt (EMA vs SMA, 1-100)

**Date:** 2026-04-17
**Strategy:** Turtle+Chandelier (frozen: EP=21, CHAND=20, ATR=24, CAP=3, HM=45)
**Phase 1:** NoDOGE 6-window sweep, VL=1-100, both SMA and EMA (200 configs)
**Phase 2:** 9-universe validation of top-3 configs

## Winner (Most Robust Across 9 Universes)

- **VL=55, SMA** — 9-way avg Sharpe=6.9993, avg Ret=+123.5%, pass 45/54
- **Baseline VL=1 SMA** — NoDOGE avg Sharpe=7.5007
## Phase 2: 9-Universe Validation
## Phase 2: 9-Universe Validation

| Universe | VL | Method | Pass | Avg Sharpe | Avg Ret |
|---|---|---|---|---|---|
| Base5 | 53 | SMA | 6/6 | 9.5106 | +185.7% |
| Base5 | 54 | SMA | 6/6 | 9.5106 | +185.7% |
| Base5 | 55 | SMA | 6/6 | 9.5106 | +185.7% |
| NoDOGE | 53 | SMA | 6/6 | 11.4468 | +194.7% |
| NoDOGE | 54 | SMA | 6/6 | 11.4468 | +194.7% |
| NoDOGE | 55 | SMA | 6/6 | 11.4468 | +194.7% |
| Legacy4 | 53 | SMA | 5/6 | 6.0369 | +63.9% |
| Legacy4 | 54 | SMA | 5/6 | 6.0369 | +63.9% |
| Legacy4 | 55 | SMA | 5/6 | 6.6400 | +68.9% |
| Legacy5BNB | 53 | SMA | 5/6 | 8.4262 | +98.2% |
| Legacy5BNB | 54 | SMA | 5/6 | 8.4262 | +98.2% |
| Legacy5BNB | 55 | SMA | 5/6 | 8.4262 | +98.2% |
| OldGuardNoBNB | 53 | SMA | 5/6 | 5.2535 | +60.2% |
| OldGuardNoBNB | 54 | SMA | 5/6 | 5.2535 | +60.2% |
| OldGuardNoBNB | 55 | SMA | 5/6 | 5.2535 | +60.2% |
| LargeCaps5 | 53 | SMA | 6/6 | 9.3770 | +185.5% |
| LargeCaps5 | 54 | SMA | 6/6 | 9.3770 | +185.5% |
| LargeCaps5 | 55 | SMA | 6/6 | 9.3770 | +185.5% |
| Legacy3 | 53 | SMA | 4/6 | 4.2302 | +84.7% |
| Legacy3 | 54 | SMA | 4/6 | 4.1339 | +83.5% |
| Legacy3 | 55 | SMA | 4/6 | 4.1339 | +83.5% |
| LowVolume5 | 53 | SMA | 4/6 | 3.3956 | +152.7% |
| LowVolume5 | 54 | SMA | 4/6 | 3.3425 | +152.7% |
| LowVolume5 | 55 | SMA | 4/6 | 3.3425 | +152.7% |
| OldGuard4 | 53 | SMA | 3/6 | 4.5840 | +79.0% |
| OldGuard4 | 54 | SMA | 3/6 | 4.5840 | +79.0% |
| OldGuard4 | 55 | SMA | 4/6 | 4.8629 | +82.0% |

## Charts
- `charts/vol_ema_comparison.png` — equity curve comparison
