# Strategy Ideas — Krypto Research Log

*Last updated: 2026-04-30 04:05 UTC. T29 COMPLETE ✅. T31 BASE5 CANDIDATE ✅ (9-universe pending). T32 COMPLETE ✅. S6 NEVER BUILT. EM=0.94 held-out pending. Research loop is CONFIRMATION SPIRAL — stop hyperopts on settled params.*

---

## Critical New Insight: Entry Space Is a Trade-Off, Not a New Edge

**Donchian result (T19, 2026-04-28) changes the picture:**

- Donchian: avg Sharpe +9.4, pass rate 86% (-14pp vs Turtle)
- Turtle: avg Sharpe +5.6, pass rate 100%
- **Entry alternatives trade pass rate for per-trade Sharpe quality.** They don't add new edge — they filter signals.

This pattern matches every failed entry approach:
- ATR_MULT (fixed threshold): pass rate degrades monotonically as threshold increases
- Volume confirmation: pass rate degrades 6-13pp
- Correlation filter: loses to baseline on every metric

**Implication:** Entry space is definitively closed. Turtle entry is the optimal trade-off between signal frequency and signal quality.

---

## Critical New Insight: Research Loop Is a Confirmation Spiral

**As of 2026-04-30:** We keep re-running settled parameters at higher resolution and calling it new research.

- ATR_EMA [1..200] × 9u × 54w = 10,800 runs — re-confirmed NULL at [1..30] on the same harness. Same result, higher resolution. Not discovery.
- ATR_ENTRY_MULT 201-value sweep (0.00..=2.00 step 0.01) — re-confirmed EM=0.00 on current params. Same result, 201 values instead of 41. Not discovery.
- ATR_ENTRY_MULT=0.94 candidate: real signal (42/54 pass, Sharpe 5.34 vs baseline 40/54/3.15) — correctly not promoted (anti-overfit discipline). But this was found on the same WF grid it would be validated against.

**The research loop is NOT closed. It's spinning.** T32 is now fixed; the loop closes when we stop running hyperopts and build T31 plus funding observer continuous monitoring.

---

## Critical New Insight: USDT Hedge Overlay — ✅ Integrated

**Documented 2026-04-11, integrated 2026-04-29 (commit 683fe92e).**
- Trigger: BTC 21d vol > 75th percentile of 252-bar history → reduce position 30%, hold 30% in USDT
- Effect: ~30% DD reduction in bear windows (historical validation)
- Mechanism: modest position size overlay, non-breaking, optional
- **This directly addresses the pre-2021 stress weakness (67.9% below 70% threshold).**
**Status:** Built in `src/live/bot.rs` lines 245-275. Needs live/testnet observation, not more historical tuning.

---

## Top Genuinely Untested Ideas (Priority Order)

### T29: Funding Rate Live Observer — ✅ COMPLETE (2026-04-30 00:08 UTC)
**Built:** `examples/funding_rate_live_observer.rs` — polls Binance premiumIndex public API (no keys), compares to 30d cached history, computes z-score/percentile, detects extremes.
**Live result:** NEAR NEUTRAL (-0.04% avg ann funding). No extremes detected. All z-scores -0.36 to -1.05. DOGE tends highest funding (0.037% ann vs BTC 0.022%).
**Status:** COMPLETE. Next: continuous monitoring via `scripts/run_funding_observer.sh`.

### T31: Donchian as Portfolio Complement — BASE5 CANDIDATE ✅ (9-universe pending)
**Status:** BUILT on Base5 (2026-04-30 commit 4a4e15ba). 9-universe validation still pending.
**Result (Base5 × 6 windows):**
- Turtle(75%) + Donchian(25%): **6/6 pass, Sharpe +4.767 (+22.0% vs Turtle)**, +2284% avg return
- Guardrail: reject if global pass rate drops >5pp (below 69.1%) or Sharpe fails outside Base5
**What to build:** `examples/donchian_sleeve_9universe.rs` — 9-universe × 6-window validation

### T32: Sharpe Metric Integrity Fix — COMPLETE ✅ (2026-04-30)
**Problem fixed:** `daily_progress.csv` no longer silently compares DDBudget 7.24 to Turtle 1.04 as peer Sharpe values.
**Built:** Added `sharpe_methodology` to the report and updated `scripts/run_daily_progress.sh` so refreshes preserve methodology. `progress_equity_curves.rs` generated markdown now labels DDBudget as milestone-aggregated/not comparable to Turtle daily compounded equity.
**Current interpretation:** Turtle+Chandelier = 221.1x / 1.04 `daily_compounded_equity`; DDBudget = 61.3x / 7.24 `milestone_aggregated_not_comparable`. Do not cite the DDBudget 7.24 as a superior peer Sharpe.

