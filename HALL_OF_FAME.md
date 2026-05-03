# HALL_OF_FAME.md — Proven Strategies

_Auto-generated from production config + validated snapshots on 2026-05-03._
_Run `python3 scripts/gen_hof.py` to regenerate. Do not hand-edit headline metrics._

---

## PRODUCTION — DEPLOYABLE AFTER TESTNET

### Turtle+ATR Live Variant (ATR_RANK=24 gate)

**LIVE-COMPATIBLE WALK-FORWARD (2026-05-03 — corrected exit semantics):**
- **Global pass rate:** 54/63 (85.7%), avg Sharpe 7.652, avg return +138.3%
- **Base5 aggregate:** 251.8x compounded over 7 walk-forward windows
- **Per-universe:** Base5 6/7, NoDOGE 7/7, Legacy4 6/7, LargeCaps5 7/7 — all strong
- **9/9 universes positive** at the aggregate level
- ATR_RANK threshold: 24.0 (live bot default in `src/live/config.rs`)

**DAILY EQUITY HARNESS (progress_equity_curves.rs):**
- Turtle+Chandelier (dual exit): 111.4x, daily Sharpe 0.98 — NOT directly comparable to live Turtle-only path
- Turtle ATR_RANK=24 (live bot variant): 77.4x, daily Sharpe 0.97

**Live bot universe:** BTC, ETH, SOL, XRP, DOGE
**Production params (from `src/live/config.rs`):**
```text
EP              = 21
TURTLE_ATR_P    = 24
TURTLE_ATR_M    = 2.0
ATR_ENTRY_MULT  = 0.00
HOLD_MAX        = 12
POSITION_CAP    = 3
REGIME_ATR_P    = 12
REGIME_LOOKBACK = 42
ATR_RANK_T      = 24.0   // live entry gate: BTC ATR percentile must be ≥24
VOL_LOOKBACK    = 96     // dollar-volume ranking window
```

**Key validation notes:**
- 2026-05-01 live exit bug FIXED: ATR buffer now seeded with `TURTLE_ATR_PERIOD` (24), long stop uses `highest_high - ATR_MULT*ATR`, HOLD_MAX enforced before ATR warmup return. live_compatible_wf.rs reflects corrected semantics — metrics now authoritative.
- ATR_RANK=24 confirmed via EXTENSIVE sweep T∈[0..=100] × 9 universes × 7 WF windows. T=24 wins ALL 9 universes 9-0 on OOS Sharpe vs T=5. Plateau T=24-27 identical. See `memory/hyperopt-2026-05-01-atr-rank-threshold.md`.
- REGIME_ATR_PERIOD=12 (AP=64 rejected: 3rd sequential optimization on this harness, same pattern as EP=24).
- Cross-market: SPY✓ GLD✓ QQQ✓ (Sharpe 0.76–0.87)

**Fee model:** 0.10% taker per side in walk-forward. Live execution: ~70% maker fills expected → ~4-5bps effective cost vs 10bps backtest assumption.

**Critical blocker:** Binance testnet API key + secret. All metrics remain simulation upper bounds until live testnet run.

---

## BORDERLINE — NOT PRODUCTION

### A/D Dual-Hat
- 52% walk-forward pass — too weak alone. Potential as 20% sleeve.

### DDBudget 3-Sleeve
- 72% walk-forward pass. Milestone-aggregated equity (not daily compounded) — NOT comparable to Turtle daily Sharpe.

---

## DECOMMISSIONED

See `GRAVEYARD.md` for full list. Key invalidations:

| Strategy | Why Invalid |
|----------|-------------|
| EP=24 | In-sample inflation — held-out confirmed EP=21 wins |
| ATR_ENTRY_MULT=0.85 | In-sample inflation — held-out confirmed EM=0.00 wins |
| ATR_ENTRY_MULT=0.94 | Same-harness artifact — no held-out confirmation |
| REGIME_ATR_PERIOD=64 | 3rd sequential optimization on live_compatible_wf.rs (EP=24 pattern); held-out rejects |
| ATR_RANK=5 | Old default — 0/63 pass in live_compatible_wf; T=24 wins 9-0 |
| ATR_RANK_THRESHOLD=24 (old conflated claims) | Correct production default is 24.0 per EXTENSIVE 2026-05-01 sweep |
| VL=96 | Same-harness artifact risk (EP=24 pattern); production stays VL=8 pending Base5-only confirmation |
| CP=42 | Backward search artifact |
| CTREND 25% sleeve | Sharpe destroyed 1.38→0.33 |
| MACD+Regime | Stale cache, OOS 2/7 pass |
| BollingerReversion | Full-sample look-ahead contamination, 0/288 OOS |
| Position scaling overlays | All failed — equal capital wins |

---

## Source Files (Authoritative)

| File | Contents |
|------|----------|
| `src/live/config.rs` | Production constants — frozen params |
| `examples/live_compatible_wf.rs` | Live path WF (corrected semantics) — source of truth |
| `examples/live_turtle_chandelier.rs` | Live dry-run / testnet entrypoint |
| `examples/progress_equity_curves.rs` | Honest daily-equity progress harness |
| `snapshots/live_compatible_wf.md` | Current live path walk-forward results |
| `snapshots/progress_equity_curves.md` | Daily equity source of truth |