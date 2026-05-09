# PLAN.md — Krypto Research and Execution Plan

## State: 2026-05-09 05:09 UTC — CRITIQUE CYCLE

**Research CLOSED. Live testnet BLOCKED 5+ weeks. Suspension animation identified — 3/5 commits overhead. New research direction: cross-exchange divergence surveillance.**

**Research loop genuinely closed. Live testnet BLOCKED 5+ weeks. Operational readiness is now the only productive path forward.**

---

## Brutal Self-Assessment (This Session's Findings)

### Overhead ratio is a real failure mode
Last 8 commits: **4/8 real research** (e3bf3209 docs fix, dc49ad36 exact-live verification, 35d9011c FC verification, 8f014e47 T86 kill) vs **4/8 pure overhead** (plan updates, memory logs, daily tracking, chore commits).

Pattern: the project produces the appearance of activity without advancing deployment. "Critique and plan update" is the third such commit in a row. The plan updating itself is not a productive activity.

### Research is genuinely closed
All testable Turtle+Chandelier concepts are either validated, killed, or verified non-promotable. The only path to new knowledge is live market data or new infrastructure (cross-exchange data layer). Neither is currently blocked by anything except API keys.

### Maker-fill uncertainty is the biggest single deployment risk
- Reported Sharpe: 1.03
- Honest range: [0.6–1.3] depending on maker-fill rate
- If live maker-fill is 30% (not 70%), Sharpe ≈ 0.6 — barely acceptable
- This is **not** a minor risk. It is the dominant source of deployment uncertainty.

### 2026 YTD underperformance is a structural unresolved problem
- Strategy: -22.7% YTD
- BTC: +12.7% YTD
- ATR_RANK gate skips entries in divergence/chop regimes but does NOT reduce position size
- No fix identified. Documented but unsolved.

### No live feedback loop after 5+ weeks blocked
Every production number is a simulation upper bound. We don't know if the fee model, slippage assumptions, or position sizing are correct. Passive waiting is not a plan.

---

## 3 Most Promising Unbuilt Ideas

### 1. Cross-Exchange Price Divergence Surveillance (PRIORITY: LOW-MEDIUM)
**Concept:** Monitor BTCUSDT Binance vs BTCUSD Coinbase/Kraken. When Binance trades >0.5% above other CEX for >4h sustained, capture mean-reversion via exchange price-discovery lag arb.
**Different from:** basis carry (funding/roll spread), funding rate MR (perpetual premium structure).
**Why it could work:** Crypto liquidity is fragmented. Large Binance-USDT flow creates persistent premium. Legitimate arb window exists for slow institutional money.
**Requirements:** Multi-exchange data feeds, sub-1% fees, cross-exchange execution infra.
**Status:** Concept only. Data layer does not exist. This is a materially different infrastructure build, not a parameter tweak. Not started.

### 2. Vol-Scaled Position Sizing Without Kelly (PRIORITY: LOW)
**Concept:** Replace fixed `HEDGE_SIZE_MULT=0.25` with per-symbol inverse-vol sizing: high-vol symbols get smaller notional, low-vol get larger. Goal is drawdown reduction, not Sharpe maximization.
**Why it's different from T86:** T86 killed Kelly fraction calculation — inverse-vol scaled the Kelly fraction itself, anti-leveraging the tail. The revised concept uses vol only as a risk allocation signal (smaller size in high-vol), not as an optimal-fraction calculator. The mechanism is different enough to be worth revisiting with this specific framing.
**Risk:** Could systematically undersize highest-vol winners (exactly what T86 showed). T73 guardrail applies: must preserve top-10 set.
**Status:** Concept only. Requires mechanism redesign and exact-live verification.

### 3. Regime-Adaptive Exit Multiplier (PRIORITY: LOW)
**Concept:** Current Turtle ATR multiplier is fixed at 2.0 regardless of regime. Tighten in chop (ATR_MULT=1.5 catches smaller reversions), loosen in trending (ATR_MULT=2.5 lets winners run).
**Why it's hard:** ATR_RANK T=5 gates entries by high-vol regimes. Exit tightening would interact with the entry gate. Mechanism design is complex and untested. Could fire too early in volatile trends.
**Status:** Concept only. Requires clearly defined trigger condition. Not started.

---

## Execution Priorities (updated 2026-05-09)

