# Live-Compatible Walk-Forward Results

**Strategy:** Turtle-only exit (matching `src/live/bot.rs` after 2026-05-01 bug fix)
- Entry: Turtle breakout (EP=21) + ATR_RANK(AP=12, LB=42, T=5.0) gate
- Exit: Turtle ATR trailing stop (AP=24, M=2.0) + HOLD_MAX=12
- Risk overlay: USDT 30% size when BTC 21d ATR > 50th pct of 252d history
- Fee: 0.10% taker (both sides)
- VOL_LOOKBACK: 96

## Global Results (56/63 pass, 252-bar windows)
| Metric | Value |
|--------|-------|
| Pass Rate | 56/63 (88.9%) |
| Avg Sharpe | 5.171 |
| Avg Return | 112.4% |

## Per-Universe Summary
| Universe | Pass | Sharpe | Return% | DD% | Trades |
|----------|-------|--------|---------|-----|--------|
| Base5 | 7/7 | 7.50 | 219.8% | 25.1% | 78 |
| NoDOGE | 7/7 | 8.02 | 112.7% | 21.0% | 76 |
| Legacy4 | 6/7 | 3.88 | 129.2% | 27.4% | 79 |
| Legacy5BNB | 6/7 | 4.16 | 133.2% | 20.9% | 70 |
| OldGuardNoBNB | 5/7 | 2.86 | 118.1% | 26.0% | 78 |
| LargeCaps5 | 7/7 | 7.44 | 92.7% | 22.0% | 74 |
| Legacy3 | 6/7 | 5.08 | 78.4% | 26.1% | 81 |
| LowVolume5 | 6/7 | 3.28 | 72.1% | 37.7% | 87 |
| OldGuard4 | 6/7 | 4.33 | 55.7% | 29.1% | 83 |

*Pass = windows with ≥3 trades AND Sharpe>0 / total windows.*
