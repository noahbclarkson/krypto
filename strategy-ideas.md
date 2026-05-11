# Strategy Ideas — Krypto Research Log

*Last updated: 2026-05-11 16:05 UTC. Research coma confirmed (12.5% research rate). Research loop CLOSED. Execution phase: historical replay mode (no API keys). Walk-forward Sharpe conflation alert. TurtleATR-only never validated vs 2018 bear.*

---

## Critical Alerts

### 🚨 Research Loop CLOSED — Execution Phase WITHOUT Keys
All testable ideas exhausted. Historical replay mode is the execution path until API keys arrive. Build it this session. 6+ weeks blocked on Binance testnet API keys.

### 🚨 THE TWO SYSTEMS PROBLEM — CLOSED (2026-05-11 08:01 UTC)
Chart now shows 2.76x live bot vs 621x research harness honestly. Rule: only 2.76x / Sharpe 1.02 is production. 621x is research diagnostic ONLY.

### ⚠️ 2026 YTD Underperformance: Silent Failure (-3.2%)
ATR_RANK T=5 gate failing silently all year. Not "accepted limitation" — active structural failure. Non-stationary gate mechanism partially understood (T95) but not fixed.

### ⚠️ Walk-Forward Sharpe Conflation Alert (NEW 16:05 UTC)
Per-window walk-forward Sharpe ~5-6 is a RESEARCH HARNESS metric (dual Chandelier+Turtle). The live bot's daily-account Sharpe is **1.02**. These are NOT the same metric. Do not cite ~5-6 Sharpe as production performance — it describes a different strategy.

### ⚠️ TurtleATR-Only Never Validated Against 2018 Bear (NEW 16:05 UTC)
T12 held-out (pre-2021) validated DUAL exit (Chandelier+Turtle). Live bot uses Turtle ATR-only ONLY. TurtleATR-only has NEVER been validated against 2018-style bear data in isolation. This is the highest-priority validation gap.

### ⚠️ 91% Convex Tail — Mechanism Partially Understood (T95 DONE)
Top-10 = 91% of log return. T95 analysis (a4b2614b, 2026-05-11 12:10 UTC): 8/10 entered after BTC drawdowns >10%/21d. Low-vol Q1 BTC regimes are accidental winner environments, not designed. T73/T95 guardrail: any new filter must preserve the top-10 tail.

### ⚠️ Chandelier Dual-Exit Live Bot Experiment: GRAVEYARD
Two different strategies (621x vs 2.76x), not exit optimization. Chandelier fires faster and risks cutting the top-10 winners. Not testing.

### T80: OOS Universe Validation — GENERALIZATION FAILURE
- **11/18 pass (61.1%)**, avg Sharpe **0.149**, avg return **+2.3%/window**, 160 trades
- MATIC strong (6/6), AVAX borderline (4/6), UNI catastrophic (1/6)
- **Fails promotion guardrail** (≥70% pass + Sharpe ≥0.5)

---

## All C-Items: CLOSED

| Item | Status | Notes |
|------|--------|-------|
| C16 | CLOSED PERMANENTLY | CHAND_PERIOD sweep proved all 98 values identical; non-binding |
| C17 | NEVER BUILT | code was never written; permanently unbuilt |
| C18 | ACCEPTABLE — CLOSED | equity 2.808x at 40% fill; low sensitivity confirmed |
| C19 | GRAVEYARD | harness passed (6/6), exact-live failed (2.74x vs 2.89x) |

---

## Operational Infrastructure: CLOSED

### M1: Equity Trajectory Monitor
**Status:** Closed as task — owned by cron automation as permanent monitoring layer.

---

## 3 Most Promising Unbuilt Ideas

### 1. Historical Replay Mode (PRIORITY: HIGH — BUILD THIS SESSION)
**No API keys required.** Uses existing parquet cache. Validates `src/live/bot.rs` code path. Provides paper-trading proxy. Proposed 08:05 this session — NOT YET BUILT.

**Concept:** Load Base5 parquet files → run `LiveBot::process_bar` bar-by-bar → report daily equity, trade log, Sharpe, MaxDD. Output should match `live_bot_exact_equity.rs` exactly if logic is correct.

