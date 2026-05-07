# C19: Rebalancing Overlay on Exact Live Path

Generated: 2026-05-07 09:10 UTC

## Method

Mirror `examples/live_bot_exact_equity.rs` (same per-symbol event loop, same ATR buffer semantics) with close_losers I=5 overlay:

- When bars since last rebal >= 5 AND current P&L <= 5%, close position and immediately re-enter.
- new position inherits fresh ATR buffer (effectively restarts Turtle clock).

## Results

| Metric | C19 Rebal | Baseline (T65) | Delta |
|---|---:|---:|---:|
| Equity | 2.74x | 2.89x | -5.1% |
| Sharpe | 0.93 | 0.98 | -0.05 |
| MaxDD | 27.9% | 29.0% | -1.1pp |
| Trades | 293 | 298 | -5 |
| WinRate | 45.4% | — | — |
| Rebal events | 23 | — | — |

**Verdict:** GRAVEYARD: both equity AND Sharpe degrade → close permanently

## Per-Year

| Year | Equity | Return% | Sharpe | MaxDD% |
|---|---:|---:|---:|---:|
| 2021 | 1.54x | +54.3% | 2.61 | 8.2% |
| 2022 | 1.17x | -23.4% | -0.74 | 26.7% |
| 2023 | 1.76x | +50.7% | 1.69 | 18.8% |
| 2024 | 2.48x | +42.2% | 1.68 | 17.8% |
| 2025 | 3.01x | +21.6% | 1.04 | 10.7% |
| 2026 | 2.65x | -10.7% | -3.45 | 10.7% |

## Files

- `snapshots/c19_rebal_exact_live_equity.csv`
- `snapshots/c19_rebal_exact_live_trades.csv`
