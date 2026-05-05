# PLAN.md — Krypto Research and Execution Plan

**State: 2026-05-05 09:20 UTC**

## What Changed This Session

1. **T64 COMPLETE (2026-05-05 09:20 UTC):** Regime Sharpe decomposition built and run. Production T=5 attribution: 114.19x / Sharpe 1.68 / MaxDD 51.2% / 154 trades (calendar-day attribution; T59 exact headline remains 112.27x / Sharpe 3.14).
2. **Direction regime finding:** Bull_21d Sharpe 1.80 and Bear_21d Sharpe 1.80 — the leader is not purely bear-only.
3. **Volatility regime finding:** Chop Q1 Sharpe 1.07 and trend-vol Q4 Sharpe 1.16 are the weakest buckets.
4. **ATR_RANK=5 control:** T=0 no-gate attribution = 206.95x / Sharpe 1.75 / MaxDD 45.9% / 189 trades. T=5 improves bear/trend-vol Sharpe slightly/materially but cuts equity/trades and worsens attribution DD. Do not promote T=0 without held-out validation.
5. **Anti-spin check:** Last 3-5 sessions over-spent on hyperopts/critique/doc drift. Next work should close T63/T62 or execution readiness, not rerun settled threshold fights.

## Current Truth

- **Live bot:** Turtle-only exit + ATR_RANK=5 regime gate
- **Live path equity:** 112.27x / Sharpe 3.14 / MaxDD 99.3% / 154 trades / 1794 days
- **⚠️ MaxDD 99.3% is near-total capital destruction.** Strategy survived only because unlevered spot. If someone put in $100K and saw $700, they'd quit. Not a low-risk strategy.
- **ATR_RANK=5 filter is mixed, not clearly defensive.** T64 attribution shows T=5 improves bear Sharpe slightly (1.80 vs T=0 1.74) and trend-vol Sharpe (1.16 vs 0.70), but cuts equity/trades and worsens attribution MaxDD. T=5 stays production only because high-threshold ATR_RANK variants failed held-out; T=0 would need held-out validation before any change.
- **2022 dominates equity.** The year 2022 (99.2x cumulative equity) accounts for most of the cumulative return. This means the strategy is heavily dependent on large bear-market trends. Single big trend year risk.
- **Walk-forward Sharpe (5.17) ≠ daily equity Sharpe (3.14).** Different methodologies. Use 3.14 on charts.
- **All Turtle params frozen.** No more hyperopts needed. Only live execution advances the project.
- **Live testnet blocked on Noah's Binance testnet API keys (5+ weeks).** T53 mock exchange would bypass this.

## Production Params (FINAL — 2026-05-05)

```text
EP                  = 21
TURTLE_ATR_P        = 24
TURTLE_ATR_M        = 2.0
HOLD_MAX            = 12
POSITION_CAP        = 3
ATR_ENTRY_MULT      = 0.00
REGIME_ATR_P        = 17     // AP=17 wins held-out vs AP=63 (Sharpe 7.715 vs 5.721)
REGIME_LOOKBACK     = 42
ATR_RANK_THRESHOLD  = 5.0    // T=24/T=65 fail held-out. T=5 is production.
VOL_LOOKBACK        = 96
SIZE_MULT           = 0.70   // INERT — pure risk knob
HEDGE_PCT           = 75     // INERT — pure risk knob
```

## Next Tasks (Priority Order)

### T63: Per-Bar PnL Attribution — IMMEDIATE (1 session)
**Status:** UNBUILT. Third session overdue.
- **Problem:** Unknown if edge is from few large wins (fragile) or many small edges (robust). Unknown how much equity comes from 2022 mega-trend vs distributed across years.
- **Action:** Using T59 Turtle-only equity data, decompose: (a) winning vs losing trade distribution, (b) fee cost as % of gross, (c) max consecutive losing bars, (d) equity % from top-5 trades vs rest.
- **Why:** If top-5 trades = 80% of equity, the strategy is a "bet on rare mega-trends" and is fragile. If equity is distributed across 50+ trades, it's robust. This is the most important remaining trust question after T64.
- **Output:** Trade attribution table + equity breakdown by trade size bucket.

### T62: Weekend Effect Filter — IMMEDIATE (1 session)
**Status:** UNBUILT.
- **Problem:** Crypto weekend volume is 30-50% lower. Breakouts on Sat/Sun bars may be structurally less reliable due to thinner books and higher slippage.
- **Action:** Add day-of-week filter to Turtle entries. Skip entries on Saturday/Sunday bars (or reduce position size). Test on existing T59 daily data immediately.
- **Why:** Immediately testable. No new data required. One session to build + validate. We keep choosing complex over simple — this is simple.
- **Output:** Walk-forward pass rate delta with vs without weekend filter. If no improvement, reject.

### T61: Binance aggTrades Order Flow Signal (MEDIUM — 1-2 Sessions)
**Status:** UNBUILT.
- **Problem:** LOB NOBI (T55) is 6+ weeks away. Need microstructure alpha without the wait.
- **Action:** Download historical aggTrades from `data.binance.vision`. Aggregate buy/sell imbalance over 5-min windows → rolling daily net flow → test as Turtle entry confirmation gate.
- **Why:** Genuinely novel microstructure edge. Order flow imbalance is a proven alpha source in TradFi. Not waiting for LOB data.
- **Risk:** Multi-session. Data download pipeline + signal harness + validation.

### T53: Mock Exchange Bypass — BLOCKED ON NOAH'S KEYS
**Status:** UNBUILT. 5+ weeks overdue.
- Noah's Binance testnet API keys blocking live testnet
- Build Rust HTTP/WS server that mocks binance-rs-async endpoints
- Seed with historical 1m parquet data
- Test `src/live/bot.rs` end-to-end without Binance credentials
- **This is the highest-leverage path forward** for validating real execution assumptions

## COMPLETED (Recent)

### T59: Turtle-Only Daily Equity Curve — DONE ✔ (2026-05-05 00:38 UTC)
**Results:** 112.27x / Sharpe 3.14 / MaxDD 99.3% / 154 trades / 1794 days
- Bug fixed: turtle_signal used >= instead of > (off-by-one)
- Per-year breakdown: 2020=11.2x, 2021=10.4x, 2022=99.2x, 2023=118.8x, 2024=112.3x
- 2022 bear market dominates cumulative equity (99.2x in one year)
- Live path equity CONFIRMED. Config updated to AP=17.

### T60: Per-Year Performance Decomposition — DONE ✔ (2026-05-05)
**Status:** COMPLETE via T59 output. See above.

### T64: Regime Sharpe Decomposition — DONE ✔ (2026-05-05 09:20 UTC)
**Results:** Production T=5 attribution = 114.19x / Sharpe 1.68 / MaxDD 51.2% / 154 trades. T=0 no-gate control = 206.95x / Sharpe 1.75 / MaxDD 45.9% / 189 trades.
- Bull/bear Sharpe balanced: 1.80 / 1.80. Not purely bear-only.
- Weak buckets are vol regimes: chop Q1 1.07, trend-vol Q4 1.16.
- ATR_RANK=5 is mixed: slight bear Sharpe lift and trend-vol lift, but lower equity/trades and worse attribution DD. T=0 needs held-out validation before any production change.
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