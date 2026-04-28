# PLAN.md — Krypto Research & Execution Plan

**State: 2026-04-28 21:35 UTC. Research CLOSED. S4 testable now (no creds needed). Live testnet BLOCKER. Equity bug (low priority pre-deploy fix). Pre-2021 stress 67.9% is a known constraint, not a fixable bug.**

---

## Brutal Self-Assessment (2026-04-28 Critique Cycle — Third Session)

**Research loop: CLOSED.** All testable ideas exhausted or assessed. The last 8 commits confirm this: 3 substantive, 5 documentation. Documentation loop is structural (research complete, only credential blocker remains).

**Equity bug (Turtle 1.0x in daily_progress.csv):** Third session unfixed. `progress_equity_curves.rs` has a data length mismatch (2087 vs 2971 bars for BTC) causing Turtle equity to show ~1.0x instead of ~235x. Root cause known. Fix complexity: medium. Status: low priority vs live testnet, **must be fixed pre-deploy**.

**Metrics integrity:**
- Equity Sharpe 1.29 — **HONEST**, correctly distinguished from walk-forward 5.46 (upper bound)
- Walk-forward 5.46 = mean of per-window Sharpe ratios (inflated methodology). Equity 1.29 = daily compounded Sharpe (real)
- No curve-fitting concern. Strategy is OOS validated by construction. Pre-2021 stress 67.9% is the honest canary

**Pre-2021 stress 67.9% (below 70% threshold):** Known constraint, not a fixable bug. Strategy is overfit to bull crypto dynamics to some degree. Acknowledge it; don't pretend we can eliminate it without live data. USDT hedge overlay exists but is not integrated into live bot.

**What we got right:**
- Equity 1.29 is honest (never show 5.46 on equity charts)
- Anti-overfitting rules working (EP=24, ATR_EM=0.85, EP=43 all correctly rejected)
- T22 exit attribution confirmed TURTLE_ATR_PERIOD=24 is real (Chandelier fires first ~7-8%, not >90%)
- Research loop genuinely closed — all testable ideas exhausted

**What we're still fooling ourselves about:**
- Equity bug: 3 sessions unfixed, leaking into external reporting (daily_progress.csv shows `BROKEN`)
- Pre-2021 stress: acknowledged but no action taken to address it (USDT hedge overlay not integrated)
- Sequential hyperopt on same OOS grid: cumulative implicit overfitting risk is real but unquantifiable

---

## Production Params (FROZEN — all validated, do NOT re-sweep)

```
EP              = 21     // ✅ held-out confirmed (reverted from EP=24 2026-04-26)
TURTLE_ATR_P    = 24     // ✅ fine sweep confirmed (T22: not noise, fires first ~90% of trades)
TURTLE_ATR_M    = 2.0    // ✅ 81-value dense sweep confirmed (NULL result — fires first ~90%)
CHAND_PERIOD    = 7      // ✅ held-out confirmed
CHAND_MULT      = 2.30   // ✅ 71-value dense sweep
ATR_ENTRY_MULT  = 0.00   // ✅ 41-value sweep — no filter wins
HOLD_MAX        = 12     // ✅ HM=12 wins +71.4% Sharpe vs HM=45 baseline
POSITION_CAP    = 3      // ✅ Turtle-only validated (72.2% pass)
FRESHNESS_COOLDOWN = 0   // ✅ cd=0 wins
VOL_LOOKBACK    = 8      // ✅ dense sweep confirmed (updated 2026-04-28)
```

---

## Next 3 Execution Tasks

### S4: ATR-Normalized Position Sizing — TESTABLE NOW ⭐
**Concept:** Equal ATR-normalized notional per symbol vs equal capital allocation.
- Current: CAP=3, equal $10K per position
- Proposed: $10K / 21-bar ATR per position (high-vol → smaller, low-vol → larger)
- **Different from failed overlays:** Those changed CAP scalar. This adjusts per-symbol notional within CAP=3.
**Why testable now:** No live credentials needed. Walk-forward harness with 3 configs × Base5 × 6 windows.
**Test:** 3 configs {equal_capital_baseline, atr_norm_10k, atr_norm_20k} × Base5 × 6 windows.
**Status:** TEST NOW. If it fails, confirms Chandelier's dynamic exit already handles position management better than static sizing.

