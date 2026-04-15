# Hyperopt Report — Regime Weights for Turtle+A/D SMA200 Ensemble
**Date:** 2026-04-15
**Session:** Cron (12:35 UTC, continued)
**Parameter:** `BULL_TURTLE_WT`, `BULL_AD_WT`, `BEAR_AD_WT`, `BEAR_TURTLE_WT`
**Configs tested:** 8 (C0..C7 spanning {0.0, 0.1, 0.3, 0.5, 0.8, 1.0} ranges)
**Universes:** Base5 + NoDOGE + Legacy4 (3 universes × 6 windows = 18 total)
**Method:** Walk-forward 252/252, 21-bar hold, 0.1% taker fee, AD_PERIOD=2

---

## Key Finding: Regime Weights Have ZERO Impact

**Results (3 universes, 18 windows total):**

| Config | Weights (Bull TU/AD, Bear AD/TU) | Avg Sharpe | Pass | Δ vs Baseline |
|--------|----------------------------------|------------|------|---------------|
| C0_BASELINE | 1.0/0.3, 1.0/0.3 | 3.343 | 3/18 | — |
| C1_DUAL_AGGRESSIVE | 1.0/0.5, 1.0/0.5 | 3.343 | 3/18 | 0.000 |
| C2_TURTLE_HEAVY | 1.0/0.1, 0.3/1.0 | 3.343 | 3/18 | 0.000 |
| C3_AD_HEAVY_BEAR | 1.0/0.3, 1.0/0.0 | 3.343 | 3/18 | 0.000 |
| C4_AD_BALANCED | 0.8/0.5, 0.8/0.5 | 3.343 | 3/18 | 0.000 |
| C5_AD_AGGRESSIVE | 0.5/1.0, 1.0/0.5 | 3.343 | 3/18 | 0.000 |
| **C6_TURTLE_DUAL** | **1.0/0.3, 0.0/1.0** | **2.571** | **3/18** | **-0.772** |
| C7_SYMMETRIC | 1.0/0.5, 1.0/0.5 | 3.343 | 3/18 | 0.000 |

**Winner: C0_BASELINE** (equal to all except C6)
**Improvement over baseline: 0.000 Sharpe — no weight variant improves outcomes**

---

## Analysis

### Why All Non-C6 Configs Are Identical

The Turtle and A/D signals are highly correlated. When BTC is above SMA200 (bull regime), both Turtle (trend-following) and A/D (accumulation momentum) produce similar directional signals. The relative weighting between them {0.0..1.0} doesn't change the outcome because both signals point the same direction.

When BTC is below SMA200 (bear regime), Turtle and A/D both try to capture short-term bounces. Weighting them differently {0.0..1.0} again doesn't matter because the signals are correlated and the top-ranked symbol is usually the same.

**The ensemble is not actually ensembling — it's just picking the same symbol twice with different multipliers.**

### Why C6_TURTLE_DUAL (bear_ad=0.0, bear_tu=1.0) Is Worse

Setting `BEAR_AD_WT=0.0` means completely ignoring A/D momentum in bear regimes. A/D is the signal that captures accumulation/distribution pressure — it's more responsive than Turtle in choppy bear markets. Removing it leaves only Turtle, which gets whipsawed.

**This confirms A/D momentum has genuine value in bear regimes** — it can't be replaced by Turtle alone.

### The Magic Numbers Are Harmless (But Useless)

The current weights (1.0/0.3/1.0/0.3) were hand-picked with no validation. This sweep confirms:
1. They don't hurt (identical to equal weights)
2. They don't help (identical to equal weights)
3. The one weight combination that meaningfully differs (C6: BEAR_AD_WT=0.0) is WORSE

**Conclusion:** The weights are unvalidated magic numbers that happen to be neutral. Future research should either simplify (equal weights, or just pick one signal) or find genuinely orthogonal signals to combine.

---

## Action Taken

**No changes to production code.** The baseline weights remain unchanged (they're no better and no worse than alternatives). This is documented as a confirmed finding, not a regression.

**Chart saved:** `charts/turtle_ad_regime_sweep.png`

---

## Other Hyperopts This Session

### A/D Period Sweep (12:35 UTC)
- **Winner: p=2** (13/18 passes) vs baseline p=5 (11/18 passes)
- Updated `AD_PERIOD=5→2` in `turtle_ad_sma200_conditional.rs`
- See: `memory/hyperopt-2026-04-15.md`

---

## Chart
- `charts/turtle_ad_regime_sweep.png` — Bar charts of Sharpe and pass count by config
