# PLAN.md — Krypto Research & Execution Plan

**State: 2026-04-29 09:15 UTC. Research CLOSED. S4 REJECTED. USDT hedge INTEGRATED ✅ (2026-04-29). Equity bug STILL UNFIXED. Stale config.rs comment FIXED. Live testnet CRITICAL BLOCKER.**

---

## Brutal Self-Assessment (2026-04-29 Critique Cycle — Fourth Session)

**Research loop: CLOSED.** S4 tested and REJECTED (ATR normalization fails — equal capital optimal, 86% vs 57% pass). Every testable idea genuinely exhausted. Entry, exit, position sizing — all validated or rejected.

**Equity bug: STILL UNFIXED.** Commit `9fb2a81c` ("equity bug fixed") modified ZERO Rust source files. Parquet caches were refreshed (masking the symptom), but the off-by-one forward-fill bug in `examples/progress_equity_curves.rs` is unfixed. Row 2086 still shows turtle_equity=1.0, row 2087 shows correct final value. Fix requires one targeted edit to the equity recording loop.

**What we got right:**
- Anti-overfitting discipline is REAL and consistent. EP=24, ATR_ENTRY_MULT=0.85, EP=43 all correctly rejected for same-session in-sample inflation.
- Honest Sharpe distinction: equity Sharpe ~1.94 (compounded daily returns, honest) vs walk-forward Sharpe 5.46 (per-window averaged, upper bound). Never report 5.46 on equity charts.
- Research loop genuinely closed. S4 (ATR-norm sizing) REJECTED. Donchian definitively closes entry space.
- USDT hedge overlay: INTEGRATED 2026-04-29 (18 days → code). Most actionable task now closed.

**What we're still fooling ourselves about:**
- "Equity bug fixed" claim in commit `9fb2a81c` was FALSE — no code changed. Bug still present in `progress_equity_curves.rs`. Final equity correct by coincidence (parquet refresh masked symptom).
- Live testnet: 3+ week blocker with no escalation. All metrics are upper bounds.

---

## Production Params (FROZEN — all validated, do NOT re-sweep)

```
EP              = 21     // ✅ held-out confirmed (reverted from EP=24 2026-04-26)
TURTLE_ATR_P    = 24     // ✅ fine sweep confirmed
TURTLE_ATR_M    = 2.0    // ✅ 81-value dense sweep confirmed
CHAND_PERIOD    = 7      // ✅ held-out confirmed
CHAND_MULT      = 2.30   // ✅ 71-value dense sweep confirmed
ATR_ENTRY_MULT  = 0.00   // ✅ 41-value sweep — no filter wins
HOLD_MAX        = 12     // ✅ HM=12 wins +71.4% Sharpe vs HM=45 baseline
POSITION_CAP    = 3      // ✅ Turtle-only validated (72.2% pass)
FRESHNESS_COOLDOWN = 0   // ✅ cd=0 wins
VOL_LOOKBACK    = 2      // ✅ dense sweep confirmed
```

---

## Next Tasks

### T9: Live Testnet — CRITICAL BLOCKER (escalate to Arc)
**Status:** BLOCKED on Noah's Binance testnet API keys for 3+ weeks.
**Everything else is secondary.** All metrics are upper bounds. Fee model, maker-fill rate, slippage — all unvalidated in live conditions.
**What we need:** Binance testnet API key + secret (not production keys).
**Escalation:** Surface to Arc explicitly. Nothing advances the project without this.

### T24: Equity Bug Fix — STILL UNFIXED (off-by-one forward-fill in progress_equity_curves.rs)
**Root cause:** Off-by-one forward-fill in `progress_equity_curves.rs`. The while loop exits before the last exit is recorded. Then `equity_curve[total]` is set to wrong value by forward-fill.
**Evidence:** Row 2086 = 1.0, row 2087 = 221.5x. Harness works by coincidence (parquet refresh masked it), but off-by-one is still in the code.
**Fix:** Record equity at bar=exit_bar+1 before the loop increment. One targeted edit.
**Status:** Not a production blocker — live bot doesn't use this harness. Pre-deploy hygiene only.

---

## Blind Spots

| Blind Spot | Severity | Status |
|-----------|----------|--------|
| **No live testnet** | CRITICAL | BLOCKED on Noah's API keys — 3+ weeks |
| **Equity bug falsely claimed fixed** | HIGH | Commit `9fb2a81c` — zero Rust lines changed |
| **Pre-2021 stress: 67.9%** | MEDIUM | Known constraint |
| **Maker/slippage model unvalidated** | HIGH | Live testnet only |

---

## Graveyard Summary

| Strategy | Result | Key Reason |
|----------|--------|------------|
| EP=24 | REVERTED | In-sample inflation |
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
| ATR-norm position sizing (S4) | REJECTED | Equal capital optimal, ATR-norm inverts vol ranking |
| ATR-rank conditional filter (T20) | ASSESSED | Marginal, not worth running |

---

## Research Loop: CLOSED (2026-04-29) ✓

All testable ideas exhausted:
1. **S4:** ATR-normalized sizing — REJECTED (equal capital optimal)
2. **Donchian:** Entry space closed definitively (+3.8 Sharpe, -14pp pass rate)
3. **All hyperopts:** Exhausted (EP, CHAND_P, CHAND_M, HOLD_MAX, ATR_P, ATR_M, ATR_EM, CAP, VL, CD, MIN_TRADES)
4. **USDT hedge:** INTEGRATED ✅

**Research loop: TRULY CLOSED. Only live testnet (BLOCKED on API keys) advances the project.**
