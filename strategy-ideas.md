# Strategy Ideas — Krypto Research Log

*Last updated: 2026-05-02 04:35 UTC. VL=96 confirmed NULL (T37: 6/6 tie, 0 delta). AP=64 cliff suspicious (needs held-out T44). T40 (RAE) still untested — highest-value remaining mechanism. Live testnet BLOCKED 5+ weeks.*

---

## Critical Alert: REGIME_ATR_PERIOD=64 Has Suspicious Overfitting Cliff

**AP=64 (REGIME_ATR_PERIOD=64) promoted 2026-05-02:** From extensive sweep (AP∈[5..=80 step 1] × 9 universes × 7 windows): 55/63 pass, Sharpe 6.19, DD -7.0pp vs AP=12 baseline.

**The problem:** "sole peak — Sharpe drops sharply to AP=65 (3.07) and AP=63 (5.27)." A -46% Sharpe drop at AP=65 is a classic single-parameter overfitting signature. AP=65 is also a parameter value — the discontinuity is extreme.

**Rule:** Suspicious single-parameter cliffs require held-out validation. T44 is the held-out test (pre-2026 split). If AP=64 fails held-out, revert to AP=12 or AP=39 (tied 55/63 at Sharpe 5.43, stable plateau).

---

## Critical Alert: Harness Mismatch With Live Bot

`live_compatible_wf.rs` hardcodes `regime_atr_period=12` (from old joint sweep). Live bot uses `AP=64`. The 55/63 pass rate and Sharpe 6.188 from the harness reflect AP=12, not AP=64. We don't know what AP=64 actually produces on the live Turtle-only path.

**Do not cite `snapshots/live_compatible_wf.md` as authoritative** until T38-SYNC completes.

---

## Top 3 Most Promising Unbuilt Ideas

### #1: T44 — REGIME_ATR_PERIOD=64 Held-Out Validation (HIGH — immediate)
**Status:** UNVALIDATED. Required before AP=64 is trusted.
**Why:** The cliff at AP=65 (6.19→3.07 = -46%) is suspicious. This could be genuine parameter boundary or artifact.
**What:** Run pre-2026 held-out test (2018-2025 data) comparing AP=64 vs AP=12. If AP=64 wins on held-out: accept. If AP=64 loses or ties: revert to AP=12.
**Mechanism:** AP=64 uses 64-bar ATR for regime detection. That's ~10 weeks. Combined warmup: AP+LB+2 = 108 bars before filter activates.

### #2: T38-SYNC — Sync live_compatible_wf.rs to AP=64 (HIGH — prerequisite for trustworthy equity)
**Status:** MISMATCH. Harness uses AP=12, live bot uses AP=64.
**What:** Update harness to hardcode AP=64 (or read from config). Re-run and regenerate snapshot. This is the prerequisite for a trustworthy equity number and for comparing live vs backtest behavior.

### #3: T40 — Regime-Adaptive Exit (RAE) — Vol-Conditional Chandelier Multiplier (HIGH — genuinely novel)
**Status:** UNBUILT. 3+ sessions overdue. **This is the highest-value untested mechanism in the exit space.**
**Why different from prior GRAVEYARD attempt:** Prior vol-contingent attempt tested UNIFORM multiplier — all configs produced identical results. RAE proposes CONDITIONAL switching: high-vol → M×1.1 (looser), low-vol → M×0.9 (tighter), neutral → M=2.30.
**What to build:** `examples/regime_adaptive_exit_walkforward.rs` — grid of high_vol_mult × low_vol_mult × 9 universes × 6 windows.
**Reject if:** No improvement over fixed M=2.30.

---

## Unbuilt Ideas (Priority Within Category)

### Infrastructure / Trust (Must Fix)
- [x] **T36**: Sync progress_equity_curves.rs to config.rs — COMPLETE
- [x] **T37**: VL=96 vs VL=8 Base5 confirmation — **NULL RESULT: 6/6 tie, 0 delta → VL=8 retained**
- [ ] **T38-SYNC**: Sync live_compatible_wf.rs to AP=64 — harness mismatch (AP=12 vs AP=64)
- [ ] **T41**: Regenerate live_compatible_wf.md — stale report (T=5 label vs T=24 code)
- [ ] **S6 GRAVEYARD**: close_losers incompatible with Turtle-only exit (0 trades) — clean up

