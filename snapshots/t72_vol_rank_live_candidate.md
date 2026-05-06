# T72: VOL_LOOKBACK Live-Semantics Candidate

Generated: 2026-05-06 09:21 UTC

## Scope

This harness isolates T72 over common timestamp-aligned Base5 symbols: exact `src/live/bot.rs` entry semantics plus a VOL_LOOKBACK dollar-volume rank gate. It is a deployability audit, not a hyperopt, and does not change production bot code.

## Methodology / Exactness Notes

- Entry: exact as-coded Turtle breakout (`close >= max close` over current-inclusive EP-bar window).
- Entry gate: ATR_RANK(AP=17, LB=41, T=5.0) using `bot.rs` normalized ATR percentile semantics.
- Volume ranking: only symbols in the top 3 by 92-bar rolling dollar volume are eligible for entry.
- Hedge: BTC ATR21 > 45th percentile of 252 daily TR history => position size × 0.40.
- Exit: Turtle ATR-only stop (`highest_high - ATR_MULT * ATR`) with HOLD_MAX checked before ATR readiness.
- Fees: `LiveConfig::default().fee_pct = 4.00 bps/side`, applied to entry and exit execution prices.
- Accounting: economic mark-to-market account equity; accounting records trade PnL and fees size-aware, matching the exact-live replay; production code is unchanged unless separately promoted.

## Parameter Table

| Parameter | Value |
|---|---:|
| TURTLE_EP | 21 |
| TURTLE_ATR_PERIOD | 24 |
| TURTLE_ATR_MULT | 2.00 |
| ATR_ENTRY_MULT | 0.00 |
| HOLD_MAX | 12 |
| POSITION_CAP | 3 |
| REGIME_ATR_PERIOD | 17 |
| REGIME_LOOKBACK | 41 |
| ATR_RANK_THRESHOLD | 5.0 |
| VOL_LOOKBACK | 92 |
| HEDGE_ATR_PCT | 0.45 |
| HEDGE_SIZE_MULT | 0.40 |
| fee_pct | 0.000400 |

## Full-History Results

| Metric | Value |
|---|---:|
| Days | 1795 |
| Final equity | 1.01x |
| Annualised return | 0.3% |
| Daily account Sharpe | 0.09 |
| Max drawdown | 30.1% |
| Trades | 207 |
| Win rate | 45.9% |
| Entry candidates before ATR gate | 705 |
| ATR gate skips | 236 |
| Hedged entries | 131 |
| Open positions liquidated at end | 0 |

## T72 Verdict

The exact as-coded live replay generated in the same session was **2.56x / Sharpe 0.95 / MaxDD 28.8% / 298 trades**. This isolated VOL_LOOKBACK gate produced **1.01x / Sharpe 0.09 / MaxDD 30.1% / 207 trades**.

**Decision:** do not wire `VOL_LOOKBACK=92` ranking into `src/live/bot.rs` without a new mechanism. The missing volume-rank path is not the source of the live/research equity gap.

## Yearly Table

| Year | End Equity | Return | Sharpe | MaxDD |
|---:|---:|---:|---:|---:|
| 2021 | 1.12x | 11.8% | 0.99 | 10.2% |
| 2022 | 0.89x | -20.1% | -1.01 | 28.3% |
| 2023 | 1.01x | 12.6% | 1.06 | 14.5% |
| 2024 | 1.02x | 1.4% | 0.18 | 10.2% |
| 2025 | 1.12x | 9.5% | 0.83 | 9.5% |
| 2026 | 1.01x | -9.5% | -3.12 | 11.9% |

## Top-Trade Attribution

| Metric | Value |
|---|---:|
| Equity without top 5 log contributors | 0.76x |
| Equity without top 10 log contributors | 0.63x |
| Top 5 share of log return | 2101.7% |
| Top 10 share of log return | 3470.3% |

### Top 10 Trades

| Rank | Symbol | Entry | Exit | Held | Size | Trade Ret | Equity Mult | Reason | Hedge |
|---:|---|---|---|---:|---:|---:|---:|---|---|
| 1 | BTCUSDT | 2021-10-04 00:00:00 | 2021-10-15 00:00:00 | 11 | 0.333 | 25.2% | 1.0840 | TURTLE_ATR | false |
| 2 | ETHUSDT | 2021-07-28 00:00:00 | 2021-08-04 00:00:00 | 7 | 0.333 | 18.3% | 1.0611 | TURTLE_ATR | false |
| 3 | ETHUSDT | 2025-08-07 00:00:00 | 2025-08-12 00:00:00 | 5 | 0.333 | 17.3% | 1.0577 | TURTLE_ATR | false |
| 4 | ETHUSDT | 2022-07-16 00:00:00 | 2022-07-18 00:00:00 | 2 | 0.333 | 16.5% | 1.0551 | TURTLE_ATR | false |
| 5 | SOLUSDT | 2025-07-16 00:00:00 | 2025-07-21 00:00:00 | 5 | 0.333 | 12.6% | 1.0421 | TURTLE_ATR | false |
| 6 | DOGEUSDT | 2023-01-10 00:00:00 | 2023-01-14 00:00:00 | 4 | 0.333 | 12.1% | 1.0403 | TURTLE_ATR | false |
| 7 | ETHUSDT | 2025-07-14 00:00:00 | 2025-07-16 00:00:00 | 2 | 0.333 | 11.8% | 1.0393 | TURTLE_ATR | false |
| 8 | SOLUSDT | 2025-05-08 00:00:00 | 2025-05-13 00:00:00 | 5 | 0.333 | 11.6% | 1.0388 | TURTLE_ATR | false |
| 9 | ADAUSDT | 2021-08-19 00:00:00 | 2021-08-24 00:00:00 | 5 | 0.333 | 11.3% | 1.0376 | TURTLE_ATR | false |
| 10 | SOLUSDT | 2024-03-07 00:00:00 | 2024-03-15 00:00:00 | 8 | 0.133 | 27.8% | 1.0371 | TURTLE_ATR | true |

## Files

- `snapshots/t72_vol_rank_live_candidate_equity.csv` — daily mark-to-market account equity
- `snapshots/t72_vol_rank_live_candidate_trades.csv` — trade ledger
