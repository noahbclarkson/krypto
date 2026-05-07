# PLAN.md — Krypto Research and Execution Plan

## 20:05 UTC Critique — Operational Phase Priorities

**State: 2026-05-07 20:05 UTC — Research CLOSED. Hygiene and operational infrastructure only.**

### Key Findings

1. **Harness gap confirmed AGAIN (4th instance):** T84 HEDGE_LOOKBACK sweep found LB=147 winner in walk-forward (9/9 pass, Sharpe 1.24) but exact-live replay collapsed to 2.10x vs 2.77x with LB=252. Walk-forward robustness ≠ exact-live robustness. The walk-forward harness systematically overestimates.

2. **Stale snapshot:** `snapshots/live_bot_exact_equity.md` shows LB=147 / 2.10x from the T84 test run. Config.rs has LB=252 (production default), but the snapshot was never regenerated after the revert. Authoritative production numbers are ~2.77x / Sharpe 1.03 / MaxDD 22.3% / 286 trades from the T83/T75 series.

3. **Fee model range:** daily Sharpe 1.03 assumes 4bp taker/side with 0% maker. Actual range [0.6, 1.3] depending on maker fill rate (30-80%). Do not report point estimates.

4. **Top-10 concentration confirmed at 90.9%.** Equity without top 10 = 1.10x. The strategy is a convex tail hunt, not a diversified portfolio. This is real and the single most important risk factor.

5. **No live execution path has ever been tested.** The mock exchange is incomplete. The exact-live harness is a simulation, not a test. The first real exchange interaction is the true test.

### Biggest Blind Spots

| Blind Spot | Impact |
|-----------|--------|
| Walk-forward ≠ exact-live | 4 failures; pass rate is not a sufficient filter |
| Stale snapshot data | PLAN.md and HOF reference 2.77x, but live_bot_exact_equity.md shows 2.10x |
| Never shipped anything live | Entire project is simulation; real execution path untested |
| Maker-fill uncertainty | Sharpe range [0.6, 1.3] is too wide to know true performance |
| 462 example files, no cleanup | Old artifacts create noise; future sessions can't find current state |

### Anti-Spin Rules (unchanged)

1. No more Turtle-family parameter sweeps unless a new mechanism is proposed.
2. No "audit" tasks — write the test or close the issue.
3. Every task must have an execute-or-close decision. No "defer to next session."
4. The 176.79x number appears in HOF once: as diagnostic output, not production performance.
5. Daily account Sharpe only on equity charts. Per-window walk-forward Sharpe is not comparable.
6. Report fee-adjusted Sharpe as a range (maker fill uncertain), not a point estimate.
7. **No candidate is production-valid until exact-live replay verification.**
8. Top-10 = 90.9% of log return. Any new filter must preserve the convex tail.

## Next Steps (Operational, Not Research)

| Priority | Task | Blocker |
|----------|------|---------|
| 1 | **M1 Discord Integration** — run M1 in cron, post rolling return + alert status to #krypto | None — ops work |
| 2 | **Stale snapshot cleanup** — re-run `live_bot_exact_equity.rs` with LB=252 confirm 2.77x; archive/delete superseded snapshot files | None — hygiene |
| 3 | **Live Deployment Safety Checklist** — write `docs/LIVE_DEPLOYMENT_CHECKLIST.md` with MaxDD shutdown trigger, maker-fill monitoring, daily equity reporting | None — docs |

**Research truly closed. Only operational infrastructure and deployment preparation remain.**

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
| 1 | Integrate M1 equity monitor into Discord cron reporting | None — operational |
| 2 | Pre-deployment safety checklist | None — docs task |
| 3 | Noah: provide Binance testnet API key + secret | Noah action required |