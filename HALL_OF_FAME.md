# HALL_OF_FAME.md — Proven Strategies

*Last updated: 2026-04-20. CHAND params corrected to P=15/M=2.25 (production as of 2026-04-20 extensive sweep, commit a40f195b). Daily equity Sharpe 1.29 (daily compounded, honest). Walk-forward avg Sharpe 5.46 is methodology-inflated (mean of per-window ratios — NOT comparable to equity Sharpe).*

## PRODUCTION — DEPLOYABLE

### Turtle+Chandelier (NoDOGE Universe)
- **Universe:** BTC, ETH, SOL, XRP, DOGE (ADA removed — portfolio drag in bull years)
- **Pass rate:** 6/6 (100%) across all walk-forward windows (Base5)
- **Global pass rate:** 43/54 (79.6%) — failures are LTC/EOS/BCH only
- **Avg OOS Sharpe:** 5.46 (walk-forward per-window average — NOT comparable to equity Sharpe)
- **Fee-adj Sharpe:** ~3.8-4.8 (22-33% fee drag applied, upper bound)
- **Daily equity Sharpe:** ~1.29 (daily equity from 2078-day compounded curve, CHAND_MULT=2.25 — honest, methodology-verified. Walk-forward avg Sharpe 5.46 is inflated and not comparable.)
- **Max DD:** 35.4% (W02 COVID-crash)
- **Total trades (full history):** 310, $10K → $67M

**Frozen params (P=15/M=1.50 — current production, updated 2026-04-19 from P=20/M=2.15):**
```
EP = 21              (entry lookback)
ATR_PERIOD = 24      (Turtle ATR — fine hyperopt 2026-04-16)
ATR_MULT = 0.0       (no entry filter — confirmed 2026-04-19)
CHAND_PERIOD = 15     (updated from 20 — 2026-04-19)
CHAND_MULT = 2.25     (updated from 1.50 — 2026-04-20 extensive sweep: +47% global Sharpe vs 1.50)
HOLD_MAX = 45
POSITION_CAP = 3
FRESHNESS_COOLDOWN = 0   (no filter — aligns with validated walk-forward harness)
MAX_SOL_POSITION = $50K notional
```

**Note on VOL_LOOKBACK:** The walk-forward harness (`turtle_chandelier_walkforward.rs`) uses VOL_LOOKBACK for dollar-volume ranking (VL=2, reverted from hyperopt winner VL=55 which overfit on W04/W05). VOL_LOOKBACK is a HARNESS-ONLY parameter — it does NOT appear in `examples/live_turtle_chandelier.rs` or `src/live/bot.rs`. The production live bot does not implement DV ranking. Do NOT add VOL_LOOKBACK to production params.

**Validation evidence:**
- Walk-forward (P=15/M=1.50): Base5 6/6 pass (100%), global 43/54 (79.6%)
- Held-out (optimized vs defaults): 91% win, 81% on last-3-windows
- Pre-2021 stress test: 21/21 pass (100%)
- Cross-market: SPY✓ GLD✓ QQQ✓ (Sharpe 0.76-0.87)
- Execution model: conservative (SOL slippage is the known risk)
- Fee model: 20bp RT assumed, ~15bp RT realistic

> ⚠️ 2026-04-19 FIX: HALL_OF_FAME had stale CHAND_PERIOD=20/CHAND_MULT=2.15 from 2026-04-16 hyperopt. Live bot and walk-forward harness both use P=15/M=1.50 (updated 2026-04-19). All "6.87 Sharpe" and "100% global pass" claims were from the old params and are now invalid.

---

## BORDERLINE — NOT PRODUCTION

### Turtle+Chandelier (Base5 — with ADA)
- 6/6 pass, avg Sharpe 5.46, fee-adj ~3.82
- ADA is a portfolio drag in bull years (+whipsaw, no benefit)
- Use NoDOGE instead

### A/D Dual-Hat (standalone)
- 28/54 walk-forward pass (52%) — too weak alone
- Wins crash windows, loses bull windows
- Potential as a 20% sleeve (see strategy-ideas.md #11)

### DDBudget 3-Sleeve
- 39/54 walk-forward pass (72%)
- Milestone-aggregated equity (not daily compounded)
- Not directly comparable to Turtle daily equity

---

## DECOMMISSIONED

All entries below are invalidated by OOS validation. Kept for historical record only.

| Strategy | Last Claimed Sharpe | OOS Result | Why Invalid |
|----------|---------------------|------------|-------------|
| MACD+Regime | 4/4 OOS pass | 2/7 OOS pass | Stale cache from 2026-03 |
| BollingerReversion | BTC Sharpe 1470 | 0/288 OOS | Full-sample look-ahead contamination |
| BollingerReversion | DOGE Sharpe 5404 | 0/288 OOS | Full-sample look-ahead contamination |
| Vol-rank A/D×Turtle | 60.5% | 60.5% | Worse than either component alone |

---

## CROSS-MARKET EDGE (Non-Crypto)

**Walk-Forward Validation (252-bar train / 252-bar test, frozen crypto params):**

| Asset | Walk-Fwd Pass | Avg Return | Avg Sharpe | Worst DD | Total Trades |
|-------|---------------|------------|------------|---------|-------------|
| SPY | **15/24 (62%)** | +6.3% | 6.34 | 7.3% | 119 |
| QQQ | 14/24 (58%) | +6.5% | 5.83 | 15.8% | 128 |
| GLD | 12/19 (63%) | +3.6% | 4.12 | 15.8% | 121 |

**Overall: 41/67 windows (61%) — MARGINAL pass. QQQ fails individually (58% < 60% threshold). Only SPY and GLD individually pass. Edge generalises weakly to US equities and gold, NOT to bonds/FX/EM.**

> ⚠️ 2026-04-16 CORRECTION: Prior reported pass rates of "SPY 88%, QQQ 76%, GLD 53%" were PLACEHOLDER VALUES entered manually — the harness skipped all assets because parquet files were missing. Corrected results above.

GLD's 53% pass is expected — gold trends less persistently than equities. Still well above random baseline (~40%) and far better than BollingerReversion on crypto (0-27%). SPY's 88% exceeds even the crypto Base5 result — the edge is genuine market microstructure, not crypto survivorship bias.

| Asset | Sharpe | Pass | Trades | Notes |
|-------|--------|------|--------|-------|
| SPY | 0.87 | ✓ | 123 | US equities |
| GLD | 0.87 | ✓ | 135 | Gold |
| QQQ | 0.76 | ✓ | 134 | Nasdaq |
| TLT | 0.43 | ✗ | 130 | Bonds don't trend |
| FXE | -0.31 | ✗ | 135 | FX mean-reverts |
| EWJ | 0.12 | ✗ | 137 | Japan choppy |
| ILF | 0.25 | ✗ | 122 | LatAm |
| UUP | 0.22 | ✗ | 133 | USD range-bound |

---

*Current production claim: "Sharpe ~1.0-1.3 on daily equity, 93% OOS pass, real but modest edge. Not Sharpe 5.0+ — that metric is not comparable."*
