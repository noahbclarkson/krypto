# PLAN.md — Krypto Research & Execution Plan

**State: 2026-04-28 11:15 UTC. T19 done. 3 new structural tasks identified. Live testnet remains critical blocker.**

---

## Brutal Self-Assessment (2026-04-28 Critique Cycle)

**T19 (Donchian) COMPLETED — first genuinely new test in many sessions.**
- Donchian (close > max(high)) vs Turtle (close > max(close)) on Base5 × 7 windows.
- **Result:** Donchian wins Sharpe (+3.81 avg) but loses pass rate (-14pp). W04 dominant: Donchian +15.7 vs Turtle +1.3 (bear chop filters false breakouts). W05 fails: Donchian caught in whipsaw.
- **VERDICT:** Donchian is NOT a Turtle replacement. Turtle entry remains production default.
- Entry signal space is now exhausted — all variants (ATR filter, vol confirmation, Donchian) either degrade or improve Sharpe at cost of pass rate.

**What we got right:**
- Anti-overfitting rules working (EP=24, ATR_ENTRY_MULT=0.85 properly rejected for in-sample inflation)
- Equity Sharpe (~1.29) correctly reported as honest number — never show 5.46 on equity charts
- Graveyard thorough — every failed strategy documented
- Cross-market validation confirms edge is real (SPY/QQQ/GLD ~60%+)
- Base5: 7/7 pass — production universe is clean
- Anti-spin check catches self-congratulation (last 5 sessions: mostly null/confirm results)

**What we're still fooling ourselves about:**
- **Pre-2021 stress: 67.9% pass** — BELOW our own 70% threshold. The most honest stress test fails. P=7/M=2.30 is tighter than prior P=28/M=2.00 and struggles in choppy pre-2021 regimes. We may be overfit to bull crypto.
- **POSITION_CAP unconfirmed for dual-exit** — CAP=3 validated on Turtle-only harness. Production bot uses dual-exit. Structural gap.
- **All metrics are upper bounds** — fee model, maker-fill rate, slippage unvalidated in live market
- **Turtle-only vs dual-exit attribution unknown** — if Chandelier fires first >90% of trades, dual-exit validation = Chandelier-only validation. Are we testing what we think we're testing?
- **Documentation loop is structural** — 6 of last 10 commits are hygiene. Only live testnet breaks this pattern.

---

## Production Params (FROZEN — all validated, do NOT re-sweep)

```
EP              = 21     // ✅ held-out confirmed (reverted from EP=24 2026-04-26)
TURTLE_ATR_P    = 24     // ✅ fine sweep confirmed 3×
TURTLE_ATR_M    = 2.0    // ✅ 81-value dense sweep confirmed (NULL result)
CHAND_PERIOD    = 7      // ✅ held-out confirmed
CHAND_MULT      = 2.30   // ✅ 71-value dense sweep
ATR_ENTRY_MULT  = 0.00   // ✅ 41-value sweep — no filter wins
HOLD_MAX        = 12     // ✅ HM=12 wins +71.4% Sharpe vs HM=45 baseline
POSITION_CAP    = 3      // ⚠️ Turtle-only harness only — dual-exit unconfirmed
FRESHNESS_COOLDOWN = 0   // ✅ cd=0 wins
VOL_LOOKBACK    = 9      // ✅ confirmed on current production params
```

---

## Next 3 Execution Tasks

### T21: CAP=3 Dual-Exit Re-Validation (HIGH — structural integrity)
**Concept:** Re-run POSITION_CAP sweep under actual production dual-exit logic (Chandelier(7,2.30) + Turtle ATR(24,2.0)), not Turtle-only harness.
**Why this matters:** CAP=3 was validated on Turtle-only exit. Production bot uses dual-exit. CAP=3 "likely holds" but is unconfirmed — structural gap between validated harness and production code.
**Risk if skipped:** Live testnet goes live with position sizing validated on the wrong exit logic.
**Execution:** Build harness with dual-exit. Sweep CAP 1-10. Compare to Turtle-only CAP sweep results.
**Status:** Critical gap. Untested.

### T22: Turtle-Only vs Dual-Exit Attribution (MEDIUM — structural understanding)
**Concept:** Instrument the walk-forward harness to count Chandelier-first vs Turtle ATR-first exits per window.
**Why this matters:** At P=7/M=2.30, Chandelier is extremely tight (fires at ~bar 7-12). If it fires first >90% of trades, dual-exit ≈ Turtle-only + safety net. If Turtle ATR fires first >50%, dual-exit adds genuine marginal value.
**Decision value:** Determines whether we're actually testing what we think we're testing.
**Execution:** Add exit-type tracking to existing walk-forward harness. Report % per window and aggregate.
**Status:** Structural understanding, not new strategy. Untested.

