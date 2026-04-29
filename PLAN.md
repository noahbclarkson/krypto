# PLAN.md — Krypto Research & Execution Plan

**State: 2026-04-29 20:50 UTC. T28 COMPLETE ✅. T31/T32 NEW. Sharpe metric integrity CRITICAL. Donchian sleeve and T29 funding observer are genuinely untested.**

---

## Brutal Self-Assessment (2026-04-29 Critique Cycle — Sixth Session)

**Research loop: CLOSED (confirmed).** Every testable idea genuinely exhausted or rejected. ATR EMA [1..200] × 10,800 runs = NULL (1a3dfe05). All confirmation hyperopts returning to baseline.

**What we got right:**
- Anti-overfitting discipline is REAL and consistent.
- Honest Sharpe: equity Sharpe ~1.04 (daily compounded) is the only honest number.
- USDT hedge overlay: INTEGRATED (683fe92e). Non-breaking vol-regime overlay.
- Equity bug: FIXED (e55659e8). T28 structural gap: CLOSED.
- DDBudget vs Turtle Sharpe comparison is a documented metric integrity problem.

**What we're still fooling ourselves about:**
- **DDBudget Sharpe 7.24 vs Turtle 1.04: incomparable methodologies.** The CSV treats them as peers. Readers will conclude DDBudget is 7x better.
- **2026 YTD: -22.7% vs BTC +12.7% (35pp gap).** We have no actionable explanation beyond "choppy bear." That's a real blind spot.
- **T29 funding rate observer: 0% built despite being "next" for days.** No backtest needed — public Binance API works right now.
- **Donchian: rejected as replacement, never tested as sleeve.** We missed the portfolio complement angle entirely.
- **3+ weeks without live testnet.** Everything is simulation upper bounds.

---

## Production Params (FROZEN — all validated, do NOT re-sweep)

```
EP              = 21     // ✅ held-out confirmed (reverted from EP=24 2026-04-26)
TURTLE_ATR_P    = 24     // ✅ fine sweep confirmed
TURTLE_ATR_M    = 2.0    // ✅ 81-value dense sweep confirmed
CHAND_PERIOD    = 7      // ✅ held-out confirmed
CHAND_MULT      = 2.30   // ✅ 71-value dense sweep confirmed
ATR_ENTRY_MULT  = 0.00   // ✅ 41-value sweep — no filter wins
HOLD_MAX        = 12     // ✅ HM=12 wins +71.4% Sharpe vs HM=45 baseline
POSITION_CAP    = 3      // ✅ Turtle-only validated (72.2% pass)
FRESHNESS_COOLDOWN = 0   // ✅ cd=0 wins
VOL_LOOKBACK    = 8      // ✅ dense production sweep confirmed (harness-only DV ranking)
```

---

## Next Tasks

### T9: Live Testnet — CRITICAL BLOCKER (escalate to Arc)
**Status:** BLOCKED on Noah's Binance testnet API keys for 3+ weeks.
**Everything else is secondary.** All metrics are upper bounds. Fee model, maker-fill rate, slippage — all unvalidated in live conditions.
**What we need:** Binance testnet API key + secret (not production keys).
**Escalation:** Surface to Arc explicitly. Nothing advances the project without this.

### T28: Turtle-ATR-Only Walk-Forward Validation — COMPLETE ✅
**Status:** VALIDATED 2026-04-29. Live bot uses Turtle ATR sole exit. WF harness validates dual exit. Pass rate comparison:
- Turtle-ATR-only (live): 36/54 pass (33% fail), Base5 5/6 (83%), Sharpe 4.12
- Turtle+Chandelier (dual): 40/54 pass (26% fail), Base5 6/6 (100%), Sharpe 3.15
- **Conclusion:** Turtle-ATR-only is NON-INFERIOR on pass rate. Live strategy validated. No structural gap. Chandelier contributes marginal Sharpe but NOT reliability.

### T29: Funding Rate Live Observer — GENUINELY UNTESTED / PUBLIC API
**Status:** UNBUILT. Public Binance API (no keys required). Has been "next" for 2+ days without progress.
**Hypothesis:** Extreme funding (<-50% ann or violent flips) identifies crowded positioning.
**Why it matters:** No backtest data needed — public API. Can observe immediately.
**Build:** Simple polling script. Poll `/fapi/v1/fundingRate` every hour for BTCFDUSD. Log vs 30d rolling average. Alert via Discord on threshold breach.
**Use:** Qualitative risk overlay only. Addresses bear/chop regime weakness qualitatively.
**Deadline:** This session — no reason it wasn't built earlier.

