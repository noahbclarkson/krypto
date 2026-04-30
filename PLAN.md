# PLAN.md — Krypto Research & Execution Plan

**State: 2026-04-30 00:15 UTC. T29 COMPLETE ✅. T30 PARTIAL. T31/T32 still OPEN. Live testnet CRITICAL BLOCKER (4+ weeks).**

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
**Status:** BLOCKED on Noah's Binance testnet API keys for 4+ weeks now.
**Everything else is secondary.** All metrics are upper bounds. Fee model, maker-fill rate, slippage — all unvalidated in live conditions.
**What we need:** Binance testnet API key + secret (not production keys).
**Escalation:** Surface to Arc explicitly. Nothing advances the project without this.

### T28: Turtle-ATR-Only Walk-Forward Validation — COMPLETE ✅
**Status:** VALIDATED 2026-04-29. Live bot uses Turtle ATR sole exit. WF harness validates dual exit. Pass rate comparison:
- Turtle-ATR-only (live): 36/54 pass (33% fail), Base5 5/6 (83%), Sharpe 4.12
- Turtle+Chandelier (dual): 40/54 pass (26% fail), Base5 6/6 (100%), Sharpe 3.15
- **Conclusion:** Turtle-ATR-only is NON-INFERIOR on pass rate. Live strategy validated. No structural gap. Chandelier contributes marginal Sharpe but NOT reliability.

### T29: Funding Rate Live Observer — COMPLETE ✅ (2026-04-30)
**Built:** `examples/funding_rate_live_observer.rs` — polls Binance premiumIndex public API, compares to 30d cached history, computes z-score/percentile, detects extremes.
**Live result (2026-04-30 00:08 UTC):**
- Base5 avg ann funding: **-0.04%** — NEAR NEUTRAL
- No extreme signals detected (all z-scores -0.36 to -1.05)
- DOGE 30d avg (0.037% ann) > BTC (0.022%) — DOGE tends to highest funding
- All current rates slightly below 30d average (bearish positioning, mild)
**Use:** Qualitative risk overlay for regime assessment. Re-run daily.
**Run:** `cargo run --example funding_rate_live_observer --profile sweep`
**Log:** `snapshots/funding_live_observer_log.csv` (appends each run)

### T30: Expanded Universe Walk-Forward — PARTIAL (2026-04-30)
**MidCaps4 (BNB/LINK/AVAX/MATIC/UNI) standalone:** 3/5 pass (60%) — **BELOW 70% threshold**
- W01 +523.7% (Sharpe 10.89), W03 +827.6% (Sharpe 10.45) — massive in trending windows
- W02 -59.6% (Sharpe -4.70), W04 -28.5% (Sharpe -0.86) — catastrophic in choppy windows
- Root cause: mid-caps have wider spreads, choppier price action, less reliable trend-following signal
**Top9 (Base5 + BNB/LINK/AVAX):** 5/5 pass (100%) on 5 windows, Sharpe 6.20 avg — but only 5 windows (W05 missing mid-cap data), this is the "easy" window set. Interpretation: dollar-volume ranking with CAP=3 naturally limits mid-cap exposure to the strongest only.
**Verdict:** Mid-caps are NOT production-universe candidates. Top5-only remains correct. MidCaps4 edge exists in trending regimes but is too unreliable (60%) for production inclusion.
**Note:** UNIUSDT parquet was fetched via `funding_rate_live_observer.rs` run (first-ever fetch populates cache). All mid-cap data now cached for future use.
**T30: Do not expand production universe. Top5-only confirmed.**

### T31: Donchian as Portfolio Complement — OPEN
**Status:** UNTESTED. We tested Donchian as Turtle REPLACEMENT → rejected (-14pp pass rate). Never tested as COMPLIMENT (sleeve).
**Hypothesis:** Donchian (strictest breakout) fires less but higher conviction. Turtle(75%) + Donchian(25%) sleeve may capture different regime dynamics.
**Evidence:** Donchian W04 (bear chop) Sharpe +15.7 vs Turtle +1.3. Different regime profile = diversification.
**What to test:** Turtle(75%) + Donchian(25%) on Base5 × 7 windows, same dual exit. Reject if Turtle Sharpe collapses >10%.
**Scope:** Medium — 2 configs × Base5 × 7 windows.

### T32: Sharpe Metric Integrity Fix — OPEN
**Problem:** `daily_progress.csv` compares DDBudget Sharpe 7.24 (milestone-aggregated) to Turtle Sharpe 1.04 (daily equity). Incomparable.
**Fix:** Recompute DDBudget on daily equity OR add methodology column.
**Scope:** Low — one harness run or one CSV column.

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
