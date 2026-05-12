# Strategy Ideas — Krypto Research Log

*Last updated: 2026-05-12 18:01 UTC. T99/T100/T104 built. Research loop closed. Execution loop blocked. M1 Discord closed as misunderstanding. API key blocker escalated once; do not re-escalate without new information.*

---

## Status: Research Loop Closed — Execution Loop Blocked

All Turtle-family parameters are settled. All validation paths are exhausted. No further simulation research is justified without new data or live execution.

**What remains:**
- Live testnet: the only path to new validated information
- Production universe documentation: reduce deployment ambiguity around Base5 / hold-outs
- Order-flow dynamic position sizing: only if treated as a new mechanism, not a Turtle entry filter
- API key blocker: report clearly; do not re-escalate every run

---

## 3 Most Promising Ideas (Updated 2026-05-12)

### 1. Production Universe Document (PRIORITY: MEDIUM — Documentation/Deployment)
**What:** T80 showed UNI 1/6 pass, MATIC 6/6 pass, AVAX 4/6 pass. The edge is universe-selected, not universal. Without live testnet, we cannot know which universe the bot is currently operating in.

**Why:** Production needs a clear in-scope universe statement: Base5 is BTC/ETH/SOL/XRP/DOGE/ADA, with explicit survivorship-bias caveat and hold-out failures.

**Status:** UNBUILT. Only do this if it materially reduces deployment ambiguity; avoid docs-only churn.

### 2. Order-Flow Dynamic Position Sizing (PRIORITY: LOW-MEDIUM — New Mechanism Only)
**What:** Taker-buy pressure cache exists (T76) and pressure-as-entry-filter was rejected (T61). A different mechanism would be dynamic sizing, not filtering: high pressure → full slot, weak pressure → reduced slot.

**Guardrail:** Must preserve T104 top-winner set and verify exact-live performance. Do not repackage the rejected median-pressure entry gate.

**Status:** UNBUILT. Suspended until live testnet or a genuine Track C session.

### 3. 2026 YTD Failure Monitoring (PRIORITY: LOW — No Fix Path Yet)
**What:** 2026 YTD remains -3.2%, Sharpe -1.17. T104 does not solve this; it clarifies that the convex tail comes from early rebound continuation and that stricter ATR_RANK gates would delete too many winners.

**Status:** Documented risk only. Do not create another ATR_RANK/Turtle-family sweep without a genuinely new mechanism.

---

## Closed Items (DO NOT REOPEN)

| Item | Resolution | Date |
|---|---|---|
| Turtle-Only Pre-2021 Validation | DONE — T99: 5/5 symbols positive, geo-mean 1.52x | 2026-05-12 |
| Historical Replay Mode | DONE — T100: 286 trades through process_bar(), matches exact-live | 2026-05-12 |
| Top-Winner Decomposition | DONE — T104: 91.4% top-10 log share; early rebound/continuation mechanism corrected | 2026-05-12 |
| M1 Discord Integration | CLOSED — misunderstanding; M1 is console/PNG only, not a Discord poster | 2026-05-12 |
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
8. **M1 Discord is closed.** Do not reopen unless a new Discord-delivery requirement is explicitly created.
9. **API key blocker: 6+ weeks, escalated once. Do not re-escalate every run.**
10. **Research loop is closed. No further Turtle-family validation without live data.**
11. **Top-winner decomposition closed by T104. Any new filter must preserve early-rebound convex tail.**
12. **Universe selection is survivorship bias.** Base5 is in-scope; UNI is not.