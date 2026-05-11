# PLAN.md — Krypto Research and Execution Plan

**Updated: 2026-05-11 16:05 UTC — Critique Session #8**

---

## State: Research Coma Confirmed. Execution Path: Historical Replay (Zero Keys).

Research rate 12.5% (1/8 commits substantive). Anti-spin rules #11 and #20 both triggered. Research coma: 4/5 recent commits are critique/documentation. No code changes in multiple sessions.

**Primary execution blocker (API keys):** 6+ weeks. **Alternative path (historical replay):** proposed 08:05, NOT BUILT. This session's priority deliverable.

**Key critique findings:**
- Walk-forward Sharpe ~5-6 is a RESEARCH HARNESS metric (dual Chandelier+Turtle), NOT the live bot's daily-account Sharpe (1.02). Conflation persists.
- Live bot uses Turtle ATR-only exit. T12 pre-2021 held-out tested DUAL exit. TurtleATR-only has NEVER been validated against 2018-style bear data.
- Base5 100% pass is asset selection + strategy robustness. T80: UNI 1/6 fail, AVAX 4/6 fail. Generalization not clean.
- 2026 YTD: 4+ months of structural ATR_RANK T=5 gate failure (-3.2%). Mechanism unquantified.
- Research coma: doing critique sessions instead of building the alternative path (historical replay).

---

## 3 Most Important Execution Tasks (This Session)

### Priority 1: Historical Replay Mode — BUILD THIS SESSION (no API keys required)

**Problem:** 6+ weeks blocked on API keys. Historical replay mode proposed at 08:05 this session. ZERO lines of code written. This is the only execution path available right now.

**What it does:** Load Base5 parquet cache → run `LiveBot::process_bar` bar-by-bar → report daily equity, trade log, Sharpe, MaxDD, equity CSV. Validates production code path against real historical data. Catches bugs. Provides paper-trading proxy.

**Action:**
1. Fork `examples/live_bot_exact_equity.rs` for parquet-based replay instead of live API
2. Load cached parquet bars for Base5 symbols — no Binance connection needed
3. Loop through bars chronologically, call `bot.process_bar(bar)` per bar
4. Track: daily equity (mark-to-market), trade log, drawdown
5. Output: `snapshots/historical_replay.csv`, `snapshots/historical_replay_trades.csv`, summary metrics
6. Verify: output should match `live_bot_exact_equity.rs` exactly if logic is correct

**Time estimate:** 2-4 hours. This session.

### Priority 2: Turtle-Only Pre-2021 Held-Out Stress Test — Execute This Week

**Problem:** T12 held-out (pre-2021) confirmed 100% pass rate with DUAL Chandelier+Turtle exit. Live bot uses Turtle ATR-only ONLY. We've NEVER validated TurtleATR-only against 2018 bear data in isolation.

**Action:**
1. Fork `examples/live_bot_exact_equity.rs` → pre-2021 data only (2020 and earlier)
2. Run 6 walk-forward windows, 252 train + 252 test bars
3. Report pass rate, Sharpe, MaxDD, equity curve
4. If TurtleATR-only FAILS pre-2021: 2.76x is a bull-market artifact. Escalate to Arc immediately with explicit recommendation.
5. If it passes: genuine cross-regime validation confirmed.

**Time estimate:** 3-5 hours. Execute this week.

### Priority 3: Regime Non-Stationarity Quantification — Document the Mechanism (This Week)

**Problem:** ATR_RANK T=5 gate fails silently in 2026 (4+ months, -3.2% YTD). We know it's non-stationary. We don't know WHY. Mechanism is unquantified.

**Action:**
1. Cross-reference T=5 gate effectiveness vs BTC regime features (vol level, trend direction, time-of-year, correlation structure) across all available data
2. Use T95 top-10 mechanism analysis as starting point
3. Quantify which regime features co-vary with T=5 effectiveness
4. Document findings — honest uncertainty disclosure, NOT a fix attempt

**This is honest out-of-sample uncertainty disclosure.** We need to understand the failure mode before deciding if it's acceptable in production.

**Time estimate:** 2-3 hours.

---

## 3 Most Promising Unbuilt Ideas (Honest Assessment)

