# PLAN.md — Krypto Research and Execution Plan

**Updated: 2026-05-10 16:05 UTC — Critique Session #4**

---

## Critical Finding (This Session): Three Documentation Loops, Zero Execution

- T89 (ATR period): 3 confirmation sweeps, all IDENTICAL. INERT.
- T92 (ATR_RANK threshold): Swept again even though T=5 was settled T65.
- Two-Systems Problem: Documented 3 sessions. Harness still runs wrong strategy.
- M1: Monitor built, but direct WhatsApp→Discord path is broken (cron delivery workaround is acceptable but not a fix).
- Progress chart: Still shows 621x equity as headline. Label changed but data still from wrong harness.

**Rule:** No more critique sessions. The next session must execute or close items.

---

## Anti-Spin: Executable Tasks (Not Documents)

### Priority 1: FIX Progress Harness — Run Live Bot Equity and Update CSV (EXECUTE)

**Problem (STILL NOT FIXED after 3 sessions):** The progress chart equity CSV (`snapshots/progress_equity_curves.csv`) is fed by the research dual-exit harness (621x). The live bot exact equity (2.76x) is not in the chart.

**Action:**
1. `cargo run --example live_bot_exact_equity --profile sweep 2>&1` — get live bot daily equity series
2. Check if `snapshots/live_bot_exact_equity_equity.csv` (or equivalent) exists
3. If not, create `examples/export_live_bot_equity.rs` that outputs daily equity CSV named `live_bot_exact_equity_daily.csv`
4. Update `examples/progress_equity_curves.rs` to ADD a 5th column: `live_bot_equity` = exact-live Turtle ATR-only path
5. Update chart script `plot_progress.py` to include live bot equity as a separate line (2.76x = production truth)
6. Label: "Turtle ATR-only (LIVE BOT): 2.76x" vs "Turtle+Chandelier dual exit (RESEARCH): 621x"

**This is a 30-minute fix.** Do not discuss. Execute.

### Priority 2: ESCALATE to Arc — Anti-Spin Rule #11 (EXECUTE)

**Rule mandate:** "If 5+ consecutive commits are docs/ops/monitoring with 0 new research, escalate."

**Current state:** 12 consecutive commits (2 days): 2 research, 10 ops/docs. Rule #11 triggered MULTIPLE times without escalation.

**Message to Arc:**
> "Krypto project is in documentation loop. 12 consecutive commits: 10 ops/docs, 2 marginal hyperopts. Anti-spin rule #11 has been triggered without escalation. Dominant blocker: Noah's Binance testnet API keys (5+ weeks). Maker-fill Sharpe range [0.6–1.3] is unconstrained. We need either (a) API keys urgently, or (b) explicit decision to stop waiting and pivot to a different path forward. What's the contingency?"

### Priority 3: Maker-Fill Scenario Analysis (EXECUTE — can do without API keys)

**Problem:** Fee-adjusted Sharpe range [0.6–1.3] is our biggest unknown. We can model it better without live data.

**Action:**
1. Run `live_bot_exact_equity` with different fee assumptions:
   - 0% maker (pure taker 0.08%): what equity/Sharpe do we get?
   - 50% maker (realistic 0.04% effective): what do we get?
   - 70% maker (optimistic 0.028% effective): what do we get?
2. Document each scenario in a new `snapshots/maker_fill_scenario_analysis.csv`
3. This constrains the range [0.6–1.3] with actual numbers instead of estimates
4. Post summary to Discord: "Maker-fill sensitivity: X% maker → Sharpe Y"

---

## 3 Most Promising Unbuilt Ideas (Honest Assessment)

### 1. Maker-Fill Scenario Modeling (PRIORITY: HIGH — EXECUTE THIS SESSION)
Run exact-live equity under 3 fee assumptions (0%, 50%, 70% maker fill). Constrain the [0.6–1.3] range with actual data. This is the most valuable thing we can do without API keys.

### 2. Dual-Exit Live Bot Experiment (PRIORITY: MEDIUM — POST-KEYS)
Concept: Add Chandelier(7,2.30) dual-exit to live bot. The dual-exit research harness gets 621x vs live bot's 2.76x. Hypothesis: dual-exit might improve equity profile.
T73 risk: Chandelier fires faster and could cut winners. Needs T73-style top-winner preservation audit before any test.
**Do not execute without API keys.** This is a live testnet experiment.

### 3. Cross-Universe Generalization Documentation (PRIORITY: LOW — ACCEPT LIMITS)
UNI fails 1/6 (Sharpe -0.81). AVAX marginal 4/6 (Sharpe 0.13). MATIC 6/6 but data ends 2024-09-10.
Honest statement: strategy is universe-sensitive. Works on trending high-beta crypto. Fix is universe selection, not parameter patching. Document this as a known limitation. Do not attempt to "fix" UNI via parameter changes.

---

## Current Production Truth (Authoritative)

- **Exact as-coded live bot:** 2.76x / daily account Sharpe **1.02** / MaxDD **22.3%** / 286 trades / 1,799 days. Turtle ATR-only exit. **ONLY cite this as production.**
- **Progress harness (dual Chandelier+Turtle ATR):** 621x — RESEARCH DIAGNOSTIC ONLY, different strategy.
- **Maker-fill uncertainty:** Sharpe **[0.6–1.3]** — UNCONSTRAINED.
- **Walk-forward per-window Sharpe:** 5-6 — RESEARCH harness only, not comparable to account Sharpe.
- **Top-10 trade concentration:** **91%** of compounded log return. Structural fragility.
- **2026 YTD underperformance:** **-22.7%** via binary ATR_RANK gate. Accepted limitation.

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
13. **Maker-fill uncertainty is the dominant deployment risk.** It belongs on every status report.
14. **Progress harness 621x ≠ live bot 2.76x.** Do not conflate.
15. **2026 YTD underperformance: accept as known limitation.**
16. **No more critique sessions without execution.** Documents without code changes are noise.
17. **Anti-spin rule #11 escalation is now mandatory, not optional.**
