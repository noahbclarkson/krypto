# PLAN.md — Krypto Research and Execution Plan

**State: 2026-05-02 04:35 UTC. VL=96 confirmed NULL (T37: 6/6 tie). AP=64 cliff concern + harness AP mismatch requires validation. T40 (RAE) still untested — highest-value remaining mechanism. Live testnet BLOCKED 5+ weeks on API keys.**

---

## Progress
- Turtle+Chandelier equity (daily, honest): **221.5x / Sharpe 1.04** (live_compatible_wf harness, VL=8, ATR_RANK=24)
- Turtle+ATR_RANK=24 equity: **86.1x / Sharpe 1.01** (progress_equity_curves harness)
- Live bot WF (ATR_RANK=24, Turtle-only): **52/63 pass (82.5%), Sharpe 5.590, +132.3% avg return** ✅
- `live_compatible_wf.rs`: ATR_RANK=24 in code ✅ — BUT `snapshots/live_compatible_wf.md` is STALE (documents T=5, not T=24) — **T41 UNRESOLVED**
- VL=96: **CONFIRMED NULL** — T37: 6/6 windows tie, 0 delta → VL=8 retained
- `live_compatible_wf.rs` hardcodes `regime_atr_period=12`; live bot uses `AP=64` — **T38 harness mismatch UNRESOLVED**
- Live testnet: BLOCKED on Noah's API keys (5+ weeks)

---

## Current Truth

- `src/live/bot.rs` live path is **Turtle-only exit**; no Chandelier exit in deployed bot.
- ATR_RANK=24: live bot validated 52/63 pass (82.5%), Sharpe 5.590, +132.3% avg ret
- ATR rank filter (T=24) is the dominant signal — VL becomes irrelevant at T=24 (T37: 0 delta). The strategy is effectively "Trade Turtle breakouts when BTC is in a high-vol regime."
- REGIME_ATR_PERIOD=64 in config.rs — but harness uses AP=12. Results may not match live bot.
- VOL_LOOKBACK=8: production default (VL=96 claim deleted — same-harness artifact, T37 confirmed null)
- **Research loop in documentation spiral: AP=64 suspicious cliff (Sharpe 6.19→3.07 at AP=65), harness mismatch, stale snapshot — these are trust-lab fixes, not edge hunting.**

---

## Production Params (Frozen)

```text
EP                  = 21
TURTLE_ATR_P        = 24
TURTLE_ATR_M        = 2.0
HOLD_MAX            = 12
POSITION_CAP        = 3
FRESHNESS_COOLDOWN  = 0
ATR_ENTRY_MULT      = 0.00
REGIME_ATR_PERIOD   = 64     # Swept under live Turtle-only path 2026-05-02; needs held-out confirm
REGIME_LOOKBACK     = 42
ATR_RANK_THRESHOLD  = 24
VOL_LOOKBACK        = 8      # VL=96 claim deleted (T37: 6/6 tie, artifact pattern)
```

---

## Next Tasks (Priority Order)

### T41: Regenerate live_compatible_wf.md — IMMEDIATE (documentation debt)
**Status:** STALE. `snapshots/live_compatible_wf.md` documents `T=5` but code has `T=24`. The run was correct (T=24 used) but the description is wrong. Also documents `VOL_LOOKBACK: 8` which is now correct — but AP=12 description vs live bot's AP=64 is a genuine mismatch.
**Action:** Run `cargo run --example live_compatible_wf --profile sweep` → regenerate snapshot from actual harness output. The harness currently uses AP=12 hardcoded — this is T38 territory.

### T44: REGIME_ATR_PERIOD=64 Held-Out Validation — HIGH
**Status:** UNVALIDATED. AP=64 promoted from extensive sweep (AP∈[5..=80 step 1] × 9 universes × 7 windows). But "sole peak, cliff at 65 (Sharpe 6.19→3.07)" is suspicious overfitting signal. AP=65 is also a parameter value — the discontinuity is extreme.
**Action:** Run pre-2026 held-out test (2018-2025 windows only) comparing AP=64 vs AP=12. If AP=64 wins on held-out: accept. If AP=64 loses or ties: revert to AP=12 (AP=39 is backup at 55/63 tied).
**Anti-overfit rule:** Marginal wins (<3 windows) on same grid need held-out confirmation.

### T38-SYNC: Sync live_compatible_wf.rs to config.rs (AP=64) — HIGH
**Status:** MISMATCH. Harness hardcodes `regime_atr_period=12`; live bot uses `AP=64`. The 55/63 pass rate and Sharpe 6.188 from the current harness reflect AP=12, not AP=64. We don't know what AP=64 produces without re-running.
**Action:** Update `live_compatible_wf.rs` to read REGIME_ATR_PERIOD from a config module or hardcode AP=64. Re-run harness and regenerate snapshot. This is prerequisite for trustworthy equity number.

