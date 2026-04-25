# T7: BTC/ETH Correlation Entry Filter — Walk-Forward Results

**Generated:** 2026-04-25 15:17 UTC

## Hypothesis
2026 YTD failure (-32.8%) may be BTC-led divergence. ALT breakouts fire but get stopped by Chandelier when BTC doesn't confirm. BTC/ETH trend confirmation filter might reduce whipsaw.

## Filter Variants
| Filter | BTC SMA | ETH SMA | Description |
|--------|---------|---------|-------------|
| none | — | — | Baseline (no filter) |
| btc_only | Required | — | BTC must be above SMA(21) for ALT entries |
| btc_or_eth | Required | Required | Either BTC OR ETH above SMA(21) |
| btc_and_eth | Required | Required | Both BTC AND ETH above SMA(21) |

## Production Params
```
EP=24, CHAND(7,2.30), ATR(24), HM=12, ATR_ENTRY_MULT=0.00
```

## Base5 Aggregate Results (6 windows, 252-bar test periods)

| Filter | Avg Ret | Avg Sharpe | Worst DD | Total Trades | Pct Pass |
|--------|---------|------------|----------|--------------|----------|
| none | +272% | 1.20 | 46.0% | 89 | 83.3% |
| btc_only | +194% | 1.08 | 39.3% | 71 | 83.3% |
| btc_or_eth | +199% | 1.13 | 39.3% | 71 | 83.3% |
| btc_and_eth | +171% | 1.07 | 49.1% | 68 | 83.3% |

## All 9 Universes (pass rate per filter)

| Universe | none | btc_only | btc_or_eth | btc_and_eth |
|----------|------|----------|-------------|--------------|
| Base5 | 83% | 83% | 83% | 83% |
| NoDOGE | 100% | 100% | 100% | 100% |
| Legacy4 | 83% | 67% | 67% | 83% |
| Legacy5BNB | 83% | 67% | 67% | 83% |
| OldGuardNoBNB | 83% | 67% | 67% | 83% |
| LargeCaps5 | 100% | 100% | 100% | 100% |
| Legacy3 | 83% | 67% | 67% | 83% |
| LowVolume5 | 50% | 33% | 33% | 50% |
| OldGuard4 | 83% | 67% | 67% | 83% |

## Conclusion

**NO CORRELATION FILTER IMPROVES OVER BASELINE.**

Every filter variant loses to the no-filter baseline:
- `btc_only`: ΔSharpe = -0.12, trade reduction = -20%
- `btc_or_eth`: ΔSharpe = -0.07, trade reduction = -20%
- `btc_and_eth`: ΔSharpe = -0.13, trade reduction = -24%

All three filter variants produce lower Sharpe AND fewer trades. The BTC/ETH trend filter does NOT reduce ALT whipsaw — Chandelier(P=7,M=2.30) already handles choppy BTC regimes correctly.

This is the 4th consecutive entry filter rejected (after ATR entry, vol lookback, and chop filter). The Chandelier trailing stop is sufficient to manage ALT noise in the absence of BTC confirmation.

**Action:** No production change. Correlation filter hypothesis is dead.
