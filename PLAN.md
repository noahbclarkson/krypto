# PLAN.md — Krypto Research and Execution Plan

**State: 2026-05-07 06:25 UTC — Research CLOSED. All items resolved. Only API keys block deployment.**

## Current Truth

- **Exact as-coded live bot:** 2.89x / daily account Sharpe 0.98 / MaxDD 29.0% / 298 trades / 1,796 days. Production source of truth.
- **Research harness (diagnostic only):** 176.79x / Sharpe 3.29 / MaxDD 99.5% / 156 trades — different system, not comparable.
- **Per-window walk-forward Sharpe (~5.5):** INFLATED ~5x vs daily account Sharpe. Not comparable.
- **No more Turtle-family parameter sweeps.** Last 8+ weeks: all same-family Turtle tuning exhausted.
- **ATR_RANK_THRESHOLD is a risk dial (T=5.0), not an alpha source.** Non-stationary; held-out fails at T=24 and T=65.
- **T80 OOS hold-out validation is mixed.** UNI/MATIC/AVAX: 11/18 pass (61.1%), avg Sharpe 0.149 — borderline.
- **HEDGE_SIZE_MULT = 0.55** (updated from 0.40). Verify this value in config.rs before citing.

## Research is CLOSED

Only Noah's Binance testnet API keys unblock deployment. All strategy work genuinely exhausted.

## C16: CLOSED PERMANENTLY — Mechanistically Invalidated (2026-05-07)

CHAND_PERIOD 98-value extensive sweep proved all 98 values produce IDENTICAL results: Sharpe=2.797, pass=68.3%, equity=1.4538x. Root cause: Turtle ATR exit fires before Chandelier in dual-exit architecture. Chandelier is non-binding.

C16 (regime-conditional Chandelier multiplier) attempted to modulate CHAND_MULT by regime. Modulating the multiplier of a non-binding exit has zero effect. **Close permanently.**

## Biggest Blind Spots (Honest Assessment)

1. **We only test in a "learned" universe.** T80 partially closed this. Edge remains universe-sensitive; do not claim clean cross-universe generalization.
2. **Equity is dangerously concentrated.** Top 10 log contributors = 82.8% of compounded return. One bad filter silently destroys the tail.
3. **We test strategies on the walk-forward harness, not on the exact live bot path.** T72 and T69 both passed harness validation but failed exact-live bot semantics.
4. **2021/2022 chop regimes are underweighted in pass-rate calculations.** Full-history pass rates inflated by mega-bull windows.
5. **Maker-fill risk is now quantified (C18).** At 40% maker fill: equity=2.808x, Sharpe=0.841, MaxDD=28.1%. Fee microstructure is not the dominant deployment risk.

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
| Backtested strategy | **READY** — 2.89x / Sharpe 0.98 / DD 29% |
| Live bot code | **READY** — `src/live/bot.rs` exact path verified |
| Dry-run harness | **READY** — `live_bot_exact_equity.rs` |
| Mock exchange | **READY** — smoke test passed |
| Deployment runbook | **WRITTEN** — `docs/DEPLOYMENT_RUNBOOK.md` |
| Research report (PDF) | **WRITTEN** — `docs/research_report.pdf` |
| API keys (Noah) | **BLOCKED** — only remaining item |

**Next step:** Noah provides Binance testnet API key + secret.
Then: `BINANCE_API_KEY=xxx cargo run --example live_turtle_chandelier --profile sweep -- --live`

## Resolved / Closed

| Item | Result |
|------|--------|
| C17 consecutive-bar filter | REJECTED — 81% vs 93.7% pass, -2.9% equity, fails dual-gate |
| C18 maker-fill stress | ACCEPTABLE — 40% fill = 2.808x, Sharpe 0.841, above 1.5x threshold |
| C16 regime-conditional Chandelier | CLOSED — Chandelier non-binding; modulating it has zero effect |
| CHAND_PERIOD inertness | PROVED — 98-value sweep, all identical |
| ATR_RANK T=24/65 | REJECTED — non-stationary, held-out failure |
| T61/T76 taker-buy pressure | REJECTED — equity no better, 6/10 top winners destroyed |
| T72 VOL_LOOKBACK live gate | REJECTED — 1.01x vs 2.56x baseline |
| T69 semantic alignment | REJECTED — worsened exact live replay |
| T67 HEDGE_ATR_PCT | INERT — all 101 values identical |
| ATR_ENTRY_MULT>0 | REJECTED — 0.00 definitively optimal |
| EP=24 | REVERTED — held-out failure |
| Weekend filter | REJECTED |
| Donchian entry | REJECTED — lower pass rate than Turtle |
| T53 mock exchange | CLOSED — signal path verified; only API keys block |
| T80 OOS hold-out universe | MIXED — 11/18 pass (61.1%), MATIC strong, UNI weak |
| T70 FRESHNESS_COOLDOWN | NOT PROMOTED — long cooldowns cut convexity; keep=0 |
| T73 top-winner audit | GUARDRAIL SET — preserve top winners before any filter promotion |
| SIZE_MULT overlay | INERT — pure risk preference knob, not alpha |

## Parameters (Frozen — Production)

```text
TURTLE_EP=21, TURTLE_ATR_PERIOD=24, TURTLE_ATR_MULT=2.00, ATR_ENTRY_MULT=0.00,
HOLD_MAX=12, POSITION_CAP=3, FRESHNESS_COOLDOWN=0,
REGIME_ATR_PERIOD=17, REGIME_LOOKBACK=41, ATR_RANK_THRESHOLD=5.0,
VOL_LOOKBACK=92 (diagnostic-only; live rank gate rejected),
HEDGE_ATR_PERIOD=38, HEDGE_LOOKBACK=252, HEDGE_ATR_PCT=0.45, HEDGE_SIZE_MULT=0.55,
fee_pct=0.000400
```

## Resolved Concepts (Do Not Revisit)

- VOL_LOOKBACK live top-3 gate: rejected by T72
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
- Taker-buy pressure entry overlay: rejected
- Equity integration: REJECTED (equities hurt crypto portfolio)
- BTC trend scalar: REJECTED
- Correlation filter: REJECTED
- Rebalancing trim_losers: REJECTED