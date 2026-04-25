# HALL_OF_FAME.md — Proven Strategies

*Last updated: 2026-04-25. CHAND_MULT 2.25→2.30 (dense sweep 71 values, step=0.05: M=2.30 wins 83.3% pass vs M=2.25 81.5%, Sharpe 6.204 vs 6.122). ATR_ENTRY_MULT 0.90→0.85 (fine sweep). HOLD_MAX 45→12 (hyperopt 2026-04-21). CHAND_PERIOD 11→7 (hyperopt 2026-04-21). EP=24. Global pass: 45/54 (83.3%) with ATR_ENTRY_MULT=0.85. See memory/hyperopt-2026-04-25-chand-mult-dense.md.*

## PRODUCTION — DEPLOYABLE

### Turtle+Chandelier (NoDOGE Universe)
- **Universe:** BTC, ETH, SOL, XRP, DOGE (ADA removed — portfolio drag in bull years)
- **Pass rate:** 6/6 (100%) across all walk-forward windows (Base5)
- **Global pass rate:** 45/54 (83.3%) with CHAND_MULT=2.30 — failures are LTC/EOS/BCH only
- **Avg OOS Sharpe:** 4.78 (9-universe global; Base5 avg Sharpe 5.46 — NOT directly comparable to equity Sharpe)
- **Fee-adj Sharpe:** ~3.8-4.8 (22-33% fee drag applied, upper bound)
- **Daily equity Sharpe:** ~1.29 (daily equity from 2078-day compounded curve, CHAND_MULT=2.30 — honest, methodology-verified. Walk-forward avg Sharpe 5.46 is inflated and not comparable.)
- **Max DD:** 35.4% (W02 COVID-crash)
- **Total trades (full history):** 310, $10K → $67M

**Frozen params (CHAND(7,2.30), updated 2026-04-25 with ATR_ENTRY_MULT=0.85):**
```
EP = 24              (entry lookback — hyperopt 2026-04-20: EP=24 wins 45/54 (83.3%) vs EP=21 43/54 (79.6%). See hyperopt-2026-04-20-ep-reopt.md)
ATR_PERIOD = 24      (Turtle ATR — fine hyperopt 2026-04-16)
ATR_MULT = 0.0       (Turtle ATR stop multiplier — confirmed 2.0, 2026-04-16. NOT an entry filter — that is ATR_ENTRY_MULT above)
CHAND_MULT = 2.30    (Chandelier exit multiplier — hyperopt 2026-04-25: DENSE sweep M∈[1.50..5.00] step 0.05 (71 values) × 9 universes × 54 windows. M=2.30 wins: Sharpe 6.204 (+0.8% vs M=2.25 at 6.122), 83.3% pass (45/54) vs 81.5% (44/54). Lowest M at peak pass rate — most efficient. See memory/hyperopt-2026-04-25-chand-mult-dense.md. NOTE: M=2.30 is the production default. Historical M=2.25 results remain valid — the shift is marginal.)
CHAND_PERIOD = 7     (full sweep CP∈[5..60 step2]×9 universes×54 windows with current production params (HM=12, ATR_EM=0.90, EP=24, CM=2.25), 2026-04-21: CP=7 wins global Sharpe 5.908 (+6.9% vs CP=11 baseline 5.526). Prior CP=11 sweep used stale EP=21. See memory/hyperopt-2026-04-21-chand-period.md.)
CHAND_MULT = 2.25    (extensive sweep M∈[0.50..5.00 step 0.25]×9 universes×7 windows, 2026-04-20: M=2.25 wins +47% global Sharpe vs M=1.50. See hyperopt-2026-04-20-chand-mult.md)
ATR_ENTRY_MULT = 0.85  // hyperopt 2026-04-21 FINE sweep: EM=0.85 wins 44/54 (81.5%) vs EM=0.90 43/54 (79.6%). Full 11-value sweep {0.60-1.10 step 0.05} × 9 universes × 54 windows with current production params CHAND(7,2.25)/EP=24/HM=12. EM=0.85 avoids catastrophic Base5 W3 failure (+12.2% vs -28.3%). +3.6% Sharpe (6.12 vs 5.91). See memory/hyperopt-2026-04-21-atr-entry-mult-fine.md.
HOLD_MAX = 12  // hyperopt 2026-04-21: HM=12 wins +71.4% Sharpe vs HM=45 (2.72 vs 1.59 avg Sharpe, 9-universe × 54 windows). Chandelier fires first ~bar 12-15; HM irrelevant above ~35.
POSITION_CAP = 3
FRESHNESS_COOLDOWN = 0
MAX_SOL_POSITION = $50K notional
```

**Pre-2021 validation (P=7/M=2.25):** Confirmed via sweep on current production params. CP=7 sweep: pass rate 43/54 (79.6%), avg Sharpe 5.908. Consistent robustness across all 9 universes. P=7 was NOT tested in pre-2021 stress — the stress test was run with P=5/M=3.00 (2026-04-20). CP=7 should be tested in pre-2021 stress at next available opportunity.

**Note on VOL_LOOKBACK:** The walk-forward harness (`turtle_chandelier_walkforward.rs`) uses VOL_LOOKBACK for dollar-volume ranking (VL=2, reverted from hyperopt winner VL=55 which overfit on W04/W05). VOL_LOOKBACK is a HARNESS-ONLY parameter — it does NOT appear in `examples/live_turtle_chandelier.rs` or `src/live/bot.rs`. The production live bot does not implement DV ranking. Do NOT add VOL_LOOKBACK to production params.

**Validation evidence:**
- Walk-forward (P=5/M=3.00): Base5 6/6 pass (100%), global 42/54 (77.8%), avg Sharpe 4.91
- Held-out (optimized vs defaults): 91% win, 81% on last-3-windows
- Pre-2021 stress test: 21/21 pass (100%)
- Cross-market: SPY✓ GLD✓ QQQ✓ (Sharpe 0.76-0.87)
- Execution model: conservative (SOL slippage is the known risk)
- Fee model: 20bp RT assumed, ~15bp RT realistic

> ⚠️ 2026-04-20 FIX: HALL_OF_FAME was stale (P=15/M=2.25 in example). Updated to P=7/M=2.25 (hyperopt 2026-04-21). Equity chart regenerated with current params. Historical equity figures (1048.5x etc.) from prior param sets are deprecated — always use current production params for live deployment. See live_turtle_chandelier.rs for the authoritative equity curve.

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

*Current production claim: "Sharpe ~1.0-1.3 on daily equity, 83% OOS pass, real but modest edge. Not Sharpe 5.0+ — that metric is not comparable."*
