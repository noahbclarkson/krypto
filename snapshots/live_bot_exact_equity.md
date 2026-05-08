# T65: Exact Live-Bot Source-of-Truth Equity

Generated: 2026-05-08 15:07 UTC

## Scope

This harness replays the current `src/live/bot.rs` daily event logic over common timestamp-aligned Base5 symbols in configured symbol order. It is a source-of-truth audit harness, not a new hyperopt.

## Methodology / Exactness Notes

- Entry: current `bot.rs` Turtle condition as coded: current-inclusive EP window and equality allowed (`close < max_close` is rejected, equality passes).
- Entry gate: ATR_RANK(AP=17, LB=41, T=5.0) using `bot.rs` normalized ATR percentile semantics.
- Volume ranking: `VOL_LOOKBACK=92` is in config but **not used** by `src/live/bot.rs`; this exact harness therefore does not apply VL ranking.
- Hedge: BTC ATR38 > 45th percentile of 252 daily TR history => position size × 0.25.
- Exit: Turtle ATR-only stop (`highest_high - ATR_MULT * ATR`) with HOLD_MAX checked before ATR readiness.
- Fees: `LiveConfig::default().fee_pct = 4.00 bps/side`, applied to entry and exit execution prices.
- Accounting: economic mark-to-market account equity. This intentionally does **not** copy the live UI `BotState` accounting bug that ignores trade size in `record_trade`.

## Parameter Table

| Parameter | Value |
|---|---:|
| TURTLE_EP | 21 |
| TURTLE_ATR_PERIOD | 24 |
| TURTLE_ATR_MULT | 2.00 |
| ATR_ENTRY_MULT | 0.00 |
| HOLD_MAX | 15 |
| POSITION_CAP | 3 |
| REGIME_ATR_PERIOD | 17 |
| REGIME_LOOKBACK | 41 |
| ATR_RANK_THRESHOLD | 5.0 |
| VOL_LOOKBACK | 92 (unused by bot.rs) |
| HEDGE_ATR_PERIOD | 38 |
| HEDGE_LOOKBACK | 252 |
| HEDGE_ATR_PCT | 0.45 |
| HEDGE_SIZE_MULT | 0.25 |
| fee_pct | 0.000400 |

## Full-History Results

| Metric | Value |
|---|---:|
| Days | 1797 |
| Final equity | 2.76x |
| Annualised return | 22.9% |
| Daily account Sharpe | 1.02 |
| Max drawdown | 22.3% |
| Trades | 286 |
| Win rate | 46.9% |
| Entry candidates before ATR gate | 511 |
| ATR gate skips | 225 |
| Hedged entries | 175 |
| Open positions liquidated at end | 0 |

## Yearly Table

| Year | End Equity | Return | Sharpe | MaxDD |
|---:|---:|---:|---:|---:|
| 2021 | 1.67x | 66.6% | 3.09 | 8.2% |
| 2022 | 1.33x | -20.1% | -0.58 | 21.8% |
| 2023 | 1.85x | 39.0% | 1.59 | 14.7% |
| 2024 | 2.28x | 22.8% | 1.52 | 9.6% |
| 2025 | 2.85x | 25.1% | 1.35 | 10.7% |
| 2026 | 2.76x | -3.2% | -1.19 | 6.3% |

## Top-Trade Attribution

| Metric | Value |
|---|---:|
| Equity without top 5 log contributors | 1.51x |
| Equity without top 10 log contributors | 1.09x |
| Top 5 share of log return | 59.4% |
| Top 10 share of log return | 91.4% |

### Top 10 Trades

| Rank | Symbol | Entry | Exit | Held | Size | Trade Ret | Equity Mult | Reason | Hedge |
|---:|---|---|---|---:|---:|---:|---:|---|---|
| 1 | SOLUSDT | 2023-01-11 00:00:00 | 2023-01-14 00:00:00 | 3 | 0.333 | 48.0% | 1.1602 | TURTLE_ATR | false |
| 2 | DOGEUSDT | 2022-10-28 00:00:00 | 2022-10-29 00:00:00 | 1 | 0.333 | 45.1% | 1.1502 | TURTLE_ATR | false |
| 3 | XRPUSDT | 2021-07-28 00:00:00 | 2021-08-11 00:00:00 | 14 | 0.333 | 37.8% | 1.1260 | TURTLE_ATR | false |
| 4 | SOLUSDT | 2021-07-30 00:00:00 | 2021-08-14 00:00:00 | 15 | 0.333 | 36.3% | 1.1211 | HOLD_MAX | false |
| 5 | SOLUSDT | 2021-08-27 00:00:00 | 2021-08-30 00:00:00 | 3 | 0.333 | 25.2% | 1.0840 | TURTLE_ATR | false |
| 6 | BTCUSDT | 2021-10-04 00:00:00 | 2021-10-15 00:00:00 | 11 | 0.333 | 25.2% | 1.0840 | TURTLE_ATR | false |
| 7 | ADAUSDT | 2021-08-04 00:00:00 | 2021-08-10 00:00:00 | 6 | 0.333 | 21.8% | 1.0727 | TURTLE_ATR | false |
| 8 | ETHUSDT | 2021-07-28 00:00:00 | 2021-08-04 00:00:00 | 7 | 0.333 | 18.3% | 1.0611 | TURTLE_ATR | false |
| 9 | DOGEUSDT | 2022-04-03 00:00:00 | 2022-04-05 00:00:00 | 2 | 0.333 | 17.7% | 1.0590 | TURTLE_ATR | false |
| 10 | XRPUSDT | 2025-07-14 00:00:00 | 2025-07-17 00:00:00 | 3 | 0.333 | 17.6% | 1.0586 | TURTLE_ATR | false |

## Files

- `snapshots/live_bot_exact_equity.csv` — daily mark-to-market account equity
- `snapshots/live_bot_exact_trades.csv` — trade ledger