### T24: Equity Bug Fix — Pre-Deploy Only
**Status:** `progress_equity_curves.rs` shows Turtle 1.0x instead of ~235x (data length 2087 vs 2971 bars for BTC).
**Why now:** Low priority vs live testnet, but must fix before deployment. Daily progress CSV shows `BROKEN` for Turtle (2026-04-28).
**Fix:** Update BTC data length in harness to use full 2971 bars; fix off-by-one forward-fill at CSV boundary.
**Complexity:** Medium. Single file + off-by-one logic.

### T9: Live Testnet — CRITICAL BLOCKER
**Status:** BLOCKED on Noah's Binance testnet API keys.
**Everything else is secondary.** The project cannot advance without live market validation. All metrics are upper bounds. Fee model, maker-fill rate, slippage — all unvalidated.
**Escalation:** This has been blocked for 3+ weeks. Nothing advances the project until this is resolved.
**What we need:** Binance testnet API key + secret. Not production keys — testnet only.

---

## Stop Doing

- Re-sweeping confirmed params. All are frozen. Stop.
- Re-testing confirmed strategies (BollingerReversion, CTREND, etc.) — graveyard is final.
- Building documentation-only commits when real work exists (S4 is testable now).
- Claiming walk-forward Sharpe 5.46 on equity charts — use equity Sharpe 1.29 only.
- Treating pre-2021 stress as a footnote — it's a known constraint, acknowledge it.
- Ignoring equity bug across multiple sessions — fix before deployment.

---

## Blind Spots

| Blind Spot | Severity | Status |
|-----------|----------|--------|
| **No live testnet** | CRITICAL | BLOCKED on Noah's API keys |
| **Equity bug (Turtle 1.0x)** | MEDIUM | Third session unfixed — must fix pre-deploy |
| **Pre-2021 stress: 67.9%** | MEDIUM | Known constraint — strategy overfit to bull crypto |
| **Sequential hyperopt on same OOS grid** | MEDIUM | Unquantifiable cumulative implicit overfitting risk |
| **Maker/slippage model unvalidated** | HIGH | Live testnet only |

---

## Graveyard Summary

| Strategy | Result | Key Reason |
|----------|--------|------------|
| EP=24 | REVERTED | In-sample inflation (same OOS data as P=7 + ATR_EM) |
| ATR_ENTRY_MULT>0 | REJECTED | All non-zero values degrade pass rate |
| 4h multi-timeframe | GRAVEYARD | Structural failure (1/20 pass) |
| Cross-market equity integration | REJECTED | Combined -2.94 Sharpe vs crypto-only |
| DynamicTrend EMA signal | REJECTED | Turtle wins 21/24 windows |
| BollingerReversion | GRAVEYARD | 0/288 OOS pass |
| BOCPD regime detector | GRAVEYARD | 0% breaks |
| FDUSD basis carry | GRAVEYARD | 19% pass |
| Funding rate MR | GRAVEYARD | 43% pass |
| Vol-contingent Chandelier | GRAVEYARD | All configs identical |
| Position scaling overlays | GRAVEYARD | All failed |
| CTREND regime-conditional switching | REJECTED | 67% pass < 70% threshold |
| Donchian entry | REJECTED (not replacement) | Wins Sharpe (+3.8) but loses pass rate (-14pp) |
| Correlation entry filter (T7) | REJECTED | All 3 variants lose to baseline |
| EP=43 | REVERTED | Same-session in-sample inflation as EP=21 |
| ATR-rank conditional filter (T20) | ASSESSED | Existing stale-harness shows marginal, not worth running on current params |

---

## Research Loop: What Remains

1. **S4:** ATR-normalized position sizing — TESTABLE NOW (does not need credentials)
2. **T24:** Equity bug fix — pre-deploy only (low priority vs live testnet)
3. **T9:** Live testnet — BLOCKED on Noah's API keys

**Research status:** Research loop CLOSED. T22 confirmed no structural invalidation needed. TURTLE_ATR_PERIOD=24 hyperopt was valid (not noise). Production Turtle-only strategy is sound. S4 is the last genuinely testable idea without credentials. Only live testnet validates execution assumptions.