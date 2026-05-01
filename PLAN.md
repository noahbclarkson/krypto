# PLAN.md — Krypto Research and Execution Plan

**State: 2026-05-01 06:30 UTC. LIVE BOT EXIT BUG FIXED ✅ — Turtle-only live exit was effectively a timeout/delayed exit because ATR buffer seeded with `CHAND_PERIOD=7` while `TURTLE_ATR_PERIOD=24`/`HOLD_MAX=12`, and the long stop used `lowest_low - ATR` instead of `highest_high - ATR`. Fixed in `src/live/bot.rs` with unit tests. Prior Turtle-only live-path metrics are not authoritative until rerun under corrected semantics. Live testnet remains BLOCKED on Noah's Binance testnet API keys.**

---

## Current Truth

- `src/live/bot.rs` live path is **Turtle-only exit**; no Chandelier exit exists in the deployed bot.
- `examples/progress_equity_curves.rs` tracks dual Chandelier+Turtle research baseline plus a separate ATR_RANK=5 variant; it is not the same as live code.
- ATR_RANK=5 is integrated in live config/code (`REGIME_ATR_PERIOD=12`, `REGIME_LOOKBACK=42`, `ATR_RANK_THRESHOLD=5.0`), but prior Turtle-only validation used old stop semantics and must be rerun.
- S6 `close_losers I=5` is graveyarded as live-incompatible because it depended on Chandelier’s longer hold path.
- DDBudget Sharpe is milestone-aggregated and not comparable to Turtle daily-equity Sharpe.

---

## Production Params (Frozen but Revalidation Needed)

```text
EP                  = 21
TURTLE_ATR_P        = 24     // bug-fixed 2026-05-01; corrected live-path validation needed
TURTLE_ATR_M        = 2.0
HOLD_MAX            = 12
POSITION_CAP        = 3
FRESHNESS_COOLDOWN  = 0
ATR_ENTRY_MULT      = 0.00
REGIME_ATR_P        = 12     // BTC ATR period for ATR-rank entry gate
REGIME_LOOKBACK     = 42
ATR_RANK_THRESHOLD  = 5.0
CHAND_PERIOD        = 7      // research/legacy config only; not live exit
CHAND_MULT          = 2.30   // research/legacy config only; not live exit
VOL_LOOKBACK        = 8      // VL=90 rejected as same-harness artifact
```

---

## Next Tasks (Priority Order)

### T38: Revalidate Corrected Live Turtle-Only Exit — HIGH PRIORITY
**Status:** REQUIRED after 2026-05-01 bug fix. Build/run a corrected live-path walk-forward matching `src/live/bot.rs` exactly:
- Turtle-only long exit: `highest_high - ATR_MULT * ATR`
- ATR buffer seeded with `TURTLE_ATR_PERIOD`
- HOLD_MAX enforced independent of ATR warmup
- ATR_RANK(AP=12, LB=42, T=5) entry gate
- USDT high-vol size overlay
- corrected fees

Label results explicitly as `LIVE_COMPATIBLE`. This is Track A trust work, not new edge hunting.

### T41: Live-Path Parity Gate — MAKE RESEARCH AND BOT MATCH
**Status:** CRITICAL. Every result must be labeled:
- `LIVE_COMPATIBLE` — matches `src/live/bot.rs`
- `RESEARCH_ONLY` — requires Chandelier/maintenance not present live
- `REQUIRES_LIVE_INTEGRATION` — promising, but invalid as production evidence until bot code changes

Either integrate Chandelier dual-exit into live and revalidate, or stop calling dual-exit results deployable.

### T39: Actual Live Turtle ATR Stop Period Sweep — DEFER UNTIL T38
After corrected live-path validation exists, sweep `TURTLE_ATR_PERIOD ∈ {12,15,18,21,24,30}`. Do not use old `turtle_atr_period_sweep.rs` for production decisions; it reproduced the old unreachable-stop behavior and produced degenerate identical results across ATR periods.

### T9: Live Testnet — CRITICAL BLOCKER
**Status:** BLOCKED on Noah's Binance testnet API key + secret. All metrics remain simulation bounds until 30-day testnet paper trading runs.

---

## Anti-Spin Rules

1. No more nearby hyperopt comparisons on already-settled params unless they are part of corrected live-path validation.
2. Do not cite dual-exit Chandelier metrics as deployable unless live bot implements Chandelier.
3. Do not cite prior Turtle-only ATR_RANK validation as authoritative after the 2026-05-01 stop fix.
4. Prefer trust/deployability work over new benchmark fights.
5. If blocked on credentials, say so plainly.

---

## Graveyard / Rejections

| Strategy | Result | Key Reason |
|---|---|---|
| S6 close_losers I=5 | GRAVEYARD | Validated on dual-exit harness; incompatible with Turtle-only live bot. |
| Donchian sleeve | REJECTED | 9-universe 34/54 pass (63%) < guardrail. |
| ATR_ENTRY_MULT=0.94 | REJECTED | Held-out lost vs EM=0.00. |
| VOL_LOOKBACK=90 | REJECTED | Same-harness artifact; not integrated. |
| Mid-caps | REJECTED | 60% pass < 70% threshold. |
| MACD+Regime | GRAVEYARD | 2/7 OOS pass. |
