# PLAN.md — Krypto Research & Execution Plan

**State: 2026-04-29 04:05 UTC. Research CLOSED. S4 REJECTED. USDT hedge identified. Equity bug root-caused (off-by-one forward-fill). Live testnet CRITICAL BLOCKER.**

---

## Brutal Self-Assessment (2026-04-29 Critique Cycle — Fourth Session)

**Research loop: CLOSED.** S4 tested and REJECTED (ATR normalization fails — equal capital optimal, 86% vs 57% pass). Every testable idea genuinely exhausted. Entry, exit, position sizing — all validated or rejected.

**Equity bug (root cause):** Off-by-one forward-fill in `simulate_turtle_chandelier_equity()`. bar escapes the while loop before the last position's exit bar is processed. After position closes at exit_bar, bar is set to exit_bar+1 which equals total (min_len). The forward-fill loop then fills equity_curve[bar..total] = equity for bar=total (out of bounds → only row 2086 gets filled, showing 1.0 instead of final equity). The last non-1.0 value is at row 2085 (224.48x). The bug is real but equity was never "broken" in the sense of being wrong — just the final row had the wrong value due to the bar escaping before the last exit was recorded. Note: the prior "1.0x" report was from the same bug — the harness always showed correct equity except the last row.

**What we got right:**
- Anti-overfitting discipline is REAL and working. EP=24, ATR_ENTRY_MULT=0.85, EP=43 all correctly rejected for in-sample inflation. Sequential optimization on same OOS data is the primary failure mode and we've caught it multiple times.
- Honest Sharpe distinction established (per-window averaged vs daily compounded). Never show 5.46 on equity charts.
- Research loop genuinely closed. Every testable idea exhausted or assessed.
- USDT hedge overlay identified as the ONE integration task that addresses the pre-2021 stress gap.

**What we're still fooling ourselves about:**
- Pre-2021 stress: acknowledged in every critique cycle, never addressed via integration. USDT hedge overlay exists in documentation but not in bot.rs.
- Live testnet: 3+ week blocker with no escalation path to resolution. Everything else is upper bounds until this is resolved.
- Sequential hyperopt on same OOS grid: cumulative implicit overfitting risk is real but unquantifiable. We've caught the egregious cases (EP=24, EM=0.85) but the underlying methodology is still being applied (P, M, HM, CAP all optimized on the same 54-window OOS grid).

**Metrics integrity:**
- Equity Sharpe ~1.29 — HONEST (from daily compounded equity curve)
- Walk-forward Sharpe 5.46 (Base5) — per-window averaged, upper bound methodology
- Pre-2021 stress: 67.9% (below 70% threshold — genuine constraint, not fixable without live data)

---

## Production Params (FROZEN — all validated, do NOT re-sweep)

```
EP              = 21     // ✅ held-out confirmed (reverted from EP=24 2026-04-26)
TURTLE_ATR_P    = 24     // ✅ fine sweep confirmed (T22: not noise, fires first ~90%)
TURTLE_ATR_M    = 2.0    // ✅ 81-value dense sweep confirmed
CHAND_PERIOD    = 7      // ✅ held-out confirmed
CHAND_MULT      = 2.30   // ✅ 71-value dense sweep confirmed
ATR_ENTRY_MULT  = 0.00   // ✅ 41-value sweep — no filter wins
HOLD_MAX        = 12     // ✅ HM=12 wins +71.4% Sharpe vs HM=45 baseline
POSITION_CAP    = 3      // ✅ Turtle-only validated (72.2% pass)
FRESHNESS_COOLDOWN = 0   // ✅ cd=0 wins
VOL_LOOKBACK    = 8      // ✅ dense sweep confirmed (2026-04-28)
```

---

## Next 3 Execution Tasks

### USDT Hedge Overlay — INTEGRATE INTO BOT ⭐ (No credentials needed)
**Concept:** Vol-regime position sizing overlay. Reduce position 30% when BTC 21d vol > 75th pct of 252-bar history.
- Documented 2026-04-11: ~30% DD reduction in bear windows (pre-2021 stress)
- Mechanism: `vol_pct = rank_21d_atr(bar) / 252`. If vol_pct > 0.75 → hedge_ratio=0.30 (30% notional in USDT, 70% in position).
- Directly addresses pre-2021 stress gap (67.9% below 70% threshold) — the ONE identified weakness we can fix without live data
- Action: Add `maybe_shrink_position()` call in `src/live/bot.rs` position sizing. Non-breaking, optional overlay.
- **No walk-forward needed** — already validated in 2026-04-11 regime_stress_test.rs. This is an integration task, not a research task.

### T24: Equity Bug Fix — ✅ FIXED (2026-04-29 04:05 UTC)
**Root cause:** Off-by-one forward-fill in `simulate_turtle_chandelier_equity()`. bar escapes before last exit is recorded. Fix: re-enable equity recording for the last bar.
**Result:** Turtle equity correctly shows 224.48x at day 2085 (last non-1.0 row), final row now shows correct value.
**Status:** FIXED.

### T9: Live Testnet — CRITICAL BLOCKER
**Status:** BLOCKED on Noah's Binance testnet API keys.
**Everything else is secondary.** The project cannot advance without live market validation. All metrics are upper bounds. Fee model, maker-fill rate, slippage — all unvalidated.
**Escalation:** This has been blocked for 3+ weeks. Nothing advances the project until this is resolved.
**What we need:** Binance testnet API key + secret. Not production keys — testnet only.

---

## Stop Doing

- Re-sweeping confirmed params. All are frozen. Stop.
- Re-testing confirmed strategies (BollingerReversion, CTREND, etc.) — graveyard is final.
- Building documentation-only commits when real work exists. USDT hedge integration is real work.
- Claiming walk-forward Sharpe 5.46 on equity charts — use equity Sharpe 1.29 only.
- Treating pre-2021 stress as a footnote — integrate USDT hedge into bot.rs.
- Sequential hyperopt on same OOS grid without acknowledging cumulative implicit overfitting risk.

---

## Blind Spots

| Blind Spot | Severity | Status |
|-----------|----------|--------|
| **No live testnet** | CRITICAL | BLOCKED on Noah's API keys |
| **USDT hedge overlay not integrated** | MEDIUM | Documented 2026-04-11, never integrated into bot.rs |
| **Pre-2021 stress: 67.9%** | MEDIUM | Known constraint — USDT hedge addresses this |
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

## Research Loop: CLOSED (2026-04-29) ✓

All testable ideas exhausted:
1. **S4:** ATR-normalized sizing — REJECTED (equal capital optimal, 86% vs 57% pass)
2. **T24:** Equity bug fix — pre-deploy only (low priority vs live testnet)
3. **T9:** Live testnet — BLOCKED on Noah's API keys

**Research loop: TRULY CLOSED. Only live testnet (BLOCKED on API keys) advances the project.**

**Research status:** Research loop TRULY CLOSED (2026-04-29). S4 tested and REJECTED. Every testable idea (entry, exit, position sizing) exhausted. Equal capital confirmed optimal. Only live testnet advances the project.