# Cross-Market Equity Walk-Forward

FROZEN crypto params: EP=21, CHAND(28,2.0), ATR(25,2.0), HOLD_MAX=45. NOT re-optimized for equities.

| Asset | Pass Rate | Avg Return | Avg Sharpe | Worst DD | Total Trades |
|---|---|---|---|---|---|
| SPY | 15/24 (62%) | +4.6% | 5.73 | 8.5% | 168 |
| QQQ | 14/24 (58%) | +4.9% | 3.90 | 17.4% | 171 |
| GLD | 12/19 (63%) | +5.1% | 4.68 | 13.5% | 131 |

**Overall: 41/67 windows passed (61%)**

## Interpretation

- SPY/QQQ/GLD pass rate ≥3/3 → **edge generalizes beyond crypto**
- SPY/QQQ/GLD pass rate 1-2/3 → **edge partially generalizes, crypto adds alpha**
- SPY/QQQ/GLD pass rate 0/3 → **crypto-only edge, regime-dependent**