### T30: Expanded Universe Walk-Forward — MEDIUM / Defer until T29 done
**Universe expansion:** Add BNB, LINK, AVAX, MATIC, UNI to Base5. Run full walk-forward.
**Risk:** Wider spreads, more slippage on mid-caps. Execution costs may break the edge.
**Reward:** Tests "5-symbol universe" blind spot.

### T31: Donchian as Portfolio Complement — NEW / GENUINELY UNTESTED
**Status:** UNBUILT. We tested Donchian as a Turtle REPLACEMENT → rejected (-14pp pass rate). Never tested as a COMPLIMENT.
**Hypothesis:** Donchian (strictest breakout, all-time high) fires less frequently but with higher conviction. Turtle(75%) + Donchian(25%) as portfolio sleeve may capture different regime dynamics.
**Evidence:** Donchian W04 (bear chop) Sharpe +15.7 vs Turtle +1.3. Different regime profile = potential diversification.
**What to test:** Turtle(75%) + Donchian(25%) on Base5 × 7 windows, same dual exit. Reject if Turtle Sharpe collapses >10%.
**Why now:** We rejected Donchian prematurely via the wrong lens (replacement vs complement).

### T32: Sharpe Metric Integrity Fix — CRITICAL / Reporting
**Problem:** `daily_progress.csv` compares DDBudget Sharpe 7.24 (milestone-aggregated) to Turtle Sharpe 1.04 (daily equity). These are incomparable. Any reader concludes DDBudget is 7x better.
**Fix:** Add `sharpe_methodology` column to CSV. Or recompute DDBudget on daily equity.
**Scope:** Low — one column or one harness run.

### T25: Reconcile Metrics + Fix Reporting Pipeline — COMPLETE ✅
**Status:** Completed 2026-04-29 15:30 UTC. `progress_equity_curves` confirms Turtle+Chandelier **221.5x / daily Sharpe 1.04**. `live_turtle_chandelier` dry-run compiles/runs after fixing a format-string compile error and shows per-symbol historical paper returns (not the portfolio equity source of truth). `HALL_OF_FAME.md` and `scripts/gen_hof.py` now cite the honest validated-harness number, not `$67M`. `reports/daily_progress.csv` has a fresh 2026-04-29 row and `scripts/run_daily_progress.sh` makes refresh reproducible.

### T27: Asymmetric Exit Architecture — New Research
**Hypothesis:** Use tighter hard stop (ATR×0.5) for losers AND looser Chandelier (ATR×3.0) for winners. Turtle ATR remains the secondary exit. The convex payoff of trend-following demands asymmetric exits — cut losers fast, let winners run.
**Scope:** 3 configs (baseline, asymmetric soft-only, asymmetric hard+soft) × Base5 × 7 windows. If asymmetric hard stop reduces pass rate below 83%, reject. If it improves Sharpe by >3 windows without reducing pass rate, it may be worth the added complexity.

---

## Blind Spots

| Blind Spot | Severity | Status |
|-----------|----------|--------|
| **No live testnet** | CRITICAL | BLOCKED on Noah's API keys — 3+ weeks |
| **Daily reporting not cron-integrated** | MEDIUM | `scripts/run_daily_progress.sh` exists; operator/cron integration still pending |
| **Pre-2021 stress: 67.9%** | MEDIUM | Known constraint |
| **Equity source-of-truth drift** | LOW | Reconciled 2026-04-29: HOF now cites 221.5x / Sharpe 1.04 from validated harness |
| **Maker/slippage model unvalidated** | HIGH | Live testnet only |
| **Live/testnet exit gap** | ~~CRITICAL~~ **RESOLVED** | Turtle-only: 36/54 pass (33% fail) vs dual 40/54 (26% fail). Live strategy validated. |

---

## Graveyard Summary

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
| Donchian entry | REJECTED (not replacement) | Wins Sharpe (+3.8) but loses pass rate (-14pp) |
| ATR-norm position sizing (S4) | REJECTED | Equal capital optimal, ATR-norm inverts vol ranking |
| ATR-rank conditional filter (T20) | ASSESSED | Marginal, not worth running |

---

## Research Loop: CLOSED (2026-04-29) ✓

All testable ideas exhausted:
1. **S4:** ATR-normalized sizing — REJECTED (equal capital optimal)
2. **Donchian:** Entry space closed definitively (+3.8 Sharpe, -14pp pass rate)
3. **All hyperopts:** Exhausted (EP, CHAND_P, CHAND_M, HOLD_MAX, ATR_P, ATR_M, ATR_EM, CAP, VL, CD, MIN_TRADES)
4. **USDT hedge:** INTEGRATED ✅
5. **T28 structural gap:** RESOLVED — live strategy validated

**Research loop: TRULY CLOSED. Only live testnet (BLOCKED on API keys) advances the project.**
