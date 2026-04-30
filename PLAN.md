# PLAN.md — Krypto Research & Execution Plan

**State: 2026-04-30 12:01 UTC. T33 COMPLETE ✅. S6 9-universe validation COMPLETE ✅. close_losers I=5 CANDIDATE. trim_losers I=5 REJECTED. Live testnet CRITICAL BLOCKER (4+ weeks).**

---

## Production Params (FROZEN)

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
VOL_LOOKBACK    = 8      // ✅ T33 Base5 revalidation: VL=90 = same-harness spiral, reverted
ATR_EMA_PERIOD  = 1      // ✅ confirmed NULL [1..200]
```

---

## Next Tasks

### T34: Live Bot Dual-Exit Gap Verification
**Status:** ⚠️ IDENTIFIED (not fixed). Live bot uses Turtle-only exit. Walk-forward uses dual Chandelier+Turtle ATR. ~26pp gap unverified in code.
**Gap:** Live bot (`src/live/bot.rs` lines 396-425): Turtle ATR SOLE exit. Walk-forward: dual Chandelier+Turtle ATR. T22 attribution showed dual-exit 93% vs Turtle-only 67%.
**Options:**
1. Add Chandelier exit to bot.rs (requires code change + revalidation)
2. Accept gap as known live/backtest divergence and document it
**Decision:** Defer. Live testnet (blocked on API keys) is the real path forward. Without live data, dual-exit implementation is speculative.

### S6: Rebalancing 9-Universe Validation — COMPLETE ✅
**Result:** `close_losers I=5` generalized across 9 universes × 6 windows: 48/54 pass (88.9%) vs baseline 46/54 (85.2%), Sharpe +6.895 vs +3.828, avg return +89.9% vs +87.8%, trades 1135 vs 1455 (turnover down, not up). **Status: CANDIDATE.**
**Rejected:** `trim_losers I=5` improved pass (51/54) and DD (-1.7pp) but Sharpe was identical to baseline (+3.828) and return fell (+81.1% vs +87.8%); mechanism mostly size/accounting reshuffle, not robust edge.
**Evidence:** `examples/rebalancing_9universe.rs`, `snapshots/rebalancing_9universe.csv`, `snapshots/rebalancing_9universe.md`.
**Next:** Do not promote blindly. If touching live execution, first resolve T34 live bot dual-exit divergence; otherwise live testnet credentials remain the real blocker.

### T9: Live Testnet — CRITICAL BLOCKER
**Status:** BLOCKED on Noah's Binance testnet API keys for 4+ weeks.
**Everything else is secondary.** All metrics are upper bounds.
**What we need:** Binance testnet API key + secret (not production keys).

---

## Blind Spots

| Blind Spot | Severity | Status |
|-----------|----------|----------|
| **VOL_LOOKBACK 90 validated → REVERTED** | ✅ CLOSED | T33 done: VL=8 and VL=90 tied on Base5. VL=90 = same-harness spiral. Reverted to VL=8. |
| **Live bot dual-exit gap** | HIGH | T34 identified but not fixed. Live = Turtle-only. WF = dual Chandelier+Turtle. |
| **2026 YTD no root cause** | MEDIUM | No live-vs-backtest divergence test. |
| **S6 9-universe validation** | ✅ CLOSED | close_losers I=5 candidate survived 9u×6; trim_losers rejected. |
| **No live testnet** | CRITICAL | BLOCKED on Noah's API keys — 4+ weeks |

---

## Graveyard Summary (additions since last cycle)

| Strategy | Result | Key Reason |
|----------|--------|------------|
| ATR_EMA [1..200] | NULL | Confirmed NULL at [1..30]. 10,800 runs = spinning. |
| ATR_ENTRY_MULT 201-value sweep | EM=0.00 | Confirmed on current params. EM=0.94: held-out REJECTED (10/18 vs 11/18). |
| HOLD_MAX [1..100] | Confirmed | HM=12 confirmed again. Repeat confirmation. |
| Donchian sleeve | REJECTED | 9-universe: 34/54 pass (63%) < 69.1% guardrail. |
| VOL_LOOKBACK 90 | REVERTED | T33 Base5 revalidation: VL=8 and VL=90 tied (5/6, same Sharpe). Same-harness confirmation spiral. Reverted to VL=8. |
| Rebalancing trim_losers I=5 | REJECTED | 9u×6: pass 51/54 and DD improved, but Sharpe identical to baseline (+3.828) and return lower (+81.1% vs +87.8%). |
