# T64: Regime Sharpe Decomposition

Generated: 2026-05-05 09:16 UTC

## Method

- Universe: Base5 production symbols: `BTCUSDT, ETHUSDT, SOLUSDT, XRPUSDT, DOGEUSDT, ADAUSDT`
- Strategy: Turtle-only live path, EP=21, ATR(24,2.00), HOLD_MAX=12, CAP=3, VL=96, fee=10.0bps/side
- Production gate: BTC ATR_RANK(AP=17, LB=42, T=5.0); control uses T=0 no gate
- Daily Sharpe uses calendar-day returns including flat/cash days; trade PnL is allocated geometrically across held bars for attribution.
- This attribution curve is not a replacement for T59's exact event-compounded headline (112.27x / Sharpe 3.14); small equity differences can occur because T64 spreads trade PnL across held days to classify regimes.
- Bull/bear: BTC 21d return >/< 0. Chop/trend: BTC 21d realised vol bottom/top quartile over the test period (q25=0.0228, q75=0.0386).

## Headline

| Strategy | Equity | Sharpe | MaxDD | Trades |
|----------|--------|--------|-------|--------|
| prod_atr_rank_5 | 114.19x | 1.68 | 51.2% | 154 |
| no_gate_t0 | 206.95x | 1.75 | 45.9% | 189 |

## Production ATR_RANK=5

| Regime | Days | Nonzero days | Trades | Equity | Sharpe |
|--------|------|--------------|--------|--------|--------|
| all_days | 1794 | 604 | 154 | 114.19x | 1.68 |
| bull_21d | 1003 | 469 | 110 | 6.33x | 1.80 |
| bear_21d | 791 | 135 | 44 | 18.04x | 1.80 |
| chop_vol_q1 | 449 | 102 | 22 | 1.38x | 1.07 |
| trend_vol_q4 | 449 | 140 | 39 | 1.76x | 1.16 |

## No-gate control T=0

| Regime | Days | Nonzero days | Trades | Equity | Sharpe |
|--------|------|--------------|--------|--------|--------|
| all_days | 1794 | 762 | 189 | 206.95x | 1.75 |
| bull_21d | 1003 | 587 | 133 | 12.42x | 1.88 |
| bear_21d | 791 | 175 | 56 | 16.66x | 1.74 |
| chop_vol_q1 | 449 | 180 | 44 | 2.84x | 1.71 |
| trend_vol_q4 | 449 | 163 | 40 | 1.33x | 0.70 |

## Interpretation

- Production regime asymmetry: bull Sharpe 1.80, bear Sharpe 1.80, chop Sharpe 1.07, trend-vol Sharpe 1.16.
- ATR_RANK=5 vs no gate: full equity 114.19x vs 206.95x; full Sharpe 1.68 vs 1.75; bear Sharpe 1.80 vs 1.74; bull Sharpe 1.80 vs 1.88.
- This is attribution, not a new hyperopt. T=5 remains production because high-threshold ATR_RANK variants already failed held-out validation; this control checks whether the tiny low-vol gate is materially helping or just reducing sample size.