| Priority | Task | Status | Blocker |
|----------|------|--------|---------|
| **1** | **M1 Discord integration** — wire M1 to #krypto in cron | Execute now (15 min) | None |
| **2** | Chandelier removal from all docs/HOF — confirm bot.rs is sole source of truth | Execute now (15 min) | None |
| **3** | **Ask Arc about API key escalation** — what's the actual plan if Noah's keys don't come? | Execute now | None |
| **4** | Live testnet | BLOCKED | Noah: API keys |
| **5** | Cross-exchange data layer | Not started | Requires infra build |

---

## Pattern Confirmed: Harness-Pass ≠ Production-Valid (Established, Not New)

Three candidates passed harness validation and failed exact-live replay:
- T72 VOL_LOOKBACK gate: 1.01x vs 2.56x (-60.5%)
- T69 semantic alignment: 1.02x vs 2.55x (-60.0%)
- C19 rebalancing: 2.74x vs 2.89x (-5.2%)

**Rule:** No candidate is deployment-ready until exact-live replay verification. Walk-forward pass is NOT sufficient.

---

## Current Truth (Authoritative)

- **Exact as-coded live bot:** 2.76x / daily account Sharpe 1.03 / MaxDD 22.3% / 286 trades / 1,797 days. Production source of truth.
- **Maker-fill uncertainty:** Sharpe [0.6–1.3] depending on live fill assumptions. Report as range.
- **Top-10 trade concentration:** 90.9% of compounded log return. Equity without top-10 = 1.10x. Structural risk.
- **T80 OOS generalization:** 11/18 pass (61.1%), avg Sharpe 0.149. Edge is universe-sensitive.
- **Research harness (diagnostic only):** 176.79x / Sharpe 3.29 / MaxDD 99.5% — NOT comparable to live bot.

---

## Parameters (Frozen — Production)

```text
TURTLE_EP=21, TURTLE_ATR_PERIOD=24, TURTLE_ATR_MULT=2.00, ATR_ENTRY_MULT=0.00,
HOLD_MAX=15, POSITION_CAP=3, FRESHNESS_COOLDOWN=0,
REGIME_ATR_PERIOD=17, REGIME_LOOKBACK=41, ATR_RANK_THRESHOLD=5.0,
VOL_LOOKBACK=92 (diagnostic-only; not used by bot.rs entry logic),
HEDGE_ATR_PERIOD=38, HEDGE_LOOKBACK=252, HEDGE_ATR_PCT=0.45, HEDGE_SIZE_MULT=0.25,
fee_pct=0.000400
```

---

## Research Concepts CLOSED This Session

| Concept | Result | Key Finding |
|---------|--------|------------|
| T86 Vol-scaled Kelly | **GRAVEYARD** | 2.28x vs 2.76x (-17%). Inverse-vol Kelly anti-leveraged the tail. Mechanism flawed. |
| FC freshness cooldown | **VERIFIED NOT PROMOTED** | Pass-rate improvement purely mechanical (fewer trades = lower variance). FC=0 default. |
| Chandelier fix-or-remove | **ALREADY DONE** | Live bot is Turtle-only sole exit. Docs clarified. |

---

## Research is CLOSED

All testable Turtle+Chandelier concepts are closed or killed. Only API keys or new infrastructure (cross-exchange data layer) enables further progress.

---

## Critical Open Questions

1. **What is the plan if API keys don't come?** 5+ weeks blocked. No escalation path documented. No alternative route identified.
2. **Can maker-fill assumptions be validated against real exchange data?** Even one week of live order book data constrains the [0.6–1.3] Sharpe range significantly.
3. **Should the project accept research is closed and shift entirely to operational readiness?** M1 integration, automated daily reporting, deployment runbook verification — these don't require API keys.

---

## Anti-Overfitting Rules (updated)

1. No more Turtle-family parameter sweeps unless a new mechanism is proposed.
2. No more "audit" tasks — write the test or close the issue.
3. Every task must have an execute-or-close decision. No "defer to next session."
4. The 176.79x number is diagnostic output, not production performance.
5. Daily account Sharpe only on equity charts. Per-window walk-forward Sharpe is not comparable.
6. Report fee-adjusted Sharpe as a range [0.6–1.3], not a point estimate.
7. **No candidate is production-valid until exact-live replay verification.**
8. Top-10 = 91% of log return. Any new filter must preserve the convex tail.
9. **Suspension animation is a real failure mode.** If 5+ consecutive commits are docs/ops/monitoring with 0 new research, escalate.
10. **Operational tasks (M1 integration, docs cleanup) are higher priority when research is exhausted.**
11. **If blocked on external dependency for 5+ weeks, need an explicit plan — not passive waiting.**
12. **Maker-fill uncertainty is the dominant deployment risk.** It belongs on every status report until resolved.

