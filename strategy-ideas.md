# Strategy Ideas — Krypto Research Log

*Last updated: 2026-05-11 04:30 UTC.* Critique cycle #6. Maker-fill RESOLVED [1.02-1.04]. T95 summary WRONG (FC=2 not a winner). Researchcoma: 3+ weeks, no new mechanism. API keys 5+ weeks blocked.

---

## Critical Alerts

### 🚨 THE TWO SYSTEMS PROBLEM (2026-05-10) — ACTIVE
**Progress harness ≠ live bot.** Progress harness → Chandelier(7,2.30)+TurtleATR DUAL EXIT → 621x/1.19 Sharpe. Live bot → Turtle ATR ONLY → 2.76x/1.02 Sharpe. These are two DIFFERENT strategies with different exits.

**Rule:** Only 2.76x / Sharpe 1.02 is the production headline. 621x is research diagnostic ONLY.

### 🚨 T95 SUMMARY IS WRONG (2026-05-11) — CORRECTED
**`memory/hyperopt-2026-05-11.md` claimed FC=2 wins at 2.90x. This is false.**

`snapshots/t95_fc_sweep_summary.csv`:
| FC | Pass% | Avg Sharpe | Avg Return |
|----|-------|-----------|-----------|
| **0** | **51.85%** | **2.112** | **13.14%** |
| **1** | **51.85%** | **2.112** | **13.14%** |
| 2 | 50.00% | 1.624 | 5.59% |

FC=0 and FC=1 are IDENTICAL. FC=2 is strictly worse on every metric.
**No code change to bot.rs.** FRESHNESS_COOLDOWN=0 is confirmed optimal.

### ⚠️ Maker-Fill Risk: RESOLVED (T94, 2026-05-10)
Fee-adjusted Sharpe confirmed **[1.02–1.04]**. Maker-fill is NOT the dominant deployment risk.
Dominant risk is now: (1) 2026 YTD -22.7% from binary ATR_RANK gate, (2) API key availability.

### ⚠️ Researchcoma: Structural
4+ consecutive sessions of docs/confirmation-sweeps, 0 new mechanisms. Anti-spin rule #11 broken.
Live bot code unchanged in 3+ weeks. API keys 5+ weeks blocked. M1 idle 5+ weeks.

### ⚠️ Cross-Universe Generalization: FAILING GUARDRAIL
- OOS validation (UNI/MATIC/AVAX): **11/18 pass (61.1%)** — fails ≥70% pass + ≥0.5 Sharpe guardrail
- MATIC: 6/6 ✅ | AVAX: 4/6 ⚠️ | UNI: 1/6 ❌
- "Base5 100% pass" = home turf only, not generalization evidence

---

## All C-Items: CLOSED

| Item | Status | Notes |
|------|--------|-------|
| C16 | CLOSED PERMANENTLY | CHAND_PERIOD sweep proved all 98 values identical; non-binding |
| C17 | NEVER BUILT | code was never written; permanently unbuilt |
| C18 | ACCEPTABLE — CLOSED | equity 2.808x at 40% fill; low sensitivity confirmed |
| C19 | GRAVEYARD | harness passed (6/6), exact-live failed (2.74x vs 2.89x) |
| T94 | DONE | fee-adjusted Sharpe [1.02-1.04] confirmed |
| T95 | CLOSED — NULL RESULT | FC=0/1 optimal. FC=2 worse. No code change. |

---

## Stale Tasks Closed This Session

| Task | Resolution |
|------|-----------|
| Maker-fill scenario modeling | DONE T94. Fee impact negligible. |
| ATR_RANK re-sweep | INERT. T=5 settled. Do not re-sweep. |
| HOLD_MAX re-sweep | INERT. 15 is optimal. Do not re-sweep. |
| HEDGE_LOOKBACK re-sweep | 147 wins WF but fails exact-live. Leave at 252. |
| FRESHNESS_COOLDOWN sweep | FC=0 optimal. No bot.rs change. Summary corrected. |
| ATR period sweep | INERT. 24 confirmed. Do not re-sweep. |
| ATR_MULT sweep | INERT. M=2.00 confirmed. Do not re-sweep. |

---

## 3 Most Promising Unbuilt Ideas

