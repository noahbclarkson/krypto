# T104 Top-Winner Mechanism Decomposition

**Status:** GENERATED from `snapshots/live_bot_exact_trades.csv` by `examples/t104_top_winner_decomposition.rs`. This is an understanding/regression artifact, not a filter or parameter sweep.

## Executive finding

Top-10 trades contribute **0.927 log equity** out of total **1.014** = **91.4%** of compounded log return. The corrected mechanism is: **early rebound/continuation inside still-damaged BTC long-term regimes**, not entries after a negative BTC 21d return.

Important correction versus the 2026-05-11 prose note: `btc_21d_return` is positive for every current top-10 entry. The negative values quoted there correspond to `btc_sma50_over_sma200`, a long-term trend-damage proxy.

## Summary stats

- BTC 21d return positive at entry: **10/10** (avg +15.5%)
- BTC SMA50/SMA200 proxy below zero: **8/10**
- BTC ATR percentile < 24: **8/10**; < 5: **0/10**
- Held <= 3 bars: **5/10** (avg hold 6.5 bars)
- Hedge active on trade: **0/10**
- Regime counts: bull_continuation=2, early_rebound_in_bear=8
- Exit counts: HOLD_MAX=1, TURTLE_ATR=9

## Top-10 roster

| Rank | Symbol | Entry | Exit | Hold | Exit | Trade ret | Position | Hedge | BTC regime label | BTC 21d ret | SMA50/SMA200 proxy | BTC ATR pct | Log contrib |
|---:|---|---|---|---:|---|---:|---:|---|---|---:|---:|---:|---:|
| 1 | SOLUSDT | 2023-01-11 | 2023-01-14 | 3 | TURTLE_ATR | +48.0% | 33.3% | no | early_rebound_in_bear | +6.6% | -13.5% | 14.6% | 0.149 |
| 2 | DOGEUSDT | 2022-10-28 | 2022-10-29 | 1 | TURTLE_ATR | +45.1% | 33.3% | no | early_rebound_in_bear | +5.4% | -20.9% | 22.0% | 0.140 |
| 3 | XRPUSDT | 2021-07-28 | 2021-08-11 | 14 | TURTLE_ATR | +37.8% | 33.3% | no | early_rebound_in_bear | +18.2% | -22.4% | 12.2% | 0.119 |
| 4 | SOLUSDT | 2021-07-30 | 2021-08-14 | 15 | HOLD_MAX | +36.3% | 33.3% | no | early_rebound_in_bear | +24.8% | -22.2% | 19.5% | 0.114 |
| 5 | SOLUSDT | 2021-08-27 | 2021-08-30 | 3 | TURTLE_ATR | +25.2% | 33.3% | no | early_rebound_in_bear | +14.6% | -12.3% | 9.8% | 0.081 |
| 6 | BTCUSDT | 2021-10-04 | 2021-10-15 | 11 | TURTLE_ATR | +25.2% | 33.3% | no | bull_continuation | +9.5% | +3.7% | 56.1% | 0.081 |
| 7 | ADAUSDT | 2021-08-04 | 2021-08-10 | 6 | TURTLE_ATR | +21.8% | 33.3% | no | early_rebound_in_bear | +21.0% | -22.0% | 53.7% | 0.070 |
| 8 | ETHUSDT | 2021-07-28 | 2021-08-04 | 7 | TURTLE_ATR | +18.3% | 33.3% | no | early_rebound_in_bear | +18.2% | -22.4% | 12.2% | 0.059 |
| 9 | DOGEUSDT | 2022-04-03 | 2022-04-05 | 2 | TURTLE_ATR | +17.7% | 33.3% | no | early_rebound_in_bear | +22.8% | -13.8% | 9.8% | 0.057 |
| 10 | XRPUSDT | 2025-07-14 | 2025-07-17 | 3 | TURTLE_ATR | +17.6% | 33.3% | no | bull_continuation | +13.8% | +10.8% | 12.2% | 0.057 |

## Mechanism interpretation

1. **The largest winners are not fresh BTC drawdown entries.** All 10 current top winners had positive BTC 21d returns at entry.
2. **They mostly occur while the longer-term BTC trend is still damaged.** 8/10 have negative SMA50/SMA200 proxy values, so the bot is buying early rebound continuation before the long-term trend has fully healed.
3. **They are fast.** 5/10 exit within three daily bars; the edge is burst capture, not long trend patience.
4. **Higher ATR_RANK gates still fail the convex-tail guardrail.** 8/10 are below ATR_RANK 24, so T=24/T=65 remove too much of the compounding engine. T=5 remains the permissive production gate.
5. **Position size does not explain the concentration.** 10/10 current top winners are full 1/3 slots; hedge did not affect this current top-10 set.

## Production implication

The corrected story remains high-kurtosis and fragile: the bot gets paid when selected assets continue an early rebound while broader BTC trend state still looks impaired. Additional filters must preserve these early-rebound trades or they remove the only compounding engine and leave mostly whipsaws.

## Outputs

- `snapshots/t104_top_winner_decomposition.csv`
- `snapshots/t104_top_winner_decomposition.md`
