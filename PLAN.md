# PLAN.md — Krypto Research & Execution Plan

**State: 2026-04-30 04:05 UTC. T29 COMPLETE ✅. T31 BASE5 CANDIDATE ✅ (9-universe pending). T32 COMPLETE ✅. S6 PENDING (never built). EM=0.94 held-out validation PENDING. Live testnet CRITICAL BLOCKER (4+ weeks).**

---

## Brutal Self-Assessment (2026-04-30 Critique Cycle — Sixth Session)

**Research loop: CONFIRMATION SPIRAL — now in its 3rd consecutive session.**

Last 5 commits: 2/5 genuine new work (T31 Donchian sleeve), 3/5 hyperopt repeats + docs. HOLD_MAX [1..100] confirmed HM=12 again. ATR_EMA [1..200] confirmed NULL again. ATR_ENTRY_MULT 201-value sweep confirmed EM=0.00 again. These are settled results. Stop confirming them.

**What we got right:**
- T31 Donchian sleeve: BUILT on Base5 (6/6 pass, +22% Sharpe). Candidate, needs 9-universe validation.
- T32 Sharpe methodology: FIXED ✅
- Anti-overfit discipline held: EM=0.94 correctly not promoted
- Reports are now honest and trustworthy

**What we're still fooling ourselves about:**
- **"Research loop CLOSED" — premature.** ATR_EMA, ATR_ENTRY_MULT, HOLD_MAX all re-confirmed in this session alone. Same results, higher resolution. Not discovery.
- **2026 YTD -22.7% vs BTC +12.7% — "bear whipsaw" is not a root cause analysis.** It's a description, not an explanation. Is there a live-vs-backtest divergence? Quantify it.
- **Live bot dual-exit gap not verified.** Walk-forward validated dual Chandelier+Turtle ATR (93% pass). Live bot may use Turtle-only (67% pass). Gap of ~26pp not acknowledged.
- **EM=0.94 held-out validation: never built.** Candidate since 2026-04-29. Next step is held-out, not more grid sweeps.

---

## Production Params (FROZEN — all validated, do NOT re-sweep)

```
EP              = 21     // ✅ held-out confirmed
TURTLE_ATR_P    = 24     // ✅ fine sweep confirmed
TURTLE_ATR_M    = 2.0    // ✅ confirmed
CHAND_PERIOD    = 7      // ✅ 71-value dense sweep confirmed
CHAND_MULT      = 2.30   // ✅ 71-value dense sweep confirmed
ATR_ENTRY_MULT  = 0.00   // ✅ EM=0.94 CANDIDATE — needs held-out validation
HOLD_MAX        = 12     // ✅ confirmed [1..100] repeat sweep
POSITION_CAP    = 3      // ✅ confirmed
FRESHNESS_COOLDOWN = 0   // ✅ confirmed
VOL_LOOKBACK    = 8      // ✅ confirmed
ATR_EMA_PERIOD  = 1      // ✅ confirmed NULL [1..200]
```

---

## Next Tasks

### S6: Rebalancing Frequency / Winner-Loser Maintenance — NEVER BUILT
**Status:** UNTESTED. Listed since 2026-04-11. Never built. Genuinely novel.
**Hypothesis:** Current logic opens a position and waits for Chandelier/Turtle ATR exit. Hypothesis: periodic rebalancing (every N bars: re-rank open positions by unrealized PnL, trim or close worst performer if >2 bars in loss, let leaders run) may improve capital efficiency without suppressing trend convexity.
**Why it is worth testing:** Does not require API keys, does not require live data. Can be tested immediately on historical data.
**Reject if:** Increases turnover materially, collapses pass rate after fees.
**What to build:** `examples/rebalancing_sweep.rs` — sweep rebalance_interval ∈ {5, 10, 15, 21, 30, 42} bars, rebalance_type ∈ {trim_losers, close_losers, redistribute}. Run on Base5 × 6 windows. Compare against no-rebalancing baseline.

