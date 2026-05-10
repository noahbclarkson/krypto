# Strategy Ideas — Krypto Research Log

*Last updated: 2026-05-10 08:05 UTC.* Critique cycle #2. CRITICAL FINDING: progress harness (621x) uses dual Chandelier+Turtle exit — DIFFERENT STRATEGY from live bot (2.76x, Turtle ATR-only). These are not the same strategy measured differently. M1 integration still 5+ weeks overdue — final call.

---

## Critical Alerts

### 🚨 THE TWO SYSTEMS PROBLEM (NEW 2026-05-10) — CRITICAL
**Progress harness ≠ live bot.** The progress harness (`progress_equity_curves.rs`) uses Chandelier(7,2.30)+TurtleATR DUAL EXIT → 621x/1.19 Sharpe. The live bot (`src/live/bot.rs`) uses Turtle ATR ONLY → 2.76x/1.02 Sharpe. These are two DIFFERENT strategies with different exit mechanisms. The "621x PRODUCTION CANDIDATE" label is WRONG.

**Rule:** Only 2.76x / Sharpe 1.02 is the production headline. 621x is research diagnostic ONLY.

### 🚨 M1 Discord Integration: FINAL CALL
Status: 5+ weeks overdue. This is the last "highest priority" mention.
This session: execute or close explicitly. No more plan entries about it.

### ⚠️ 2026 YTD Underperformance: Accepted as Known Limitation
ATR_RANK T=5 gate is binary — skips entries in low-vol regimes but positions stay FULL size.
No fix attempted: any fix risks T73 top-winner destruction (SOL 2023-01-11 was low-vol at entry).
Decision: document and accept.

### ⚠️ Suspension Animation: Structural, Not Temporary
Last 8 commits: 2 research + 6 overhead. Research rate: 25% and falling.
Anti-spin rule #11 TRIGGERED again. Next cron must execute OR close tasks.

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

### 1. M1 Discord Integration (PRIORITY: EXECUTE — NOT DISCUSS)
Status: 5+ weeks overdue. Execute now. Run monitor → post to #krypto → commit.

### 2. Maker-Fill Hypothesis Testing (PRIORITY: MEDIUM — BLOCKED)
Concept: One week of dry-run execution to constrain the [0.6–1.3] Sharpe range.
Why it matters: Dominant deployment risk. Even rough confirmation of 70% fill rate changes risk model materially.
Status: Blocked by API keys.

### 3. Dual-Exit Live Bot Experiment (PRIORITY: LOW — NEW 2026-05-10)
Concept: Progress harness uses Chandelier(7,2.30)+TurtleATR dual exit (621x). Live bot uses Turtle ATR-only (2.76x). These are different strategies. Adding Chandelier as secondary exit to live bot might improve equity profile.
Risk (T73): Chandelier fires faster and could cut winners that drive 91% of log returns. Needs T73-style top-winner audit before any test.
Status: Concept only. Requires top-winner preservation audit first.

---

## Sharpe Taxonomy

| Type | Value | Notes |
|------|-------|-------|
| Live bot daily compounded (Turtle ATR-only) | **1.02** | Authoritative production number |
| Progress harness (dual Chandelier+Turtle ATR) | 1.19 | RESEARCH — different exit |
| Per-window walk-forward | ~5-6 | RESEARCH harness only; 5x inflated vs account |
| Fee-adjusted range (30–80% maker fill) | **0.6–1.3** | Estimated; point estimate prohibited |

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

**Only blocker:** Noah's Binance testnet API keys (5+ weeks blocked).

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
13. **2026 YTD underperformance is accepted limitation.** Binary gate cannot be fixed without T73 destruction risk.
