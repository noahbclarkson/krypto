# Strategy Ideas — Krypto Research Log

*Last updated: 2026-05-08 16:05 UTC.* Critique session complete. Chandelier non-binding is the new highest-priority open question.

---

## Critical Alerts

### NEW: Live Bot is Already Turtle-Only — Documentation Needs Cleanup (2026-05-08)
**Situation:** `src/live/bot.rs` uses Turtle ATR as the **sole exit**. Chandelier is NOT in the live code path. HOF and docs still say "dual Chandelier+Turtle ATR exit" — this is wrong.
**Action required:** Update HOF to reflect "Turtle ATR sole exit." Takes 15 minutes. Zero risk. Highest-ROI open task.
**Status:** Decision executed (documentation fix only — code already correct).

### T80: OOS Universe Validation — GENERALIZATION FAILURE
- **11/18 pass (61.1%)**, avg Sharpe **0.149**, avg return **+2.3%/window**, 160 trades
- MATIC strong (6/6), AVAX borderline (4/6), UNI catastrophic (1/6)
- **Fails promotion guardrail** (≥70% pass + Sharpe ≥0.5): 9pp below pass, 0.35 below Sharpe
- **Honest statement:** edge is universe-sensitive, concentrated in high-beta trending pairs; does NOT generalize cleanly to unseen pairs

---

## All C-Items: CLOSED

| Item | Status | Notes |
|------|--------|-------|
| C16 | CLOSED PERMANENTLY | CHAND_PERIOD sweep proved all 98 values identical; Chandelier non-binding; modulating has zero effect |
| C17 | NEVER BUILT | e6f4ed05 only committed CHAND_PERIOD sweep; C17 code was never written; permanently unbuilt |
| C18 | ACCEPTABLE — CLOSED | equity 2.808x at 40% fill; low sensitivity confirmed |
| C19 | GRAVEYARD | harness passed (6/6), exact-live failed (2.74x vs 2.89x); pattern confirmed: harness-pass ≠ production-valid |

---

## Operational Infrastructure (Not Research)

### M1: Equity Trajectory Monitor — Integrate into Discord
**Status:** Built (`examples/m1_equity_trajectory_monitor.rs`) but idle — not integrated into Discord alerting.

**What it does:** Computes 60d rolling return + Sharpe, per-year distribution, alert thresholds, tail concentration.

**What needs to happen:** Run M1 in cron sessions, post rolling return + alert status to #krypto. If rolling return < 10th percentile: explicit alert. This turns a research artifact into operational infrastructure.

**Priority: HIGH.** This is the highest-ROI operational task available. No research value but essential for live deployment readiness.

---

## 3 Most Promising Unbuilt Ideas (2026-05-08 critique)

### 1. HOF/Documentation Cleanup: "Turtle ATR Sole Exit" (PRIORITY: HIGH)
**Problem:** HOF and docs say "dual Chandelier+Turtle ATR exit." `src/live/bot.rs` uses Turtle ATR as the **sole exit**. Chandelier is NOT in the live code path. This is a documentation error.
**Action:** Update HOF and docs to say "Turtle ATR sole exit." Live bot already does this. Takes 15 minutes. Zero risk.
**Status:** Execute now. Highest-ROI open task.

### 2. Cross-Exchange Price Divergence Surveillance (PRIORITY: MEDIUM)
**Idea:** Monitor BTCUSDT Binance vs BTCUSD Kraken/Coinbase for slow divergence >0.5% sustained >4h.
**Different from:** basis carry (funding/roll spread vs price-discovery lag).
**Why it could work:** Crypto liquidity is fragmented. Binance-USDT flow creates persistent premium vs USD-backed spot markets.
**Requirements:** Multi-exchange data feeds, sub-1% fees, cross-exchange execution infra.
**Status:** Concept only. Requires data layer build before testable.

### 3. Regime-Adaptive Exit Multiplier (PRIORITY: LOW)
**Idea:** Current Turtle ATR multiplier is fixed at 2.0 regardless of regime. Tighten in chop (ATR_MULT=1.5), loosen in trending (ATR_MULT=2.5).
**Why it's hard:** ATR_RANK T=5 already gates entries. Need mechanism distinct from existing gate. Exit tightening in chop could fire too early in volatile trends.
**Status:** Concept only. Not started. Requires mechanism design before testable.

