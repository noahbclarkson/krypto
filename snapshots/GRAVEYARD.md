
## BTC-ETH Cointegration Pair Trade (2026-05-03) — GRAVEYARD
**T52 | BTC-ETH Cointegration Walk-Forward | Rejected decisively**

### Mechanism
Rolling OLS beta (BTC→ETH hedge ratio) over 252-bar training windows. Spread = ETH - beta×BTC.
Z-score of spread triggers mean-reversion entry/exit. Equal-weight pair allocation.

### Configuration
12 configs tested: LB ∈ {20,40,60} × z_entry ∈ {1.5,2.0,2.5} × z_exit ∈ {0.5,1.0}

### Results (6 walk-forward windows, 3179 daily bars, 2018–2026)
| Metric | Range |
|--------|-------|
| Sharpe | -0.235 to -0.525 (ALL negative) |
| Return | -16% to -45% (ALL negative) |
| MaxDD | 69% to 92% (ALL catastrophic) |
| Windows pass | 0-2/6 (best: LB60_E2.5_X1.0_H40 = 17%) |
| Guardrail | ≥60% windows, ≥30 trades, Sharpe>0 → ALL FAIL |

### Mechanism verdict
The cointegrating beta IS real (0.0176 to 0.0693 across regimes). But the spread's
mean-reversion is too slow and too small to recover fees+slippage on a two-leg pair.
Equal-weight allocation means each leg pays full taker+slipeach side (20bps + 10bps/leg/side).
Gross spread return ≈ 0; costs are prohibitive.

### Academic backing
Frontiers in Finance, Jan 2026 — BTC-ETH cointegrating coefficient ~0.0587.
**Confirmed real, but edge insufficient for live trading.**

### Files
`examples/btc_eth_cointegration_wf.rs` — harness (12 configs × 6 WF windows)

### Lesson
Every mean-reversion approach in crypto has failed: RSI, BollingerReversion, OFI, VPIN,
4h MR, 1h MR, funding rate, basis carry, and now BTC-ETH cointegration.
The reliable crypto edge is directional trend-following. Accept it. Stop searching MR in crypto.
