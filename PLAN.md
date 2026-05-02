# PLAN.md — Krypto Research and Execution Plan

**State: 2026-05-02 08:44 UTC. Critique cycle complete. Research loop is documentation spiral, not discovery spiral. AP=64 has same-harness artifact risk. T37 (VL=96) 2+ sessions overdue. Live testnet BLOCKED on API keys (5+ weeks).**

---

## Critique Findings (2026-05-02 Morning)

**Project in documentation spiral, not discovery spiral.** Last 8 commits: 5 docs/audits, 3 hyperopts (2 confirming already-known params). Sequential optimization on same OOS harness (EP=24 pattern) applies to REGIME_ATR_PERIOD=64 and REGIME_LOOKBACK=42 — both found on live_compatible_wf.rs in sequence with ATR_RANK=24.

**Genuinely strong:** ATR_RANK=24 (52/63 pass, 9/9 positive, 68% Sharpe vs T=5, plateau T=24-27). Well-validated, sitting on robust plateau.

**Genuinely unresolved:**
- VL=96: 2+ sessions overdue for Base5 confirmation
- AP=64: same-harness artifact risk — needs held-out validation before trust
- T38 still partial after 3+ sessions — no clean full-history equity from live bot path
- Live testnet: 5+ weeks blocked on API keys

**Biggest blind spot:** Optimizing into a bull market. No 12-18 month sustained bear window in OOS data. Fee model may be pessimistic by 2-3x (70% maker fills → ~4-5bps real vs 10bps backtest).

---

## Progress: Research Loop CLOSED (Documentation Spiral)

- Turtle equity (daily, honest): 108.1x / Sharpe 0.98
- Live bot WF (ATR_RANK=24): **52/63 pass (82.5%), Sharpe 5.590, +132.3% avg return** ✅
- ATR_RANK=24 sits on plateau T=24-27 — robust to mis-specification ✅
- REGIME_LOOKBACK=42: confirmed optimal via 196-value sweep (LB=42-45 plateau) ✅
- REGIME_ATR_PERIOD=64: UNCONFIRMED — same-harness artifact risk. Needs held-out validation.
- Live testnet: BLOCKED on Noah's API keys (5+ weeks)

---

## Current Truth

- `src/live/bot.rs` live path is **Turtle-only exit**; no Chandelier exit exists in the deployed bot.
- ATR_RANK=24: live bot validated (52/63 pass, Sharpe 5.590, +132.3% avg ret) — promotes from candidate to PRODUCTION DEFAULT.
- VOL_LOOKBACK: now defined in `src/live/config.rs` as 8 (conservative). T37 COMPLETE: VL=96 claim rejected via Base5 re-validation (0 Sharpe delta). VL=8 confirmed as production default.
- `progress_equity_curves.rs` CHAND_P=7 ✅ (was 11 — fixed 2026-04-20).
- Live bot equity (75.1x) from progress_equity_curves.rs — not from live_compatible_wf.rs export.

---

## Production Params (Frozen — VERIFY BEFORE CITING)

```text
EP                  = 21
TURTLE_ATR_P        = 24
TURTLE_ATR_M        = 2.0
HOLD_MAX            = 12
POSITION_CAP        = 3
FRESHNESS_COOLDOWN  = 0
ATR_ENTRY_MULT      = 0.00
REGIME_ATR_P        = 64      ← UNCONFIRMED: same-harness risk, pending held-out vs AP=12
REGIME_LOOKBACK     = 42      ✅ confirmed optimal (196-value sweep)
ATR_RANK_THRESHOLD  = 24      ✅ confirmed (plateau T=24-27)
VOL_LOOKBACK        = 8       ← conservative; VL=96 unconfirmed (T37 pending)
```

---

## Next Tasks (Priority Order)

### T38-FULL: Re-architect Progress Harness to Match Live Bot
**Status:** UNBUILT. Progress harness must match bot.rs exactly. Delete DDBudget inflated numbers.

### T40: Regime-Adaptive Exit (RAE)
**Status:** UNBUILT. Genuinely novel mechanism — conditional Chandelier multiplier based on ATR percentile rank (vs prior uniform multiplier failure).
- Mechanism: high-vol → M×1.1 (looser), low-vol → M×0.9 (tighter), neutral → M=2.30
- Build `examples/regime_adaptive_exit_walkforward.rs` — grid × Base5 × 6 windows
- Reject if no improvement over fixed M=2.30
- Closes vol-conditional exit space definitively

### T9: Mock Exchange (Bypass Testnet Blocker)
**Status:** BLOCKED on Noah's Binance testnet API key + secret. 4+ weeks blocked. All metrics remain simulation upper bounds.

---

## Anti-Spin Rules

1. **Do not cite any "live Turtle-only" equity number until T38-FINAL exports it from the live-compatible harness.**
2. VOL_LOOKBACK is undefined in config.rs — do not claim any VOL value as "production default" until T36 defines it.
3. T37 completed: VL=96 rejected. VL=8 is the fixed default.
4. No more hyperopts on settled parameters (ATR_EMA, ATR_ENTRY_MULT, FRESHNESS_COOLDOWN, HOLD_MAX, CHAND_MULT confirmed 2-3× each).
5. If blocked on credentials, say so plainly.
6. T38 partial (live_compatible_wf.rs) is credible but incomplete — no equity export, no Base5 breakdown, VOL mismatch vs progress harness.

---

## Anti-Overfit Rules

1. Minimum 3-window (5.5%) improvement on OOS before accepting any param change.
2. No sequential optimization on same data (EP=24 lesson — applies to AP=64).
3. Held-out validation required for marginal wins (< 3 windows over baseline).
4. Equity curve dominance required (>80% of time bars).
5. Never re-run confirmed params at higher resolution on the same harness (VL=96 lesson).
6. **Same-harness artifact check:** If 3+ params were optimized on the same harness in sequence, the latest param needs held-out validation before trust.

---

## Graveyard / Rejections

| Strategy | Result | Key Reason |
|---|---|---|
| S6 close_losers I=5 | GRAVEYARD | Incompatible with Turtle-only live exit (0 trades). Dual-exit only. |
| Donchian sleeve | REJECTED | 34/54 pass (63%) < guardrail 69.1%. |
| ATR_ENTRY_MULT=0.94 | REJECTED | Held-out 10/18 vs baseline 11/18. Anti-overfit discipline. |
| VOL_LOOKBACK=96 | UNCONFIRMED | Same-harness artifact risk (EP=24 pattern). Keep VL=8 until T37 confirms. |
| Mid-caps | REJECTED | 60% pass < 70% threshold. |
| ATR-norm position sizing | REJECTED | Equal capital optimal; ATR-norm inverts vol ranking. |
| Asymmetric exit | REJECTED | All configs identical to baseline. |
| Position scaling overlays | GRAVEYARD | All failed — Chandelier already manages it. |
| 4h Multi-Timeframe Turtle | GRAVEYARD | 1/20 pass — structural timeframe incompatibility. |
| BollingerReversion | GRAVEYARD | 0/288 OOS — signal actively harmful. |
| REGIME_ATR_PERIOD=64 | UNCONFIRMED | Same-harness artifact risk — pending held-out vs AP=12 |