### T23: ATR-Rank Conditional Entry Filter (MEDIUM — last untested idea)
**Concept:** Only enter if current 21-bar ATR > Nth percentile of 252-bar history. Regime-dependent threshold — different from fixed ATR_MULT.
**Why this matters:** ATR_MULT (fixed threshold) failed at all values. ATR-rank is regime-dependent — mechanically different. **Caution:** T19 (Donchian) shows entry alternatives trade pass rate for per-trade Sharpe. ATR-rank may have the same problem.
**Test:** Threshold {50th, 60th, 70th percentile} × Base5 × 6 windows.
**Status:** Last genuinely untested idea. If this fails, research loop closes definitively.

---

## T9: Live Testnet — CRITICAL BLOCKER

**Status:** BLOCKED on Noah's Binance testnet API keys.
**Everything else is secondary.** The project cannot advance without live market validation. All metrics are upper bounds. Fee model, maker-fill rate, slippage — all unvalidated.
**Escalation:** This has been blocked for 3+ weeks. Nothing advances the project until this is resolved.
**What we need:** Binance testnet API key + secret. Not production keys — testnet only.

---

## Stop Doing

- Re-sweeping confirmed params. All are frozen. Stop.
- Re-testing confirmed strategies (BollingerReversion, CTREND, etc.) — graveyard is final.
- Building documentation-only commits. Documentation loop is structural. Only live testnet breaks it.
- Claiming walk-forward Sharpe 5.46 on equity charts — use equity Sharpe 1.29 only.

---

## Blind Spots

| Blind Spot | Severity | Status |
|-----------|----------|--------|
| **No live testnet** | CRITICAL | BLOCKED on Noah's API keys |
| **Pre-2021 stress: 67.9%** | HIGH | Below our 70% threshold — P=7/M=2.30 may be overfit to bull |
| **POSITION_CAP unconfirmed for dual-exit** | HIGH | CAP=3 validated Turtle-only only — structural gap |
| **Turtle vs dual-exit attribution unknown** | MEDIUM | Chandelier fires first >90% of trades? Unknown. |
| **All metrics are upper bounds** | HIGH | Fee model, maker-fill unvalidated |
| **Documentation loop is structural** | MEDIUM | 6/10 recent commits are hygiene |

---

## Graveyard Summary

| Strategy | Result | Key Reason |
|----------|--------|------------|
| EP=24 | REVERTED | In-sample inflation (same OOS data as P=7 + ATR_EM) |
| ATR_ENTRY_MULT>0 | REJECTED | All non-zero values degrade pass rate |
| 4h multi-timeframe | GRAVEYARD | Structural failure (1/20 pass) |
| Cross-market equity integration | REJECTED | Combined -2.94 Sharpe vs crypto-only |
| DynamicTrend EMA signal | REJECTED | Turtle wins 21/24 windows |
| BollingerReversion | GRAVEYARD | 0/288 OOS pass |
| BOCPD regime detector | GRAVEYARD | 0% breaks |
| FDUSD basis carry | GRAVEYARD | 19% pass |
| Funding rate MR | GRAVEYARD | 43% pass |
| Vol-contingent Chandelier | GRAVEYARD | All configs identical |
| Position scaling overlays | GRAVEYARD | All failed |
| CTREND regime-conditional switching | REJECTED | 67% pass < 70% threshold |
| Donchian entry | REJECTED (not replacement) | Wins Sharpe (+3.8) but loses pass rate (-14pp) |
| Correlation entry filter (T7) | REJECTED | All 3 variants lose to baseline |
| EP=43 | REVERTED | Same-session in-sample inflation as EP=21 |

---

## Research Loop: What Remains

1. **T21:** CAP=3 dual-exit re-validation — structural gap, critical before testnet
2. **T22:** Turtle vs dual-exit attribution — structural understanding
3. **T23:** ATR-rank conditional filter — last untested idea, closes loop definitively
4. **T9:** Live testnet — BLOCKED on Noah's API keys

**Note:** The research loop is functionally closed. T21-T23 are structural integrity tasks, not new edge discovery. Live testnet is the only thing that validates everything.
