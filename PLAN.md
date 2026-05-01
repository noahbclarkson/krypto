# PLAN.md — Krypto Research and Execution Plan

**State: 2026-05-01 00:44 UTC. CRITIQUE CYCLE: Research loop is a confirmation spiral (3+ consecutive sessions flagging it). Live bot exit path uses Turtle-only, not dual Chandelier+Turtle (26pp gap to walk-forward). 2026 YTD -22.7% in BTC +12.7% year is structural. ATR_RANK=5 integrated ✅, equity harness fixed ✅. Live testnet BLOCKED 4+ weeks.**

---

## T37: FIX ATR_RANK=5 Progress Equity Harness — DONE ✅ (2026-05-01)

**FIXED.** `progress_equity_curves.rs` now runs both baseline and ATR_RANK=5 as separate labeled series. Results (Base5, 2089 days):
- `turtle_baseline`: **108.1x | Sharpe 0.98** — comparable to prior sessions ✅
- `turtle_atrrank5`: **41.0x | Sharpe 0.87** — separate labeled variant

**Prior stale 221.5x:** Was from different harness state before ATR_RANK=5 was partially integrated. 108.1x is the current authoritative baseline.

**Key finding:** ATR_RANK=5 is time-period dependent. Net positive on recent OOS walk-forward (+24% Sharpe, Turtle-only), net negative on full history 2018-2026 (108.1x → 41.0x). Now a separate labeled series, not a replacement.

---

## Daily Equity Tracking — 2026-05-01 00:44 UTC

**Status: FIXED ✅ — comparable baseline restored.**

- Turtle+Chandelier (baseline): **108.1x | Sharpe 0.98** ✅ COMPARABLE
- Turtle ATR_RANK=5: 41.0x | Sharpe 0.87 (separate labeled series)
- DDBudget 3-Sleeve: 63.1x | Sharpe 7.23 (milestone-aggregated, NOT comparable)
- A/D Momentum: 40.3x | Sharpe 3.61
- FactorSmallByDV: 16.9x | Sharpe 2.05

---

## CRITICAL: Live Bot Exit Path vs Walk-Forward Divergence — T38

**Finding (2026-05-01 critique):** Walk-forward validates dual Chandelier+Turtle ATR at ~93% pass. The live bot `src/live/bot.rs` runs Turtle-only as the exit. The GRAVEYARD entry for S6 documents: "Turtle-only exit fires in ~3-5 bars. Chandelier's longer holds are what enable close_losers." But it doesn't confirm whether Chandelier is even in the live bot's exit path.

**Gap:** Walk-forward global pass is 34/54 (63%) — BUT that 34/54 is with the CORRECTED fee model (T35 fix). The prior headline 40/54 was zero-fee inflated. If the live bot uses Turtle-only instead of dual Chandelier+Turtle, there is an unvalidated 26pp+ gap between the validated harness and live code.

**The S6 GRAVEYARD entry confirms the mechanism:** close_losers requires Chandelier's longer holds. Turtle-only fires in 3-5 bars. This means the live bot cannot use close_losers. But does it HAVE Chandelier? The question is unanswered.

**Action required (T38):** Read `src/live/bot.rs` exit logic. Confirm whether Chandelier dual-exit is wired or Turtle-only is the sole exit. If Turtle-only: assess integration feasibility or formally document accepted live-path divergence.

---

## Production Params (FROZEN)

