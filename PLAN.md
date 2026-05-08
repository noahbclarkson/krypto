# PLAN.md — Krypto Research and Execution Plan

## State: 2026-05-08 08:05 UTC — CRITIQUE SESSION: Operational Infrastructure Phase

**Research closed. Operational infrastructure priority.**

Key critique findings (2026-05-08 08:05 UTC):
1. Last 5 commits: 5/5 documentation/hygiene/data-refresh. Zero new alpha since T83 (HSM=0.25, 2026-05-07).
2. **T86 LB=8 regime lookback = noise.** LB=8 equity 3.36x vs LB=41 equity 3.36x — delta 0.0004x (rounding artifact). No production change warranted.
3. **Suspension animation confirmed.** Research closed + no live testnet = zero real feedback. Documentation commits are filling the void, not advancing the project.
4. **Maker-fill = existential risk, unquantified.** If fill is 20% not 40%, real Sharpe ≈ 0.6. Live testnet is the only honest test.
5. **M1 still idle.** Every cron cycle without M1 Discord integration is a missed monitoring opportunity.
6. **2026 regime modeled but unfixed.** Turtle -22.7% YTD vs BTC +12.7% is a divergence/whipsaw regime. ATR_RANK is a gate (skip entries), not a position-size reducer. If 2026 continues as chop+divergence, strategy continues losing.

**Execution priorities:** (1) Run M1 monitor + post to Discord, (2) Vol-scaled position sizing test, (3) Live Deployment Safety Checklist

## Current Truth

- **Exact as-coded live bot:** 2.77x / daily account Sharpe 1.03 / MaxDD 22.3% / 286 trades / 1,796 days. Production source of truth after T83 defensive hedge-size retune and 15:05 UTC rerun.
- **Top-10 trade concentration:** 90.9% of compounded log return after latest exact-live refresh. Equity without top-10 = 1.10x. Structural risk.
- **Research harness (diagnostic only):** 176.79x / Sharpe 3.29 / MaxDD 99.5% / 156 trades — different system, not comparable.
- **Per-window walk-forward Sharpe (~5.5):** INFLATED ~5x vs daily account Sharpe. Not comparable.
- **T80 OOS hold-out: generalization failure.** UNI/MATIC/AVAX: 11/18 pass (61.1%), avg Sharpe 0.149. Edge is universe-sensitive, NOT cleanly cross-universal.
- **HEDGE_SIZE_MULT = 0.25. HOLD_MAX = 15.** All params frozen.

## Research is CLOSED

Only Noah's Binance testnet API keys unblock deployment. All strategy work genuinely exhausted.

## C16: CLOSED PERMANENTLY — Mechanistically Invalidated (2026-05-07)

CHAND_PERIOD 98-value extensive sweep proved all 98 values produce IDENTICAL results: Sharpe=2.797, pass=68.3%, equity=1.4538x. Root cause: Turtle ATR exit fires before Chandelier in dual-exit architecture. Chandelier is non-binding.

C16 (regime-conditional Chandelier multiplier) attempted to modulate CHAND_MULT by regime. Modulating the multiplier of a non-binding exit has zero effect. **Close permanently.**

## Biggest Blind Spots (Honest Assessment)

1. **We only test in a "learned" universe.** T80 confirmed: UNI fails 5/6. Edge concentrated in high-beta trending crypto pairs. Do NOT claim cross-universe generalization.
2. **Equity dangerously concentrated.** Top-10 = 90.9% of log return after T83/latest refresh. Equity without top-10 = 1.10x. One bad filter silently destroys the tail.
3. **Harness-pass ≠ production-valid.** T72, T69, C19 all passed harness tests and FAILED exact-live replay. Pattern established.
4. **2021/2022 chop regimes underweighted.** Full-history pass rates inflated by mega-bull windows.
5. **Maker-fill risk is quantified (C18).** Low sensitivity confirmed. Fee microstructure is NOT the dominant deployment risk.
6. **M1 equity monitor is built but idle.** Not integrated into Discord alerting.

## Anti-Spin Rules

1. No more Turtle-family parameter sweeps unless a new mechanism is proposed.
2. No more "audit" tasks — write the test or close the issue.
3. Every task must have an execute-or-close decision. No "defer to next session."
4. The 176.79x number appears in HOF once: as diagnostic output, not production performance.
5. Daily account Sharpe only on equity charts. Per-window walk-forward Sharpe is not comparable.
6. Report fee-adjusted Sharpe as a range (maker fill uncertain), not a point estimate.

## Deployment Status

