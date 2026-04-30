# Hyperparameter Audit — 2026-04-29

**Session:** Kira Hyperparameter Optimization Session
**Mission:** Strip assumptions, find better defaults — audit every hardcoded constant
**Params tested:** ATR_ENTRY_MULT × current production config
**Chart:** `charts/atr_entry_mult_current_comparison.png`

---

## Mission: Strip Assumptions

### What Was Already Known

All primary Turtle+Chandelier parameters were already extensively validated:

| Parameter | Value | Sweep Method |
|-----------|-------|-------------|
| EP | 21 | 96-value fine sweep (5-100 step 1) |
| CHAND_PERIOD | 7 | 28-value extensive sweep (5-60 step 2) |
| CHAND_MULT | 2.30 | 71-value dense sweep (1.50-5.00 step 0.05) |
| TURTLE_ATR_PERIOD | 24 | 18-value fine sweep (18-35 step 1) |
| TURTLE_ATR_MULT | 2.00 | 9-value coarse sweep (1.0-5.0 step 0.5) |
| HOLD_MAX | 12 | 19-value sweep (5-180) |
| POSITION_CAP | 3 | 10-value sweep (1-10) |
| VOL_LOOKBACK | 8 | 100-value dense sweep (1-100 step 1) |
| MIN_TRADES | 3 | 20-value extensive sweep |
| ATR_ENTRY_MULT | 0.00 | 41-value coarse sweep (0.00-2.00 step 0.05) — **ON STALE PARAMS** |

### The Critical Gap Found

**ATR_ENTRY_MULT was never swept with current production params (CHAND_P=7, CHAND_M=2.30, EP=21).**

Prior sweeps:
- `atr_entry_mult_prod_sweep.rs`: 11 values × stale CHAND_P=11/M=2.25/EP=24
- `atr_entry_mult_full_sweep.rs`: 41 values × stale CHAND_P=11/M=2.25/EP=24 → EM=0.00 winner
- `atr_entry_mult_fine_sweep.rs`: 201 values × stale EP=24, Base5 only (6 windows)

**All prior sweeps used CHAND_P=11, not current CHAND_P=7.**

---

## This Sweep: ATR_ENTRY_MULT Fine-Grained with Current Params

**Harness:** `examples/atr_entry_mult_current_sweep.rs`
**Params:** CHAND(7,2.30)/EP=21/HM=12/CAP=3/ATR(24,2.0)/VL=8
**Scope:** EM ∈ [0.00..2.00] step 0.01 → **201 values** × 9 universes × 6 windows = **~10,854 window-runs**
**Runtime:** 10.4s

---

## Key Finding: Parameter Interaction

With stale CHAND_P=11, EM=0.00 was optimal.
With current CHAND_P=7, the ATR_ENTRY_MULT landscape **changes dramatically**.

### Full Sweep Results (selected checkpoints)

| EM | PASS | PASS% | SHARPE | RETURN% | DD% | TRADES |
|----|------|-------|--------|---------|-----|--------|
| **0.00** | 40/54 | 74.07% | 3.15 | +105.3% | 35.4% | 721 |
| 0.10 | 37/54 | 68.52% | 4.30 | +107.6% | 33.2% | — |
| 0.20 | 35/54 | 64.81% | 3.47 | +84.9% | 34.6% | — |
| 0.50 | 33/54 | 61.11% | 2.86 | +55.6% | 36.5% | — |
| 0.80 | 39/54 | 72.22% | 3.73 | +57.2% | 30.9% | — |
| 0.85 | 40/54 | 74.07% | 4.79 | +70.0% | 28.9% | — |
| 0.86 | 40/54 | 74.07% | 4.88 | +72.1% | 28.9% | — |
| 0.88 | 41/54 | 75.93% | 5.21 | +75.5% | 28.6% | — |
| 0.89 | 41/54 | 75.93% | 5.07 | +73.2% | 28.5% | — |
| 0.90 | 41/54 | 75.93% | 5.10 | +73.4% | 28.4% | — |
| 0.91 | 41/54 | 75.93% | 5.27 | +74.2% | 28.3% | — |
| 0.92 | 41/54 | 75.93% | 5.18 | +73.0% | 28.6% | — |
| 0.93 | 41/54 | 75.93% | 4.93 | +71.2% | 29.1% | — |
| **0.94** | **42/54** | **77.78%** | **5.34** | **+73.0%** | **28.3%** | **486** |
| 0.95 | 41/54 | 75.93% | 5.27 | +77.6% | 29.0% | — |
| 1.00 | 39/54 | 72.22% | 5.89 | +76.7% | 29.0% | — |
| 1.07 | 41/54 | 75.93% | **7.86** | +91.9% | 24.8% | 428 |
| 1.08 | 41/54 | 75.93% | 7.84 | +91.7% | 24.8% | — |
| 1.09 | 41/54 | 75.93% | 7.42 | +89.0% | 25.2% | — |
| 1.10 | 41/54 | 75.93% | 7.42 | +89.0% | 25.2% | — |
| 1.50 | 25/54 | 46.30% | 9.18* | +57.1% | 24.0% | — |

