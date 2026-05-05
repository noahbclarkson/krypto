# PLAN.md — Krypto Research and Execution Plan

**State: 2026-05-05 15:51 UTC — EQUITY TRACKING CYCLE**

## Today: Equity Tracking
- Progress harness: Turtle+Chandelier 619.9x / Sharpe 1.19 | Turtle ATR_RANK=24 56.1x / Sharpe 0.91
- Live bot walk-forward: 44/54 pass (83%), Sharpe 6.12, Base5 6/6 (100%)
- Charts regenerated: `charts/progress_equity_curves_daily.png`
- daily_progress.csv updated
- Progress: STABLE — no new edge, data refreshed correctly

**Best Sharpe today:** Turtle-only walk-forward **6.12** (83% pass, Base5 100%)

## What Changed This Session

1. **Critique complete:** Last 5 builds reviewed. T63/T64 are useful trust work; VL=92 is a defensible plateau tweak; the repeated critique/doc-drift loop remains a real process failure.
2. **Source-of-truth drift identified as the biggest blind spot:** `reports/daily_progress.csv`, `HALL_OF_FAME.md`, live-bot comments, live-compatible harness docs, and current config do not all describe the same strategy/metrics.
3. **Exact live bot still not validated:** T63 validates Turtle-only AP17/T5/VL92, but `src/live/bot.rs` also applies a hardcoded USDT hedge overlay (BTC ATR21 > 75th pct → `size *= 0.70`) not modeled by T63/T64.
4. **Hedge sweep artifacts are untrustworthy:** pre-existing untracked `usdt_hedge_3d_*` outputs report impossible pass counts (e.g. 68/63). Do not use them until the aggregation bug is fixed.
5. **Metric reality check:** Sharpe 5.0+ is mostly walk-forward per-window scoring, not account-level daily Sharpe. Honest current Turtle-only research headline is 176.79x / Sharpe 3.29 / MaxDD 99.5% / 156 trades, excluding the live bot hedge overlay.

## Current Truth

- **Live bot code path:** Turtle-only exit + ATR_RANK=5 regime gate + hardcoded USDT hedge size reducer (`size *= 0.70` when BTC ATR21 > 75th percentile of 252-bar history).
- **Validated research path:** Turtle-only exit + ATR_RANK=5, AP=17/LB=42/T=5, VL=92, no live-bot hedge overlay.
- **Validated research equity (T63/Turtle-only):** 176.79x / Sharpe 3.29 / MaxDD 99.5% / 156 trades.
- **⚠️ Exact live bot equity is still unknown** because the hardcoded hedge overlay is not included in the validated Turtle-only equity/attribution harnesses.
- **⚠️ MaxDD 99.5% is near-total capital destruction.** Strategy survived only because unlevered spot. This is not low-risk in any investor-real sense.
- **ATR_RANK=5 filter is mixed, not clearly defensive.** T64 attribution: T=5 improves bear Sharpe slightly (1.80 vs T=0 1.74) and trend-vol Sharpe (1.16 vs 0.70), but cuts equity/trades and worsens attribution MaxDD. T=0 needs held-out/live-bot validation before any change.
- **Top-trade dependency is acceptable but real.** T63: top 5 trades explain 38.2% of log-return; equity without top 5 remains 24.51x. Top 10 explain 60.9%; equity without top 10 falls to 7.58x.
- **Reports are stale.** `reports/daily_progress.csv` and `HALL_OF_FAME.md` do not match AP17/VL92/T63/T64 state. Do not quote them as production truth until regenerated.
- **Live testnet blocked on Noah's Binance testnet API keys (5+ weeks).** T53 mock exchange remains the bypass.

## Production Params (CODE — 2026-05-05)

