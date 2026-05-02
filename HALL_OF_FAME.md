# HALL_OF_FAME.md — Proven Strategies

_Auto-generated from production config + validated snapshots on 2026-04-30._
_Run `python3 scripts/gen_hof.py` to regenerate. Do not hand-edit headline metrics._

---

## PRODUCTION — DEPLOYABLE AFTER TESTNET

### Turtle+Chandelier / Turtle ATR live variant
- **Equity harness universe:** Base5 — BTC, ETH, SOL, XRP, DOGE, ADA
- **Live bot universe:** BTC, ETH, SOL, XRP, DOGE
- **Base5 walk-forward pass rate:** 5/6 (83.3%)
- **Global walk-forward pass rate:** 34/54 (63.0%) (9-universe, current validated harness)
- **Walk-forward avg Sharpe:** 3.170 (743 trades, per-window metric)
- **Daily equity Sharpe:** 1.00 (honest compounded-equity metric)
- **Validated daily equity:** $10K → $1,241,000 (124.1x)

**Important reconciliation:** The old `$10K → $67M` headline was a stale/full-sample artifact and is no longer cited. The authoritative current daily-equity number is `snapshots/progress_equity_curves.md`: 124.1x / Sharpe 1.00.

**Frozen production params (from `src/live/config.rs`):**
```text
EP              = 21     // Turtle entry lookback
CHAND_PERIOD    = 7      // Stored in config; secondary validation layer
CHAND_MULT      = 2.30   // Stored in config; secondary validation layer
TURTLE_ATR_P    = 24     // Turtle ATR stop period
TURTLE_ATR_M    = 2.0    // Turtle ATR stop multiplier
ATR_ENTRY_MULT  = 0.00   // Entry filter — any non-zero degrades pass rate
HOLD_MAX        = 12     // Max hold bars
POSITION_CAP    = 3      // Max concurrent positions
REGIME_ATR_P    = 64     // hyperopt 2026-05-02: AP=64 wins on live Turtle-only path. 55/63 pass, Sharpe 6.19 vs AP=12 (53/63, 5.43). Extensive sweep AP∈[5..=80 step 1] × 9 universes × 7 WF windows. AP=64 sharp peak (AP=65 cliff at Sharpe 3.07). AP=39 tied backup at 55/63.
REGIME_LOOKBACK = 42     // BTC ATR percentile lookback
ATR_RANK_THRESH = 24.0   // hyperopt 2026-05-01: T=24 wins T=5 52/63 pass (82.5%) vs 45/63 (71.4%). ALL 9 universes 9-0 on OOS Sharpe. T=24 plateau T=24-27 identical.
```

**Validation evidence:**
- Progress equity harness: 124.1x, daily Sharpe 1.00
- Walk-forward (Base5): 5/6 (83.3%)
- Walk-forward (global 9-universe): 34/54 (63.0%)
- Pre-2021 held-out stress: 19/28 (67.9%)
- T22 exit attribution: Chandelier adds secondary robustness; live bot currently uses Turtle ATR as sole live exit
- 2026-05-01 caveat: live Turtle-only exit implementation was bug-fixed (`highest_high - ATR`, ATR buffer seeded with `TURTLE_ATR_PERIOD`, HOLD_MAX enforced independently). Prior Turtle-only live-path metrics need revalidation under corrected semantics.
- ATR_RANK=24: PRODUCTION DEFAULT — live bot validated 52/63 pass (82.5%), Sharpe 5.590, +132.3% avg return. T=24 beats T=5 in ALL 9 universes 9-0 on OOS Sharpe. Promoted from candidate after live_compatible_wf re-run (2026-05-01).
- Cross-market: SPY✓ GLD✓ QQQ✓ (Sharpe 0.76–0.87)

**Fee model:** 0.04% taker fee in live dry-run; prior execution realism suggested ~22–33% Sharpe degradation under realistic costs.

**Critical blocker:** Binance testnet API key + secret. All metrics remain simulation upper bounds until corrected live-path revalidation and 30-day testnet paper trading run.

---

## BORDERLINE — NOT PRODUCTION

### Turtle+Chandelier (Base5 — with ADA)
- ADA has been a portfolio drag in bull years (+whipsaw, no benefit). Live bot excludes ADA.

### A/D Dual-Hat (standalone)
- 52% walk-forward pass — too weak alone. Potential as a 20% sleeve.

### DDBudget 3-Sleeve
- 72% walk-forward pass. Milestone-aggregated equity (not daily compounded).

---

## DECOMMISSIONED

See `GRAVEYARD.md` for full list. Key invalidations:

| Strategy | Why Invalid |
|----------|-------------|
| EP=24 | In-sample inflation — held-out confirmed EP=21 wins |
| ATR_ENTRY_MULT=0.85 | In-sample inflation — held-out confirmed EM=0.00 wins |
| CP=42 | Backward search artifact |
| CTREND 25% fixed sleeve | Sharpe destroyed 1.38→0.33 |
| MACD+Regime | Stale cache, OOS 2/7 pass |
| BollingerReversion | Full-sample look-ahead contamination, 0/288 OOS |
| Regime switching | All configs fail |
| Position scaling overlays | All failed — equal capital wins |

---

## CROSS-MARKET EDGE (Non-Crypto)

- SPY: valid (Sharpe ~0.76)
- GLD: valid (Sharpe ~0.81)
- QQQ: valid (Sharpe ~0.87)

---

## Source Files (Authoritative)

| File | Contents |
|------|----------|
| `src/live/config.rs` | Production constants — frozen params |
| `examples/live_turtle_chandelier.rs` | Live dry-run / testnet entrypoint |
| `examples/progress_equity_curves.rs` | Honest daily-equity progress harness |
| `snapshots/progress_equity_curves.md` | Current daily equity + Sharpe source of truth |
| `snapshots/turtle_chandelier_9way_wf_latest.md` | Current walk-forward validation source |
