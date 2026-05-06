# T73 Top-Winner Conditions Audit

Source: `snapshots/live_bot_exact_trades.csv` from exact live-bot replay after T75 (`2.81x / Sharpe 1.01 / MaxDD 28.2% / 298 trades`).

Method: rank trades by `ln(account_equity_mult)` because that is the contribution to compounded account equity. Features are computed at the entry date from cached daily Base5 candles. BTC trend regime = bull when BTC close and SMA50 are above SMA200, bear when both below SMA200, otherwise chop. Dollar-volume rank is VL=92 rolling average volume × current close among Base5, top-3 gate matching the rejected T72 candidate. ATR_RANK uses live bot AP=17/LB=41 normalized BTC ATR percentile semantics.

## Summary

- Top 10 log contributors account for **82.8%** of total positive/negative compounded log return in this replay.
- Symbols: SOLUSDT=4, XRPUSDT=2, DOGEUSDT=1, BTCUSDT=1, ADAUSDT=1, ETHUSDT=1
- BTC trend regimes at entry: bear=5, chop=3, bull=2
- Exit reasons: TURTLE_ATR=9, HOLD_MAX=1
- Checked-filter top-winner kills: weekend=0/10, dv_top3=8/10, ATR_RANK>=24=5/10, ATR_RANK>=65=8/10, low_vol_filter=2/10

## Guardrail conclusions

- **Do not add the VL=92 top-3 dollar-volume gate.** It would have excluded 8/10 top winners in the exact-live ledger, matching the T72 replay collapse.
- **Do not revisit high ATR_RANK thresholds.** A T=24 gate would have skipped 5/10 top winners; T=65 would have skipped 8/10. This is the convex-tail reason the held-out high-threshold variants were dangerous.
- **Weekend filter did not hit this exact top-10 set** (0/10), but T62 remains rejected on full walk-forward metrics; do not use this audit alone to revive it.
- **Low-vol/chop filters are directly harmful** for 2/10 top winners, including the largest two contributors. BTC trend labels are also mixed: the largest winners are not all clean bull-trend entries.
- **HOLD_MAX is not a dominant top-winner exit in this exact replay:** 1/10 top winners exited by HOLD_MAX; most convex winners resolved quickly through the Turtle ATR path.

## Top 10 table

| # | Symbol | Entry | Regime | BTC ATR pct | Sym ATR pct | DV rank | Weekend? | Vol regime | Exit | Held | Eq mult | Filters that would exclude |
|---:|---|---|---|---:|---:|---:|---|---|---|---:|---:|---|
| 1 | SOLUSDT | 2023-01-11 (Wed) | bear | 14.6 | 14.7 | 5 | no | low_vol_q1 | TURTLE_ATR | 3 | 1.1602 | DV top-3 gate(rank 5); ATR_RANK>=24(14.6); ATR_RANK>=65(14.6); low-vol/chop filter |
| 2 | DOGEUSDT | 2022-10-28 (Fri) | bear | 22.0 | 19.4 | 5 | no | low_vol_q1 | TURTLE_ATR | 1 | 1.1502 | DV top-3 gate(rank 5); ATR_RANK>=24(22.0); ATR_RANK>=65(22.0); low-vol/chop filter |
| 3 | SOLUSDT | 2021-07-30 (Fri) | bear | 19.5 | 61.5 | 6 | no | mid_vol | HOLD_MAX | 12 | 1.0951 | DV top-3 gate(rank 6); ATR_RANK>=24(19.5); ATR_RANK>=65(19.5) |
| 4 | SOLUSDT | 2021-08-27 (Fri) | chop | 9.8 | 91.3 | 6 | no | mid_vol | TURTLE_ATR | 3 | 1.0840 | DV top-3 gate(rank 6); ATR_RANK>=24(9.8); ATR_RANK>=65(9.8) |
| 5 | BTCUSDT | 2021-10-04 (Mon) | bull | 56.1 | 27.4 | 1 | no | high_vol_q4 | TURTLE_ATR | 11 | 1.0840 | ATR_RANK>=65(56.1) |
| 6 | ADAUSDT | 2021-08-04 (Wed) | bear | 53.7 | 36.9 | 4 | no | mid_vol | TURTLE_ATR | 6 | 1.0727 | DV top-3 gate(rank 4); ATR_RANK>=65(53.7) |
| 7 | XRPUSDT | 2024-11-28 (Thu) | bull | 70.7 | 100.0 | 5 | no | mid_vol | TURTLE_ATR | 3 | 1.0648 | DV top-3 gate(rank 5) |
| 8 | SOLUSDT | 2021-08-13 (Fri) | chop | 43.9 | 65.5 | 6 | no | mid_vol | TURTLE_ATR | 2 | 1.0644 | DV top-3 gate(rank 6); ATR_RANK>=65(43.9) |
| 9 | XRPUSDT | 2021-08-10 (Tue) | chop | 70.7 | 24.2 | 5 | no | mid_vol | TURTLE_ATR | 1 | 1.0621 | DV top-3 gate(rank 5) |
| 10 | ETHUSDT | 2021-07-28 (Wed) | bear | 12.2 | 40.5 | 2 | no | mid_vol | TURTLE_ATR | 7 | 1.0611 | ATR_RANK>=24(12.2); ATR_RANK>=65(12.2) |

## Notes

- `account_equity_mult` includes position size, hedge sizing, and exact live-bot fee assumptions; raw symbol return can be larger.
- This audit is descriptive guardrail analysis, not a new strategy. It argues against adding more entry filters until a filter is explicitly proven not to delete convex winners.