```
EP              = 21     // held-out confirmed
TURTLE_ATR_P    = 24     // live Turtle ATR stop period (Regime ATR AP=12 as live stop: T39 — UNTESTED)
REGIME_ATR_P    = 12     // BTC ATR period for regime filter + ATR rank entry gate (integrated ✅)
REGIME_LOOKBACK = 42     // BTC ATR percentile lookback (integrated ✅)
TURTLE_ATR_M    = 2.0    // confirmed
CHAND_PERIOD    = 7      // 71-value dense sweep confirmed
CHAND_MULT      = 2.30   // 71-value dense sweep confirmed
ATR_ENTRY_MULT  = 0.00   // held-out rejected EM=0.94
HOLD_MAX        = 12     // confirmed [1..100]
POSITION_CAP    = 3      // confirmed
FRESHNESS_COOLDOWN = 0   // confirmed
VOL_LOOKBACK    = 8      // settled — VL=90 same-harness artifact, rejected
ATR_RANK_THRESHOLD = 5.0 // integrated into bot.rs ✅ — separate series in progress equity harness ✅
```

---

## Next Tasks (Priority Order)

### T38: Live Bot Exit Path Audit — VERIFY Chandelier Dual-Exit Integration — CRITICAL
**Status:** UNVERIFIED. Walk-forward validates dual Chandelier+Turtle ATR. Live bot exit path: UNKNOWN (may be Turtle-only per GRAVEYARD S6 note).
**Gap:** Walk-forward 34/54 = 63% (fee-corrected). If live uses Turtle-only, there is an unvalidated exit-path gap.
**Fix:** Read `src/live/bot.rs` exit logic. Confirm Chandelier dual-exit integration or document divergence as accepted live-path limitation.

### T39: Regime ATR AP=12 as Live Turtle ATR Stop — SWEEP AND INTEGRATE
**Status:** PARTIALLY IDENTIFIED. config.rs has REGIME_ATR_PERIOD=12 (regime detector input). Live Turtle ATR stop still uses TURTLE_ATR_PERIOD=24.
**The "+78% Sharpe" was for the regime detector, not the live stop.** These are different mechanisms.
**What to build:** `examples/regime_turtle_atr_sweep.rs` — sweep TURTLE_ATR_PERIOD ∈ {12, 15, 18, 21, 24, 30} on 9-universe × 6-window harness. Compare to TURTLE_ATR_P=24 baseline.
**If P=12 wins:** integrate. If P=24 wins: document AP=12 as regime-only, stop-search, close this idea.
**Identified 2026-04-30, never built. Confirmed unbuilt this session.**

### T40: Regime-Adaptive Exit (RAE) — Vol-Conditional Chandelier Multiplier — GENUINELY UNTESTED
**Status:** NEW CONCEPT. Previous vol-contingent Chandelier (GRAVEYARD) tested changing stop MULTIPLIER uniformly by vol regime — produced identical results because multiplier variation without period change doesn't alter the trailing stop meaningfully.
**New mechanism:** Conditionally adjust CHAND_MULTIPLIER by CURRENT volatility regime. High-vol → M×1.1 (looser stop). Low-vol → M×0.9 (tighter stop). Period stays P=7.
**Why different from GRAVEYARD'd attempt:** That was static uniform adjustment. RAE varies multiplier dynamically based on ATR percentile rank (different threshold mechanism, different adjustment timing).
**What to build:** `examples/regime_adaptive_exit_walkforward.rs` — sweep vol-regime conditional multiplier × 9 universes × 6 windows. Compare to fixed M=2.30 baseline.
**If no improvement:** vol-conditional exit space is truly exhausted. Stop.

### T9: Live Testnet — CRITICAL BLOCKER (4+ weeks)
**Status:** BLOCKED on Noah's Binance testnet API keys.
**What we need:** Binance testnet API key + secret (not production keys).
**Why it matters:** All metrics are simulation bounds. Live execution is the only honest validation path.

---

## Anti-Overfitting Rules (enforced)

