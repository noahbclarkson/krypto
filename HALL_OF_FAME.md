# HALL_OF_FAME.md — Proven Strategies

*Last updated: 2026-04-16. Archive of validated production candidates.*

## PRODUCTION — DEPLOYABLE

### Turtle+Chandelier (NoDOGE Universe)
- **Universe:** BTC, ETH, SOL, XRP, DOGE (ADA removed — portfolio drag in bull years)
- **Pass rate:** 6/6 (100%) across all walk-forward windows
- **Avg OOS Sharpe:** 6.87 (walk-forward per-window average — NOT comparable to equity Sharpe)
- **Fee-adj Sharpe:** ~4.8 (22-33% fee drag applied)
- **Daily equity Sharpe:** ~1.34 (honest metric — use on charts)
- **Max DD:** 35.4% (W02 COVID-crash)
- **Total trades (full history):** 310, $10K → $67M

**Frozen params (as of 2026-04-14):**
```
EP = 21          (entry lookback)
ATR_PERIOD = 25  (Turtle ATR)
ATR_MULT = 0.0   (no entry filter)
CHAND_PERIOD = 28
CHAND_MULT = 2.00
HOLD_MAX = 45
POSITION_CAP = 3
MAX_SOL_POSITION = $50K notional
```

**Validation evidence:**
- Held-out (optimized vs defaults): 91% win, 81% on last-3-windows
- Pre-2021 stress test: 21/21 pass (100%)
- Cross-market: SPY✓ GLD✓ QQQ✓ (Sharpe 0.76-0.87)
- Execution model: conservative (SOL slippage is the known risk)
- Fee model: 20bp RT assumed, ~15bp RT realistic

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

**Verdict:** Edge generalizes to US equities and gold. NOT to bonds/FX/EM.

---

*Current production claim: "Sharpe ~1.0-1.3 on daily equity, 93% OOS pass, real but modest edge. Not Sharpe 5.0+ — that metric is not comparable."*
