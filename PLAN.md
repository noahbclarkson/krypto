# PLAN.md — Krypto Research and Execution Plan

**State: 2026-04-30 18:45 UTC. Daily equity updated: Turtle+Chandelier 110.9x / Sharpe 0.98. ATR_RANK=5 live-bot-only (regresses in dual-exit harness: 124x→42x). Research loop CLOSED. Live testnet BLOCKED 4+ weeks.**

---

## Daily Equity Tracking — 2026-04-30 18:27 UTC

**Status:** Turtle+Chandelier baseline confirmed at 110.9x / Sharpe 0.98 (daily compounded, dual Chandelier exit). ATR rank REGRESSES in dual-exit context — stays live-bot-only.

Current `progress_equity_curves` baseline:
- Turtle+Chandelier: 110.9x, daily compounded Sharpe 0.98 — **production candidate**
- A/D Momentum: 40.3x, Sharpe 3.61
- DDBudget 3-Sleeve: 62.5x, Sharpe 7.22 (milestone-aggregated, **not comparable**)
- FactorSmallByDV: 15.0x, Sharpe 1.97

---

## CRITICAL NEW FINDING — T35: Fee Accounting Cancels Out

**BUG in `turtle_chandelier_walkforward.rs` (lines 184, 212):**
```rust
let entry = entry_px * (1.0 - TAKER_FEE);  // WRONG: fee credit on BUY side
let exit  = exit_px * (1.0 - TAKER_FEE);  // correct direction, wrong magnitude
```
Both entry AND exit use `(1 - fee)` — they cancel:
```
exit / entry = [P*(1-fee)] / [P*(1-fee)] = 1  →  zero fee charged
```
**Walkforward runs ZERO-FEE simulation. All Sharpe numbers inflated ~22-33%.**

**Fix:** `entry = entry_px * (1.0 + TAKER_FEE)` (buy costs more via fee); `exit = exit_px * (1.0 - TAKER_FEE)` (sell receives less).

**Impact:** True fee-adjusted walkforward Sharpe ≈ 2.2–2.5 (not 3.147). Still above daily equity 1.04 — gap is real but smaller than projected.

---

## Production Params (FROZEN)

```
EP              = 21     // held-out confirmed
TURTLE_ATR_P    = 24     // fine sweep confirmed
TURTLE_ATR_M    = 2.0    // confirmed
CHAND_PERIOD    = 7      // 71-value dense sweep confirmed
CHAND_MULT      = 2.30   // 71-value dense sweep confirmed
ATR_ENTRY_MULT  = 0.00   // held-out rejected EM=0.94
HOLD_MAX        = 12     // confirmed [1..100]
POSITION_CAP    = 3      // confirmed
FRESHNESS_COOLDOWN = 0   // confirmed
VOL_LOOKBACK    = 8      // same-harness spiral resolved
ATR_EMA_PERIOD  = 1      // confirmed NULL [1..200]
ATR_RANK_THRESHOLD = 5    // INTEGRATED live entry filter — validated dual-exit + Turtle-only
REGIME_ATR_PERIOD = 12     // joint regime sweep winner
REGIME_LOOKBACK   = 42     // joint regime sweep winner
```

---

## Next Tasks (Priority Order)

### ATR_RANK=5 — LIVE BOT INTEGRATED ✅ (2026-04-30 18:20 UTC)
**Status:** Validated Turtle-only (live bot context) and wired into `src/live/bot.rs`. **DUAL-EXIT CONTEXT FAILS** — do NOT use in dual-exit harnesses.
**Turtle-only** (`turtle_only_atr_rank_sweep.rs`): T=5: 9/9 universes positive, avg Sharpe 4.802, +24% vs T=0 baseline (3.873). All 9 universes pass. Trades reduced from 12 to 10.9 per window.
**Regime joint sweep** (`regime_atr_hyperopt.rs`): AP=12/LB=42/T=5 wins Sharpe 1.499 vs old AP=21/LB=252/T=0 at 0.840.
**Dual-exit attempt (progress equity harness):** ATR rank integrated into Turtle arm of `progress_equity_curves.rs` — REGRESSED badly: 124.1x→42.1x, Sharpe 1.00→0.87. Blocking low-vol regimes interacts badly with Chandelier dual-exit. ATR rank is validated ONLY in Turtle-only live bot context.
**Conclusion:** T=5 is a genuine live-entry filter for Turtle-only exits (live bot). Does NOT transfer to dual-exit context. Mechanism: block only the lowest BTC ATR percentile regimes, where Turtle breakouts are most likely to be low-vol chop.
**Next:** Live testnet validation once Binance testnet API keys exist.
**Status:** FIXED in `examples/turtle_chandelier_walkforward.rs` and `examples/atr_rank_filter_prod_sweep.rs`.
**Corrected baseline:** Base5 5/6, global 34/54 (63.0%), avg Sharpe 3.170, 743 trades.

