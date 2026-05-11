# Strategy Ideas — Krypto Research Log

*Last updated: 2026-05-11 08:05 UTC. Research loop closed. Execution phase. Three new execution tasks: Turtle-only pre-2021 held-out test, top-10 mechanism documentation, historical replay mode (no API keys).

---

## Critical Alerts

### 🚨 THE TWO SYSTEMS PROBLEM (NEW 2026-05-10) — CRITICAL
**Progress harness ≠ live bot.** The progress harness (`progress_equity_curves.rs`) uses Chandelier(7,2.30)+TurtleATR DUAL EXIT → 621x/1.19 Sharpe. The live bot (`src/live/bot.rs`) uses Turtle ATR ONLY → 2.76x/1.02 Sharpe. These are two DIFFERENT strategies with different exit mechanisms. The "621x PRODUCTION CANDIDATE" label is WRONG.

**Rule:** Only 2.76x / Sharpe 1.02 is the production headline. 621x is research diagnostic ONLY.

### 🚨 Chart Conflation: 3rd Deferral — Execute or Close
**Problem:** `progress_equity_curves.csv` shows research harness (621x) as the "turtle_equity" line. Live bot (2.76x) is NOT in the chart. This has been deferred 3 sessions.
**Decision:** Execute Priority 1 this session or explicitly close and remove from all future plans. No 4th deferral.

### ⚠️ 2026 YTD Underperformance: Silent Failure, Not Accepted
ATR_RANK T=5 gate is binary — skips entries in low-vol regimes but positions stay FULL size. YTD performance -22.7% via the gate. This is NOT "accepted limitation" language — it is an active, documented structural failure. The gate works in some eras and fails in others (T=24 and T=65 both fail held-out). We cannot fix it without destroying the top-10 tail (SOL 2023-01-11 was low-vol at entry). The honest statement: the strategy has a non-stationary entry gate that we cannot stabilize without deeper regime understanding.

### ⚠️ Top-10 Winner Mechanism: UNEXPLAINED CONVEX STRUCTURE
**New critical alert (2026-05-11).**
Top-10 trades = 91% of compounded log return. We can list the conditions (low-vol BTC regime, SOL/DOGE entries, low dollar-volume rank) but CANNOT explain the mechanism. Two largest winners: SOL 2023-01-11 and DOGE 2022-10-28. Both entered in low-vol BTC Q1 environments. The strategy is "a few large directional bets on high-beta crypto in ugly BTC regimes" — not "a robust trend-following system."
**Implication:** Any new entry filter must prove it preserves the convex tail. We cannot explain the tail, so we cannot confidently protect it. This is the dominant production risk.

### T80: OOS Universe Validation — GENERALIZATION FAILURE
- **11/18 pass (61.1%)**, avg Sharpe **0.149**, avg return **+2.3%/window**, 160 trades
- MATIC strong (6/6), AVAX borderline (4/6), UNI catastrophic (1/6)
- **Fails promotion guardrail** (≥70% pass + Sharpe ≥0.5): 9pp below pass, 0.35 below Sharpe

---

## All C-Items: CLOSED

| Item | Status | Notes |
|------|--------|-------|
| C16 | CLOSED PERMANENTLY | CHAND_PERIOD sweep proved all 98 values identical; non-binding |
| C17 | NEVER BUILT | code was never written; permanently unbuilt |
| C18 | ACCEPTABLE — CLOSED | equity 2.808x at 40% fill; low sensitivity confirmed |
| C19 | GRAVEYARD | harness passed (6/6), exact-live failed (2.74x vs 2.89x) |

---

## Operational Infrastructure (Not Research)

### M1: Equity Trajectory Monitor — Integrate into Discord
**Status:** Built (`examples/m1_equity_trajectory_monitor.rs`) but idle — not integrated into Discord alerting.
Priority: HIGH. Final call — execute this session or close explicitly.

---

## 3 Most Promising Unbuilt Ideas

### 1. Top-10 Winner Mechanism Analysis (PRIORITY: HIGH — EXECUTE)
Not a strategy. A written explanation of WHY the convex tail exists. Cross-reference each top-10 winner's entry conditions against all 286 trades. Understand: (a) position size vs symbol selection as winner driver, (b) exit speed (bars held) vs winner size correlation, (c) what differentiated these 10 entries from the median trade. This is required before any new entry filter can be safely added.

### 2. Regime Non-Stationarity Quantification (PRIORITY: MEDIUM — DOCUMENT ONLY)
ATR_RANK T=5 is non-stationary: works in some BTC eras, fails in others. T=24 and T=65 both fail held-out. Document the mechanism: is it BTC vol level? BTC trend direction? Time-of-year? Correlation structure? Quantify which regime features co-vary with T=5's effectiveness. Honest out-of-sample uncertainty disclosure — not a fix attempt.

