# Hyperparameter Optimization — CHAND_PERIOD Fine Sweep
**Session:** 2026-04-20 18:20 UTC
**Agent:** Kira (krypto cron)
**Mission:** Fine-grid validation of CHAND_PERIOD — step 1 in critical region [5-30]

---

## Context

CHAND_PERIOD was swept in the AM session (2026-04-20) using step 2 across [5..60]:
- **Prior result:** CP=11 wins (+1.9% vs CP=15, 79.6% global pass rate)
- **Caveat:** Step 2 may have missed the true optimum by 1 bar
- **Risk:** Coarse sweep used CM=1.50; production is CM=2.25 — interaction unknown

This fine sweep uses **step 1** in the critical region [5-30] with **current production params** (CM=2.25, EP=21, ATR=24, ATR_M=2.0, HM=45, CAP=3).

---

## Method

- **Parameter:** CHAND_PERIOD ∈ {5, 6, 7, ..., 30} — **26 values** (step 1)
- **Universe:** Base5 (BTC, ETH, SOL, XRP, DOGE, ADA — 6 windows)
- **Fixed params:** CM=2.25, EP=21, ATR=24, ATR_M=2.0, HM=45, CAP=3, VOL=2
- **Walk-forward:** 252-bar train / 252-bar test
- **Equity curves:** Exported for top 3 candidates (CP=11, 12, 13)
- **9-universe validation:** Winner (CP=13) tested across all 9 universes

---

## Results

### Base5 Fine Sweep (sorted by Sharpe)

| CP | AvgRet% | AvgSharpe | AvgDD% | AvgTrades | PassRate% |
|----|---------|-----------|--------|-----------|-----------|
| **13** | 155.2 | **7.012** | -19.1 | 10.0 | 100.0% |
| 12 | 155.1 | 7.006 | -19.1 | 10.0 | 100.0% |
| **11** | 154.7 | **6.993** | -19.2 | 10.0 | 100.0% |
| 7 | 142.8 | 6.965 | -21.0 | 10.3 | 83.3% |
| 15 | 160.5 | 6.953 | -19.2 | 10.0 | 100.0% |
| 17-30 | 157-160 | 6.844 | -19.3 | 10.0 | 100.0% |
| 10 | 147.4 | 6.831 | -19.2 | 10.2 | 100.0% |
| 14 | 152.6 | 6.656 | -19.2 | 10.2 | 100.0% |
| **5-9** | 137-140 | **5.9-6.6** | -22-23 | 10.7 | **66.7%** |

### 9-Universe Validation (CP=13)

| Universe | AvgRet% | AvgSharpe | PassRate% |
|----------|---------|-----------|-----------|
| Base5 | 155.2 | 7.012 | 100.0% |
| NoDOGE | 154.5 | 6.024 | 100.0% |
| LargeCap | 154.5 | 6.024 | 100.0% |
| OldGuard | 72.8 | 4.616 | 85.7% |
| Legacy4 | 63.4 | 4.549 | 87.5% |
| Legacy5 | 63.4 | 4.549 | 87.5% |
| Legacy3 | 53.5 | 3.485 | 85.7% |
| OldGuard4 | 53.5 | 3.485 | 85.7% |
| LowVol | 73.3 | 2.695 | 71.4% |
| **Global** | | | **88.7% (55/62)** |

---

## Key Findings

### 1. CP=13 Wins, but CP=11 is Statistically Equivalent
- CP=13: Sharpe 7.012; CP=11: Sharpe 6.993 — **Δ = +0.3%, within noise**
- The fine sweep confirms the coarse sweep was essentially correct
- CP=11 is the true optimum zone, not a coincidence of step-2 granularity

### 2. Phase Transition at CP ≤ 9
- CP=5 through CP=9: **pass rate collapses to 66.7%** (1/3 of windows fail)
- Sharpe drops to 5.9-6.6 — Chandelier fires too aggressively
- **CP ≥ 10 required for stable operation** — this is a hard constraint

### 3. Structural Plateau from CP=17+
- CP=17 through CP=30: all produce **identical results** (Sharpe 6.844)
- At CP≥17, Turtle ATR dominates the Chandelier — CP becomes irrelevant
- The Chandelier exit is only active in the CP=10-16 range

### 4. CP=11-13 is the Optimal Band
- All three: 100% pass rate, Sharpe 6.993-7.012
- CP=13 has marginally best Sharpe (+0.3%)
- CP=11 has the most pre-2021 validation evidence

---

## Decision: CP=11 Stays as Production Default

**Rationale:**
1. CP=13's +0.3% Sharpe advantage over CP=11 is within noise for 6 windows
2. CP=11 has pre-2021 paired stress test validation (21/21 pass, ΔSH=-0.02 vs P=28/M=2.0)
3. CP=11 was confirmed by coarse sweep across 9×6=54 windows
4. The practical difference in equity curves is indistinguishable

**CP=13 is documented as a marginal alternative** for users who want the slight Sharpe boost at the cost of less historical validation.

---

## Chart

**File:** `charts/chand_period_fine_comparison.png` (3-panel)
- Panel A: Sharpe vs CP (step 1, 26 values) — winner highlighted
- Panel B: Pass Rate vs CP — sub-optimal zone marked
- Panel C: Log-scale equity curves — CP=11, 12, 13 overlaid

**Also generated:** `charts/chand_period_fine_chart.py` (source script)

---

## Files Created

| File | Purpose |
|------|---------|
| `examples/chand_period_fine_sweep.rs` | Fine-grid sweep harness |
| `snapshots/chand_period_fine_sweep.csv` | Summary metrics (26 CP values) |
| `snapshots/chand_period_fine_9u_validation.csv` | 9-universe validation for CP=13 |
| `snapshots/chand_period_cp{11,12,13}_equity.csv` | Equity curves for top 3 |
| `charts/chand_period_fine_chart.py` | Charting script |
| `charts/chand_period_fine_comparison.png` | 3-panel comparison chart |

---

## Conclusion

**All production parameters remain at validated defaults.** CHAND_PERIOD fine sweep (step 1) confirms:
- CP=13 marginal winner (Sharpe 7.012, within noise of CP=11 at 6.993)
- Phase transition at CP≤9: hard constraint — CP must be ≥10
- Plateau at CP≥17: Turtle ATR dominates, Chandelier becomes irrelevant
- CP=11 confirmed as robust production default (pre-2021 validated)

The fine sweep adds confidence to the coarse result and documents the complete shape of the Sharpe curve around the optimum.

*Generated: 2026-04-20 18:25 UTC | Commit: ebc494c7, a81e62f8*
