# PLAN.md — Krypto Research and Execution Plan

**State: 2026-04-30 12:20 UTC. T35 DISCOVERED (fee cancel bug). S6 CANDIDATE pending T35. ATR_RANK untested. Live testnet BLOCKED 4+ weeks.**

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

**Fix:** `entry = entry_px * (1.0 + TAKER_FEE)` (buy costs more via fee); `exit = exit_px * (1.0 - TAKER_FEE)` (sell receives less). After fix, re-run 9-universe walkforward to get true baseline. Update all fee-sensitivity claims on corrected baseline.

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
```

---

## Next Tasks (Priority Order)

### T35: Fix Fee Accounting Bug — CRITICAL
**Status:** NEW. Walkforward runs zero-fee due to cancel.
**Action:** Fix `turtle_chandelier_walkforward.rs` entry fee `(1-fee)` → `(1+fee)`. Re-run 9-universe × 6-window walkforward. Get true fee-adj Sharpe. Update HALL_OF_FAME.md with corrected numbers.
**Evidence:** `examples/turtle_chandelier_walkforward.rs` lines 184, 212.
**Priority:** CRITICAL — affects every live deployment decision.

### T36: Re-Run S6 Validation Under Correct Fee Model
**Status:** CANDIDATE. Pending T35 results.
**Action:** After T35 fix, rerun rebalancing 9-universe harness. If S6 still wins, promote to production. If not, reject cleanly.
**What to build:** Nothing new — run `examples/rebalancing_9universe.rs` with fixed fee model.

### ATR_RANK Conditional Entry — UNTESTED (mechanically novel)
**Hypothesis:** Enter only when current 21-bar ATR > 60th percentile of 252-bar history.
**Mechanism:** High-vol = trending regime = valid setups. Low-vol = chop = filter out.
**Difference from ATR_ENTRY_MULT:** Fixed threshold vs percentile threshold. Mechanically novel — worth one dedicated run.
**Risk:** Vol-rank may be too slow (same failure mode as vol-contingent Chandelier which was identically zero across all configs).
**What to build:** `examples/atr_rank_entry_sweep.rs` — sweep `atr_rank_threshold ∈ {50, 60, 70, 80}` with ATR rank computed as `percentile_rank(current_21_ATR, 252_bar_history`. Base5 × 6 windows.
**Reject if:** Pass rate drops materially or Sharpe negative. One run, then done.

### T34: Live Bot Dual-Exit Gap — Unverified Claim Flagged
**Status:** UNRESOLVED. Live bot Turtle-only. Comment in `bot.rs` line 4 claims "Turtle-only wins +1.47 Sharpe over dual-exit" — **never verified in any harness.**
**Action:** Edit the comment in `bot.rs` line 4 to remove the unverified Sharpe claim. Replace with: `// Exit: Turtle ATR trailing stop ONLY. Chandelier gap acknowledged — see T34 in PLAN.md.`
**Do not:** Add Chandelier to live bot without live testnet validation first.

### T9: Live Testnet — CRITICAL BLOCKER (4+ weeks)
**Status:** BLOCKED on Noah's Binance testnet API keys.
**What we need:** Binance testnet API key + secret (not production keys).

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
| Fee cancel bug (T35) | CRITICAL | Walkforward runs 0-fee. All Sharpe inflated. Fix before any deployment. |
| Live bot dual-exit gap | HIGH | T34 — unverified Sharpe claim in bot.rs comment. Flag it. |
| 2026 YTD root cause | MEDIUM | Regime-inherent or live divergence? Need live data to answer. |
| Anti-overfitting theater | MEDIUM | Rules exist but VL=90 violated them. Document the violation. |
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
| **Fee cancel bug (T35)** | BUG | Walkforward (1-fee)/(1-fee) = zero fee. All Sharpe inflated 22-33%. |
