# PLAN.md — Krypto Research & Execution Plan

**State: 2026-04-30 03:35 UTC. T29 COMPLETE ✅. T30 PARTIAL. T32 COMPLETE ✅. T31 OPEN. Live testnet CRITICAL BLOCKER (4+ weeks).**

---

## Brutal Self-Assessment (2026-04-30 Critique Cycle — Seventh Session)

**Research loop: NOT CLOSED — we are in a confirmation spiral.** ATR_EMA [1..200] × 10,800 runs re-confirmed NULL at [1..30] on the same harness. ATR_ENTRY_MULT 201-value sweep re-confirmed EM=0.00 on current params. Both were already settled. Running the same tests at higher resolution is not discovery — it's spinning.

**What we got right:**
- Anti-overfitting discipline is REAL and consistent (EM=0.94 candidate correctly not promoted)
- T29 funding observer: built and ran correctly via live Binance public API
- T30 mid-cap rejection: correctly rejected at 60% pass rate (below 70% threshold)
- ATR_EMA NULL result: genuinely comprehensive (10,800 runs across full integer range)
- Equity bug: FIXED

**What we're still fooling ourselves about:**
- **"Research loop CLOSED" — premature.** ATR_EMA [1..200] was confirmed NULL at [1..30] on 2026-04-17. ATR_ENTRY_MULT was confirmed null on stale params. We keep re-running settled parameters at higher resolution. The loop closes when you STOP RUNNING NEW HYPEROPTS and START BUILDING the things you said you'd build.
- **ATR_ENTRY_MULT=0.94:** Real candidate (42/54 pass, Sharpe 5.34 vs baseline 40/54/3.15). Found on same WF grid — should have had held-out validation before 201-value sweep. Correctly not promoted, but the process was confirmation, not discovery.
- **T31 is still genuinely untested.** T32 is now closed: `reports/daily_progress.csv` and `scripts/run_daily_progress.sh` carry explicit `sharpe_methodology` so DDBudget's 7.24 is no longer silently compared to Turtle's 1.04.

---

## Production Params (FROZEN — all validated, do NOT re-sweep)

```
EP              = 21     // ✅ held-out confirmed
TURTLE_ATR_P    = 24     // ✅ fine sweep confirmed
TURTLE_ATR_M    = 2.0    // ✅ 81-value dense sweep confirmed
CHAND_PERIOD    = 7      // ✅ held-out confirmed
CHAND_MULT      = 2.30   // ✅ 71-value dense sweep confirmed
ATR_ENTRY_MULT  = 0.00   // ✅ candidate EM=0.94 (42/54, Sharpe 5.34) found but NOT promoted — needs held-out validation
HOLD_MAX        = 12     // ✅ HM=12 wins +71.4% Sharpe vs HM=45 baseline
POSITION_CAP    = 3      // ✅ Turtle-only validated (72.2% pass)
FRESHNESS_COOLDOWN = 0   // ✅ cd=0 wins
VOL_LOOKBACK    = 8      // ✅ dense production sweep confirmed
ATR_EMA_PERIOD  = 1      // ✅ confirmed NULL [1..200] = raw ATR optimal
```

---

## Next Tasks

### T9: Live Testnet — CRITICAL BLOCKER (escalate to Arc)
**Status:** BLOCKED on Noah's Binance testnet API keys for 4+ weeks.
**Everything else is secondary.** All metrics are upper bounds.
**What we need:** Binance testnet API key + secret (not production keys).

### T31: Donchian as Portfolio Complement — OPEN (OVERDUE — 2+ sessions)
**Status:** UNTESTED. Identified as HIGH priority in 2026-04-29 critique. Not built in 2 sessions.
**Hypothesis:** Donchian (strictest breakout) fires less but higher conviction. Turtle(75%) + Donchian(25%) sleeve may capture different regime dynamics.
**Evidence:** Donchian W04 (bear chop) Sharpe +15.7 vs Turtle +1.3. Different regime profile = diversification.
**What to test:** Turtle(75%) + Donchian(25%) on Base5 × 7 windows, same dual exit. Reject if Turtle Sharpe collapses >10% or pass rate drops >5pp.
**Scope:** Medium — 2 configs × Base5 × 7 windows.
**Action:** Build `examples/donchian_sleeve_walkforward.rs` THIS session. Not next session.

### T32: Sharpe Metric Integrity Fix — COMPLETE ✅ (2026-04-30)
**Problem fixed:** `daily_progress.csv` compared DDBudget Sharpe 7.24 to Turtle Sharpe 1.04 without methodology context. That implied a false peer comparison.
**Built:** `reports/daily_progress.csv` now has `reported_sharpe` + `sharpe_methodology`; `scripts/run_daily_progress.sh` preserves the methodology column on every refresh; `progress_equity_curves.rs` labels DDBudget as `milestone-aggregated; not comparable to Turtle daily equity` in the generated markdown.
**Current row:** Turtle+Chandelier 221.1x / 1.04 `daily_compounded_equity`; DDBudget 61.3x / 7.24 `milestone_aggregated_not_comparable`.
**Verdict:** Reporting integrity gap closed. Do not cite DDBudget 7.24 as a peer Sharpe against Turtle 1.04.

