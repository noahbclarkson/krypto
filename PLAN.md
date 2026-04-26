# PLAN.md — Krypto Research & Execution Plan

**State: 2026-04-26 14:37 UTC. Anti-overfitting discipline SOLID. Research loop CLOSED except T11/S4. Live testnet BLOCKED on API keys. W05 live failure is the #1 unfixed problem.**

---

## Brutal Self-Assessment (2026-04-26 Critique — Revised)

**What we got right:**
- Anti-overfitting: EP=24 + ATR_EM=0.85 properly rejected for in-sample inflation. Held-out process is correct.
- T10 HOF script: code is source of truth, not markdown.
- Honest equity Sharpe (~1.29) vs inflated walk-forward Sharpe (6.29).
- Cross-market audit: edge generalizes to SPY/GLD/QQQ (Sharpe 0.76-0.87). Not crypto survivorship bias.

**Where we're fooling ourselves:**
- 83% global pass = bull-era weighted. Pre-2021 stress = 67.9% — the honest number for choppy/bear regimes.
- **W05 live failure (-22.7% YTD vs BTC +12.7%)** is the real story. The strategy is losing in the current live regime. Walk-forward "100% Base5 pass" doesn't cover this.
- No diversification: all eggs in Turtle+Chandelier. CTREND T6/T6-NEXT/T13 all FAILED.
- Fee model optimistic: 70% maker fill assumption unverified in live. Could be 40-50% in fast markets.
- 13/54 failing windows: unclassified (asset-specific vs regime-wide). T11 never ran.

**Anti-overfitting enforcement: at risk.** We keep re-sweeping confirmed params (ATR_PERIOD 3×, CHAND_MULT 2×, CHAND_PERIOD 2×, EP 2×). Need hard stop rule.

---

## Production Params (FROZEN — all validated, do NOT re-sweep)

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

**DO NOT re-sweep these params. They are confirmed.**

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

### T11: Failure Mode Diagnostic — HIGH PRIORITY (✅ DONE)
**Question:** 13/54 global walk-forward windows fail. Are they asset-specific (LTC/EOS/BCH only) or regime-wide (2+ production symbols fail)?
**Test:** For each failing window, report per-symbol pass/fail. Classify each as:
- Asset-specific: only LTC/EOS/BCH fail → production universe is clean
- Regime-wide: 2+ production symbols (BTC/ETH/SOL/XRP/DOGE) fail → live tail-risk problem
**Decision:** If ≥3 regime-wide failures → CTREND sleeve becomes urgent (portfolio protection). If all 13 are asset-specific → production universe is clean.
**Why this matters:** W05 live failure (-22.7% YTD) might be a regime-wide failure in disguise.

### S4: Vol-Adaptive Chandelier (Structural Rethink) — MEDIUM (✅ DONE -- GRAVEYARD)
**Concept:** Current Chandelier(P=7, M=2.30) is static. In choppy high-vol regimes (like 2026 YTD), the tight multiplier fires constantly causing whipsaw losses.
**Idea:** Use 252-bar realized vol rank (our standard ATR baseline):
- Vol > 75th pctile → M=3.0+ (wider stop, holds through noise)
- Vol < 25th pctile → M=1.75 (tighter stop)
**Why this might work:** Previous vol-contingent test (2026-04-12) FAILED because 21-bar vol rank is too slow-moving. 252-bar rank matches the ATR baseline and may capture the slow-moving vol regime shift we're looking for.
**Test:** Chandelier P=7, M ∈ {1.75, 2.30, 3.00} conditional on 252-bar vol percentile. Compare to static M=2.30 on Base5 (6 windows).

### T9: Live Testnet — BLOCKED (Noah's API keys needed)
This is the only path to genuinely new knowledge. Nothing else matters until we get live fill data.
- Measure actual maker fill rate per symbol (expect 70%, could be 40-50% in fast markets)
- Compare live equity curve to backtest prediction
- Detect regime drift before it kills the portfolio

---

## Stop Doing

- **Re-running confirmed params:** ATR_PERIOD 3×, CHAND_MULT 2×, CHAND_PERIOD 2×, EP 2×. STOP.
- **CTREND fixed sleeve:** T6-NEXT REJECTED (Sharpe 1.38→0.33). T13 REJECTED (67% pass). CTREND as sleeve is dead.
- **4h multi-timeframe:** Confirmed structural failure (1/20 pass). GRAVEYARD.
- **Research loop spin:** Every session without live data is optimization without validation. Only run T11/S4 if API keys remain blocked.
- **Documentation-only sprints:** Last 5 commits: 3/5 docs/audit. Zero new capability.

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
| **W05 live failure (current regime)** | CRITICAL | Strategy losing live. No solution identified. T11 needed to classify. |
| **No diversification** | CRITICAL | All eggs in Turtle+Chandelier. CTREND T6/T6-NEXT/T13 all FAILED. |
| **Fee model uncertainty** | HIGH | 70% maker fill unverified in live. Could be 40-50% in fast markets. |
| **T11 failure mode unclassified** | HIGH | 13/54 failures: asset-specific vs regime-wide? |
| **S4 vol-adaptive Chandelier** | MEDIUM | Previous attempt failed (21-bar rank too fast). 252-bar untested. |
| **Live testnet blocked** | CRITICAL | API keys needed. Nothing else matters until unblocked. |

---

### T11: Failure Mode Diagnostic — HIGH PRIORITY (✅ DONE)
**Question:** 13/54 global walk-forward windows fail. Are they asset-specific (LTC/EOS/BCH only) or regime-wide (2+ production symbols fail)?
**Test:** For each failing window, report per-symbol pass/fail. Classify each as:
- Asset-specific: only LTC/EOS/BCH fail → production universe is clean
- Regime-wide: 2+ production symbols (BTC/ETH/SOL/XRP/DOGE) fail → live tail-risk problem
**Why this matters:** W05 live failure (-22.7% YTD) might be a regime-wide failure in disguise. If ≥3 regime-wide failures found, CTREND sleeve becomes urgent portfolio protection.

### S4: Vol-Adaptive Chandelier (Structural Rethink) — MEDIUM (✅ DONE -- GRAVEYARD)
**Concept:** Current Chandelier(P=7, M=2.30) is static. In choppy high-vol regimes, the tight multiplier fires constantly causing whipsaw.
**Idea:** Use 252-bar realized vol rank (matching ATR baseline):
- Vol > 75th pctile → M=3.0+ (wider stop, holds through noise)
- Vol < 25th pctile → M=1.75 (tighter stop)
**Why now:** Previous vol-contingent test (2026-04-12) FAILED because 21-bar rank is too slow-moving. 252-bar rank matches ATR baseline and may behave differently.

### T9: Live Testnet — BLOCKED on API keys
**This is the only path to genuinely new knowledge.**

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
| Vol-contingent Chandelier | GRAVEYARD | All configs identical (21-bar rank too fast) |
| Position scaling overlays | GRAVEYARD | All failed — Chandelier sufficient |
| Regime switching | GRAVEYARD | All failed |
| CTREND 25% fixed sleeve | REJECTED | Sharpe destroyed 1.38→0.33 |
| CTREND conditional switching | REJECTED | 67% pass < 75% threshold |

---

## Research Loop: What Remains

1. **T11:** Failure mode diagnostic — DONE: all failures asset-specific, production CLEAN (HIGH, done)
2. **S4:** Vol-adaptive Chandelier with 252-bar vol rank — GRAVEYARD: no benefit (MEDIUM, done)
3. **T9:** Live testnet (BLOCKED on Noah's API keys)
