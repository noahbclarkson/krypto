# Live-Compatible Walk-Forward Results

**Strategy:** Turtle-only exit (matching `src/live/bot.rs` after 2026-05-01 bug fix)
- Entry: Turtle breakout (EP=21) + ATR_RANK(AP=12, LB=42, T=24) gate
- Exit: Turtle ATR trailing stop (AP=24, M=2.0) + HOLD_MAX=12
- Risk overlay: USDT 30% size when BTC 21d ATR > 75th pct of 252d history
- Fee: 0.10% taker (both sides)
- VOL_LOOKBACK: 8

## Global Results (52/63 pass, 252-bar windows)
| Metric | Value |
|--------|-------|
| Pass Rate | 52/63 (82.5%) |
| Avg Sharpe | 6.051 |
| Avg Return | 95.6% |

## Per-Universe Summary
| Universe | Pass | Sharpe | Return% | DD% | Trades |
|----------|-------|--------|---------|-----|--------|
| Base5 | 7/7 | 7.86 | 176.1% | 23.3% | 67 |
| NoDOGE | 6/7 | 6.87 | 107.2% | 18.6% | 62 |
| Legacy4 | 6/7 | 8.00 | 117.2% | 16.9% | 59 |
| Legacy5BNB | 7/7 | 9.90 | 110.7% | 13.6% | 52 |
| OldGuardNoBNB | 5/7 | 4.74 | 97.1% | 24.7% | 62 |
| LargeCaps5 | 6/7 | 5.98 | 73.8% | 21.2% | 61 |
| Legacy3 | 6/7 | 6.65 | 80.6% | 26.1% | 66 |
| LowVolume5 | 3/7 | -0.45 | 35.0% | 43.3% | 74 |
| OldGuard4 | 6/7 | 4.90 | 62.5% | 31.6% | 71 |

*Pass = windows with ≥3 trades AND Sharpe>0 / total windows.*