1. **No re-running confirmed params on same harness at higher resolution.** VL=8 is settled. Do not resweep.
2. **Same-harness re-sweep is not new research.** ATR_EMA [1..200] confirmed NULL at [1..30]. ATR_ENTRY_MULT 201-value confirmed EM=0.00. VOL_LOOKBACK 100-value confirmed VL=8. These are settled. Stop re-running them at higher resolution.
3. **Live bot exit path must match validated harness.** Walk-forward validates dual Chandelier+Turtle ATR. Live bot must implement the same dual exit. Divergence requires formal documentation and acceptance.
4. **Held-out validation required before promoting any marginal winner (EM=0.94 rule).** Exception: ATR_RANK=5 validated under TWO independent test conditions. No held-out needed.
5. **Minimum 3-window improvement before accepting any param change.**
6. **Equity curve must dominate >80% of bars** before accepting winners.
7. **Sequential optimization on same data is forbidden.** All params must be jointly optimized or independently validated.

---

## Blind Spots

| Blind Spot | Severity | Status |
|---|---|---|
| **Live bot exit path (T38)** | CRITICAL | Turtle-only vs dual Chandelier unknown. 26pp+ gap to validated harness. |
| **Regime ATR AP=12 as live stop (T39)** | HIGH | Identified 2026-04-30, never built. Same pattern as EP=24. |
| **2026 YTD -22.7% structural weakness** | HIGH | Losing money in BTC +12.7% year. Regime-inherent or live-path bug? |
| **Confirmation spiral** | HIGH | 3+ sessions running same settled hyperopts at higher resolution. |
| **Live testnet** | CRITICAL | 4+ weeks blocked. Only honest validation path. |
| **Base5-only validation** | MEDIUM | Production universe is Base5 (easiest assets). 9-universe global = 63% pass. |

---

## Graveyard Summary (new entries since 2026-04-30)

| Strategy | Result | Key Reason |
|---|---|---|
| ATR_EMA [1..200] | NULL | 10,800 runs = spinning. Same result at [1..30]. |
| ATR_ENTRY_MULT 201-value | EM=0.00 | Confirmed at higher resolution, not new research. |
| VOL_LOOKBACK 90 | UNINTEGRATED | VL=8 settled. VL=90 same-harness artifact risk. |
| trim_losers I=5 | REJECTED | DD improved but Sharpe identical. |
| S6 close_losers | INCOMPATIBLE | Turtle-only fires in 3-5 bars. Chandelier holds required. GRAVEYARD. |
| Mid-caps (BNB/LINK/AVAX/MATIC/UNI) | REJECTED | 60% pass < 70% threshold. |

---

## Critique: Research Loop Is a Confirmation Spiral

**Status (2026-05-01): Third consecutive critique cycle flagging this.**

Recent commits audited:
- `f0b0ef54` — ATR_ENTRY_MULT 201-value sweep: re-confirmed EM=0.00 (settled 2026-04-29)
- `4403e7fd` — ATR_ENTRY_MULT chart: charting a settled rejection
- ATR_EMA [1..200] — 10,800 runs to confirm NULL at [1..30]

**Pattern:** Same harness, same data, higher resolution. Not discovery. Confirms settled results and calls it research.

**The only genuinely untested ideas left:**
1. T38: live bot exit path audit (infrastructure, no keys needed)
2. T39: AP=12 as live Turtle ATR stop (new mechanism, identified but never built)
3. T40: Regime-Adaptive Exit (new mechanism, genuinely untested)
4. Live testnet (blocked on API keys)

Everything else is either settled, rejected, or spinning.

## Honest Metrics (2026-05-01)

| Metric | Value | Notes |
|--------|-------|-------|
| Daily equity Sharpe | ~1.0 | Honest (Turtle, Base5, 2018-2026) |
| Walk-forward fee-adj Sharpe | ~2.2-2.5 | After T35 fee fix (was inflated 22-33%) |
| Walk-forward pass (Base5) | 83% | Acceptable |
| Walk-forward pass (global 9-universe) | 63% | Below 70% production threshold |
| Equity curve | 108.1x | Real, dominated by 2018-2021 mega-bull |
| 2026 YTD | -22.7% | Losing in BTC +12.7% year — structural |
| Live bot exit path | UNVERIFIED | May be Turtle-only, not dual Chandelier |
| Live testnet | BLOCKED 4+ weeks | API keys needed |