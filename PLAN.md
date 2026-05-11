# PLAN.md — Krypto Research and Execution Plan

**Updated: 2026-05-11 00:05 UTC — Critique Session #6**

---

## Critical Finding: Chart Conflation + Structural Fragility

This session's brutal assessment: the 2.76x live bot equity is built on a convex structure where **top-10 trades = 91% of compounded log return**. We can describe the winners (low-vol regime entry, SOL/DOGE, low dollar-volume rank) but cannot explain the mechanism. This is our biggest production risk — not fees, not parameter tuning, not API keys.

The **progress equity chart STILL conflates two different strategies** (live bot 2.76x vs research harness 621x). The fix has been deferred 3 sessions.

**M1 Discord integration is closed** as a "task" — it is now owned by the cron automation as a permanent monitoring layer. Not a task to do, a service that should always be running.

---

## 3 Most Important Execution Tasks (This Session)

### Priority 1: FIX Progress Harness Chart — 3rd and Final Deferral

**Problem:** `snapshots/progress_equity_curves.csv` shows `turtle_equity` from the dual Chandelier+Turtle research harness (621x) as the Turtle line. The live bot (2.76x, Turtle ATR-only) is NOT in the CSV. Noah sees 621x and thinks it's the live bot.

**Action (30 minutes, no API keys):**
1. Check `snapshots/live_bot_exact_equity.csv` has `day` + `equity` columns
2. Edit `charts/plot_progress.py` to add a 5th line: `live_bot_equity` from `live_bot_exact_equity.csv` — labelled "Turtle ATR-only (LIVE BOT): 2.76x"
3. Regenerate PNG → commit

**Rule:** If this fix is not done this session, explicitly close it as "not happening" and remove from all future plans. Three deferrals is a pattern, not a backlog.

---

### Priority 2: Document Top-10 Winner Mechanism

**Problem:** We know top-10 = 91% of log return. We know the winners entered in low-vol BTC regimes (SOL 2023-01-11, DOGE 2022-10-28) at low dollar-volume rank. We do NOT know why these conditions produce large winners. Understanding the mechanism is required before any new filter can be added.

**Action:**
1. Run `examples/t73_top_winner_conditions.md` analysis again with new context — cross-reference each top-10 winner's entry conditions against the broader distribution of all 286 trades
2. Document: (a) what was different about these 10 entries vs the median trade, (b) whether exit speed (bars held) correlates with winner size, (c) whether position size or symbol selection explains the tail
3. Write findings to `memory/top10_mechanism_2026-05-11.md`

**This is a data analysis task, not a code change.** The goal is a written explanation of the convex structure, not a fix.

---

### Priority 3: Escalate Testnet Blocker to Arc — With Explicit Contingency

**Problem:** 5+ weeks blocked on Binance testnet API keys. All remaining questions (fee model, slippage, regime adaptation, 2026 underperformance) can only be answered with live execution. We are doing simulation work that is exhausting its useful output.

**Message to Arc:**
> "Krypto research simulation is approaching terminal state. Maker-fill confirmed [1.02-1.04] — not dominant. All Turtle params settled. Top-10 = 91% of return documented. Remaining open questions (2026 YTD -22.7%, 2025-2026 OOS regime validity, real fee/slippage) require live testnet execution. API key blocker is 5+ weeks old. Request: (1) confirm ETA for testnet keys, OR (2) authorize alternative path (dry-run mode with historical data replay as proxy for live execution), OR (3) explicitly decide to pause cron sessions until keys arrive. Anti-spin rule #11 escalation."

---

## 3 Most Promising Unbuilt Ideas (Honest Assessment)

### 1. Top-10 Winner Mechanism Documentation (PRIORITY: HIGH — EXECUTE THIS SESSION)
Not a new strategy. A written explanation of WHY the convex tail exists. Required before any filter can be added without destroying the right tail. SOL 2023-01-11 and DOGE 2022-10-28 are the largest contributors — understand them before adding any new entry condition.

### 2. Binance Testnet Activation (PRIORITY: HIGH — BLOCKED ON ARC ESCALATION)
All remaining questions are execution questions, not simulation questions. If keys arrive within 2 weeks: activate testnet, run live bot on testnet, compare equity against exact-live replay. If keys don't arrive: document the decision to pause research until keys are available.

### 3. Regime Non-Stationarity Quantification (PRIORITY: MEDIUM — DOCUMENT ONLY)
ATR_RANK T=5 is non-stationary — works in some BTC eras, fails in others. T=24 and T=65 fail held-out. We know the gate is regime-dependent. Document the mechanism: is it BTC vol level? BTC trend direction? Time-of-year? Correlation structure? Quantify which regime features co-vary with T=5's effectiveness. This is honest out-of-sample uncertainty disclosure, not a fix.

---

## Current Production Truth (Authoritative)

- **Exact as-coded live bot:** 2.76x / daily account Sharpe **1.03** / MaxDD **22.3%** / 286 trades / 1,796 days. Turtle ATR-only exit. **ONLY cite this as production.**
- **Progress harness (dual Chandelier+Turtle):** 621x — RESEARCH DIAGNOSTIC ONLY, different strategy.
- **Maker-fill uncertainty:** fee-adjusted Sharpe **[1.02-1.04]** — confirmed by T94, not the dominant risk.
- **Top-10 trade concentration:** **91%** of compounded log return. Structural fragility — NOT resolved.
- **2026 YTD underperformance:** **-22.7%** via binary ATR_RANK gate. Silent failure. Not accepted, documented.
- **Walk-forward Sharpe 5-6:** per-window comparison metric only — 5x inflated vs daily account Sharpe 1.03. Not comparable.

---

## Anti-Overfitting Rules (Immutable)

1. No Turtle-family parameter sweeps unless a new mechanism is proposed.
2. No "audit" tasks — write the test or close the issue.
3. Every task must have an execute-or-close decision. No "defer to next session."
4. **621x / 176.79x numbers are research diagnostics, NOT production performance.**
5. Daily account Sharpe only on equity charts. Per-window walk-forward Sharpe is not comparable.
6. Report fee-adjusted Sharpe as a range, not a point estimate.
7. **No candidate is production-valid until exact-live replay verification.**
8. Top-10 = 91% of log return. Any new filter must preserve the convex tail.
9. **Chandelier either fires or is removed. Non-binding exits are docs errors.**
10. **Universe selection is survivorship bias.** Always disclose which assets.
11. **Suspension animation is a real failure mode.** Escalate after one failed attempt, not five.
12. **If blocked on external dependency for 5+ weeks, need an explicit plan.**
13. **Maker-fill uncertainty is confirmed [1.02-1.04] — not dominant.**
14. **Progress harness 621x ≠ live bot 2.76x.** Do not conflate.
15. **2026 YTD underperformance: silent failure, document it.**
16. **M1 Discord: closed as task — owned by cron automation as monitoring layer.**
17. **Progress harness chart fix: 3rd deferral — execute or close explicitly.**
18. **Top-10 winner mechanism: understand before adding any new filter.**
19. **Research rate below 50% for 3 consecutive sessions: escalate to Arc.**
20. **If next session has 0 commits with code changes (not docs/tracking), escalate to Arc.**