### T31: Donchian Sleeve 9-Universe Validation — PENDING
**Status:** Base5 candidate built (6/6 pass, +22% Sharpe vs Turtle). Needs 9-universe validation before production decision.
**Guardrail:** Reject if global pass rate drops >5pp (below 69.1%) or Sharpe improvement fails outside Base5.
**What to build:** `examples/donchian_sleeve_9universe.rs` — run 75/25 Turtle/Donchian sleeve on all 9 universes × 6 windows.

### EM=0.94 Held-Out Validation — PENDING
**Status:** CANDIDATE identified 2026-04-29 (42/54 pass, Sharpe 5.34 vs baseline 40/54/3.15). NOT promoted. Found on same WF grid.
**What to build:** `examples/atr_entry_mult_held_out.rs` — test EM=0.00 vs EM=0.94 on pre-2021 held-out data only. Pre-2021 data has NEVER been used to select EM=0.94. If EM=0.94 wins held-out → promote. If not → leave EM=0.00.
**Anti-overfit note:** This is NOT a grid sweep. It's a single held-out comparison. Must not re-run the 9×6 grid.

### T9: Live Testnet — CRITICAL BLOCKER
**Status:** BLOCKED on Noah's Binance testnet API keys for 4+ weeks.
**Everything else is secondary.** All metrics are upper bounds.
**What we need:** Binance testnet API key + secret (not production keys).

---

## Blind Spots

| Blind Spot | Severity | Status |
|-----------|----------|----------|
| **Live bot dual-exit gap** | HIGH | Walk-forward: dual Chandelier+Turtle ATR = 93% pass. Live bot: Turtle-only = 67% pass. ~26pp gap not verified or acknowledged. Verify live code path. |
| **2026 YTD no root cause** | HIGH | -22.7% Turtle vs +12.7% BTC = 35.4pp gap. "Bear whipsaw" is not an analysis. Is there a live-vs-backtest divergence? Quantify. |
| **S6 rebalancing: never built** | MEDIUM | Listed 2026-04-11. Never built. Genuinely novel, no keys needed. |
| **EM=0.94 held-out: never built** | MEDIUM | Candidate since 2026-04-29. Next step is held-out, not more grid sweeps. |
| **T31 9-universe validation: pending** | MEDIUM | Base5 candidate built. 9-universe needed for production decision. |
| **Research loop: confirmation spiral** | MEDIUM | ATR_EMA, ATR_ENTRY_MULT, HOLD_MAX all re-confirmed this session alone. Stop confirming settled params. |
| **Funding observer: not continuous** | LOW | T29 built but not running continuously. Need hourly cron. |
| **No live testnet** | CRITICAL | BLOCKED on Noah's API keys — 4+ weeks |

---

## Graveyard Summary (additions since last cycle)

| Strategy | Result | Key Reason |
|----------|--------|------------|
| ATR_EMA [1..200] | NULL | Re-confirmed NULL at [1..30]. 10,800 runs = spinning. |
| ATR_ENTRY_MULT 201-value sweep | NULL | Re-confirmed EM=0.00. 201 values = spinning. |
| HOLD_MAX [1..100] full sweep | Confirmed | HM=12 confirmed again. Repeat of 2026-04-21 sweep. |
| ATR_ENTRY_MULT=0.94 | CANDIDATE | Real signal but found on same WF grid — needs held-out validation |
| Donchian sleeve | BASE5 CANDIDATE | +22% Sharpe on Base5. Needs 9-universe validation before promotion. |

---

## Research Loop: CONFIRMATION SPIRAL — NOT CLOSED

The loop closes when we STOP hyperopts on settled params and START building:
1. S6 rebalancing harness (no keys needed, genuinely novel)
2. T31 9-universe Donchian sleeve validation
3. EM=0.94 held-out validation
4. Live bot dual-exit gap verification

**Only live testnet (BLOCKED on API keys), T31, S6, and EM=0.94 held-out advance the project.**
