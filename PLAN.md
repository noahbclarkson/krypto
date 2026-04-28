# PLAN.md — Krypto Research & Execution Plan

**State: 2026-04-28 10:32 UTC. T19 Donchian COMPLETED. Research loop CLOSED. Live testnet is the only path forward.**

---

## Brutal Self-Assessment (2026-04-28)

**T19 (Donchian) COMPLETED — first genuinely new test in many sessions.**
- Donchian (close > max(high)) vs Turtle (close > max(close)) on Base5 × 7 windows.
- **Result:** Donchian wins Sharpe (+3.81 avg) but loses pass rate (-14pp). W04 dominant: Donchian +15.7 vs Turtle +1.3 (bear chop filters false breakouts). W05 fails: Donchian caught in whipsaw.
- **VERDICT:** Donchian is NOT a Turtle replacement. Turtle entry remains production default.
- Entry signal space is now exhausted — all variants (ATR filter, vol confirmation, Donchian) either degrade or improve Sharpe at cost of pass rate.

**What we got right:**
- Anti-overfitting rules working (EP=24 properly rejected for in-sample inflation)
- Equity Sharpe (~1.29) correctly reported as honest number — never show 5.46 on equity charts
- Graveyard thorough — every failed strategy documented
- Cross-market validation confirms edge is real (SPY/QQQ/GLD ~60%+)
- Base5: 7/7 pass — production universe is clean

**What we're still fooling ourselves about:**
- All metrics are upper bounds — fee model, maker-fill rate, slippage unvalidated in live market
- POSITION_CAP: validated for Turtle-only harness, not dual-exit production bot — likely holds but unconfirmed

---

## Production Params (FROZEN — all validated, do NOT re-sweep)

```
EP              = 21     // ✅ held-out confirmed (reverted from EP=24 2026-04-26)
TURTLE_ATR_P    = 24     // ✅ fine sweep confirmed
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

## Next Execution Tasks

### T20: ATR-Rank Conditional Entry Filter (LOW PRIORITY — untested)
**Concept:** Only enter if current 21-bar ATR > 60th percentile of 252-bar history. Regime-dependent threshold — different from fixed ATR_MULT.
**Status:** Untested. Genuinely novel. But T19 result suggests entry space is saturated.
**Decision:** Run T20 only if Noah wants more research. Otherwise skip — live testnet is higher value.

### T9: Live Testnet — CRITICAL BLOCKER
**Status:** BLOCKED on Noah's Binance testnet API keys.
**Everything else is secondary.** The project cannot advance without live market validation.
**What we need:** Binance testnet API key + secret. Not production keys — testnet only.

---

## Stop Doing

- Re-sweeping confirmed params. All are frozen. Stop.
- Re-testing confirmed strategies (BollingerReversion, CTREND, etc.) — graveyard is final.
- Building documentation-only commits. Documentation loop is structural. Only live testnet breaks it.

---

## Blind Spots

| Blind Spot | Severity | Status |
|-----------|----------|--------|
| **No live testnet** | CRITICAL | BLOCKED on Noah's API keys |
| **POSITION_CAP unconfirmed for dual-exit** | HIGH | CAP=3 validated Turtle-only only |
| **All metrics are upper bounds** | HIGH | Fee model, maker-fill unvalidated |
| **Entry signal space exhausted** | HIGH | T19 complete — no more untested ideas |

---

## Graveyard Summary

All strategies confirmed dead:

| Strategy | Result | Key Reason |
|----------|--------|------------|
| EP=24 | REVERTED | In-sample inflation |
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
| Donchian entry | REJECTED (not replacement) | Wins Sharpe (+3.8) but loses pass rate (-14pp). Turtle remains default. |

---

## Research Loop: What Remains

1. **T20:** ATR-rank conditional filter — untested, low priority vs live testnet
2. **T9:** Live testnet — BLOCKED on Noah's API keys. The only thing that validates everything.

**Note:** Research loop is CLOSED. All testable ideas are exhausted. The loop closes only when live testnet provides real market feedback.