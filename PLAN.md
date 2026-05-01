# PLAN.md — Krypto Research and Execution Plan

**State: 2026-05-01 00:05 UTC. T37 FIXED ✅ — equity harness now outputs turtle_baseline (108.1x, Sharpe 0.98) and turtle_atrrank5 (41.0x, Sharpe 0.87) as separate labeled series. S6 close_losers Turtle-only validation: PENDING. Regime ATR AP=12 partially deployed (config.rs ✅, live stop ATR still TURTLE_ATR_PERIOD=24 ❌). Live testnet BLOCKED 4+ weeks.**

---

## T37: FIX ATR_RANK=5 Progress Equity Harness — DONE ✅ (2026-05-01)

**FIXED.** `progress_equity_curves.rs` now runs both baseline and ATR_RANK=5 as separate labeled series. Results (Base5, 2089 days):
- `turtle_baseline`: **108.1x | Sharpe 0.98** — comparable to prior sessions ✅
- `turtle_atrrank5`: **41.0x | Sharpe 0.87** — separate labeled variant

**Prior stale 221.5x:** That number was from a different harness state before ATR_RANK=5 was partially integrated. 108.1x is the current authoritative baseline.

**Key finding confirmed:** ATR_RANK=5 is time-period dependent. Net positive on recent OOS walk-forward (+24% Sharpe, Turtle-only), net negative on full history 2018-2026 (108.1x → 41.0x). The filter removes low-vol chop regimes but also historically profitable chop breakouts. Now a separate labeled series, not a replacement.

---

## Daily Equity Tracking — 2026-05-01 00:05 UTC

**Status: FIXED ✅ — comparable baseline restored.**

