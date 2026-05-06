# PLAN.md — Krypto Research and Execution Plan

**State: 2026-05-06 08:05 UTC — CRITIQUE: documentation loop detected, code audit required**

## Current Truth

- **Exact as-coded live bot (T65/T67):** 2.55x / daily account Sharpe 0.95 / MaxDD 28.8% / 298 trades / 1,795 days.
- **Research harness (T59 diagnostic):** 176.79x / Sharpe 3.29 / MaxDD 99.5% / 156 trades — NOT production, NOT live bot.
- **VOL_LOOKBACK gap (CRITICAL):** `VOL_LOOKBACK=92` is documented as "configured but UNUSED by src/live/bot.rs entry logic." The research harness runs with vol-adaptive ranking; the live bot does not. This is the strongest candidate for the 70x equity gap.
- **T69 semantic alignment:** REJECTED — made live equity WORSE (1.02x vs 2.54x). The gap is structural, not semantic.
- **4 consecutive sessions** ended in documentation commits about T70 as "next priority." No execution. Documentation loop confirmed.

## Critical: Code Audit Before More Parameter Tuning

T70 was defined as an audit — but 4 sessions of "audit" produced 0 changes to `src/live/bot.rs`. The actual fix is in the code, not in more markdown.

**Immediate action:** Confirm whether `src/live/bot.rs` implements any form of volume-based ranking or sizing. If yes: wire it to confirm the research params still hold. If no: implement VL=92 ranking or explicitly document why live bot intentionally doesn't use it.

## Anti-Spin

The last 8 commits = all same-family Turtle parameter work. HEDGE_ATR_PCT (NULL), HEDGE_SIZE_MULT (risk dial), REGIME_LOOKBACK (small change), FRESHNESS_COOLDOWN (cd=0 confirmed), T69 semantic patch (REJECTED). None changed production equity. The 2.55x figure has been stable across all of them.

We are optimizing parameters on a 9-universe validation grid we've used since April — the grid may itself be over-fit to our parameter choices.

**We have no out-of-sample universe that wasn't part of the optimization history.**

## Next Tasks (Priority Order)

### T72: Open `src/live/bot.rs` — Confirm VOL_LOOKBACK Gap — CODE, NOT DOCUMENTATION
**Status:** UNBUILT. 4 consecutive sessions said "T70 is next" without executing.
- Read `src/live/bot.rs` entry logic — specifically the ranking/sizing path.
- Confirm whether `VOL_LOOKBACK=92` ranking is implemented anywhere in the live bot.
- If not implemented: either add it to the live bot path and rerun exact-live harness, OR update HOF to say live bot intentionally uses equal sizing (explaining the equity gap).
- If implemented: trace why the 2.55x still differs from 176.79x by more than just the ranking difference.
- **This is a 30-minute file read. Not a sweep. Not a document. An actual file.**

### T73: Top-Winner Conditions Audit — DATA ANALYSIS, NOT CODE
**Status:** UNBUILT.
- Use `snapshots/live_bot_exact_trades.csv` (298 trades, ranked by log-return).
- For top-10 winners: extract entry date, symbol, ATR percentile at entry, market regime (bull/bear/chop from BTC trend), Chandelier exit triggered at bar N.
- For each: note whether a plausible filter (ATR rank gate, weekend filter, vol regime) would have excluded it.
- Goal: know if our filters accidentally kill the convex tail. This is the guardrail for every future filter decision.
- No code needed — just Python or Excel analysis on the CSV.

### T74: Commit Or Trash The Uncommitted Sweep
**Status:** UNBUILT.
- `examples/turtle_atr_mult_live_extensive.rs` — uncommitted, generating uncommitted CSV outputs.
- Run it or kill it. Don't let stale sweeps sit in the working tree.
- If results are better than current ATR_MULT=2.0, update production and commit. If not, delete the file.

### T70 (RESOLUTION): Semantic Gap Resolution — AFTER T72
**Status:** UNBUILT. Superseded by T72 (the actual code read).
- Once T72 confirms whether VL ranking is in the live bot, the gap either closes or is explained.
- Option A: wire VL=92 into live bot → rerun exact-live harness → new equity becomes production.
- Option B: document that live bot intentionally uses equal sizing → update HOF production headline.
- No more documentation loops. Either change the code or accept the number.

### T53: Mock Exchange Bypass — STILL BLOCKED
**Status:** UNBUILT (5+ weeks overdue). Supersedes T61.
- Original stub `mock_live_bot.rs` (461 lines) and `mock_live_bot_v2.rs` (503 lines) exist.
- Wire `src/live/bot.rs` → mock → verify same signals as T65 harness.
- Blocked by: no cached 1m parquet (daily/4h only), so local HTTP/WS mock needs either 1m downloader or daily-bar scope reduction.
- After T72: decide whether mock is still the priority or whether VL ranking fix makes execution readiness more urgent.

### T61: Binance aggTrades Order-Flow Signal — AFTER T70/T53
**Status:** UNBUILT. GENUINELY NEW INFORMATION DIMENSION.
- Download historical Binance `aggTrades` → taker buy/seller-initiated imbalance → daily confirmation/size features.
- Must pass top-winner skip audit (T73) before any filter promotion.

## Recently Closed

### T69 semantic alignment candidate — REJECTED (2026-05-05)
- 1.02x / Sharpe 0.10 / MaxDD 30.8% / 200 trades — WORSE than exact live bot (2.54x).
- Do not patch `src/live/bot.rs` with this candidate.

### T67 HEDGE_ATR_PCT — NULL RESULT (2026-05-05)
- All 101 values identical (56/63 pass, Sharpe 6.941, 689 trades). Inert parameter.

### T66 HEDGE_SIZE_MULT — PURE RISK DIAL (2026-05-05)
- 0.70 → 0.40. Not alpha.

### T68 DD abandonment stress — DONE (2026-05-06)
- 20% DD: human review trigger only. 30%+ never breached in-sample.
- Hard abandonment leaves 1.27x, misses 3 top-10 winners.

### T67 production metrics regeneration — DONE (2026-05-06)
- HOF/reports now use exact-live T65/T67 only.

## Resolved Concepts (Do Not Revisit)

- HEDGE_ATR_PCT: 101 identical values — dead code
- ATR_ENTRY_MULT: 0.00 definitively optimal
- REGIME_LOOKBACK: 41 confirmed (LB=42 rejected)
- FRESHNESS_COOLDOWN: 0 wins (live bot updated)
- EP=24: reverted (held-out failure)
- Weekend filter: rejected (56/63 vs 58/63 pass)
- ATR_RANK=24/65: failed held-out
- Donchian: 63% pass < 69.1% guardrail
- All non-trend strategies: dead or borderline

## Parameters (Frozen)

```
TURTLE_EP=21, TURTLE_ATR_PERIOD=24, TURTLE_ATR_MULT=2.00, ATR_ENTRY_MULT=0.00,
HOLD_MAX=12, POSITION_CAP=3, FRESHNESS_COOLDOWN=0,
REGIME_ATR_PERIOD=17, REGIME_LOOKBACK=41, ATR_RANK_THRESHOLD=5.0,
VOL_LOOKBACK=92 (CONFIGURED BUT NOT USED BY bot.rs — T72 required),
HEDGE_ATR_PCT=0.45 (INERT), HEDGE_SIZE_MULT=0.40 (risk dial)
fee_pct=0.000400
```