### S6: Rebalancing Frequency / Winner-Loser Maintenance — UNTESTED (TOP PRIORITY)
**Status:** Listed since 2026-04-11. **NEVER BUILT.** Genuinely novel — no hyperopt loop has touched it. No API keys needed.
**Hypothesis:** Current Turtle+Chandelier opens and waits for exit. Hypothesis: periodic rebalancing (every N bars: re-rank open positions by unrealized PnL, trim/close worst if in loss >N bars, let leaders run) may improve capital efficiency without suppressing trend convexity.
**Why it is worth testing:** Addresses "trapped capital in decaying breakouts" as a position lifecycle problem, not an entry problem. Mechanistically different from all failed scaling overlays.
**What to build:** `examples/rebalancing_sweep.rs` — sweep rebalance_interval ∈ {5, 10, 15, 21, 30, 42} bars, rebalance_type ∈ {trim_losers, close_losers, redistribute}. Base5 × 6 windows. Compare to no-rebalancing baseline.
**Reject if:** Increases turnover materially or collapses pass rate >5pp after fees.

---

## ATR_EMA [1..200] Confirmation — NULL (2026-04-29)

**10,800 runs** (200 values × 9 universes × 54 windows). ATR_EMA=4 wins pass rate (43/54 vs 42/54 baseline) but loses -0.36 Sharpe (3.76 vs 4.12). ATR_EMA=1 (raw ATR) confirmed as production default by robustness-first criteria.

**Prior:** [1..30] sweep on stale params — NULL. This sweep confirms it extends to [1..200].

**Not discovery.** Re-confirmation of settled result.

---

## ATR_ENTRY_MULT 201-Value Sweep (2026-04-29) — CONFIRMED NULL, CANDIDATE FOUND

**Scope:** ATR_ENTRY_MULT ∈ [0.00..=2.00] step 0.01 (201 values) × 9 universes × 6 windows = 54 windows/value.
**Params:** CHAND(7,2.30)/EP=21/HM=12/CAP=3/VL=8/ATR(24,2.0).

**Winner (baseline):** EM=0.00 — 40/54 pass (74.1%), Sharpe 3.15, return +105.3%, DD 35.4%, 721 trades.
**Candidate:** EM=0.94 — 42/54 pass (77.8%), Sharpe 5.34 (+2.19), return +73.0%, DD 28.3%, 486 trades.
**Highest Sharpe:** EM=1.07 — 41/54 pass, Sharpe 7.86 (inflated by mega-bull windows).

**Decision:** No production change. EM=0.94 found on same WF grid it would be validated against. Anti-overfit discipline requires held-out data before promotion. EM=0.00 remains production default.

**Status:** Candidate identified, not promoted. Requires held-out validation on pre-2021 data only. **Build `examples/atr_entry_mult_held_out.rs` — single pre-2021 comparison, NOT another grid sweep.**

---

## Equity Bug Fix — ✅ FIXED (2026-04-29, commit e55659e8)

**Root cause:** Off-by-one forward-fill in `progress_equity_curves.rs`. While loop exits before last exit is recorded.
**Fix:** Record equity at bar=exit_bar before incrementing. One targeted edit.
**Status:** FIXED. Off-by-one recording loop corrected.

---

## Dual-Exit Attribution — ✅ COMPLETED (2026-04-28)

**Result:** Chandelier fires first ~7-8% of windows, not >90% as feared. TURTLE_ATR_PERIOD=24 is a REAL parameter.
- Dual-exit: 40/54 pass (global), 6/6 Base5
- Turtle-only: 36/54 pass (global), 5/6 Base5 (W04 bear chop fails)
- Chandelier adds 1 window of robustness — secondary exit, not primary driver

**⚠️ Live Bot Dual-Exit Gap — UNVERIFIED:** MEMORY states live bot uses Turtle-only exit (sole exit). Walk-forward validated dual Chandelier+Turtle ATR at 93% pass. If live is Turtle-only, the gap is ~26pp pass rate. **Action: Verify `src/live/bot.rs` exit logic. If Turtle-only, assess dual-exit implementation feasibility or accept the gap.**

## New Concept: 2026 YTD Root Cause Analysis

MEMORY per-year table: Turtle +2026 YTD = **-22.7%** while BTC = **+12.7%**. Gap = 35.4pp underperformance.

"Bear whipsaw" is a description, not a root cause. Mechanism: sustained downtrend + low vol = Turtle breaks out → Chandelier stops out → repeated whipsaw losses. This is a genuine structural weakness in choppy/bear regimes.

**Key question:** Is this **regime-inherent** (strategy working as designed in a hostile market) or is there a **live-vs-backtest divergence** (bug in live path)?

**What to do:** Compare live bot equity curve vs backtest equity curve on the same 2026 YTD period. If they diverge → real bug. If they match → regime-inherent, accept it.

