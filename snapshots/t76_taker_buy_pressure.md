# T76 — Binance Taker-Buy Pressure Feasibility

Binance daily klines expose taker-buy quote volume, giving a public no-auth order-flow proxy: `taker_buy_pressure = taker_buy_quote_vol / quote_vol`.

## Per-symbol next-day returns by taker-buy pressure quintile

| Symbol | Q | Pressure range | N | Avg next-day ret | Annualised Sharpe |
|---|---:|---:|---:|---:|---:|
| BTCUSDT | 1 | 0.000-0.475 | 440 | -0.042% | -0.27 |
| BTCUSDT | 2 | 0.475-0.487 | 439 | +0.347% | 2.00 |
| BTCUSDT | 3 | 0.487-0.497 | 440 | +0.202% | 1.23 |
| BTCUSDT | 4 | 0.497-0.505 | 439 | +0.056% | 0.37 |
| BTCUSDT | 5 | 0.505-1.000 | 441 | +0.206% | 1.35 |
| DOGEUSDT | 1 | 0.000-0.476 | 440 | -0.205% | -0.91 |
| DOGEUSDT | 2 | 0.476-0.489 | 439 | +0.226% | 0.83 |
| DOGEUSDT | 3 | 0.489-0.498 | 440 | +0.005% | 0.01 |
| DOGEUSDT | 4 | 0.498-0.509 | 439 | +0.753% | 1.71 |
| DOGEUSDT | 5 | 0.509-1.000 | 441 | +1.613% | 1.54 |
| ETHUSDT | 1 | 0.000-0.482 | 440 | +0.131% | 0.65 |
| ETHUSDT | 2 | 0.482-0.493 | 439 | +0.173% | 0.78 |
| ETHUSDT | 3 | 0.493-0.501 | 440 | +0.268% | 1.17 |
| ETHUSDT | 4 | 0.501-0.511 | 439 | +0.378% | 1.61 |
| ETHUSDT | 5 | 0.511-1.000 | 441 | +0.038% | 0.21 |
| SOLUSDT | 1 | 0.000-0.479 | 419 | +0.014% | 0.04 |
| SOLUSDT | 2 | 0.479-0.492 | 418 | +0.571% | 1.60 |
| SOLUSDT | 3 | 0.492-0.502 | 419 | +0.558% | 1.94 |
| SOLUSDT | 4 | 0.502-0.512 | 418 | +0.425% | 1.39 |
| SOLUSDT | 5 | 0.512-1.000 | 420 | +0.153% | 0.48 |
| XRPUSDT | 1 | 0.000-0.475 | 440 | +0.026% | 0.11 |
| XRPUSDT | 2 | 0.475-0.488 | 439 | +0.308% | 1.18 |
| XRPUSDT | 3 | 0.488-0.497 | 440 | +0.184% | 0.63 |
| XRPUSDT | 4 | 0.497-0.507 | 439 | +0.061% | 0.19 |
| XRPUSDT | 5 | 0.507-1.000 | 441 | +0.560% | 1.74 |

## Cross-sectional pressure spread

Long top-2 pressure / short bottom-2 pressure across Base5: **+0.209%/day**, Sharpe **0.70**, win rate **50.2%**, N=2094 days.

## Verdict

- Taker-buy pressure is **real data we were throwing away**: the standard Binance kline response already contains the needed order-flow fields.
- Signal quality is **mixed by symbol**. BTC/XRP/DOGE show higher next-day returns at high pressure; ETH/SOL invert or flatten.
- Cross-sectional high-minus-low pressure has only a modest Sharpe, so this is not strong enough to promote as a Turtle entry filter.
- Use as future research input / feature cache, not as a production gate. Any candidate must still pass the T73 convex-winner preservation guardrail.
