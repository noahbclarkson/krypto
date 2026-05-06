# T80: OOS Universe Validation

Generated: 2026-05-06 21:11 UTC

## Scope

Hold-out symbols: **UNIUSDT, MATICUSDT, AVAXUSDT**. These pairs were reserved because they are outside the repeatedly optimized 9-universe grid. BTCUSDT is used only for ATR_RANK/hedge regime gates, not as a traded symbol.

## Method

- Six most recent walk-forward windows per symbol: 252 train bars + 252 test bars.
- Entry/exit mirrors `examples/live_bot_exact_equity.rs` and the as-coded daily `src/live/bot.rs` Turtle path.
- Entry: current-inclusive Turtle EP=21 window, equality allowed.
- Gate: ATR_RANK(AP=17, LB=41, T=5.0); hedge size multiplier 0.40 when active.
- Exit: Turtle ATR-only (`highest_high - 2.00×ATR24`) plus HOLD_MAX=12; fee 4.00 bps/side.
- `VOL_LOOKBACK=92` remains unused by exact live bot entry logic.

## Global Result

| Metric | Value |
|---|---:|
| Pass rate | 11/18 (61.1%) |
| Avg Sharpe | 0.149 |
| Avg return/window | 2.3% |
| Avg MaxDD | 6.1% |
| Trades | 160 |

**Verdict:** MIXED: hold-out evidence is borderline; do not treat as clean generalization.

## Window Results

| Symbol | W | Test Start | Test End | Equity | Sharpe | MaxDD | Trades | Pass |
|---|---:|---|---|---:|---:|---:|---:|---|
| UNIUSDT | 1 | 2022-03-17 00:00:00 | 2022-11-23 00:00:00 | 0.851x | -1.35 | 16.9% | 10 | FAIL |
| UNIUSDT | 2 | 2022-11-24 00:00:00 | 2023-08-02 00:00:00 | 0.961x | -0.76 | 6.5% | 8 | FAIL |
| UNIUSDT | 3 | 2023-08-03 00:00:00 | 2024-04-10 00:00:00 | 0.999x | -0.01 | 3.9% | 14 | FAIL |
| UNIUSDT | 4 | 2024-04-11 00:00:00 | 2024-12-18 00:00:00 | 1.023x | 0.42 | 7.7% | 7 | PASS |
| UNIUSDT | 5 | 2024-12-19 00:00:00 | 2025-08-27 00:00:00 | 0.967x | -0.50 | 6.1% | 7 | FAIL |
| UNIUSDT | 6 | 2025-08-28 00:00:00 | 2026-05-06 00:00:00 | 0.954x | -2.64 | 4.6% | 4 | FAIL |
| MATICUSDT | 1 | 2020-07-22 00:00:00 | 2021-03-30 00:00:00 | 1.274x | 2.34 | 3.7% | 14 | PASS |
| MATICUSDT | 2 | 2021-03-31 00:00:00 | 2021-12-07 00:00:00 | 1.155x | 1.29 | 10.5% | 16 | PASS |
| MATICUSDT | 3 | 2021-12-08 00:00:00 | 2022-08-16 00:00:00 | 1.106x | 1.12 | 5.6% | 5 | PASS |
| MATICUSDT | 4 | 2022-08-17 00:00:00 | 2023-04-25 00:00:00 | 1.018x | 0.24 | 9.1% | 13 | PASS |
| MATICUSDT | 5 | 2023-04-26 00:00:00 | 2024-01-02 00:00:00 | 1.061x | 1.56 | 2.6% | 8 | PASS |
| MATICUSDT | 6 | 2024-01-03 00:00:00 | 2024-09-10 00:00:00 | 1.004x | 0.17 | 2.2% | 5 | PASS |
| AVAXUSDT | 1 | 2022-03-17 00:00:00 | 2022-11-23 00:00:00 | 0.884x | -1.56 | 12.7% | 7 | FAIL |
| AVAXUSDT | 2 | 2022-11-24 00:00:00 | 2023-08-02 00:00:00 | 1.115x | 1.21 | 3.7% | 9 | PASS |
| AVAXUSDT | 3 | 2023-08-03 00:00:00 | 2024-04-10 00:00:00 | 1.030x | 0.63 | 3.6% | 17 | PASS |
| AVAXUSDT | 4 | 2024-04-11 00:00:00 | 2024-12-18 00:00:00 | 1.022x | 0.50 | 2.6% | 7 | PASS |
| AVAXUSDT | 5 | 2024-12-19 00:00:00 | 2025-08-27 00:00:00 | 0.971x | -0.73 | 6.1% | 6 | FAIL |
| AVAXUSDT | 6 | 2025-08-28 00:00:00 | 2026-05-06 00:00:00 | 1.014x | 0.75 | 1.4% | 3 | PASS |

## Per-Symbol Summary

| Symbol | Pass | Avg Equity | Avg Sharpe | Avg DD | Trades |
|---|---:|---:|---:|---:|---:|
| UNIUSDT | 1/6 | 0.959x | -0.81 | 7.6% | 50 |
| MATICUSDT | 6/6 | 1.103x | 1.12 | 5.6% | 61 |
| AVAXUSDT | 4/6 | 1.006x | 0.13 | 5.0% | 49 |

Pass definition: ≥3 trades and Sharpe > 0.0 in the test window. Promotion guardrail from PLAN: global pass rate ≥70% and avg Sharpe ≥0.5.

## Files

- `snapshots/t80_oos_universe_validation.csv` — per-window data
