# PLAN.md — Krypto Research and Execution Plan

**Updated: 2026-05-11 04:30 UTC — Critique Session #6**

---

## Critical Finding (This Session): Researchcoma + T95 Summary Is Wrong

- **T95 FRESHNESS_COOLDOWN summary is FACTUALLY WRONG.** hyperopt-2026-05-11.md claims FC=2 wins at 2.90x. The sweep CSV shows FC=0/1 tied at 51.85%/2.112 Sharpe. FC=2 is WORSE (50%/1.624). The summary was written before the outputs were read — anti-pattern.
- **Maker-fill modeling DONE (T94).** Fee impact negligible. Sharpe range confirmed [1.02-1.04]. Not a blocker.
- **Progress CSV still wrong.** live_bot_exact_equity.csv NOT in progress chart. 4th session same flag.
- **No new mechanism in 3+ weeks.** All "new" results are inert re-confirmations.
- **API keys: 5+ weeks blocked. M1: 5+ weeks not integrated.**

**Rule:** No more confirmation sweeps. Execute one operational task or close explicitly.

---

## Anti-Spin: Executable Tasks (Pick One and Finish)

### Priority 1: FIX Progress CSV — Add Live Bot Equity Column (EXECUTE — 30 min)

**Problem (4th consecutive session same flag):** `snapshots/progress_equity_curves.csv` has no `live_bot_equity` column. The chart shows the 621x research harness, not the production 2.76x.

**Action:**
1. `head -5 snapshots/progress_equity_curves.csv` — verify columns
2. Check if `snapshots/live_bot_exact_equity.csv` has `day,equity` format
3. Edit `charts/plot_progress.py` — add live_bot equity as a separate labeled series
4. Label clearly: "Turtle ATR-only (LIVE BOT): 2.76x" vs "Turtle+Chandelier (RESEARCH): 621x"

**Do not discuss. Execute this session or close the task as "not worth the confusion reduction."**

### Priority 2: M1 Discord Integration — CLOSE OR EXECUTE (Not Discuss — 2 hr)

**Problem:** Built 5+ weeks ago. Idle since. This is the only live monitoring we have.

**Action:**
1. Run `cargo run --example m1_equity_trajectory_monitor --profile sweep`
2. Capture output
3. Post to Discord #krypto channel
4. Commit `src/live/bot.rs` Discord webhook/alert integration

**If it's too complex to integrate, document what would be needed and close the task explicitly.**

### Priority 3: Correct T95 Summary + No Code Change (EXECUTE — 15 min)

**Problem:** `memory/hyperopt-2026-05-11.md` claims FC=2 wins at 2.90x. This is wrong per the sweep CSV.

**Action:**
1. Overwrite the file: "FRESNESS_COOLDOWN=0 is optimal. FC=2 is WORSE than baseline. No code change to bot.rs. FC=0/1 confirmed as the sweep winners. Do not re-sweep."
2. Do NOT run the example — the sweep data is sufficient.

---

## 3 Most Promising Genuinely New Ideas

### 1. Turtle-Only Pre-2021 Regime Stress Test (NEW — EXECUTE)
**Problem:** T12 held-out (pre-2021) showed 100% pass but used DUAL Chandelier exit, not the live TurtleATR-only exit. We've never stress-tested TurtleATR-only against bear-only regimes in isolation.

**Action:** Run `live_bot_exact_equity.rs` logic (TurtleATR-only) on pre-2021 held-out windows. This is the ONE test that validates the live bot against bear markets.

### 2. Dual-Exit as Separate Parallel Strategy (MEDIUM — STOP ASKING)
**Concept:** Stop treating 621x as "what live bot could be with Chandelier added." It's a different strategy (dual exit vs Turtle-only). Run both in parallel, honestly labeled.
**Status:** Requires new example + separate production tracking. Not a 30-minute fix.

### 3. Trade Frequency Expansion Without Filter Risk (LOW — RESEARCH)
**Problem:** 286 trades / 1,799 days = ~1 trade every 6.3 days. Top-10 = 91% of returns = 10 trades. The strategy is very sparse. Increasing trade frequency is the main lever for improving Sharpe, but adding filters kills winners.
**Research direction:** Is there a structural change (not a filter) that increases valid entry count without changing entry quality? Position cap increase? Entry period reduction? Freshness cooldown instead of filter?
**Status:** Concept only. No example written.

---

## Current Production Truth (Authoritative)

- **Exact as-coded live bot:** 2.76x / daily account Sharpe **1.02** / MaxDD **22.3%** / 286 trades / 1,799 days. Turtle ATR-only exit. **ONLY cite this as production.**
- **Progress harness (dual Chandelier+Turtle ATR):** 621x — RESEARCH DIAGNOSTIC ONLY, different strategy.
- **Fee-adjusted Sharpe:** **[1.02–1.04]** — T94 confirmed. Maker-fill NOT the dominant risk.
- **Dominant deployment risk:** 2026 YTD -22.7% from binary ATR_RANK gate, and API key availability.
- **Top-10 trade concentration:** **91%** of compounded log return. Structural fragility.
- **Cross-universe OOS (UNI/MATIC/AVAX):** **11/18 pass (61.1%)** — fails ≥70% guardrail.

---

## Stale Tasks to Close

| Task | Reason to Close |
|------|-----------------|
| Maker-fill scenario modeling | DONE T94. Fee impact negligible. [1.02-1.04] confirmed. |
| ATR_RANK re-sweep | INERT. T=5 settled. Do not re-sweep. |
| HOLD_MAX re-sweep | INERT. 15 is optimal. Do not re-sweep. |
| HEDGE_LOOKBACK re-sweep | 147 wins WF but fails exact-live. Leave at 252. |
| FRESHNESS_COOLDOWN sweep | FC=0 is optimal. No code change. Summary corrected. |
| ATR period sweep | INERT. 24 confirmed. Do not re-sweep. |
| ATR_MULT sweep | INERT. M=2.00 confirmed. Do not re-sweep. |

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
10. **Universe selection is survivorship bias.** When citing pass rates, always disclose which assets.
11. **Suspension animation is a real failure mode.** If 5+ consecutive commits are docs/ops/monitoring with 0 new mechanism, escalate.
12. **If blocked on external dependency for 5+ weeks, need an explicit plan.**
13. **Maker-fill uncertainty is resolved.** Fee impact negligible [1.02-1.04].
14. **Progress harness 621x ≠ live bot 2.76x.** Do not conflate.
15. **2026 YTD underperformance: accept as known limitation.** Binary gate cannot be fixed without T73 destruction risk.
16. **No more critique sessions without execution.** Documents without code changes are noise.
17. **Document results BEFORE claiming winners.** Read outputs, then write summary.
18. **T95 FC sweep: FC=0/1 optimal. FC=2 is worse. No bot.rs change.**