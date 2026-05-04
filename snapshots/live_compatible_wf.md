# Live-Compatible Walk-Forward Results

**Strategy:** Turtle-only exit (matching `src/live/bot.rs` after 2026-05-01 bug fix)
- Entry: Turtle breakout (EP=21) + ATR_RANK(AP=12, LB=42, T=24) gate
- Exit: Turtle ATR trailing stop (AP=24, M=2.0) + HOLD_MAX=12
- Risk overlay: USDT 30% size when BTC 21d ATR > 75th pct of 252d history
- Fee: 0.10% taker (both sides)
- VOL_LOOKBACK: 96

## Global Results (55/63 pass, 252-bar windows)
| Metric | Value |
|--------|-------|
| Pass Rate | 55/63 (87.3%) |
| Avg Sharpe | 6.371 |
| Avg Return | 150.8% |

## Per-Universe Summary
| Universe | Pass | Sharpe | Return% | DD% | Trades |
|----------|-------|--------|---------|-----|--------|
| Base5 | 7/7 | 8.91 | 198.6% | 23.8% | 75 |
| NoDOGE | 7/7 | 10.56 | 154.1% | 18.2% | 74 |
| Legacy4 | 6/7 | 5.02 | 163.8% | 30.9% | 77 |
| Legacy5BNB | 6/7 | 7.31 | 199.9% | 19.8% | 68 |
| OldGuardNoBNB | 5/7 | 3.56 | 147.9% | 29.1% | 74 |
| LargeCaps5 | 7/7 | 10.15 | 143.5% | 18.1% | 73 |
| Legacy3 | 6/7 | 4.15 | 73.9% | 28.4% | 79 |
| LowVolume5 | 5/7 | 3.40 | 217.5% | 37.8% | 82 |
| OldGuard4 | 6/7 | 4.27 | 58.1% | 31.6% | 80 |

*Pass = windows with ≥3 trades AND Sharpe>0 / total windows.*
