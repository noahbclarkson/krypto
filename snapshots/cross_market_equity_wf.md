# Cross-Market Equity Walk-Forward

FROZEN crypto params: EP=21, CHAND(28,2.0), ATR(25,2.0), HOLD_MAX=45. NOT re-optimized for equities.

| Asset | Pass Rate | Avg Return | Avg Sharpe | Worst DD | Total Trades |
|---|---|---|---|---|---|
| SPY | 15/17 (88%) | +6.3% | 6.34 | 7.3% | 119 |
| QQQ | 13/17 (76%) | +6.5% | 5.83 | 15.8% | 128 |
| GLD | 9/17 (53%) | +3.6% | 4.12 | 15.8% | 121 |

**Overall: 37/51 windows passed (73%)**

## Interpretation

- SPY/QQQ/GLD pass rate ≥3/3 → **edge generalizes beyond crypto**
- SPY/QQQ/GLD pass rate 1-2/3 → **edge partially generalizes, crypto adds alpha**
- SPY/QQQ/GLD pass rate 0/3 → **crypto-only edge, regime-dependent**