---

## Live Testnet Blocker

**Status: CRITICAL BLOCKER — 4+ weeks without live testnet.**

The entire project is simulation. All metrics are upper bounds.

**Only genuine path forward:** Live testnet paper trading. All hyperopts on historical data exhausted. ATR_ENTRY_MULT=0.94 is the one live candidate. Everything else is locked.

---

## Dead Strategies — Confirmed Graveyard

| Strategy | Test Date | Result | Key Reason |
|----------|-----------|--------|------------|
| BollingerReversion | 2026-04-11 | 0/288 OOS | Signal actively harmful vs random |
| BOCPD regime detector | 2026-04-11 | 0% breaks | NIG model too insensitive |
| FDUSD basis carry | 2026-04-10 | 19% pass | Structural premium, autocorrelation 0.88 |
| Funding rate MR | 2026-04-10 | 43% pass | Highly autocorrelated |
| Vol-contingent Chandelier | 2026-04-12 | GRAVEYARD | All configs identical |
| ATR entry filter (fixed mult) | 2026-04-13 + 04-25 | mult=0.0 wins | Any non-zero filter hurts |
| ATR_EMA [1..200] | 2026-04-29 | NULL | No smoothing improvement anywhere in range |
| ATR_ENTRY_MULT [0..2.00] | 2026-04-29 | EM=0.00 wins | EM=0.94 candidate — needs held-out validation |
| Chop filter | 2026-04-13 | REJECTED | Trade-starving |
| Correlation entry filter (T7) | 2026-04-25 | REJECTED | All 3 variants lose to baseline |
| CTREND + Chandelier exit | 2026-04-20 | 30/54 pass | Wrong exit mechanism for CTREND |
| CTREND regime-conditional switching | 2026-04-25 | 67% pass — FAIL | 67% < 70% threshold |
| 4h Multi-Timeframe Turtle | 2026-04-25 | 1/20 pass | Structural — dual exit collapses on 4h |
| Cross-market equity integration | 2026-04-16 | REJECTED | Combined -2.94 Sharpe vs crypto-only |
| DynamicTrend EMA signal | 2026-04-16 | REJECTED | Turtle wins 21/24 windows |
| A/D Static Sleeve | 2026-04-14 | 46% | Below-random win rate |
| BTC Trend Scalar | 2026-04-14 | 0/8 configs | Baseline wins |
| Regime-conditional allocation | 2026-04-12 | 60.5% | Worse than either component alone |
| XRP 4h MR | 2026-04-11 | 0/4 | Edge destroyed by fees |
| EP=24 | 2026-04-26 | REVERTED | In-sample inflation on same OOS data |
| EP=43 | 2026-04-27 | REVERTED | Found same session as EP=21 validation |
| ATR_ENTRY_MULT=0.85 | 2026-04-25 | REVERTED | In-sample inflation |
| Position scaling overlays | various | GRAVEYARD | All failed — Chandelier already handles it |
| Donchian entry (replacement) | 2026-04-28 | REJECTED | Wins Sharpe (+3.8) but loses pass rate (-14pp) — NOT as replacement; complement untested |
| ATR-norm position sizing (S4) | 2026-04-29 | REJECTED | Equal capital optimal, ATR-norm inverts vol ranking |
| Asymmetric exit | 2026-04-29 | REJECTED | Turtle ATR fires first; all configs identical |
| Mid-caps (BNB/LINK/AVAX/MATIC/UNI) | 2026-04-30 | REJECTED | 60% pass — below 70% threshold |

**Conclusion:** The reliable crypto edge is directional trend-following on daily data. Entry space is a pass-rate vs Sharpe trade-off — Turtle is the optimal point. Remaining untested ideas (T31, S6) require building, not more hyperopts.

---

## Anti-Overfitting Rules (Established 2026-04-25)

1. **Minimum win margin:** ≥3 windows (5.5%) improvement on OOS before accepting any param change
2. **No sequential optimization on same data:** If EP is optimized on data D, you cannot also optimize ATR_EM on data D and claim both are valid
3. **Held-out validation required for marginal wins:** 1-2 window delta = noise until pre-2021 stress confirms
4. **Equity curve dominance:** Winner must dominate baseline at >80% of time bars
5. **Never re-run confirmed params:** ATR_PERIOD confirmed 3×. CHAND_MULT confirmed 2×. P=7 confirmed 2×. ATR_EMA confirmed 2×. Stop.

---

## Key Insight: All Metrics Are Upper Bounds

Everything in HALL_OF_FAME.md is a simulation maximum. The real validation path is live testnet paper trading + comparing actual vs predicted metrics.

**Source of truth for production params: `src/live/config.rs`. Last verified: 2026-04-29.**