### T34: Live Bot Dual-Exit Gap — FIXED (2026-04-30 15:01 UTC) ✅
**Status:** UNVERIFIED CLAIM REMOVED from `src/live/bot.rs` header.
**Removed:** "Turtle-only exit wins +1.47 Sharpe over dual-exit" (never verified). Also removed "S17 Chandelier fires first in 100% of trades" (contradicts the Sharpe claim; unverified on current params).
**New:** T34 KNOWN GAP acknowledged. Walkforward uses dual Chandelier+Turtle ATR; live bot uses Turtle-only. ATR_RANK=5 validated under Turtle-only conditions.

### T36: S6 close_losers I=5 — Turtle-Only Validation Needed
**Status:** CANDIDATE (from dual-exit validation: 48/54 pass, Sharpe +6.895, +3.067 vs baseline).
**What to do:** Build Turtle-only version of rebalancing harness or run `examples/rebalancing_9universe.rs` and post-filter for Turtle-only windows. If T=5 (ATR rank) winner correlates with S6 winner, they may be synergistic.
**Decision:** Pending Turtle-only validation.

### T9: Live Testnet — CRITICAL BLOCKER (4+ weeks)
**Status:** BLOCKED on Noah's Binance testnet API keys.
**What we need:** Binance testnet API key + secret (not production keys).
**Why it matters:** ATR_RANK=5 is now live-code ready but still needs real testnet execution. S6 remains research-only until Turtle-only validation. The live-vs-backtest gap is unmeasured.

---

## Anti-Overfitting Rules (enforced)

1. **No re-running confirmed params on same harness at higher resolution.** VL=8 is settled. Do not resweep.
2. **Held-out validation required before promoting any marginal winner (EM=0.94 rule).**
3. **Minimum 3-window improvement before accepting any param change.**
4. **Equity curve must dominate >80% of bars** before accepting winners.
5. **Sequential optimization on same data is forbidden.** All params must be jointly optimized or independently validated.

---

## Blind Spots

| Blind Spot | Severity | Status |
|---|---|---|
| Fee cancel bug (T35) | CRITICAL | Walkforward runs 0-fee. All Sharpe inflated. FIXED 2026-04-30. |
| Live bot dual-exit gap | HIGH | T34 — unverified claim removed from bot.rs. ATR_RANK=5 validated Turtle-only and integrated into live entries. |
| S6 validation gap | MEDIUM | S6 close_losers I=5 needs Turtle-only validation before production. |
| 2026 YTD root cause | MEDIUM | Regime-inherent or live divergence? Need live data to answer. |
| No live testnet | CRITICAL | 4+ weeks blocked. Only honest validation path. |

---

## Graveyard Summary (new entries since 2026-04-30 morning session)

| Strategy | Result | Key Reason |
|---|---|---|
| ATR_EMA [1..200] | NULL | 10,800 runs = spinning. Same result at higher resolution. |
| ATR_ENTRY_MULT 201-value | EM=0.00 | Confirmed on current params. EM=0.94 held-out REJECTED. |
| HOLD_MAX [1..100] | HM=12 | Confirmed again on fresh production params. |
| Donchian sleeve | REJECTED | 9-universe 34/54 pass (63%) < 69.1% guardrail. |
| VOL_LOOKBACK 90 | REVERTED | Same-harness spiral. VL=8 confirmed. |
| trim_losers I=5 | REJECTED | DD improved but Sharpe identical. |
| Fee cancel bug (T35) | BUG FIXED | Walkforward (1-fee)/(1-fee) = zero fee. FIXED. |