| Component | Status |
|-----------|--------|
| Backtested strategy | **READY** — 2.77x / Sharpe 1.03 / DD 22.3% / 286 trades |
| Live bot code | **READY** — `src/live/bot.rs` exact path verified |
| Dry-run harness | **READY** — `live_bot_exact_equity.rs` |
| Mock exchange | **READY** — smoke test passed |
| Deployment runbook | **WRITTEN** — `docs/DEPLOYMENT_RUNBOOK.md` |
| Equity monitor (M1) | **BUILT** — idle, needs Discord integration |
| API keys (Noah) | **BLOCKED** — only remaining item |

**Next step:** Noah provides Binance testnet API key + secret.
Then: `BINANCE_API_KEY=xxx cargo run --example live_turtle_chandelier --profile sweep -- --live`

## Resolved / Closed

| Item | Result |
|------|--------|
| C17 consecutive-bar filter | NEVER BUILT — e6f4ed05 only committed CHAND_PERIOD sweep; permanently unbuilt |
| C18 maker-fill stress | ACCEPTABLE — equity 2.808x at 40% fill; low sensitivity confirmed |
| C16 regime-conditional Chandelier | CLOSED — Chandelier non-binding; modulating it has zero effect |
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

## Parameters (Frozen — Production)

```text
TURTLE_EP=21, TURTLE_ATR_PERIOD=24, TURTLE_ATR_MULT=2.00, ATR_ENTRY_MULT=0.00,
HOLD_MAX=15, POSITION_CAP=3, FRESHNESS_COOLDOWN=0,
REGIME_ATR_PERIOD=17, REGIME_LOOKBACK=41, ATR_RANK_THRESHOLD=5.0,
VOL_LOOKBACK=92 (diagnostic-only; not used by bot.rs entry logic),
HEDGE_ATR_PERIOD=38, HEDGE_LOOKBACK=252, HEDGE_ATR_PCT=0.45, HEDGE_SIZE_MULT=0.25,
fee_pct=0.000400
```

## Resolved Concepts (Do Not Revisit)

- VOL_LOOKBACK live top-3 gate: rejected by T72 (1.01x vs 2.56x)
- TURTLE_ATR_MULT: 2.00 confirmed; no more nearby sweeps
- HEDGE_ATR_PCT: 101 identical — dead code
- ATR_ENTRY_MULT: 0.00 definitively optimal
- FRESHNESS_COOLDOWN: 0 wins; longer cooldowns sacrifice convexity
- EP=24: reverted (held-out failure)
- Weekend filter: rejected
- ATR_RANK=24/65: failed held-out; T=5.0 is risk dial only
- Donchian: 63% < 69.1% guardrail; not Turtle replacement
- All non-trend strategies: dead or borderline
- Vol-contingent Chandelier: dead (too slow-moving)
- Taker-buy pressure entry overlay: rejected (6/10 top winners killed)
- Equity integration: REJECTED (equities hurt crypto portfolio)
- BTC trend scalar: REJECTED
- Correlation filter: REJECTED
- Rebalancing close_losers I=5: GRAVEYARD (harness passed, exact-live failed)
- CHAND_PERIOD: PROVED inert — all 98 values identical (Chandelier non-binding)
- C16 regime-conditional Chandelier: CLOSED (non-binding exit)
- C17 consecutive-bar filter: NEVER BUILT (e6f4ed05 sweep-only)
- C19 rebalancing: GRAVEYARD (exact-live verification failed)


## Next Steps (Operational, Not Research)

| Priority | Task | Blocker |
|----------|------|---------|
| 1 | **M1 Discord integration — post rolling return to #krypto each cron cycle** | None — execute now |
| 2 | Vol-scaled position sizing exact-live test | None — one harness run, must verify exact-live not just WF |
| 3 | Write `docs/LIVE_DEPLOYMENT_SAFETY_CHECKLIST.md` | **COMPLETE** ✅ |
| 4 | 176.79x HOF entry | **ASSESSED — sufficient diagnostic label exists, no change needed** |
| 5 | Noah: provide Binance testnet API key + secret | Noah action required |

## Execution Details

### M1 Discord Integration — CRITICAL
**Status:** Built but idle. Every cron cycle should produce a 1-line status update.
**Execution:** `cargo run --example m1_equity_trajectory_monitor --profile sweep 2>&1`
**Post to Discord #krypto:** Include 60d return, rolling Sharpe, equity vs 1y peak, alert status.
**Format:** `📊 M1 [HH:MM UTC]: 60d +X.X% | Sharpe X.XX | DD -X.X% | vs 1y peak -X.X% | STATUS`
**This is the highest-ROI operational task.**

### Vol-Scaled Position Sizing — Exact-Live Test Only
**Key rule from T69/T72/C19 pattern:** Walk-forward pass is NOT sufficient. Must run exact-live replay.
**If exact-live equity < 2.76x baseline → close concept.**
**If exact-live equity ≥ 2.76x → verify Sharpe and MaxDD improve, then promote.**