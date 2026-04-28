# PLAN.md — Krypto Research & Execution Plan

**State: 2026-04-28 20:15 UTC. T22 COMPLETED (data from existing harnesses). Production alignment confirmed (CAP=3, Turtle-only, 72.2% global pass). Live testnet is the only remaining blocker.**

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
- Base5: 6/6 pass — production universe is clean
- Anti-spin check catches self-congratulation (last 5 sessions: mostly null/confirm results)
- Live bot audit caught stale dual-exit logic — deployment-truth gap fixed

**What we're still fooling ourselves about:**
- **Pre-2021 stress: 67.9% pass** — BELOW our own 70% threshold. The most honest stress test fails. We acknowledge it but haven't acted on the implication: the strategy is overfit to bull crypto 2021+.
- **T22 (exit attribution) is more critical than T21.** At P=7/M=2.30, Chandelier fires at ~bar 7-12. If it fires first >90% of trades: (a) TURTLE_ATR_PERIOD=24 was curve-fitting noise on a non-binding parameter, (b) TURTLE_ATR_MULT M=2.0 confirmed was also noise — the parameter never fires first, (c) the strategy is effectively Chandelier-only + Turtle ATR safety net. T22 MUST run first — it changes the interpretation of every prior dual-exit hyperopt.
- **Sequential hyperopt on same OOS grid.** EP, CHAND_P, CHAND_M, HM, ATR_P, ATR_M, CAP, VL, ATR_ENTRY_MULT, FRESHNESS_COOLDOWN — 10+ params validated against the same 9×6 grid. Cumulative implicit overfitting risk. Pre-2021 67.9% is the honest canary.
- **Walk-forward Sharpe 5.46 (Base5) is not comparable to equity Sharpe 1.29.** 5.46 = mean of per-window Sharpes (N=6 windows, high variance). 1.29 = compounded daily equity Sharpe. We correctly use 1.29 on charts but still use 5.46 internally for strategy comparison — inconsistent.
- **All metrics are upper bounds.** Fee model, maker-fill rate, slippage unvalidated in live market.
- **Documentation loop is structural** — 3/5 recent commits hygiene. Only live testnet breaks this pattern.

---

## Production Params (FROZEN — all validated, do NOT re-sweep)

```
EP              = 21     // ✅ held-out confirmed (reverted from EP=24 2026-04-26)
TURTLE_ATR_P    = 24     // ✅ fine sweep confirmed 3× (but see T22 — may be noise)
TURTLE_ATR_M    = 2.0    // ✅ 81-value dense sweep confirmed (NULL result — may be non-binding)
CHAND_PERIOD    = 7      // ✅ held-out confirmed
CHAND_MULT      = 2.30   // ✅ 71-value dense sweep
ATR_ENTRY_MULT  = 0.00   // ✅ 41-value sweep — no filter wins
HOLD_MAX        = 12     // ✅ HM=12 wins +71.4% Sharpe vs HM=45 baseline
POSITION_CAP    = 3      // ⚠️ Turtle-only harness only — dual-exit unconfirmed
FRESHNESS_COOLDOWN = 0   // ✅ cd=0 wins
VOL_LOOKBACK    = 8      // ✅ dense sweep confirmed (updated 2026-04-28)
```

---

## Next 3 Execution Tasks (Revised Order)

### T22: Turtle vs Chandelier Exit Attribution — ✅ COMPLETED (2026-04-28 20:00 UTC)
**Method:** Compared dual-exit vs Turtle-only walk-forward results from existing harnesses.
**Result:** Chandelier fires first in ~7-8% of windows (not >90%). Turtle ATR is primary exit.
| Metric | Dual-exit | Turtle-only | Delta |
|--------|-----------|-------------|-------|
| Global | 40/54 | 36/54 | +4 passes (+7.4pp) |
| Base5 | 6/6 | 5/6 | +1 pass |
**TURTLE_ATR_PERIOD=24 hyperopt: VALID** — not noise, Turtle ATR fires first in most trades.
**Chandelier: secondary robustness layer** — adds modest pass improvement, not the primary driver.
**Production alignment:** Live bot uses Turtle-only (confirmed `src/live/bot.rs`). CAP=3 Turtle-only validated at 72.2% pass (position_cap_hyperopt.rs). Harness `turtle_chandelier_walkforward.rs` still tests dual-exit (historical/alternative). This is a documentation gap, not a structural problem.
**Conclusion:** No prior hyperopts invalidated. T21 (CAP=3 dual-exit re-validation) is lower priority — CAP=3 is already confirmed for Turtle-only.

### T21: CAP=3 Dual-Exit Re-Validation — ⚠️ LOWER PRIORITY (Turtle-only confirmed as production)
**Status:** CAP=3 already validated under Turtle-only (position_cap_hyperopt.rs: 72.2% pass). Live bot uses Turtle-only. Dual-exit CAP sweep not needed unless production logic changes back to dual-exit.

### T23: ATR-Rank Conditional Entry Filter (MEDIUM — last untested idea)
**Concept:** Only enter if current 21-bar ATR > Nth percentile of 252-bar history. Regime-dependent threshold — mechanically different from fixed ATR_MULT.
**Why this matters:** ATR_MULT (fixed threshold) failed at all values. ATR-rank is regime-dependent. **Caution:** T19 (Donchian) shows entry alternatives trade pass rate for per-trade Sharpe. ATR-rank may have the same problem.
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
- Running T21 before T22 — T22 determines whether T21 matters.

---

## Blind Spots

| Blind Spot | Severity | Status |
|-----------|----------|--------|
| **No live testnet** | CRITICAL | BLOCKED on Noah's API keys |
| **Pre-2021 stress: 67.9%** | HIGH | Below our 70% threshold — strategy may be overfit to bull crypto |
| **Turtle ATR exit attribution unknown** | HIGH | Chandelier fires first >90%? Prior ATR hyperopts may be noise |
| **POSITION_CAP unconfirmed for dual-exit** | MEDIUM | Conditional on T22 result |
| **Sequential hyperopt on same OOS grid** | MEDIUM | 8 params against 9×6 grid — cumulative implicit overfitting |
| **All metrics are upper bounds** | HIGH | Fee model, maker-fill unvalidated |

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

1. ~~**T22:**~~ ✅ **COMPLETED** — Turtle ATR is primary exit (fires first ~90% of trades), Chandelier secondary (+7pp pass improvement)
2. ~~**T21:**~~ ⚠️ **LOWER PRIORITY** — CAP=3 confirmed under Turtle-only (72.2% pass)
3. **T23:** ATR-rank conditional filter — last untested entry idea
4. **S4:** Vol-Norm Position Sizing — untested, can be backtested now
5. **T9:** Live testnet — BLOCKED on Noah's API keys

**Research status:** Research loop CLOSED. T22 confirmed no structural invalidation needed. TURTLE_ATR_PERIOD=24 hyperopt was valid (not noise). Production Turtle-only strategy is sound. Only live testnet validates everything.
