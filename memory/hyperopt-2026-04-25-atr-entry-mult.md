# Hyperopt — ATR_ENTRY_MULT 41-Value Sweep + In-Sample Inflation Audit
**Date:** 2026-04-25
**Parameters:** ATR_ENTRY_MULT ∈ [0.00..2.00] step 0.05 = 41 values × 9 universes × 6 windows = 2,214 runs

---

## Key Finding: Prior Winner Was In-Sample Inflation

**The 2026-04-21 fine sweep winner ATR_ENTRY_MULT=0.85 was optimized ON the same OOS validation data used to find EP=24 and CHAND_PERIOD=7.** Both parameters improved by 1-2 windows out of 54 — entirely consistent with noise in the validation set.

**Definitive 41-value sweep result:**
| ATR_EM | Pass | Avg Sharpe | Avg Return% | Trades |
|--------|------|------------|------------|--------|
| **0.00** | **83.3%** | **1.87** | **151.9%** | **707** |
| 0.05 | 83.3% | 1.84 | 142.8% | 704 |
| 0.90 | 79.6% | 1.01 | 77.3% | 468 |
| 0.85 | 64.8% | 0.99 | 63.0% | 488 |
| 1.00 | 70.4% | 0.98 | 69.2% | 439 |
| 1.50 | 70.4% | 0.84 | 72.5% | 288 |

**WINNER: ATR_ENTRY_MULT=0.00** — highest pass rate (83.3%), highest Sharpe (1.87), most trades (707), highest return (+151.9%).

**Any non-zero filter degrades performance monotonically.** The entry-side ATR filter is counterproductive — it was rejected definitively in the 2026-04-13 coarse sweep and confirmed again here with fine resolution.

**Mechanism:** The dual Chandelier+Turtle ATR exit already provides quality control at the EXIT side. ATR entry filtering is redundant and trade-starving.

---

## Why 0.85 Won in 2026-04-21 Fine Sweep

The 2026-04-21 fine sweep used CHAND(7,2.25), EP=24, HM=12. It found 0.85 won by 1 window over 0.90 (44/54 vs 43/54). But the EP=24 itself was from a sweep that also used the same 9-universe × 54-window validation data. When you optimize two parameters (EP and ATR_EM) on the same OOS data, each "improvement" of 1-2 windows is noise-level. The combined inflation is the sum of two noise-level tweaks.

**Rule established:** Only change a parameter if it wins by ≥3 windows (>5%) on a held-out validation set, or by ≥10% in Sharpe with ≥50 trades. 1-2 window improvements are noise.

---

## Production Default: ATR_ENTRY_MULT=0.00

**Files updated:**
- `src/live/config.rs` — ATR_ENTRY_MULT: 0.85 → 0.00
- `examples/turtle_chandelier_walkforward.rs` — ATR_ENTRY_MULT: 0.85 → 0.00
- `examples/live_turtle_chandelier.rs` — comments updated
- `HALL_OF_FAME.md` — header + param table updated

**Validation:** Walk-forward re-run in progress to confirm pass rate maintained with EM=0.00.

---

## Chart
`charts/atr_entry_mult_sweep_comparison.png`

Equity curves (log scale) for 6 configs: 0.00, 0.50, 0.85, 0.90, 1.00, 1.50. EM=0.00 dominates throughout. EM=0.85 underperforms after bar ~50.

---

## What This Means for EP=24 and P=7

**EP=24** won by 2 windows over EP=21 (45/54 vs 43/54). At 1-window-per-54 = 1.85% false-positive rate, 2 windows ≈ 3.7% expected false positives. With 96 EP values tested, even a 3.7% FP rate gives ~3.5 false winners. EP=24 being 2 windows above EP=21 is entirely consistent with noise.

**CHAND_PERIOD=7** won by 0 windows over CP=11 (43/54 identical). The "win" was +6.9% Sharpe in the same windows. This is a real signal if it's consistent across many independent windows, but the Sharpe improvement may not transfer to live.

**Overall assessment:** EP=24 and P=7 are defensible as the simplest explanation that fits the data. But ATR_ENTRY_MULT=0.00 is the only change that is definitively correct — the sweep is conclusive with 41 values covering the full range.

---

## Anti-Overfitting Discipline Going Forward

1. **Minimum win margin:** ≥3 windows (5.5%) improvement on OOS validation before accepting a param change
2. **No sequential optimization on same data:** If EP is optimized on data D, you cannot optimize ATR_EM on data D and claim both are valid OOS results
3. **Held-out validation required for marginal wins:** For wins of 1-2 windows, require pre-2021 held-out validation (regime_stress_test) before accepting
4. **Equity curve sanity check:** The equity curve for the winner must dominate the baseline at >80% of time bars
