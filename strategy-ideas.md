# Strategy Ideas — Krypto Research Log

*Last updated: 2026-05-07 20:05 UTC. Research CLOSED. Operational infrastructure phase.*

---

## Critical Alerts

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
| C18 | ACCEPTABLE — CLOSED | equity 2.808x at 40% maker fill; low sensitivity confirmed |
| C19 | GRAVEYARD | harness passed (6/6), exact-live failed (2.74x vs 2.89x); pattern confirmed: harness-pass ≠ production-valid |

---

## Operational Infrastructure (Not Research)

## Operational Infrastructure (Not Research)

### M1: Equity Trajectory Monitor — Integrate into Discord
**Status:** Built (`examples/m1_equity_trajectory_monitor.rs`) but idle — not integrated into Discord alerting.

**What it does:** Computes 60d rolling return + Sharpe, per-year distribution, alert thresholds, tail concentration.

**What needs to happen:** Run M1 in cron sessions, post rolling return + alert status to #krypto. If rolling return < 10th percentile: explicit alert. This turns a research artifact into operational infrastructure.

**Priority: HIGH.** This is the highest-ROI operational task available. No research value but essential for live deployment readiness.

---

## New Concept: Vol-Scaled Position Sizing (Untested)

**Idea:** Replace fixed `HEDGE_SIZE_MULT=0.25` with vol-scaled position size per symbol — Kelly-based or risk-parity scaling based on realized vol.

**Mechanism:** For each active position, size = `base_size / realized_vol(symbol, lookback=21)` — high-vol symbols get smaller positions, low-vol get larger. Different from:
- ATR_ENTRY_MULT (entry gate, not position size)
- Vol-contingent Chandelier (exit multiplier, not size)
- BTC trend scalar (regime overlay, not per-symbol size)

**Hypothesis:** Vol-scaled sizing improves risk-adjusted returns by dynamically allocating capital to lower-vol, more predictable moves. The fixed HSM is a blunt instrument — it applies the same haircut to all positions regardless of their risk profile.

**Test:** Base5 × 6 walk-forward windows, compare vol-scaled vs fixed HSM=0.25 on Sharpe, MaxDD, pass rate.

**Risk:** Could reduce convexity in trending windows if it systematically undersizes the highest-vol winners.

---

## New Concept: Cross-Exchange Spread Surveillance (Untested)

**Idea:** Monitor Binance vs other CEXs (Kraken, Coinbase, Gemini) for slow price divergence on the same pair. Not basis carry (killed) — legal-arbitrage from exchange microstructure differences.

**Mechanism:** If BTCUSDT on Binance is >0.5% above BTCUSD on Kraken for >4h, capture the mean-reversion spread via triangular or cross-exchange execution. The divergence is typically caused by liquidity imbalances, not fundamental mispricing.

**Why it's different from killed items:** Basis carry trades the spread between BTCUSD and BTCUSDT perpetuals — that's a funding/roll spread trade. Cross-exchange surveillance trades price discovery lag between exchanges — different mechanism entirely.

**Requirements:** Multi-exchange data feeds, sub-1% fee, execution infrastructure for cross-exchange execution.

**Status:** Untested. Requires Binance + at least one other CEX price feed. Not actionable until multi-exchange data layer is built.

---

## Honest Deployment Statement

### Pre-Deployment Safety Checklist
**Status:** Not written. Docs exist (DEPLOYMENT_RUNBOOK.md) but no safety checklist.

**What needs to happen:** Write `docs/LIVE_DEPLOYMENT_CHECKLIST.md` covering:
- Pre-launch: verify config, verify API keys, verify data feed connectivity, verify maker/taker fee tiers
- Kill-switch criteria: what MaxDD or drawdown duration triggers manual shutdown
- Maker-fill monitoring: track actual vs assumed (0–40% range) and alert on persistent taker-dominant execution
- Daily equity reporting: M1 output to Discord each cron cycle

---

## Pattern Confirmed: Harness-Pass ≠ Production-Valid

Three candidates that passed harness validation and failed exact-live replay:

| Candidate | Harness Result | Exact-Live Result | Delta |
|-----------|---------------|-------------------|-------|
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
8. Top-10 = 85.0% of log return. Any new filter must preserve the convex tail.

---

## Production Parameters (Frozen)

```
TURTLE_EP=21, TURTLE_ATR_PERIOD=24, TURTLE_ATR_MULT=2.00, ATR_ENTRY_MULT=0.00,
HOLD_MAX=15, POSITION_CAP=3, FRESHNESS_COOLDOWN=0,
REGIME_ATR_PERIOD=17, REGIME_LOOKBACK=41, ATR_RANK_THRESHOLD=5.0,
VOL_LOOKBACK=92 (diagnostic-only; not used by bot.rs entry logic),
HEDGE_ATR_PERIOD=38, HEDGE_LOOKBACK=252, HEDGE_ATR_PCT=0.45, HEDGE_SIZE_MULT=0.55,
fee_pct=0.000400
```

---

## Honest Deployment Statement

**What we have:** Turtle ATR trend-following on daily crypto bars. Real, profitable (3.13x historical), modest Sharpe (1.03), MaxDD 23.7%. Walk-forward validated on Base5 (100% pass, 6/6 windows). Edge concentrated in high-beta trending crypto pairs.

**What we don't have:** Cross-universe generalization (UNI fails 5/6). Live monitoring (M1 idle). Automated risk controls. Real execution feedback. Infrastructure for autonomous operation.

**Only blocker:** Noah's Binance testnet API keys.
