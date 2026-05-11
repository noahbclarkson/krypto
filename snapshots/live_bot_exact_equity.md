# T65: Exact Live-Bot Source-of-Truth Equity

Generated: 2026-05-11 15:34 UTC

## Scope

This harness replays the current `src/live/bot.rs` daily event logic over common timestamp-aligned Base5 symbols in configured symbol order. It is a source-of-truth audit harness, not a new hyperopt.

## Methodology / Exactness Notes

- Entry: current `bot.rs` Turtle condition as coded: current-inclusive EP window and equality allowed (`close < max_close` is rejected, equality passes).
- Entry gate: ATR_RANK(AP=17, LB=41, T=5.0) using `bot.rs` normalized ATR percentile semantics.
- Volume ranking: `VOL_LOOKBACK=92` is in config but **not used** by `src/live/bot.rs`; this exact harness therefore does not apply VL ranking.
- Hedge: BTC ATR38 > 9th percentile of 252 daily TR history => position size × 0.25.
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
| HEDGE_ATR_PCT | 0.09 |
| HEDGE_SIZE_MULT | 0.25 |
| fee_pct | 0.000400 |

## Full-History Results

| Metric | Value |
|---|---:|
| Days | 1800 |
| Final equity | 1.42x |
| Annualised return | 7.4% |
| Daily account Sharpe | 0.90 |
| Max drawdown | 9.0% |
| Trades | 286 |
| Win rate | 46.9% |
| Entry candidates before ATR gate | 519 |
| ATR gate skips | 233 |
| Hedged entries | 286 |
| Open positions liquidated at end | 0 |

## Yearly Table

| Year | End Equity | Return | Sharpe | MaxDD |
|---:|---:|---:|---:|---:|
| 2021 | 1.12x | 11.8% | 2.31 | 2.7% |
| 2022 | 1.07x | -4.0% | -0.47 | 6.0% |
| 2023 | 1.28x | 18.8% | 1.93 | 6.2% |
| 2024 | 1.42x | 11.3% | 1.21 | 9.0% |
| 2025 | 1.49x | 4.5% | 0.66 | 4.9% |
| 2026 | 1.42x | -4.5% | -2.86 | 5.5% |

## Top-Trade Attribution

| Metric | Value |
|---|---:|
| Equity without top 5 log contributors | 1.19x |
| Equity without top 10 log contributors | 1.06x |
| Top 5 share of log return | 51.4% |
| Top 10 share of log return | 84.7% |

### Top 10 Trades

| Rank | Symbol | Entry | Exit | Held | Size | Trade Ret | Equity Mult | Reason | Hedge |
|---:|---|---|---|---:|---:|---:|---:|---|---|
| 1 | XRPUSDT | 2024-11-28 00:00:00 | 2024-12-01 00:00:00 | 3 | 0.083 | 48.6% | 1.0405 | TURTLE_ATR | true |
| 2 | SOLUSDT | 2023-01-11 00:00:00 | 2023-01-14 00:00:00 | 3 | 0.083 | 48.0% | 1.0400 | TURTLE_ATR | true |
| 3 | DOGEUSDT | 2022-10-28 00:00:00 | 2022-10-29 00:00:00 | 1 | 0.083 | 45.1% | 1.0376 | TURTLE_ATR | true |
| 4 | ADAUSDT | 2023-11-23 00:00:00 | 2023-12-08 00:00:00 | 15 | 0.083 | 40.7% | 1.0339 | HOLD_MAX | true |
| 5 | XRPUSDT | 2021-07-28 00:00:00 | 2021-08-11 00:00:00 | 14 | 0.083 | 37.8% | 1.0315 | TURTLE_ATR | true |
| 6 | SOLUSDT | 2021-07-30 00:00:00 | 2021-08-14 00:00:00 | 15 | 0.083 | 36.3% | 1.0303 | HOLD_MAX | true |
| 7 | SOLUSDT | 2024-03-07 00:00:00 | 2024-03-15 00:00:00 | 8 | 0.083 | 27.8% | 1.0232 | TURTLE_ATR | true |
| 8 | ADAUSDT | 2024-11-20 00:00:00 | 2024-11-22 00:00:00 | 2 | 0.083 | 27.0% | 1.0225 | TURTLE_ATR | true |
| 9 | SOLUSDT | 2023-10-29 00:00:00 | 2023-11-01 00:00:00 | 3 | 0.083 | 25.2% | 1.0210 | TURTLE_ATR | true |
| 10 | SOLUSDT | 2021-08-27 00:00:00 | 2021-08-30 00:00:00 | 3 | 0.083 | 25.2% | 1.0210 | TURTLE_ATR | true |

## Files

- `snapshots/live_bot_exact_equity.csv` — daily mark-to-market account equity
- `snapshots/live_bot_exact_trades.csv` — trade ledger
