# T65: Exact Live-Bot Source-of-Truth Equity

Generated: 2026-05-06 00:09 UTC

## Scope

This harness replays the current `src/live/bot.rs` daily event logic over common timestamp-aligned Base5 symbols in configured symbol order. It is a source-of-truth audit harness, not a new hyperopt.

## Methodology / Exactness Notes

- Entry: current `bot.rs` Turtle condition as coded: current-inclusive EP window and equality allowed (`close < max_close` is rejected, equality passes).
- Entry gate: ATR_RANK(AP=17, LB=41, T=5.0) using `bot.rs` normalized ATR percentile semantics.
- Volume ranking: `VOL_LOOKBACK=92` is in config but **not used** by `src/live/bot.rs`; this exact harness therefore does not apply VL ranking.
- Hedge: BTC ATR21 > 45th percentile of 252 daily TR history => position size × 0.40.
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
| HOLD_MAX | 12 |
| POSITION_CAP | 3 |
| REGIME_ATR_PERIOD | 17 |
| REGIME_LOOKBACK | 41 |
| ATR_RANK_THRESHOLD | 5.0 |
| VOL_LOOKBACK | 92 (unused by bot.rs) |
| HEDGE_ATR_PCT | 0.45 |
| HEDGE_SIZE_MULT | 0.40 |
| fee_pct | 0.000400 |

## Full-History Results

| Metric | Value |
|---|---:|
| Days | 1795 |
| Final equity | 2.55x |
| Annualised return | 20.9% |
| Daily account Sharpe | 0.94 |
| Max drawdown | 28.8% |
| Trades | 298 |
| Win rate | 48.0% |
| Entry candidates before ATR gate | 525 |
| ATR gate skips | 227 |
| Hedged entries | 187 |
| Open positions liquidated at end | 1 |

## Yearly Table

| Year | End Equity | Return | Sharpe | MaxDD |
|---:|---:|---:|---:|---:|
| 2021 | 1.60x | 60.2% | 2.89 | 5.8% |
| 2022 | 1.21x | -24.2% | -0.77 | 28.7% |
| 2023 | 1.75x | 43.8% | 1.79 | 11.9% |
| 2024 | 2.06x | 17.5% | 1.09 | 13.0% |
| 2025 | 2.75x | 33.2% | 1.58 | 7.9% |
| 2026 | 2.55x | -7.2% | -2.06 | 12.6% |

## Top-Trade Attribution

| Metric | Value |
|---|---:|
| Equity without top 5 log contributors | 1.48x |
| Equity without top 10 log contributors | 1.08x |
| Top 5 share of log return | 57.8% |
| Top 10 share of log return | 91.5% |

### Top 10 Trades

| Rank | Symbol | Entry | Exit | Held | Size | Trade Ret | Equity Mult | Reason | Hedge |
|---:|---|---|---|---:|---:|---:|---:|---|---|
| 1 | SOLUSDT | 2023-01-11 00:00:00 | 2023-01-14 00:00:00 | 3 | 0.333 | 48.0% | 1.1602 | TURTLE_ATR | false |
| 2 | DOGEUSDT | 2022-10-28 00:00:00 | 2022-10-29 00:00:00 | 1 | 0.333 | 45.1% | 1.1502 | TURTLE_ATR | false |
| 3 | SOLUSDT | 2021-07-30 00:00:00 | 2021-08-11 00:00:00 | 12 | 0.333 | 28.5% | 1.0951 | HOLD_MAX | false |
| 4 | SOLUSDT | 2021-08-27 00:00:00 | 2021-08-30 00:00:00 | 3 | 0.333 | 25.2% | 1.0840 | TURTLE_ATR | false |
| 5 | BTCUSDT | 2021-10-04 00:00:00 | 2021-10-15 00:00:00 | 11 | 0.333 | 25.2% | 1.0840 | TURTLE_ATR | false |
| 6 | ADAUSDT | 2021-08-04 00:00:00 | 2021-08-10 00:00:00 | 6 | 0.333 | 21.8% | 1.0727 | TURTLE_ATR | false |
| 7 | XRPUSDT | 2024-11-28 00:00:00 | 2024-12-01 00:00:00 | 3 | 0.133 | 48.6% | 1.0648 | TURTLE_ATR | true |
| 8 | SOLUSDT | 2021-08-13 00:00:00 | 2021-08-15 00:00:00 | 2 | 0.333 | 19.3% | 1.0644 | TURTLE_ATR | false |
| 9 | XRPUSDT | 2021-08-10 00:00:00 | 2021-08-11 00:00:00 | 1 | 0.333 | 18.6% | 1.0621 | TURTLE_ATR | false |
| 10 | ETHUSDT | 2021-07-28 00:00:00 | 2021-08-04 00:00:00 | 7 | 0.333 | 18.3% | 1.0611 | TURTLE_ATR | false |

## Files

- `snapshots/live_bot_exact_equity.csv` — daily mark-to-market account equity
- `snapshots/live_bot_exact_trades.csv` — trade ledger
