# Strategy Ideas — Krypto Research Log

*Last updated: 2026-05-11 20:14 UTC. Critical new finding: FRESHNESS_COOLDOWN=93 added to production bot.rs without held-out validation. Research coma (2nd consecutive critique-only session). Escalate to Arc if next session is also 0 code commits.*

---

## Critical Alerts

### 🚨 FRESHNESS_COOLDOWN=93: New Production Mechanism — UNVALIDATED (NEW 2026-05-11)
**T96 promoted FC=93 to production bot.rs without held-out pre-2021 validation. T97 REVERTED: FC=93 produced 0/34 held-out passes (0 trades) vs FC=0 at 20/34 passes (+1.54 Sharpe). Reverted to FC=0 in bot.rs + config.rs.** This is a new mechanism (93-bar re-entry cooldown ≈ 3 months) added via 101-value sweep on in-sample data. Same pattern produced EP=24, ATR_ENTRY_MULT=0.85, HAP=0.09 as false positives. Rule #7: "No candidate is production-valid until exact-live replay verification." FC=93 was not held-out tested. **Action required this session:** Run pre-2021 held-out test. If FC=93 fails held-out, revert FC to 0 in bot.rs immediately.

### ⚠️ Turtle-Only Pre-2021 Held-Out: 3rd Consecutive Plan, NOT Built
Turtle ATR-only (live bot exit) has NEVER been validated against pre-2021 bear data. T12 held-out tested DUAL Chandelier+Turtle exit (100% pass). These are mechanically different strategies. If Turtle ATR-only fails pre-2021, 2.76x is a bull-market artifact. **This is the single most important validation remaining. Execute or explicitly close.**

### ⚠️ 2026 YTD Underperformance: 5 Months, Silent Failure
ATR_RANK T=5 gate is binary — skips entries in low-vol regimes but positions stay FULL size. YTD performance -3.2% via the gate. This is NOT "accepted limitation" — it is an active, documented structural failure. The gate works in some eras and fails in others (T=24 and T=65 both fail held-out). We cannot fix it without destroying the top-10 tail (SOL 2023-01-11 was low-vol at entry). Honest statement: the strategy has a non-stationary entry gate that we cannot stabilize without deeper regime understanding.

### ⚠️ Top-10 Winner Mechanism: UNEXPLAINED CONVEX STRUCTURE
Top-10 trades = 91% of compounded log return. We can list the conditions (low-vol BTC regime, SOL/DOGE entries, low dollar-volume rank) but CANNOT explain the mechanism. Two largest winners: SOL 2023-01-11 and DOGE 2022-10-28. Both entered in low-vol BTC Q1 environments. The strategy is "a few large directional bets on high-beta crypto in ugly BTC regimes" — not "a robust trend-following system."
**Implication:** Any new entry filter must prove it preserves the convex tail.

### T80: OOS Universe Validation — GENERALIZATION FAILURE
- **11/18 pass (61.1%)**, avg Sharpe **0.149**, avg return **+2.3%/window**, 160 trades
- MATIC strong (6/6), AVAX borderline (4/6), UNI catastrophic (1/6)
- **Fails promotion guardrail** (≥70% pass + Sharpe ≥0.5): 9pp below pass, 0.35 below Sharpe

---

## Execution Tasks (Not Research)

### FRESHNESS_COOLDOWN=93 Held-Out Validation (PRIORITY: CRITICAL — EXECUTE THIS SESSION)
**Status:** FC=93 added to bot.rs T96 without held-out validation. Same pattern that produced EP=24 false positive. **Fork `examples/live_bot_exact_equity.rs` → pre-2021 data only → compare FC=0 vs FC=93.** If FC=93 fails held-out, revert to FC=0 immediately.

### Historical Replay Mode (PRIORITY: HIGH — BUILD THIS SESSION)
**Status:** Proposed 08:05 UTC, 12+ hours later, zero code written. No API keys required — uses existing parquet cache. Fork `live_bot_exact_equity.rs` but call `LiveBot::process_bar` directly against cached parquet bars. Validates production code path.

---

## 3 Most Promising Unbuilt Ideas

### 1. FRESHNESS_COOLDOWN=93 Held-Out Validation (PRIORITY: CRITICAL — EXECUTE THIS SESSION)
New production mechanism. Must validate against pre-2021 before trusting. If it fails, revert FC to 0 in bot.rs. Same pattern that produced EP=24 false positive — this is the test that would catch it.

### 2. Turtle-Only Pre-2021 Held-Out Test (PRIORITY: HIGH — EXECUTE THIS WEEK)
Most important validation remaining. Live bot uses Turtle ATR-only (no Chandelier). T12 tested dual Chandelier+Turtle (100% pass). If Turtle ATR-only fails pre-2021, 2.76x is a bull-market artifact.

