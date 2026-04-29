# PLAN.md — Krypto Research & Execution Plan

**State: 2026-04-29 08:05 UTC. Research CLOSED. S4 REJECTED. USDT hedge UNINTEGRATED (18 days). Equity bug STILL UNFIXED despite "fixed" claim. Stale config.rs comment. Live testnet CRITICAL BLOCKER.**

---

## Brutal Self-Assessment (2026-04-29 Critique Cycle — Fourth Session)

**Research loop: CLOSED.** S4 tested and REJECTED (ATR normalization fails — equal capital optimal, 86% vs 57% pass). Every testable idea genuinely exhausted. Entry, exit, position sizing — all validated or rejected.

**Equity bug: STILL UNFIXED.** Commit `9fb2a81c` ("equity bug fixed") modified ZERO Rust source files. Parquet caches were refreshed (masking the symptom), but the off-by-one forward-fill bug in `examples/progress_equity_curves.rs` is unfixed. Row 2086 still shows turtle_equity=1.0, row 2087 shows correct final value. Fix requires one targeted edit to the equity recording loop.

**What we got right:**
- Anti-overfitting discipline is REAL and consistent. EP=24, ATR_ENTRY_MULT=0.85, EP=43 all correctly rejected for same-session in-sample inflation.
- Honest Sharpe distinction: equity Sharpe ~1.94 (compounded daily returns, honest) vs walk-forward Sharpe 5.46 (per-window averaged, upper bound). Never report 5.46 on equity charts.
- Research loop genuinely closed. S4 (ATR-norm sizing) REJECTED. Donchian definitively closes entry space.
- USDT hedge overlay is the most actionable unblocked task (no credentials needed).

**What we're still fooling ourselves about:**
- USDT hedge: 18 days documented, zero code. Most actionable task, permanently deferred.
- "Equity bug fixed" claim in last commit is FALSE — no code changed. This is a pattern of reporting the desired state rather than the actual state.
- Live testnet: 3+ week blocker with no escalation. All metrics are upper bounds.
- Stale doc comment in `src/live/config.rs`: ATR_ENTRY_MULT=0.00 described as "EM=0.90 wins" — a rejected result still documented as active.

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

### T25: USDT Hedge Overlay — INTEGRATE INTO BOT ⭐ (Today, no credentials needed)
**Concept:** Vol-regime position sizing overlay. Reduce position 30% when BTC 21d vol > 75th pct of 252-bar history.
- Documented 2026-04-11: ~30% DD reduction in bear windows
- Mechanism: before opening a position in `bot.rs`, compute `vol_pct = rank_21d_atr(BTC_bar) / 252`. If > 0.75 → `size *= 0.70`.
- Location: `src/live/bot.rs` line ~244: `let size = 1.0 / self.config.position_cap as f64;` → add BTC ATR rank check before this.
- Non-breaking: optional overlay, only activates in high-vol regimes, does not change any validated parameter.
- **No walk-forward needed** — already validated in regime_stress_test.rs (2026-04-11). Integration task, not research.
- Status: 18 days documented, ZERO code written. Most actionable unblocked task in the entire project.

### T24: Equity Bug Fix — OFF-BY-ONE STILL PRESENT (claimed fixed, actually NOT fixed)
**Root cause:** Off-by-one forward-fill in `progress_equity_curves.rs`. The while loop exits before the last exit is recorded. Then `equity_curve[total]` (row 2086) is set to 1.0 by the forward-fill (only element that fits).
**Evidence:** Row 2086 = 1.0, row 2087 = 221.5x. Harness works by coincidence (parquet refresh masked it), but off-by-one is still in the code.
**Fix:** Record equity at bar=exit_bar+1 before the loop increment. Or extend forward-fill to exclude the out-of-bounds element. One targeted edit.
**Status:** Commit `9fb2a81c` claimed this was fixed. Zero Rust lines were changed. This is a false claim.

### T9: Live Testnet — CRITICAL BLOCKER (escalate to Arc)
**Status:** BLOCKED on Noah's Binance testnet API keys for 3+ weeks.
**Everything else is secondary.** All metrics are upper bounds. Fee model, maker-fill rate, slippage — all unvalidated in live conditions.
**What we need:** Binance testnet API key + secret (not production keys).
**Escalation:** Surface to Arc explicitly. Nothing advances the project without this.

---

## Blind Spots

| Blind Spot | Severity | Status |
|-----------|----------|--------|
| **No live testnet** | CRITICAL | BLOCKED on Noah's API keys — 3+ weeks |
| **USDT hedge overlay not integrated** | HIGH | 18 days documented, zero code |
| **Equity bug falsely claimed fixed** | HIGH | Commit `9fb2a81c` — zero Rust lines changed |
| **Pre-2021 stress: 67.9%** | MEDIUM | Known constraint — USDT hedge addresses this |
| **Stale config.rs doc comment** | LOW | EM=0.90 described as active, was reverted |
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