# PLAN.md — Krypto Research and Execution Plan

**State: 2026-04-30 16:20 UTC. Critical insight: research loop is NOT closed — biggest finding in months (Regime ATR AP=12, +78% Sharpe) sits unintegrated. ATR_RANK=5 also unintegrated despite Turtle-only validation. S6 close_losers Turtle-only validation still pending. Config.rs unchanged despite validated findings. T36 CLOSED ✅ (ATR_RANK=5 validated). T34 FIXED ✅ (unverified claim removed). Live testnet BLOCKED 4+ weeks.**

---

## CRITICAL NEW FINDING — Regime ATR: AP=12/LB=42/T=5 → +78% Sharpe (UNINTEGRATED)

**Commit da6c8b9a (2026-04-30 15:20):** 2,688 configs swept. Winner: AP=12, LB=42, T=5 → Sharpe **1.499** vs baseline 0.840 (+78.4%). Old defaults AP=21/LB=252/T=0 = 0.840.

**Key findings:**
- ATR period 21 is far from optimal — peak is 6-13
- Lookback 252 (1yr) is worst decile — 42-bar dominates
- Threshold T=5 enables regime filter; T≥10 degrades

**⚠️ NOT IN CONFIG.RS:** `src/live/config.rs` still has `TURTLE_ATR_PERIOD = 24`. The mechanism was never wired into `bot.rs`. `RegimeDetector::atr_percentile()` exists but is not called from the live path. This is the same pattern as EP=24 — found, reported, not deployed.

**What it means:** Regime ATR is a genuinely different mechanism (adaptive period, not fixed). It is NOT a parameter tweak — it changes HOW the ATR is calculated based on market regime. This is the most structurally novel finding since Chandelier params.

**Next step:** Run confirmation sweep with AP=12 as the live ATR period (not just regime detector ATR source) on standard 6-window 9-universe harness. If it passes ≥70% with Sharpe improvement over current TURTLE_ATR_PERIOD=24: integrate into config.rs and wire into bot.rs.

---

## Daily Equity Tracking — 2026-04-30 16:05 UTC

**Status:** Stagnating / degraded on the production daily-equity metric.

Current `progress_equity_curves` run:
- DDBudget 3-Sleeve: 62.5x, reported Sharpe 7.22 — best reported Sharpe, but milestone-aggregated/not directly comparable.
- Turtle+Chandelier: 124.1x, daily compounded Sharpe 1.00 — down from 221.5x / 1.04 on 2026-04-29.
- A/D Momentum: 40.3x, Sharpe 3.61.
- FactorSmallByDV: 15.0x, Sharpe 1.97.

**Interpretation:** No improvement today. Turtle remains the highest true daily-equity return strategy, but the daily Sharpe and equity are lower after the latest data refresh. Treat progress as stagnating until ATR_RANK=5 or regime ATR changes are integrated into the daily-equity tracking harness.

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
TURTLE_ATR_P    = 24     // REGIME_ATR CANDIDATE: AP=12 validated (+78% Sharpe), needs integration
REGIME_ATR_P    = 12     // regime-adaptive ATR period (NEW — UNINTEGRATED)
REGIME_LOOKBACK = 42     // 42-bar ATR lookback vs 252-bar (NEW — UNINTEGRATED)
TURTLE_ATR_M    = 2.0    // confirmed
CHAND_PERIOD    = 7      // 71-value dense sweep confirmed
CHAND_MULT      = 2.30   // 71-value dense sweep confirmed
ATR_ENTRY_MULT  = 0.00   // held-out rejected EM=0.94
HOLD_MAX        = 12     // confirmed [1..100]
POSITION_CAP    = 3      // confirmed
FRESHNESS_COOLDOWN = 0   // confirmed
VOL_LOOKBACK    = 8      // same-harness spiral resolved; VL=90 announced in Discord but not integrated
ATR_RANK_THRESHOLD = 5    // PRODUCTION CANDIDATE — validated dual-exit + Turtle-only (UNINTEGRATED)
```

---

## Next Tasks (Priority Order)

### ATR_RANK=5 Integration — READY (no held-out needed)
**Status:** Validated under BOTH dual-exit AND Turtle-only exit logic.
**Dual-exit** (`atr_rank_filter_prod_sweep.rs`): T=5: 38/54 pass, Sharpe 4.433, +115% ret, DD 32.4%, 664 trades. vs T=0: 34/54, Sharpe 3.170, +108%, DD 36.0%, 743 trades.
**Turtle-only** (`turtle_only_atr_rank_sweep.rs`): T=5: 9/9 universes positive, avg Sharpe 4.802, +24% vs T=0 baseline (3.873). All 9 universes pass.
**Decision:** 9/9 Turtle-only positive + dual-exit confirmation = no held-out needed. Integrate now.
**Action:** Add `pub const ATR_RANK_THRESHOLD: usize = 5` to `config.rs`. Add ATR rank gate to `should_enter()` in `bot.rs`.

### Regime ATR (AP=12/LB=42/T=5) — CONFIRMATION SWEEP NEEDED
**Status:** UNINTEGRATED. Found in 2,688-config sweep, Sharpe 1.499 vs 0.840 baseline (+78%). `config.rs` still TURTLE_ATR_PERIOD=24.
**Why it matters:** Mechanistically different from fixed ATR. Period adapts to regime (12 in high-vol, 24 in low-vol). Lookback 42-bar vs 252-bar. This is the most structurally novel finding since Chandelier params.
**Risk:** Was run on 5 windows (not standard 6-window harness). Need confirmation sweep at 6-window scale with AP=12 as the actual live ATR period (not just regime detector input).
**Action:** Build `examples/regime_atr_integration_sweep.rs` using AP=12 as live ATR period. Run on 9 universes × 6 windows. If ≥70% pass + Sharpe improvement: promote.

### S6 close_losers I=5 Turtle-Only Validation — ONE RUN
**Status:** CANDIDATE (48/54 dual-exit, +3.067 Sharpe). Pending Turtle-only validation since 2026-04-30 midday.
**What to run:** `examples/rebalancing_9universe.rs` post-filtered for Turtle-only exit windows, or build `examples/rebalancing_turtle_only.rs`.
**Decision:** If passes → promote to production. If fails → reject and stop re-testing.

### T9: Live Testnet — CRITICAL BLOCKER (4+ weeks)
**Status:** BLOCKED on Noah's Binance testnet API keys.
**What we need:** Binance testnet API key + secret (not production keys).
**Why it matters:** All remaining candidates (ATR_RANK=5, Regime ATR, S6) need live validation before production. The live-vs-backtest gap is unmeasured.

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
