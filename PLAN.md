# PLAN.md — Krypto Research and Execution Plan

**State: 2026-05-01 16:11 UTC. STAGNATING. Turtle daily equity 108.1x (was 110.9x yesterday, -2.5%). Live Turtle-only WF: 71.4% pass / Sharpe 3.315. Research closed. Only live testnet (blocked on Noah's API keys) advances the project.**

---

## Progress: STAGNATING
- Turtle equity: 110.9x → 108.1x (-2.5%, normal variance)
- DDBudget: 62.5x → 63.1x (+0.6x, marginal)
- Walk-forward: Turtle-only live-compatible 45/63 pass / 3.315 Sharpe (stable)
- Live testnet: BLOCKED on Noah's API keys (4+ weeks)

---

## Current Truth

- `src/live/bot.rs` live path is **Turtle-only exit**; no Chandelier exit exists in the deployed bot.
- Bug fix (commit `79a3442d`): ATR buffer seeded with `TURTLE_ATR_PERIOD`, long stop uses `highest_high - ATR`, HOLD_MAX enforced independently of ATR warmup.
- **ALL prior Turtle-only walk-forward results are now stale.** Do not cite them as production evidence until T38 revalidation.
- `progress_equity_curves.rs` uses `CHAND_PERIOD=11` — production is `CHAND_PERIOD=7`. Equity numbers from this harness may be unreliable.
- VOL_LOOKBACK VL=96 (commit `76c6fa12`) — same-harness artifact risk identical to EP=24. Not confirmed on Base5-only or held-out data.
- ATR_RANK=5: live integrated, but Turtle-only walk-forward validation is stale (pre-bug-fix).

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
ATR_RANK_THRESHOLD  = 5.0
CHAND_PERIOD        = 7      # research/legacy config only; not live exit
CHAND_MULT          = 2.30   # research/legacy config only; not live exit
VOL_LOOKBACK        = 8      # VL=96 unconfirmed; keep VL=8 until Base5 confirms otherwise
```

---

## Next Tasks (Priority Order)

### T38-FINAL: Export equity curve + reconcile live vs research — CRITICAL
**Status:** PARTIAL. `live_compatible_wf.rs` ran 45/63 pass (71.4%) / Sharpe 3.315 / 121% avg ret. BUT:
- No full-history equity CSV exported
- No Base5-only breakdown (the production universe)
- `VOL_LOOKBACK=8` in harness vs `VOL_LOOKBACK=2` in progress_equity_curves.rs — unresolved
- No comparison to prior stale Turtle-only results
**Required:** Export full equity CSV from `live_compatible_wf.rs` matching src/live/bot.rs exactly. Sync VOL_LOOKBACK between live harness and progress harness. Produce the "honest" live Turtle-only equity number. Label `LIVE_COMPATIBLE` vs `RESEARCH_ONLY`.

### T36: Sync progress_equity_curves.rs harness to config.rs — HIGH
**Status:** CRITICAL. Progress harness uses `VOL_LOOKBACK=2`, live harness uses `VOL_LOOKBACK=8`. Also confirmed `CHAND_PERIOD=7` is correct. VOL_LOOKBACK must be defined in `src/live/config.rs` before any harness can claim to match production. Action:
1. Add `VOL_LOOKBACK: usize` to `src/live/config.rs` (default 8 until T37 confirms otherwise)
2. Sync progress harness to match live bot's actual VOL_LOOKBACK value
3. Regenerate `charts/progress_equity_curves_daily.png` with correct, consistent params

### T37: Base5-only VL=96 vs VL=8 confirmation — HIGH
**Status:** UNCONFIRMED. VL=96 found on same harness that produced VL=8 (EP=24 pattern). Run `live_compatible_wf.rs` on Base5 only (6 windows) with VL=96 vs VL=8. If VL=96 wins Base5: promote to config. If VL=96 loses: delete VL=96 claim from all files, stop referencing it. Do NOT let VL=96 sit in the repo as an unconfirmed claim.

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