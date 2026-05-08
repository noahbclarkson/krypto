# PLAN.md — Krypto Research and Execution Plan

## State: 2026-05-08 12:09 UTC — CRITIQUE SESSION: Suspension Animation Confirmed

**Research closed. Operational infrastructure phase. Project in suspension awaiting API keys.**

Key critique findings (2026-05-08 12:09 UTC):
1. **Last 5 commits: 0 alpha.** All docs/hygiene/ops — suspension animation confirmed.
2. **Top-10 = 90.9% log return concentration.** Equity without top-10 = 1.10x. Structural tail risk — one catastrophic drawdown on top contributors destroys most equity.
3. **Maker-fill = existential risk, unquantified.** Point estimate (40%) vs real range (0–80%) = Sharpe 0.6–1.3. Live testnet is the only honest test.
4. **M1 built but not wired.** Every cron cycle without a Discord post is a missed heartbeat.
5. **2026 regime underperformance is structural.** -22.7% YTD vs BTC +12.7%. ATR_RANK is a gate (skip entries), not a position-size reducer. No current parameter fixes chop+divergence.
6. **Generalization claim is narrow.** Base5 = best crypto assets. UNI (11th cap) fails 5/6 hold-outs. Edge concentrated in high-beta trending pairs, not crypto-universal.

## Execution priorities (updated 2026-05-08):
1. **M1 Discord integration** — wire to Discord #krypto each cron. Built, not wired. Execute NOW.
2. **Vol-scaled position sizing** — exact-live test only (must avoid harness-gap pattern). Testable now.
3. **Live testnet** — only remaining blocker is Noah's API keys.

## Current Truth

- **Exact as-coded live bot:** 2.77x / daily account Sharpe 1.03 / MaxDD 22.3% / 286 trades / 1,796 days. Production source of truth.
- **Top-10 trade concentration:** 90.9% of compounded log return. Equity without top-10 = 1.10x. Structural risk.
- **Research harness (diagnostic only):** 176.79x / Sharpe 3.29 / MaxDD 99.5% / 156 trades — different system, not comparable.
- **Per-window walk-forward Sharpe (~5.5):** INFLATED ~5x vs daily account Sharpe. Not comparable.
- **T80 OOS hold-out: generalization failure.** UNI/MATIC/AVAX: 11/18 pass (61.1%), avg Sharpe 0.149. Edge is universe-sensitive.
- **HEDGE_SIZE_MULT = 0.25. HOLD_MAX = 15.** All params frozen.

## Research is CLOSED

Only Noah's Binance testnet API keys unblock deployment. All strategy work genuinely exhausted.

## C16: CLOSED PERMANENTLY — Mechanistically Invalidated (2026-05-07)

CHAND_PERIOD 98-value extensive sweep proved all 98 values produce IDENTICAL results: Sharpe=2.797, pass=68.3%, equity=1.4538x. Root cause: Turtle ATR exit fires before Chandelier in dual-exit architecture. Chandelier is non-binding.

C16 (regime-conditional Chandelier multiplier) attempted to modulate CHAND_MULT by regime. Modulating the multiplier of a non-binding exit has zero effect. **Close permanently.**

## Biggest Blind Spots (Honest Assessment)

1. **We only test in a "learned" universe.** T80 confirmed: UNI fails 5/6. Edge concentrated in high-beta trending crypto pairs. Do NOT claim cross-universe generalization.
2. **Equity dangerously concentrated.** Top-10 = 90.9% of log return. Equity without top-10 = 1.10x. One bad filter silently destroys the tail.
3. **Harness-pass ≠ production-valid.** T72, T69, C19 all passed harness tests and FAILED exact-live replay. Pattern established.
4. **2021/2022 chop regimes underweighted.** Full-history pass rates inflated by mega-bull windows.
5. **Maker-fill risk is unquantified.** Fee microstructure is NOT the dominant deployment risk, but the maker-fill assumption is the biggest single source of uncertainty.
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
| Deployment safety checklist | **WRITTEN** — `docs/LIVE_DEPLOYMENT_CHECKLIST.md` |
| Equity monitor (M1) | **BUILT** — needs Discord integration |
| API keys (Noah) | **BLOCKED** — only remaining item |

**Next step:** Noah provides Binance testnet API key + secret.
Then: `BINANCE_API_KEY=xxx cargo run --example live_turtle_chandelier --profile sweep -- --live`

## Resolved / Closed

| Item | Result |
|------|--------|
| C17 consecutive-bar filter | NEVER BUILT — e6f4ed05 only committed CHAND_PERIOD sweep; permanently unbuilt |
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

| Priority | Task | Status | Blocker |
|----------|------|--------|---------|
| 1 | **M1 Discord integration** — wire to Discord #krypto each cron | Execute now — built, not wired | None |
| 2 | Vol-scaled position sizing — exact-live test | One harness run required | None |
| 3 | Live testnet deployment | BLOCKED | Noah: API keys required |

### M1 Discord Integration — Execute NOW

**Status:** Built but idle. Every cron cycle should produce a 1-line status update.
**Execution:** `cargo run --example m1_equity_trajectory_monitor --profile sweep 2>&1`
**Post to Discord #krypto:** One-line status with 60d return, rolling Sharpe, equity vs 1y peak, alert status.
**Today's output (2026-05-08 12:09 UTC):** `📊 M1 12:09 UTC: 60d +0.9% | Sharpe 0.57 | DD -2.5% | vs 1y peak -7.5% | 🟢 GREEN`
**Priority: HIGHEST.** This is the only currently executable task that advances operational readiness.

### Vol-Scaled Position Sizing — Exact-Live Test Only

**Concept:** Replace fixed HSM=0.25 with per-symbol sizing: `size = base / realized_vol(symbol, 21-bar)`. High-vol get smaller, low-vol get larger.
**Different from rejected items:** Not an entry gate (ATR_ENTRY_MULT), not an exit multiplier (vol-contingent Chandelier), not a regime overlay (BTC trend scalar).
**Anti-overfitting rule from T69/T72/C19 pattern:** Walk-forward pass is NOT sufficient. Must run exact-live replay.
**If exact-live equity < 2.76x baseline → close concept.**
**If exact-live equity ≥ 2.76x → verify Sharpe and MaxDD improve, then promote.**

### Live Testnet — Only Remaining Blocker

**Only step:** Noah provides Binance testnet API key + secret.
**Then:** `BINANCE_API_KEY=xxx cargo run --example live_turtle_chandelier --profile sweep -- --live`

## Pattern: Harness-Pass ≠ Production-Valid (Established, Not New)

Three candidates that passed harness validation and failed exact-live replay:
- T72 VOL_LOOKBACK gate: 1.01x vs 2.56x (-60.5%)
- T69 semantic alignment: 1.02x vs 2.55x (-60.0%)
- C19 rebalancing: 2.74x vs 2.89x (-5.2%)

**Rule:** No candidate is deployment-ready until exact-live replay verification.