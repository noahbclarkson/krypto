# PLAN.md — Krypto Research and Execution Plan

**State: 2026-05-02 00:14 UTC. T38 COMPLETE. VOL_LOOKBACK=8 synced in equity harness. Equity: Turtle+Chandelier 143.4x / Sharpe 1.02; Turtle+ATR_RANK=24 86.1x / Sharpe 1.01. Live testnet BLOCKED on API keys.**


---

## Progress
- Turtle+Chandelier equity (daily, honest): **143.4x / Sharpe 1.02** (VL=8 fix: was 108.1x at VL=2)
- Turtle+ATR_RANK=24 equity: **86.1x / Sharpe 1.01**
- Live bot WF (ATR_RANK=24, Turtle-only): **52/63 pass (82.5%), Sharpe 5.590, +132.3% avg return** ✅
- progress_equity_curves.rs: **VOL_LOOKBACK=8** synced with config.rs ✅ (was VL=2 stale)
- Chart bug (dimension mismatch): **FIXED** ✅
- Live testnet: BLOCKED on Noah's API keys (5+ weeks)

---


## Current Truth

- `src/live/bot.rs` live path is **Turtle-only exit**; no Chandelier exit exists in the deployed bot.
- ATR_RANK=24: live bot validated (52/63 pass, Sharpe 5.590, +132.3% avg ret)
- VOL_LOOKBACK=8 defined in `src/live/config.rs` ✅ and synced in progress_equity_curves.rs ✅
- T38: COMPLETE (equity harness synced, chart fixed, daily pipeline running)
- T37: VL=96 Base5-only confirmation — still pending

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
REGIME_ATR_P        = 12
REGIME_LOOKBACK     = 42
ATR_RANK_THRESHOLD  = 24
VOL_LOOKBACK       = 8
```

---

## Next Tasks (Priority Order)

### T37: Base5-only VL=96 vs VL=8 confirmation — HIGH
**Status:** UNCONFIRMED. VL=96 found on same harness that produced VL=8 (EP=24 artifact pattern). Run `live_compatible_wf.rs` on Base5 only (6 windows) with VL=96 vs VL=8. Must run BEFORE promoting VL=96.

### T38-FINAL: Export equity curve + reconcile live vs research — COMPLETE ✅
**Status:** COMPLETE (2026-05-02). `live_compatible_wf.rs` at ATR_RANK=24: **52/63 pass (82.5%), Sharpe 5.590, +132.3% avg ret**. `progress_equity_curves.rs`: Turtle+Chandelier 143.4x / Sharpe 1.02; Turtle+ATR_RANK=24 86.1x / Sharpe 1.01. Chart pipeline fixed. T38 CLOSED.

### T36: Sync progress_equity_curves.rs harness to config.rs — COMPLETE ✅
**Status:** COMPLETE. `VOL_LOOKBACK` defined in `src/live/config.rs` (default 8). `vol_lookback` field added to `LiveConfig`. ATR_RANK=24 propagated to progress harness. **2026-05-02:** VL=2→8 fix applied in progress_equity_curves.rs (harness was using stale VL=2). Chart dimension bug fixed. Daily pipeline working. T38 COMPLETE.

### T37: Base5-only VL=96 vs VL=8 confirmation — HIGH
**Status:** UNCONFIRMED. VL=96 found on same harness that produced VL=8 (EP=24 pattern). Run `live_compatible_wf.rs` on Base5 only (6 windows) with VL=96 vs VL=8. If VL=96 wins Base5: promote to config. If VL=96 loses: delete VL=96 claim from all files, stop referencing it. Do NOT let VL=96 sit in the repo as an unconfirmed claim.

### T40: Regime-Adaptive Exit (RAE) — MEDIUM (build once)
**Status:** UNBUILT. Genuinely novel mechanism — conditional Chandelier multiplier based on ATR percentile rank (vs prior uniform multiplier failure).
- Mechanism: high-vol → M×1.1 (looser), low-vol → M×0.9 (tighter), neutral → M=2.30
- Build `examples/regime_adaptive_exit_walkforward.rs` — 16-value grid × Base5 × 6 windows = 480 runs
- Reject if no improvement over fixed M=2.30
- This closes the vol-conditional exit space (exhausted via uniform multiplier → try conditional)

### T9: Live Testnet — CRITICAL BLOCKER
**Status:** BLOCKED on Noah's Binance testnet API key + secret. 5+ weeks blocked. All metrics remain simulation upper bounds.

---

## Anti-Spin Rules

1. ~~Do not cite any "live Turtle-only" equity number until T38-FINAL exports it from the live-compatible harness.~~ **T38 COMPLETE.**
2. ~~VOL_LOOKBACK is undefined in config.rs~~ — now defined (VL=8 in config.rs ✅ and in progress_equity_curves.rs ✅).
3. Do not promote VL=96 until T37 Base5-only confirmation.
4. No more hyperopts on settled parameters (ATR_EMA, ATR_ENTRY_MULT, FRESHNESS_COOLDOWN, HOLD_MAX, CHAND_MULT confirmed 2-3× each).
5. If blocked on credentials, say so plainly.
6. T38 partial (live_compatible_wf.rs) is credible but incomplete — no equity export, no Base5 breakdown, VOL mismatch vs progress harness.

---

## Anti-Overfit Rules

1. Minimum 3-window (5.5%) improvement on OOS before accepting any param change.
2. No sequential optimization on same data.
3. Held-out validation required for marginal wins (< 3 windows over baseline).
4. Equity curve dominance required (>80% of time bars).
5. Never re-run confirmed params at higher resolution on the same harness. VL=96 is a violation of this rule.

---

## Graveyard / Rejections

| Strategy | Result | Key Reason |
|---|---|---|
| S6 close_losers I=5 | GRAVEYARD | Incompatible with Turtle-only live exit (0 trades). Dual-exit only. |
| Donchian sleeve | REJECTED | 34/54 pass (63%) < guardrail 69.1%. |
| ATR_ENTRY_MULT=0.94 | REJECTED | Held-out 10/18 vs baseline 11/18. Anti-overfit discipline. |
| VOL_LOOKBACK=96 | UNCONFIRMED | Same-harness artifact risk (EP=24 pattern). Keep VL=8 until Base5 confirms. |
| Mid-caps | REJECTED | 60% pass < 70% threshold. |
| ATR-norm position sizing | REJECTED | Equal capital optimal; ATR-norm inverts vol ranking. |
| Asymmetric exit | REJECTED | All configs identical to baseline. |
| Position scaling overlays | GRAVEYARD | All failed — Chandelier already manages it. |
| 4h Multi-Timeframe Turtle | GRAVEYARD | 1/20 pass — structural timeframe incompatibility. |
| BollingerReversion | GRAVEYARD | 0/288 OOS — signal actively harmful. |