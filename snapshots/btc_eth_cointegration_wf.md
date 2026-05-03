# BTC-ETH Cointegration Walk-Forward (T52)
**Date: 2026-05-03 | Status: GRAVEYARD**

## Configuration
- Data: BTCUSDT + ETHUSDT daily, 3179 bars, 2018–2026
- Walk-forward: 6 windows, 252-bar training, rest OOS
- Fee: 10bps taker/leg/side (pair = 20bps round-trip) + 5bps slippage/leg/side
- Entry: |z-score| > entry threshold
- Exit: |z-score| < exit threshold OR max_hold bars
- Beta: OLS rolling estimate from training window (fixed for OOS period)

## Beta by Window
| Window | Train Period | Beta |
|--------|-------------|------|
| W1 | 0–252 | 0.0537 |
| W2 | 487–739 | 0.0176 |
| W3 | 974–1226 | 0.0314 |
| W4 | 1461–1713 | 0.0693 |
| W5 | 1948–2200 | 0.0494 |
| W6 | 2435–2687 | 0.0210 |

## Results
| Config | Trades | Wins | Sharpe | Return | MaxDD | Windows |
|--------|--------|------|--------|--------|-------|---------|
| LB20_E2.0_X1.0_H20 | 531 | 282 | -0.486 | -36.9% | 77.8% | 0/6 |
| LB20_E2.0_X0.5_H20 | 480 | 254 | -0.496 | -36.6% | 80.6% | 2/6 |
| LB20_E1.5_X0.5_H20 | 643 | 349 | -0.371 | -39.5% | 84.1% | 1/6 |
| LB20_E2.5_X1.0_H20 | 384 | 195 | -0.489 | -34.4% | 69.3% | 1/6 |
| LB40_E2.0_X1.0_H30 | 294 | 162 | -0.332 | -29.3% | 76.5% | 1/6 |
| LB40_E2.0_X0.5_H30 | 267 | 145 | -0.431 | -35.6% | 71.5% | 0/6 |
| LB40_E1.5_X0.5_H30 | 358 | 201 | -0.525 | -45.9% | 85.2% | 0/6 |
| LB40_E2.5_X1.0_H30 | 209 | 109 | -0.251 | -20.8% | 71.0% | 1/6 |
| LB60_E2.0_X1.0_H40 | 217 | 120 | -0.326 | -26.4% | 81.6% | 1/6 |
| LB60_E2.0_X0.5_H40 | 190 | 104 | -0.245 | -24.0% | 79.4% | 0/6 |
| LB60_E1.5_X0.5_H40 | 264 | 142 | -0.277 | -33.7% | 91.8% | 1/6 |
| LB60_E2.5_X1.0_H40 | 153 | 86  | -0.235 | -16.0% | 68.9% | 1/6 |

**VERDICT: REJECTED.** All 12 configs negative Sharpe, all negative returns.
Best config (LB60_E2.5_X1.0_H40): -0.235 Sharpe, -16.0% return, 68.9% DD, 1/6 windows.
Mechanism real (beta cointegrates) but spread edge insufficient vs fees.

## Conclusion
BTC-ETH cointegration: confirmed real, edge insufficient. GRAVEYARD.
Every MR approach in crypto has failed. Directional trend-following is the only viable edge.
