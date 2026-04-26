# HALL_OF_FAME.md — Proven Strategies

_Auto-generated from `src/live/config.rs` + `examples/live_turtle_chandelier.rs`
on 2026-04-26. DO NOT EDIT MANUALLY — edit source files and regenerate._

---

## PRODUCTION — DEPLOYABLE

### Turtle+Chandelier (NoDOGE Universe)
- **Universe:** BTC, ETH, SOL, XRP, DOGE (ADA removed — portfolio drag in bull years)
- **Base5 pass rate:** 6/6 (100%)
- **Global pass rate:** 45/54 (83%) (9-universe)
- **Daily equity Sharpe:** ~1.29 (honest, methodology-verified)
- **Max DD:** 35.4% (W02 COVID-crash)
- **Historical equity:** $10K → $67M (310 trades)

**Frozen production params (from `src/live/config.rs`):**
```
EP              = 21     // Turtle entry lookback
CHAND_PERIOD    = 7  // Chandelier ATR period
CHAND_MULT      = 2.30   // Chandelier ATR multiplier (71-value dense sweep)
TURTLE_ATR_P    = 24    // Turtle ATR stop period
TURTLE_ATR_M    = 2.0    // Turtle ATR stop multiplier
ATR_ENTRY_MULT  = 0.00    // Entry filter — any non-zero degrades pass rate
HOLD_MAX        = 12      // Max hold bars (Chandelier fires ~bar 12-15)
POSITION_CAP    = 3     // Max concurrent positions
```

**Validation evidence:**
- Walk-forward (Base5): 6/6 (100%)
- Walk-forward (global 9-universe): 45/54 (83%)
- Pre-2021 held-out stress: 19/28 (67.9%)
- Cross-market: SPY✓ GLD✓ QQQ✓ (Sharpe 0.76–0.87)

**Fee model:** ~0.04% RT taker, realistic ~0.02% RT. Fee-adj Sharpe ≈ 3.1–3.7.

**⚠️ VOL_LOOKBACK is harness-only.** The walk-forward harness uses VOL_LOOKBACK for dollar-volume ranking. This is a HARNESS parameter — NOT in production code. Production `src/live/bot.rs` does not use DV ranking.

---

## BORDERLINE — NOT PRODUCTION

### Turtle+Chandelier (Base5 — with ADA)
- ADA is a portfolio drag in bull years (+whipsaw, no benefit). Use NoDOGE instead.

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
| Position scaling overlays | All failed — Chandelier sufficient |

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
| `examples/live_turtle_chandelier.rs` | Strategy logic + equity figures |
| `examples/turtle_chandelier_walkforward.rs` | Validation harness (VOL_LOOKBACK is harness-only) |

_Run `python3 scripts/gen_hof.py` to regenerate this file._