*Sharpe inflated by near-zero returns in failed windows (numerical artifact).

---

## Mechanism解释

With CHAND_P=7 (tight — fires at ~bar 7-12), the Chandelier stop is aggressive. Combined with EM=0.00:
- Many weak breakouts enter at EM=0, then get stopped by Chandelier P=7 within 7-12 bars
- These are "whipsaw losses" — enter, immediately stopped, repeat
- Trade frequency is high but quality is low → lower pass rate

With EM=0.86-0.94:
- ATR filter blocks breakouts where close < max_close + 0.86-0.94×ATR
- Only stronger momentum breakouts enter → fewer but higher-quality trades
- Chandelier P=7 fires on real reversals, not on weak momentum
- Pass rate improves by ~3-4 percentage points

**The ATR_ENTRY_MULT and CHAND_PERIOD interact**: the tighter the Chandelier stop, the more the entry filter matters.

---

## Anti-Overfitting Discipline

⚠️ **Anti-overfitting flag:** This sweep result was found ON the same walk-forward validation data used to select all other production params. Following anti-overfitting rules:

1. **EM=0.00** was validated on stale params (CHAND_P=11) — not comparable to current sweep
2. **EM=0.94** was found on the current sweep data — in-sample optimization risk
3. **Verdict: NO production default change without held-out validation**

The plateau (EM=0.86-0.95 → 75-78% pass) is a robust feature, not a single-point spike. But even selecting from a plateau involves implicit optimization.

**Recommended held-out test:** Pre-2021 data (held-out from all prior sweeps), current CHAND_P=7. If EM=0.94 holds ≥70% pass on held-out, promote to production.

---

## All-Parameter Audit Summary

| Parameter | Status | Notes |
|-----------|--------|-------|
| EP=21 | ✅ CONFIRMED | Dense sweep, robust winner |
| CHAND_P=7 | ✅ CONFIRMED | Extensive sweep |
| CHAND_M=2.30 | ✅ CONFIRMED | 71-value dense sweep |
| TURTLE_ATR_P=24 | ✅ CONFIRMED | Fine sweep 18-35 |
| TURTLE_ATR_M=2.00 | ✅ CONFIRMED | 9-value sweep, NULL finding |
| ATR_ENTRY_MULT | ⚠️ PARAMETER INTERACTION | EM=0.94 candidate vs EM=0.00; needs held-out |
| HOLD_MAX=12 | ✅ CONFIRMED | Dense sweep |
| POSITION_CAP=3 | ✅ CONFIRMED | 10-value sweep |
| VOL_LOOKBACK=8 | ✅ CONFIRMED | 100-value dense sweep |
| MIN_TRADES=3 | ✅ CONFIRMED | 20-value extensive sweep |
| ATR_EMA=1 | ✅ CONFIRMED | 200-value NULL finding |
| TAKER_FEE=0.001 | ⚠️ UNTESTED | Live = 0.0004; harness = 0.001. Fee gap unvalidated |
| DDBudget params | 🔴 UNTESTED IN CURRENT HARNESS | ATR_P=100, EMA params, BB params not swept in current framework |

---

## TakER FEE discrepancy (structural)

- Walk-forward harness: `TAKER_FEE = 0.001` (0.1% per side)
- Live bot config: `fee_pct = 0.0004` (0.04% per side)
- Maker fill rate (estimated ~70%): effective fee ≈ 0.012% + 0.035% slippage = ~0.047% per side

**This is a structural discrepancy**: the walk-forward harness overstates fees by ~2-3x vs live maker execution. This means walk-forward Sharpe is conservative by ~22-33% (from 2026-04-13 execution realism analysis). Not a parameter to tune, but a known bias.

---

## Recommendation

**Current production ATR_ENTRY_MULT=0.00 is based on stale CHAND_P=11 validation.**
**The sweep with current CHAND_P=7 identifies EM=0.94-1.07 as the robust-optimal range.**

**Immediate action:**
1. Build held-out validation harness (pre-2021 data, CHAND_P=7, sweep EM ∈ {0.00, 0.50, 0.86, 0.94, 1.00, 1.07})
2. If EM=0.94 holds ≥70% on held-out → promote to production default
3. If EM=0.00 holds on held-out → retain current default
4. Do NOT change default until held-out confirms

**Files:**
- Harness: `examples/atr_entry_mult_current_sweep.rs`
- Data: `snapshots/atr_entry_mult_current_sweep.csv`
- Equity: `snapshots/atr_entry_mult_current_equity.csv`
- Summary: `snapshots/atr_entry_mult_current_summary.csv`
- Chart: `charts/atr_entry_mult_current_comparison.png`
