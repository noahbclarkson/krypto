# HALL_OF_FAME.md — Proven Strategies

_Auto-generated from production config + validated snapshots on 2026-05-04._
_Run `python3 scripts/gen_hof.py` to regenerate. Do not hand-edit headline metrics._

---

## PRODUCTION — DEPLOYABLE AFTER TESTNET

### Turtle+Chandelier / Turtle ATR live variant
- **Equity harness universe:** Base5 — BTC, ETH, SOL, XRP, DOGE, ADA
- **Live bot universe:** BTC, ETH, SOL, XRP, DOGE
- **Base5 walk-forward pass rate:** 6/6 (100.0%)
- **Global walk-forward pass rate:** 37/54 (68.5%) (9-universe, current validated harness)
- **Walk-forward avg Sharpe:** 4.241 (724 trades, per-window metric)
- **Daily equity Sharpe:** 0.99 (honest compounded-equity metric)
- **Validated daily equity:** $10K → $869,000 (86.9x, dual-exit Turtle+Chandelier; fee-corrected 2026-05-04 T56)
  **Important:** prior 113.6x was inflated by fee sign bug (`entry_px*(1-TAKER_FEE)` made entry cheaper). Fixed: `entry_px*(1+TAKER_FEE)` → 86.9x. Sharpe recalculated from daily compounded returns.

The authoritative current daily-equity number is from `progress_equity_curves.rs` (fee-corrected 2026-05-04):
- Dual-exit Turtle+Chandelier (no filter): **86.9x / Sharpe 0.43**
- Dual-exit Turtle+ATR_RANK=5 (production config): **33.9x / Sharpe 0.42**

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
REGIME_ATR_P    = 12     // BTC ATR period for regime filter
REGIME_LOOKBACK = 42     // BTC ATR percentile lookback
ATR_RANK_THRESH = 5.0    // Minimum BTC ATR percentile rank for entries
```

**Validation evidence:**
- Progress equity harness (dual exit, no filter): 86.9x, daily Sharpe 0.43 [fee-corrected T56]
- Progress equity harness (dual exit, ATR_RANK=5): 33.9x, daily Sharpe 0.42 [fee-corrected T56]
- Walk-forward Turtle-only (T=5, VL=96): 55/63 pass (87.3%), Sharpe 4.910, Base5 622.98x aggregate
- Walk-forward (Base5): 6/6 (100.0%) — walk-forward 37/54 (68.5%) (9-universe)
- Pre-2021 held-out stress: T=5 14/22 pass, Sharpe +0.664 (best of all T values tested)
- ATR_RANK=65 REJECTED (held-out: 10/22 pass, Sharpe -1.900). T=24 REJECTED (10/22, -0.964). Only T=0/5 survive held-out.
- T22 exit attribution: Chandelier adds secondary robustness; live bot currently uses Turtle ATR as sole live exit
- Cross-market: SPY✓ GLD✓ QQQ✓ (Sharpe 0.76–0.87)

**Fee model:** 0.04% taker fee in live dry-run; prior execution realism suggested ~22–33% Sharpe degradation under realistic costs.

**Critical blocker:** Binance testnet API key + secret. All metrics remain simulation upper bounds until 30-day testnet paper trading runs.

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