### Research (Genuinely Untested)
- [ ] **T40**: Regime-Adaptive Exit (RAE) — vol-conditional Chandelier multiplier
- [ ] **T42**: ATR_ENTRY_MULT=0.94 held-out validation (pre-2026 split)
- [ ] **T43**: Mid-cap re-test on Base5+LargeCaps5 only

### Blocked on Credentials
- [ ] **T9**: Live testnet (BLOCKED 5+ weeks on Noah's Binance testnet API keys)

---

## Tested and Rejected (Do Not Revisit Without New Mechanism)

| Strategy | Result | Key Reason |
|---|---|---|
| VOL_LOOKBACK=96 | **CONFIRMED NULL** | T37: 6/6 windows TIE, 0 delta vs VL=8. ATR rank filter makes VL irrelevant. VL=96 same-harness artifact. |
| S6 close_losers I=5 | GRAVEYARD | Incompatible with Turtle-only live exit (0 trades). Dual-exit only. |
| Donchian sleeve | REJECTED | 63% < 69.1% guardrail |
| ATR_ENTRY_MULT=0.94 | REJECTED (candidate pending T42) | Held-out 10/18 vs baseline 11/18. Pending T42 pre-2026 held-out. |
| ATR-norm position sizing | REJECTED | Equal capital optimal; ATR-norm inverts vol ranking |
| Asymmetric exit | REJECTED | All configs identical to baseline |
| Mid-caps (global) | REJECTED (pending T43) | 60% pass < 70% threshold. Re-test on Base5+LargeCaps5 only. |
| 4h Multi-Timeframe Turtle | GRAVEYARD | 1/20 pass — structural timeframe incompatibility |
| BollingerReversion | GRAVEYARD | 0/288 OOS — signal actively harmful |
| Position scaling overlays | GRAVEYARD | All failed — Chandelier already manages it |
| Vol-contingent Chandelier (uniform) | GRAVEYARD | All configs identical — uniform multiplier doesn't change behavior |
| ATR entry × volume confirmation | REJECTED | 40 configs, all inferior to no filter |
| A/D static sleeve | REJECTED | Below-random win rate, -6.2% vs Turtle |
| CTREND 25% fixed sleeve | REJECTED | Sharpe destroyed 1.38→0.33 |
| Equity integration | REJECTED | Combined Sharpe 1.05 vs crypto-only 4.00 |

---

## Research Loop Status: NOT CLOSED — T40 Still Untested

**The loop is not closed because we haven't found the right idea — it's closed because all testable ideas have been tested and rejected. Only T40 (RAE), T42, T43, and infrastructure fixes (T38-SYNC, T41, T44) remain.**

**Key insight from T37:** The ATR rank filter (T=24) is so dominant that VL becomes irrelevant (0 delta in 6/6 windows). The strategy is effectively "Trade Turtle breakouts when BTC is in a high-vol regime." This makes the vol regime signal the most important component. AP=64 needs held-out validation.

**Remaining paths:**
1. **Live testnet** (BLOCKED 5+ weeks on API keys — only path for real feedback)
2. **T38-SYNC / T41 infrastructure** (fixes credibility of what we have)
3. **T44 AP=64 held-out** (resolves the cliff concern)
4. **T40 RAE** (genuinely novel — only untested exit mechanism)
5. **T42/T43** (cleanup of old candidates)

---

## Anti-Overfitting Rules (Established 2026-04-25, Updated 2026-05-02)

1. Minimum 3-window (5.5%) improvement on OOS before accepting any param change
2. No sequential optimization on same data (EP=24 lesson)
3. Never re-run confirmed params at higher resolution on the same harness (VL=96 lesson — CONFIRMED)
4. Held-out validation required for marginal wins (< 3 windows over baseline)
5. Equity curve dominance required (>80% of time bars)
6. **Absolute guardrails over relative improvement** (Donchian: +11% Sharpe but 63% pass < 69.1% guardrail → REJECTED)
7. Suspicious single-parameter cliffs (AP=65 Sharpe -46%) require held-out validation before promotion

---

## Key Insight: All Metrics Are Upper Bounds

Everything in HALL_OF_FAME.md is a simulation maximum. The real validation path is live testnet paper trading + comparing actual vs predicted metrics.

**Source of truth for production params: `src/live/config.rs`. Last verified: 2026-05-02.**