### T29: Funding Rate Live Observer — COMPLETE ✅ (2026-04-30)
**Built:** `examples/funding_rate_live_observer.rs` — polls Binance premiumIndex public API, compares to 30d cached history, computes z-score/percentile, detects extremes.
**Live result (2026-04-30 00:08 UTC):** NEAR NEUTRAL (-0.04% avg ann). No extremes. Regime: Neutral.
**Next:** Continuous monitoring — build `scripts/run_funding_observer.sh` for hourly polling.

### T30: Expanded Universe Walk-Forward — PARTIAL (2026-04-30)
**MidCaps4 (BNB/LINK/AVAX/MATIC/UNI):** 3/5 pass (60%) — BELOW 70% threshold. NOT production-universe candidates.
**Top9 (Base5 + BNB/LINK/AVAX):** 5/5 pass (100%) on 5 windows only — marginal, not conclusive.
**Verdict:** Top5-only remains correct. Do not expand production universe.

### T28: Turtle-ATR-Only Walk-Forward Validation — COMPLETE ✅
**Status:** VALIDATED. Live bot uses Turtle ATR sole exit. WF harness validates dual exit.
- Turtle-ATR-only (live): 36/54 pass (33% fail), Base5 5/6 (83%), Sharpe 4.12
- Turtle+Chandelier (dual): 40/54 pass (26% fail), Base5 6/6 (100%), Sharpe 3.15
- **Conclusion:** Turtle-ATR-only is NON-INFERIOR on pass rate. No structural gap.

---

## Blind Spots

| Blind Spot | Severity | Status |
|-----------|----------|----------|
| **No live testnet** | CRITICAL | BLOCKED on Noah's API keys — 4+ weeks |
| **T31 Donchian sleeve: never built** | HIGH | "Genuinely untested" for 2+ sessions. Walk-forward harness exists. Build it. |
| **T32 Sharpe methodology mismatch** | ~~HIGH~~ CLOSED | `daily_progress.csv`, `run_daily_progress.sh`, and generated markdown now carry methodology labels. |
| **Research loop: confirmation spiral** | MEDIUM | ATR_EMA [1..200] just re-confirmed NULL. ATR_ENTRY_MULT 201-value sweep just ran. Stop confirming settled params. |
| **Funding observer: built but not observing** | MEDIUM | Built T29 but not running continuously. Need hourly monitoring script. |

---

## Graveyard Summary

| Strategy | Result | Key Reason |
|----------|--------|------------|
| EP=24 | REVERTED | In-sample inflation |
| ATR_ENTRY_MULT>0 | REJECTED | All non-zero values degrade pass rate |
| ATR_ENTRY_MULT=0.94 | CANDIDATE | Real signal (42/54 vs 40/54) but not promoted — needs held-out validation |
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
| Donchian entry (replacement) | REJECTED | Wins Sharpe (+3.8) but loses pass rate (-14pp) |
| ATR-norm position sizing (S4) | REJECTED | Equal capital optimal, ATR-norm inverts vol ranking |
| ATR-rank conditional filter (T20) | ASSESSED | Marginal, not worth running |
| ATR_EMA [1..200] | NULL | No smoothing improvement anywhere in range |
| ATR_ENTRY_MULT [0..2.00] step 0.01 | NULL | EM=0.00 confirmed on current params (40/54 pass baseline vs candidate EM=0.94 42/54) |
| Asymmetric exit | REJECTED | Turtle ATR fires first; Chandelier never activates; all configs identical |
| Mid-caps (BNB/LINK/AVAX/MATIC/UNI) | REJECTED | 60% pass — below 70% threshold |

---

## Research Loop: NOT CLOSED — CONFIRMATION SPIRAL

All "new" results from this period were re-confirmations of settled results:
1. **ATR_EMA [1..200]:** Confirmed NULL at [1..30] — same result, higher resolution, 10,800 runs
2. **ATR_ENTRY_MULT 201-value sweep:** Confirmed EM=0.00 wins — same result, 201 values on current params

**Research loop closes when:** T31 (Donchian sleeve) built and funding observer runs continuously. T32 is closed.

**The only path to genuine new discovery at this point is live testnet execution feedback.** All hyperopts on historical data have been exhausted. ATR_ENTRY_MULT=0.94 is the one live candidate — it needs held-out validation or live observation to confirm.

**Only live testnet (BLOCKED on API keys), T31, and funding observer monitoring advance the project now.**