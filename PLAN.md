# PLAN.md — Krypto Research & Execution Plan

**State: 2026-04-27 09:53 UTC. T6-NEXT REJECTED (CTREND sleeve fails Sharpe criteria). T12 DONE (CP=7 held-out confirmed). T10 still pending. Live testnet BLOCKED on Noah's API keys.**

---

## Brutal Self-Assessment (2026-04-26 Critique)

**Inflated claims:**
- 83% OOS pass rate is optimistic. Real number probably 75-80% (sequential optimization on same OOS data)
- Daily equity Sharpe ~1.29 is honest — methodology verified
- $10K→$67M is real but 2018-2019 crypto was a unique hyper-bull regime

**Known weaknesses:**
- W05 (2025-2026 YTD choppy/bear) is the CURRENT LIVE regime and the strategy fails here
- ETH/SOL survive W05, BTC/XRP/ADA fail — partial hedge but not enough
- CTREND sleeve REJECTED — too costly on Sharpe

**What's real:**
- Anti-overfitting discipline (post-2026-04-25) is solid — caught EP=24 and EM=0.85 via held-out
- EP=21, CM=2.30, HM=12, ATR=24 are clean params with held-out confirmation
- CP=7 held-out confirmed (100% pass, Sharpe 1.38 — best among CP values)
- Maker-fill model validated (~70% maker fills)
- Edge generalizes to SPY/GLD (not crypto survivorship bias)

**Honest live expectation:** Sharpe ~1.0-1.1 in live conditions. 75-80% real pass rate.

---

## Production Params (FROZEN — all validated)

```
EP = 21              // ✅ held-out confirmed (T3): 27/29 pass
CHAND_PERIOD = 7     // ✅ held-out confirmed (T12): 100% pass, Sharpe 1.38
CHAND_MULT = 2.30    // ✅ 71-value dense sweep confirmed
HOLD_MAX = 12        // ✅ +71.4% Sharpe vs HM=45
ATR_ENTRY_MULT = 0.00 // ✅ held-out confirmed: 11/18 vs EM=0.85's 10/18
ATR_PERIOD = 24      // ✅ confirmed 3× — no further sweep
POSITION_CAP = 3
FRESHNESS_COOLDOWN = 0
MAX_SOL_POSITION = $50K notional
```

---

## Next 3 Execution Tasks

### T6-NEXT: CTREND Portfolio Sleeve — ✅ DONE (REJECTED)
**Result: FAIL — Sharpe cost too high**
- Turtle 75/25 CTREND(EMA8/32,hold=30): DD +3.6pp ✓ but Sharpe 1.38→0.33 (-76%)
- CTREND standalone Sharpe = -2.82 (loses money overall)
- 25% CTREND allocation destroys Turtle Sharpe. DD improvement doesn't compensate.
- **CTREND as fixed sleeve: REJECTED.** Dynamic/conditional switching: untested (could be viable).
- See `snapshots/t6_next_ctrend_sleeve_results.csv`, `examples/t6_next_ctrend_sleeve_walkforward.rs`

### T12: CP=7 Clean Held-Out Confirmation — ✅ DONE (CONFIRMED)
**Result: CP=7 WINS — 100% held-out pass, Sharpe 1.38 (best of all CP values)**
- 6 CP values × 6 WF windows × 5 symbols on pre-2021 held-out data
- CP=7: 30/30 (100%), Sharpe 1.38, DD 34%
- CP=11: 30/30 (100%), Sharpe 0.41 (3.4x less than CP=7)
- CP=42: 30/30 (100%), Sharpe -0.37 (negative)
- Production default CP=7 CONFIRMED. Held-out confirms OOS walk-forward result.
- Caveat: held-out dominated by easy 2017-2018 bull. 2019-2020 regime stress = 67.9%.
- See `snapshots/t12_cp_held_out_wf.csv`, `examples/t12_cp_held_out_v2.rs`

### T13: CTREND Regime-Conditional Switching — 🟡 NEW (HIGH)
**Concept:** Instead of fixed sleeve (which fails), use regime-conditional switching: when Turtle enters choppy regime → switch 25-50% to CTREND.
**Why this could work:** CTREND helps in choppy regimes (BTC W01: +7.4pp DDimp) but costs too much in trending regimes. Conditional switching avoids the trending-regime cost.
**Status:** Untested. Requires live data or cleaner backtest design.