---

## Honest Deployment Statement

### Pre-Deployment Safety Checklist
**Status:** WRITTEN ✅ — `docs/LIVE_DEPLOYMENT_CHECKLIST.md` covers pre-launch verification, kill-switch criteria, maker-fill monitoring, and daily equity reporting.

---

## Pattern Confirmed: Harness-Pass ≠ Production-Valid

Three candidates that passed harness validation and failed exact-live replay:

| Candidate | Harness Result | Exact-Live Result | Delta |
|-----------|---------------|-------------------|--------|
| T72 VOL_LOOKBACK gate | passed | 1.01x vs 2.56x | -60.5% |
| T69 semantic alignment | passed | 1.02x vs 2.55x | -60.0% |
| C19 rebalancing | 6/6 pass | 2.74x vs 2.89x | -5.2% |

**Rule:** No candidate is production-valid until exact-live replay verification. The `live_compatible_wf` harness has systematically different entry/exit semantics. Do not cite live_compatible_wf results as deployment-ready.

---

## Sharpe Taxonomy

| Type | Value | Notes |
|------|-------|-------|
| Daily compounded account (exact-live) | **1.03** | Authoritative production number |
| Per-window walk-forward | ~5.5 | INFLATED ~5x; not comparable to account Sharpe |
| Fee-adjusted range (30–80% maker fill) | **0.6–1.3** | Estimated; point estimate prohibited |

Report fee-adjusted Sharpe as a range, not a point estimate.

---

## Anti-Overfitting Rules

1. No Turtle-family parameter sweeps unless a new mechanism is proposed.
2. No "audit" tasks — write the test or close the issue.
3. Every task must have an execute-or-close decision. No "defer to next session."
4. The 176.79x number appears in HOF once: as diagnostic output, not production performance.
5. Daily account Sharpe only on equity charts. Per-window walk-forward Sharpe is not comparable.
6. Report fee-adjusted Sharpe as a range (maker fill uncertain), not a point estimate.
7. **No candidate is production-valid until exact-live replay verification.**
8. Top-10 = 91% of log return. Any new filter must preserve the convex tail.
9. **Chandelier either fires or is removed.** Non-binding exits are documentation errors, not valid strategy complexity.
10. **Universe selection is survivorship bias.** When citing pass rates, always disclose which assets were selected and why.
11. **Suspension animation is a real failure mode.** If 5+ consecutive commits are docs/ops/monitoring with 0 alpha, escalate — don't keep doing the same thing.
12. **2026 YTD underperformance is a live research question**, not just "structural regime." Document which specific market conditions are causing it and whether any parameter change could help.

---

## Production Parameters (Frozen)

```
TURTLE_EP=21, TURTLE_ATR_PERIOD=24, TURTLE_ATR_MULT=2.00, ATR_ENTRY_MULT=0.00,
HOLD_MAX=15, POSITION_CAP=3, FRESHNESS_COOLDOWN=0,
REGIME_ATR_PERIOD=17, REGIME_LOOKBACK=41, ATR_RANK_THRESHOLD=5.0,
VOL_LOOKBACK=92 (diagnostic-only; not used by bot.rs entry logic),
HEDGE_ATR_PERIOD=38, HEDGE_LOOKBACK=252, HEDGE_ATR_PCT=0.45, HEDGE_SIZE_MULT=0.25,
fee_pct=0.000400
```

---

## Honest Deployment Statement

**What we have:** Turtle ATR trend-following on daily crypto bars. Real, profitable (2.76x historical), modest Sharpe (1.03), MaxDD 22.3%. Walk-forward validated on Base5 (100% pass, 6/6 windows). Edge concentrated in high-beta trending crypto pairs.

**What we don't have:** Cross-universe generalization (UNI fails 5/6). Live monitoring (M1 idle). Automated risk controls. Real execution feedback. Infrastructure for autonomous operation. Chandelier that actually fires.

**Only blocker:** Noah's Binance testnet API keys.