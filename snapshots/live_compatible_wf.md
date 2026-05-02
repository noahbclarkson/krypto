# Live-Compatible Walk-Forward Results

**Strategy:** Turtle-only exit (matching `src/live/bot.rs` after 2026-05-01 bug fix)
- Entry: Turtle breakout (EP=21) + ATR_RANK(AP=12, LB=42, T=5) gate
- Exit: Turtle ATR trailing stop (AP=24, M=2.0) + HOLD_MAX=12
- Risk overlay: USDT 30% size when BTC 21d ATR > 75th pct of 252d history
- Fee: 0.10% taker (both sides)
- VOL_LOOKBACK: 8

## Global Results (55/63 pass, 252-bar windows)
| Metric | Value |
|--------|-------|
| Pass Rate | 55/63 (87.3%) |
| Avg Sharpe | 6.188 |
| Avg Return | 73.8% |

## Per-Universe Summary
| Universe | Pass | Sharpe | Return% | DD% | Trades |
|----------|-------|--------|---------|-----|--------|
| Base5 | 6/7 | 4.47 | 101.4% | 20.5% | 52 |
| NoDOGE | 7/7 | 9.49 | 66.4% | 16.8% | 52 |
| Legacy4 | 7/7 | 8.82 | 101.5% | 13.7% | 49 |
| Legacy5BNB | 6/7 | 11.01 | 108.5% | 11.4% | 44 |
| OldGuardNoBNB | 6/7 | 3.71 | 84.0% | 22.1% | 51 |
| LargeCaps5 | 7/7 | 9.17 | 47.5% | 17.0% | 49 |
| Legacy3 | 6/7 | 6.12 | 67.9% | 17.9% | 53 |
| LowVolume5 | 5/7 | 0.80 | 43.2% | 31.1% | 57 |
| OldGuard4 | 5/7 | 2.11 | 44.3% | 26.8% | 57 |

*Pass = windows with ≥3 trades AND Sharpe>0 / total windows.*
