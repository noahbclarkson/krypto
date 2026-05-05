# T59: Turtle-Only Daily Equity

Generated: 2026-05-05 14:12 UTC

## Production Params

| Parameter | Value |
|-----------|-------|
| TURTLE_ENTRY | 21 |
| TURTLE_ATR_PERIOD | 24 |
| TURTLE_ATR_MULT | 2.00 |
| HOLD_MAX | 12 |
| POSITION_CAP | 3 |
| VOL_LOOKBACK | 92 |
| REGIME_ATR_PERIOD | 17 |
| REGIME_LOOKBACK | 42 |
| ATR_RANK_THRESHOLD | 5 |
| TAKER_FEE | 10.0bps |

## Full-History Results

| Metric | Value |
|--------|-------|
| Final equity | 176.79x (17579.0%) |
| Annualised Sharpe | 3.29 |
| Annualised return | 186.6% |
| Max drawdown | 99.5% |
| Total trades | 156 |
| Trading days | 1794 |

## Comparison

- Dual Chandelier+Turtle (progress harness): 619.9x / Sharpe 1.19
- Turtle-only (this harness): 176.79x / Sharpe 3.29

**Live bot path matches:** `examples/turtle_only_equity.rs`
