# Live-Compatible Walk-Forward Results

**Strategy:** Turtle-only exit (matching `src/live/bot.rs` after 2026-05-01 bug fix)
- Entry: Turtle breakout (EP=21) + ATR_RANK(AP=12, LB=42, T=5) gate
- Exit: Turtle ATR trailing stop (AP=24, M=2.0) + HOLD_MAX=12
- Risk overlay: USDT 30% size when BTC 21d ATR > 75th pct of 252d history
- Fee: 0.10% taker (both sides)
- VOL_LOOKBACK: 8

## Global Results (52/63 pass, 252-bar windows)
| Metric | Value |
|--------|-------|
| Pass Rate | 52/63 (82.5%) |
| Avg Sharpe | 5.590 |
| Avg Return | 132.3% |

## Per-Universe Summary
| Universe | Pass | Sharpe | Return% | DD% | Trades |
|----------|-------|--------|---------|-----|--------|
| Base5 | 7/7 | 6.98 | 136.0% | 21.5% | 63 |
| NoDOGE | 7/7 | 6.10 | 59.3% | 20.7% | 61 |
| Legacy4 | 6/7 | 8.09 | 179.5% | 14.3% | 58 |
| Legacy5BNB | 6/7 | 7.22 | 188.0% | 12.9% | 52 |
| OldGuardNoBNB | 5/7 | 4.36 | 159.3% | 24.6% | 61 |
| LargeCaps5 | 7/7 | 5.44 | 47.4% | 23.2% | 61 |
| Legacy3 | 6/7 | 6.11 | 114.0% | 23.7% | 65 |
| LowVolume5 | 3/7 | 2.55 | 225.3% | 39.9% | 75 |
| OldGuard4 | 5/7 | 3.47 | 81.8% | 30.6% | 71 |

*Pass = windows with ≥3 trades AND Sharpe>0 / total windows.*
