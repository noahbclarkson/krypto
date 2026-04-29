# HALL_OF_FAME.md — Proven Strategies

_Auto-generated from production config + validated snapshots on 2026-04-29._
_Run `python3 scripts/gen_hof.py` to regenerate. Do not hand-edit headline metrics._

---

## PRODUCTION — DEPLOYABLE AFTER TESTNET

### Turtle+Chandelier / Turtle ATR live variant
- **Equity harness universe:** Base5 — BTC, ETH, SOL, XRP, DOGE, ADA
- **Live bot universe:** BTC, ETH, SOL, XRP, DOGE
- **Base5 walk-forward pass rate:** 6/6 (100.0%)
- **Global walk-forward pass rate:** 40/54 (74.1%) (9-universe, current validated harness)
- **Walk-forward avg Sharpe:** 3.147 (721 trades, per-window metric)
- **Daily equity Sharpe:** 1.04 (honest compounded-equity metric)
- **Validated daily equity:** $10K → $2,215,000 (221.5x)

**Important reconciliation:** The old `$10K → $67M` headline was a stale/full-sample artifact and is no longer cited. The authoritative current daily-equity number is `snapshots/progress_equity_curves.md`: 221.5x / Sharpe 1.04.

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
```

**Validation evidence:**
- Progress equity harness: 221.5x, daily Sharpe 1.04
- Walk-forward (Base5): 6/6 (100.0%)
- Walk-forward (global 9-universe): 40/54 (74.1%)
- Pre-2021 held-out stress: 19/28 (67.9%)
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
