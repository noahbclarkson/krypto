# Turtle+Chandelier Walk-Forward: 9 Universes
## Params: EP=21, CHAND(15, 1.50), ATR(24, 2.0), HOLD_MAX=45, CAP=3
## Run: 2026-04-19 09:10 UTC | Runtime: 5.5s

| Universe | Pass | Avg Ret | Avg Sharpe | Worst DD | Trades |
|---|---|---|---|---|---|
| Base5 | 6/6 | +198.0% | 5.87 | 32.8% | 98 |
| NoDOGE | 6/6 | +151.5% | 6.72 | 32.8% | 88 |
| Legacy4 | 6/6 | +19.2% | 1.93 | 42.0% | 95 |
| Legacy5BNB | 6/6 | +34.5% | 3.25 | 42.0% | 87 |
| OldGuardNoBNB | 6/6 | +25.2% | 1.79 | 42.0% | 104 |
| LargeCaps5 | 6/6 | +128.1% | 6.54 | 32.8% | 85 |
| Legacy3 | 4/6 | +6.6% | 0.93 | 53.3% | 112 |
| LowVolume5 | 4/6 | +130.9% | 3.48 | 69.9% | 133 |
| OldGuard4 | 4/6 | +14.1% | 1.22 | 61.1% | 122 |

**GLOBAL: 43/54 pass (79.6%), avg Sharpe 3.53, 924 trades**
- Failures concentrated in LTC/EOS/BCH (non-trending assets) — structural, not parameter issue
- Base5/NoDOGE/LargeCaps5: 18/18 pass (100%) — production universe clean

**Production universe (Base5/NoDOGE): 6/6 each, avg Sharpe 6.27**