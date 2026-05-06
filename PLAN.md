# PLAN.md — Krypto Research and Execution Plan

**State: 2026-05-06 09:20 UTC — T72 closed; VOL_LOOKBACK live gate rejected**

## Current Truth

- **Exact as-coded live bot (T65/T67, rerun 2026-05-06):** 2.56x / daily account Sharpe 0.95 / MaxDD 28.8% / 298 trades / 1,795 days.
- **Research harness (T59 diagnostic):** 176.79x / Sharpe 3.29 / MaxDD 99.5% / 156 trades — NOT production, NOT live bot, and not a sane replacement target given near-total drawdown.
- **VOL_LOOKBACK gap resolved (T72):** `src/live/bot.rs` does **not** implement dollar-volume ranking. Isolating a top-3 `VOL_LOOKBACK=92` gate while keeping exact live entry semantics worsened Base5 replay to **1.01x / Sharpe 0.09 / MaxDD 30.1% / 207 trades**. Do **not** wire VL ranking into the live bot without a new mechanism.
- **T69 semantic alignment:** REJECTED — made live equity WORSE (1.02x vs 2.54x). The gap is structural, not a missing one-line parameter.
- **T74 stale ATR_MULT sweep closed:** `TURTLE_ATR_MULT=2.00` remains winner (47/60 pass, Sharpe 1.294). No config change.

## Critical: Stop Treating Research Equity As The Live Target

The 176.79x diagnostic harness and the 2.56x exact live bot are different systems with different risk. The exact live bot is the production source of truth. Missing `VOL_LOOKBACK` was tested directly and is not the fix.

**Immediate action:** move from gap speculation to convexity/execution readiness. The next highest-value work is T73 top-winner conditions audit, then T53/mock readiness if no new blocker appears.

## Anti-Spin

The last 8+ commits were same-family Turtle parameter or source-of-truth work. Useful trust debt is now mostly closed: exact-live metrics regenerated, T69 rejected, T72 VL-only gate rejected, T74 ATR_MULT stale sweep resolved. Do not reopen nearby parameter comparisons unless there is genuinely new data or a deployability blocker.

We are optimizing parameters on a 9-universe validation grid we've used since April — the grid may itself be over-fit to our parameter choices.

**We have no out-of-sample universe that wasn't part of the optimization history.**

## Next Tasks (Priority Order)

### T73: Top-Winner Conditions Audit — DATA ANALYSIS, NOT CODE
**Status:** UNBUILT.
- Use `snapshots/live_bot_exact_trades.csv` (298 trades, ranked by log-return).
- For top-10 winners: extract entry date, symbol, ATR percentile at entry, market regime (bull/bear/chop from BTC trend), exit reason/bar count.
- For each: note whether a plausible filter (ATR rank gate, weekend filter, vol regime, dollar-volume gate) would have excluded it.
- Goal: know if our filters accidentally kill the convex tail. This is the guardrail for every future filter decision.
- No code change needed — Python analysis + markdown/CSV output is enough.

### T53: Mock Exchange Bypass — STILL BLOCKED / READINESS SCOPE
**Status:** UNBUILT (5+ weeks overdue). Supersedes T61 for deployability.
- Original stub `mock_live_bot.rs` (461 lines) and `mock_live_bot_v2.rs` (503 lines) exist.
- Wire `src/live/bot.rs` → mock → verify same signals as T65 harness.
- Blocked by: no cached 1m parquet (daily/4h only), so local HTTP/WS mock needs either 1m downloader or daily-bar scope reduction.
- After T73, decide whether the next useful session is a daily-bar mock scope reduction or a live-credentials blocker statement.

### T61: Binance aggTrades Order-Flow Signal — AFTER T73/T53
**Status:** UNBUILT. GENUINELY NEW INFORMATION DIMENSION.
- Download historical Binance `aggTrades` → taker buy/seller-initiated imbalance → daily confirmation/size features.
- Must pass top-winner skip audit (T73) before any filter promotion.

## Recently Closed

### T72 VOL_LOOKBACK live-semantics gate — REJECTED (2026-05-06)
- Code audit: `src/live/bot.rs` has no volume-rank path; `config.vol_lookback` is not referenced by entry logic.
- Candidate harness: `examples/t72_vol_rank_live_candidate.rs` kept exact live current-inclusive/equality entry semantics and added only top-3 VL=92 dollar-volume gating.
- Result: **1.01x / Sharpe 0.09 / MaxDD 30.1% / 207 trades** vs exact live **2.56x / Sharpe 0.95 / MaxDD 28.8% / 298 trades**.
- Decision: do not implement VL ranking in live bot; document it as diagnostic-only/rejected for live gate.

### T74 TURTLE_ATR_MULT live extensive sweep — CLOSED (2026-05-06)
- 91 values, M=0.50..=5.00 step 0.05 on current live-style Turtle-only path.
- Winner remains **M=2.00**: 47/60 pass (78.3%), Sharpe 1.294, avg return +20.54%, DD 11.87%, 3,118 trades.
- No production config change. Stale uncommitted sweep is now committed as evidence instead of left dangling.

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

- VOL_LOOKBACK live top-3 gate: rejected by T72 (1.01x vs 2.56x exact live)
- TURTLE_ATR_MULT: 2.00 reconfirmed by T74; no more nearby ATR_MULT sweeps
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

```text
TURTLE_EP=21, TURTLE_ATR_PERIOD=24, TURTLE_ATR_MULT=2.00, ATR_ENTRY_MULT=0.00,
HOLD_MAX=12, POSITION_CAP=3, FRESHNESS_COOLDOWN=0,
REGIME_ATR_PERIOD=17, REGIME_LOOKBACK=41, ATR_RANK_THRESHOLD=5.0,
VOL_LOOKBACK=92 (diagnostic-only; live rank gate rejected by T72),
HEDGE_ATR_PCT=0.45 (INERT), HEDGE_SIZE_MULT=0.40 (risk dial)
fee_pct=0.000400
```