### T40: Regime-Adaptive Exit (RAE) — HIGH (genuinely novel)
**Status:** UNBUILT. 3+ sessions overdue. Mechanism differs from prior GRAVEYARD vol-contingent attempt (uniform multiplier → all configs identical). RAE proposes CONDITIONAL switching: high-vol → M×1.1 (looser), low-vol → M×0.9 (tighter), neutral → M=2.30.
**Build:** `examples/regime_adaptive_exit_walkforward.rs` — grid of high_vol_mult × low_vol_mult × 9 universes × 6 windows.
**Reject if:** No improvement over fixed M=2.30.
**This is the highest-value untested mechanism in the exit space.**

### T42: ATR_ENTRY_MULT=0.94 Pre-2026 Held-Out Validation — MEDIUM
**Status:** UNRUN. Candidate (42/54 pass, Sharpe 5.34 vs baseline 40/54/3.15) found on same WF grid. Anti-overfit correctly kept EM=0.00. Clean held-out test on pre-2026 data only.
**Build:** `examples/atr_entry_mult_heldout.rs` — test EM=0.00 vs EM=0.94 on pre-2026 windows (2018-2025 holdout). If EM=0.94 wins held-out: promote. If not: remove the claim from all files.

### T43: Mid-Cap Re-Test on Base5+LargeCaps5 Only — MEDIUM
**Status:** UNBUILT. Prior rejection (60% global pass < 70% threshold) was dragged down by legacy assets (LTC/EOS/BCH). Base5+LargeCaps5 = 12 windows with the 6 most liquid pairs. If 10+/12 pass: mid-cap expansion viable for production universe.
**Build:** `examples/midcap_base5_largecaps_wf.rs` — Base5 + LargeCaps5 × 6 windows.

### T9: Live Testnet — CRITICAL BLOCKER
**Status:** BLOCKED on Noah's Binance testnet API key + secret. 5+ weeks blocked. All metrics remain simulation upper bounds.

---

## Anti-Spin Rules

1. VL=96 claim DELETED from all files — T37 confirmed null (6/6 tie). VL=8 confirmed.
2. Do not cite `snapshots/live_compatible_wf.md` as authoritative until T41 + T38-SYNC complete.
3. Do not promote AP=64 without held-out validation (T44).
4. **Do not close research loop — T40 (RAE) still untested.** T40 is the only genuinely novel mechanism in the exit space.
5. No hyperopts on settled parameters (ATR_EMA, FRESHNESS_COOLDOWN, HOLD_MAX, CHAND_MULT confirmed 2-3× each).
6. If blocked on credentials, say so plainly.
7. Research loop is NOT closed — it's in documentation-spiral mode (3+ sessions without new strategy work).

---

## Anti-Overfit Rules

1. Minimum 3-window (5.5%) improvement on OOS before accepting any param change.
2. No sequential optimization on same data.
3. Never re-run confirmed params at higher resolution on the same harness. VL=96 is a confirmed violation.
4. Held-out validation required for marginal wins (< 3 windows over baseline).
5. Equity curve dominance required (>80% of time bars).
6. **Absolute guardrails over relative improvement** (Donchian: +11% Sharpe but 63% < 69.1% guardrail → REJECTED).
7. Suspicious single-parameter cliffs (AP=65 Sharpe -46%) require held-out validation before promotion.

---

## Graveyard / Rejections

| Strategy | Result | Key Reason |
|---|---|---|
| S6 close_losers I=5 | GRAVEYARD | Incompatible with Turtle-only live exit (0 trades). Dual-exit only. |
| Donchian sleeve | REJECTED | 34/54 pass (63%) < guardrail 69.1%. |
| ATR_ENTRY_MULT=0.94 | REJECTED (candidate pending T42) | Held-out 10/18 vs baseline 11/18. Pending T42 pre-2026 held-out validation. |
| VOL_LOOKBACK=96 | REJECTED (confirmed NULL) | T37: 6/6 windows tie, 0 delta vs VL=8. Same-harness artifact pattern. |
| Mid-caps (global) | REJECTED (pending T43) | 60% pass < 70% threshold. Re-test on Base5+LargeCaps5 only (T43). |
| ATR-norm position sizing | REJECTED | Equal capital optimal; ATR-norm inverts vol ranking. |
| Asymmetric exit | REJECTED | All configs identical to baseline. |
| Position scaling overlays | GRAVEYARD | All failed — Chandelier already manages it. |
| 4h Multi-Timeframe Turtle | GRAVEYARD | 1/20 pass — structural timeframe incompatibility. |
| BollingerReversion | GRAVEYARD | 0/288 OOS — signal actively harmful. |
| Vol-contingent Chandelier (uniform) | GRAVEYARD | All configs identical — uniform multiplier doesn't change behavior |
| ATR entry × volume confirmation | REJECTED | 40 configs, all inferior to no filter |
| A/D static sleeve | REJECTED | Below-random win rate, -6.2% vs Turtle |
| CTREND 25% fixed sleeve | REJECTED | Sharpe destroyed 1.38→0.33 |
| Equity integration | REJECTED | Combined Sharpe 1.05 vs crypto-only 4.00 |