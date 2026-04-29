# PLAN.md — Krypto Research & Execution Plan

**State: 2026-04-29 15:30 UTC. T25 COMPLETE. Equity bug FIXED. Reporting pipeline REPRODUCIBLE. HOF metric reconciled to 221.5x / Sharpe 1.04. USDT hedge INTEGRATED. Research loop CLOSED. Live testnet CRITICAL BLOCKER (3+ weeks).**

---

## Brutal Self-Assessment (2026-04-29 Critique Cycle — Fourth Session)

**Research loop: CLOSED.** S4 tested and REJECTED (ATR normalization fails — equal capital optimal, 86% vs 57% pass). Every testable idea genuinely exhausted. Entry, exit, position sizing — all validated or rejected.

**What we got right:**
- Anti-overfitting discipline is REAL and consistent. EP=24, ATR_ENTRY_MULT=0.85, EP=43 all correctly rejected for same-session in-sample inflation.
- Honest Sharpe distinction: equity Sharpe ~1.29 (compounded daily returns, honest) vs walk-forward Sharpe 5.46 (per-window averaged, upper bound). Never report 5.46 on equity charts.
- Research loop genuinely closed. S4 (ATR-norm sizing) REJECTED. Donchian definitively closes entry space.
- USDT hedge overlay: INTEGRATED (2026-04-29, commit 683fe92e). Non-breaking bear-risk overlay.
- Equity bug: FIXED (e55659e8). Off-by-one recording loop corrected.

**What we're still fooling ourselves about:**
- Live testnet: 3+ week blocker. All metrics are still simulation upper bounds.
- Pre-2021 stress: 67.9% — BELOW our own 70% threshold. Choppy/bear regimes remain the real failure mode.
- Reporting is better but not fully automated by cron. `scripts/run_daily_progress.sh` now makes the run reproducible, but cron integration is still manual/operator work.

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

**Research loop: TRULY CLOSED. Only live testnet (BLOCKED on API keys) advances the project.**
