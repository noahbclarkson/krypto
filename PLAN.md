# PLAN.md — Krypto Research and Execution Plan

**State: 2026-05-01 20:13 UTC. CRITIQUE COMPLETE. Research closed. Infrastructure debt mounting. Testnet blocker 4+ weeks. The project is consolidating, not advancing.**

---

## Critical Alert: Infrastructure Debt is the Problem

The research loop is genuinely closed. Every testable idea has been tested. The issue is infrastructure:
1. `progress_equity_curves.rs` has WRONG CHAND_PERIOD (11 vs production 7) — equity numbers unreliable
2. VOL_LOOKBACK mismatch between progress harness (VL=2) and live bot (VL=8) — cannot reconcile equity
3. S6 close_losers wrongly labeled "candidate" — incompatible with Turtle-only exit (0 trades in live path)
4. DDBudget Sharpe methodology label inconsistent across rows
5. All Turtle-only walk-forward metrics are stale (pre-bug-fix semantics)

**Fix the pipes before claiming more results.**

---

## Progress: STAGNATING (Infrastructure Debt Phase)

- Turtle equity: 108.1x (STALE — progress harness wrong CHAND_PERIOD)
- Walk-forward: 45/63 pass / 3.315 Sharpe (Turtle-only, stale — pre-bug-fix data)
- Live testnet: BLOCKED on Noah's API keys (4+ weeks)
- Research loop: CLOSED (genuinely — all mechanisms exhausted)

---

## Current Truth

