# Live-Compatible Walk-Forward Results

**Strategy:** Turtle-only exit (matching `src/live/bot.rs` after 2026-05-01 bug fix)
- Entry: Turtle breakout (EP=21) + ATR_RANK(AP=12, LB=42, T=24) gate
- Exit: Turtle ATR trailing stop (AP=24, M=2.0) + HOLD_MAX=12
- Risk overlay: USDT 30% size when BTC 21d ATR > 75th pct of 252d history
- Fee: 0.10% taker (both sides)
- VOL_LOOKBACK: 96

## Global Results (54/63 pass, 252-bar windows)
| Metric | Value |
|--------|-------|
| Pass Rate | 54/63 (85.7%) |
| Avg Sharpe | 7.652 |
| Avg Return | 138.3% |

## Per-Universe Summary
| Universe | Pass | Sharpe | Return% | DD% | Trades |
|----------|-------|--------|---------|-----|--------|
| Base5 | 7/7 | 9.34 | 158.3% | 21.7% | 63 |
| NoDOGE | 7/7 | 10.18 | 118.2% | 14.9% | 60 |
| Legacy4 | 6/7 | 9.06 | 207.8% | 14.6% | 59 |
| Legacy5BNB | 6/7 | 9.57 | 190.8% | 10.9% | 52 |
| OldGuardNoBNB | 4/7 | 4.94 | 186.6% | 21.1% | 59 |
| LargeCaps5 | 7/7 | 9.84 | 102.9% | 15.1% | 58 |
| Legacy3 | 6/7 | 7.01 | 86.3% | 23.4% | 65 |
| LowVolume5 | 4/7 | 1.42 | 109.5% | 37.6% | 72 |
| OldGuard4 | 7/7 | 7.50 | 84.3% | 26.0% | 68 |

*Pass = windows with ≥3 trades AND Sharpe>0 / total windows.*
