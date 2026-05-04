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
| Avg Sharpe | 4.910 |
| Avg Return | 129.7% |

## Per-Universe Summary
| Universe | Pass | Sharpe | Return% | DD% | Trades |
|----------|-------|--------|---------|-----|--------|
| Base5 | 7/7 | 7.23 | 263.3% | 26.9% | 78 |
| NoDOGE | 7/7 | 8.19 | 122.8% | 22.4% | 76 |
| Legacy4 | 6/7 | 3.59 | 151.2% | 31.8% | 79 |
| Legacy5BNB | 6/7 | 3.77 | 154.9% | 24.6% | 70 |
| OldGuardNoBNB | 5/7 | 2.55 | 137.4% | 30.3% | 78 |
| LargeCaps5 | 7/7 | 7.51 | 96.3% | 23.7% | 74 |
| Legacy3 | 6/7 | 4.42 | 89.2% | 29.4% | 81 |
| LowVolume5 | 5/7 | 3.20 | 90.6% | 41.3% | 87 |
| OldGuard4 | 6/7 | 3.73 | 61.2% | 33.5% | 83 |

*Pass = windows with ≥3 trades AND Sharpe>0 / total windows.*
