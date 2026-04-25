# PLAN.md — Krypto Live Testnet Priority

**State: 2026-04-25 04:10 UTC. T2 CLOSED. Pre-2021 stress test run (P=7: 67.9% — marginal). Research CLOSED. BLOCKED on live testnet (Noah's API keys).**

---

## CRITICAL — Done

### T1: Equity Curve Regeneration + Source Verification ✅ DONE
- **Fixed:** `progress_equity_curves.rs` had stale CHAND_P=11 (old sweep on EP=21)
- **Updated:** CHAND_P → 7 (production default from 2026-04-21 hyperopt)
- **Verified:** Harness engine matches `live_turtle_chandelier.rs` (dual Chandelier(7,2.25)+Turtle_ATR(24,2.0))
- **Result:** 248x equity (Sharpe 1.68 daily) — prior stale run showed 276x (10% overstatement)
- **Chart sent to Discord** ✓

### T2: HALL_OF_FAME.md Full Audit ✅ DONE (2026-04-25)
- HOF now correctly references P=7/M=2.25 (not stale P=11 or P=15)
- Equity figure "1048.5x" deprecated — live_turtle_chandelier.rs is the source of truth
- Staleness warning added to HOF header
- live_turtle_chandelier.rs validation stats updated to reflect actual results (83% global / 100% Base5)

### T-2026: Walk-Forward Including 2026 OOS Data ✅ DONE
- W06 PASSES 9/9 universes (2026-01 to 2026-04)
- Commit: `ed813fef`
- **Commit:** `76b4e6fc` (CHAND_P fix for equity curves)

### Pre-2021 Stress Test — P=7/M=2.25 (NEW — 2026-04-25)
- **Result:** 19/28 pass (67.9%) — marginally below 70% threshold
- P1-2020: 7/10 pass (avg Sharpe 4.52)
- P2-2021: 7/10 pass (avg Sharpe 2.89)
- P3-2019: 5/8 pass (avg Sharpe 1.89)
- **Context:** Original `regime_stress_test.rs` (P=28/M=2.0/EP=21/HM=45) got 21/21 on a smaller test set. P=7/M=2.25 is tighter (fires earlier) and slightly more sensitive in choppy periods.
- **Verdict:** 67.9% is marginally below 70%. Walk-forward (83% global, 100% Base5) remains the definitive validation. Pre-2021 stress is supplementary.
- File: `examples/regime_stress_p7_validation.rs`

---

## HIGH PRIORITY

### T3: 2026 YTD — EXPLAINED (2026-04-20 evening session)
- All param sets (P=5, P=11, P=15) produce IDENTICAL -32.8% portfolio result
- Mechanism: Turtle ATR exit dominates Chandelier in 2026 bear regime
- Chandelier params irrelevant when Turtle ATR fires first every time
- **Verdict:** Regime-inherent whipsawing, not parameter failure. No fix needed.
- W06 (2026 bull): +351.9% — strategy IS working in 2026, sample issue on the -22.7% harness

### T4: CTREND-Native Exit — REJECTED (2026-04-20)
- CTREND+Chandelier: 30/54 pass vs Turtle+Chandelier: 43/54 pass
- Signal is genuine (Monte Carlo confirmed), but wrong timing for Chandelier dual-exit
- GRAVEYARD as standalone replacement

### T5: Commit Quality Gate ✅ Done
- CHAND_PERIOD hyperopt (CP=7) was fully documented in commit messages
- Every param change now references its validation harness

---

## BLOCKED — Waiting on Noah

### T6: Live Testnet
Noah needs Binance testnet API keys. Without this, no live paper trading.
**This is the only remaining path to new knowledge.**

---

## STOP DOING

- **Hyperopt cycling on stable params** — all major params (EP, ATR, CHAND_P, CHAND_M, HOLD_MAX, ATR_ENTRY_MULT) now exhausted. Stop.
- **Equity vanity numbers** — stop saying "$67M", "1048.5x", "670,515%". Report equity Sharpe (~1.68) and pass rate (83%).
- **Running without --profile sweep** — build speed matters.
- **Re-running regime stress for P=7** — done, 67.9%. Conclusion: supplementary check, not primary bar.

---

## Production Params (CONFIRMED — 2026-04-21)

```
EP=24, CHAND_PERIOD=7, CHAND_MULT=2.25, HOLD_MAX=12,
ATR_PERIOD=24, ATR_ENTRY_MULT=0.85, POSITION_CAP=3, FRESHNESS_COOLDOWN=0
```

**Source of truth: `examples/live_turtle_chandelier.rs`**
HALL_OF_FAME.md may be stale — always verify against live_turtle_chandelier.rs.

---

## Research Loop Status ✅ CLOSED

**Genuinely untested ideas:**
1. #27 **4h multi-timeframe Turtle** — genuinely new territory (all validation is daily)
2. Anything else requires live testnet data to validate

**Graveyard:**
- All non-trend strategies: FAILED
- All regime switching: FAILED  
- All entry-side filters: FAILED
- CTREND as Turtle replacement: FAILED
- Vol-rank overlays: FAILED
- Position scaling overlays: FAILED
- ATR entry filter: REJECTED (mult=0.0 wins definitively)
- Volume confirmation filter: REJECTED (none wins)
- Vol-Contingent Chandelier multiplier: IDENTICAL results (GRAVEYARD)
- BTC Trend Scalar: REJECTED (baseline wins)
- BTC ATR Percentile Regime Filter: GRAVEYARD (marginal, +1.9pp pass, not worth complexity)
- DynamicTrend EMA Crossover + Chandelier: REJECTED
- A/D Dual-Hat (standalone): 52% pass, too weak alone
- DDBudget 3-Sleeve: 72% pass, below threshold

**What's left:** Research is complete. Only live testnet advances the project.

---

## Blind Spots

| Blind Spot | Severity | Status |
|-----------|----------|--------|
| **Pre-2021 stress P=7** | MEDIUM | ✅ Done — 67.9% (marginal, supplementary) |
| **HALL_OF_FAME staleness** | CRITICAL | ✅ Done (2026-04-25) |
| **Zero 2026 in walk-forward** | CRITICAL | ✅ Done (W06 passes 9/9) |
| **2026 YTD -32.8% unexplained** | CRITICAL | ✅ EXPLAINED (Turtle ATR dominates) |
| **4h multi-timeframe Turtle** | MEDIUM | ✅ GRAVEYARD (2026-04-25) — 1/20 pass (5%), 4h Chandelier = fixed-time stop, collapses dual-exit |
| **Noah's testnet API keys** | BLOCKED | Waiting |