# PLAN.md — Krypto Research and Execution Plan

**State: 2026-05-01 21:25 UTC. ATR_RANK=24 PROMOTED. Live bot validated: 52/63 pass (82.5%), Sharpe 5.590. Config gap closed (VOL_LOOKBACK defined). Live testnet BLOCKED on API keys.**

---

## Progress: ATR_RANK=24 PROMOTED
- Turtle equity (daily, honest): 108.1x / Sharpe 0.98
- Live bot WF (ATR_RANK=24): **52/63 pass (82.5%), Sharpe 5.590, +132.3% avg return** ✅
- ATR_RANK=5 (prior): 45/63 pass (71.4%), Sharpe 3.315 → REGRESSED to candidate status
- Live testnet: BLOCKED on Noah's API keys (5+ weeks)

---

## Current Truth

- `src/live/bot.rs` live path is **Turtle-only exit**; no Chandelier exit exists in the deployed bot.
- ATR_RANK=24: live bot validated (52/63 pass, Sharpe 5.590, +132.3% avg ret) — promotes from candidate to PRODUCTION DEFAULT.
- VOL_LOOKBACK: now defined in `src/live/config.rs` as 8 (conservative). T37 (Base5-only VL=96 confirmation) still pending — do not promote VL=96 until T37 completes.
- `progress_equity_curves.rs` CHAND_P=7 ✅ (was 11 — fixed 2026-04-20).
- ATR_RANK=24 beats ATR_RANK=5 in ALL 9 universes 9-0 on OOS Sharpe in `live_compatible_wf.rs`.

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
REGIME_ATR_P        = 64
REGIME_LOOKBACK     = 42
ATR_RANK_THRESHOLD  = 24
```

---

## Next Tasks (Priority Order)

### T38-FINAL: Live bot walk-forward validation — COMPLETE ✅
**Status:** COMPLETE. `live_compatible_wf.rs` exports full time series equity to CSV per-universe. Base5 aggregate equity: 43.69x (7 windows). live_compatible_wf.md description fixed to reflect actual AP=64, T=24 params (was stale). T38 COMPLETE — no further infrastructure debt.

### T40: Regime-Adaptive Exit (RAE) — MEDIUM (build once)
**Status:** UNBUILT. Genuinely novel mechanism — conditional Chandelier multiplier based on ATR percentile rank (vs prior uniform multiplier failure).
- Mechanism: high-vol → M×1.1 (looser), low-vol → M×0.9 (tighter), neutral → M=2.30
- Build `examples/regime_adaptive_exit_walkforward.rs` — 16-value grid × Base5 × 6 windows = 480 runs
- Reject if no improvement over fixed M=2.30
- This closes the vol-conditional exit space (exhausted via uniform multiplier → try conditional)

### T9: Live Testnet — CRITICAL BLOCKER
**Status:** BLOCKED on Noah's Binance testnet API key + secret. 4+ weeks blocked. All metrics remain simulation upper bounds.

---

## Anti-Spin Rules

1. **Do not cite any "live Turtle-only" equity number until T38-FINAL exports it from the live-compatible harness.**
2. VOL_LOOKBACK is undefined in config.rs — do not claim any VOL value as "production default" until T36 defines it.
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