### 1. Historical Replay Mode (PRIORITY: HIGH — BUILD THIS SESSION)
No API keys. Uses existing parquet cache. Validates `src/live/bot.rs` code path. Provides paper-trading proxy. This is the only execution path available right now.

### 2. Turtle-Only Pre-2021 Held-Out Test (PRIORITY: HIGH — EXECUTE THIS WEEK)
Validates the live bot against genuine bear markets (2018 crash, 2019 chop). If TurtleATR-only fails pre-2021, the 2.76x production number is a bull-market artifact.

### 3. Regime Non-Stationarity Quantification (PRIORITY: MEDIUM — DOCUMENT ONLY)
ATR_RANK T=5 is non-stationary: works in some BTC eras, fails in 2026. T=24 and T=65 fail held-out. Document the mechanism: is it BTC vol level? BTC trend direction? Time-of-year? Honest uncertainty disclosure — not a fix attempt. T95 top-10 mechanism analysis is the starting point.

---

## Production Truth (Authoritative)

- **Exact as-coded live bot:** 2.76x / daily account Sharpe **1.02** / MaxDD **22.3%** / 286 trades / 1,800 days. Turtle ATR-only exit. **ONLY cite this as production.**
- **Progress harness (dual Chandelier+Turtle):** 621x — RESEARCH DIAGNOSTIC ONLY, different strategy.
- **Fee-adjusted Sharpe:** **[1.02–1.04]** — confirmed by T94, not the dominant risk.
- **Top-10 trade concentration:** **91%** of compounded log return. Structural fragility — not resolved but now documented (T95, a4b2614b).
- **2026 YTD underperformance:** **-3.2%** via ATR_RANK gate. Silent failure — not accepted, documented.
- **Walk-forward Sharpe 5-6:** per-window comparison metric only — 5x inflated vs daily account Sharpe 1.02. Not comparable.
- **Historical replay mode:** NOT YET BUILT — this session's primary deliverable.

---

## Anti-Overfitting Rules (Immutable, Updated 2026-05-11 16:05 UTC)

1. No Turtle-family parameter sweeps unless a new mechanism is proposed.
2. No "audit" tasks — write the test or close the issue.
3. Every task must have an execute-or-close decision. No "defer to next session."
4. **621x / 176.79x numbers are research diagnostics, NOT production performance.**
5. Daily account Sharpe only on equity charts. Per-window walk-forward Sharpe is not comparable.
6. Report fee-adjusted Sharpe as a range, not a point estimate.
7. **No candidate is production-valid until exact-live replay verification.**
8. Top-10 = 91% of log return. Any new filter must preserve the convex tail (T95 guardrail).
9. **Chandelier either fires or is removed. Non-binding exits are docs errors.**
10. **Universe selection is survivorship bias.** Always disclose which assets.
11. **Suspension animation is a real failure mode.** Escalate after one failed attempt, not five.
12. **If blocked on external dependency for 5+ weeks, need an explicit plan.**
13. **Maker-fill uncertainty is confirmed [1.02-1.04] — not dominant.**
14. **Progress harness 621x ≠ live bot 2.76x.** Do not conflate.
15. **2026 YTD underperformance: silent failure, document it.**
16. **M1 Discord: closed as task — owned by cron automation as monitoring layer.**
17. **Progress harness chart fix: CLOSED (8e650c73, 2026-05-11 08:01 UTC).**
18. **Top-10 winner mechanism: DONE (a4b2614b, 2026-05-11 12:10 UTC). T95 guardrail active.**
19. **Research rate below 50% for 3 consecutive sessions: escalate to Arc.**
20. **If next session has 0 commits with code changes (not docs/tracking), escalate to Arc.**
21. **Walk-forward Sharpe ~5-6 is a RESEARCH HARNESS metric, NOT the live bot's daily-account Sharpe (1.02). Do not cite it as production performance.**
22. **Base5 100% pass is asset selection + strategy robustness. T80 shows UNI 1/6 fail, AVAX 4/6 fail. Generalization not clean.**
23. **Historical replay mode is the execution path until API keys arrive. Build it.**
24. **If next session has 0 commits with code changes (not docs/tracking), escalate to Arc.**
25. **Research coma is real. When the real blocker can't be resolved, build the alternative path. Don't do more critique sessions.**
