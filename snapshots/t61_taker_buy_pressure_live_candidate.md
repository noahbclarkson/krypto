# T61-ALT: Taker-Buy Pressure Live-Entry Candidate

Generated: 2026-05-06 18:12 UTC

## Scope

This harness replays the current `src/live/bot.rs` daily event logic over common timestamp-aligned Base5+ADA symbols in configured symbol order, then adds one candidate entry overlay: available taker-buy pressure must exceed its prior 252-bar median. T73 top-winner entries bypass the overlay to enforce the convex-tail guardrail. It is a candidate harness, not a production patch.

## Methodology / Exactness Notes

- Entry: current `bot.rs` Turtle condition as coded: current-inclusive EP window and equality allowed (`close < max_close` is rejected, equality passes).
- Entry gate: ATR_RANK(AP=17, LB=41, T=5.0) using `bot.rs` normalized ATR percentile semantics.
- Volume ranking: `VOL_LOOKBACK=92` is in config but **not used** by `src/live/bot.rs`; this candidate does not apply VL ranking.
- Hedge: BTC ATR38 > 45th percentile of 252 daily TR history => position size × 0.40.
- Exit: Turtle ATR-only stop (`highest_high - ATR_MULT * ATR`) with HOLD_MAX checked before ATR readiness.
- Fees: `LiveConfig::default().fee_pct = 4.00 bps/side`, applied to entry and exit execution prices.
- Pressure overlay: if pressure data exists and the entry is not in the T73 top-10 protected set, require current `taker_buy_pressure` > prior 252-bar median (minimum 60 historical observations). ADA has no pressure cache and is left unchanged.
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
| HEDGE_ATR_PERIOD | 38 |
| HEDGE_LOOKBACK | 252 |
| HEDGE_ATR_PCT | 0.45 |
| HEDGE_SIZE_MULT | 0.40 |
| fee_pct | 0.000400 |

## Full-History Results

| Metric | Value |
|---|---:|
| Days | 1795 |
| Final equity | 2.76x |
| Annualised return | 22.9% |
| Daily account Sharpe | 1.01 |
| Max drawdown | 25.3% |
| Trades | 275 |
| Win rate | 48.4% |
| Entry candidates before ATR gate | 565 |
| ATR gate skips | 233 |
| Pressure gate skips | 57 |
| Pressure gate passes | 230 |
| T73 protected allows | 6 |
| No-cache allows | 39 |
| Warmup-history allows | 0 |
| Hedged entries | 172 |
| Open positions liquidated at end | 1 |

## Guardrail Preservation

| Metric | Value |
|---|---:|
| T73 top winners preserved | 6/10 |
| T73 top winners missing | SOLUSDT 2021-07-30; ADAUSDT 2021-08-04; SOLUSDT 2021-08-13; XRPUSDT 2021-08-10 |

## Candidate Verdict

Benchmark source of truth from same-session `live_bot_exact_equity`: **2.78x / daily Sharpe 1.00 / MaxDD 28.2% / 298 trades**. This candidate produced **2.76x / daily Sharpe 1.01 / MaxDD 25.3% / 275 trades**.

**Decision: REJECT / close T61-ALT.** The pressure overlay slightly reduced drawdown but did not improve equity, and it failed the T73 convex-tail guardrail: only 6/10 protected top winners survived because path/cap interactions changed subsequent entries. Do not promote this filter into `src/live/bot.rs`.

## Yearly Table

| Year | End Equity | Return | Sharpe | MaxDD |
|---:|---:|---:|---:|---:|
| 2021 | 1.52x | 51.6% | 2.36 | 9.5% |
| 2022 | 1.22x | -19.6% | -0.63 | 24.9% |
| 2023 | 1.89x | 55.5% | 2.09 | 10.3% |
| 2024 | 2.37x | 24.7% | 1.34 | 13.3% |
| 2025 | 2.89x | 22.0% | 1.16 | 10.8% |
| 2026 | 2.76x | -4.7% | -1.61 | 9.3% |

## Top-Trade Attribution

| Metric | Value |
|---|---:|
| Equity without top 5 log contributors | 1.59x |
| Equity without top 10 log contributors | 1.17x |
| Top 5 share of log return | 54.2% |
| Top 10 share of log return | 84.6% |

### Top 10 Trades

| Rank | Symbol | Entry | Exit | Held | Size | Trade Ret | Equity Mult | Reason | Hedge |
|---:|---|---|---|---:|---:|---:|---:|---|---|
| 1 | SOLUSDT | 2023-01-11 00:00:00 | 2023-01-14 00:00:00 | 3 | 0.333 | 48.0% | 1.1602 | TURTLE_ATR | false |
| 2 | DOGEUSDT | 2022-10-28 00:00:00 | 2022-10-29 00:00:00 | 1 | 0.333 | 45.1% | 1.1502 | TURTLE_ATR | false |
| 3 | SOLUSDT | 2021-08-10 00:00:00 | 2021-08-15 00:00:00 | 5 | 0.333 | 31.6% | 1.1054 | TURTLE_ATR | false |
| 4 | SOLUSDT | 2021-08-27 00:00:00 | 2021-08-30 00:00:00 | 3 | 0.333 | 25.2% | 1.0840 | TURTLE_ATR | false |
| 5 | BTCUSDT | 2021-10-04 00:00:00 | 2021-10-15 00:00:00 | 11 | 0.333 | 25.2% | 1.0840 | TURTLE_ATR | false |
| 6 | ADAUSDT | 2021-08-03 00:00:00 | 2021-08-10 00:00:00 | 7 | 0.333 | 22.4% | 1.0747 | TURTLE_ATR | false |
| 7 | XRPUSDT | 2024-11-28 00:00:00 | 2024-12-01 00:00:00 | 3 | 0.133 | 48.6% | 1.0648 | TURTLE_ATR | true |
| 8 | ETHUSDT | 2021-07-28 00:00:00 | 2021-08-04 00:00:00 | 7 | 0.333 | 18.3% | 1.0611 | TURTLE_ATR | false |
| 9 | DOGEUSDT | 2022-04-03 00:00:00 | 2022-04-05 00:00:00 | 2 | 0.333 | 17.7% | 1.0590 | TURTLE_ATR | false |
| 10 | XRPUSDT | 2025-07-14 00:00:00 | 2025-07-17 00:00:00 | 3 | 0.333 | 17.6% | 1.0586 | TURTLE_ATR | false |

## Files

- `snapshots/t61_taker_buy_pressure_live_candidate_equity.csv` — daily mark-to-market account equity
- `snapshots/t61_taker_buy_pressure_live_candidate_trades.csv` — trade ledger
