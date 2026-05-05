# Live-Compatible Walk-Forward Results

**Strategy:** Turtle-only exit (matching `src/live/bot.rs` after 2026-05-01 bug fix)
- Entry: Turtle breakout (EP=21) + ATR_RANK(AP=17, LB=42, T=5) gate
- Exit: Turtle ATR trailing stop (AP=24, M=2.0) + HOLD_MAX=12
- Risk overlay: USDT 30% size when BTC 21d ATR > 45th pct of 252d history
- Fee: 0.10% taker (both sides)
- VOL_LOOKBACK: 92

## Global Results (59/63 pass, 252-bar windows)
| Metric | Value |
|--------|-------|
| Pass Rate | 59/63 (93.7%) |
| Avg Sharpe | 7.577 |
| Avg Return | 72.4% |

## Per-Universe Summary
| Universe | Pass | Sharpe | Return% | DD% | Trades |
|----------|-------|--------|---------|-----|--------|
| Base5 | 7/7 | 8.82 | 87.9% | 15.7% | 77 |
| NoDOGE | 7/7 | 10.45 | 87.5% | 12.1% | 74 |
| Legacy4 | 7/7 | 7.33 | 79.1% | 16.1% | 79 |
| Legacy5BNB | 7/7 | 9.23 | 87.8% | 11.8% | 70 |
| OldGuardNoBNB | 6/7 | 6.42 | 70.4% | 16.0% | 76 |
| LargeCaps5 | 7/7 | 10.34 | 88.9% | 12.1% | 73 |
| Legacy3 | 6/7 | 6.39 | 64.2% | 14.8% | 80 |
| LowVolume5 | 5/7 | 2.34 | 25.9% | 29.6% | 84 |
| OldGuard4 | 7/7 | 6.88 | 59.9% | 15.8% | 81 |

*Pass = windows with ≥3 trades AND Sharpe>0 / total windows.*
