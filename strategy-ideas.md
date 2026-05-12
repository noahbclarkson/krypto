# Strategy Ideas — Krypto Research Log

*Last updated: 2026-05-12 12:08 UTC. T99 and T100 built. T99/T100/Historical Replay all DONE. Research loop closed. Execution loop blocked. M1 Discord NOT done after 5+ weeks. API key blocker escalated.*

---

## Status: Research Loop Closed — Execution Loop Blocked

All Turtle-family parameters are settled. All validation paths are exhausted. No further simulation research is justified without new data or live execution.

**What remains:**
- Live testnet: the only path to new validated information
- Top-winner mechanism decomposition: the only path to understanding the 91% convex tail
- M1 Discord integration: fix it or explicitly close it (5+ weeks broken)
- API key escalation: Arc decision required next session

---

## 3 Most Promising Ideas (Updated 2026-05-12)

### 1. Top-Winner Mechanism Decomposition (PRIORITY: HIGH — New)
**What:** The top-10 trades account for 91% of total log return. Understanding WHY these wins occurred is the only path to reducing concentration risk without destroying the edge.

**Key finding (T73):** The largest winners (SOL 2023-01-11, DOGE 2022-10-28) are NOT obvious bull breakouts. They are low-vol/Q1 BTC regime entries. The convex tail comes from counter-intuitive setups — the exact entries that naive filters would exclude.

**What to do:** For each top-10 winner, document:
- BTC vol regime at entry (high/low/medium percentile of 252-bar history)
- ATR_RANK value at entry (was the gate active?)
- Position size and holding period
- What market regime (bull/bear/chop) BTC was in

**Goal:** Find a structural explanation. Is the tail concentrated in specific vol regimes? Specific position sizes? Specific holding periods? If we understand the mechanism, we can make an informed decision about concentration tolerance rather than just accepting it as "structural."

**This is NOT a filter task.** We are not trying to improve Sharpe. We are trying to understand the edge we already have.

**Status:** UNBUILT. Do not turn this into a parameter sweep.

### 2. Cross-Universe Generalization Test (PRIORITY: MEDIUM — Execution Required)
**What:** T80 showed UNI 1/6 pass, MATIC 6/6 pass, AVAX 4/6 pass. The edge is universe-selected, not universal. Without live testnet, we cannot know which universe the bot is currently operating in.

**Honest problem:** We train on Base5 (BTC/ETH/SOL/XRP/DOGE/ADA) and hope the edge generalizes. UNI failure suggests it doesn't generalize cleanly to all crypto assets. This is survivorship bias in the training universe.

**What to do:** Document the generalization gap honestly. For production deployment, we need a clear answer to "which assets are in-scope and why." The answer is currently "Base5 only" but this is not prominently stated anywhere.

**Status:** UNBUILT. Requires execution, not simulation.

### 3. M1 Discord Integration Fix-or-Close (PRIORITY: CRITICAL — 5+ Weeks Broken)
**What:** Memory says "M1 Discord integration: 5+ weeks, not done. Final call next session." (2026-05-10). Charts generated but never confirmed delivered to Noah's Discord. `sessions_history` returns 0 messages.

**The claim has been carried as "done" for 5+ weeks without end-to-end verification.**

**What to do:** Next session: either (a) build the full M1→Discord pipeline and verify it works with a test message, or (b) explicitly close it — remove from memory, PLAN, and strategy-ideas.md. Do not carry a broken 5-week promise.

**Status:** UNBUILT. Broken.

---

## Closed Items (DO NOT REOPEN)

| Item | Resolution | Date |
|---|---|---|
| Turtle-Only Pre-2021 Validation | DONE — T99: 5/5 symbols positive, geo-mean 1.52x | 2026-05-12 |
| Historical Replay Mode | DONE — T100: 286 trades through process_bar(), matches exact-live | 2026-05-12 |
| Progress Chart Fix | DONE — 8e650c73 adds live bot line to plot_progress.py | 2026-05-11 |
| Fee Impact | CONFIRMED [1.02-1.04] — not dominant deployment risk | 2026-05-10 |
| ATR_ENTRY_MULT | CONFIRMED 0.00 optimal — any filter hurts | 2026-04-13 |
| ATR_RANK threshold | CONFIRMED T=5; T=24 and T=65 fail held-out | 2026-05-04 |
| HOLD_MAX | CONFIRMED 15 | 2026-05-10 |
| HEDGE_SIZE_MULT | CONFIRMED 0.25 | 2026-05-07 |
| VOL_LOOKBACK gate | REJECTED — kills equity, do not add to live bot | 2026-05-06 |
| All Turtle-family params | FROZEN — no sweeps without new mechanism | 2026-05-12 |

---

## Sharpe Taxonomy (Authoritative)

| Type | Value | Note |
|---|---|---|
| Live bot daily compounded (Turtle ATR-only) | **1.02** | Authoritative production number |
| Progress harness (dual Chandelier+Turtle ATR) | 1.19 | RESEARCH — different exit |
| Per-window walk-forward | ~5-6 | NOT comparable — per-window, not account-level |
| Fee-adjusted (T94) | **1.02–1.04** | Confirmed range; not dominant risk |
| 2026 YTD | -1.17 | Real silent failure |

---

## Production Parameters (Frozen — 2026-05-12)

```
TURTLE_EP=21, TURTLE_ATR_PERIOD=24, TURTLE_ATR_MULT=2.00, ATR_ENTRY_MULT=0.00,
HOLD_MAX=15, POSITION_CAP=3, FRESHNESS_COOLDOWN=0,
REGIME_ATR_PERIOD=17, REGIME_LOOKBACK=41, ATR_RANK_THRESHOLD=5.0,
VOL_LOOKBACK=92 (configured; unused by bot.rs after T72 rejection),
HEDGE_ATR_PERIOD=38, HEDGE_LOOKBACK=252, HEDGE_ATR_PCT=0.45, HEDGE_SIZE_MULT=0.25,
fee_pct=0.000400
```

Exit: Turtle ATR(24,2.0) trailing stop ONLY. Chandelier stored for compatibility but does not fire.

---

## Anti-Spin Rules (Active)

1. No Turtle-family parameter sweeps unless a new mechanism is proposed.
2. Every task must have an execute-or-close decision.
3. **621x / 176.79x numbers are research diagnostics, NOT production performance.**
4. Daily account Sharpe only on equity charts. Per-window walk-forward Sharpe not comparable.
5. Report fee-adjusted Sharpe as a range, not a point estimate.
6. **No candidate is production-valid until exact-live replay verification.**
7. Top-10 = 91% of log return. Any new filter must preserve the convex tail.
8. **M1 Discord: 5+ weeks not done. Fix or explicitly close. Do not carry as latent.**
9. **API key blocker: 6+ weeks. Escalate to Arc per anti-spin rule #12.**
10. **Research loop is closed. No further Turtle-family validation without live data.**
11. **Top-winner decomposition: understand the edge, not a new filter.**
12. **Universe selection is survivorship bias.** Base5 is in-scope; UNI is not.