### 3. Dual-Exit Live Bot Experiment (PRIORITY: LOW)
Adding Chandelier(7,2.30) dual-exit to live bot vs Turtle ATR-only. These are DIFFERENT strategies (621x vs 2.76x). T73 risk: Chandelier fires faster and could cut winners that drive 91% of log returns. Requires top-winner preservation audit before any test. Low priority — the two strategies are already well-characterized as separate.

---

## Sharpe Taxonomy

| Type | Value | Notes |
|------|-------|-------|
| Live bot daily compounded (Turtle ATR-only) | **1.02** | Authoritative production number |
| Progress harness (dual Chandelier+Turtle ATR) | 1.19 | RESEARCH — different exit |
| Per-window walk-forward | ~5-6 | RESEARCH harness only; 5x inflated vs account |
| Fee-adjusted (maker-fill confirmed, T94) | **1.02–1.04** | Confirmed range; not the dominant risk |

Report fee-adjusted Sharpe as a range, not a point estimate.

---

## Production Parameters (Frozen)

```
TURTLE_EP=21, TURTLE_ATR_PERIOD=24, TURTLE_ATR_MULT=2.00, ATR_ENTRY_MULT=0.00,
HOLD_MAX=15, POSITION_CAP=3, FRESHNESS_COOLDOWN=0,
REGIME_ATR_PERIOD=17, REGIME_LOOKBACK=41, ATR_RANK_THRESHOLD=5.0,
VOL_LOOKBACK=92 (configured; unused by bot.rs after T72),
HEDGE_ATR_PERIOD=38, HEDGE_LOOKBACK=252, HEDGE_ATR_PCT=0.45, HEDGE_SIZE_MULT=0.25,
fee_pct=0.000400
```

Exit: Turtle ATR(24,2.0) trailing stop ONLY. Chandelier is stored for compatibility but does not fire (T34 KNOWN GAP).

---

## Honest Deployment Statement

**What we have:** Turtle ATR trend-following on daily crypto bars. Turtle ATR-only exit (live bot). Real, modest edge (2.76x / Sharpe 1.02). Walk-forward validated on Base5 (100% pass, 6/6 windows). Edge concentrated in high-beta trending crypto pairs.

**What we don't have:** Cross-universe generalization (UNI 1/6 fail). Live monitoring (M1 idle). Real execution feedback. Dual-exit performance (that is a different strategy — the 621x is not "the same strategy with better exits," it's a different strategy).

**Only blocker:** Noah's Binance testnet API keys (5+ weeks blocked). All remaining questions are execution questions, not simulation questions.

---

## Anti-Overfitting Rules

1. No Turtle-family parameter sweeps unless a new mechanism is proposed.
2. No "audit" tasks — write the test or close the issue.
3. Every task must have an execute-or-close decision. No "defer to next session."
4. The 621x / 176.79x numbers appear in progress charts as research diagnostics, NOT production performance.
5. Daily account Sharpe only on equity charts. Per-window walk-forward Sharpe is not comparable.
6. Report fee-adjusted Sharpe as a range (maker fill uncertain), not a point estimate.
7. **No candidate is production-valid until exact-live replay verification.**
8. Top-10 = 91% of log return. Any new filter must preserve the convex tail.
9. **Chandelier either fires or is removed. Non-binding exits are docs errors, not valid strategy complexity.**
10. **Universe selection is survivorship bias.** When citing pass rates, always disclose which assets.
11. **Suspension animation is a real failure mode.** If 5+ consecutive commits are docs/ops/monitoring with 0 alpha, escalate.
12. **Progress harness 621x ≠ live bot 2.76x.** These are different strategies with different exits. Do not conflate them.
13. **2026 YTD underperformance: silent failure.** Not "accepted limitation" — an active documented structural failure.

---

## New: Historical Replay Mode (NO API KEYS REQUIRED)

**Problem:** 5+ weeks blocked on API keys. Alternative path never tried.

**Concept:** Run `src/live/bot.rs` against historical cached parquet bars in bar-by-bar replay mode. Uses existing data cache. No Binance API required.

**Value:**
- Validates the live bot code path against real historical data
- Catches bugs before deployment
- Paper-trading proxy without live keys
- Should produce identical output to `live_bot_exact_equity.rs` if logic is correct

**Status:** Unbuilt. Proposed 2026-05-11. Priority 3 in PLAN.
