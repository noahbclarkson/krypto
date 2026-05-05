# Live-Compatible Walk-Forward Results

**Strategy:** Turtle-only exit (matching `src/live/bot.rs` after 2026-05-01 bug fix)
- Entry: Turtle breakout (EP=21) + ATR_RANK(AP=12, LB=42, T=24) gate
- Exit: Turtle ATR trailing stop (AP=24, M=2.0) + HOLD_MAX=12
- Risk overlay: USDT 30% size when BTC 21d ATR > 75th pct of 252d history
- Fee: 0.10% taker (both sides)
- VOL_LOOKBACK: 92

## Global Results (57/63 pass, 252-bar windows)
| Metric | Value |
|--------|-------|
| Pass Rate | 57/63 (90.5%) |
| Avg Sharpe | 6.874 |
| Avg Return | 132.8% |

## Per-Universe Summary
| Universe | Pass | Sharpe | Return% | DD% | Trades |
|----------|-------|--------|---------|-----|--------|
| Base5 | 7/7 | 8.89 | 129.9% | 24.5% | 77 |
| NoDOGE | 7/7 | 10.87 | 132.6% | 16.4% | 74 |
| Legacy4 | 6/7 | 6.01 | 169.3% | 24.9% | 79 |
| Legacy5BNB | 7/7 | 7.95 | 191.6% | 18.2% | 70 |
| OldGuardNoBNB | 6/7 | 5.13 | 155.0% | 24.8% | 76 |
| LargeCaps5 | 7/7 | 10.46 | 130.7% | 16.4% | 73 |
| Legacy3 | 6/7 | 5.09 | 113.4% | 25.4% | 80 |
| LowVolume5 | 5/7 | 2.25 | 76.6% | 39.3% | 84 |
| OldGuard4 | 6/7 | 5.20 | 95.9% | 27.3% | 81 |

*Pass = windows with ≥3 trades AND Sharpe>0 / total windows.*