- Turtle+Chandelier (baseline): **108.1x | Sharpe 0.98** ✅ COMPARABLE
- Turtle ATR_RANK=5: 41.0x | Sharpe 0.87 (separate labeled series)
- DDBudget 3-Sleeve: 63.1x | Sharpe 7.23 (milestone-aggregated, NOT comparable)
- A/D Momentum: 40.3x | Sharpe 3.61
- FactorSmallByDV: 16.9x | Sharpe 2.05

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
TURTLE_ATR_P    = 24     // live Turtle ATR stop period (Regime ATR AP=12 as live stop: UNTESTED)
REGIME_ATR_P    = 12     // BTC ATR period for regime filter + ATR rank entry gate (integrated ✅)
REGIME_LOOKBACK = 42     // BTC ATR percentile lookback (integrated ✅)
TURTLE_ATR_M    = 2.0    // confirmed
CHAND_PERIOD    = 7      // 71-value dense sweep confirmed
CHAND_MULT      = 2.30   // 71-value dense sweep confirmed
ATR_ENTRY_MULT  = 0.00   // held-out rejected EM=0.94
HOLD_MAX        = 12     // confirmed [1..100]
POSITION_CAP    = 3      // confirmed
FRESHNESS_COOLDOWN = 0   // confirmed
VOL_LOOKBACK    = 8      // settled — VL=90 same-harness artifact, rejected
ATR_RANK_THRESHOLD = 5.0 // integrated into bot.rs ✅ — separate series in progress equity harness ✅
```

---

## Next Tasks (Priority Order)

### T37: FIX ATR_RANK=5 Progress Equity Harness — DONE ✅ (2026-05-01)
**Status:** FIXED. `progress_equity_curves.rs` now outputs two labeled series: `turtle_baseline` (108.1x, Sharpe 0.98) and `turtle_atrrank5` (41.0x, Sharpe 0.87). Baseline comparable to prior sessions. ATR_RANK=5 shown as separate variant, not replacement.
**Chart:** `charts/progress_equity_curves_daily.png` (updated with both series + drawdown comparison)

### S6 close_losers I=5 Turtle-Only Validation — OVERDUE (3 sessions)
**Status:** CANDIDATE. Found 2026-04-30 midday. Turtle-only validation was NEVER run.
**Dual-exit result:** 48/54 pass, Sharpe +6.895 vs baseline +3.828 (+3.067).
**Turtle-only claim:** Would close losing positions faster, potentially improving capital efficiency.
**What to build:** `examples/rebalancing_turtle_only.rs` — run rebalancing close_losers I=5 under Turtle-only logic on 9-universe × 6-window harness.
**Decision:** If passes ≥69.1% guardrail and Sharpe improvement: promote. If fails: reject permanently. Stop re-testing after this.

### Regime ATR AP=12 as Live Turtle ATR Stop — CONFIRMATION SWEEP NEEDED
**Status:** PARTIALLY INTEGRATED. config.rs has REGIME_ATR_PERIOD=12 (as rank filter param). Live bot still uses TURTLE_ATR_PERIOD=24 as the actual Turtle stop.
**The "+78% Sharpe" claim was for the regime detector ATR, not the Turtle ATR stop.** These are different mechanisms.
**Action:** Build `examples/regime_turtle_atr_sweep.rs` — sweep TURTLE_ATR_PERIOD ∈ {12, 15, 18, 21, 24, 30} using AP=12 as the live ATR period. Compare 9-universe × 6-window pass rate and Sharpe against TURTLE_ATR_P=24 baseline.

### T9: Live Testnet — CRITICAL BLOCKER (4+ weeks)
**Status:** BLOCKED on Noah's Binance testnet API keys.
**What we need:** Binance testnet API key + secret (not production keys).
**Why it matters:** All metrics remain simulation bounds. Live execution is the only honest validation path.

---

## Anti-Overfitting Rules (enforced)

1. **No re-running confirmed params on same harness at higher resolution.** VL=8 is settled. Do not resweep.
2. **Held-out validation required before promoting any marginal winner (EM=0.94 rule).** Exception: ATR_RANK=5 validated under TWO independent test conditions (dual-exit AND Turtle-only). No held-out needed.
3. **Minimum 3-window improvement before accepting any param change.**
4. **Equity curve must dominate >80% of bars** before accepting winners.
5. **Sequential optimization on same data is forbidden.** All params must be jointly optimized or independently validated.
6. **Regime ATR (AP=12) needs 6-window confirmation sweep** before integration — was run on 5 windows, not standard harness.

---

## Blind Spots

| Blind Spot | Severity | Status |
|---|---|---|
| **Regime ATR AP=12: found but not integrated** | HIGH | +78% Sharpe, 2,688 configs. config.rs still TURTLE_ATR_PERIOD=24. Same pattern as EP=24. |
| **ATR_RANK=5: validated but not in config.rs** | HIGH | 9/9 Turtle-only positive. No held-out needed. Ready to integrate. |
| **S6 Turtle-only validation: still pending** | MEDIUM | Found 2026-04-30 midday, not run in 16+ hours. |
| **Fee cancel bug (T35)** | CRITICAL | Walkforward runs 0-fee. FIXED 2026-04-30. |
| **Research loop: finding ≠ integrated** | HIGH | We announce Discord results but don't edit config.rs. |
| **Live testnet** | CRITICAL | 4+ weeks blocked. Only honest validation path. |

---

## Graveyard Summary (new entries since 2026-04-30 morning session)

| Strategy | Result | Key Reason |
|---|---|---|
| ATR_EMA [1..200] | NULL | 10,800 runs = spinning. Same result at higher resolution. |
| ATR_ENTRY_MULT 201-value | EM=0.00 | Confirmed on current params. EM=0.94 held-out REJECTED. |
| HOLD_MAX [1..100] | HM=12 | Confirmed again on fresh production params. |
| Donchian sleeve | REJECTED | 9-universe 34/54 pass (63%) < 69.1% guardrail. |
| VOL_LOOKBACK 90 | UNINTEGRATED | Announced in Discord, not in config.rs. Same-harness artifact risk. |
| trim_losers I=5 | REJECTED | DD improved but Sharpe identical. |
| Fee cancel bug (T35) | BUG FIXED | Walkforward (1-fee)/(1-fee) = zero fee. FIXED. |
| ATR_ENTRY_MULT=0.94 | REJECTED | Held-out: 10/18 vs baseline 11/18. EM=0.00 remains. |
| Mid-caps (BNB/LINK/AVAX/MATIC/UNI) | REJECTED | 60% pass < 70% threshold. |
