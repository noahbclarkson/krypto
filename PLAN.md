# PLAN.md — Krypto Research and Execution Plan

## State: 2026-05-08 00:05 UTC — CRITIQUE SESSION: Live Bot Smoke Test Priority

**Research genuinely closed.** All testable candidates exhausted. Three structural concerns carried forward:
1. Top-10 = 91.4% of log return (equity without top-10 = 1.09x) — structural fragility
2. Harness-pass ≠ production-valid — 4 confirmed instances (T72, T69, C19, LB=147)
3. Fee-adjusted Sharpe is [0.6–1.3], not the 1.02 point estimate

**Execution priorities:** (1) M1 Discord integration, (2) Live bot dry-run smoke test, (3) Stale snapshot cleanup

## Current Truth

- **Exact as-coded live bot:** 2.75x / daily account Sharpe 1.02 / MaxDD 22.3% / 286 trades / 1,796 days. Production source of truth after latest data refresh (2026-05-07 21:22 UTC).
- **Top-10 trade concentration:** 91.4% of compounded log return. Equity without top-10 = 1.09x. Structural risk — one bad filter silently destroys the tail.
- **Harness-pass ≠ production-valid.** 4 confirmed instances: T72 (VOL gate), T69 (semantic align), C19 (rebalancing), LB=147 (hedge lookback). Walk-forward winner ≠ exact-live winner. No exceptions found.
- **Annualised return 22.9% is misleading without context:** 2022 was -20.1%, 2024 was +22.8%, 2025 was +25.1%. The 22.9% headline is propped by 2021 mega-bull (+66.6%). In "normal" years, the strategy earns 20-25% — acceptable but not exceptional.
- **Walk-forward Sharpe ~5.5:** INFLATED ~5x vs daily account Sharpe. Not comparable. T80 OOS hold-out showed avg Sharpe 0.149 on held-out symbols.
- **Fee-adjusted Sharpe:** report as range [0.6–1.3], not point estimate. Maker fill uncertain (30–80%).
- **The live bot has never run against an exchange.** Dry-run smoke test is the highest-priority execution task.

## Research is CLOSED

Only Noah's Binance testnet API keys unblock deployment. All strategy work genuinely exhausted.

## Resolved / Closed

| Item | Result |
|------|-------|
| C17 consecutive-bar filter | NEVER BUILT — permanently unbuilt |
| C18 maker-fill stress | ACCEPTABLE — equity 2.808x at 40% fill; low sensitivity confirmed |
| C19 rebalancing close_losers | GRAVEYARD — harness passed 6/6, exact-live failed (2.74x vs 2.89x) |
| LB=147 HEDGE_LOOKBACK | GRAVEYARD — walk-forward winner (9/9, Sharpe 1.24), exact-live loser (2.10x vs 2.75x) |
| CHAND_PERIOD inertness | PROVED — 98-value sweep, all identical; Chandelier non-binding |
| ATR_RANK T=24/65 | REJECTED — non-stationary, held-out failure |
| T61/T76 taker-buy pressure | REJECTED — equity no better, 6/10 top winners destroyed |
| T72 VOL_LOOKBACK live gate | REJECTED — 1.01x vs 2.56x; killed 8/10 top winners |
| T69 semantic alignment | REJECTED — worsened exact live replay to 1.02x |
| T67 HEDGE_ATR_PCT | INERT — all 101 values identical |
| T70 FRESHNESS_COOLDOWN | NOT PROMOTED — long cooldowns cut convexity; keep=0 |
| T73 top-winner audit | GUARDRAIL SET — preserve top winners before any filter promotion |
| T80 OOS hold-out universe | GENERALIZATION FAILURE — 11/18 pass (61.1%), avg Sharpe 0.149 |
| ATR_ENTRY_MULT>0 | REJECTED — 0.00 definitively optimal |
| EP=24 | REVERTED — held-out failure |
| Weekend filter | REJECTED |
| Donchian entry | REJECTED — lower pass rate than Turtle |
| SIZE_MULT overlay | INERT — pure risk preference knob, not alpha |
| T85 HEDGE_ATR_PERIOD P=38 | VALIDATED — already default, chart only, no new capability |

## Anti-Spin Rules

1. No more Turtle-family parameter sweeps unless a new mechanism is proposed.
2. No more "audit" tasks — write the test or close the issue.
3. Every task must have an execute-or-close decision. No "defer to next session."
4. Report fee-adjusted Sharpe as a range [0.6–1.3], not a point estimate.
5. Daily account Sharpe only on equity charts. Per-window walk-forward Sharpe is not comparable.
6. Do not use walk-forward pass rate as production-validity criteria — 4/4 candidates show exact-live degradation.
7. The live bot has **never run against an exchange**. Dry-run smoke test is the highest-priority execution task.
8. The 176.79x number is diagnostic only. Do not cite it as a production performance figure.

## Deployment Status

| Component | Status |
|-----------|--------|
| Backtested strategy | **READY** — 2.75x / Sharpe 1.02 / DD 22.3% / 286 trades |
| Live bot code | **READY** — `src/live/bot.rs` exact path verified |
| Dry-run harness | **READY** — `live_bot_exact_equity.rs` |
| Mock exchange | **PARTIAL** — smoke test not yet run |
| Deployment runbook | **WRITTEN** — `docs/DEPLOYMENT_RUNBOOK.md` |
| Equity monitor (M1) | **BUILT** — idle, needs Discord integration |
| Live bot smoke test | **NOT RUN** — highest priority |
| API keys (Noah) | **BLOCKED** — only remaining item |

## Next Steps (Operational, Not Research)

| Priority | Task | Blocker |
|----------|------|---------|
| 1 | **M1 Discord integration** — run M1 in cron, post rolling return + alert to #krypto | None — operational |
| 2 | **Live bot dry-run smoke test** — verify data feed, signal firing, order creation in dry-run mode | None — can run without API keys |
| 3 | **Stale snapshot cleanup** — re-run live_bot_exact_equity.rs to confirm 2.75x, archive superseded files | None — hygiene |
| 4 | Noah: provide Binance testnet API key + secret | Noah action required |

## Parameters (Frozen — Production)

```text
TURTLE_EP=21, TURTLE_ATR_PERIOD=24, TURTLE_ATR_MULT=2.00, ATR_ENTRY_MULT=0.00,
HOLD_MAX=15, POSITION_CAP=3, FRESHNESS_COOLDOWN=0,
REGIME_ATR_PERIOD=17, REGIME_LOOKBACK=41, ATR_RANK_THRESHOLD=5.0,
VOL_LOOKBACK=92 (diagnostic-only; not used by bot.rs entry logic),
HEDGE_ATR_PERIOD=38, HEDGE_LOOKBACK=252, HEDGE_ATR_PCT=0.45, HEDGE_SIZE_MULT=0.25,
fee_pct=0.000400
```