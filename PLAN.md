# PLAN.md — Krypto Research and Execution Plan

**State: 2026-05-05 04:05 UTC**

## What Changed This Session

1. **CRITIQUE CYCLE (2026-05-05 04:05 UTC).** Brutal honesty review of last 8 commits. Net useful: 2/8 (T59 equity, AP held-out). T68 hedge overlay (0.55 > 0.75) was found but never applied to config.rs — 1-line fix overdue. live_turtle_chandelier.rs runs the WRONG strategy (dual exit, 397 trades) vs actual live bot (Turtle-only, 154 trades). Reporting CSV uses wrong equity numbers.
2. **T59 COMPLETE:** Turtle-only equity = 112.27x / Sharpe 3.14 / MaxDD 99.3% / 154 trades. BUT this was run with AP=12, not current AP=17. AP=17 equity run still needed.
3. **T68 hedge overlay:** HEDGE_PCT=0.55 found > 0.75 (+4pp pass). Never applied to config.rs. IMMEDIATE action needed.
4. **New blind spots:** Live bot equity unknown for AP=17 config. live_turtle_chandelier.rs misleads about live bot performance.

## Current Truth

- **Live bot:** Turtle-only exit + ATR_RANK=5 regime gate
- **⚠️ Equity of live config (AP=17) unknown.** T59 ran AP=12. Need AP=17 equity run.
- **⚠️ live_turtle_chandelier.rs is the WRONG strategy** (dual exit, 397 trades). Not the live bot.
- **⚠️ MaxDD 99.3% is near-total capital destruction.** Survived only because unlevered spot.
- **⚠️ T68 HEDGE_PCT=0.55 never applied to config.rs.** Found correctly, never shipped.
- **T62/T63/T64: 2-3 sessions unbuilt.** We're choosing new hyperopts over closing open loops.

## IMMEDIATE Actions (This Session)

### [FIX] HEDGE_PCT=0.55 → config.rs
- T68 found pct=0.55 beats pct=0.75 (+4pp pass). config.rs still has 0.75.
- One-line change. Apply immediately.

### [FIX] live_turtle_chandelier.rs labelled correctly
- Rename output section to "(DUAL EXIT — NOT LIVE BOT)"
- Add disclaimer: "Actual live bot: Turtle-only + ATR_RANK=5, see T59 equity"

### [BUILD] T63: Per-Bar PnL Attribution
- Use T59 Turtle-only equity data
- Decompose: winners vs losers distribution, fee cost %, equity from top-5 trades, max consecutive losing bars
- Answers: "is edge from 3 mega-trades (fragile) or distributed (robust)?"
- 1 session. Previously 2 sessions unbuilt.

### [BUILD] T65: AP=17 Turtle-Only Equity Run
- Run equity curve for Turtle-only + AP=17 + ATR_RANK=5 (actual live config)
- Compare to T59 AP=12 result to measure filter cost
- 30 minutes. Closes the "equity of what we're running" blind spot.

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
ATR_RANK_THRESHOLD  = 5.0
VOL_LOOKBACK        = 96
SIZE_MULT           = 0.70
HEDGE_PCT           = 55      // PENDING: T68 found 0.55 > 0.75 (+4pp). Apply to config.rs.
```

## Next Tasks (Priority Order)

### T65: AP=17 Turtle-Only Equity Run — IMMEDIATE (30 min)
**Status:** UNBUILT. Critical for knowing actual live path equity.
- T59 ran AP=12. Production is AP=17. Equity unknown.
- Run Turtle-only + AP=17 + ATR_RANK=5 daily equity curve
- Compare to T59 AP=12 result (112.27x / Sharpe 3.14)
- Output: updated live path equity figure for reports

### T63: Per-Bar PnL Attribution — HIGH (1 session)
**Status:** UNBUILT. 3rd session overdue.
- Decompose T59 Turtle-only equity: winners vs losers, fee %, top-5 trades equity share, max consecutive losing bars
- Answers: fragile (few mega-trades) or robust (distributed)?
- Already have T59 data — just need the decomposition harness

### T62: Weekend Effect Filter — HIGH (1 session)
**Status:** UNBUILT. 2nd session overdue.
- Skip Turtle entries on Sat/Sun bars (crypto weekend vol -30-50%)
- Testable immediately on existing daily data
- Simple. We keep choosing complex over simple — this is avoidance

### T64: Regime Sharpe Decomposition — MEDIUM (1 session)
**Status:** UNBUILT. Decompose Sharpe by bull/bear/chop regime using T59 daily equity.
- Bull regime: BTC 21d return > 0
- Bear regime: BTC 21d return < 0
- Tells us exactly which conditions we win/lose in

### T53: Mock Exchange Bypass — BLOCKED ON NOAH'S KEYS
- Noah's Binance testnet API keys blocking live testnet
- Build Rust HTTP/WS server that mocks binance-rs-async endpoints
- Seed with historical 1m parquet data
- Test `src/live/bot.rs` end-to-end without Binance credentials
- **Highest-leverage path forward** for validating real execution assumptions

## COMPLETED (Recent)

### T68: Hedge Overlay Hyperopt — DONE, NOT APPLIED
- pct=0.55 wins vs pct=0.75 (+4pp pass, Sharpe difference noise)
- Result in snapshots/t68_hedge_overlay_*.csv
- **config.rs still needs HEDGE_PCT=75 → 55**

### T59: Turtle-Only Daily Equity Curve — DONE (AP=12 run)
- 112.27x / Sharpe 3.14 / MaxDD 99.3% / 154 trades / 1794 days
- BUT: AP=12 (not current AP=17). Need T65 for AP=17 equity.

### T60: Per-Year Breakdown — DONE via T59
- 2022 dominated cumulative equity
- MaxDD 99.3% in 2023

### AP Held-Out Validation — DONE
- AP=17: 4/4 pass, Sharpe 7.715, equity 1.9481x, DD 21.3% ← WINNER
- AP=63: 4/4 pass, Sharpe 5.721, equity 1.3976x, DD 26.6% ← rejected
- Config.rs updated to REGIME_ATR_PERIOD=17

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
| HEDGE_PCT=0.55 | FOUND | T68: pct=0.55 beats pct=0.75 (+4pp pass) — NOT YET APPLIED |

## Remaining Blocker

Live testnet (Noah's API keys, 5+ weeks). T53 mock exchange bypasses this.
