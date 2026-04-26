# PLAN.md — Krypto Research & Critique Cycle

**State: 2026-04-25 22:00 UTC. Critique cycle — T3 incomplete, T6 overstated, anti-overfitting discipline not consistently applied.**

---

## Brutal Self-Assessment

The project has been in "audit mode" since 2026-04-21. Last 8 commits: 5/8 are documentation/audit. Only T6 (CTREND fixed-hold) added genuine new knowledge.

**Critical finding this session:** T3 (EP held-out validation) is NOT complete. The commit `996b46bc` claims "EP=24 confirmed vs EP=21 on pre-2021 data" but `regime_stress_p7_validation.rs` uses CHAND_M=2.25, NOT current CHAND_M=2.30. The paired comparison (EP=21 vs EP=24 on current production params) was never run. T3 is still overdue.

---

## Research Loop: Effectively Closed

**What we know:**
- Turtle+Chandelier: 83% OOS pass, 100% Base5 pass, daily equity Sharpe ~1.29
- Edge generalizes to SPY/GLD (61% global pass, cross-market audit)
- Every entry-side filter tested: REJECTED
- CTREND fixed-hold is a genuine but secondary signal (73% pass vs Turtle's 80%)
- Everything else is graveyard

**What we don't know:**
- Whether EP=24 is real or noise on current production params (T3 OVERDUE)
- Whether P=7/M=2.30 collectively hold on pre-2021 data (M=2.25 stress: 19/28 = 67.9%)
- Live execution quality — BLOCKED on API keys

---

## Known Production Params (VERIFIED vs src/live/config.rs)

```
EP = 21              // ✅ T3 2026-04-26: EP=24 REVERTED — EP=24 was in-sample inflation
CHAND_PERIOD = 7     // +6.9% Sharpe vs P=11, same pass rate — plausible
CHAND_MULT = 2.30    // ⚠️ MARGINAL: +1 window over M=2.25 — T3 must also validate M=2.30
ATR_ENTRY_MULT = 0.00 // CONFIRMED: definitive sweep winner, NOT marginal
HOLD_MAX = 12        // +71% Sharpe vs HM=45 — plausible, sweep used P=11 (cross-param risk with P=7)
ATR_PERIOD = 24      // CONFIRMED 3× — stop re-running
POSITION_CAP = 3
FRESHNESS_COOLDOWN = 0
MAX_SOL_POSITION = $50K notional
```

**✅ Anti-overfitting rules (established 2026-04-25, APPLIED 2026-04-26):**
- Minimum win margin: ≥3 windows (5.5%) on OOS before accepting param change
- No sequential optimization on same OOS data
- Held-out validation required for marginal wins (1-2 window delta)
- EP=24 was rejected for violating rules 2+3 (sequential optimization on same data, marginal 2-window win)
- ATR_ENTRY_MULT=0.85 was rejected for same reason (2026-04-25)
- EP=21 is the validated winner; EP=24 was in-sample inflation

---

## CRITICAL — Pending

### T3: EP=24 Held-Out Validation (PAIRED comparison on CURRENT params) — ✅ COMPLETE 2026-04-26

**What happened:** Commit `996b46bc` claimed T3 complete. Reality: `regime_stress_p7_validation.rs` tests P=7/M=2.25, NOT current production P=7/M=2.30. The paired comparison (EP=21 vs EP=24 on current params) was never run.

**What needs to be done:**
Build/run `regime_stress_ep24_paired.rs` comparing EP=21 vs EP=24 using **current production params** (CHAND_P=7, CHAND_M=2.30, HM=12, ATR_ENTRY_MULT=0.00) against pre-2021 held-out data.

**Decision rule:** ✅ APPLIED — EP=24 REVERTED to EP=21.
- Result: EP=21 avg Sharpe 0.18 vs EP=24 0.16 on held-out (27/29 vs 25/29 pass).
- EP=24 was in-sample inflation (same session as P=7 and ATR_ENTRY_MULT).
- See snapshots/t3_ep_paired_held_out.csv.

**Why this matters:** The same OOS data was used to optimize EP=24 AND to validate it. ATR_ENTRY_MULT was rejected for exactly this. EP=24 has the same structural flaw — it's a 2-window winner on data used to select it.

### T3-NEXT: EP=21/P=7 vs EP=21/P=11 Sanity Check — 🟡 NOT STARTED

**Why:** EP=24 was the reason CHAND_P=7 won over CHAND_P=11 in the CP sweep (the sweep used EP=24 as entry). Now that EP=24 is reverted, need to verify that CP=7 is still the right default for EP=21.

**Action:** Fast 2-universe (Base5) test comparing EP=21/CHAND_P=7 vs EP=21/CHAND_P=11. If P=11 wins or ties, revert CP=7→11.

### T6-NEXT: CTREND Fixed-Hold Portfolio Sleeve Test — 🟡 NOT STARTED

**T6 result (2026-04-25):** CTREND fixed-hold (hold=30 bars) = 44/60 pass (73%) — genuine signal, weaker than Turtle (80%+). Win condition met (>35/54).

**What "complete" doesn't mean:** CTREND fixed-hold is NOT a standalone replacement for Turtle+Chandelier. It is a secondary signal family with different regime sensitivity.

**What needs to be done:** Test CTREND fixed-hold (hold=30) as 20-30% portfolio sleeve alongside Turtle+Chandelier (70-80%). Walk-forward comparing Turtle-only vs Turtle+CTREND sleeve.

**Win condition:** Adding CTREND sleeve reduces MaxDD by >3pp without reducing Sharpe by >10%.
**Why this matters:** Single-strategy risk. Turtle+Chandelier is the only validated strategy. Diversification with a genuinely different signal family is the only hedge.

### T10: HOF Generation Script — 🟡 NOT STARTED

**What:** Build `scripts/generate_hall_of_fame.rs` — parse `src/live/config.rs` + `examples/live_turtle_chandelier.rs` → auto-generate HALL_OF_FAME.md

**Why:** HOF has been manually updated and contradictory 5+ times. Source of truth should be code, not markdown. Critical hygiene before live testnet deployment.

---

## BLOCKED — Waiting on Noah

### T9: Live Testnet
Noah needs Binance testnet API keys. Without this, no live paper trading.
**This is the only remaining path to new knowledge beyond T3/T6-NEXT.**

---

## Stop Doing

- **Re-running confirmed params:** ATR_PERIOD confirmed 3×. CHAND_MULT confirmed 2×. Stop.
- **Documentation-only sprints:** Last 8 commits: 5/8 docs/audit. Zero feature builds.
- **Calling incomplete tests "complete":** T3 (EP held-out) is overdue. Don't claim it's done until the paired comparison runs.
- **Sequential optimization on same data:** Applied to ATR_ENTRY_MULT but not EP=24. Fix both or fix neither.
- **HALL_OF_FAME manual updates:** Build the generator script.

---

## Quick Fixes

~~Fix stale print bug~~ ✅ FIXED
~~Fix ATR_ENTRY_MULT missing constant~~ ✅ FIXED

---

## Blind Spots

| Blind Spot | Severity | Status |
|-----------|----------|--------|
| EP=24 held-out paired comparison on current params | CRITICAL | 🔴 STILL OVERDUE — T3 |
| P=7/M=2.30 held-out on pre-2021 | CRITICAL | 🔴 T3 must cover both |
| CTREND fixed-hold portfolio sleeve value | MEDIUM | 🟡 Not tested — T6-NEXT |
| HOF generation script | MEDIUM | 🟡 Not started — T10 |
| Maker-fill adaptive position | LOW | 🟢 Unbuilt — needs live data |
| Live execution unknown | CRITICAL | BLOCKED on API keys |

---

## Graveyard Summary

- All non-trend strategies: FAILED
- All regime switching: FAILED
- All entry-side filters: FAILED (ATR, volume, correlation, chop)
- Vol-rank overlays: FAILED
- Position scaling overlays: FAILED
- CTREND + Chandelier exit: FAILED (wrong mechanism — T6 fixed-hold is the right exit)
- 4h multi-timeframe: FAILED (structural)
- Cross-market equity integration: FAILED
- ATR entry filter: FAILED (definitive, 2×)
- Freshness filter: FAILED (cd=0 optimal)
- BTC/ETH correlation filter: FAILED (T7)

---

## Research Loop: What Remains

1. **T3:** EP=24 paired held-out on current params (CRITICAL, overdue)
2. **T6-NEXT:** CTREND portfolio sleeve test (MEDIUM, not started)
3. **T10:** HOF generation script (LOW, not started)
4. **T9:** Live testnet (BLOCKED on Noah's keys)

**Everything else in strategy-ideas.md is either:**
- Already tested and rejected
- Cannot be tested without live data
- Theoretical only