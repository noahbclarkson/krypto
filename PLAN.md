# PLAN.md — Krypto Research and Execution Plan

**State: 2026-05-02 16:05 UTC. Critique cycle complete. AP=64 REJECTED via held-out validation (same-harness artifact). Sequential optimization pattern CONFIRMED. VL=96 correctly rejected T37. Research loop remains in documentation spiral — 3/5 recent commits are pure docs.**

---

## Critique Findings (2026-05-02 Late Session)

**AP=64 REJECTED (same-harness artifact):**
- AP=64 was 3rd sequential optimization on `live_compatible_wf.rs` (T=24 → LB=42 → AP=64)
- EP=24 failed held-out validation with the same pattern
- Held-out result: AP=12 = 24/30 pass, Sharpe 0.031, Ret 82.0% vs AP=64 = 23/30, Sharpe 0.037, Ret 42.8%
- AP=64 rejected on fewer passes + lower return. Reverted to AP=12 in config.rs
- **Anti-spin rule confirmed:** Sequential optimization on same harness = artifact risk

**Genuinely resolved:**
- T40 Regime-Adaptive Exit: REJECTED — baseline M=2.30 wins all configs (2026-05-02)
- AP=64: REJECTED via held-out validation — same-harness artifact (2026-05-02)
- ATR_RANK=24: genuinely validated (first on harness, 52/63, 9-0 universe sweep)
- REGIME_LOOKBACK=42: confirmed optimal via 196-value dense sweep
- VL=96: REJECTED via Base5 confirmation — VL=8 remains

**Remaining blocker: Live testnet BLOCKED on Noah's Binance testnet API keys (5+ weeks).**

**Biggest blind spots:**
1. Sequential optimization pattern — we found it, we flagged it, AP=64 bypassed it anyway
2. Documentation spiral — 3/5 recent commits are pure docs, not discovery
3. Execution logic untested — 2026-05-01 bug would have been caught by mock exchange
4. No sustained bear market in OOS (only 2022 ~12 months)

---

## Progress: Research Loop CLOSED (Documentation Spiral)

- Turtle equity (daily, honest): 110.1x / Sharpe 0.98
- Live bot WF (ATR_RANK=24): **52/63 pass (82.5%), Sharpe 5.590, +132.3% avg return** ✅
- ATR_RANK=24 sits on plateau T=24-27 — robust to mis-specification ✅
- REGIME_LOOKBACK=42: confirmed optimal via 196-value sweep (LB=42-45 plateau) ✅
- REGIME_ATR_PERIOD=64: REJECTED — same-harness artifact. Held-out confirmed AP=12 superior.
- Live testnet: BLOCKED on Noah's API keys (5+ weeks)

---

## Current Truth

- `src/live/bot.rs` live path is **Turtle-only exit**; no Chandelier exit exists in the deployed bot.
- ATR_RANK=24: live bot validated (52/63 pass, Sharpe 5.590, +132.3% avg ret) — promotes from candidate to PRODUCTION DEFAULT.
- VOL_LOOKBACK: defined in `src/live/config.rs` as 8 (conservative). T37: VL=96 same-harness artifact rejected.
- `progress_equity_curves.rs` CHAND_P=7 ✅ (was 11 — fixed 2026-04-20).
- Live bot equity (75.1x) from progress_equity_curves.rs — not from live_compatible_wf.rs export.

---

## Production Params (Frozen — VERIFIED 2026-05-02)

```text
EP                  = 21
TURTLE_ATR_P        = 24
TURTLE_ATR_M        = 2.0
HOLD_MAX            = 12
POSITION_CAP        = 3
FRESHNESS_COOLDOWN  = 0
ATR_ENTRY_MULT      = 0.00
REGIME_ATR_P        = 12      ← REVERTED from 64 (same-harness artifact, held-out confirmed)
REGIME_LOOKBACK     = 42      ✅ confirmed optimal (196-value sweep)
ATR_RANK_THRESHOLD  = 24      ✅ confirmed (plateau T=24-27)
VOL_LOOKBACK        = 8       ← conservative; VL=96 rejected as artifact (T37)
```

---

## Next Tasks (Priority Order)

