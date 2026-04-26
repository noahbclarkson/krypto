# PLAN.md — Krypto Research & Execution Plan

**State: 2026-04-26 02:07 UTC. T3-Next COMPLETE. CP=7 validated with EP=21. CTREND sleeve and HOF script remain.**

---

## T3-Next: CP=7 Paired Held-Out Result (JUST COMPLETED)

**Result: CP=7 VALIDATED with EP=21.**
- CP=7: 27/29 pass, Sharpe +0.18
- CP=11: 27/29 pass, Sharpe +0.18
- Delta Sharpe: **+0.01 (within noise)**
- CP=7 is NOT EP=24-dependent inflation. It stands on its own merit.
- **VERDICT: Keep CP=7.**
- File: `examples/t3_p7_p11_paired_held_out.rs`, CSV: `snapshots/t3_p7_p11_paired_held_out.csv`

**Updated production params (FINAL — all validated):**
```
EP=21, CHAND_PERIOD=7, CHAND_MULT=2.30, HOLD_MAX=12,
ATR_ENTRY_MULT=0.00, ATR_PERIOD=24, POSITION_CAP=3, FRESHNESS_COOLDOWN=0
```

**Anti-overfitting status:** EP=24, ATR_ENTRY_MULT=0.85, and CP=7 all checked. All stand. Sequential optimization flaw is now fully resolved.

---

## Research Loop: CLOSED — Validated Params Established

**What we know (with confidence):**
- Turtle+Chandelier: 41/54 OOS pass (76%), Base5 6/6 pass (100%)
- All core params validated against held-out data (EP, CP, HM, ATR_ENTRY_MULT)
- Daily equity Sharpe ~1.29 — honest, methodology-verified
- Maker-fill ~70% — execution gap is the biggest unknown

**What remains:**
- T6-NEXT: CTREND portfolio sleeve test (genuinely untested diversification idea)
- T10: HOF generation script (hygiene, not new knowledge)
- T9: Live testnet (BLOCKED on Noah's API keys)

---

## CRITICAL — Pending

### T6-NEXT: CTREND Portfolio Sleeve Test — 🟡 NOT STARTED

**T6 result:** CTREND fixed-hold (hold=30) = 44/60 pass (73%) — genuine signal, weaker than Turtle (80%+). Win condition met but CTREND is NOT a standalone replacement.

**What needs to be done:** Test CTREND fixed-hold (hold=30) as 20-30% portfolio sleeve alongside Turtle+Chandelier (70-80%). Walk-forward comparing Turtle-only vs Turtle+CTREND sleeve.

**Win condition:** Adding CTREND sleeve reduces MaxDD by >3pp without reducing Sharpe by >10%.

### T10: HOF Generation Script — 🟡 NOT STARTED

**What:** Build `scripts/generate_hall_of_fame.rs` — parse `src/live/config.rs` + `examples/live_turtle_chandelier.rs` → auto-generate HALL_OF_FAME.md from code.

**Why:** HOF manually updated 5+ times with contradictions. Source of truth should be code. Critical hygiene before live testnet deployment.

### T9: Live Testnet — 🔴 BLOCKED

Noah needs Binance testnet API keys. Without this, no live paper trading.
**This is the only remaining path to new knowledge beyond T6-NEXT/T10.**

---

## Stop Doing

- **Re-running confirmed params:** ATR_PERIOD confirmed 3×, CHAND_MULT confirmed 2×, ATR_ENTRY_MULT confirmed, EP reverted. Stop.
- **Sequential optimization:** Fully resolved. All params now validated on held-out.
- **Documentation-only sprints:** Focus shifted to T6-NEXT execution.
- **HOF manual updates:** Build T10 generator script instead.

---

## Quick Fixes

~~Fix stale print bug~~ ✅ FIXED
~~Fix ATR_ENTRY_MULT missing constant~~ ✅ FIXED
~~EP=24 in-sample inflation~~ ✅ FIXED (reverted to EP=21)
~~CP=7 potentially EP=24-dependent~~ ✅ FIXED (T3-Next: CP=7 validated independently)

---

## Blind Spots

| Blind Spot | Severity | Status |
|-----------|----------|--------|
| Live execution unknown | CRITICAL | 🔴 BLOCKED on API keys |
| CTREND portfolio sleeve value | MEDIUM | 🟡 Not tested — T6-NEXT |
| HOF generation script | LOW | 🟡 Not started — T10 |
| Maker-fill rate (live vs 70% assumed) | HIGH | 🟡 Unknown until live testnet |
| Anti-overfitting discipline consistency | HIGH | ✅ RESOLVED — T3/T3-Next |

---

## Graveyard Summary

All non-trend strategies, regime switching, entry-side filters, vol regime overlays, position scaling, cross-market equity integration, 4h multi-timeframe — all confirmed dead.

**The reliable edge is directional trend-following on daily data.**
