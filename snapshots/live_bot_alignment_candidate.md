# T69: Live-Bot Semantic Alignment Candidate

Generated: 2026-05-05 21:11 UTC

## Scope

This harness tests a candidate live-bot semantic alignment over common timestamp-aligned Base5 symbols: strict prior-window entry + VOL_LOOKBACK dollar-volume rank gate + size-aware economic accounting. It is a deployability audit, not a new hyperopt, and does not change production bot code.

## Methodology / Exactness Notes

- Entry: strict prior-window Turtle breakout (`close > max close of prior EP bars`; current bar excluded, equality rejected).
- Entry gate: ATR_RANK(AP=17, LB=41, T=5.0) using `bot.rs` normalized ATR percentile semantics.
- Volume ranking: only symbols in the top 3 by 92-bar rolling dollar volume are eligible for entry.
- Hedge: BTC ATR21 > 45th percentile of 252 daily TR history => position size × 0.40.
- Exit: Turtle ATR-only stop (`highest_high - ATR_MULT * ATR`) with HOLD_MAX checked before ATR readiness.
- Fees: `LiveConfig::default().fee_pct = 4.00 bps/side`, applied to entry and exit execution prices.
- Accounting: economic mark-to-market account equity; candidate accounting records trade PnL and fees size-aware; production code is unchanged unless separately promoted.

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
| Days | 1794 |
| Final equity | 1.02x |
| Annualised return | 0.4% |
| Daily account Sharpe | 0.10 |
| Max drawdown | 30.8% |
| Trades | 200 |
| Win rate | 45.0% |
| Entry candidates before ATR gate | 692 |
| ATR gate skips | 239 |
| Hedged entries | 126 |
| Open positions liquidated at end | 0 |

## Yearly Table

| Year | End Equity | Return | Sharpe | MaxDD |
|---:|---:|---:|---:|---:|
| 2021 | 1.16x | 15.8% | 1.29 | 10.2% |
| 2022 | 0.89x | -22.9% | -1.33 | 29.4% |
| 2023 | 1.00x | 12.5% | 1.07 | 14.1% |
| 2024 | 1.04x | 2.9% | 0.33 | 8.9% |
| 2025 | 1.12x | 8.1% | 0.73 | 9.1% |
| 2026 | 1.02x | -9.0% | -2.98 | 11.4% |

## Top-Trade Attribution

| Metric | Value |
|---|---:|
| Equity without top 5 log contributors | 0.77x |
| Equity without top 10 log contributors | 0.64x |
| Top 5 share of log return | 1385.9% |
| Top 10 share of log return | 2313.5% |

### Top 10 Trades

| Rank | Symbol | Entry | Exit | Held | Size | Trade Ret | Equity Mult | Reason | Hedge |
|---:|---|---|---|---:|---:|---:|---:|---|---|
| 1 | BTCUSDT | 2021-10-04 00:00:00 | 2021-10-15 00:00:00 | 11 | 0.333 | 25.2% | 1.0840 | TURTLE_ATR | false |
| 2 | ETHUSDT | 2025-08-07 00:00:00 | 2025-08-12 00:00:00 | 5 | 0.333 | 17.3% | 1.0577 | TURTLE_ATR | false |
| 3 | ETHUSDT | 2022-07-16 00:00:00 | 2022-07-18 00:00:00 | 2 | 0.333 | 16.5% | 1.0551 | TURTLE_ATR | false |
| 4 | ETHUSDT | 2022-03-23 00:00:00 | 2022-04-04 00:00:00 | 12 | 0.333 | 15.8% | 1.0528 | HOLD_MAX | false |
| 5 | SOLUSDT | 2025-07-16 00:00:00 | 2025-07-21 00:00:00 | 5 | 0.333 | 12.6% | 1.0421 | TURTLE_ATR | false |
| 6 | DOGEUSDT | 2023-01-10 00:00:00 | 2023-01-14 00:00:00 | 4 | 0.333 | 12.1% | 1.0403 | TURTLE_ATR | false |
| 7 | ETHUSDT | 2025-07-14 00:00:00 | 2025-07-16 00:00:00 | 2 | 0.333 | 11.8% | 1.0393 | TURTLE_ATR | false |
| 8 | SOLUSDT | 2025-05-08 00:00:00 | 2025-05-13 00:00:00 | 5 | 0.333 | 11.6% | 1.0388 | TURTLE_ATR | false |
| 9 | ADAUSDT | 2021-08-19 00:00:00 | 2021-08-24 00:00:00 | 5 | 0.333 | 11.3% | 1.0376 | TURTLE_ATR | false |
| 10 | SOLUSDT | 2024-03-07 00:00:00 | 2024-03-15 00:00:00 | 8 | 0.133 | 27.8% | 1.0371 | TURTLE_ATR | true |

## Files

- `snapshots/live_bot_alignment_candidate_equity.csv` — daily mark-to-market account equity
- `snapshots/live_bot_alignment_candidate_trades.csv` — trade ledger
