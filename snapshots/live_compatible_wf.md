# Live-Compatible Walk-Forward Results

**Strategy:** Turtle-only exit (matching `src/live/bot.rs` after 2026-05-01 bug fix)
- Entry: Turtle breakout (EP=21) + ATR_RANK(AP=17, LB=42, T=5) gate
- Exit: Turtle ATR trailing stop (AP=24, M=2.0) + HOLD_MAX=12
- Risk overlay: USDT 30% size when BTC 21d ATR > 45th pct of 252d history
- Fee: 0.10% taker (both sides)
- VOL_LOOKBACK: 92

## Global Results (58/63 pass, 252-bar windows)
| Metric | Value |
|--------|-------|
| Pass Rate | 58/63 (92.1%) |
| Avg Sharpe | 7.079 |
| Avg Return | 114.6% |

## Per-Universe Summary
| Universe | Pass | Sharpe | Return% | DD% | Trades |
|----------|-------|--------|---------|-----|--------|
| Base5 | 7/7 | 8.80 | 119.3% | 21.0% | 77 |
| NoDOGE | 7/7 | 10.54 | 119.9% | 15.4% | 74 |
| Legacy4 | 6/7 | 6.45 | 140.9% | 21.4% | 79 |
| Legacy5BNB | 7/7 | 8.27 | 160.6% | 15.8% | 70 |
| OldGuardNoBNB | 6/7 | 5.69 | 130.2% | 20.9% | 76 |
| LargeCaps5 | 7/7 | 10.24 | 122.7% | 15.4% | 73 |
| Legacy3 | 6/7 | 5.54 | 92.5% | 22.0% | 80 |
| LowVolume5 | 5/7 | 2.39 | 64.3% | 35.3% | 84 |
| OldGuard4 | 7/7 | 5.78 | 80.9% | 23.6% | 81 |

*Pass = windows with ≥3 trades AND Sharpe>0 / total windows.*