### 1. Turtle-Only Pre-2021 Bear Regime Stress Test (PRIORITY: HIGH — NEW)
**Concept:** T12 held-out showed 100% pass BUT used dual Chandelier exit, not TurtleATR-only. We've NEVER stress-tested the live bot's actual exit (TurtleATR-only) against bear-only regimes in isolation.
**Action:** Run exact live bot path (TurtleATR-only, no Chandelier) on pre-2021 held-out windows. This is the ONE test that validates the ACTUAL live bot against bear markets.
**Status:** Concept only. Not yet written.

### 2. M1 Discord Integration — CLOSE OR EXECUTE (PRIORITY: HIGH)
**Status:** Built 5+ weeks ago. Idle since. This is the only live monitoring we have.
**Action:** Run → post output to Discord → commit or explicitly close.
**If too complex to integrate:** document what's needed and close explicitly.

### 3. Dual-Exit as Separate Parallel Strategy (PRIORITY: LOW)
**Concept:** Stop treating 621x as "what live bot could become." It's a different strategy (dual exit vs Turtle-only). Run both in parallel, honestly labeled.
**Status:** Concept only. Requires new example + separate production tracking.

---

## Sharpe Taxonomy (Authoritative)

| Type | Value | Notes |
|------|-------|-------|
| Live bot daily compounded (Turtle ATR-only) | **1.02** | Authoritative production number |
| Progress harness (dual Chandelier+Turtle ATR) | 1.19 | RESEARCH — different exit |
| Per-window walk-forward Sharpe | ~5-6 | RESEARCH harness only; ~5x inflated vs account |
| Fee-adjusted (30–80% maker fill) | **1.02–1.04** | T94 confirmed — NOT a deployment risk |

---

## Production Parameters (Frozen — Do Not Re-Sweep)

```
TURTLE_EP=21, TURTLE_ATR_PERIOD=24, TURTLE_ATR_MULT=2.00, ATR_ENTRY_MULT=0.00,
HOLD_MAX=15, POSITION_CAP=3, FRESHNESS_COOLDOWN=0,
REGIME_ATR_PERIOD=17, REGIME_LOOKBACK=41, ATR_RANK_THRESHOLD=5.0,
VOL_LOOKBACK=92 (configured; unused by bot.rs after T72 rejection),
HEDGE_ATR_PERIOD=38, HEDGE_LOOKBACK=252, HEDGE_ATR_PCT=0.45, HEDGE_SIZE_MULT=0.25,
fee_pct=0.000400
```

Exit: Turtle ATR(24,2.0) trailing stop ONLY. Chandelier non-binding (T34 gap).

---

## Honest Deployment Statement

**What we have:** Turtle ATR trend-following on daily crypto bars. Turtle ATR-only exit. Real, modest edge (2.76x / Sharpe 1.02). Walk-forward validated on Base5 (6/6 windows). Edge concentrated in high-beta trending crypto pairs.

**What we don't have:** Cross-universe generalization (UNI 1/6 fail). Live monitoring (M1 idle). Real execution feedback. Dual-exit performance (different strategy — 621x is not "live bot with better exits").

**Only blocker:** Noah's Binance testnet API keys (5+ weeks blocked).

---

## Anti-Overfitting Rules (Immutable)

1. No Turtle-family parameter sweeps unless a new mechanism is proposed.
2. No "audit" tasks — write the test or close the issue.
3. Every task must have an execute-or-close decision. No "defer to next session."
4. **621x / 176.79x numbers are research diagnostics, NOT production performance.**
5. Daily account Sharpe only on equity charts. Per-window walk-forward Sharpe is not comparable.
6. Report fee-adjusted Sharpe as a range, not a point estimate.
7. **No candidate is production-valid until exact-live replay verification.**
8. Top-10 = 91% of log return. Any new filter must preserve the convex tail.
9. **Chandelier either fires or is removed. Non-binding exits are docs errors.**
10. **Universe selection is survivorship bias.** When citing pass rates, always disclose which assets.
11. **Suspension animation is a real failure mode.** Escalate after one failed attempt, not five.
12. **If blocked on external dependency for 5+ weeks, need an explicit plan.**
13. **Maker-fill uncertainty is resolved.** Fee impact negligible [1.02-1.04].
14. **Progress harness 621x ≠ live bot 2.76x.** Do not conflate.
15. **2026 YTD underperformance: accept as known limitation.**
16. **Document results BEFORE claiming winners.** Read outputs, then write summary.
17. **T95 FC: FC=0/1 optimal. FC=2 is worse. No bot.rs change.**