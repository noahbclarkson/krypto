# Live-Compatible Walk-Forward Results

**Strategy:** Turtle-only exit (matching `src/live/bot.rs` after 2026-05-01 bug fix)
- Entry: Turtle breakout (EP=21) + ATR_RANK(AP=12, LB=42, T=5) gate
- Exit: Turtle ATR trailing stop (AP=24, M=2.0) + HOLD_MAX=12
- Risk overlay: USDT 30% size when BTC 21d ATR > 75th pct of 252d history
- Fee: 0.10% taker (both sides)
- VOL_LOOKBACK: 8

## Global Results (45/63 pass, 252-bar windows)
| Metric | Value |
|--------|-------|
| Pass Rate | 45/63 (71.4%) |
| Avg Sharpe | 3.315 |
| Avg Return | 121.0% |

## Per-Universe Summary
| Universe | Pass | Sharpe | Return% | DD% | Trades |
|----------|-------|--------|---------|-----|--------|
| Base5 | 4/7 | 2.35 | 157.7% | 38.2% | 81 |
| NoDOGE | 5/7 | 4.41 | 56.1% | 31.7% | 80 |
| Legacy4 | 5/7 | 4.18 | 135.6% | 28.0% | 77 |
| Legacy5BNB | 6/7 | 3.00 | 137.7% | 30.4% | 71 |
| OldGuardNoBNB | 5/7 | 2.37 | 107.2% | 34.3% | 81 |
| LargeCaps5 | 5/7 | 4.52 | 51.5% | 31.1% | 78 |
| Legacy3 | 6/7 | 4.87 | 99.3% | 28.6% | 79 |
| LowVolume5 | 4/7 | 1.70 | 285.9% | 42.3% | 90 |
| OldGuard4 | 5/7 | 2.42 | 57.8% | 35.4% | 86 |

*Pass = windows with ≥3 trades AND Sharpe>0 / total windows.*
