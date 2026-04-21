# PLAN.md — Krypto Strategy & Execution Plan

**State: 2026-04-21 04:06 UTC.**
**HOLD_MAX 45→12 (2026-04-21 hyperopt). EP=24 in production (live bot + all harnesses).**
**BLOCKED: live testnet pending Noah's API keys.**

---

## 🚨 CRITICAL: 2026 YTD — Our Only Genuine OOS Data

All 54 walk-forward windows end before 2026. We have ZERO OOS validation in the current regime.
Our only real-world test: **2026 YTD = -22.7%, Sharpe -5.31**. Catastrophic.

**This is the most important validation step we have never done.**
Extending a walk-forward window to 2026-01-01 to 2026-04-21 must be the FIRST task.
If the strategy fails 2026, our entire param set (EP=24, HM=12, P=11/M=2.25) may be tuned to historical bull regimes.

---

## Top 3 Execution Tasks

### T1 (CRITICAL): Walk-Forward on 2026 Data — The Only Real Test

**Why:** 54/54 walk-forward windows end before 2026. EP=24 and HM=12 have never been tested in the current market.
**Hypothesis to test:** Does EP=24 / CHAND(11, 2.25) / HM=12 work in 2026 regime?
**What to run:** Adapt `turtle_chandelier_walkforward.rs` — extend W05 (most recent window) to include 2026-01-01 to 2026-04-21.
**Report:** Sharpe, return, maxDD, trade count, pass/fail (sh>0).
**Accept criteria:** If HM=12 produces Sharpe < -2.0 on 2026 data → HM=12 may be too aggressive for bear/chop regimes. Consider HM=35 (where Chandelier fires first regardless).

### T2 (HIGH): Fix progress_equity_curves.rs — Stale Params Since 2026-04-20

**Status:** live_turtle_chandelier.rs updated to CHAND_P=11/CHAND_M=2.25 on 2026-04-20.
**But:** `examples/progress_equity_curves.rs` STILL has CHAND_P=15/CHAND_M=1.50.
**The equity chart shows the wrong parameters.** This is the same error class as BollingerReversion DOGE 5404x.
**Action:** Update progress_equity_curves.rs to CHAND_P=11, CHAND_M=2.25, EP=24, HM=12 → run → verify → regenerate PNG only after verified.
**Hygiene:** Verify with grep before running. Do NOT send stale charts to Discord.

### T3 (MEDIUM): CTREND + CTREND-Native Exit Walk-Forward

**Prior result:** CTREND + Chandelier exit → 30/54 pass (44% fail) — exit was wrong.
**Monte Carlo confirmed:** CTREND signal is GENUINE (0/500 shuffled beat real).
**Why it failed:** CTREND fires SLOW (multi-horizon smoothing). Chandelier(P=11,2.25) fires fast — kills positions before they develop.
**Hypothesis:** CTREND needs a SLOWER exit: ATR(40-60, 2.5-3.0) OR RSI-regime exit.
**Sweep:** ATR_period {40,50,60} × ATR_mult {2.5,3.0} × exit_type {ATR, RSI, fixed_hold}
**Baseline to beat:** Turtle+Chandelier = 45/54 (83.3%). Any CTREND variant beating 40/54 is viable.
**Why it matters:** If CTREND works with its own exit, we have two uncorrelated signal families. Monte Carlo says the signal is real — the exit wasn't.

---

## Production Params (VERIFIED FROM SOURCE — 2026-04-21)

```
# examples/live_turtle_chandelier.rs (line 28-33):
EP = 24
CHAND_PERIOD = 11
CHAND_MULT = 2.25
ATR_PERIOD = 24
ATR_MULT = 0.0  (no entry filter — confirmed 2026-04-19)
HOLD_MAX = 12   (updated 2026-04-21: +71.4% Sharpe vs HM=45)
POSITION_CAP = 3
FRESHNESS_COOLDOWN = 0
MAX_SOL_POSITION = $50K notional
```

**HALL_OF_FAME.md:** Correct as of e36b7b21 (CHAND_PERIOD=11, CHAND_MULT=2.25).
**DO NOT change EP** — EP=24 is in production code since daac533a. The "revert EP to 21" item in prior PLAN was stale and never executed.

---

## 🚫 STOP DOING

- **ANY param sweep on frozen params** (EP, CHAND_P, CHAND_M, ATR_P, ATR_MULT) — all done to exhaustion
- **Regenerating equity charts** without first verifying params in the harness source
- **Calling walk-forward "OOS"** when all windows predate 2026
- **Claiming Sharpe 5.0+** on any external-facing report (5.46 is per-window average, not comparable to standard Sharpe)
- **Hygiene loop on HALL_OF_FAME** — it's correct now. Stop fixing it every session.

---

## Live Testnet — BLOCKED on API Keys (7+ days)

Noah needs to provide testnet API keys. Only T1 (historical walk-forward) works without them.
All live execution infrastructure is built (FillLog, slippage tracker, dry-run mode).

---

## Blind Spots

| Blind Spot | Severity | Status |
|-----------|----------|---------|
| **No OOS in 2026 regime** | CRITICAL | T1 — never done |
| **progress_equity_curves.rs stale** | CRITICAL | T2 — P=15/M=1.50, needs update |
| **HM=12 never tested in 2026** | HIGH | T1 — may be too aggressive |
| **Walk-forward Sharpe inflation** | HIGH | 5.46 is per-window avg, not std annualised |
| **No live testnet data** | CRITICAL | BLOCKED |
| **CTREND-native exit untested** | MEDIUM | T3 — genuinely new |
| **4h timeframe untested** | LOW | Not in top-3 priority |

---

## Research Loop Status

**Closed.** No genuinely new strategy ideas remain.
All ATR entry filters, vol regime filters, position scaling, regime switching, non-trend strategies — all graveyard'd with clean kill reasons.

**Genuinely untested:**
1. CTREND + CTREND-native exit (T3) — signal confirmed genuine by Monte Carlo
2. 4h multi-timeframe — different from failed 4h MR (strategy class issue)
3. **2026 OOS walk-forward (T1)** — most important, never done

---

## Archive: Prior Hygiene Items (RESOLVED)

These items from prior PLAN versions are now resolved:
- HALL_OF_FAME.md wrong (CHAND_PERIOD=15/M=1.50) → **FIXED (e36b7b21)**
- EP=24 vs EP=21 confusion → **RESOLVED: EP=24 is production**
- progress_equity_curves.rs was stale → **T2 (still open)**
- ATR entry filter → **CLOSED (0.00 confirmed twice)**

**Do not re-open these items unless new evidence requires it.**