```text
EP                  = 21
TURTLE_ATR_P        = 24
TURTLE_ATR_M        = 2.0
HOLD_MAX            = 12
POSITION_CAP        = 3
ATR_ENTRY_MULT      = 0.00
REGIME_ATR_P        = 17
REGIME_LOOKBACK     = 42
ATR_RANK_THRESHOLD  = 5.0
VOL_LOOKBACK        = 92
LIVE_HEDGE_TRIGGER  = BTC ATR21 > 75th percentile of last 252 bars
LIVE_HEDGE_SIZE     = size * 0.70
```

## Next Tasks (Priority Order)

### T65: Exact Live-Bot Source-of-Truth Harness — IMMEDIATE (1 session)
**Status:** UNBUILT.
- **Problem:** T63/T64 validate Turtle-only AP17/VL92, but `src/live/bot.rs` includes a hardcoded USDT hedge overlay not modeled by the validation harnesses. Reports/HOF also cite stale metrics.
- **Action:** Build or refactor one harness that imports/duplicates the exact `src/live/bot.rs` decision path: Turtle entry, ATR_RANK AP17/T5, VL92, Turtle ATR exit, position cap, fees, and live hedge sizing. Output one canonical markdown/CSV.
- **Why:** Until this exists, every production metric is provisional and source-of-truth drift will continue.
- **Output:** `snapshots/live_bot_exact_equity.md` with equity, daily Sharpe, MaxDD, trades, yearly breakdown, top-trade attribution, and explicit param table.

### T66: Metrics Source-of-Truth Regeneration — IMMEDIATE (1 session)
**Status:** UNBUILT.
- **Problem:** `HALL_OF_FAME.md` and `reports/daily_progress.csv` are stale/inconsistent (AP=12/old equity) and conflict with T63/T64 AP17/VL92 results.
- **Action:** After T65, regenerate/update HOF and daily progress from the canonical live-bot exact snapshot only. Fix stale AP/T comments in live docs. Add a rule: reports must state whether metrics are daily compounded, per-window walk-forward, attribution, or milestone-aggregated.
- **Why:** We are currently flattering ourselves by mixing incompatible Sharpe definitions. This is operationally dangerous.
- **Output:** Clean HOF/report entries that quote exactly one production headline and label all non-comparable metrics.

### T62: Weekend Effect Filter — SIMPLE EDGE TEST (1 session)
**Status:** UNBUILT.
- **Problem:** Crypto weekend volume is structurally thinner; weekend breakouts may be lower quality and higher slippage.
- **Action:** On the T65 exact harness, compare baseline vs skip/reduce-size Saturday/Sunday entries. Use pass-rate-first ranking and inspect whether top-10 winning trades are skipped.
- **Why:** Immediately testable with existing data and directly relevant to execution. But it must not be promoted if it misses rare breakout winners.
- **Output:** Baseline vs weekend-filter table, top-trade skip audit, accept/reject verdict.

### T61: Binance aggTrades Order Flow Signal (NEXT AFTER TRUST GAP)
**Status:** UNBUILT.
- Download historical aggTrades from `data.binance.vision` and aggregate buy/sell imbalance into daily confirmation features.
- Only start after T65/T66, otherwise we will add another signal into a confused metric stack.

### T53: Mock Exchange Bypass — EXECUTION BLOCKER
**Status:** UNBUILT. 5+ weeks overdue.
- Build local HTTP/WS mock exchange seeded from historical 1m parquet and test `src/live/bot.rs` end-to-end without Binance credentials.
- This becomes top priority after T65/T66 if Noah's testnet keys remain blocked.

## COMPLETED (Recent)

### T63: Per-Trade PnL Attribution — DONE ✔ (2026-05-05 12:20 UTC)
**Results:** 176.79x / Sharpe 3.29 / MaxDD 99.5% / 156 trades (Turtle-only AP17/T5/VL92, no live-bot hedge overlay)
- Fee drag: 32.6% additive, 4.7% of gross.
- Win rate: 53.8%, avg win +13.76%, avg loss -6.81%, W/L 2.02x.
- Top 5 trades explain 38.2% of log-return; equity without top 5 remains 24.51x.
- Top 10 trades explain 60.9%; equity without top 10 is 7.58x.
- Verdict: real convex trend-following edge, not single-trade mirage. But missing rare breakout winners can destroy performance.
- Files: `examples/t63_trade_attribution.rs`, `snapshots/t63_trade_attribution.{md,csv}`.