### 2. Turtle-Only Pre-2021 Held-Out Stress Test (PRIORITY: HIGH — EXECUTE THIS WEEK)
T12 held-out (pre-2021) confirmed 100% pass rate with DUAL Chandelier+Turtle exit. Live bot uses Turtle ATR-only. We've never validated TurtleATR-only against bear-only pre-2021 regimes in isolation.

**If TurtleATR-only fails pre-2021:** 2.76x is a bull-market artifact — escalate to Arc immediately.
**If it passes:** genuine cross-regime validation confirmed.

### 3. Regime Non-Stationarity Quantification (PRIORITY: MEDIUM — DOCUMENT ONLY)
ATR_RANK T=5 is non-stationary: works in some BTC eras, fails in 2026. T=24 and T=65 fail held-out. Document the mechanism: BTC vol level? BTC trend direction? Time-of-year? Correlation structure? T95 top-10 mechanism analysis is the starting point.

---

## Sharpe Taxonomy

| Type | Value | Notes |
|------|-------|-------|
| Live bot daily compounded (Turtle ATR-only) | **1.02** | Authoritative production number |
| Progress harness (dual Chandelier+Turtle ATR) | 1.19 | RESEARCH — different exit |
| Per-window walk-forward | ~5-6 | RESEARCH harness only; 5x inflated vs account |
| Fee-adjusted (maker-fill confirmed, T94) | **1.02–1.04** | Confirmed range; not the dominant risk |

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

**What we have:** Turtle ATR trend-following on daily crypto bars. Turtle ATR-only exit (live bot). Real, modest edge (2.76x / Sharpe 1.02). Walk-forward validated on Base5 (100% pass, 6/6 windows). Edge concentrated in high-beta trending crypto pairs. 91% of return from 10 trades.

**What we don't have:** Cross-universe generalization (UNI 1/6 fail). Live execution feedback. ATR_RANK gate is non-stationary (fails 2026 silently). Survivorship bias in Base5 selection.

**Only blocker:** Noah's Binance testnet API keys (6+ weeks). Historical replay mode is the alternative validation path.

---

## Anti-Overfitting Rules

1. No Turtle-family parameter sweeps unless a new mechanism is proposed.
2. No "audit" tasks — write the test or close the issue.
3. Every task must have an execute-or-close decision. No "defer to next session."
4. **621x / 176.79x numbers are research diagnostics, NOT production performance.**
5. Daily account Sharpe only on equity charts. Per-window walk-forward Sharpe is not comparable.
6. Report fee-adjusted Sharpe as a range, not a point estimate.
7. **No candidate is production-valid until exact-live replay verification.**
8. Top-10 = 91% of log return. Any new filter must preserve the convex tail (T73/T95 guardrail).
9. **Chandelier either fires or is removed. Non-binding exits are docs errors.**
10. **Universe selection is survivorship bias.** Always disclose which assets.
11. **Suspension animation is a real failure mode.** Escalate after one failed attempt, not five.
12. **If blocked on external dependency for 5+ weeks, need an explicit plan.**
13. **Maker-fill uncertainty is confirmed [1.02-1.04] — not dominant.**
14. **Progress harness 621x ≠ live bot 2.76x.** Do not conflate.
15. **2026 YTD underperformance: silent failure, document it.**
16. **M1 Discord: closed as task — owned by cron automation.**
17. **Progress harness chart fix: CLOSED (8e650c73, 2026-05-11 08:01 UTC).**
18. **Top-10 winner mechanism: DONE (a4b2614b, 2026-05-11 12:10 UTC). T95 guardrail active.**
19. **Research rate below 50% for 3 consecutive sessions: escalate to Arc.**
20. **If next session has 0 commits with code changes (not docs/tracking), escalate to Arc.**
21. **Historical replay mode: BUILD THIS SESSION. No more waiting for API keys.**
22. **Chandelier dual-exit live bot experiment: GRAVEYARD — not testing.**

---

## Anti-Spin: What We're NOT Doing

- No new parameter sweeps (research loop closed)
- No more critique-only documentation loops
- No Chandelier dual-exit live bot experiment
- No escalation to Arc without explicit contingency + alternative path already executing
- No waiting for API keys as excuse for zero execution