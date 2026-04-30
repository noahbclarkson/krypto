# PLAN.md — Krypto Research & Execution Plan

**State: 2026-04-30 08:05 UTC. S6 BUILT ✅. T31 REJECTED ✅. EM=0.94 REJECTED ✅. VL=90 UNVALIDATED ON BASE5 ⚠️. Live testnet CRITICAL BLOCKER (4+ weeks).**

---

## Brutal Self-Assessment (2026-04-30 Critique Cycle — Seventh Session)

**Research loop: CONFIRMATION SPIRAL continues — VL=90 edition.**

This session: 2.5/5 genuine new work. S6 rebalancing genuinely built, T31 genuinely rejected. BUT:
- VOL_LOOKBACK changed 8→90 via 54,000 sims on the same harness that confirmed VL=8 one day prior. Same methodology, same windows, higher resolution. This is the EP=24 pattern (same-harness resweep → failed held-out).
- VL=90 was NOT re-tested on Base5 production universe. 9-universe aggregate improvement ≠ Base5 improvement.
- HOLD_MAX [1..100] re-confirmed HM=12 again — already confirmed 2026-04-21.

**What we got right:**
- S6 rebalancing: BUILT (close_losers I=5, 6/6 pass, +3.14 Sharpe). First genuinely untested idea built in 5+ weeks.
- T31 Donchian sleeve: REJECTED (9-universe, 63% < 69.1% guardrail). Correctly done.
- EM=0.94: REJECTED (held-out 10/18 vs baseline 11/18). Anti-overfit discipline held.
- Reports honest: methodology labels prevent misreading Sharpe numbers ✅

**What we're still fooling ourselves about:**
- **VOL_LOOKBACK 8→90 = same-harness confirmation spiral.** VL=8 confirmed on 2026-04-29 via 100-value sweep. VL=90 found via 100-value sweep one day later on same harness. Was NOT tested on Base5. Needs Base5 re-validation before trust.
- **Live bot dual-exit gap: still unverified.** Live bot uses Turtle-only (sole exit). Walk-forward uses dual Chandelier+Turtle ATR. 26pp gap acknowledged in PLAN but never verified in code.
- **2026 YTD root cause: still not quantified.** Live-vs-backtest divergence test doesn't exist.

---

## Production Params (FROZEN — requires VL=90 Base5 re-validation)

```
EP              = 21     // ✅ held-out confirmed
TURTLE_ATR_P    = 24     // ✅ fine sweep confirmed
TURTLE_ATR_M    = 2.0    // ✅ confirmed
CHAND_PERIOD    = 7      // ✅ 71-value dense sweep confirmed
CHAND_MULT      = 2.30   // ✅ 71-value dense sweep confirmed
ATR_ENTRY_MULT  = 0.00   // ✅ held-out rejected EM=0.94
HOLD_MAX        = 12     // ✅ confirmed [1..100]
POSITION_CAP    = 3      // ✅ confirmed
FRESHNESS_COOLDOWN = 0   // ✅ confirmed
VOL_LOOKBACK    = 90     // ⚠️ UNVALIDATED — changed 2026-04-30, not tested on Base5
ATR_EMA_PERIOD  = 1      // ✅ confirmed NULL [1..200]
```

---

## Next Tasks

### T33: VOL_LOOKBACK 90 vs 8 — Base5 Re-Validation (HIGHEST PRIORITY)
**Status:** ⚠️ UNVALIDATED. Latest commit changed VL from 8→90 via 54,000 sims. Not tested on Base5.
**Risk:** VL=8 was the winner on Base5×6 windows (2026-04-29). VL=90 is a 9-universe aggregate winner but may underperform on Base5 specifically. This is the EP=24 failure pattern: global winner ≠ production-universe winner.
**What to build:** Run VL=90 vs VL=8 on Base5 × 6 windows. If VL=90 wins Base5, update production default. If VL=90 loses Base5, revert to VL=8 and do NOT resweep.
**Do NOT run a 100-value sweep.** Just compare VL=90 vs VL=8. 12 runs.

### T34: Live Bot Dual-Exit Gap Verification
**Status:** Acknowledged since 2026-04-29. Live bot uses Turtle-only. Walk-forward uses dual Chandelier+Turtle ATR.
**Gap:** ~26pp pass rate (Turtle-only 67% vs dual 93% in T22 attribution).
**What to build:** Verify `src/live/bot.rs` exit logic. If Turtle-only: assess dual Chandelier implementation cost, or formally document the gap as a known live/in-sample divergence.
**This is the #1 production readiness blocker** (after API keys).

### S6: Rebalancing 9-Universe Validation — CANDIDATE, NOT PROMOTED
**Status:** close_losers I=5 built on Base5 (6/6 pass, +3.14 Sharpe). Needs 9-universe × 6 windows validation before production consideration.
**What to build:** `examples/rebalancing_9universe.rs` — only close_losers I=5 vs no-rebalancing. No interval re-sweep. 9 universes × 6 windows.
**Reject if:** 9-universe pass rate drops >5pp below baseline, or materially increases turnover.

### T9: Live Testnet — CRITICAL BLOCKER
**Status:** BLOCKED on Noah's Binance testnet API keys for 4+ weeks.
**Everything else is secondary.** All metrics are upper bounds.
**What we need:** Binance testnet API key + secret (not production keys).

---

## Blind Spots

| Blind Spot | Severity | Status |
|-----------|----------|----------|
| **VOL_LOOKBACK 90 not validated on Base5** | HIGH | Latest commit (cfd19ba2). VL=8 was Base5 winner 2026-04-29. VL=90 not tested on Base5. EP=24 pattern risk. |
| **Live bot dual-exit gap** | HIGH | Live: Turtle-only. WF: dual Chandelier+Turtle. ~26pp gap unverified. |
| **2026 YTD no root cause** | MEDIUM | -22.7% Turtle vs +12.7% BTC. No live-vs-backtest divergence test exists. |
| **Research loop: VL confirmation spiral** | MEDIUM | VL=8 confirmed 2026-04-29. VL=90 found 2026-04-30 via same methodology. Stop resweeping settled params. |
| **Funding observer: not continuous** | LOW | T29 built but not running hourly. Need cron job. |
| **No live testnet** | CRITICAL | BLOCKED on Noah's API keys — 4+ weeks |

---

## Graveyard Summary (additions since last cycle)

| Strategy | Result | Key Reason |
|----------|--------|------------|
| ATR_EMA [1..200] | NULL | Confirmed NULL at [1..30]. 10,800 runs = spinning. |
| ATR_ENTRY_MULT 201-value sweep | EM=0.00 | Confirmed on current params. EM=0.94: held-out REJECTED (10/18 vs 11/18). |
| HOLD_MAX [1..100] | Confirmed | HM=12 confirmed again. Repeat confirmation. |
| Donchian sleeve | REJECTED | 9-universe: 34/54 pass (63%) < 69.1% guardrail. |
| VOL_LOOKBACK 8→90 | ⚠️ UNVALIDATED | Same-harness resweep. Not tested on Base5. May be EP=24 pattern. |

---

## Research Loop: CONFIRMATION SPIRAL — NOT CLOSED

The loop closes when we:
1. T33: Re-validate VL=90 vs VL=8 on Base5 (12 runs, NOT a 100-value sweep)
2. T34: Verify live bot dual-exit gap in code
3. S6: 9-universe rebalancing validation for close_losers I=5
4. Live testnet (BLOCKED on API keys)

**Stop running hyperopts on settled params. VL=90, ATR_EMA, HOLD_MAX are all settled.**
