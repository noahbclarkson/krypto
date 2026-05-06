# PLAN.md — Krypto Research and Execution Plan

**State: 2026-05-06 12:14 UTC — critique update after T72/T73/T75**

## Current Truth

- **Exact as-coded live bot after T75 (rerun 2026-05-06 12:06):** 2.81x / daily account Sharpe 1.01 / MaxDD 28.2% / 298 trades / 1,795 days.
- **Production source of truth:** `examples/live_bot_exact_equity.rs`, `snapshots/live_bot_exact_equity.md`, `reports/daily_progress.csv`, `HALL_OF_FAME.md`.
- **Research harness (T59 diagnostic):** 176.79x / Sharpe 3.29 / MaxDD 99.5% / 156 trades — NOT production and not a sane deployment target.
- **T72 VOL_LOOKBACK gate:** rejected. Exact live + top-3 VL=92 dollar-volume gate fell to 1.01x / Sharpe 0.09 / MaxDD 30.1% / 207 trades.
- **T73 convex-tail audit:** complete. Top-10 exact-live trades = 82.8% of compounded log return; VL=92 top-3 would exclude 8/10; ATR_RANK>=24 would exclude 5/10; low-vol/chop filters would kill the largest two winners.
- **T75 HEDGE_ATR_PERIOD:** promoted from 21 to 38. 96-value sweep winner: 50/60 pass, Sharpe 1.432, avg return +22.12%, DD 11.51%. Exact-live rerun now 2.81x.

## Critical: Stop Optimizing The Same Price-Only Family

Useful trust debt is mostly closed: exact-live metrics regenerated, T69 rejected, T72 rejected, T73 completed, T74 stale sweep closed, T75 hedge period tuned. The remaining high-value work is not another nearby Turtle/ATR/hedge sweep.

The current bot is plausible, not spectacular: 23.4% annualized with 28.2% MaxDD. The Sharpe 5+ numbers are research/per-window diagnostics, not live account performance.

## Biggest Blind Spot

We have no clean out-of-sample universe that has not been touched by the April/May optimization loop, and we still do not have an end-to-end execution harness for the actual live bot. Convex-tail dependence makes filters dangerous: top 10 trades drive 82.8% of log return.

## Next Tasks (Priority Order)

### T53: Daily-Bar Mock Exchange Bypass — DEPLOYABILITY BLOCKER
**Status:** UNBUILT / PARTIAL COST SMOKE ONLY.
- Stop waiting for perfect cached 1m data. Build a reduced-scope daily-bar mock path first.
- Goal: wire `src/live/bot.rs` / `LiveBot::process_bar` through a mock exchange adapter and reconcile generated orders/fills/state against `snapshots/live_bot_exact_trades.csv`.
- Required outputs:
  - compiled example or integration test that drives the real live bot path;
  - fill ledger with fees/slippage;
  - mismatch report vs exact-live signal ledger;
  - clear list of what remains impossible without 1m/WS data.
- Why first: without this, production readiness is still an assumption.

### T61: Binance aggTrades Order-Flow Signal — ONLY NEW INFORMATION DIMENSION
**Status:** UNBUILT.
- Download historical Binance `aggTrades` for a small pilot set first (BTC/ETH/SOL around top-winner windows and known losing windows).
- Aggregate taker buy/sell imbalance, trade-count imbalance, large-trade imbalance, and imbalance persistence into daily features.
- Test as a **sizing/confirmation feature before hard filtering**. T73 proved hard filters can kill the convex tail.
- Promotion guardrails:
  - exact-live-path daily equity only;
  - top-10 winner skip audit required;
  - fees included;
  - at least 30 trades after any gating.

### T76: Untouched OOS Universe / Era Stress Test — CURVE-FIT CHECK
**Status:** UNBUILT.
- Build a validation set that has not been used for parameter selection: legacy Binance pairs, lower-liquidity survivors, and earlier eras where data exists.
- Freeze current production params before running.
- Goal is falsification, not improvement: answer whether the 2.81x/Sharpe 1.01 live bot generalizes beyond Base5/known 9-universe optimization history.
- Output: one markdown report with pass/fail by universe/era and a clear “deploy confidence up/down” verdict.

## Recently Closed

### T75 HEDGE_ATR_PERIOD extensive sweep — CLOSED (2026-05-06)
- 96 values tested. Winner: HEDGE_ATR_PERIOD=38.
- Summary: 50/60 pass, Sharpe 1.432, avg return +22.12%, DD 11.51%.
- Baseline 21: 47/60 pass, Sharpe 1.294, avg return +20.54%, DD 11.87%.
- Exact-live rerun: 2.81x / Sharpe 1.01 / MaxDD 28.2%.

### T73 Top-Winner Conditions Audit — CLOSED (2026-05-06)
- Top 10 log contributors = 82.8% of total compounded log return.
- Regimes: 5 bear / 3 chop / 2 bull. Convex winners are not clean bull-only entries.
- VL=92 top-3 gate would kill 8/10 top winners; high ATR_RANK gates would kill 5–8/10.
- Decision: no new hard entry filters without a top-winner preservation audit.

### T72 VOL_LOOKBACK live-semantics gate — REJECTED (2026-05-06)
- `src/live/bot.rs` has no volume-rank entry path.
- Adding only top-3 VL=92 ranking to exact live semantics worsened replay to 1.01x / Sharpe 0.09.
- Decision: do not wire VL ranking into live bot without a new mechanism.

### T74 TURTLE_ATR_MULT live extensive sweep — CLOSED (2026-05-06)
- M=2.00 reconfirmed. No production config change.

### T69 semantic alignment candidate — REJECTED (2026-05-05)
- 1.02x / Sharpe 0.10 / MaxDD 30.8% / 200 trades; worse than exact live.

## Resolved Concepts (Do Not Revisit Without New Mechanism)

- VOL_LOOKBACK live top-3 gate: rejected by T72.
- TURTLE_ATR_MULT: 2.00 reconfirmed by T74.
- HEDGE_ATR_PERIOD: 38 promoted by T75; do not nearby-sweep again without new evidence.
- HEDGE_ATR_PCT: 101 identical values — threshold is not alpha.
- ATR_ENTRY_MULT: 0.00 definitively optimal.
- REGIME_LOOKBACK: 41 confirmed.
- FRESHNESS_COOLDOWN: 0 wins.
- EP=24: reverted after held-out failure.
- Weekend filter: rejected.
- ATR_RANK=24/65: held-out/top-winner failure.
- Donchian: lower pass rate than production guardrail.
- All non-trend strategies: dead or borderline.

## Frozen Production Parameters

```text
TURTLE_EP=21, TURTLE_ATR_PERIOD=24, TURTLE_ATR_MULT=2.00, ATR_ENTRY_MULT=0.00,
HOLD_MAX=12, POSITION_CAP=3, FRESHNESS_COOLDOWN=0,
REGIME_ATR_PERIOD=17, REGIME_LOOKBACK=41, ATR_RANK_THRESHOLD=5.0,
VOL_LOOKBACK=92 (diagnostic-only; live rank gate rejected by T72),
HEDGE_ATR_PERIOD=38, HEDGE_LOOKBACK=252,
HEDGE_ATR_PCT=0.45 (threshold not alpha), HEDGE_SIZE_MULT=0.40 (risk dial),
fee_pct=0.000400
```