### 3. Regime Non-Stationarity Quantification (PRIORITY: MEDIUM — DOCUMENT ONLY)
ATR_RANK T=5 is non-stationary: works in some BTC eras, fails in 2026 (5 months). Document the mechanism: BTC vol level? Trend direction? Time-of-year? Correlation structure? Honest uncertainty disclosure — not a fix attempt.

---

## Sharpe Taxonomy

| Type | Value | Notes |
|------|-------|-------|
| Live bot daily compounded (Turtle ATR-only) | **1.02** | Authoritative production number |
| Progress harness (dual Chandelier+Turtle ATR) | 1.19 | RESEARCH — different exit |
| Per-window walk-forward | ~5-6 | RESEARCH harness only; 5x inflated vs account |
| Fee-adjusted (maker-fill confirmed, T94) | **1.02–1.04** | Confirmed range; not the dominant risk |

Report fee-adjusted Sharpe as a range, not a point estimate.

---

## Production Parameters (Frozen)

```
TURTLE_EP=21, TURTLE_ATR_PERIOD=24, TURTLE_ATR_MULT=2.00, ATR_ENTRY_MULT=0.00,
HOLD_MAX=15, POSITION_CAP=3, FRESHNESS_COOLDOWN=93,
REGIME_ATR_PERIOD=17, REGIME_LOOKBACK=41, ATR_RANK_THRESHOLD=5.0,
VOL_LOOKBACK=92 (configured; unused by bot.rs after T72),
HEDGE_ATR_PERIOD=38, HEDGE_LOOKBACK=252, HEDGE_ATR_PCT=0.45, HEDGE_SIZE_MULT=0.25,
fee_pct=0.000400
```

Exit: Turtle ATR(24,2.0) trailing stop ONLY. Chandelier is stored for compatibility but does not fire (T34 KNOWN GAP). **FRESHNESS_COOLDOWN=93 — NEEDS PRE-2021 HELD-OUT VALIDATION.**

---

## Honest Deployment Statement

**What we have:** Turtle ATR trend-following on daily crypto bars. Turtle ATR-only exit (live bot). Real, modest edge (2.76x / Sharpe 1.02). Walk-forward validated on Base5 (100% pass, 6/6 windows). Edge concentrated in high-beta trending crypto pairs. FRESHNESS_COOLDOWN=93 is a new addition that needs validation.

**What we don't have:** Pre-2021 held-out validation of Turtle ATR-only (the actual production exit). Pre-2021 held-out validation of FC=93. Historical replay mode. Live execution feedback. Cross-universe generalization proof.

**Only blocker:** Noah's Binance testnet API keys (6+ weeks blocked). All remaining questions are execution questions, not simulation questions.

---

## Anti-Overfitting Rules

1. No Turtle-family parameter sweeps unless a new mechanism is proposed.
2. No "audit" tasks — write the test or close the issue.
3. Every task must have an execute-or-close decision. No "defer to next session."
4. The 621x / 176.79x numbers appear in progress charts as research diagnostics, NOT production performance.
5. Daily account Sharpe only on equity charts. Per-window walk-forward Sharpe is not comparable.
6. Report fee-adjusted Sharpe as a range (maker fill uncertain), not a point estimate.
7. **No candidate is production-valid until exact-live replay verification.**
8. Top-10 = 91% of log return. Any new filter must preserve the convex tail.
9. **Chandelier either fires or is removed. Non-binding exits are docs errors, not valid strategy complexity.**
10. **Universe selection is survivorship bias.** When citing pass rates, always disclose which assets.
11. **Suspension animation is a real failure mode.** If 5+ consecutive commits are docs/ops/monitoring with 0 alpha, escalate.
12. **Progress harness 621x ≠ live bot 2.76x.** These are different strategies with different exits. Do not conflate them.
13. **2026 YTD underperformance: silent failure.** Not "accepted limitation" — an active documented structural failure.
14. **FRESHNESS_COOLDOWN is a production mechanism. Any change requires held-out pre-2021 validation before production-valid.**
15. **Two consecutive critique-only sessions = escalate to Arc. Research coma is a real failure mode.**
16. **If a task appears in 3+ consecutive PLAN.md documents as "Priority N" and is not executed, either execute it or explicitly close it.**

---

## Historical Replay Mode (NO API KEYS REQUIRED)

**Problem:** 6+ weeks blocked on API keys. Alternative path never tried.

**Concept:** Run `src/live/bot.rs` against historical cached parquet bars in bar-by-bar replay mode. Uses existing data cache. No Binance API required.

**Value:**
- Validates the live bot code path against real historical data
- Catches bugs before deployment
- Paper-trading proxy without live keys
- Should produce identical output to `live_bot_exact_equity.rs` if logic is correct

**Status:** Unbuilt. Proposed 2026-05-11 08:05 UTC. Zero code written as of 2026-05-11 20:14 UTC.