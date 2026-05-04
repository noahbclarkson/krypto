# Live-Compatible Walk-Forward Results

**Strategy:** Turtle-only exit (matching `src/live/bot.rs` after 2026-05-01 bug fix)
- Entry: Turtle breakout (EP=21) + ATR_RANK(AP=12, LB=42, T=24) gate
- Exit: Turtle ATR trailing stop (AP=24, M=2.0) + HOLD_MAX=12
- Risk overlay: USDT 30% size when BTC 21d ATR > 75th pct of 252d history
- Fee: 0.10% taker (both sides)
- VOL_LOOKBACK: 96

## Global Results (56/63 pass, 252-bar windows)
| Metric | Value |
|--------|-------|
| Pass Rate | 56/63 (88.9%) |
| Avg Sharpe | 6.106 |
| Avg Return | 116.1% |

## Per-Universe Summary
| Universe | Pass | Sharpe | Return% | DD% | Trades |
|----------|-------|--------|---------|-----|--------|
| Base5 | 7/7 | 8.80 | 192.6% | 19.7% | 69 |
| NoDOGE | 7/7 | 9.35 | 83.0% | 18.5% | 66 |
| Legacy4 | 6/7 | 5.84 | 130.8% | 20.4% | 66 |
| Legacy5BNB | 7/7 | 6.18 | 131.3% | 17.6% | 60 |
| OldGuardNoBNB | 4/7 | 2.08 | 104.7% | 25.8% | 65 |
| LargeCaps5 | 7/7 | 9.12 | 66.8% | 19.9% | 64 |
| Legacy3 | 6/7 | 4.26 | 67.8% | 24.2% | 70 |
| LowVolume5 | 6/7 | 5.60 | 221.0% | 29.1% | 70 |
| OldGuard4 | 6/7 | 3.72 | 47.1% | 27.2% | 69 |

*Pass = windows with ≥3 trades AND Sharpe>0 / total windows.*