### T10: HOF Generation Script — 🟡 OVERDUE (3+ sessions)
**Concept:** Parse `src/live/config.rs` + `examples/live_turtle_chandelier.rs` → auto-generate HALL_OF_FAME.md from code
**Why:** HOF has been manually updated and contradictory 5+ times. Source of truth should be code, not markdown.
**Status:** Three sessions overdue. Stop skipping.

---

## BLOCKED — Waiting on Noah

### T9: Live Testnet
Noah needs Binance testnet API keys. Without this, no live paper trading.
**This is the only remaining path to genuinely new knowledge.**

---

## Stop Doing

- **Re-running confirmed params:** ATR_PERIOD confirmed 3×. CHAND_MULT confirmed 2×. EP confirmed held-out. Stop.
- **CTREND fixed sleeve:** REJECTED — Sharpe cost too high. Conditional switching only.
- **Documentation-only sprints:** Last 10 commits: 4/10 docs/audit. Zero new capability.
- **Ignoring W05 live performance:** 2026 YTD = -22.7% while BTC +12.7%. The strategy is failing in the current live regime.

---

## Quick Fixes (Done/Verified)

✅ EP=24 REVERTED — in-sample inflation, held-out confirmed EP=21
✅ ATR_ENTRY_MULT=0.85 REVERTED — in-sample inflation, held-out confirmed EM=0.00
✅ CHAND_PERIOD backward search bug FIXED — production walk-forward uses forward search
✅ CP=42 REJECTED — backward search artifact, production default CP=7 unchanged
✅ ATR_PERIOD confirmed 3× — no further sweep
✅ CHAND_MULT dense sweep confirmed 2× — no further sweep
✅ T6-NEXT: CTREND fixed sleeve REJECTED — Sharpe destroyed
✅ T12: CP=7 held-out CONFIRMED — 100% pass, Sharpe 1.38

---

## Blind Spots

| Blind Spot | Severity | Status |
|-----------|----------|--------|
| W05 live performance (current regime) | CRITICAL | We're losing live. No solution yet (T13 pending). |
| CP=7 clean held-out test | HIGH | T12 — DONE, confirmed 100% |
| CTREND conditional switching | HIGH | T13 — NEW, untested |
| HOF generation script | MEDIUM | T10 — overdue, not technical |
| Live execution unknown | CRITICAL | BLOCKED on API keys |

---

## Graveyard Summary

All strategies below confirmed dead or secondary:

| Strategy | Result | Why |
|----------|--------|-----|
| EP=24 | REVERTED | In-sample inflation, failed held-out |
| ATR_ENTRY_MULT=0.85 | REVERTED | In-sample inflation, failed held-out |
| CP=42 | REJECTED | Backward search artifact |
| ATR_ENTRY_MULT>0 | REJECTED | All values degrade pass rate |
| ATR_PERIOD re-sweep | NULL | ATR=24 confirmed 3× |
| CHAND_MULT re-sweep | NULL | M=2.30 confirmed 2× |
| 4h multi-timeframe | GRAVEYARD | Structural failure (1/20 pass) |
| Cross-market equity integration | REJECTED | Combined -2.94 Sharpe vs crypto-only |
| DynamicTrend EMA signal | REJECTED | Turtle wins 21/24 windows |
| BollingerReversion | GRAVEYARD | 0/288 OOS pass |
| BOCPD regime detector | GRAVEYARD | 0% breaks |
| FDUSD basis carry | GRAVEYARD | 19% pass |
| Funding rate MR | GRAVEYARD | 43% pass |
| Vol-contingent Chandelier | GRAVEYARD | All configs identical |
| Position scaling overlays | GRAVEYARD | All failed — Chandelier sufficient |
| Regime switching | GRAVEYARD | All failed |
| CTREND 25% fixed sleeve | REJECTED | Sharpe destroyed 1.38→0.33 |

---

## Research Loop: What Remains

1. **T13:** CTREND regime-conditional switching (HIGH — untested)
2. **T10:** HOF generation script (MEDIUM — 3+ sessions overdue)
3. **T9:** Live testnet (BLOCKED on Noah's API keys)
