# PLAN.md — Krypto Research and Execution Plan

**Updated: 2026-05-11 08:05 UTC — Critique Session #6**

---

## Critical Finding (This Session): The Research Loop Is Closed — Execute, Don't Research

The research loop IS structurally exhausted. Every parameter has been swept. Every filter rejected. T73 guardrail established. The problem is not missing strategy — it's that we can't execute the one we have.

**New finding:** We have zero live execution feedback. M1 equity monitor is idle after 5+ weeks. API keys are 5+ weeks blocked. The historical replay infrastructure (no keys needed) has never been built. Stop waiting. Build the replay.

---

## Critical Alert: 91% Convex Tail Mechanism UNEXPLAINED

Top-10 trades = 91% of compounded log return. SOL 2023-01-11 and DOGE 2022-10-28 (our two largest winners) entered in LOW-VOL BTC regimes — exactly the regime the ATR_RANK gate is supposed to filter. They slipped through by luck, not design.

**Rule:** Any new entry filter must prove it preserves these 2 specific trades. We can't protect what we don't understand. Top-10 mechanism documentation is a prerequisite for any future filter work.

---

## Top-3 Execution Tasks (This Session)

### Priority 1: Turtle-Only Pre-2021 Held-Out Regime Test (EXECUTE — 2-3 hr)

**Problem:** T12 held-out (pre-2021) confirmed 100% pass BUT used dual Chandelier exit. The live bot uses Turtle ATR-only only. We have never stress-tested TurtleATR-only against bear-only regimes in isolation. If it fails pre-2021, the 2.76x production number is a bull-market artifact.

**Action:**
1. Fork `examples/live_bot_exact_equity.rs`
2. Filter to pre-2021 windows only (bar dates before 2021-01-01)
3. Run the exact live bot path (Turtle ATR-only, current config)
4. Report: pass rate, Sharpe, MaxDD, trade count per window
5. Compare to T12 Chandelier dual-exit results on same windows

**Execute this session or explicitly close and document why.**

---

### Priority 2: Top-10 Winner Mechanism Documentation (EXECUTE — 2-3 hr)

**Problem:** 91% of log return from 10 trades. We know WHO they are (from t73_top_winner_conditions.csv) but not WHY they worked. Cannot safely add any new entry filter without understanding the tail.

**Action:**
1. Read `snapshots/t73_top_winner_conditions.csv` — all 10 winners
2. For each winner: BTC regime at entry, position size (slot 1/2/3), symbol, entry date, bars held, exit type
3. Cross-reference against all 286 trades: what differentiated these 10?
4. Document: (a) position size vs symbol selection as winner driver, (b) exit speed vs winner size correlation, (c) regime conditions at entry vs median trade
5. Write `snapshots/t73_top_winner_mechanism.md`

**Execute this session or explicitly close. No more deferrals.**

---

### Priority 3: Historical Replay Mode — No API Keys Required (EXECUTE — 3-4 hr)

**Problem:** 5+ weeks blocked on API keys. Alternative path never tried: run `src/live/bot.rs` against historical cached parquet bars in replay mode. Validates the live code path, catches bugs before deployment, provides paper-trading proxy.

**Action:**
1. Write `examples/historical_replay.rs`
2. Load Base5 parquet cache (already downloaded) — bar-by-bar
3. Run `LiveBot::process_bar` loop over full history
4. Report: daily equity, trade log, Sharpe, MaxDD
5. Compare to `live_bot_exact_equity.rs` output — should match identically if logic is correct
6. This is the "live testnet proxy" — if the code path works in replay, it's likely correct for live

**Execute this session. No API keys needed — uses existing data cache.**

---

## M1: Explicit Close-or-Execute (NOT DEFER — CLOSE THIS SESSION)

Built 5+ weeks ago. Confirmed working. Idle since. Either:
- **Execute:** Integrate M1 equity monitor output into Discord webhook ping — send a chart update to #krypto now
- **Close:** Remove from all future plans. Stop deferring.

Pick one. No more PLAN entries for M1 after this session.

---

## Anti-Spin: No More Documentation-Only Sessions

Rule #16: "No more critique sessions without execution." This session is the mandated critique. Next session MUST be code execution — either Priority 1, 2, or 3 from above. Documents without code changes are noise.

---

## Current Production Truth (Authoritative)

- **Exact as-coded live bot:** 2.76x / daily account Sharpe **1.02** / MaxDD **22.3%** / 286 trades / 1,800 days. Turtle ATR-only exit. **ONLY cite this as production.**
- **Progress harness (dual Chandelier+Turtle ATR):** 621x — RESEARCH DIAGNOSTIC ONLY, different strategy.
- **Fee-adjusted Sharpe:** **[1.02–1.04]** — T94 confirmed. Maker-fill NOT the dominant risk.
- **Dominant deployment risk:** API key availability + zero live execution feedback.
- **Top-10 trade concentration:** **91%** of compounded log return. Structural fragility. Unexplained mechanism.
- **Cross-universe OOS (UNI/MATIC/AVAX):** **11/18 pass (61.1%)** — fails ≥70% guardrail.

---

## Stale Tasks to Close

| Task | Reason to Close |
|------|-----------------|
| Maker-fill modeling | DONE T94. Fee impact negligible. [1.02-1.04] confirmed. |
| ATR_RANK re-sweep | INERT. T=5 settled. Do not re-sweep. |
| HOLD_MAX re-sweep | INERT. 15 is optimal. Do not re-sweep. |
| HEDGE_LOOKBACK re-sweep | 147 wins WF but fails exact-live. Leave at 252. |
| FRESHNESS_COOLDOWN sweep | FC=0 is optimal. No code change. Summary corrected. |
| ATR period sweep | INERT. 24 confirmed. Do not re-sweep. |
| ATR_MULT sweep | INERT. M=2.00 confirmed. Do not re-sweep. |
| M1 Discord integration | 5+ weeks. Execute now or close explicitly. |
| Progress chart fix | DONE (8e650c7). Closed. |

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
11. **If 5+ consecutive commits are docs/ops/monitoring with 0 new mechanism, escalate.**
12. **If blocked on external dependency for 5+ weeks, need an explicit plan.**
13. Maker-fill uncertainty is resolved. Fee impact negligible [1.02-1.04].
14. **Progress harness 621x ≠ live bot 2.76x.** Do not conflate.
15. **2026 YTD underperformance: structural fragility, not accepted limitation.**
16. **No more critique sessions without execution.** Documents without code changes are noise.
17. **Document results BEFORE claiming winners.** Read outputs, then write summary.
18. **Top-10 mechanism is a prerequisite for any future entry filter work.**
19. **Stop waiting for API keys. Build historical replay — no keys required.**