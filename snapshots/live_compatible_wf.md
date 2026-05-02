# Live-Compatible Walk-Forward Results

**Strategy:** Turtle-only exit (matching `src/live/bot.rs` after 2026-05-01 bug fix)
- Entry: Turtle breakout (EP=21) + ATR_RANK(AP=12, LB=42, T=24) gate
- Exit: Turtle ATR trailing stop (AP=24, M=2.0) + HOLD_MAX=12
- Risk overlay: USDT 30% size when BTC 21d ATR > 75th pct of 252d history
- Fee: 0.10% taker (both sides)
- VOL_LOOKBACK: 8

## Global Results (53/63 pass, 252-bar windows)
| Metric | Value |
|--------|-------|
| Pass Rate | 53/63 (84.1%) |
| Avg Sharpe | 5.428 |
| Avg Return | 85.4% |

## Per-Universe Summary
| Universe | Pass | Sharpe | Return% | DD% | Trades |
|----------|-------|--------|---------|-----|--------|
| Base5 | 7/7 | 7.14 | 141.1% | 22.0% | 64 |
| NoDOGE | 7/7 | 6.43 | 64.9% | 21.0% | 61 |
| Legacy4 | 6/7 | 7.77 | 100.5% | 19.7% | 60 |
| Legacy5BNB | 6/7 | 7.63 | 92.9% | 19.3% | 53 |
| OldGuardNoBNB | 5/7 | 3.48 | 77.9% | 28.8% | 62 |
| LargeCaps5 | 7/7 | 6.08 | 50.5% | 22.1% | 60 |
| Legacy3 | 6/7 | 6.17 | 77.3% | 26.0% | 66 |
| LowVolume5 | 4/7 | 1.77 | 114.1% | 46.3% | 77 |
| OldGuard4 | 5/7 | 2.39 | 49.7% | 35.0% | 71 |

*Pass = windows with ≥3 trades AND Sharpe>0 / total windows.*