### T59: Turtle-Only Daily Equity Curve — DONE ✔ (2026-05-05, updated after VL92)
**Results:** 176.79x / Sharpe 3.29 / MaxDD 99.5% / 156 trades / 1794 days (AP17/T5/VL92)
- Earlier T59 112.27x / Sharpe 3.14 used VL96 before the dense AP17 sweep promoted VL92.
- Live path research equity confirmed for Turtle-only/AP17/T5/VL92, but exact `src/live/bot.rs` remains unconfirmed because of hardcoded hedge overlay.

### T60: Per-Year Performance Decomposition — DONE ✔ (2026-05-05)
**Status:** COMPLETE via Turtle-only equity output.
- 2022 mega-trend remains the dominant contributor to compounded equity.
- Use exact-live T65 harness before quoting final production yearly metrics.

### T64: Regime Sharpe Decomposition — DONE ✔ (2026-05-05 09:20 UTC)
**Results:** Production T=5 attribution = 114.19x / Sharpe 1.68 / MaxDD 51.2% / 154 trades. T=0 no-gate control = 206.95x / Sharpe 1.75 / MaxDD 45.9% / 189 trades.
- Bull/bear Sharpe balanced: 1.80 / 1.80. Not purely bear-only.
- Weak buckets are vol regimes: chop Q1 1.07, trend-vol Q4 1.16.
- ATR_RANK=5 is mixed: slight bear Sharpe lift and trend-vol lift, but lower equity/trades and worse attribution DD. T=0 needs held-out/live-path validation before any production change.
- Files: `examples/regime_sharpe_decomposition.rs`, `snapshots/regime_sharpe_decomposition.{csv,md}`.

### T62 (AP=17 held-out validation): DONE ✔ (2026-05-04)
- AP=17: 4/4 pass, Sharpe 7.715, equity 1.9481x, DD 21.3% ← WINNER
- AP=63: 4/4 pass, Sharpe 5.721, equity 1.3976x, DD 26.6% ← rejected
- AP=12: 2/4 pass ← rejected
- Config.rs updated to REGIME_ATR_PERIOD=17. AP question CLOSED.

## COMPLETED (Historical)

| Task | Status | Key Finding |
|------|--------|-------------|
| ATR_RANK=24 | GRAVEYARD | Held-out: 10/22 pass/-0.964 Sharpe vs T=5 14/22/+0.664 |
| ATR_ENTRY_MULT all | REJECTED | EM=0.00 wins definitively |
| EP=24 | REVERTED | Held-out: 25/29 vs EP=21 27/29 |
| Short-side sleeve | GRAVEYARD | 37.5% pass vs 69.1% guardrail |
| SIZE_MULT overlay | INERT | Pure risk knob, no alpha |
| Funding Rate Regime Filter | GRAVEYARD | 4,590 runs, pass NEVER improves |
| Donchian sleeve | REJECTED | 63% < 69.1% guardrail |
| 4h Multi-Timeframe Turtle | GRAVEYARD | 1/20 pass — structural timeframe incompatibility |
| BollingerReversion | GRAVEYARD | 0/288 OOS — signal actively harmful |
| ATR-norm position sizing | REJECTED | Equal capital optimal; ATR-norm inverts vol ranking |
| CTREND 25% sleeve | REJECTED | Sharpe destroyed 1.38→0.33 |
| A/D static sleeve | REJECTED | Below-random win rate |

## Remaining Blocker

Live testnet (Noah's API keys, 5+ weeks). T53 mock exchange bypasses this.