---

## Deployment Status

| Component | Status |
|-----------|--------|
| Backtested strategy | **READY** — 2.76x / Sharpe 1.03 / DD 22.3% / 286 trades |
| Live bot code | **READY** — `src/live/bot.rs` exact path verified |
| Dry-run harness | **READY** — `live_bot_exact_equity.rs` |
| Mock exchange | **READY** — smoke test passed |
| Deployment runbook | **WRITTEN** — `docs/DEPLOYMENT_RUNBOOK.md` |
| Deployment safety checklist | **WRITTEN** — `docs/LIVE_DEPLOYMENT_CHECKLIST.md` |
| Equity monitor (M1) | **BUILT** — needs Discord integration |
| API keys (Noah) | **BLOCKED** — only remaining item |

---

## Resolved / Closed

| Item | Result |
|------|--------|
| C17 consecutive-bar filter | NEVER BUILT — permanently unbuilt |
| C18 maker-fill stress | ACCEPTABLE — equity 2.808x at 40% fill; low sensitivity confirmed |
| C16 regime-conditional Chandelier | CLOSED — Chandelier non-binding; modulating has zero effect |
| C19 rebalancing close_losers | GRAVEYARD — harness passed 6/6, exact-live failed (2.74x vs 2.89x) |
| CHAND_PERIOD inertness | PROVED — 98-value sweep, all identical |
| ATR_RANK T=24/65 | REJECTED — non-stationary, held-out failure |
| T61/T76 taker-buy pressure | REJECTED — equity no better, 6/10 top winners destroyed |
| T72 VOL_LOOKBACK live gate | REJECTED — 1.01x vs 2.56x; killed 8/10 top winners |
| T69 semantic alignment | REJECTED — worsened exact live replay to 1.02x |
| T67 HEDGE_ATR_PCT | INERT — all 101 values identical |
| T70 FRESHNESS_COOLDOWN | NOT PROMOTED — long cooldowns cut convexity; keep=0 |
| T73 top-winner audit | GUARDRAIL SET — preserve top winners before any filter promotion |
| T80 OOS hold-out universe | GENERALIZATION FAILURE — 11/18 pass (61.1%), avg Sharpe 0.149, UNI 1/6 |
| ATR_ENTRY_MULT>0 | REJECTED — 0.00 definitively optimal |
| EP=24 | REVERTED — held-out failure |
| Weekend filter | REJECTED |
| Donchian entry | REJECTED — lower pass rate than Turtle |
| SIZE_MULT overlay | INERT — pure risk preference knob, not alpha |
| T86 Vol-scaled Kelly | GRAVEYARD — 2.28x vs 2.76x (-17%), anti-leveraged tail |
| T83 HEDGE_SIZE_MULT | PROMOTED — 0.25 is robustness winner (86.7% pass vs 76.7% at 0.55) |

---

## Next Steps (Priority Order)

### 1. M1 Discord Integration — Execute NOW
**Status:** Built but idle. Not integrated into Discord alerting.
**Execution:** `cargo run --example m1_equity_trajectory_monitor --profile sweep 2>&1`
**Post to Discord #krypto:** One-line status with 60d return, rolling Sharpe, equity vs 1y peak, alert status.
**Priority: HIGHEST.** This is the highest-ROI currently executable task. No API keys required.

### 2. Ask Arc About API Key Escalation — Execute NOW
**Status:** 5+ weeks blocked. No explicit plan documented.
**Action:** Send to `agent:main:main` asking: "What's the actual plan if Noah's Binance testnet keys don't come? Need an explicit alternative or escalation path."
**Priority: HIGHEST.** This is a governance issue, not a research issue.

### 3. Chandelier Docs Cleanup — Execute NOW
**Status:** bot.rs already correct (Turtle-only). HOF updated in e3bf3209. Confirm all remaining docs reference Turtle ATR sole exit.
**Priority: LOW.** 15 minutes. Confirms documentation accuracy.