- `src/live/bot.rs` live path is **Turtle-only exit**; no Chandelier exit in deployed bot
- Bug fix (commit `79a3442d`): ATR buffer seeded with `TURTLE_ATR_PERIOD`, long stop uses `highest_high - ATR`, HOLD_MAX enforced independently of ATR warmup
- **`progress_equity_curves.rs` uses `CHAND_PERIOD=11`** — production is `CHAND_PERIOD=7`. Equity numbers from this harness are unreliable. T36 has been open since 2026-05-01.
- **VOL_LOOKBACK mismatch**: Progress harness uses VL=2, live bot uses VL=8. Equity figures from these two harnesses are not directly comparable.
- **VL=96**: Same-harness artifact pattern (EP=24). Keep VL=8 in production until T37 confirms otherwise.
- **S6 close_losers**: INCOMPATIBLE with live Turtle-only exit (0 trades). Not a candidate — GRAVEYARD.

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
VOL_LOOKBACK        = 8      # production default; VL=96 UNCONFIRMED
```

---

## Next Tasks (Priority Order)

### T38-FINAL: Corrected Live Turtle-Only Walk-Forward — CRITICAL (BLOCKED ON T36)
**Status:** PARTIAL — stale data. All Turtle-only metrics are post-fix but the OOS data hasn't been re-run under corrected semantics.

**Required:**
1. Fix `progress_equity_curves.rs` to use `CHAND_PERIOD=7` and `VOL_LOOKBACK=8` (T36 dependency)
2. Export full-history equity CSV from `live_compatible_wf.rs` with corrected stop
3. Produce the one trustworthy equity number: Turtle-only, daily compounded, production params
4. Label `LIVE_COMPATIBLE` vs `RESEARCH_ONLY` in all output

**No new param discovery here — trust lab only. The bug fix may have improved or degraded pass rate; we need the corrected number.**

### T36: Sync progress_equity_curves.rs to config.rs — HIGH (unblocks T38)
**Status:** CRITICAL. Progress harness uses CHAND_PERIOD=11 and VL=2; production uses CHAND_PERIOD=7 and VL=8. Equity numbers from progress harness are unreliable until synced.

**Action:**
1. Update `progress_equity_curves.rs` to use `config.rs` params for CHAND_PERIOD and VOL_LOOKBACK
2. Re-run `cargo run --example progress_equity_curves --profile sweep`
3. The resulting equity/ Sharpe is the ONE number to cite for "Turtle daily equity"
4. Regenerate `charts/progress_equity_curves_daily.png`

### T37: VL=96 vs VL=8 Base5-Only Confirmation — MEDIUM
**Status:** UNCONFIRMED. Same-harness artifact risk identical to EP=24 pattern. Run `live_compatible_wf.rs` on Base5 only (6 windows) with VL=96 vs VL=8. If VL=96 wins Base5: promote to config. If VL=96 loses: remove from all claims and keep VL=8. Do NOT let VL=96 sit unconfirmed.

**Small, definitive:** one harness run, clear outcome either way.

### T40: Regime-Adaptive Exit (RAE) — Vol-Conditional Chandelier Multiplier — MEDIUM
**Status:** UNBUILT. Genuinely novel mechanism — conditional Chandelier multiplier based on ATR percentile rank. Prior vol-contingent attempt (uniform multiplier change → all configs identical) was GRAVEYARD. RAE proposes conditional: high-vol → M×1.1 (looser), low-vol → M×0.9 (tighter). This is mechanistically different.

**Build once:** `examples/regime_adaptive_exit_walkforward.rs` — grid of high/low multipliers × 9 universes × 6 windows. Reject if no improvement over fixed M=2.30.

### S6 close_losers — GRAVEYARD (correct mislabeling)
**Status:** WRONGLY labeled "candidate" in MEMORY.md. Produces 0 trades on Turtle-only exit. Chandelier dual-exit harness validation is irrelevant to live bot. Move to GRAVEYARD immediately. Do not re-label or re-test without changing the live exit mechanism.

### T9: Live Testnet — CRITICAL BLOCKER
**Status:** BLOCKED on Noah's Binance testnet API key + secret for 4+ weeks. All metrics remain simulation upper bounds. This is the only path that produces real market feedback.

---

## Anti-Spin Rules

1. **Do not cite `progress_equity_curves.rs` equity until T36 syncs CHAND_PERIOD to 7 and VL to 8.**
2. **Do not cite any Turtle-only walk-forward pass rate until T38 is rerun on corrected stop semantics.**
3. **Do not promote VL=96 until T37 Base5-only confirmation.**
4. **S6 close_losers is not a candidate — it is incompatible with the live Turtle-only exit.**
5. No more hyperopts on settled parameters. Research loop is closed.
6. If blocked on credentials, say so plainly.

---

## Anti-Overfit Rules (Established 2026-04-25)

1. Minimum 3-window (5.5%) improvement on OOS before accepting any param change
2. No sequential optimization on same data (EP=24 lesson — VL=96 is same pattern)
3. Never re-run confirmed params at higher resolution on the same harness (VL=96 = same harness as VL=8)
4. Held-out validation required for marginal wins
5. Equity curve dominance required (>80% of time bars)
6. Absolute guardrails over relative improvement (Donchian: +11% Sharpe but 63% < 69.1% guardrail → REJECTED)

---

## Graveyard / Rejections (Updated 2026-05-01)

| Strategy | Result | Key Reason |
|---|---|---|
| S6 close_losers I=5 | GRAVEYARD | Incompatible with Turtle-only live exit (0 trades). Chandelier-only. |
| Donchian sleeve | REJECTED | 63% < 69.1% guardrail. Higher Sharpe, lower pass rate. |
| ATR_ENTRY_MULT=0.94 | REJECTED | Held-out 10/18 vs baseline 11/18. Anti-overfit. |
| VOL_LOOKBACK=96 | UNCONFIRMED | Same-harness artifact (EP=24 pattern). Keep VL=8 until T37. |
| ATR_EMA [1..200] | CONFIRMED NULL | No improvement anywhere in range |
| FRESHNESS_COOLDOWN [0..70] | CONFIRMED NULL | cd=0 optimal |
| HOLD_MAX [1..100] | CONFIRMED NULL | HM=12 optimal |
| CHAND_MULT [1.5..5.0] | CONFIRMED NULL | M=2.30 optimal |
| Mid-caps | REJECTED | 60% < 70% threshold |
| 4h Multi-Timeframe Turtle | GRAVEYARD | 1/20 pass — structural timeframe incompatibility |
| BollingerReversion | GRAVEYARD | 0/288 OOS — signal actively harmful |
| Position scaling overlays | GRAVEYARD | All failed — Chandelier already manages it |
| ATR-norm position sizing | REJECTED | Equal capital optimal; ATR-norm inverts vol ranking |
| Asymmetric exit | REJECTED | All configs identical to baseline |