# T84 Hyperopt Report — HEDGE_LOOKBACK (2026-05-07)

## Parameter Audited

**HEDGE_LOOKBACK** — the true-range history lookback used in the USDT hedge overlay.
Hardcoded at `252` since the hedge was first added. Never systematically tested under current production params.

**Current production context:**
- HEDGE_ATR_PERIOD=38 (T75 winner)
- HEDGE_ATR_PCT=0.45 (T67 winner)
- HEDGE_SIZE_MULT=0.25 (T83 winner)
- ATR_RANK(AP=17, LB=41, T=5)
- TurtleATR(24, 2.0), EP=21, HOLD_MAX=15, CAP=3

## Sweep Design

- **Harness:** Walk-forward, exact-live Turtle-only path (mirrors `live_bot_exact_equity.rs`)
- **Range:** LB ∈ {21, 42, 63, 84, 105, 126, 147, 168, 189, 210, 252, 294, 336, 378, 420, 504, 630} — 17 values
- **Validation:** 9 universes × ~7 walk-forward windows = 63 OOS windows per LB value
- **Metrics:** universes-passed, avg Sharpe, avg return, avg DD, total trades

## Results

| HEDGE_LOOKBACK | Universes Pass | Pass Rate | Avg Sharpe | Avg Return | Avg DD | Trades |
|:---:|:---:|:---:|:---:|:---:|:---:|:---:|
| **21** | 8/9 | 88.9% | 0.834 | +14.7% | 9.1% | 2889 |
| **42** | 8/9 | 88.9% | 0.935 | +15.3% | 9.0% | 2889 |
| **63** | 8/9 | 88.9% | 1.035 | +15.9% | 8.7% | 2889 |
| 84 | 8/9 | 88.9% | 0.912 | +12.0% | 8.7% | 2889 |
| **105** | **9/9** | **100%** | 0.924 | +10.7% | 8.2% | 2889 |
| **126** | **9/9** | **100%** | 1.189 | +12.0% | 6.5% | 2889 |
| **147** | **9/9** | **100%** | **1.243** | +10.8% | 5.3% | 2889 |
| **168** | **9/9** | **100%** | 1.173 | +8.8% | 4.2% | 2889 |
| 189 | 8/9 | 88.9% | 1.135 | +8.5% | 4.2% | 2889 |
| 210 | 8/9 | 88.9% | 1.135 | +8.5% | 4.2% | 2889 |
| **252 (baseline)** | 8/9 | 88.9% | 1.135 | +8.5% | 4.2% | 2889 |
| 294 | 8/9 | 88.9% | 1.092 | +8.4% | 4.3% | 2889 |
| 336 | 8/9 | 88.9% | 1.020 | +11.9% | 5.3% | 2889 |
| **378** | **9/9** | **100%** | 0.994 | +13.4% | 6.7% | 2889 |
| **420** | **9/9** | **100%** | 1.046 | +17.3% | 7.0% | 2889 |
| 504 | 8/9 | 88.9% | 1.105 | +23.9% | 7.5% | 2889 |
| 630 | 8/9 | 88.9% | 1.088 | +27.2% | 9.1% | 2889 |

**Walk-forward winner:** LB=147 (Sharpe 1.243, 9/9 pass, DD 5.3%)

## Exact-Live Verification

`live_bot_exact_equity.rs` with HEDGE_LOOKBACK=147 (updated config):

| Metric | LB=147 | LB=252 (prev) | Delta |
|---|:---:|:---:|:---:|
| Equity | **2.10x** | **2.77x** | **-24.2%** |
| Daily Sharpe | **0.87** | **1.03** | -0.16 |
| MaxDD | **16.4%** | **22.3%** | -5.9pp |
| Trades | 286 | 286 | 0 |

**LB=147 loses to LB=252 on the exact live path.** The walk-forward harness winner (LB=147: Sharpe 1.243) does NOT generalize to the actual live bot path.

## Decision

**HEDGE_LOOKBACK = 252 remains the production default.**

The T84 result is another instantiation of the established harness-gap pattern:

| Candidate | Harness | Exact-Live | Delta |
|---|---|---|---|
| T72 VOL gate | passed | 1.01x vs 2.56x | -60.5% |
| T69 semantic align | passed | 1.02x vs 2.55x | -60.0% |
| T84 LB=147 | Sharpe 1.243, 9/9 | 2.10x vs 2.77x | -24.2% |

The USDT hedge overlay's position-sizing mechanism is MORE sensitive to HEDGE_LOOKBACK in the exact-live path than in the research walk-forward harness. The hedge triggers less often with LB=147 (shorter window → higher threshold), leading to less position reduction → larger positions → worse performance in the exact live accounting.

## Anti-Pattern Confirmed

> **Every candidate that "wins" in the research walk-forward harness fails or degrades on exact-live replay.**

The walk-forward harness (9-universe × 7-window) is measuring something structurally different from the exact live path. The gap is now established across 4 candidates. No parameter from the research harness has ever improved exact-live performance when promoted.

**Bottom line:** The exact-live 2.77x is the honest production target. Research harness parameters are for diagnostic understanding only, not production configuration.

## Charts

- `/home/ubuntu/.openclaw/workspace-krypto/charts/comparison_chart.png` — HEDGE_LOOKBACK sweep metrics
- `/home/ubuntu/.openclaw/workspace-krypto/charts/t84_table.png` — full results table

## Files

- `examples/t84_hedge_lookback_extensive.rs` — sweep harness
- `snapshots/t84_hedge_lookback_summary.csv` — global summary
- `snapshots/t84_hedge_lookback_windows.csv` — per-universe per-window results