### T40: Regime-Adaptive Exit (RAE)
**Status:** UNBUILT. Genuinely novel — never tested.
- Mechanism: conditional Chandelier multiplier based on ATR percentile rank (vs prior uniform multiplier failure)
- Previous uniform vol-contingent Chandelier failed: all configs identical (21-bar vol rank too fast)
- RAE uses regime-based vol: AP=12, LB=42 — slow enough to be a regime classifier
- High-vol → M×1.1 (looser), Low-vol → M×0.9 (tighter), neutral → M=2.30
- Build `examples/regime_adaptive_exit_walkforward.rs` — grid × Base5 × 6 windows
- Reject if no improvement over fixed M=2.30
- If passes: build and validate. If fails: GRAVEYARD with explicit mechanism failure note

### T41: Mock Exchange (Bypass Testnet Blocker)
**Status:** UNBUILT. 5+ weeks blocked on API keys.
- Lightweight Rust HTTP/WS server mocking binance-rs-async endpoints
- Seed with historical 1m klines to simulate fills and slippage
- Would have caught 2026-05-01 Turtle exit bug before testnet
- Unblocks execution logic testing without credentials

### T9: Live Testnet
**Status:** BLOCKED on Noah's Binance testnet API keys (5+ weeks). This is the only remaining path forward.

---

## Anti-Spin Rules

1. **Do not cite any "live Turtle-only" equity number until T38-FINAL exports it from the live-compatible harness.**
2. VOL_LOOKBACK is defined in config.rs as VL=8 (T37: VL=96 same-harness artifact rejected).
3. No more hyperopts on settled parameters (ATR_EMA, ATR_ENTRY_MULT, FRESHNESS_COOLDOWN, HOLD_MAX, CHAND_MULT confirmed 2-3× each).
4. If blocked on credentials, say so plainly.
5. T38 partial (live_compatible_wf.rs) is credible but incomplete — no equity export, no Base5 breakdown, VOL mismatch vs progress harness.
6. **Max 2 sequential optimizations per harness before mandatory held-out validation.** (AP=64 was #3 → REJECTED. ATR_RANK=24 was #1 → valid. LB=42 was #2 → marginal, prior theoretical justification saved it.)

---

## Anti-Overfit Rules

1. Minimum 3-window (5.5%) improvement on OOS before accepting any param change.
2. No sequential optimization on same data (EP=24 lesson — applies to AP=64).
3. Never re-run confirmed params at higher resolution on the same harness (VL=96 lesson).
4. Held-out validation required for marginal wins (< 3 windows over baseline).
5. Equity curve dominance required (>80% of time bars).
6. Never re-run confirmed params at higher resolution on the same harness (VL=96 lesson).
7. **Max 2 sequential optimizations per harness** before mandatory held-out validation.
8. **Same-harness artifact check:** If 3+ params were optimized on the same harness in sequence, the latest param needs held-out validation before trust. (AP=64 was #3 → REJECTED.)

---

## Graveyard / Rejections

| Strategy | Result | Key Reason |
|---|---|---|
| S6 close_losers I=5 | GRAVEYARD | Incompatible with Turtle-only live exit (0 trades). Dual-exit only. |
| Donchian sleeve | REJECTED | 34/54 pass (63%) < guardrail 69.1%. |
| ATR_ENTRY_MULT=0.94 | REJECTED | Held-out 10/18 vs baseline 11/18. Anti-overfit discipline. |
| VOL_LOOKBACK=96 | REJECTED | Same-harness artifact (EP=24 pattern). Keep VL=8 until T37 confirms. |
| Mid-caps | REJECTED | 60% pass < 70% threshold. |
| ATR-norm position sizing | REJECTED | Equal capital optimal; ATR-norm inverts vol ranking. |
| Asymmetric exit | REJECTED | All configs identical to baseline. |
| Position scaling overlays | GRAVEYARD | All failed — Chandelier already manages it. |
| 4h Multi-Timeframe Turtle | GRAVEYARD | 1/20 pass — structural timeframe incompatibility. |
| BollingerReversion | GRAVEYARD | 0/288 OOS — signal actively harmful. |
| REGIME_ATR_PERIOD=64 | REJECTED | Same-harness artifact (sequential optimization #3 on live_compatible_wf.rs). Held-out: AP=12 > AP=64. Reverted to AP=12. |