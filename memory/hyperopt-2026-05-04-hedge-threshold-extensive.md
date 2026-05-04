# Hyperparameter Optimization: USDT Hedge Percentile Threshold (Extensive)
**Date:** 2026-05-04 15:21 UTC
**Author:** Kira

## Parameter Audited
**HEDGE_PCT** — BTC 21-day ATR percentile threshold for the USDT hedge position-size overlay.

Located at: `src/live/bot.rs` line 286, hardcoded as `0.75` (75th percentile).
When BTC 21d ATR > HEDGE_PCT-th percentile of its 252-bar history → position size *= 0.70.

**Never systematically optimized** — assumed correct since implementation.

## Extensive Range Tested
- **Range:** HEDGE_PCT ∈ [0..=100] step 1 — **101 values** (full logical range)
- **Validation:** Walk-forward across **9 universes × 7 windows = 63 OOS windows per value**
- **Total simulations:** 101 × 63 = 6,363 + additional equity curve runs
- **Harness:** `examples/usdt_hedge_threshold_extensive.rs`
- **Runtime:** 14.5 seconds

## Key Results
| PCT | Pass Rate | Sharpe | Return% | DD% | Base5 Eq | Mechanism |
|-----|-----------|--------|---------|-----|----------|-----------|
| 0 | 55/63 (87.3%) | 4.815 | 90.4% | 23.6% | 183x | Max hedge (always on) |
| 16 | **57/63 (90.5%)** | **5.041** | 95.4% | 23.6% | 216x | Near-always (84% bars) |
| 50 | 56/63 (88.9%) | 5.171 | 112.4% | 26.1% | 459x | Median threshold |
| **75** | **55/63 (87.3%)** | **4.910** | **129.7%** | **29.3%** | **623x** | **Current production** |
| 100 | 55/63 (87.3%) | 4.815 | 156.3% | 32.8% | 681x | Disabled (no hedge) |

## Finding: HEDGE_PCT is INERT (Pure Risk Knob)
**All 101 threshold values produce exactly 706 trades.** The overlay never blocks entries — it only scales position size.

- **Return and DD scale linearly** with the threshold: more hedging = less return AND less drawdown
- **Return/DD ratio is constant** (~4.0-4.8) across all thresholds — no risk-adjusted alpha
- **Pass rate improvement at PCT=16** is an artifact: globally shrinking positions makes 2 marginal windows barely pass by reducing DD below the fail threshold
- **PCT=16 is mechanistically equivalent to SIZE_MULT=0.70 globally** (fires 84% of the time)

## Verdict: NO CHANGE — HEDGE_PCT=75 CONFIRMED

The 75th percentile threshold is a reasonable default that:
1. Preserves most return (130% avg vs 156% disabled — only 17% sacrifice)
2. Reduces drawdown meaningfully (29.3% vs 32.8% — 10.7% reduction)
3. Only fires in genuinely high-vol regimes (top quartile of BTC ATR history)
4. Is mechanistically appropriate: hedge when volatility is abnormal, not always

PCT=16 "wins" pass rate by being equivalent to a global 30% position reduction, which is not a regime-conditional strategy — it's just smaller positions everywhere.

**Anti-spin:** This is the same result as the SIZE_MULT sweep (2026-05-03): the overlay is INERT as alpha. Both the threshold and the multiplier are cosmetic risk knobs. They are valid risk management tools but do not generate alpha.

## Files
- **Harness:** `examples/usdt_hedge_threshold_extensive.rs`
- **Sweep data:** `snapshots/hedge_threshold_extensive_sweep.csv`
- **Time-series equity:** `snapshots/hedge_threshold_extensive_timeseries.csv`
- **Summary:** `snapshots/hedge_threshold_extensive_summary.md`
- **Equity chart:** `charts/comparison_chart.png`
- **Heatmap chart:** `charts/hedge_threshold_heatmap.png`
- **Chart script:** `charts/plot_hedge_threshold.py`

## Chart Reference
`charts/comparison_chart.png` — Base5 compounded equity curves (log scale) + drawdown panel.
Shows Baseline (PCT=75), Winner (PCT=16), and multiple runner-ups across 1,618 bars of walk-forward data.

`charts/hedge_threshold_heatmap.png` — Pass rate and Sharpe bar charts across all 101 threshold values.
Red dashed line = current production (75), green dashed line = winner (16).
